#import <Foundation/Foundation.h>
#import <ScreenCaptureKit/ScreenCaptureKit.h>
#import <CoreMedia/CoreMedia.h>
#import <CoreVideo/CoreVideo.h>
#import <VideoToolbox/VideoToolbox.h>

#include <atomic>
#include <cstdint>
#include <cstring>
#include <mutex>
#include <vector>

namespace {

constexpr uint32_t kCodecAv1 = 1;
constexpr uint32_t kCodecH265 = 2;
constexpr uint32_t kCodecH264 = 3;
constexpr int64_t kTimeScale100Ns = 10000000;

CMVideoCodecType codec_type_from_u8(uint8_t codec) {
  switch (codec) {
    case kCodecH265: return kCMVideoCodecType_HEVC;
    case kCodecH264: return kCMVideoCodecType_H264;
    case kCodecAv1: return 'av01';
    default: return kCMVideoCodecType_H264;
  }
}

NSString* codec_profile(uint8_t codec) {
  switch (codec) {
    case kCodecH265: return (__bridge NSString*)kVTProfileLevel_HEVC_Main_AutoLevel;
    case kCodecH264: return (__bridge NSString*)kVTProfileLevel_H264_High_AutoLevel;
    default: return nil;
  }
}

bool sample_is_keyframe(CMSampleBufferRef sample_buffer) {
  CFArrayRef attachments = CMSampleBufferGetSampleAttachmentsArray(sample_buffer, false);
  if (!attachments || CFArrayGetCount(attachments) == 0) {
    return true;
  }
  CFDictionaryRef attachment = static_cast<CFDictionaryRef>(CFArrayGetValueAtIndex(attachments, 0));
  if (!attachment) {
    return true;
  }
  return CFDictionaryGetValue(attachment, kCMSampleAttachmentKey_NotSync) == nullptr;
}

bool append_parameter_set(CMFormatDescriptionRef format_description,
                          bool hevc,
                          size_t index,
                          std::vector<uint8_t>& out) {
  const uint8_t* ptr = nullptr;
  size_t len = 0;
  size_t count = 0;
  int nal_header_len = 0;
  OSStatus status = noErr;
  if (hevc) {
    status = CMVideoFormatDescriptionGetHEVCParameterSetAtIndex(
      format_description, index, &ptr, &len, &count, &nal_header_len);
  } else {
    status = CMVideoFormatDescriptionGetH264ParameterSetAtIndex(
      format_description, index, &ptr, &len, &count, &nal_header_len);
  }
  if (status != noErr || !ptr || len == 0) {
    return false;
  }
  static constexpr uint8_t start_code[] = {0, 0, 0, 1};
  out.insert(out.end(), std::begin(start_code), std::end(start_code));
  out.insert(out.end(), ptr, ptr + len);
  return true;
}

bool append_length_prefixed_sample(CMBlockBufferRef block_buffer, std::vector<uint8_t>& out) {
  const size_t total_len = CMBlockBufferGetDataLength(block_buffer);
  if (total_len == 0) {
    return false;
  }

  std::vector<uint8_t> bytes(total_len);
  if (CMBlockBufferCopyDataBytes(block_buffer, 0, total_len, bytes.data()) != noErr) {
    return false;
  }

  size_t offset = 0;
  while (offset < bytes.size()) {
    if (bytes.size() - offset < 4) {
      return false;
    }
    uint32_t len = 0;
    std::memcpy(&len, bytes.data() + offset, sizeof(len));
    len = CFSwapInt32BigToHost(len);
    offset += 4;
    if (len == 0 || bytes.size() - offset < len) {
      return false;
    }
    static constexpr uint8_t start_code[] = {0, 0, 0, 1};
    out.insert(out.end(), std::begin(start_code), std::end(start_code));
    out.insert(out.end(), bytes.data() + offset, bytes.data() + offset + len);
    offset += len;
  }
  return true;
}

bool sample_to_wire(uint8_t codec, CMSampleBufferRef sample_buffer, std::vector<uint8_t>& out, bool* keyframe_out) {
  if (!CMSampleBufferDataIsReady(sample_buffer)) {
    return false;
  }
  CMBlockBufferRef block_buffer = CMSampleBufferGetDataBuffer(sample_buffer);
  if (!block_buffer) {
    return false;
  }

  const bool keyframe = sample_is_keyframe(sample_buffer);
  if (keyframe_out) {
    *keyframe_out = keyframe;
  }

  out.clear();
  if (codec == kCodecAv1) {
    const size_t len = CMBlockBufferGetDataLength(block_buffer);
    if (len == 0) {
      return false;
    }
    out.resize(len);
    return CMBlockBufferCopyDataBytes(block_buffer, 0, len, out.data()) == noErr;
  }

  CMFormatDescriptionRef format_description = CMSampleBufferGetFormatDescription(sample_buffer);
  if (keyframe && format_description) {
    if (codec == kCodecH265) {
      append_parameter_set(format_description, true, 0, out);
      append_parameter_set(format_description, true, 1, out);
      append_parameter_set(format_description, true, 2, out);
    } else if (codec == kCodecH264) {
      append_parameter_set(format_description, false, 0, out);
      append_parameter_set(format_description, false, 1, out);
    }
  }
  return append_length_prefixed_sample(block_buffer, out);
}

struct MacosStreamBridge;

} // namespace

@interface PartiesMacosStreamOutput : NSObject <SCStreamOutput, SCStreamDelegate>
@property(nonatomic, assign) MacosStreamBridge* bridge;
@end

namespace {

struct MacosStreamBridge {
  ~MacosStreamBridge() {
    stop();
    if (encoder) {
      VTCompressionSessionInvalidate(encoder);
      CFRelease(encoder);
      encoder = nullptr;
    }
  }

  void stop() {
    SCStream* local_stream = nil;
    {
      std::lock_guard<std::mutex> lock(mutex);
      local_stream = stream;
      stream = nil;
      output = nil;
    }
    if (local_stream) {
      dispatch_semaphore_t sem = dispatch_semaphore_create(0);
      [local_stream stopCaptureWithCompletionHandler:^(NSError* _Nullable) {
        dispatch_semaphore_signal(sem);
      }];
      dispatch_semaphore_wait(sem, dispatch_time(DISPATCH_TIME_NOW, NSEC_PER_SEC));
    }
  }

  void handle_frame(CMSampleBufferRef sample_buffer) {
    if (!sample_buffer || !CMSampleBufferDataIsReady(sample_buffer) || !encoder_ready.load(std::memory_order_relaxed)) {
      return;
    }

    CFArrayRef attachments = CMSampleBufferGetSampleAttachmentsArray(sample_buffer, false);
    if (attachments && CFArrayGetCount(attachments) > 0) {
      CFDictionaryRef attachment = static_cast<CFDictionaryRef>(CFArrayGetValueAtIndex(attachments, 0));
      NSNumber* status = attachment
        ? (__bridge NSNumber*)CFDictionaryGetValue(attachment, (__bridge const void*)SCStreamFrameInfoStatus)
        : nil;
      if (status && status.integerValue != SCFrameStatusComplete) {
        return;
      }
    }

    CVImageBufferRef image_buffer = CMSampleBufferGetImageBuffer(sample_buffer);
    if (!image_buffer) {
      return;
    }

    const uint64_t frame = frame_number.fetch_add(1, std::memory_order_relaxed);
    CMTime pts = CMTimeMake(static_cast<int64_t>(frame * frame_duration_100ns), kTimeScale100Ns);
    CMTime duration = CMTimeMake(frame_duration_100ns, kTimeScale100Ns);
    NSDictionary* frame_properties = nil;
    if (force_keyframe.exchange(false, std::memory_order_relaxed)) {
      frame_properties = @{ (__bridge NSString*)kVTEncodeFrameOptionKey_ForceKeyFrame: @YES };
    }

    VTEncodeInfoFlags info_flags = 0;
    OSStatus status = VTCompressionSessionEncodeFrame(
      encoder,
      image_buffer,
      pts,
      duration,
      (__bridge CFDictionaryRef)frame_properties,
      nullptr,
      &info_flags);
    if (status != noErr) {
      failed.store(true, std::memory_order_relaxed);
    }
  }

  void handle_encoded(OSStatus status, CMSampleBufferRef sample_buffer) {
    if (status != noErr || !sample_buffer) {
      failed.store(true, std::memory_order_relaxed);
      return;
    }

    std::vector<uint8_t> bytes;
    bool keyframe = false;
    if (!sample_to_wire(codec, sample_buffer, bytes, &keyframe) || bytes.empty()) {
      return;
    }

    std::lock_guard<std::mutex> lock(mutex);
    pending.swap(bytes);
    pending_keyframe = keyframe;
  }

  std::mutex mutex;
  PartiesMacosStreamOutput* output = nil;
  SCStream* stream = nil;
  VTCompressionSessionRef encoder = nullptr;
  std::vector<uint8_t> pending;
  std::vector<uint8_t> readable;
  bool pending_keyframe = false;
  bool readable_keyframe = false;
  std::atomic<bool> encoder_ready{false};
  std::atomic<bool> failed{false};
  std::atomic<bool> force_keyframe{true};
  std::atomic<uint64_t> frame_number{0};
  uint8_t codec = kCodecH264;
  int64_t frame_duration_100ns = 333333;
};

void compression_output_callback(void* output_callback_ref_con,
                                 void*,
                                 OSStatus status,
                                 VTEncodeInfoFlags,
                                 CMSampleBufferRef sample_buffer) {
  auto* bridge = static_cast<MacosStreamBridge*>(output_callback_ref_con);
  if (!bridge) {
    return;
  }
  bridge->handle_encoded(status, sample_buffer);
}

SCShareableContent* copy_shareable_content_sync() {
  __block SCShareableContent* content = nil;
  dispatch_semaphore_t sem = dispatch_semaphore_create(0);
  [SCShareableContent getShareableContentWithCompletionHandler:^(SCShareableContent* _Nullable shareableContent, NSError* _Nullable) {
    content = shareableContent;
    dispatch_semaphore_signal(sem);
  }];
  dispatch_semaphore_wait(sem, dispatch_time(DISPATCH_TIME_NOW, 5 * NSEC_PER_SEC));
  return content;
}

SCContentFilter* create_filter(uint8_t source_kind, uint64_t source_id) {
  SCShareableContent* content = copy_shareable_content_sync();
  if (!content) {
    return nil;
  }

  if (source_kind == 0) {
    for (SCDisplay* display in content.displays) {
      if (display.displayID == source_id) {
        return [[SCContentFilter alloc] initWithDisplay:display excludingWindows:@[]];
      }
    }
  } else {
    for (SCWindow* window in content.windows) {
      if (window.windowID == source_id) {
        return [[SCContentFilter alloc] initWithDesktopIndependentWindow:window];
      }
    }
  }
  return nil;
}

bool create_encoder(MacosStreamBridge* bridge, uint16_t width, uint16_t height, uint8_t codec, uint32_t fps, uint32_t bitrate) {
  NSDictionary* source_attributes = @{
    (__bridge NSString*)kCVPixelBufferPixelFormatTypeKey: @(kCVPixelFormatType_32BGRA),
    (__bridge NSString*)kCVPixelBufferIOSurfacePropertiesKey: @{},
    (__bridge NSString*)kCVPixelBufferMetalCompatibilityKey: @YES,
  };
  VTCompressionSessionRef session = nullptr;
  OSStatus status = VTCompressionSessionCreate(
    kCFAllocatorDefault,
    width,
    height,
    codec_type_from_u8(codec),
    nullptr,
    (__bridge CFDictionaryRef)source_attributes,
    kCFAllocatorDefault,
    compression_output_callback,
    bridge,
    &session);
  if (status != noErr || !session) {
    return false;
  }

  VTSessionSetProperty(session, kVTCompressionPropertyKey_RealTime, kCFBooleanTrue);
  VTSessionSetProperty(session, kVTCompressionPropertyKey_AllowFrameReordering, kCFBooleanFalse);

  float quality = 1.0f;
  CFNumberRef quality_ref = CFNumberCreate(kCFAllocatorDefault, kCFNumberFloat32Type, &quality);
  if (quality_ref) {
    VTSessionSetProperty(session, kVTCompressionPropertyKey_Quality, quality_ref);
    CFRelease(quality_ref);
  }

  int32_t bitrate_i32 = static_cast<int32_t>(bitrate);
  CFNumberRef bitrate_ref = CFNumberCreate(kCFAllocatorDefault, kCFNumberSInt32Type, &bitrate_i32);
  if (bitrate_ref) {
    VTSessionSetProperty(session, kVTCompressionPropertyKey_AverageBitRate, bitrate_ref);

    int32_t one_second = 1;
    int32_t bytes_per_second = static_cast<int32_t>(bitrate / 8);
    CFNumberRef bytes_per_second_ref = CFNumberCreate(kCFAllocatorDefault, kCFNumberSInt32Type, &bytes_per_second);
    CFNumberRef one_second_ref = CFNumberCreate(kCFAllocatorDefault, kCFNumberSInt32Type, &one_second);
    if (bytes_per_second_ref && one_second_ref) {
      const void* values[] = {bytes_per_second_ref, one_second_ref};
      CFArrayRef limits = CFArrayCreate(kCFAllocatorDefault, values, 2, &kCFTypeArrayCallBacks);
      if (limits) {
        VTSessionSetProperty(session, kVTCompressionPropertyKey_DataRateLimits, limits);
        CFRelease(limits);
      }
    }
    if (bytes_per_second_ref) {
      CFRelease(bytes_per_second_ref);
    }
    if (one_second_ref) {
      CFRelease(one_second_ref);
    }

    CFRelease(bitrate_ref);
  }
  int32_t fps_i32 = static_cast<int32_t>(fps);
  CFNumberRef fps_ref = CFNumberCreate(kCFAllocatorDefault, kCFNumberSInt32Type, &fps_i32);
  if (fps_ref) {
    VTSessionSetProperty(session, kVTCompressionPropertyKey_ExpectedFrameRate, fps_ref);
    CFRelease(fps_ref);
  }
  int32_t key_interval = static_cast<int32_t>(fps * 2);
  CFNumberRef key_ref = CFNumberCreate(kCFAllocatorDefault, kCFNumberSInt32Type, &key_interval);
  if (key_ref) {
    VTSessionSetProperty(session, kVTCompressionPropertyKey_MaxKeyFrameInterval, key_ref);
    CFRelease(key_ref);
  }
  NSString* profile = codec_profile(codec);
  if (profile) {
    VTSessionSetProperty(session, kVTCompressionPropertyKey_ProfileLevel, (__bridge CFStringRef)profile);
  }

  status = VTCompressionSessionPrepareToEncodeFrames(session);
  if (status != noErr) {
    VTCompressionSessionInvalidate(session);
    CFRelease(session);
    return false;
  }

  bridge->encoder = session;
  return true;
}

} // namespace

@implementation PartiesMacosStreamOutput
- (void)stream:(SCStream*)stream didOutputSampleBuffer:(CMSampleBufferRef)sampleBuffer ofType:(SCStreamOutputType)type {
  (void)stream;
  if (type != SCStreamOutputTypeScreen || !_bridge) {
    return;
  }
  _bridge->handle_frame(sampleBuffer);
}

- (void)stream:(SCStream*)stream didStopWithError:(NSError*)error {
  (void)stream;
  (void)error;
  if (_bridge) {
    _bridge->failed.store(true, std::memory_order_relaxed);
  }
}
@end

extern "C" {

MacosStreamBridge* parties_macos_stream_create(uint8_t source_kind,
                                               uint64_t source_id,
                                               uint8_t codec,
                                               uint16_t width,
                                               uint16_t height,
                                               uint32_t fps,
                                               uint32_t bitrate) {
  if (@available(macOS 12.3, *)) {
    if (source_id == 0 || width == 0 || height == 0 || fps == 0 || bitrate == 0) {
      return nullptr;
    }

    auto* bridge = new MacosStreamBridge();
    bridge->codec = codec;
    bridge->frame_duration_100ns = static_cast<int64_t>(kTimeScale100Ns / fps);

    if (!create_encoder(bridge, width, height, codec, fps, bitrate)) {
      delete bridge;
      return nullptr;
    }

    SCContentFilter* filter = create_filter(source_kind, source_id);
    if (!filter) {
      delete bridge;
      return nullptr;
    }

    SCStreamConfiguration* config = [[SCStreamConfiguration alloc] init];
    config.width = width;
    config.height = height;
    config.minimumFrameInterval = CMTimeMake(1, fps);
    config.queueDepth = 3;
    config.pixelFormat = kCVPixelFormatType_32BGRA;
    config.showsCursor = YES;
    config.capturesAudio = NO;

    PartiesMacosStreamOutput* output = [[PartiesMacosStreamOutput alloc] init];
    output.bridge = bridge;

    SCStream* stream = [[SCStream alloc] initWithFilter:filter configuration:config delegate:output];
    if (!stream) {
      delete bridge;
      return nullptr;
    }

    NSError* add_error = nil;
    dispatch_queue_t queue = dispatch_queue_create("parties.macos.screen-stream", DISPATCH_QUEUE_SERIAL);
    if (![stream addStreamOutput:output type:SCStreamOutputTypeScreen sampleHandlerQueue:queue error:&add_error]) {
      (void)add_error;
      delete bridge;
      return nullptr;
    }

    __block BOOL started = NO;
    dispatch_semaphore_t sem = dispatch_semaphore_create(0);
    [stream startCaptureWithCompletionHandler:^(NSError* _Nullable error) {
      started = error == nil;
      dispatch_semaphore_signal(sem);
    }];
    dispatch_semaphore_wait(sem, dispatch_time(DISPATCH_TIME_NOW, 5 * NSEC_PER_SEC));
    if (!started) {
      delete bridge;
      return nullptr;
    }

    {
      std::lock_guard<std::mutex> lock(bridge->mutex);
      bridge->output = output;
      bridge->stream = stream;
    }
    bridge->encoder_ready.store(true, std::memory_order_relaxed);
    return bridge;
  }
  return nullptr;
}

void parties_macos_stream_destroy(MacosStreamBridge* bridge) {
  delete bridge;
}

void parties_macos_stream_force_keyframe(MacosStreamBridge* bridge) {
  if (bridge) {
    bridge->force_keyframe.store(true, std::memory_order_relaxed);
  }
}

int parties_macos_stream_poll(MacosStreamBridge* bridge) {
  if (!bridge) {
    return -1;
  }
  if (bridge->failed.load(std::memory_order_relaxed)) {
    return -1;
  }
  std::lock_guard<std::mutex> lock(bridge->mutex);
  if (bridge->pending.empty()) {
    return 0;
  }
  bridge->readable.swap(bridge->pending);
  bridge->pending.clear();
  bridge->readable_keyframe = bridge->pending_keyframe;
  bridge->pending_keyframe = false;
  return 1;
}

const uint8_t* parties_macos_stream_encoded_ptr(MacosStreamBridge* bridge) {
  if (!bridge || bridge->readable.empty()) {
    return nullptr;
  }
  return bridge->readable.data();
}

uintptr_t parties_macos_stream_encoded_len(MacosStreamBridge* bridge) {
  if (!bridge) {
    return 0;
  }
  return bridge->readable.size();
}

int parties_macos_stream_encoded_keyframe(MacosStreamBridge* bridge) {
  if (!bridge) {
    return 0;
  }
  return bridge->readable_keyframe ? 1 : 0;
}

}
