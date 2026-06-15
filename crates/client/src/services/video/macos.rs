use std::{
  ffi::{CStr, c_char, c_void},
  ops::Range,
  ptr,
  ptr::NonNull,
  slice,
  sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
    mpsc,
  },
  thread,
  time::{Duration, Instant},
};

use bytes::Bytes;
use core_foundation_sys::{
  array::{CFArrayGetValueAtIndex, CFArrayRef},
  base::{Boolean, CFAllocatorRef, CFRelease, CFTypeRef, OSStatus, kCFAllocatorDefault, kCFAllocatorNull},
  data::CFDataCreate,
  dictionary::{
    CFDictionaryCreate, CFDictionaryGetValue, CFDictionaryRef, kCFTypeDictionaryKeyCallBacks,
    kCFTypeDictionaryValueCallBacks,
  },
  number::{CFNumberCreate, kCFBooleanFalse, kCFBooleanTrue, kCFNumberSInt32Type},
  string::{CFStringCreateWithBytes, CFStringRef, kCFStringEncodingUTF8},
};
use core_media_sys::{
  block_buffer::{
    CMBlockBufferCopyDataBytes, CMBlockBufferCreateWithMemoryBlock, CMBlockBufferGetDataLength, CMBlockBufferRef,
  },
  format_description::CMVideoFormatDescriptionRef,
  sample_buffer::{CMSampleBufferRef, CMSampleTimingInfo},
  time::{CMTime, kCMTimeFlags_Valid, kCMTimeInvalid},
};
use core_video_sys::pixel_buffer::{
  CVPixelBufferGetBaseAddressOfPlane, CVPixelBufferGetBytesPerRowOfPlane, CVPixelBufferGetHeight,
  CVPixelBufferGetPixelFormatType, CVPixelBufferGetWidth, CVPixelBufferLockBaseAddress, CVPixelBufferRef,
  CVPixelBufferUnlockBaseAddress, kCVPixelBufferIOSurfacePropertiesKey, kCVPixelBufferMetalCompatibilityKey,
  kCVPixelBufferPixelFormatTypeKey, kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange,
};
use opus::{Application as OpusApplication, Bitrate as OpusBitrate, Channels as OpusChannels, Encoder as OpusEncoder};

use super::{
  DecodedVideoPixelFormat, NativeDecodedVideoFrame, NativeVideoBackend, VideoBroadcast, VideoBroadcastConfig,
  VideoDecodeConfig, VideoDecoder, VideoError, VideoFrameDecoder, VideoFrameLoopback, software::SoftwareVideoDecoder,
};
use crate::{
  network::{
    protocol::{VideoCodecId, data::VideoFrame},
    server::{Server, VideoFrameSend},
  },
  services::{
    desktop_capture::{DesktopCaptureSource, DesktopCaptureSourceKind},
    screen_share_sources::ScreenShareSourceKind,
  },
};

#[allow(dead_code)]
const BACKEND_ORDER: [NativeVideoBackend; 1] = [NativeVideoBackend::AppleVideoToolbox];
const NO_ERR: OSStatus = 0;
const K_CM_VIDEO_CODEC_TYPE_H264: u32 = 0x6176_6331; // 'avc1'
const K_CM_VIDEO_CODEC_TYPE_HEVC: u32 = 0x6876_6331; // 'hvc1'
const K_CM_VIDEO_CODEC_TYPE_AV1: u32 = 0x6176_3031; // 'av01'
const OBU_SEQUENCE_HEADER: u8 = 1;
const SIMULATE_UNSUPPORTED_AV1_ENV: &str = "PARTIES_SIMULATE_UNSUPPORTED_AV1";
const ALLOW_CPU_VIDEO_FALLBACK_ENV: &str = "PARTIES_MACOS_ALLOW_CPU_VIDEO_FALLBACK";
const MAX_KEYFRAME_INTERVAL_SECONDS: u32 = 600;
const ENCODE_FRAME_DURATION_100NS: i64 = 10_000_000;
const STREAM_AUDIO_SAMPLE_RATE: u32 = 48_000;
const STREAM_AUDIO_CHANNELS: usize = 2;
const STREAM_AUDIO_FRAME_SAMPLES_PER_CHANNEL: usize = 960;
const STREAM_AUDIO_FRAME_SAMPLES: usize = STREAM_AUDIO_FRAME_SAMPLES_PER_CHANNEL * STREAM_AUDIO_CHANNELS;
const STREAM_AUDIO_BITRATE: i32 = 64_000;
const STREAM_AUDIO_MAX_PACKET_BYTES: usize = 1_275;

#[repr(C)]
struct VTDecompressionSession(c_void);

type VTDecompressionSessionRef = *mut VTDecompressionSession;
type VTDecodeFrameFlags = u32;
type VTDecodeInfoFlags = u32;

#[repr(C)]
struct VTCompressionSession(c_void);

type VTCompressionSessionRef = *mut VTCompressionSession;
type VTEncodeInfoFlags = u32;

#[repr(C)]
struct MacosStreamBridge(c_void);

#[repr(C)]
struct MacosEncodedBuffer(c_void);

#[repr(C)]
struct MacosAudioBuffer(c_void);

type VTDecompressionOutputCallback = extern "C" fn(
  decompression_output_ref_con: *mut c_void,
  source_frame_ref_con: *mut c_void,
  status: OSStatus,
  info_flags: VTDecodeInfoFlags,
  image_buffer: CVPixelBufferRef,
  presentation_time_stamp: CMTime,
  presentation_duration: CMTime,
);

#[repr(C)]
struct VTDecompressionOutputCallbackRecord {
  decompression_output_callback: VTDecompressionOutputCallback,
  decompression_output_ref_con: *mut c_void,
}

type VTCompressionOutputCallback = extern "C" fn(
  output_callback_ref_con: *mut c_void,
  source_frame_ref_con: *mut c_void,
  status: OSStatus,
  info_flags: VTEncodeInfoFlags,
  sample_buffer: CMSampleBufferRef,
);

unsafe extern "C" {
  fn CMVideoFormatDescriptionCreateFromH264ParameterSets(
    allocator: CFAllocatorRef,
    parameter_set_count: usize,
    parameter_set_pointers: *const *const u8,
    parameter_set_sizes: *const usize,
    nal_unit_header_length: i32,
    format_description_out: *mut CMVideoFormatDescriptionRef,
  ) -> OSStatus;

  fn CMVideoFormatDescriptionCreateFromHEVCParameterSets(
    allocator: CFAllocatorRef,
    parameter_set_count: usize,
    parameter_set_pointers: *const *const u8,
    parameter_set_sizes: *const usize,
    nal_unit_header_length: i32,
    extensions: CFDictionaryRef,
    format_description_out: *mut CMVideoFormatDescriptionRef,
  ) -> OSStatus;

  fn CMVideoFormatDescriptionCreate(
    allocator: CFAllocatorRef,
    codec_type: u32,
    width: i32,
    height: i32,
    extensions: CFDictionaryRef,
    format_description_out: *mut CMVideoFormatDescriptionRef,
  ) -> OSStatus;

  fn CMSampleBufferCreateReady(
    allocator: CFAllocatorRef,
    data_buffer: CMBlockBufferRef,
    format_description: CMVideoFormatDescriptionRef,
    sample_count: isize,
    sample_timing_entry_count: isize,
    sample_timing_array: *const CMSampleTimingInfo,
    sample_size_entry_count: isize,
    sample_size_array: *const usize,
    sample_buffer_out: *mut CMSampleBufferRef,
  ) -> OSStatus;

  fn CMSampleBufferDataIsReady(sample_buffer: CMSampleBufferRef) -> Boolean;

  fn CMSampleBufferGetDataBuffer(sample_buffer: CMSampleBufferRef) -> CMBlockBufferRef;

  fn CMSampleBufferGetFormatDescription(sample_buffer: CMSampleBufferRef) -> CMVideoFormatDescriptionRef;

  fn CMSampleBufferGetSampleAttachmentsArray(
    sample_buffer: CMSampleBufferRef,
    create_if_necessary: Boolean,
  ) -> CFArrayRef;

  fn CMVideoFormatDescriptionGetH264ParameterSetAtIndex(
    video_desc: CMVideoFormatDescriptionRef,
    parameter_set_index: usize,
    parameter_set_pointer_out: *mut *const u8,
    parameter_set_size_out: *mut usize,
    parameter_set_count_out: *mut usize,
    nal_unit_header_length_out: *mut i32,
  ) -> OSStatus;

  fn CMVideoFormatDescriptionGetHEVCParameterSetAtIndex(
    video_desc: CMVideoFormatDescriptionRef,
    parameter_set_index: usize,
    parameter_set_pointer_out: *mut *const u8,
    parameter_set_size_out: *mut usize,
    parameter_set_count_out: *mut usize,
    nal_unit_header_length_out: *mut i32,
  ) -> OSStatus;

  fn CVPixelBufferCreate(
    allocator: CFAllocatorRef,
    width: usize,
    height: usize,
    pixel_format_type: u32,
    pixel_buffer_attributes: CFDictionaryRef,
    pixel_buffer_out: *mut CVPixelBufferRef,
  ) -> OSStatus;

  fn VTDecompressionSessionCreate(
    allocator: CFAllocatorRef,
    video_format_description: CMVideoFormatDescriptionRef,
    video_decoder_specification: CFDictionaryRef,
    destination_image_buffer_attributes: CFDictionaryRef,
    output_callback: *const VTDecompressionOutputCallbackRecord,
    decompression_session_out: *mut VTDecompressionSessionRef,
  ) -> OSStatus;

  fn VTDecompressionSessionDecodeFrame(
    session: VTDecompressionSessionRef,
    sample_buffer: CMSampleBufferRef,
    decode_flags: VTDecodeFrameFlags,
    source_frame_ref_con: *mut c_void,
    info_flags_out: *mut VTDecodeInfoFlags,
  ) -> OSStatus;

  fn VTDecompressionSessionInvalidate(session: VTDecompressionSessionRef);

  fn VTCompressionSessionCreate(
    allocator: CFAllocatorRef,
    width: i32,
    height: i32,
    codec_type: u32,
    encoder_specification: CFDictionaryRef,
    source_image_buffer_attributes: CFDictionaryRef,
    compressed_data_allocator: CFAllocatorRef,
    output_callback: VTCompressionOutputCallback,
    output_callback_ref_con: *mut c_void,
    compression_session_out: *mut VTCompressionSessionRef,
  ) -> OSStatus;

  fn VTCompressionSessionSetProperty(
    session: VTCompressionSessionRef,
    property_key: CFStringRef,
    property_value: CFTypeRef,
  ) -> OSStatus;

  fn VTCompressionSessionPrepareToEncodeFrames(session: VTCompressionSessionRef) -> OSStatus;

  fn VTCompressionSessionEncodeFrame(
    session: VTCompressionSessionRef,
    image_buffer: CVPixelBufferRef,
    presentation_time_stamp: CMTime,
    duration: CMTime,
    frame_properties: CFDictionaryRef,
    source_frame_ref_con: *mut c_void,
    info_flags_out: *mut VTEncodeInfoFlags,
  ) -> OSStatus;

  fn VTCompressionSessionCompleteFrames(
    session: VTCompressionSessionRef,
    complete_until_presentation_time_stamp: CMTime,
  ) -> OSStatus;

  fn VTCompressionSessionInvalidate(session: VTCompressionSessionRef);

  fn parties_macos_stream_create(
    source_kind: u8,
    source_id: u64,
    codec: u8,
    width: u16,
    height: u16,
    fps: u32,
    bitrate: u32,
    audio_enabled: i32,
  ) -> *mut MacosStreamBridge;

  fn parties_macos_camera_stream_create(
    source_id: u64,
    codec: u8,
    width: u16,
    height: u16,
    fps: u32,
    bitrate: u32,
  ) -> *mut MacosStreamBridge;

  fn parties_macos_stream_destroy(bridge: *mut MacosStreamBridge);

  fn parties_macos_stream_last_error() -> *const c_char;

  fn parties_macos_stream_force_keyframe(bridge: *mut MacosStreamBridge);

  fn parties_macos_stream_poll(bridge: *mut MacosStreamBridge) -> i32;

  fn parties_macos_stream_take_encoded(bridge: *mut MacosStreamBridge) -> *mut MacosEncodedBuffer;

  fn parties_macos_encoded_buffer_ptr(buffer: *mut MacosEncodedBuffer) -> *const u8;

  fn parties_macos_encoded_buffer_len(buffer: *mut MacosEncodedBuffer) -> usize;

  fn parties_macos_encoded_buffer_keyframe(buffer: *mut MacosEncodedBuffer) -> i32;

  fn parties_macos_encoded_buffer_destroy(buffer: *mut MacosEncodedBuffer);

  fn parties_macos_stream_audio_poll(bridge: *mut MacosStreamBridge) -> i32;

  fn parties_macos_stream_take_audio(bridge: *mut MacosStreamBridge) -> *mut MacosAudioBuffer;

  fn parties_macos_audio_buffer_ptr(buffer: *mut MacosAudioBuffer) -> *const f32;

  fn parties_macos_audio_buffer_len(buffer: *mut MacosAudioBuffer) -> usize;

  fn parties_macos_audio_buffer_destroy(buffer: *mut MacosAudioBuffer);

  static kCMSampleAttachmentKey_NotSync: CFStringRef;
}

pub(super) fn encode(
  server: Arc<Server>,
  config: VideoBroadcastConfig,
  loopback: Option<VideoFrameLoopback>,
) -> Result<VideoBroadcast, VideoError> {
  let runtime = tokio::runtime::Handle::try_current()
    .map_err(|_| VideoError::new("Video broadcasting must be started from the Tokio runtime."))?;
  let stop = Arc::new(AtomicBool::new(false));
  let keyframe_requests = Arc::new(AtomicU64::new(0));
  let thread_stop = Arc::clone(&stop);
  let thread_keyframe_requests = Arc::clone(&keyframe_requests);
  let (init_tx, init_rx) = mpsc::channel();
  let thread = thread::Builder::new()
    .name("parties-video-macos-encode".to_owned())
    .spawn(move || {
      let loop_stop = Arc::clone(&thread_stop);
      if let Err(error) = run_broadcast_loop(
        server,
        config,
        runtime,
        loop_stop,
        thread_keyframe_requests,
        loopback,
        Some(init_tx),
      ) {
        thread_stop.store(true, Ordering::Relaxed);
        tracing::warn!(target: "video::encode::macos", "[video:encode/macos] broadcast loop stopped with error: {error}");
      }
    })
    .map_err(|error| VideoError::new(format!("Failed to start macOS video broadcast thread: {error}")))?;

  match init_rx.recv() {
    Ok(Ok(())) => {}
    Ok(Err(error)) => {
      stop.store(true, Ordering::Relaxed);
      let _ = thread.join();
      return Err(VideoError::new(error));
    }
    Err(_) => {
      stop.store(true, Ordering::Relaxed);
      let _ = thread.join();
      return Err(VideoError::new(
        "macOS video broadcast thread stopped before encoder initialization completed.",
      ));
    }
  }

  Ok(VideoBroadcast::from_parts_with_stop_and_keyframes(
    NativeVideoBackend::AppleVideoToolbox,
    stop,
    Some(keyframe_requests),
    vec![thread],
  ))
}

fn run_broadcast_loop(
  server: Arc<Server>,
  config: VideoBroadcastConfig,
  runtime: tokio::runtime::Handle,
  stop: Arc<AtomicBool>,
  keyframe_requests: Arc<AtomicU64>,
  loopback: Option<VideoFrameLoopback>,
  init_tx: Option<mpsc::Sender<Result<(), String>>>,
) -> Result<(), VideoError> {
  match MacosNativeStreamEncoder::new(&config) {
    Ok(mut encoder) => {
      tracing::info!(target: "video::encode::macos",
        "[video:encode/macos] native ScreenCaptureKit encoder ready: codec={:?} source={}x{} output={}x{} fps={} bitrate={}kbps",
        config.codec,
        config.source_width,
        config.source_height,
        config.output_width,
        config.output_height,
        config.fps,
        config.bitrate_kbps
      );
      if let Some(init_tx) = init_tx {
        let _ = init_tx.send(Ok(()));
      }
      return run_native_broadcast_loop(server, config, runtime, stop, keyframe_requests, loopback, &mut encoder);
    }
    Err(error) => {
      if config.source_kind == ScreenShareSourceKind::Webcam {
        tracing::warn!(target: "video::encode::macos", "[video:encode/macos] native AVFoundation webcam encoder unavailable: {error}");
        if let Some(init_tx) = init_tx {
          let _ = init_tx.send(Err(error.to_string()));
        }
        return Err(error);
      }
      if !cpu_video_fallback_enabled() {
        let fallback_error = VideoError::new(format!(
          "Native macOS ScreenCaptureKit + VideoToolbox streaming failed and CPU video fallback is disabled to avoid high CPU usage. Set {ALLOW_CPU_VIDEO_FALLBACK_ENV}=1 to allow the legacy CPU fallback. Native error: {error}"
        ));
        tracing::warn!(target: "video::encode::macos", "[video:encode/macos] {fallback_error}");
        if let Some(init_tx) = init_tx {
          let _ = init_tx.send(Err(fallback_error.to_string()));
        }
        return Err(fallback_error);
      }
      tracing::warn!(target: "video::encode::macos", "[video:encode/macos] native ScreenCaptureKit encoder unavailable; CPU fallback explicitly enabled by {ALLOW_CPU_VIDEO_FALLBACK_ENV}: {error}");
    }
  }

  tracing::info!(target: "video::encode::macos", "[video:encode/macos] creating VideoToolbox encoder");
  let mut encoder = match VTEncoder::new(&config) {
    Ok(encoder) => encoder,
    Err(error) => {
      if let Some(init_tx) = init_tx {
        let _ = init_tx.send(Err(error.to_string()));
      }
      return Err(error);
    }
  };
  tracing::info!(target: "video::encode::macos",
    "[video:encode/macos] encoder ready: codec={:?} source={}x{} output={}x{} fps={} bitrate={}kbps",
    config.codec,
    config.source_width,
    config.source_height,
    config.output_width,
    config.output_height,
    config.fps,
    config.bitrate_kbps
  );
  tracing::info!(target: "video::encode::macos", "[video:encode/macos] opening CPU capture source");
  let mut source = match CaptureSource::open(&config) {
    Ok(source) => source,
    Err(error) => {
      if let Some(init_tx) = init_tx {
        let _ = init_tx.send(Err(error.to_string()));
      }
      return Err(error);
    }
  };
  if let Some(init_tx) = init_tx {
    let _ = init_tx.send(Ok(()));
  }
  let frame_interval = Duration::from_nanos(1_000_000_000u64 / u64::from(config.fps.max(1)));
  let started_at = Instant::now();
  let mut frame_number = 0u32;
  let mut logged_first_frame = false;
  let mut logged_stream_fallback = false;
  let mut dropped_live_frames = 0u64;
  let mut handled_keyframe_requests = keyframe_requests.load(Ordering::Relaxed);

  while !stop.load(Ordering::Relaxed) {
    let loop_started_at = Instant::now();
    let requested_keyframes = keyframe_requests.load(Ordering::Relaxed);
    let force_keyframe = requested_keyframes != handled_keyframe_requests || frame_number == 0;
    if requested_keyframes != handled_keyframe_requests {
      handled_keyframe_requests = requested_keyframes;
      tracing::debug!(target: "video::encode::macos", "[video:encode/macos] keyframe requested by PLI");
    }

    let rgba = source.capture_rgba(config.output_width, config.output_height)?;
    let timestamp_100ns = started_at.elapsed().as_nanos().saturating_div(100) as i64;
    let timestamp_ms = started_at.elapsed().as_millis().min(u128::from(u32::MAX)) as u32;
    let samples = encoder.encode(&rgba, timestamp_100ns, force_keyframe)?;

    for sample in samples {
      let sample_len = sample.bytes.len();
      let sample_keyframe = sample.keyframe;
      let frame = VideoFrame {
        frame_number,
        timestamp: timestamp_ms,
        keyframe: sample_keyframe,
        width: config.output_width,
        height: config.output_height,
        codec: config.codec,
        encoded: sample.bytes.into(),
      };
      let send_result = runtime
        .block_on(server.send_live_video_frame(&frame))
        .map_err(|error| VideoError::new(format!("Failed to send video frame: {error}")))?;
      if send_result == VideoFrameSend::Dropped {
        dropped_live_frames += 1;
        if dropped_live_frames == 1 || dropped_live_frames % 120 == 0 {
          tracing::info!(target: "video::encode::macos",
            "[video:encode/macos] dropped live video frame before network queue: frame={} total_dropped={}",
            frame_number,
            dropped_live_frames
          );
        }
        continue;
      }
      if let Some(loopback) = &loopback {
        loopback(frame);
      }
      if send_result == VideoFrameSend::StreamFallback && !logged_stream_fallback {
        tracing::warn!(target: "video::encode::macos", "[video:encode/macos] live video datagrams unavailable or too large; using reliable stream fallback");
        logged_stream_fallback = true;
      }
      if !logged_first_frame {
        tracing::info!(target: "video::encode::macos",
          "[video:encode/macos] first encoded frame sent: frame={} bytes={} keyframe={} transport={:?}",
          frame_number,
          sample_len,
          sample_keyframe,
          send_result
        );
        logged_first_frame = true;
      } else if frame_number > 0 && frame_number % 120 == 0 {
        tracing::debug!(target: "video::encode::macos",
          "[video:encode/macos] encoded frame #{} sent: bytes={} keyframe={} transport={:?}",
          frame_number,
          sample_len,
          sample_keyframe,
          send_result
        );
      }
    }

    frame_number = frame_number.wrapping_add(1);
    let elapsed = loop_started_at.elapsed();
    if elapsed < frame_interval {
      thread::sleep(frame_interval - elapsed);
    }
  }

  tracing::info!(target: "video::encode::macos", "[video:encode/macos] broadcast loop stopped by request");
  Ok(())
}

fn run_native_broadcast_loop(
  server: Arc<Server>,
  config: VideoBroadcastConfig,
  runtime: tokio::runtime::Handle,
  stop: Arc<AtomicBool>,
  keyframe_requests: Arc<AtomicU64>,
  loopback: Option<VideoFrameLoopback>,
  encoder: &mut MacosNativeStreamEncoder,
) -> Result<(), VideoError> {
  let frame_interval = Duration::from_nanos(1_000_000_000u64 / u64::from(config.fps.max(1)));
  let poll_interval = frame_interval.min(Duration::from_millis(5));
  let started_at = Instant::now();
  let mut frame_number = 0u32;
  let mut logged_first_frame = false;
  let mut logged_stream_fallback = false;
  let mut dropped_live_frames = 0u64;
  let mut handled_keyframe_requests = keyframe_requests.load(Ordering::Relaxed);
  let mut audio_encoder = if config.audio_enabled {
    Some(StreamAudioEncoder::new()?)
  } else {
    None
  };

  while !stop.load(Ordering::Relaxed) {
    let requested_keyframes = keyframe_requests.load(Ordering::Relaxed);
    if requested_keyframes != handled_keyframe_requests {
      handled_keyframe_requests = requested_keyframes;
      encoder.force_keyframe();
      tracing::debug!(target: "video::encode::macos", "[video:encode/macos] keyframe requested by PLI");
    }

    let samples = encoder.poll()?;
    for sample in samples {
      let sample_len = sample.bytes.len();
      let sample_keyframe = sample.keyframe;
      let timestamp_ms = started_at.elapsed().as_millis().min(u128::from(u32::MAX)) as u32;
      let frame = VideoFrame {
        frame_number,
        timestamp: timestamp_ms,
        keyframe: sample_keyframe,
        width: config.output_width,
        height: config.output_height,
        codec: config.codec,
        encoded: sample.bytes.into(),
      };
      let send_result = runtime
        .block_on(server.send_live_video_frame(&frame))
        .map_err(|error| VideoError::new(format!("Failed to send video frame: {error}")))?;
      if send_result == VideoFrameSend::Dropped {
        dropped_live_frames += 1;
        if dropped_live_frames == 1 || dropped_live_frames % 120 == 0 {
          tracing::info!(target: "video::encode::macos",
            "[video:encode/macos] dropped live video frame before network queue: frame={} total_dropped={}",
            frame_number,
            dropped_live_frames
          );
        }
        frame_number = frame_number.wrapping_add(1);
        continue;
      }
      if let Some(loopback) = &loopback {
        loopback(frame);
      }
      if send_result == VideoFrameSend::StreamFallback && !logged_stream_fallback {
        tracing::warn!(target: "video::encode::macos", "[video:encode/macos] live video datagrams unavailable or too large; using reliable stream fallback");
        logged_stream_fallback = true;
      }
      if !logged_first_frame {
        tracing::info!(target: "video::encode::macos",
          "[video:encode/macos] first native encoded frame sent: frame={} bytes={} keyframe={} transport={:?}",
          frame_number,
          sample_len,
          sample_keyframe,
          send_result
        );
        logged_first_frame = true;
      } else if frame_number > 0 && frame_number % 120 == 0 {
        tracing::debug!(target: "video::encode::macos",
          "[video:encode/macos] native encoded frame #{} sent: bytes={} keyframe={} transport={:?}",
          frame_number,
          sample_len,
          sample_keyframe,
          send_result
        );
      }
      frame_number = frame_number.wrapping_add(1);
    }

    if let Some(audio_encoder) = audio_encoder.as_mut() {
      if let Some(audio) = encoder.poll_audio()? {
        audio_encoder.encode_samples(&server, audio.as_ref())?;
      }
    }

    thread::sleep(poll_interval);
  }

  tracing::info!(target: "video::encode::macos", "[video:encode/macos] native broadcast loop stopped by request");
  Ok(())
}

struct StreamAudioEncoder {
  encoder: OpusEncoder,
  pcm_frame: Vec<f32>,
  opus_packet: Vec<u8>,
  logged_first_packet: bool,
}

impl StreamAudioEncoder {
  fn new() -> Result<Self, VideoError> {
    let mut encoder = OpusEncoder::new(STREAM_AUDIO_SAMPLE_RATE, OpusChannels::Stereo, OpusApplication::Audio)
      .map_err(|error| VideoError::new(format!("Failed to create macOS stream audio Opus encoder: {error}")))?;
    encoder
      .set_bitrate(OpusBitrate::Bits(STREAM_AUDIO_BITRATE))
      .map_err(|error| VideoError::new(format!("Failed to configure macOS stream audio Opus bitrate: {error}")))?;
    tracing::debug!(target: "audio::encode::macos", "[audio:encode/macos] stream audio capture enabled");
    Ok(Self {
      encoder,
      pcm_frame: Vec::with_capacity(STREAM_AUDIO_FRAME_SAMPLES),
      opus_packet: vec![0; STREAM_AUDIO_MAX_PACKET_BYTES],
      logged_first_packet: false,
    })
  }

  fn encode_samples(&mut self, server: &Server, samples: &[f32]) -> Result<(), VideoError> {
    let mut cursor = 0;
    while cursor < samples.len() {
      let space = STREAM_AUDIO_FRAME_SAMPLES - self.pcm_frame.len();
      let end = (cursor + space).min(samples.len());
      self.pcm_frame.extend_from_slice(&samples[cursor..end]);
      cursor = end;
      self.flush_if_ready(server)?;
    }
    Ok(())
  }

  fn flush_if_ready(&mut self, server: &Server) -> Result<(), VideoError> {
    if self.pcm_frame.len() < STREAM_AUDIO_FRAME_SAMPLES {
      return Ok(());
    }

    let packet_len = self
      .encoder
      .encode_float(&self.pcm_frame, &mut self.opus_packet)
      .map_err(|error| VideoError::new(format!("Failed to encode macOS stream audio packet: {error}")))?;
    server
      .send_stream_audio(&self.opus_packet[..packet_len])
      .map_err(|error| VideoError::new(format!("Failed to send macOS stream audio packet: {error}")))?;
    self.pcm_frame.clear();

    if !self.logged_first_packet {
      tracing::debug!(target: "audio::encode::macos", "[audio:encode/macos] first stream audio packet sent: bytes={packet_len}");
      self.logged_first_packet = true;
    }
    Ok(())
  }
}

struct MacosNativeStreamEncoder {
  handle: NonNull<MacosStreamBridge>,
}

unsafe impl Send for MacosNativeStreamEncoder {}

struct NativeEncodedBytes {
  handle: NonNull<MacosEncodedBuffer>,
}

unsafe impl Send for NativeEncodedBytes {}

struct NativeAudioSamples {
  handle: NonNull<MacosAudioBuffer>,
}

unsafe impl Send for NativeAudioSamples {}

impl NativeEncodedBytes {
  fn new(handle: NonNull<MacosEncodedBuffer>) -> Self {
    Self { handle }
  }

  fn len(&self) -> usize {
    unsafe { parties_macos_encoded_buffer_len(self.handle.as_ptr()) }
  }

  fn is_empty(&self) -> bool {
    self.len() == 0
  }
}

impl AsRef<[u8]> for NativeEncodedBytes {
  fn as_ref(&self) -> &[u8] {
    let len = self.len();
    if len == 0 {
      return &[];
    }
    let ptr = unsafe { parties_macos_encoded_buffer_ptr(self.handle.as_ptr()) };
    if ptr.is_null() {
      &[]
    } else {
      unsafe { slice::from_raw_parts(ptr, len) }
    }
  }
}

impl Drop for NativeEncodedBytes {
  fn drop(&mut self) {
    unsafe {
      parties_macos_encoded_buffer_destroy(self.handle.as_ptr());
    }
  }
}

impl NativeAudioSamples {
  fn new(handle: NonNull<MacosAudioBuffer>) -> Self {
    Self { handle }
  }

  fn len(&self) -> usize {
    unsafe { parties_macos_audio_buffer_len(self.handle.as_ptr()) }
  }

  fn is_empty(&self) -> bool {
    self.len() == 0
  }
}

impl AsRef<[f32]> for NativeAudioSamples {
  fn as_ref(&self) -> &[f32] {
    let len = self.len();
    if len == 0 {
      return &[];
    }
    let ptr = unsafe { parties_macos_audio_buffer_ptr(self.handle.as_ptr()) };
    if ptr.is_null() {
      &[]
    } else {
      unsafe { slice::from_raw_parts(ptr, len) }
    }
  }
}

impl Drop for NativeAudioSamples {
  fn drop(&mut self) {
    unsafe {
      parties_macos_audio_buffer_destroy(self.handle.as_ptr());
    }
  }
}

impl MacosNativeStreamEncoder {
  fn new(config: &VideoBroadcastConfig) -> Result<Self, VideoError> {
    if config.source_kind == ScreenShareSourceKind::Webcam {
      return Self::new_camera(config);
    }

    let source_kind = match config.source_kind {
      ScreenShareSourceKind::Screen => 0,
      ScreenShareSourceKind::Window => 1,
      ScreenShareSourceKind::Webcam => {
        return Err(VideoError::new("Webcam is not a ScreenCaptureKit desktop source."));
      }
    };
    let handle = unsafe {
      parties_macos_stream_create(
        source_kind,
        u64::from(config.source_id),
        config.codec as u8,
        config.output_width,
        config.output_height,
        config.fps.max(1),
        config.bitrate_kbps.saturating_mul(1000),
        i32::from(config.audio_enabled),
      )
    };
    let handle = NonNull::new(handle).ok_or_else(|| {
      let native_error = macos_stream_last_error();
      VideoError::new(format!(
        "ScreenCaptureKit + VideoToolbox failed for source {:?}/{} at {}x{}: {}.",
        config.source_kind, config.source_id, config.output_width, config.output_height, native_error
      ))
    })?;
    Ok(Self { handle })
  }

  fn new_camera(config: &VideoBroadcastConfig) -> Result<Self, VideoError> {
    let handle = unsafe {
      parties_macos_camera_stream_create(
        u64::from(config.source_id),
        config.codec as u8,
        config.output_width,
        config.output_height,
        config.fps.max(1),
        config.bitrate_kbps.saturating_mul(1000),
      )
    };
    let handle = NonNull::new(handle).ok_or_else(|| {
      let native_error = macos_stream_last_error();
      VideoError::new(format!(
        "AVFoundation + VideoToolbox failed for webcam source {} at {}x{}: {}.",
        config.source_id, config.output_width, config.output_height, native_error
      ))
    })?;
    Ok(Self { handle })
  }

  fn poll(&mut self) -> Result<Vec<EncodedSample>, VideoError> {
    let result = unsafe { parties_macos_stream_poll(self.handle.as_ptr()) };
    if result < 0 {
      return Err(VideoError::new(
        "ScreenCaptureKit + VideoToolbox failed while polling encoded frames.",
      ));
    }
    if result == 0 {
      return Ok(Vec::new());
    }

    let buffer = unsafe { parties_macos_stream_take_encoded(self.handle.as_ptr()) };
    let Some(buffer) = NonNull::new(buffer) else {
      return Ok(Vec::new());
    };

    let keyframe = unsafe { parties_macos_encoded_buffer_keyframe(buffer.as_ptr()) != 0 };
    let owner = NativeEncodedBytes::new(buffer);
    if owner.is_empty() {
      return Ok(Vec::new());
    }
    let bytes = Bytes::from_owner(owner);
    Ok(vec![EncodedSample { bytes, keyframe }])
  }

  fn poll_audio(&mut self) -> Result<Option<NativeAudioSamples>, VideoError> {
    let result = unsafe { parties_macos_stream_audio_poll(self.handle.as_ptr()) };
    if result < 0 {
      return Err(VideoError::new(
        "ScreenCaptureKit failed while polling captured stream audio.",
      ));
    }
    if result == 0 {
      return Ok(None);
    }

    let buffer = unsafe { parties_macos_stream_take_audio(self.handle.as_ptr()) };
    let Some(buffer) = NonNull::new(buffer) else {
      return Ok(None);
    };
    let samples = NativeAudioSamples::new(buffer);
    if samples.is_empty() {
      return Ok(None);
    }
    Ok(Some(samples))
  }

  fn force_keyframe(&mut self) {
    unsafe {
      parties_macos_stream_force_keyframe(self.handle.as_ptr());
    }
  }
}

fn macos_stream_last_error() -> String {
  let ptr = unsafe { parties_macos_stream_last_error() };
  if ptr.is_null() {
    return "unknown native error".to_owned();
  }
  let value = unsafe { CStr::from_ptr(ptr) }.to_string_lossy().trim().to_owned();
  if value.is_empty() {
    "unknown native error".to_owned()
  } else {
    value
  }
}

impl Drop for MacosNativeStreamEncoder {
  fn drop(&mut self) {
    unsafe {
      parties_macos_stream_destroy(self.handle.as_ptr());
    }
  }
}

struct CaptureSource {
  kind: CaptureSourceKind,
}

enum CaptureSourceKind {
  Desktop(DesktopCaptureSource),
}

impl CaptureSource {
  fn open(config: &VideoBroadcastConfig) -> Result<Self, VideoError> {
    let kind = match config.source_kind {
      ScreenShareSourceKind::Screen | ScreenShareSourceKind::Window => {
        CaptureSourceKind::Desktop(find_desktop_source(config.source_kind, config.source_id)?)
      }
      ScreenShareSourceKind::Webcam => {
        return Err(VideoError::new(
          "Webcam capture on macOS must use native AVFoundation; no software fallback is available.",
        ));
      }
    };
    Ok(Self { kind })
  }

  fn capture_rgba(&mut self, width: u16, height: u16) -> Result<Vec<u8>, VideoError> {
    let frame = match &mut self.kind {
      CaptureSourceKind::Desktop(source) => source.capture_frame().map_err(|error| {
        VideoError::new(format!(
          "Failed to capture desktop frame: {error}. On macOS, check System Settings -> Privacy & Security -> Screen & System Audio Recording."
        ))
      })?,
    };

    normalize_rgba_frame(frame.rgba, frame.width, frame.height, width, height)
  }
}

fn find_desktop_source(kind: ScreenShareSourceKind, source_id: u32) -> Result<DesktopCaptureSource, VideoError> {
  DesktopCaptureSource::find(desktop_capture_source_kind(kind)?, source_id)
    .map_err(|error| VideoError::new(format!("Selected desktop source is no longer available: {error}")))
}

fn desktop_capture_source_kind(kind: ScreenShareSourceKind) -> Result<DesktopCaptureSourceKind, VideoError> {
  match kind {
    ScreenShareSourceKind::Screen => Ok(DesktopCaptureSourceKind::Screen),
    ScreenShareSourceKind::Window => Ok(DesktopCaptureSourceKind::Window),
    ScreenShareSourceKind::Webcam => Err(VideoError::new("Webcam is not a desktop capture source.")),
  }
}

struct VTEncoder {
  session: VTCompressionSessionRef,
  output_rx: mpsc::Receiver<EncodedCallbackSample>,
  _callback_tx: Box<mpsc::Sender<EncodedCallbackSample>>,
  width: u16,
  height: u16,
  codec: VideoCodecId,
  frame_duration_100ns: i64,
}

unsafe impl Send for VTEncoder {}

struct EncodedCallbackSample {
  status: OSStatus,
  sample_buffer: CMSampleBufferRef,
}

unsafe impl Send for EncodedCallbackSample {}

struct EncodedSample {
  bytes: Bytes,
  keyframe: bool,
}

impl VTEncoder {
  fn new(config: &VideoBroadcastConfig) -> Result<Self, VideoError> {
    let attributes = PixelBufferAttributes::native_nv12()?;
    let (output_tx, output_rx) = mpsc::channel();
    let mut callback_tx = Box::new(output_tx);
    let mut session = ptr::null_mut();
    let status = unsafe {
      VTCompressionSessionCreate(
        kCFAllocatorDefault,
        i32::from(config.output_width),
        i32::from(config.output_height),
        compression_codec_type(config.codec)?,
        ptr::null(),
        attributes.ptr,
        kCFAllocatorDefault,
        compression_output_callback,
        (&mut *callback_tx as *mut mpsc::Sender<EncodedCallbackSample>).cast(),
        &mut session,
      )
    };
    if status != NO_ERR || session.is_null() {
      return Err(VideoError::new(format!(
        "Failed to create VideoToolbox encoder session for {:?}: OSStatus {status}.",
        config.codec
      )));
    }

    set_vt_property_bool(session, "RealTime", true)?;
    set_vt_property_i32(
      session,
      "AverageBitRate",
      config.bitrate_kbps.saturating_mul(1000) as i32,
    )?;
    set_vt_property_i32(session, "ExpectedFrameRate", config.fps.max(1) as i32)?;
    set_vt_property_i32(
      session,
      "MaxKeyFrameInterval",
      config.fps.max(1).saturating_mul(MAX_KEYFRAME_INTERVAL_SECONDS) as i32,
    )?;
    if config.codec == VideoCodecId::H264 {
      set_vt_property_string(session, "ProfileLevel", "H264_Baseline_AutoLevel")?;
    } else if config.codec == VideoCodecId::H265 {
      set_vt_property_string(session, "ProfileLevel", "HEVC_Main_AutoLevel")?;
    }

    let status = unsafe { VTCompressionSessionPrepareToEncodeFrames(session) };
    if status != NO_ERR {
      unsafe {
        VTCompressionSessionInvalidate(session);
        CFRelease(session.cast());
      }
      return Err(VideoError::new(format!(
        "Failed to prepare VideoToolbox encoder session: OSStatus {status}."
      )));
    }

    Ok(Self {
      session,
      output_rx,
      _callback_tx: callback_tx,
      width: config.output_width,
      height: config.output_height,
      codec: config.codec,
      frame_duration_100ns: ENCODE_FRAME_DURATION_100NS / i64::from(config.fps.max(1)),
    })
  }

  fn encode(
    &mut self,
    rgba: &[u8],
    timestamp_100ns: i64,
    force_keyframe: bool,
  ) -> Result<Vec<EncodedSample>, VideoError> {
    while let Ok(sample) = self.output_rx.try_recv() {
      unsafe {
        CFRelease(sample.sample_buffer.cast());
      }
    }

    let pixel_buffer = RentedPixelBuffer::from_rgba(rgba, self.width, self.height)?;
    let frame_properties = if force_keyframe {
      Some(ForceKeyFrameProperties::new()?)
    } else {
      None
    };
    let mut info_flags = 0;
    let status = unsafe {
      VTCompressionSessionEncodeFrame(
        self.session,
        pixel_buffer.ptr,
        cm_time_100ns(timestamp_100ns),
        cm_time_100ns(self.frame_duration_100ns),
        frame_properties
          .as_ref()
          .map(|properties| properties.ptr)
          .unwrap_or(ptr::null()),
        ptr::null_mut(),
        &mut info_flags,
      )
    };
    if status != NO_ERR {
      return Err(VideoError::new(format!(
        "VideoToolbox failed to encode frame: OSStatus {status}."
      )));
    }

    let status = unsafe { VTCompressionSessionCompleteFrames(self.session, kCMTimeInvalid) };
    if status != NO_ERR {
      return Err(VideoError::new(format!(
        "VideoToolbox failed to complete encoded frames: OSStatus {status}."
      )));
    }

    let mut out = Vec::new();
    while let Ok(sample) = self.output_rx.try_recv() {
      let converted = encoded_sample_from_callback(self.codec, &sample)?;
      unsafe {
        CFRelease(sample.sample_buffer.cast());
      }
      if let Some(converted) = converted {
        out.push(converted);
      }
    }
    Ok(out)
  }
}

impl Drop for VTEncoder {
  fn drop(&mut self) {
    unsafe {
      VTCompressionSessionInvalidate(self.session);
      CFRelease(self.session.cast());
    }
  }
}

extern "C" fn compression_output_callback(
  output_callback_ref_con: *mut c_void,
  _source_frame_ref_con: *mut c_void,
  status: OSStatus,
  _info_flags: VTEncodeInfoFlags,
  sample_buffer: CMSampleBufferRef,
) {
  if output_callback_ref_con.is_null() || sample_buffer.is_null() {
    return;
  }
  unsafe {
    CFRetain(sample_buffer.cast());
    let tx = &*(output_callback_ref_con.cast::<mpsc::Sender<EncodedCallbackSample>>());
    let _ = tx.send(EncodedCallbackSample { status, sample_buffer });
  }
}

unsafe extern "C" {
  fn CFRetain(cf: CFTypeRef) -> CFTypeRef;
}

struct RentedPixelBuffer {
  ptr: CVPixelBufferRef,
}

impl RentedPixelBuffer {
  fn from_rgba(rgba: &[u8], width: u16, height: u16) -> Result<Self, VideoError> {
    let expected_len = usize::from(width) * usize::from(height) * 4;
    if rgba.len() != expected_len {
      return Err(VideoError::new(format!(
        "Invalid RGBA frame length: got {} bytes, expected {}.",
        rgba.len(),
        expected_len
      )));
    }

    let attributes = PixelBufferAttributes::native_nv12()?;
    let mut pixel_buffer = ptr::null_mut();
    let status = unsafe {
      CVPixelBufferCreate(
        kCFAllocatorDefault,
        usize::from(width),
        usize::from(height),
        kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange,
        attributes.ptr,
        &mut pixel_buffer,
      )
    };
    if status != NO_ERR || pixel_buffer.is_null() {
      return Err(VideoError::new(format!(
        "Failed to create encoder pixel buffer: OSStatus {status}."
      )));
    }

    let lock_status = unsafe { CVPixelBufferLockBaseAddress(pixel_buffer, 0) };
    if lock_status != NO_ERR {
      unsafe {
        CFRelease(pixel_buffer.cast());
      }
      return Err(VideoError::new(format!(
        "Failed to lock encoder pixel buffer: OSStatus {lock_status}."
      )));
    }

    let result = write_rgba_to_nv12_pixel_buffer(pixel_buffer, rgba, width, height);
    let unlock_status = unsafe { CVPixelBufferUnlockBaseAddress(pixel_buffer, 0) };
    if let Err(error) = result {
      unsafe {
        CFRelease(pixel_buffer.cast());
      }
      return Err(error);
    }
    if unlock_status != NO_ERR {
      unsafe {
        CFRelease(pixel_buffer.cast());
      }
      return Err(VideoError::new(format!(
        "Failed to unlock encoder pixel buffer: OSStatus {unlock_status}."
      )));
    }

    Ok(Self { ptr: pixel_buffer })
  }
}

impl Drop for RentedPixelBuffer {
  fn drop(&mut self) {
    unsafe {
      CFRelease(self.ptr.cast());
    }
  }
}

struct ForceKeyFrameProperties {
  ptr: CFDictionaryRef,
  key: CFStringRef,
}

impl ForceKeyFrameProperties {
  fn new() -> Result<Self, VideoError> {
    let key = cf_string("ForceKeyFrame")?;
    let keys = [key.cast::<c_void>()];
    let values = [unsafe { kCFBooleanTrue }.cast::<c_void>()];
    let dictionary = unsafe {
      CFDictionaryCreate(
        kCFAllocatorDefault,
        keys.as_ptr(),
        values.as_ptr(),
        1,
        &kCFTypeDictionaryKeyCallBacks,
        &kCFTypeDictionaryValueCallBacks,
      )
    };
    if dictionary.is_null() {
      unsafe {
        CFRelease(key.cast());
      }
      return Err(VideoError::new("Failed to create VideoToolbox keyframe properties."));
    }
    Ok(Self { ptr: dictionary, key })
  }
}

impl Drop for ForceKeyFrameProperties {
  fn drop(&mut self) {
    unsafe {
      CFRelease(self.ptr.cast());
      CFRelease(self.key.cast());
    }
  }
}

fn encoded_sample_from_callback(
  codec: VideoCodecId,
  sample: &EncodedCallbackSample,
) -> Result<Option<EncodedSample>, VideoError> {
  if sample.status != NO_ERR {
    return Err(VideoError::new(format!(
      "VideoToolbox encoder callback failed: OSStatus {}.",
      sample.status
    )));
  }
  let ready = unsafe { CMSampleBufferDataIsReady(sample.sample_buffer) };
  if ready == 0 {
    return Ok(None);
  }
  let block = unsafe { CMSampleBufferGetDataBuffer(sample.sample_buffer) };
  if block.is_null() {
    return Ok(None);
  }
  let len = unsafe { CMBlockBufferGetDataLength(block) };
  if len == 0 {
    return Ok(None);
  }

  let mut bytes = vec![0u8; len];
  let status = unsafe { CMBlockBufferCopyDataBytes(block, 0, len, bytes.as_mut_ptr().cast()) };
  if status != NO_ERR {
    return Err(VideoError::new(format!(
      "Failed to copy VideoToolbox encoded sample: OSStatus {status}."
    )));
  }

  let keyframe = sample_is_keyframe(sample.sample_buffer);
  if codec == VideoCodecId::Av1 {
    return Ok(Some(EncodedSample {
      bytes: bytes.into(),
      keyframe,
    }));
  }

  if keyframe {
    let format_description = unsafe { CMSampleBufferGetFormatDescription(sample.sample_buffer) };
    let mut prefixed = parameter_sets_annex_b(codec, format_description)?;
    prefixed.extend(length_prefixed_sample_to_annex_b(&bytes)?);
    bytes = prefixed;
  } else {
    bytes = length_prefixed_sample_to_annex_b(&bytes)?;
  }

  Ok(Some(EncodedSample {
    bytes: bytes.into(),
    keyframe,
  }))
}

fn sample_is_keyframe(sample_buffer: CMSampleBufferRef) -> bool {
  let attachments = unsafe { CMSampleBufferGetSampleAttachmentsArray(sample_buffer, 0) };
  if attachments.is_null() {
    return true;
  }
  let attachment = unsafe { CFArrayGetValueAtIndex(attachments, 0) };
  if attachment.is_null() {
    return true;
  }
  let not_sync = unsafe { CFDictionaryGetValue(attachment.cast(), kCMSampleAttachmentKey_NotSync.cast()) };
  not_sync.is_null()
}

fn parameter_sets_annex_b(
  codec: VideoCodecId,
  format_description: CMVideoFormatDescriptionRef,
) -> Result<Vec<u8>, VideoError> {
  if format_description.is_null() {
    return Ok(Vec::new());
  }
  let mut out = Vec::new();
  match codec {
    VideoCodecId::H264 => {
      append_h264_parameter_set(format_description, 0, &mut out)?;
      append_h264_parameter_set(format_description, 1, &mut out)?;
    }
    VideoCodecId::H265 => {
      append_h265_parameter_set(format_description, 0, &mut out)?;
      append_h265_parameter_set(format_description, 1, &mut out)?;
      append_h265_parameter_set(format_description, 2, &mut out)?;
    }
    VideoCodecId::Av1 | VideoCodecId::Unknown => {}
  }
  Ok(out)
}

fn append_h264_parameter_set(
  format_description: CMVideoFormatDescriptionRef,
  index: usize,
  out: &mut Vec<u8>,
) -> Result<(), VideoError> {
  let mut ptr = ptr::null();
  let mut len = 0usize;
  let mut count = 0usize;
  let mut nal_header_len = 0;
  let status = unsafe {
    CMVideoFormatDescriptionGetH264ParameterSetAtIndex(
      format_description,
      index,
      &mut ptr,
      &mut len,
      &mut count,
      &mut nal_header_len,
    )
  };
  if status != NO_ERR || ptr.is_null() || len == 0 {
    return Err(VideoError::new(format!(
      "Failed to read H.264 parameter set {index}: OSStatus {status}."
    )));
  }
  out.extend_from_slice(&[0, 0, 0, 1]);
  out.extend_from_slice(unsafe { slice::from_raw_parts(ptr, len) });
  Ok(())
}

fn append_h265_parameter_set(
  format_description: CMVideoFormatDescriptionRef,
  index: usize,
  out: &mut Vec<u8>,
) -> Result<(), VideoError> {
  let mut ptr = ptr::null();
  let mut len = 0usize;
  let mut count = 0usize;
  let mut nal_header_len = 0;
  let status = unsafe {
    CMVideoFormatDescriptionGetHEVCParameterSetAtIndex(
      format_description,
      index,
      &mut ptr,
      &mut len,
      &mut count,
      &mut nal_header_len,
    )
  };
  if status != NO_ERR || ptr.is_null() || len == 0 {
    return Err(VideoError::new(format!(
      "Failed to read H.265 parameter set {index}: OSStatus {status}."
    )));
  }
  out.extend_from_slice(&[0, 0, 0, 1]);
  out.extend_from_slice(unsafe { slice::from_raw_parts(ptr, len) });
  Ok(())
}

fn length_prefixed_sample_to_annex_b(sample: &[u8]) -> Result<Vec<u8>, VideoError> {
  let mut out = Vec::with_capacity(sample.len() + 16);
  let mut offset = 0usize;
  while offset < sample.len() {
    if sample.len().saturating_sub(offset) < 4 {
      return Err(VideoError::new(
        "VideoToolbox encoded sample has a truncated NAL length prefix.",
      ));
    }
    let len = u32::from_be_bytes([
      sample[offset],
      sample[offset + 1],
      sample[offset + 2],
      sample[offset + 3],
    ]) as usize;
    offset += 4;
    if len == 0 || sample.len().saturating_sub(offset) < len {
      return Err(VideoError::new(
        "VideoToolbox encoded sample has an invalid NAL length prefix.",
      ));
    }
    out.extend_from_slice(&[0, 0, 0, 1]);
    out.extend_from_slice(&sample[offset..offset + len]);
    offset += len;
  }
  Ok(out)
}

fn write_rgba_to_nv12_pixel_buffer(
  pixel_buffer: CVPixelBufferRef,
  rgba: &[u8],
  width: u16,
  height: u16,
) -> Result<(), VideoError> {
  let width = usize::from(width);
  let height = usize::from(height);
  let y_base = unsafe { CVPixelBufferGetBaseAddressOfPlane(pixel_buffer, 0) };
  let uv_base = unsafe { CVPixelBufferGetBaseAddressOfPlane(pixel_buffer, 1) };
  if y_base.is_null() || uv_base.is_null() {
    return Err(VideoError::new("Encoder pixel buffer does not expose NV12 planes."));
  }
  let y_stride = unsafe { CVPixelBufferGetBytesPerRowOfPlane(pixel_buffer, 0) };
  let uv_stride = unsafe { CVPixelBufferGetBytesPerRowOfPlane(pixel_buffer, 1) };
  let y_plane = unsafe { slice::from_raw_parts_mut(y_base.cast::<u8>(), y_stride * height) };
  let uv_plane = unsafe { slice::from_raw_parts_mut(uv_base.cast::<u8>(), uv_stride * (height / 2)) };

  for y in 0..height {
    for x in 0..width {
      let src = (y * width + x) * 4;
      let r = rgba[src] as f32;
      let g = rgba[src + 1] as f32;
      let b = rgba[src + 2] as f32;
      y_plane[y * y_stride + x] = clamp_u8(16.0 + 0.257 * r + 0.504 * g + 0.098 * b);
    }
  }

  for y in (0..height).step_by(2) {
    for x in (0..width).step_by(2) {
      let mut u = 0.0f32;
      let mut v = 0.0f32;
      let mut samples = 0.0f32;
      for dy in 0..2 {
        for dx in 0..2 {
          let px = x + dx;
          let py = y + dy;
          if px >= width || py >= height {
            continue;
          }
          let src = (py * width + px) * 4;
          let r = rgba[src] as f32;
          let g = rgba[src + 1] as f32;
          let b = rgba[src + 2] as f32;
          u += 128.0 - 0.148 * r - 0.291 * g + 0.439 * b;
          v += 128.0 + 0.439 * r - 0.368 * g - 0.071 * b;
          samples += 1.0;
        }
      }
      let uv = (y / 2) * uv_stride + x;
      uv_plane[uv] = clamp_u8(u / samples);
      uv_plane[uv + 1] = clamp_u8(v / samples);
    }
  }

  Ok(())
}

fn clamp_u8(value: f32) -> u8 {
  value.round().clamp(0.0, 255.0) as u8
}

fn normalize_rgba_frame(
  rgba: Vec<u8>,
  frame_width: u32,
  frame_height: u32,
  output_width: u16,
  output_height: u16,
) -> Result<Vec<u8>, VideoError> {
  let output_width = u32::from(output_width);
  let output_height = u32::from(output_height);
  if frame_width == output_width && frame_height == output_height {
    return Ok(rgba);
  }

  if frame_width == 0 || frame_height == 0 || output_width == 0 || output_height == 0 {
    return Err(VideoError::new(format!(
      "Invalid captured frame dimensions: captured={}x{} output={}x{}.",
      frame_width, frame_height, output_width, output_height
    )));
  }

  let src_stride = frame_width as usize * 4;
  let dst_stride = output_width as usize * 4;
  let mut out = vec![0u8; dst_stride * output_height as usize];
  for row in 0..output_height as usize {
    let src_y = row * frame_height as usize / output_height as usize;
    let dst_start = row * dst_stride;
    for column in 0..output_width as usize {
      let src_x = column * frame_width as usize / output_width as usize;
      let src_start = src_y * src_stride + src_x * 4;
      let dst_start = dst_start + column * 4;
      out[dst_start..dst_start + 4].copy_from_slice(&rgba[src_start..src_start + 4]);
    }
  }
  Ok(out)
}

fn compression_codec_type(codec: VideoCodecId) -> Result<u32, VideoError> {
  match codec {
    VideoCodecId::H264 => Ok(K_CM_VIDEO_CODEC_TYPE_H264),
    VideoCodecId::H265 => Ok(K_CM_VIDEO_CODEC_TYPE_HEVC),
    VideoCodecId::Av1 => Ok(K_CM_VIDEO_CODEC_TYPE_AV1),
    VideoCodecId::Unknown => Err(VideoError::new("Unknown video codec cannot be encoded.")),
  }
}

fn set_vt_property_bool(session: VTCompressionSessionRef, key: &str, value: bool) -> Result<(), VideoError> {
  let key = cf_string(key)?;
  let value = if value {
    unsafe { kCFBooleanTrue }.cast()
  } else {
    unsafe { kCFBooleanFalse }.cast()
  };
  let status = unsafe { VTCompressionSessionSetProperty(session, key, value) };
  unsafe {
    CFRelease(key.cast());
  }
  if status != NO_ERR {
    return Err(VideoError::new(format!(
      "Failed to set VideoToolbox encoder property: OSStatus {status}."
    )));
  }
  Ok(())
}

fn set_vt_property_i32(session: VTCompressionSessionRef, key: &str, value: i32) -> Result<(), VideoError> {
  let key = cf_string(key)?;
  let value = cf_number_i32(value)?;
  let status = unsafe { VTCompressionSessionSetProperty(session, key, value) };
  unsafe {
    CFRelease(key.cast());
    CFRelease(value);
  }
  if status != NO_ERR {
    return Err(VideoError::new(format!(
      "Failed to set VideoToolbox encoder property: OSStatus {status}."
    )));
  }
  Ok(())
}

fn set_vt_property_string(session: VTCompressionSessionRef, key: &str, value: &str) -> Result<(), VideoError> {
  let key = cf_string(key)?;
  let value = cf_string(value)?;
  let status = unsafe { VTCompressionSessionSetProperty(session, key, value.cast()) };
  unsafe {
    CFRelease(key.cast());
    CFRelease(value.cast());
  }
  if status != NO_ERR {
    tracing::warn!(target: "video::encode::macos", "[video:encode/macos] VideoToolbox ignored encoder string property {key:?}: OSStatus {status}");
  }
  Ok(())
}

pub(super) struct VideoToolboxVideoDecoder {
  config: VideoDecodeConfig,
  session: Option<VTSession>,
  av1_videotoolbox_unavailable: bool,
  output_rx: mpsc::Receiver<DecodedCallbackFrame>,
  output_tx: mpsc::Sender<DecodedCallbackFrame>,
}

pub(super) enum NativeVideoDecoder {
  VideoToolbox(VideoToolboxVideoDecoder),
  Software(SoftwareVideoDecoder),
}

unsafe impl Send for VideoToolboxVideoDecoder {}

struct VTSession {
  session: VTDecompressionSessionRef,
  format_description: CMVideoFormatDescriptionRef,
  _callback_tx: Box<mpsc::Sender<DecodedCallbackFrame>>,
}

unsafe impl Send for VTSession {}

struct DecodedCallbackFrame {
  status: OSStatus,
  pixel_buffer: CVPixelBufferRef,
}

unsafe impl Send for DecodedCallbackFrame {}

pub(super) fn decode(config: VideoDecodeConfig) -> Result<VideoDecoder, VideoError> {
  let provider = macos_decoder_provider(&config);
  let build = provider.create(&config)?;
  log_macos_decoder_ready(&config, &build);
  Ok(VideoDecoder::from_decoder(
    Box::new(build.decoder),
    config,
    build.backend,
  ))
}

struct MacosDecoderBuild {
  decoder: NativeVideoDecoder,
  backend: NativeVideoBackend,
  ready_path: MacosDecoderReadyPath,
}

enum MacosDecoderReadyPath {
  VideoToolbox { av1_videotoolbox_unavailable: bool },
  Software,
}

trait MacosDecoderProvider {
  fn create(&self, config: &VideoDecodeConfig) -> Result<MacosDecoderBuild, VideoError>;
}

struct VideoToolboxDecoderProvider;
struct SoftwareDecoderProvider;

fn macos_decoder_provider(config: &VideoDecodeConfig) -> &'static dyn MacosDecoderProvider {
  if config.hardware_decoding {
    &VideoToolboxDecoderProvider
  } else {
    &SoftwareDecoderProvider
  }
}

impl MacosDecoderProvider for VideoToolboxDecoderProvider {
  fn create(&self, config: &VideoDecodeConfig) -> Result<MacosDecoderBuild, VideoError> {
    let decoder = VideoToolboxVideoDecoder::new(config);
    let av1_videotoolbox_unavailable = decoder.av1_videotoolbox_unavailable;
    Ok(MacosDecoderBuild {
      decoder: NativeVideoDecoder::VideoToolbox(decoder),
      backend: NativeVideoBackend::AppleVideoToolbox,
      ready_path: MacosDecoderReadyPath::VideoToolbox {
        av1_videotoolbox_unavailable,
      },
    })
  }
}

impl MacosDecoderProvider for SoftwareDecoderProvider {
  fn create(&self, config: &VideoDecodeConfig) -> Result<MacosDecoderBuild, VideoError> {
    let decoder = SoftwareVideoDecoder::new(config)?;
    let backend = decoder.backend();
    Ok(MacosDecoderBuild {
      decoder: NativeVideoDecoder::Software(decoder),
      backend,
      ready_path: MacosDecoderReadyPath::Software,
    })
  }
}

impl VideoToolboxVideoDecoder {
  fn new(config: &VideoDecodeConfig) -> Self {
    let (output_tx, output_rx) = mpsc::channel();
    Self {
      config: config.clone(),
      session: None,
      av1_videotoolbox_unavailable: false,
      output_rx,
      output_tx,
    }
  }
}

fn log_macos_decoder_ready(config: &VideoDecodeConfig, build: &MacosDecoderBuild) {
  match build.ready_path {
    MacosDecoderReadyPath::VideoToolbox {
      av1_videotoolbox_unavailable,
    } => {
      tracing::warn!(target: "video::decode::macos",
        "[video:decode/macos] decoder ready through VideoToolbox: codec={:?} size={}x{} av1_videotoolbox_unavailable={}",
        config.codec,
        config.width,
        config.height,
        av1_videotoolbox_unavailable
      );
    }
    MacosDecoderReadyPath::Software => {
      tracing::info!(target: "video::decode::macos",
        "[video:decode/macos] decoder ready through software: backend={:?} codec={:?} size={}x{}",
        build.backend,
        config.codec,
        config.width,
        config.height
      );
    }
  }
}

impl VideoFrameDecoder for NativeVideoDecoder {
  fn decode_frame(
    &mut self,
    frame: &VideoFrame,
    output: bool,
    output_buffer: Option<Vec<u8>>,
  ) -> Result<Option<NativeDecodedVideoFrame>, VideoError> {
    match self {
      Self::VideoToolbox(decoder) => decoder.decode_frame(frame, output, output_buffer),
      Self::Software(decoder) => decoder.decode_frame(frame, output, output_buffer),
    }
  }
}

impl VideoFrameDecoder for VideoToolboxVideoDecoder {
  fn decode_frame(
    &mut self,
    frame: &VideoFrame,
    output: bool,
    _output_buffer: Option<Vec<u8>>,
  ) -> Result<Option<NativeDecodedVideoFrame>, VideoError> {
    if frame.codec == VideoCodecId::Av1 && simulate_unsupported_av1() {
      return Err(unsupported_av1_error());
    }

    if frame.codec == VideoCodecId::Av1 && self.av1_videotoolbox_unavailable {
      return Err(unsupported_av1_error());
    }

    let length_prefixed_h26x =
      matches!(frame.codec, VideoCodecId::H264 | VideoCodecId::H265) && !looks_like_annex_b(&frame.encoded);
    let can_use_encoded_sample_directly = length_prefixed_h26x && self.session.is_some();
    let access_units = if can_use_encoded_sample_directly {
      None
    } else {
      Some(AccessUnits::parse(frame.codec, frame.encoded.clone())?)
    };

    if let Some(access_units) = &access_units
      && frame.keyframe
      && self.session.is_none()
      && !access_units.can_initialize_session(frame.codec)
    {
      tracing::info!(target: "video::decode::macos",
        "[video:decode/macos] keyframe missing VideoToolbox parameter sets: codec={:?} {}",
        frame.codec,
        access_units.parameter_set_summary()
      );
    }

    let should_initialize_session = match (frame.codec, access_units.as_ref()) {
      (VideoCodecId::Av1, _) => self.session.is_none(),
      (VideoCodecId::H264 | VideoCodecId::H265, Some(access_units)) => {
        self.session.is_none() && access_units.can_initialize_session(frame.codec)
      }
      (VideoCodecId::H264 | VideoCodecId::H265, None) | (VideoCodecId::Unknown, _) => false,
    };

    if should_initialize_session {
      if let Some(session) = self.session.take() {
        drop(session);
      }
      let access_units = access_units
        .as_ref()
        .ok_or_else(|| VideoError::new("VideoToolbox decoder initialization requires parsed access units."))?;
      tracing::info!(target: "video::decode::macos",
        "[video:decode/macos] initializing VideoToolbox session from parameter sets: codec={:?} {}",
        frame.codec,
        access_units.parameter_set_summary()
      );
      match VTSession::new(&self.config, &access_units, self.output_tx.clone()) {
        Ok(session) => self.session = Some(session),
        Err(error) if frame.codec == VideoCodecId::Av1 => {
          self.av1_videotoolbox_unavailable = true;
          tracing::warn!(target: "video::decode::macos", "[video:decode/macos] VideoToolbox AV1 unavailable; refusing software decode: {error}");
          return Err(unsupported_av1_error());
        }
        Err(error) => return Err(error),
      }
    }

    let Some(session) = self.session.as_mut() else {
      return Ok(None);
    };
    let sample_data = match access_units.as_ref() {
      Some(access_units) => access_units.sample_data(frame.codec)?,
      None => frame.encoded.clone(),
    };
    let sample = SampleBuffer::new(sample_data, session.format_description, frame.timestamp)?;
    while let Ok(stale) = self.output_rx.try_recv() {
      if !stale.pixel_buffer.is_null() {
        unsafe {
          CFRelease(stale.pixel_buffer.cast());
        }
      }
    }

    let mut info_flags = 0;
    let status =
      unsafe { VTDecompressionSessionDecodeFrame(session.session, sample.ptr, 0, ptr::null_mut(), &mut info_flags) };
    if status != NO_ERR {
      return Err(VideoError::new(format!(
        "VideoToolbox failed to decode {} frame {}: OSStatus {status}.",
        codec_label(frame.codec),
        frame.frame_number
      )));
    }

    let mut latest: CVPixelBufferRef = ptr::null_mut();
    while let Ok(decoded) = self.output_rx.try_recv() {
      if decoded.status != NO_ERR {
        if !decoded.pixel_buffer.is_null() {
          unsafe {
            CFRelease(decoded.pixel_buffer.cast());
          }
        }
        return Err(VideoError::new(format!(
          "VideoToolbox output callback failed for {} frame {}: OSStatus {}.",
          codec_label(frame.codec),
          frame.frame_number,
          decoded.status
        )));
      }
      if !latest.is_null() {
        unsafe {
          CFRelease(latest.cast());
        }
      }
      latest = decoded.pixel_buffer;
    }

    if !output {
      if !latest.is_null() {
        unsafe {
          CFRelease(latest.cast());
        }
      }
      return Ok(None);
    }

    if !latest.is_null() {
      let native_image = native_image_from_pixel_buffer(latest).ok();
      let pixels = if native_image.is_some() {
        Vec::new()
      } else {
        copy_nv12_pixel_buffer(latest).unwrap_or_default()
      };
      unsafe {
        CFRelease(latest.cast());
      }
      match native_image {
        Some(native_image) => Ok(Some(NativeDecodedVideoFrame {
          format: DecodedVideoPixelFormat::Nv12,
          pixels,
          native_image: Some(native_image),
        })),
        None if !pixels.is_empty() => Ok(Some(NativeDecodedVideoFrame {
          format: DecodedVideoPixelFormat::Nv12,
          pixels,
          native_image: None,
        })),
        None => Ok(None),
      }
    } else {
      Ok(None)
    }
  }
}

fn simulate_unsupported_av1() -> bool {
  std::env::var_os(SIMULATE_UNSUPPORTED_AV1_ENV).is_some_and(|value| value == "1" || value == "true")
}

fn cpu_video_fallback_enabled() -> bool {
  std::env::var_os(ALLOW_CPU_VIDEO_FALLBACK_ENV).is_some_and(|value| value == "1" || value == "true")
}

fn unsupported_av1_error() -> VideoError {
  VideoError::new(
    "macOS VideoToolbox AV1 is unavailable and software AV1 decode is disabled to avoid excessive CPU usage. Use H.265/H.264 or a Mac with hardware AV1 decode.",
  )
}

impl VTSession {
  fn new(
    config: &VideoDecodeConfig,
    access_units: &AccessUnits,
    output_tx: mpsc::Sender<DecodedCallbackFrame>,
  ) -> Result<Self, VideoError> {
    let format_description = create_format_description(config, access_units)?;
    let attributes = PixelBufferAttributes::native_nv12()?;
    let callback_tx = Box::new(output_tx);
    let callback = VTDecompressionOutputCallbackRecord {
      decompression_output_callback,
      decompression_output_ref_con: (&*callback_tx as *const mpsc::Sender<DecodedCallbackFrame>)
        .cast_mut()
        .cast(),
    };
    let mut session = ptr::null_mut();
    let status = unsafe {
      VTDecompressionSessionCreate(
        kCFAllocatorDefault,
        format_description,
        ptr::null(),
        attributes.ptr,
        &callback,
        &mut session,
      )
    };
    if status != NO_ERR || session.is_null() {
      unsafe {
        CFRelease(format_description.cast());
      }
      return Err(VideoError::new(format!(
        "Failed to create VideoToolbox decoder session for {}: OSStatus {status}.",
        codec_label(config.codec)
      )));
    }

    Ok(Self {
      session,
      format_description,
      _callback_tx: callback_tx,
    })
  }
}

impl Drop for VTSession {
  fn drop(&mut self) {
    unsafe {
      VTDecompressionSessionInvalidate(self.session);
      CFRelease(self.session.cast());
      CFRelease(self.format_description.cast());
    }
  }
}

extern "C" fn decompression_output_callback(
  decompression_output_ref_con: *mut c_void,
  _source_frame_ref_con: *mut c_void,
  status: OSStatus,
  _info_flags: VTDecodeInfoFlags,
  image_buffer: CVPixelBufferRef,
  _presentation_time_stamp: CMTime,
  _presentation_duration: CMTime,
) {
  let sender = unsafe { &*(decompression_output_ref_con.cast::<mpsc::Sender<DecodedCallbackFrame>>()) };
  let pixel_buffer = if status == NO_ERR && !image_buffer.is_null() {
    unsafe {
      CFRetain(image_buffer.cast());
    }
    image_buffer
  } else {
    ptr::null_mut()
  };
  let _ = sender.send(DecodedCallbackFrame { status, pixel_buffer });
}

fn create_format_description(
  config: &VideoDecodeConfig,
  access_units: &AccessUnits,
) -> Result<CMVideoFormatDescriptionRef, VideoError> {
  let codec = config.codec;
  let mut format_description = ptr::null_mut();
  let status = match codec {
    VideoCodecId::H264 => {
      let sps = access_units
        .h264_sps
        .as_deref()
        .ok_or_else(|| VideoError::new("H.264 keyframe is missing SPS parameter set."))?;
      let pps = access_units
        .h264_pps
        .as_deref()
        .ok_or_else(|| VideoError::new("H.264 keyframe is missing PPS parameter set."))?;
      let pointers = [sps.as_ptr(), pps.as_ptr()];
      let sizes = [sps.len(), pps.len()];
      unsafe {
        CMVideoFormatDescriptionCreateFromH264ParameterSets(
          kCFAllocatorDefault,
          pointers.len(),
          pointers.as_ptr(),
          sizes.as_ptr(),
          4,
          &mut format_description,
        )
      }
    }
    VideoCodecId::H265 => {
      let vps = access_units
        .h265_vps
        .as_deref()
        .ok_or_else(|| VideoError::new("H.265 keyframe is missing VPS parameter set."))?;
      let sps = access_units
        .h265_sps
        .as_deref()
        .ok_or_else(|| VideoError::new("H.265 keyframe is missing SPS parameter set."))?;
      let pps = access_units
        .h265_pps
        .as_deref()
        .ok_or_else(|| VideoError::new("H.265 keyframe is missing PPS parameter set."))?;
      let pointers = [vps.as_ptr(), sps.as_ptr(), pps.as_ptr()];
      let sizes = [vps.len(), sps.len(), pps.len()];
      unsafe {
        CMVideoFormatDescriptionCreateFromHEVCParameterSets(
          kCFAllocatorDefault,
          pointers.len(),
          pointers.as_ptr(),
          sizes.as_ptr(),
          4,
          ptr::null(),
          &mut format_description,
        )
      }
    }
    VideoCodecId::Av1 => {
      let sequence_header = access_units
        .av1_sequence_header
        .as_deref()
        .ok_or_else(|| VideoError::new("AV1 keyframe is missing sequence-header OBU."))?;
      let extensions = Av1FormatExtensions::new(sequence_header)?;
      unsafe {
        CMVideoFormatDescriptionCreate(
          kCFAllocatorDefault,
          K_CM_VIDEO_CODEC_TYPE_AV1,
          i32::from(config.width),
          i32::from(config.height),
          extensions.ptr,
          &mut format_description,
        )
      }
    }
    VideoCodecId::Unknown => return Err(VideoError::new("Unsupported macOS video codec.")),
  };

  if status != NO_ERR || format_description.is_null() {
    return Err(VideoError::new(format!(
      "Failed to create {} VideoToolbox format description: OSStatus {status}.",
      codec_label(codec)
    )));
  }

  Ok(format_description)
}

struct Av1FormatExtensions {
  ptr: CFDictionaryRef,
}

impl Av1FormatExtensions {
  fn new(sequence_header_obu: &[u8]) -> Result<Self, VideoError> {
    let av1c = build_av1c(sequence_header_obu)?;
    let sample_description_atoms_key = cf_string("SampleDescriptionExtensionAtoms")?;
    let av1c_key = cf_string("av1C")?;
    let av1c_data = unsafe { CFDataCreate(kCFAllocatorDefault, av1c.as_ptr(), av1c.len() as isize) };
    if av1c_data.is_null() {
      unsafe {
        CFRelease(sample_description_atoms_key.cast());
        CFRelease(av1c_key.cast());
      }
      return Err(VideoError::new("Failed to create AV1 VideoToolbox av1C data."));
    }

    let atoms_keys = [av1c_key.cast::<c_void>()];
    let atoms_values = [av1c_data.cast::<c_void>()];
    let atoms = unsafe {
      CFDictionaryCreate(
        kCFAllocatorDefault,
        atoms_keys.as_ptr(),
        atoms_values.as_ptr(),
        atoms_keys.len() as isize,
        &kCFTypeDictionaryKeyCallBacks,
        &kCFTypeDictionaryValueCallBacks,
      )
    };
    if atoms.is_null() {
      unsafe {
        CFRelease(sample_description_atoms_key.cast());
        CFRelease(av1c_key.cast());
        CFRelease(av1c_data.cast());
      }
      return Err(VideoError::new("Failed to create AV1 VideoToolbox extension atoms."));
    }

    let format_name_key = cf_string("FormatName")?;
    let format_name = cf_string("av01")?;
    let depth_key = cf_string("Depth")?;
    let depth = cf_number_i32(24)?;
    let bits_per_component_key = cf_string("BitsPerComponent")?;
    let bits_per_component = cf_number_i32(8)?;
    let color_primaries_key = cf_string("CVImageBufferColorPrimaries")?;
    let color_primaries = cf_string("ITU_R_709_2")?;
    let transfer_key = cf_string("CVImageBufferTransferFunction")?;
    let transfer = cf_string("ITU_R_709_2")?;
    let matrix_key = cf_string("CVImageBufferYCbCrMatrix")?;
    let matrix = cf_string("ITU_R_709_2")?;
    let full_range_key = cf_string("FullRangeVideo")?;

    let keys = [
      sample_description_atoms_key.cast::<c_void>(),
      format_name_key.cast::<c_void>(),
      depth_key.cast::<c_void>(),
      bits_per_component_key.cast::<c_void>(),
      color_primaries_key.cast::<c_void>(),
      transfer_key.cast::<c_void>(),
      matrix_key.cast::<c_void>(),
      full_range_key.cast::<c_void>(),
    ];
    let values = [
      atoms.cast::<c_void>(),
      format_name.cast::<c_void>(),
      depth.cast::<c_void>(),
      bits_per_component.cast::<c_void>(),
      color_primaries.cast::<c_void>(),
      transfer.cast::<c_void>(),
      matrix.cast::<c_void>(),
      unsafe { kCFBooleanFalse }.cast::<c_void>(),
    ];
    let dictionary = unsafe {
      CFDictionaryCreate(
        kCFAllocatorDefault,
        keys.as_ptr(),
        values.as_ptr(),
        keys.len() as isize,
        &kCFTypeDictionaryKeyCallBacks,
        &kCFTypeDictionaryValueCallBacks,
      )
    };
    if dictionary.is_null() {
      unsafe {
        CFRelease(sample_description_atoms_key.cast());
        CFRelease(av1c_key.cast());
        CFRelease(av1c_data.cast());
        CFRelease(atoms.cast());
        CFRelease(format_name_key.cast());
        CFRelease(format_name.cast());
        CFRelease(depth_key.cast());
        CFRelease(depth.cast());
        CFRelease(bits_per_component_key.cast());
        CFRelease(bits_per_component.cast());
        CFRelease(color_primaries_key.cast());
        CFRelease(color_primaries.cast());
        CFRelease(transfer_key.cast());
        CFRelease(transfer.cast());
        CFRelease(matrix_key.cast());
        CFRelease(matrix.cast());
        CFRelease(full_range_key.cast());
      }
      return Err(VideoError::new("Failed to create AV1 VideoToolbox format extensions."));
    }

    unsafe {
      CFRelease(sample_description_atoms_key.cast());
      CFRelease(av1c_key.cast());
      CFRelease(av1c_data.cast());
      CFRelease(atoms.cast());
      CFRelease(format_name_key.cast());
      CFRelease(format_name.cast());
      CFRelease(depth_key.cast());
      CFRelease(depth.cast());
      CFRelease(bits_per_component_key.cast());
      CFRelease(bits_per_component.cast());
      CFRelease(color_primaries_key.cast());
      CFRelease(color_primaries.cast());
      CFRelease(transfer_key.cast());
      CFRelease(transfer.cast());
      CFRelease(matrix_key.cast());
      CFRelease(matrix.cast());
      CFRelease(full_range_key.cast());
    }

    Ok(Self { ptr: dictionary })
  }
}

impl Drop for Av1FormatExtensions {
  fn drop(&mut self) {
    unsafe {
      CFRelease(self.ptr.cast());
    }
  }
}

fn cf_string(value: &str) -> Result<CFStringRef, VideoError> {
  let string = unsafe {
    CFStringCreateWithBytes(
      kCFAllocatorDefault,
      value.as_ptr(),
      value.len() as isize,
      kCFStringEncodingUTF8,
      0,
    )
  };
  if string.is_null() {
    return Err(VideoError::new(format!(
      "Failed to create CoreFoundation string {value}."
    )));
  }
  Ok(string)
}

fn cf_number_i32(value: i32) -> Result<CFTypeRef, VideoError> {
  let number = unsafe { CFNumberCreate(kCFAllocatorDefault, kCFNumberSInt32Type, (&value as *const i32).cast()) };
  if number.is_null() {
    return Err(VideoError::new("Failed to create CoreFoundation number."));
  }
  Ok(number.cast())
}

fn build_av1c(sequence_header_obu: &[u8]) -> Result<Vec<u8>, VideoError> {
  let sequence = parse_av1_sequence_header(sequence_header_obu)?.sequence;
  let sequence_header_obu = av1_obu_with_size_field(sequence_header_obu)?;
  let mut av1c = Vec::with_capacity(4 + sequence_header_obu.len());
  av1c.push(0x80 | 1);
  av1c.push((sequence.profile << 5) | (sequence.level_idx & 0x1f));
  av1c.push(
    (sequence.tier << 7)
      | (u8::from(sequence.high_bitdepth) << 6)
      | (u8::from(sequence.twelve_bit) << 5)
      | (u8::from(sequence.monochrome) << 4)
      | (u8::from(sequence.chroma_subsampling_x) << 3)
      | (u8::from(sequence.chroma_subsampling_y) << 2)
      | (sequence.chroma_sample_position & 0x03),
  );
  av1c.push(0);
  av1c.extend_from_slice(&sequence_header_obu);
  Ok(av1c)
}

#[derive(Debug)]
struct ParsedAv1SequenceHeader {
  sequence: Av1SequenceHeader,
  payload_bytes: usize,
}

#[derive(Debug)]
struct Av1SequenceHeader {
  profile: u8,
  level_idx: u8,
  tier: u8,
  high_bitdepth: bool,
  twelve_bit: bool,
  monochrome: bool,
  chroma_subsampling_x: bool,
  chroma_subsampling_y: bool,
  chroma_sample_position: u8,
}

fn parse_av1_sequence_header(sequence_header_obu: &[u8]) -> Result<ParsedAv1SequenceHeader, VideoError> {
  let payload =
    av1_obu_payload(sequence_header_obu)?.ok_or_else(|| VideoError::new("AV1 sequence-header OBU is malformed."))?;
  let mut bits = BitReader::new(payload);

  let profile = bits.read_bits(3)? as u8;
  let _still_picture = bits.read_bool()?;
  let reduced_still_picture_header = bits.read_bool()?;

  let mut level_idx = 0;
  let mut tier = 0;
  let mut decoder_model_info = None;
  let initial_display_delay_present;
  let operating_points;

  if reduced_still_picture_header {
    operating_points = 1;
    level_idx = bits.read_bits(5)? as u8;
    initial_display_delay_present = false;
  } else {
    let timing_info_present = bits.read_bool()?;
    let decoder_model_info_present = if timing_info_present {
      bits.skip_bits(32)?; // num_units_in_display_tick
      bits.skip_bits(32)?; // time_scale
      if bits.read_bool()? {
        bits.read_uvlc()?;
      }
      bits.read_bool()?
    } else {
      false
    };

    if decoder_model_info_present {
      let buffer_delay_length = bits.read_bits(5)? + 1;
      bits.skip_bits(32)?; // num_units_in_decoding_tick
      let buffer_removal_time_length = bits.read_bits(5)? + 1;
      let frame_presentation_time_length = bits.read_bits(5)? + 1;
      decoder_model_info = Some(Av1DecoderModelInfo {
        buffer_delay_length,
        buffer_removal_time_length,
        frame_presentation_time_length,
      });
    }

    initial_display_delay_present = bits.read_bool()?;
    operating_points = bits.read_bits(5)? + 1;
  }

  for i in 0..operating_points {
    bits.skip_bits(12)?; // operating_point_idc
    let current_level_idx = bits.read_bits(5)? as u8;
    let current_tier = if current_level_idx > 7 {
      u8::from(bits.read_bool()?)
    } else {
      0
    };
    if i == 0 {
      level_idx = current_level_idx;
      tier = current_tier;
    }

    if let Some(model_info) = decoder_model_info
      && bits.read_bool()?
    {
      bits.skip_bits(model_info.buffer_delay_length)?;
      bits.skip_bits(model_info.buffer_delay_length)?;
      bits.skip_bits(1)?; // low_delay_mode_flag
    }

    if initial_display_delay_present && bits.read_bool()? {
      bits.skip_bits(4)?;
    }
  }

  let frame_width_bits = bits.read_bits(4)? + 1;
  let frame_height_bits = bits.read_bits(4)? + 1;
  bits.skip_bits(frame_width_bits)?;
  bits.skip_bits(frame_height_bits)?;

  if !reduced_still_picture_header {
    if bits.read_bool()? {
      bits.skip_bits(4)?;
      bits.skip_bits(3)?;
    }
  }

  bits.skip_bits(1)?; // use_128x128_superblock
  bits.skip_bits(1)?; // enable_filter_intra
  bits.skip_bits(1)?; // enable_intra_edge_filter

  if !reduced_still_picture_header {
    bits.skip_bits(1)?; // enable_interintra_compound
    bits.skip_bits(1)?; // enable_masked_compound
    bits.skip_bits(1)?; // enable_warped_motion
    bits.skip_bits(1)?; // enable_dual_filter
    let enable_order_hint = bits.read_bool()?;
    if enable_order_hint {
      bits.skip_bits(1)?; // enable_jnt_comp
      bits.skip_bits(1)?; // enable_ref_frame_mvs
    }
    let seq_force_screen_content_tools = bits.read_select_or_bit()?;
    if seq_force_screen_content_tools > 0 {
      bits.read_select_or_bit()?;
    }
    if enable_order_hint {
      bits.skip_bits(3)?;
    }
  }

  bits.skip_bits(1)?; // enable_superres
  bits.skip_bits(1)?; // enable_cdef
  bits.skip_bits(1)?; // enable_restoration

  let high_bitdepth = bits.read_bool()?;
  let twelve_bit = profile == 2 && high_bitdepth && bits.read_bool()?;
  let monochrome = profile != 1 && bits.read_bool()?;

  let mut color_primaries = 2;
  let mut transfer_characteristics = 2;
  let mut matrix_coefficients = 2;
  if bits.read_bool()? {
    color_primaries = bits.read_bits(8)?;
    transfer_characteristics = bits.read_bits(8)?;
    matrix_coefficients = bits.read_bits(8)?;
  }

  let (chroma_subsampling_x, chroma_subsampling_y, chroma_sample_position) = if monochrome {
    bits.skip_bits(1)?; // color_range
    (false, false, 0)
  } else if color_primaries == 1 && transfer_characteristics == 13 && matrix_coefficients == 0 {
    (false, false, 0)
  } else {
    bits.skip_bits(1)?; // color_range
    let (x, y) = match profile {
      0 => (true, true),
      1 => (false, false),
      2 if twelve_bit => (bits.read_bool()?, false),
      2 => {
        let x = bits.read_bool()?;
        let y = if x { bits.read_bool()? } else { false };
        (x, y)
      }
      _ => return Err(VideoError::new(format!("Unsupported AV1 profile {profile}."))),
    };
    let sample_position = if x && y { bits.read_bits(2)? as u8 } else { 0 };
    (x, y, sample_position)
  };

  bits.skip_bits(1)?; // separate_uv_delta_q
  bits.skip_bits(1)?; // film_grain_params_present
  bits.skip_trailing_bits()?;

  Ok(ParsedAv1SequenceHeader {
    sequence: Av1SequenceHeader {
      profile,
      level_idx,
      tier,
      high_bitdepth,
      twelve_bit,
      monochrome,
      chroma_subsampling_x,
      chroma_subsampling_y,
      chroma_sample_position,
    },
    payload_bytes: bits.bytes_consumed(),
  })
}

#[derive(Clone, Copy)]
struct Av1DecoderModelInfo {
  buffer_delay_length: u32,
  #[allow(dead_code)]
  buffer_removal_time_length: u32,
  #[allow(dead_code)]
  frame_presentation_time_length: u32,
}

fn find_av1_sequence_header_obu(encoded: &[u8]) -> Option<Vec<u8>> {
  let mut offset = 0;
  while offset < encoded.len() {
    let start = offset;
    let header = *encoded.get(offset)?;
    offset += 1;
    if header & 0x80 != 0 {
      return None;
    }
    let obu_type = (header >> 3) & 0x0f;
    let has_extension = header & 0x04 != 0;
    let has_size_field = header & 0x02 != 0;
    if has_extension {
      offset = offset.checked_add(1)?;
      encoded.get(offset - 1)?;
    }
    let payload_size = if has_size_field {
      let (value, bytes_read) = read_leb128(&encoded[offset..])?;
      offset = offset.checked_add(bytes_read)?;
      usize::try_from(value).ok()?
    } else if obu_type == OBU_SEQUENCE_HEADER {
      let header_len = offset.checked_sub(start)?;
      let parsed = parse_av1_sequence_header(&encoded[start..]).ok()?;
      let end = start.checked_add(header_len)?.checked_add(parsed.payload_bytes)?;
      if end > encoded.len() {
        return None;
      }
      return Some(encoded[start..end].to_vec());
    } else {
      encoded.len().checked_sub(offset)?
    };
    let end = offset.checked_add(payload_size)?;
    if end > encoded.len() {
      return None;
    }
    if obu_type == OBU_SEQUENCE_HEADER {
      return Some(encoded[start..end].to_vec());
    }
    offset = end;
  }
  None
}

fn av1_obu_payload(obu: &[u8]) -> Result<Option<&[u8]>, VideoError> {
  if obu.is_empty() {
    return Ok(None);
  }
  let header = obu[0];
  if header & 0x80 != 0 {
    return Err(VideoError::new("AV1 OBU forbidden bit is set."));
  }
  if ((header >> 3) & 0x0f) != OBU_SEQUENCE_HEADER {
    return Ok(None);
  }
  let has_extension = header & 0x04 != 0;
  let has_size_field = header & 0x02 != 0;
  let mut offset = 1usize;
  if has_extension {
    if offset >= obu.len() {
      return Err(VideoError::new("AV1 sequence-header OBU extension is truncated."));
    }
    offset += 1;
  }
  if has_size_field {
    let (payload_size, bytes_read) =
      read_leb128(&obu[offset..]).ok_or_else(|| VideoError::new("AV1 sequence-header OBU size is truncated."))?;
    offset += bytes_read;
    let payload_size =
      usize::try_from(payload_size).map_err(|_| VideoError::new("AV1 sequence-header OBU size is too large."))?;
    let end = offset
      .checked_add(payload_size)
      .ok_or_else(|| VideoError::new("AV1 sequence-header OBU size overflow."))?;
    if end > obu.len() {
      return Err(VideoError::new("AV1 sequence-header OBU payload is truncated."));
    }
    Ok(Some(&obu[offset..end]))
  } else {
    Ok(Some(&obu[offset..]))
  }
}

fn av1_obu_with_size_field(obu: &[u8]) -> Result<Vec<u8>, VideoError> {
  let payload = av1_obu_payload(obu)?.ok_or_else(|| VideoError::new("AV1 sequence-header OBU is malformed."))?;
  if obu[0] & 0x02 != 0 {
    return Ok(obu.to_vec());
  }

  let has_extension = obu[0] & 0x04 != 0;
  let payload_len = parse_av1_sequence_header(obu)?.payload_bytes;
  if payload_len > payload.len() {
    return Err(VideoError::new("AV1 sequence-header OBU payload length is invalid."));
  }
  let payload = &payload[..payload_len];

  let mut out = Vec::with_capacity(obu.len() + 8);
  out.push(obu[0] | 0x02);
  if has_extension {
    let extension = *obu
      .get(1)
      .ok_or_else(|| VideoError::new("AV1 sequence-header OBU extension is truncated."))?;
    out.push(extension);
  }
  write_leb128(payload.len() as u64, &mut out);
  out.extend_from_slice(payload);
  Ok(out)
}

fn read_leb128(input: &[u8]) -> Option<(u64, usize)> {
  let mut value = 0u64;
  for (i, byte) in input.iter().copied().enumerate().take(8) {
    value |= u64::from(byte & 0x7f) << (i * 7);
    if byte & 0x80 == 0 {
      return Some((value, i + 1));
    }
  }
  None
}

fn write_leb128(mut value: u64, out: &mut Vec<u8>) {
  loop {
    let mut byte = (value & 0x7f) as u8;
    value >>= 7;
    if value != 0 {
      byte |= 0x80;
    }
    out.push(byte);
    if value == 0 {
      break;
    }
  }
}

struct BitReader<'a> {
  bytes: &'a [u8],
  bit_offset: usize,
}

impl<'a> BitReader<'a> {
  fn new(bytes: &'a [u8]) -> Self {
    Self { bytes, bit_offset: 0 }
  }

  fn read_bool(&mut self) -> Result<bool, VideoError> {
    Ok(self.read_bits(1)? != 0)
  }

  fn read_bits(&mut self, count: u32) -> Result<u32, VideoError> {
    if count > 32 {
      return Err(VideoError::new("AV1 bit reader cannot read more than 32 bits."));
    }
    let mut value = 0u32;
    for _ in 0..count {
      let byte_index = self.bit_offset / 8;
      let bit_index = 7 - (self.bit_offset % 8);
      let byte = *self
        .bytes
        .get(byte_index)
        .ok_or_else(|| VideoError::new("AV1 sequence-header OBU is truncated."))?;
      value = (value << 1) | u32::from((byte >> bit_index) & 1);
      self.bit_offset += 1;
    }
    Ok(value)
  }

  fn skip_bits(&mut self, count: u32) -> Result<(), VideoError> {
    self.read_bits(count).map(|_| ())
  }

  fn read_select_or_bit(&mut self) -> Result<u32, VideoError> {
    if self.read_bool()? { Ok(2) } else { self.read_bits(1) }
  }

  fn read_uvlc(&mut self) -> Result<u32, VideoError> {
    let mut leading_zeroes = 0u32;
    while !self.read_bool()? {
      leading_zeroes += 1;
      if leading_zeroes >= 32 {
        return Err(VideoError::new("AV1 uvlc value is too large."));
      }
    }
    if leading_zeroes == 0 {
      return Ok(0);
    }
    Ok((1 << leading_zeroes) - 1 + self.read_bits(leading_zeroes)?)
  }

  fn skip_trailing_bits(&mut self) -> Result<(), VideoError> {
    if !self.read_bool()? {
      return Err(VideoError::new("AV1 sequence-header trailing bit is missing."));
    }
    while !self.bit_offset.is_multiple_of(8) {
      if self.read_bool()? {
        return Err(VideoError::new("AV1 sequence-header trailing padding bit is non-zero."));
      }
    }
    Ok(())
  }

  fn bytes_consumed(&self) -> usize {
    self.bit_offset.div_ceil(8)
  }
}

struct PixelBufferAttributes {
  ptr: CFDictionaryRef,
  pixel_format: CFTypeRef,
  iosurface_properties: CFTypeRef,
}

impl PixelBufferAttributes {
  fn native_nv12() -> Result<Self, VideoError> {
    let pixel_format_value = kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange as i32;
    let pixel_format = unsafe {
      CFNumberCreate(
        kCFAllocatorDefault,
        kCFNumberSInt32Type,
        (&pixel_format_value as *const i32).cast(),
      )
    };
    if pixel_format.is_null() {
      return Err(VideoError::new("Failed to create CoreFoundation pixel format number."));
    }

    let iosurface_properties = unsafe {
      CFDictionaryCreate(
        kCFAllocatorDefault,
        ptr::null(),
        ptr::null(),
        0,
        &kCFTypeDictionaryKeyCallBacks,
        &kCFTypeDictionaryValueCallBacks,
      )
    };
    if iosurface_properties.is_null() {
      unsafe {
        CFRelease(pixel_format.cast());
      }
      return Err(VideoError::new(
        "Failed to create empty IOSurface properties dictionary.",
      ));
    }

    let keys = [
      unsafe { kCVPixelBufferPixelFormatTypeKey }.cast::<c_void>(),
      unsafe { kCVPixelBufferMetalCompatibilityKey }.cast::<c_void>(),
      unsafe { kCVPixelBufferIOSurfacePropertiesKey }.cast::<c_void>(),
    ];
    let values = [
      pixel_format.cast::<c_void>(),
      unsafe { kCFBooleanTrue }.cast::<c_void>(),
      iosurface_properties.cast::<c_void>(),
    ];
    let dictionary = unsafe {
      CFDictionaryCreate(
        kCFAllocatorDefault,
        keys.as_ptr(),
        values.as_ptr(),
        keys.len() as isize,
        &kCFTypeDictionaryKeyCallBacks,
        &kCFTypeDictionaryValueCallBacks,
      )
    };
    if dictionary.is_null() {
      unsafe {
        CFRelease(pixel_format.cast());
        CFRelease(iosurface_properties.cast());
      }
      return Err(VideoError::new("Failed to create CoreVideo pixel buffer attributes."));
    }

    Ok(Self {
      ptr: dictionary,
      pixel_format: pixel_format.cast(),
      iosurface_properties: iosurface_properties.cast(),
    })
  }
}

impl Drop for PixelBufferAttributes {
  fn drop(&mut self) {
    unsafe {
      CFRelease(self.ptr.cast());
      CFRelease(self.pixel_format);
      CFRelease(self.iosurface_properties);
    }
  }
}

struct SampleBuffer {
  ptr: CMSampleBufferRef,
  _sample_data: Bytes,
}

impl SampleBuffer {
  fn new(
    sample_data: Bytes,
    format_description: CMVideoFormatDescriptionRef,
    timestamp_ms: u32,
  ) -> Result<Self, VideoError> {
    if sample_data.is_empty() {
      return Err(VideoError::new("Encoded video sample is empty."));
    }

    let mut block = ptr::null();
    let status = unsafe {
      CMBlockBufferCreateWithMemoryBlock(
        kCFAllocatorDefault,
        sample_data.as_ptr().cast::<c_void>().cast_mut(),
        sample_data.len(),
        kCFAllocatorNull,
        ptr::null(),
        0,
        sample_data.len(),
        0,
        &mut block,
      )
    };
    if status != NO_ERR || block.is_null() {
      return Err(VideoError::new(format!(
        "Failed to create CoreMedia block buffer: OSStatus {status}."
      )));
    }

    let timing = CMSampleTimingInfo {
      duration: unsafe { kCMTimeInvalid },
      presentation_time_stamp: cm_time_millis(timestamp_ms),
      decode_time_stamp: unsafe { kCMTimeInvalid },
    };
    let sample_size = sample_data.len();
    let mut sample = ptr::null_mut();
    let status = unsafe {
      CMSampleBufferCreateReady(
        kCFAllocatorDefault,
        block,
        format_description,
        1,
        1,
        &timing,
        1,
        &sample_size,
        &mut sample,
      )
    };
    if status != NO_ERR || sample.is_null() {
      unsafe {
        CFRelease(block.cast());
      }
      return Err(VideoError::new(format!(
        "Failed to create CoreMedia sample buffer: OSStatus {status}."
      )));
    }

    unsafe {
      CFRelease(block.cast());
    }

    Ok(Self {
      ptr: sample,
      _sample_data: sample_data,
    })
  }
}

impl Drop for SampleBuffer {
  fn drop(&mut self) {
    unsafe {
      CFRelease(self.ptr.cast());
    }
  }
}

#[derive(Default)]
struct AccessUnits {
  raw_sample: Option<Bytes>,
  av1_sequence_header: Option<Vec<u8>>,
  h264_sps: Option<Vec<u8>>,
  h264_pps: Option<Vec<u8>>,
  h265_vps: Option<Vec<u8>>,
  h265_sps: Option<Vec<u8>>,
  h265_pps: Option<Vec<u8>>,
  encoded: Bytes,
  nals: Vec<Range<usize>>,
  length_prefixed_input: bool,
}

impl AccessUnits {
  fn parse(codec: VideoCodecId, encoded: Bytes) -> Result<Self, VideoError> {
    if encoded.is_empty() {
      return Err(VideoError::new("Encoded video frame is empty."));
    }

    let mut units = Self::default();
    if codec == VideoCodecId::Av1 {
      units.av1_sequence_header = find_av1_sequence_header_obu(&encoded);
      units.raw_sample = Some(encoded);
      return Ok(units);
    }

    let nals = if looks_like_annex_b(&encoded) {
      split_annex_b_ranges(&encoded)
    } else {
      units.length_prefixed_input = true;
      split_length_prefixed_ranges(&encoded)?
    };
    units.encoded = encoded;

    for nal in nals {
      if nal.is_empty() {
        continue;
      }
      let nal_bytes = &units.encoded[nal.clone()];
      match codec {
        VideoCodecId::H264 => match nal_bytes[0] & 0x1f {
          7 => units.h264_sps = Some(nal_bytes.to_vec()),
          8 => units.h264_pps = Some(nal_bytes.to_vec()),
          _ => {}
        },
        VideoCodecId::H265 => match (nal_bytes[0] >> 1) & 0x3f {
          32 => units.h265_vps = Some(nal_bytes.to_vec()),
          33 => units.h265_sps = Some(nal_bytes.to_vec()),
          34 => units.h265_pps = Some(nal_bytes.to_vec()),
          _ => {}
        },
        VideoCodecId::Av1 | VideoCodecId::Unknown => {}
      }
      units.nals.push(nal);
    }

    if units.nals.is_empty() {
      return Err(VideoError::new("Encoded video frame contains no NAL units."));
    }

    Ok(units)
  }

  fn can_initialize_session(&self, codec: VideoCodecId) -> bool {
    match codec {
      VideoCodecId::Av1 => self.av1_sequence_header.is_some(),
      VideoCodecId::H264 => self.h264_sps.is_some() && self.h264_pps.is_some(),
      VideoCodecId::H265 => self.h265_vps.is_some() && self.h265_sps.is_some() && self.h265_pps.is_some(),
      VideoCodecId::Unknown => false,
    }
  }

  fn parameter_set_summary(&self) -> String {
    format!(
      "av1_raw={} h264_sps={} h264_pps={} h265_vps={} h265_sps={} h265_pps={} nal_count={}",
      self.raw_sample.is_some(),
      self.h264_sps.is_some(),
      self.h264_pps.is_some(),
      self.h265_vps.is_some(),
      self.h265_sps.is_some(),
      self.h265_pps.is_some(),
      self.nals.len()
    )
  }

  fn sample_data(&self, codec: VideoCodecId) -> Result<Bytes, VideoError> {
    if codec == VideoCodecId::Av1 {
      return self
        .raw_sample
        .clone()
        .ok_or_else(|| VideoError::new("Encoded AV1 frame is empty."));
    }

    if self.length_prefixed_input {
      return Ok(self.encoded.clone());
    }

    let sample_len = self.nals.iter().try_fold(0usize, |total, nal| {
      let len = nal.end.saturating_sub(nal.start);
      let _ = u32::try_from(len).map_err(|_| VideoError::new("NAL unit is too large."))?;
      total
        .checked_add(4)
        .and_then(|value| value.checked_add(len))
        .ok_or_else(|| VideoError::new("Encoded video sample is too large."))
    })?;
    let mut out = Vec::with_capacity(sample_len);
    for nal in &self.nals {
      let nal_bytes = &self.encoded[nal.clone()];
      let len = u32::try_from(nal_bytes.len()).map_err(|_| VideoError::new("NAL unit is too large."))?;
      out.extend_from_slice(&len.to_be_bytes());
      out.extend_from_slice(nal_bytes);
    }
    Ok(Bytes::from(out))
  }
}

fn native_image_from_pixel_buffer(pixel_buffer: CVPixelBufferRef) -> Result<lurq::images::ImageData, VideoError> {
  let pixel_format = unsafe { CVPixelBufferGetPixelFormatType(pixel_buffer) };
  if pixel_format != kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange {
    return Err(VideoError::new(format!(
      "VideoToolbox returned unsupported pixel format 0x{pixel_format:08x}; expected NV12/420v."
    )));
  }

  let width = unsafe { CVPixelBufferGetWidth(pixel_buffer) } as u32;
  let height = unsafe { CVPixelBufferGetHeight(pixel_buffer) } as u32;
  if width == 0 || height == 0 || width % 2 != 0 || height % 2 != 0 {
    return Err(VideoError::new(format!(
      "VideoToolbox returned invalid NV12 dimensions {width}x{height}."
    )));
  }

  let pixel_buffer = unsafe { lurq::images::MacosCvPixelBuffer::retain(pixel_buffer) };
  Ok(lurq::images::NativeImageData::from_macos_cv_pixel_buffer_nv12(width, height, pixel_buffer).image_data())
}

fn copy_nv12_pixel_buffer(pixel_buffer: CVPixelBufferRef) -> Result<Vec<u8>, VideoError> {
  let pixel_format = unsafe { CVPixelBufferGetPixelFormatType(pixel_buffer) };
  if pixel_format != kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange {
    return Err(VideoError::new(format!(
      "Unsupported decoded pixel format for CPU fallback: 0x{pixel_format:08x}."
    )));
  }

  let width = unsafe { CVPixelBufferGetWidth(pixel_buffer) };
  let height = unsafe { CVPixelBufferGetHeight(pixel_buffer) };
  if width == 0 || height == 0 || width % 2 != 0 || height % 2 != 0 {
    return Err(VideoError::new(format!(
      "Decoded pixel buffer has invalid NV12 dimensions {width}x{height}."
    )));
  }

  let lock_status = unsafe { CVPixelBufferLockBaseAddress(pixel_buffer, 1) };
  if lock_status != NO_ERR {
    return Err(VideoError::new(format!(
      "Failed to lock decoded pixel buffer for CPU fallback: OSStatus {lock_status}."
    )));
  }

  let result = copy_locked_nv12_pixel_buffer(pixel_buffer, width, height);
  let unlock_status = unsafe { CVPixelBufferUnlockBaseAddress(pixel_buffer, 1) };
  if unlock_status != NO_ERR {
    return Err(VideoError::new(format!(
      "Failed to unlock decoded pixel buffer for CPU fallback: OSStatus {unlock_status}."
    )));
  }
  result
}

fn copy_locked_nv12_pixel_buffer(
  pixel_buffer: CVPixelBufferRef,
  width: usize,
  height: usize,
) -> Result<Vec<u8>, VideoError> {
  let y_base = unsafe { CVPixelBufferGetBaseAddressOfPlane(pixel_buffer, 0) };
  let uv_base = unsafe { CVPixelBufferGetBaseAddressOfPlane(pixel_buffer, 1) };
  if y_base.is_null() || uv_base.is_null() {
    return Err(VideoError::new("Decoded pixel buffer does not expose NV12 planes."));
  }

  let y_stride = unsafe { CVPixelBufferGetBytesPerRowOfPlane(pixel_buffer, 0) };
  let uv_stride = unsafe { CVPixelBufferGetBytesPerRowOfPlane(pixel_buffer, 1) };
  let y_height = unsafe { CVPixelBufferGetHeight(pixel_buffer) };
  let uv_height = height / 2;
  if y_stride < width || uv_stride < width || y_height < height {
    return Err(VideoError::new("Decoded pixel buffer has invalid NV12 plane strides."));
  }

  let mut out = vec![0u8; width * height + width * uv_height];
  let y_src = unsafe { slice::from_raw_parts(y_base.cast::<u8>(), y_stride * y_height) };
  let uv_src = unsafe { slice::from_raw_parts(uv_base.cast::<u8>(), uv_stride * uv_height) };

  for row in 0..height {
    let src = row * y_stride;
    let dst = row * width;
    out[dst..dst + width].copy_from_slice(&y_src[src..src + width]);
  }

  let uv_dst_offset = width * height;
  for row in 0..uv_height {
    let src = row * uv_stride;
    let dst = uv_dst_offset + row * width;
    out[dst..dst + width].copy_from_slice(&uv_src[src..src + width]);
  }

  Ok(out)
}

fn looks_like_annex_b(bytes: &[u8]) -> bool {
  bytes.starts_with(&[0, 0, 1]) || bytes.starts_with(&[0, 0, 0, 1])
}

#[cfg(test)]
fn split_annex_b(bytes: &[u8]) -> Vec<Vec<u8>> {
  split_annex_b_ranges(bytes)
    .into_iter()
    .map(|range| bytes[range].to_vec())
    .collect()
}

fn split_annex_b_ranges(bytes: &[u8]) -> Vec<Range<usize>> {
  let mut out = Vec::new();
  let mut cursor = 0;
  while let Some((start_code, start_code_len)) = find_start_code(bytes, cursor) {
    let nal_start = start_code + start_code_len;
    let next = find_start_code(bytes, nal_start)
      .map(|(index, _)| index)
      .unwrap_or(bytes.len());
    if nal_start < next {
      out.push(nal_start..next);
    }
    cursor = next;
  }
  out
}

fn find_start_code(bytes: &[u8], from: usize) -> Option<(usize, usize)> {
  let mut index = from;
  while index + 3 <= bytes.len() {
    if bytes[index..].starts_with(&[0, 0, 1]) {
      return Some((index, 3));
    }
    if index + 4 <= bytes.len() && bytes[index..].starts_with(&[0, 0, 0, 1]) {
      return Some((index, 4));
    }
    index += 1;
  }
  None
}

#[cfg(test)]
fn split_length_prefixed(bytes: &[u8]) -> Result<Vec<Vec<u8>>, VideoError> {
  Ok(
    split_length_prefixed_ranges(bytes)?
      .into_iter()
      .map(|range| bytes[range].to_vec())
      .collect(),
  )
}

fn split_length_prefixed_ranges(bytes: &[u8]) -> Result<Vec<Range<usize>>, VideoError> {
  let mut out = Vec::new();
  let mut cursor = 0;
  while cursor + 4 <= bytes.len() {
    let len = u32::from_be_bytes(bytes[cursor..cursor + 4].try_into().unwrap()) as usize;
    cursor += 4;
    if len == 0 || cursor + len > bytes.len() {
      return Err(VideoError::new("Invalid length-prefixed video NAL unit."));
    }
    out.push(cursor..cursor + len);
    cursor += len;
  }
  if cursor != bytes.len() {
    return Err(VideoError::new("Trailing bytes after length-prefixed video NAL units."));
  }
  Ok(out)
}

fn cm_time_millis(timestamp_ms: u32) -> CMTime {
  CMTime {
    value: i64::from(timestamp_ms),
    timescale: 1000,
    flags: kCMTimeFlags_Valid,
    epoch: 0,
  }
}

fn cm_time_100ns(timestamp_100ns: i64) -> CMTime {
  CMTime {
    value: timestamp_100ns,
    timescale: 10_000_000,
    flags: kCMTimeFlags_Valid,
    epoch: 0,
  }
}

fn codec_label(codec: VideoCodecId) -> &'static str {
  match codec {
    VideoCodecId::Av1 => "AV1",
    VideoCodecId::H265 => "H.265",
    VideoCodecId::H264 => "H.264",
    VideoCodecId::Unknown => "Unknown",
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn macos_backend_order_matches_original_parties() {
    assert_eq!(BACKEND_ORDER, [NativeVideoBackend::AppleVideoToolbox]);
  }

  #[test]
  fn splits_annex_b_nals() {
    let nals = split_annex_b(&[0, 0, 0, 1, 0x67, 1, 2, 0, 0, 1, 0x68, 3]);
    assert_eq!(nals, vec![vec![0x67, 1, 2], vec![0x68, 3]]);
  }

  #[test]
  fn splits_length_prefixed_nals() {
    let nals = split_length_prefixed(&[0, 0, 0, 2, 0x67, 1, 0, 0, 0, 1, 0x68]).unwrap();
    assert_eq!(nals, vec![vec![0x67, 1], vec![0x68]]);
  }
}
