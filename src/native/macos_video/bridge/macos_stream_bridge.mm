#import <Foundation/Foundation.h>
#import <AVFoundation/AVFoundation.h>
#import <CoreAudio/CoreAudioTypes.h>
#import <ScreenCaptureKit/ScreenCaptureKit.h>
#import <CoreGraphics/CoreGraphics.h>
#import <CoreMedia/CoreMedia.h>
#import <CoreVideo/CoreVideo.h>
#import <VideoToolbox/VideoToolbox.h>
#import <objc/message.h>

#include <algorithm>
#include <atomic>
#include <climits>
#include <cmath>
#include <cstdint>
#include <cfloat>
#include <cstdlib>
#include <cstring>
#include <mutex>
#include <string>
#include <vector>

namespace {

constexpr uint32_t kCodecAv1 = 1;
constexpr uint32_t kCodecH265 = 2;
constexpr uint32_t kCodecH264 = 3;
constexpr int64_t kTimeScale100Ns = 10000000;
constexpr uint32_t kStreamAudioSampleRate = 48000;
constexpr uint32_t kStreamAudioChannels = 2;
constexpr uint32_t kMaxKeyFrameIntervalSeconds = 600;
static __strong id g_sparkle_updater_controller = nil;
static bool g_sparkle_startup_background_check_requested = false;

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
    case kCodecH264: return (__bridge NSString*)kVTProfileLevel_H264_Baseline_AutoLevel;
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

bool append_length_prefixed_sample_as_annex_b(CMBlockBufferRef block_buffer, std::vector<uint8_t>& out) {
  const size_t initial_size = out.size();
  size_t total_len = 0;
  size_t contiguous_len = 0;
  char* contiguous_data = nullptr;
  OSStatus pointer_status =
    CMBlockBufferGetDataPointer(block_buffer, 0, &contiguous_len, &total_len, &contiguous_data);
  if (total_len == 0) {
    return false;
  }

  std::vector<uint8_t> bytes;
  const uint8_t* data = nullptr;
  if (pointer_status == noErr && contiguous_data && contiguous_len >= total_len) {
    data = reinterpret_cast<const uint8_t*>(contiguous_data);
  } else {
    bytes.resize(total_len);
    if (CMBlockBufferCopyDataBytes(block_buffer, 0, total_len, bytes.data()) != noErr) {
      return false;
    }
    data = bytes.data();
  }

  // Length prefixes and Annex B start codes are both 4 bytes here, so the
  // converted sample has the same byte length as the VideoToolbox sample.
  out.reserve(initial_size + total_len);
  out.resize(initial_size + total_len);
  size_t offset = 0;
  size_t written = initial_size;
  while (offset < total_len) {
    if (total_len - offset < 4) {
      out.resize(initial_size);
      return false;
    }
    uint32_t len = 0;
    std::memcpy(&len, data + offset, sizeof(len));
    len = CFSwapInt32BigToHost(len);
    offset += 4;
    if (len == 0 || total_len - offset < len) {
      out.resize(initial_size);
      return false;
    }
    static constexpr uint8_t start_code[] = {0, 0, 0, 1};
    std::memcpy(out.data() + written, start_code, sizeof(start_code));
    written += sizeof(start_code);
    std::memcpy(out.data() + written, data + offset, len);
    written += len;
    offset += len;
  }
  out.resize(written);
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

  const size_t sample_len = CMBlockBufferGetDataLength(block_buffer);
  out.reserve(sample_len + 256);
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
  return append_length_prefixed_sample_as_annex_b(block_buffer, out);
}

struct MacosStreamBridge;

struct MacosEncodedBuffer {
  std::vector<uint8_t> bytes;
  bool keyframe = false;
};

struct MacosAudioBuffer {
  std::vector<float> samples;
};

struct CameraDeviceInfo {
  std::string unique_id;
  std::string name;
};

std::vector<CameraDeviceInfo>& camera_devices() {
  static std::vector<CameraDeviceInfo> devices;
  return devices;
}

uint32_t fnv1a_u32(const std::string& value) {
  uint32_t hash = 0x811C9DC5u;
  for (uint8_t byte : value) {
    hash = (hash ^ byte) * 0x01000193u;
  }
  return hash;
}

std::string& last_error() {
  static std::string message;
  return message;
}

void set_last_error(const std::string& message) {
  last_error() = message;
}

std::string ns_error_string(NSError* error) {
  if (!error) {
    return "unknown error";
  }
  NSString* description = error.localizedDescription ?: error.description;
  return std::string(description.UTF8String ?: "unknown error");
}

char* copy_c_string(const std::string& value) {
  char* out = static_cast<char*>(std::malloc(value.size() + 1));
  if (!out) {
    return nullptr;
  }
  std::memcpy(out, value.c_str(), value.size() + 1);
  return out;
}

void append_json_string(std::string& out, const std::string& value) {
  out.push_back('"');
  for (char ch : value) {
    switch (ch) {
      case '\\': out += "\\\\"; break;
      case '"': out += "\\\""; break;
      case '\n': out += "\\n"; break;
      case '\r': out += "\\r"; break;
      case '\t': out += "\\t"; break;
      default:
        if (static_cast<unsigned char>(ch) < 0x20) {
          out += ' ';
        } else {
          out.push_back(ch);
        }
        break;
    }
  }
  out.push_back('"');
}

void append_desktop_source_json(std::string& out,
                                uint64_t id,
                                int64_t x,
                                int64_t y,
                                uint64_t width,
                                uint64_t height,
                                const std::string& name,
                                const std::string& description) {
  if (width == 0 || height == 0 || name.empty()) {
    return;
  }
  if (out.back() != '[') {
    out.push_back(',');
  }
  out += "{\"id\":";
  out += std::to_string(id);
  out += ",\"x\":";
  out += std::to_string(x);
  out += ",\"y\":";
  out += std::to_string(y);
  out += ",\"width\":";
  out += std::to_string(width);
  out += ",\"height\":";
  out += std::to_string(height);
  out += ",\"name\":";
  append_json_string(out, name);
  out += ",\"description\":";
  append_json_string(out, description);
  out.push_back('}');
}

id sparkle_updater_controller() {
  return g_sparkle_updater_controller;
}

void set_sparkle_updater_controller(id controller) {
  g_sparkle_updater_controller = controller;
}

id ensure_sparkle_updater_controller() {
  id controller = sparkle_updater_controller();
  if (controller != nil) {
    return controller;
  }

  NSString* framework_path =
    [[[NSBundle mainBundle] privateFrameworksPath] stringByAppendingPathComponent:@"Sparkle.framework"];
  NSBundle* framework_bundle = [NSBundle bundleWithPath:framework_path];
  if (framework_bundle != nil && !framework_bundle.loaded) {
    NSError* error = nil;
    [framework_bundle loadAndReturnError:&error];
  }

  Class controller_class = NSClassFromString(@"SPUStandardUpdaterController");
  if (controller_class == nil) {
    return nil;
  }

  SEL selector = @selector(initWithStartingUpdater:updaterDelegate:userDriverDelegate:);
  id allocated = ((id (*)(id, SEL))objc_msgSend)((id)controller_class, @selector(alloc));
  controller = ((id (*)(id, SEL, BOOL, id, id))objc_msgSend)(allocated, selector, YES, nil, nil);
  set_sparkle_updater_controller(controller);
  return controller;
}

id sparkle_updater_from_controller(id controller) {
  if (controller == nil || ![controller respondsToSelector:@selector(updater)]) {
    return nil;
  }
  return ((id (*)(id, SEL))objc_msgSend)(controller, @selector(updater));
}

void check_sparkle_updates_in_background_once() {
  if (g_sparkle_startup_background_check_requested) {
    return;
  }

  id updater = sparkle_updater_from_controller(ensure_sparkle_updater_controller());
  if (updater == nil || ![updater respondsToSelector:@selector(checkForUpdatesInBackground)]) {
    return;
  }

  g_sparkle_startup_background_check_requested = true;
  ((void (*)(id, SEL))objc_msgSend)(updater, @selector(checkForUpdatesInBackground));
}

} // namespace

@interface PartiesMacosStreamOutput : NSObject <SCStreamOutput, SCStreamDelegate>
@property(nonatomic, assign) MacosStreamBridge* bridge;
@end

@interface PartiesMacosCameraOutput : NSObject <AVCaptureVideoDataOutputSampleBufferDelegate>
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
    AVCaptureSession* local_camera_session = nil;
    {
      std::lock_guard<std::mutex> lock(mutex);
      local_stream = stream;
      stream = nil;
      output = nil;
      local_camera_session = camera_session;
      camera_session = nil;
      camera_output = nil;
    }
    if (local_camera_session) {
      [local_camera_session stopRunning];
    }
    if (local_stream) {
      dispatch_semaphore_t sem = dispatch_semaphore_create(0);
      [local_stream stopCaptureWithCompletionHandler:^(NSError* _Nullable) {
        dispatch_semaphore_signal(sem);
      }];
      dispatch_semaphore_wait(sem, dispatch_time(DISPATCH_TIME_NOW, NSEC_PER_SEC));
    }
  }

  void handle_screen_frame(CMSampleBufferRef sample_buffer) {
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

    handle_pixel_buffer(CMSampleBufferGetImageBuffer(sample_buffer));
  }

  void handle_audio_frame(CMSampleBufferRef sample_buffer) {
    if (!sample_buffer || !CMSampleBufferDataIsReady(sample_buffer) || !audio_enabled.load(std::memory_order_relaxed)) {
      return;
    }
    CMFormatDescriptionRef format_description = CMSampleBufferGetFormatDescription(sample_buffer);
    const AudioStreamBasicDescription* asbd = format_description
      ? CMAudioFormatDescriptionGetStreamBasicDescription(format_description)
      : nullptr;
    if (!asbd || asbd->mSampleRate <= 0 || asbd->mChannelsPerFrame == 0) {
      return;
    }

    const CMItemCount frames = CMSampleBufferGetNumSamples(sample_buffer);
    if (frames <= 0) {
      return;
    }

    size_t audio_buffer_list_size = 0;
    OSStatus status = CMSampleBufferGetAudioBufferListWithRetainedBlockBuffer(
      sample_buffer,
      &audio_buffer_list_size,
      nullptr,
      0,
      kCFAllocatorDefault,
      kCFAllocatorDefault,
      kCMSampleBufferFlag_AudioBufferList_Assure16ByteAlignment,
      nullptr);
    if (status != noErr || audio_buffer_list_size == 0) {
      return;
    }

    std::vector<uint8_t> audio_buffer_list_storage(audio_buffer_list_size);
    auto* audio_buffer_list = reinterpret_cast<AudioBufferList*>(audio_buffer_list_storage.data());
    CMBlockBufferRef retained_block_buffer = nullptr;
    status = CMSampleBufferGetAudioBufferListWithRetainedBlockBuffer(
      sample_buffer,
      nullptr,
      audio_buffer_list,
      audio_buffer_list_size,
      kCFAllocatorDefault,
      kCFAllocatorDefault,
      kCMSampleBufferFlag_AudioBufferList_Assure16ByteAlignment,
      &retained_block_buffer);
    if (status != noErr) {
      if (retained_block_buffer) {
        CFRelease(retained_block_buffer);
      }
      return;
    }

    std::vector<float> stereo;
    stereo.reserve(static_cast<size_t>(frames) * kStreamAudioChannels);
    const uint32_t channels = asbd->mChannelsPerFrame;
    const bool is_float = (asbd->mFormatFlags & kAudioFormatFlagIsFloat) != 0;
    const bool is_signed_int = (asbd->mFormatFlags & kAudioFormatFlagIsSignedInteger) != 0;
    const bool is_non_interleaved = (asbd->mFormatFlags & kAudioFormatFlagIsNonInterleaved) != 0;
    const uint32_t bits = asbd->mBitsPerChannel;
    const uint32_t bytes_per_sample = bits / 8;
    if (bytes_per_sample == 0 || audio_buffer_list->mNumberBuffers == 0) {
      if (retained_block_buffer) {
        CFRelease(retained_block_buffer);
      }
      return;
    }

    auto read_sample = [&](const uint8_t* sample_ptr, float* out) -> bool {
      if (is_float && bits == 32) {
        std::memcpy(out, sample_ptr, sizeof(*out));
        return true;
      }
      if (is_signed_int && bits == 16) {
        int16_t sample = 0;
        std::memcpy(&sample, sample_ptr, sizeof(sample));
        *out = static_cast<float>(sample) / 32768.0f;
        return true;
      }
      if (is_signed_int && bits == 32) {
        int32_t sample = 0;
        std::memcpy(&sample, sample_ptr, sizeof(sample));
        *out = static_cast<float>(sample) / 2147483648.0f;
        return true;
      }
      return false;
    };

    size_t frame_count = static_cast<size_t>(frames);
    if (is_non_interleaved || audio_buffer_list->mNumberBuffers > 1) {
      for (uint32_t buffer_index = 0; buffer_index < audio_buffer_list->mNumberBuffers; ++buffer_index) {
        const AudioBuffer& buffer = audio_buffer_list->mBuffers[buffer_index];
        frame_count = std::min(frame_count, static_cast<size_t>(buffer.mDataByteSize / bytes_per_sample));
      }
    } else {
      const AudioBuffer& buffer = audio_buffer_list->mBuffers[0];
      frame_count = std::min(
        frame_count,
        static_cast<size_t>(buffer.mDataByteSize / (static_cast<size_t>(channels) * bytes_per_sample)));
    }

    for (size_t frame = 0; frame < frame_count; ++frame) {
      float left = 0.0f;
      float right = 0.0f;
      if (is_non_interleaved || audio_buffer_list->mNumberBuffers > 1) {
        const AudioBuffer& left_buffer = audio_buffer_list->mBuffers[0];
        if (!read_sample(static_cast<const uint8_t*>(left_buffer.mData) + frame * bytes_per_sample, &left)) {
          break;
        }
        if (audio_buffer_list->mNumberBuffers > 1) {
          const AudioBuffer& right_buffer = audio_buffer_list->mBuffers[1];
          if (!read_sample(static_cast<const uint8_t*>(right_buffer.mData) + frame * bytes_per_sample, &right)) {
            break;
          }
        } else {
          right = left;
        }
      } else {
        const AudioBuffer& buffer = audio_buffer_list->mBuffers[0];
        const auto* raw = static_cast<const uint8_t*>(buffer.mData);
        for (uint32_t channel = 0; channel < channels; ++channel) {
          float value = 0.0f;
          if (!read_sample(raw + (frame * channels + channel) * bytes_per_sample, &value)) {
            break;
          }
          if (channel == 0) {
            left = value;
          } else if (channel == 1) {
            right = value;
          }
        }
        if (channels == 1) {
          right = left;
        }
      }
      stereo.push_back(left);
      stereo.push_back(right);
    }

    if (retained_block_buffer) {
      CFRelease(retained_block_buffer);
    }
    if (stereo.empty()) {
      return;
    }
    std::lock_guard<std::mutex> lock(mutex);
    pending_audio.insert(pending_audio.end(), stereo.begin(), stereo.end());
    constexpr size_t max_pending_samples = kStreamAudioSampleRate * kStreamAudioChannels;
    if (pending_audio.size() > max_pending_samples) {
      pending_audio.erase(pending_audio.begin(), pending_audio.end() - max_pending_samples);
    }
  }

  void handle_camera_frame(CMSampleBufferRef sample_buffer) {
    if (!sample_buffer || !CMSampleBufferDataIsReady(sample_buffer) || !encoder_ready.load(std::memory_order_relaxed)) {
      return;
    }
    handle_pixel_buffer(CMSampleBufferGetImageBuffer(sample_buffer));
  }

  void handle_pixel_buffer(CVImageBufferRef image_buffer) {
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
  PartiesMacosCameraOutput* camera_output = nil;
  SCStream* stream = nil;
  AVCaptureSession* camera_session = nil;
  VTCompressionSessionRef encoder = nullptr;
  std::vector<uint8_t> pending;
  std::vector<uint8_t> readable;
  std::vector<float> pending_audio;
  std::vector<float> readable_audio;
  bool pending_keyframe = false;
  bool readable_keyframe = false;
  std::atomic<bool> encoder_ready{false};
  std::atomic<bool> failed{false};
  std::atomic<bool> force_keyframe{true};
  std::atomic<bool> audio_enabled{false};
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
    set_last_error("failed to create VideoToolbox encoder session: OSStatus " + std::to_string(status));
    return false;
  }

  VTSessionSetProperty(session, kVTCompressionPropertyKey_RealTime, kCFBooleanTrue);
  VTSessionSetProperty(session, kVTCompressionPropertyKey_AllowFrameReordering, kCFBooleanFalse);

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
  int32_t key_interval = static_cast<int32_t>((std::max)(fps, 1u) * kMaxKeyFrameIntervalSeconds);
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
    set_last_error("failed to prepare VideoToolbox encoder session: OSStatus " + std::to_string(status));
    VTCompressionSessionInvalidate(session);
    CFRelease(session);
    return false;
  }

  bridge->encoder = session;
  return true;
}

NSArray<AVCaptureDevice*>* native_camera_devices() {
  if (@available(macOS 10.15, *)) {
    AVCaptureDeviceDiscoverySession* session = [AVCaptureDeviceDiscoverySession
      discoverySessionWithDeviceTypes:@[
        AVCaptureDeviceTypeBuiltInWideAngleCamera,
        AVCaptureDeviceTypeExternalUnknown,
      ]
      mediaType:AVMediaTypeVideo
      position:AVCaptureDevicePositionUnspecified];
    return session.devices;
  }
  return [AVCaptureDevice devicesWithMediaType:AVMediaTypeVideo];
}

AVCaptureDevice* find_camera_device(uint64_t source_id) {
  for (AVCaptureDevice* device in native_camera_devices()) {
    std::string unique_id(device.uniqueID.UTF8String ?: "");
    if (fnv1a_u32(unique_id) == source_id) {
      return device;
    }
  }
  return nil;
}

bool ensure_camera_authorized() {
  AVAuthorizationStatus status = [AVCaptureDevice authorizationStatusForMediaType:AVMediaTypeVideo];
  if (status == AVAuthorizationStatusAuthorized) {
    return true;
  }
  if (status == AVAuthorizationStatusDenied || status == AVAuthorizationStatusRestricted) {
    set_last_error("camera permission is denied or restricted");
    return false;
  }
  if (status != AVAuthorizationStatusNotDetermined) {
    set_last_error("camera permission is unavailable");
    return false;
  }

  __block BOOL granted = NO;
  dispatch_semaphore_t sem = dispatch_semaphore_create(0);
  [AVCaptureDevice requestAccessForMediaType:AVMediaTypeVideo completionHandler:^(BOOL access_granted) {
    granted = access_granted;
    dispatch_semaphore_signal(sem);
  }];
  dispatch_semaphore_wait(sem, dispatch_time(DISPATCH_TIME_NOW, 60 * NSEC_PER_SEC));
  if (!granted) {
    set_last_error("camera permission was not granted");
  }
  return granted;
}

bool ensure_microphone_authorized() {
  AVAuthorizationStatus status = [AVCaptureDevice authorizationStatusForMediaType:AVMediaTypeAudio];
  if (status == AVAuthorizationStatusAuthorized) {
    return true;
  }
  if (status == AVAuthorizationStatusDenied || status == AVAuthorizationStatusRestricted) {
    set_last_error("microphone permission is denied or restricted");
    return false;
  }
  if (status != AVAuthorizationStatusNotDetermined) {
    set_last_error("microphone permission is unavailable");
    return false;
  }

  __block BOOL granted = NO;
  dispatch_semaphore_t sem = dispatch_semaphore_create(0);
  [AVCaptureDevice requestAccessForMediaType:AVMediaTypeAudio completionHandler:^(BOOL access_granted) {
    granted = access_granted;
    dispatch_semaphore_signal(sem);
  }];
  dispatch_semaphore_wait(sem, dispatch_time(DISPATCH_TIME_NOW, 60 * NSEC_PER_SEC));
  if (!granted) {
    set_last_error("microphone permission was not granted");
  }
  return granted;
}

bool configure_camera_format(AVCaptureDevice* device, uint16_t width, uint16_t height, uint32_t fps, uint32_t* actual_fps) {
  NSError* error = nil;
  if (![device lockForConfiguration:&error]) {
    set_last_error("failed to lock camera for configuration: " + ns_error_string(error));
    return false;
  }

  AVCaptureDeviceFormat* selected_format = nil;
  double selected_fps = 0.0;
  int64_t selected_score = INT64_MAX;
  for (AVCaptureDeviceFormat* format in device.formats) {
    CMFormatDescriptionRef description = format.formatDescription;
    CMVideoDimensions dimensions = CMVideoFormatDescriptionGetDimensions(description);
    double format_fps = 0.0;
    double fps_delta = DBL_MAX;
    for (AVFrameRateRange* range in format.videoSupportedFrameRateRanges) {
      const double clamped_fps = std::min(std::max(static_cast<double>(fps), range.minFrameRate), range.maxFrameRate);
      const double delta = std::fabs(clamped_fps - static_cast<double>(fps));
      if (delta < fps_delta || (delta == fps_delta && clamped_fps > format_fps)) {
        fps_delta = delta;
        format_fps = clamped_fps;
      }
    }
    if (format_fps <= 0.0 || fps_delta == DBL_MAX) {
      continue;
    }
    int64_t area_delta = std::llabs(
      static_cast<int64_t>(dimensions.width) * static_cast<int64_t>(dimensions.height) -
      static_cast<int64_t>(width) * static_cast<int64_t>(height));
    int64_t aspect_delta = static_cast<int64_t>(
      std::llround(std::fabs(
        (static_cast<double>(dimensions.width) / static_cast<double>(dimensions.height)) -
        (static_cast<double>(width) / static_cast<double>(height))) * 1000000.0));
    int64_t exact_bonus = dimensions.width == width && dimensions.height == height ? -1000000000000LL : 0;
    int64_t fps_penalty = static_cast<int64_t>(std::llround(fps_delta * 1000000.0));
    int64_t score = aspect_delta * 100000000LL + area_delta + fps_penalty * 1000000LL + exact_bonus;
    if (!selected_format || score < selected_score) {
      selected_format = format;
      selected_fps = format_fps;
      selected_score = score;
    }
  }

  if (selected_format) {
    device.activeFormat = selected_format;
  } else {
    [device unlockForConfiguration];
    set_last_error("camera has no usable video format");
    return false;
  }
  uint32_t configured_fps = static_cast<uint32_t>(std::max<int64_t>(1, std::llround(selected_fps)));
  CMTime frame_duration = CMTimeMake(1, static_cast<int32_t>(configured_fps));
  device.activeVideoMinFrameDuration = frame_duration;
  device.activeVideoMaxFrameDuration = frame_duration;
  [device unlockForConfiguration];
  if (actual_fps) {
    *actual_fps = configured_fps;
  }
  return true;
}

} // namespace

@implementation PartiesMacosStreamOutput
- (void)stream:(SCStream*)stream didOutputSampleBuffer:(CMSampleBufferRef)sampleBuffer ofType:(SCStreamOutputType)type {
  (void)stream;
  if (!_bridge) {
    return;
  }
  if (type == SCStreamOutputTypeScreen) {
    _bridge->handle_screen_frame(sampleBuffer);
  } else if (type == SCStreamOutputTypeAudio) {
    _bridge->handle_audio_frame(sampleBuffer);
  }
}

- (void)stream:(SCStream*)stream didStopWithError:(NSError*)error {
  (void)stream;
  (void)error;
  if (_bridge) {
    _bridge->failed.store(true, std::memory_order_relaxed);
  }
}
@end

@implementation PartiesMacosCameraOutput
- (void)captureOutput:(AVCaptureOutput*)output didOutputSampleBuffer:(CMSampleBufferRef)sampleBuffer fromConnection:(AVCaptureConnection*)connection {
  (void)output;
  (void)connection;
  if (!_bridge) {
    return;
  }
  _bridge->handle_camera_frame(sampleBuffer);
}
@end

extern "C" {

char* parties_macos_desktop_sources_json(uint8_t source_kind) {
  set_last_error("");
  std::string json = "[";
  if (@available(macOS 12.3, *)) {
    SCShareableContent* content = copy_shareable_content_sync();
    if (!content) {
      set_last_error("failed to list ScreenCaptureKit sources");
      json += "]";
      return copy_c_string(json);
    }

    if (source_kind == 0) {
      NSUInteger index = 0;
      for (SCDisplay* display in content.displays) {
        CGDirectDisplayID display_id = display.displayID;
        CGRect frame = CGDisplayBounds(display_id);
        uint64_t width = static_cast<uint64_t>(CGDisplayPixelsWide(display_id));
        uint64_t height = static_cast<uint64_t>(CGDisplayPixelsHigh(display_id));
        std::string name = "Display " + std::to_string(static_cast<unsigned long long>(++index));
        std::string description;
        append_desktop_source_json(
          json,
          display_id,
          static_cast<int64_t>(std::llround(frame.origin.x)),
          static_cast<int64_t>(std::llround(frame.origin.y)),
          width,
          height,
          name,
          description);
      }
    } else {
      for (SCWindow* window in content.windows) {
        CGRect frame = window.frame;
        if (!std::isfinite(frame.size.width) || !std::isfinite(frame.size.height) ||
            frame.size.width <= 0 || frame.size.height <= 0) {
          continue;
        }
        NSString* title = window.title ?: @"";
        NSString* app_name = window.owningApplication.applicationName ?: @"";
        std::string title_text(title.UTF8String ?: "");
        std::string app_text(app_name.UTF8String ?: "");
        std::string name;
        if (app_text.empty()) {
          name = title_text;
        } else if (title_text.empty() || title_text == app_text) {
          name = app_text;
        } else {
          name = app_text + " - " + title_text;
        }
        append_desktop_source_json(
          json,
          window.windowID,
          static_cast<int64_t>(std::llround(frame.origin.x)),
          static_cast<int64_t>(std::llround(frame.origin.y)),
          static_cast<uint64_t>(std::llround(frame.size.width)),
          static_cast<uint64_t>(std::llround(frame.size.height)),
          name,
          app_text);
      }
    }
    json += "]";
    return copy_c_string(json);
  }

  set_last_error("ScreenCaptureKit requires macOS 12.3 or newer");
  json += "]";
  return copy_c_string(json);
}

void parties_macos_string_free(char* text) {
  std::free(text);
}

int parties_macos_microphone_authorize() {
  set_last_error("");
  return ensure_microphone_authorized() ? 1 : 0;
}

uintptr_t parties_macos_camera_refresh() {
  set_last_error("");
  auto& devices = camera_devices();
  devices.clear();
  for (AVCaptureDevice* device in native_camera_devices()) {
    std::string unique_id(device.uniqueID.UTF8String ?: "");
    std::string name(device.localizedName.UTF8String ?: "");
    if (unique_id.empty()) {
      continue;
    }
    if (name.empty()) {
      name = "Camera";
    }
    devices.push_back(CameraDeviceInfo{unique_id, name});
  }
  return devices.size();
}

const char* parties_macos_camera_unique_id(uintptr_t index) {
  auto& devices = camera_devices();
  if (index >= devices.size()) {
    return nullptr;
  }
  return devices[index].unique_id.c_str();
}

const char* parties_macos_camera_name(uintptr_t index) {
  auto& devices = camera_devices();
  if (index >= devices.size()) {
    return nullptr;
  }
  return devices[index].name.c_str();
}

MacosStreamBridge* parties_macos_stream_create(uint8_t source_kind,
                                               uint64_t source_id,
                                               uint8_t codec,
                                               uint16_t width,
                                               uint16_t height,
                                               uint32_t fps,
                                               uint32_t bitrate,
                                               int audio_enabled) {
  if (@available(macOS 12.3, *)) {
    set_last_error("");
    if (source_id == 0 || width == 0 || height == 0 || fps == 0 || bitrate == 0) {
      set_last_error("invalid screen stream configuration");
      return nullptr;
    }

    auto* bridge = new MacosStreamBridge();
    bridge->codec = codec;
    bridge->frame_duration_100ns = static_cast<int64_t>(kTimeScale100Ns / fps);
    bridge->audio_enabled.store(audio_enabled != 0, std::memory_order_relaxed);

    if (!create_encoder(bridge, width, height, codec, fps, bitrate)) {
      delete bridge;
      return nullptr;
    }

    SCContentFilter* filter = create_filter(source_kind, source_id);
    if (!filter) {
      set_last_error("screen source is no longer available");
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
    config.capturesAudio = audio_enabled ? YES : NO;
    config.sampleRate = kStreamAudioSampleRate;
    config.channelCount = kStreamAudioChannels;
    if ([config respondsToSelector:@selector(setExcludesCurrentProcessAudio:)]) {
      config.excludesCurrentProcessAudio = YES;
    }

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
      set_last_error("failed to add ScreenCaptureKit output: " + ns_error_string(add_error));
      delete bridge;
      return nullptr;
    }
    if (audio_enabled && ![stream addStreamOutput:output type:SCStreamOutputTypeAudio sampleHandlerQueue:queue error:&add_error]) {
      set_last_error("failed to add ScreenCaptureKit audio output: " + ns_error_string(add_error));
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
      set_last_error("failed to start ScreenCaptureKit stream");
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
  set_last_error("ScreenCaptureKit requires macOS 12.3 or newer");
  return nullptr;
}

MacosStreamBridge* parties_macos_camera_stream_create(uint64_t source_id,
                                                      uint8_t codec,
                                                      uint16_t width,
                                                      uint16_t height,
                                                      uint32_t fps,
                                                      uint32_t bitrate) {
  set_last_error("");
  if (source_id == 0 || width == 0 || height == 0 || fps == 0 || bitrate == 0) {
    set_last_error("invalid camera stream configuration");
    return nullptr;
  }

  if (!ensure_camera_authorized()) {
    return nullptr;
  }

  AVCaptureDevice* device = find_camera_device(source_id);
  if (!device) {
    set_last_error("selected camera is no longer available");
    return nullptr;
  }
  uint32_t actual_fps = fps;
  if (!configure_camera_format(device, width, height, fps, &actual_fps)) {
    return nullptr;
  }

  auto* bridge = new MacosStreamBridge();
  bridge->codec = codec;
  bridge->frame_duration_100ns = static_cast<int64_t>(kTimeScale100Ns / actual_fps);

  if (!create_encoder(bridge, width, height, codec, actual_fps, bitrate)) {
    delete bridge;
    return nullptr;
  }

  NSError* error = nil;
  AVCaptureDeviceInput* input = [AVCaptureDeviceInput deviceInputWithDevice:device error:&error];
  if (!input || error) {
    set_last_error("failed to create AVFoundation camera input: " + ns_error_string(error));
    delete bridge;
    return nullptr;
  }

  AVCaptureVideoDataOutput* video_output = [[AVCaptureVideoDataOutput alloc] init];
  video_output.videoSettings = @{
    (__bridge NSString*)kCVPixelBufferPixelFormatTypeKey: @(kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange),
    (__bridge NSString*)kCVPixelBufferWidthKey: @(width),
    (__bridge NSString*)kCVPixelBufferHeightKey: @(height),
  };
  video_output.alwaysDiscardsLateVideoFrames = YES;

  PartiesMacosCameraOutput* output = [[PartiesMacosCameraOutput alloc] init];
  output.bridge = bridge;
  dispatch_queue_t queue = dispatch_queue_create("parties.macos.camera-stream", DISPATCH_QUEUE_SERIAL);
  [video_output setSampleBufferDelegate:output queue:queue];

  AVCaptureSession* session = [[AVCaptureSession alloc] init];
  [session beginConfiguration];
  if ([session canAddInput:input]) {
    [session addInput:input];
  } else {
    set_last_error("AVFoundation session rejected camera input");
    [session commitConfiguration];
    delete bridge;
    return nullptr;
  }
  if ([session canAddOutput:video_output]) {
    [session addOutput:video_output];
  } else {
    set_last_error("AVFoundation session rejected camera video output");
    [session commitConfiguration];
    delete bridge;
    return nullptr;
  }
  session.sessionPreset = AVCaptureSessionPresetHigh;
  [session commitConfiguration];

  {
    std::lock_guard<std::mutex> lock(bridge->mutex);
    bridge->camera_output = output;
    bridge->camera_session = session;
  }
  bridge->encoder_ready.store(true, std::memory_order_relaxed);
  [session startRunning];
  if (!session.running) {
    set_last_error("AVFoundation camera session did not start");
    delete bridge;
    return nullptr;
  }
  return bridge;
}

const char* parties_macos_stream_last_error() {
  return last_error().c_str();
}

const char* parties_macos_last_error() {
  return parties_macos_stream_last_error();
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

MacosEncodedBuffer* parties_macos_stream_take_encoded(MacosStreamBridge* bridge) {
  if (!bridge) {
    return nullptr;
  }
  std::lock_guard<std::mutex> lock(bridge->mutex);
  if (bridge->readable.empty()) {
    return nullptr;
  }
  auto* buffer = new MacosEncodedBuffer();
  buffer->bytes = std::move(bridge->readable);
  buffer->keyframe = bridge->readable_keyframe;
  bridge->readable.clear();
  bridge->readable_keyframe = false;
  return buffer;
}

const uint8_t* parties_macos_encoded_buffer_ptr(MacosEncodedBuffer* buffer) {
  if (!buffer || buffer->bytes.empty()) {
    return nullptr;
  }
  return buffer->bytes.data();
}

uintptr_t parties_macos_encoded_buffer_len(MacosEncodedBuffer* buffer) {
  if (!buffer) {
    return 0;
  }
  return buffer->bytes.size();
}

int parties_macos_encoded_buffer_keyframe(MacosEncodedBuffer* buffer) {
  if (!buffer) {
    return 0;
  }
  return buffer->keyframe ? 1 : 0;
}

void parties_macos_encoded_buffer_destroy(MacosEncodedBuffer* buffer) {
  delete buffer;
}

int parties_macos_stream_audio_poll(MacosStreamBridge* bridge) {
  if (!bridge) {
    return -1;
  }
  std::lock_guard<std::mutex> lock(bridge->mutex);
  if (bridge->pending_audio.empty()) {
    return 0;
  }
  bridge->readable_audio.swap(bridge->pending_audio);
  bridge->pending_audio.clear();
  return 1;
}

const float* parties_macos_stream_audio_ptr(MacosStreamBridge* bridge) {
  if (!bridge || bridge->readable_audio.empty()) {
    return nullptr;
  }
  return bridge->readable_audio.data();
}

uintptr_t parties_macos_stream_audio_len(MacosStreamBridge* bridge) {
  if (!bridge) {
    return 0;
  }
  return bridge->readable_audio.size();
}

MacosAudioBuffer* parties_macos_stream_take_audio(MacosStreamBridge* bridge) {
  if (!bridge) {
    return nullptr;
  }
  std::lock_guard<std::mutex> lock(bridge->mutex);
  if (bridge->readable_audio.empty()) {
    return nullptr;
  }
  auto* buffer = new MacosAudioBuffer();
  buffer->samples = std::move(bridge->readable_audio);
  bridge->readable_audio.clear();
  return buffer;
}

const float* parties_macos_audio_buffer_ptr(MacosAudioBuffer* buffer) {
  if (!buffer || buffer->samples.empty()) {
    return nullptr;
  }
  return buffer->samples.data();
}

uintptr_t parties_macos_audio_buffer_len(MacosAudioBuffer* buffer) {
  if (!buffer) {
    return 0;
  }
  return buffer->samples.size();
}

void parties_macos_audio_buffer_destroy(MacosAudioBuffer* buffer) {
  delete buffer;
}

void parties_macos_sparkle_start() {
  if ([NSThread isMainThread]) {
    ensure_sparkle_updater_controller();
    check_sparkle_updates_in_background_once();
    return;
  }

  dispatch_async(dispatch_get_main_queue(), ^{
    ensure_sparkle_updater_controller();
    check_sparkle_updates_in_background_once();
  });
}

void parties_macos_sparkle_check_for_updates() {
  dispatch_async(dispatch_get_main_queue(), ^{
    id controller = ensure_sparkle_updater_controller();
    if (controller == nil || ![controller respondsToSelector:@selector(checkForUpdates:)]) {
      return;
    }
    ((void (*)(id, SEL, id))objc_msgSend)(controller, @selector(checkForUpdates:), nil);
  });
}

}
