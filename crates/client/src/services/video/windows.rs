use std::{
  ffi::{CStr, c_char},
  mem::ManuallyDrop,
  ptr::{self, NonNull},
  sync::{
    Arc, Once,
    atomic::{AtomicBool, AtomicU64, Ordering},
    mpsc,
  },
  thread,
  time::{Duration, Instant},
};

use ::windows::{
  Win32::{
    Foundation::{CloseHandle, HANDLE, RPC_E_CHANGED_MODE, WAIT_OBJECT_0, WAIT_TIMEOUT},
    Graphics::Dxgi::{CreateDXGIFactory1, DXGI_ERROR_NOT_FOUND, IDXGIFactory1},
    Media::{
      Audio::{
        AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM,
        AUDCLNT_STREAMFLAGS_EVENTCALLBACK, AUDCLNT_STREAMFLAGS_LOOPBACK, AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY,
        AUDIOCLIENT_ACTIVATION_PARAMS, AUDIOCLIENT_ACTIVATION_PARAMS_0, AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK,
        AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS, ActivateAudioInterfaceAsync, IActivateAudioInterfaceAsyncOperation,
        IActivateAudioInterfaceCompletionHandler, IActivateAudioInterfaceCompletionHandler_Impl, IAudioCaptureClient,
        IAudioClient, PROCESS_LOOPBACK_MODE, PROCESS_LOOPBACK_MODE_EXCLUDE_TARGET_PROCESS_TREE,
        PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE, VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK, WAVEFORMATEX,
      },
      Multimedia::WAVE_FORMAT_IEEE_FLOAT,
    },
    System::{
      Com::{
        BLOB, COINIT_MULTITHREADED, CoInitializeEx, CoTaskMemAlloc, CoUninitialize,
        StructuredStorage::{PROPVARIANT, PROPVARIANT_0, PROPVARIANT_0_0, PROPVARIANT_0_0_0},
      },
      Threading::{CreateEventW, GetCurrentProcessId, WaitForSingleObject},
      Variant::VT_BLOB,
    },
  },
  core::{IUnknown, Interface, PCWSTR},
};
use opus::{Application as OpusApplication, Bitrate as OpusBitrate, Channels as OpusChannels, Encoder as OpusEncoder};

use super::{
  DecodedVideoPixelFormat, NativeDecodedVideoFrame, NativeVideoBackend, VideoBroadcast, VideoBroadcastConfig,
  VideoDecodeConfig, VideoError, VideoFrameDecoder, VideoFrameLoopback, software::SoftwareVideoDecoder,
  webcam::WebcamCapture,
};
use crate::{
  network::{
    protocol::{VideoCodecId, VideoFrame},
    server::{Server, VideoFrameSend},
  },
  services::{
    desktop_capture::{
      DesktopCaptureSource, DesktopCaptureSourceKind, find_window_process_id as find_desktop_window_process_id,
    },
    profiler,
    screen_share_sources::ScreenShareSourceKind,
  },
};

#[path = "windows/decode.rs"]
mod decoder_factory;

pub(super) use decoder_factory::decode;

#[allow(dead_code)]
const BACKEND_ORDER: [NativeVideoBackend; 3] = [
  NativeVideoBackend::NvidiaNvenc,
  NativeVideoBackend::AmdAmf,
  NativeVideoBackend::OpenH264,
];

const STREAM_AUDIO_SAMPLE_RATE: u32 = 48_000;
const STREAM_AUDIO_CHANNELS: usize = 2;
const STREAM_AUDIO_FRAME_SAMPLES_PER_CHANNEL: usize = 960;
const STREAM_AUDIO_FRAME_SAMPLES: usize = STREAM_AUDIO_FRAME_SAMPLES_PER_CHANNEL * STREAM_AUDIO_CHANNELS;
const STREAM_AUDIO_FRAME_DURATION_100NS: i64 = 200_000;
const STREAM_AUDIO_BITRATE: i32 = 64_000;
const STREAM_AUDIO_MAX_PACKET_BYTES: usize = 1_275;
const NVIDIA_VENDOR_ID: u32 = 0x10DE;
const AMD_VENDOR_ID: u32 = 0x1002;

#[repr(C)]
struct NvdecBridge {
  _private: [u8; 0],
}

#[repr(C)]
struct NvencBridge {
  _private: [u8; 0],
}

#[repr(C)]
struct AmfBridge {
  _private: [u8; 0],
}

#[repr(C)]
struct AmfDecoderBridge {
  _private: [u8; 0],
}

#[repr(C)]
struct MftH264DecoderBridge {
  _private: [u8; 0],
}

#[repr(C)]
struct GpuStreamBridge {
  _private: [u8; 0],
}

unsafe extern "C" {
  fn parties_native_log_set_callback(callback: Option<extern "C" fn(level: u8, message: *const c_char)>);
  fn parties_nvdec_create(codec: u8, width: u16, height: u16) -> *mut NvdecBridge;
  fn parties_nvdec_destroy(bridge: *mut NvdecBridge);
  fn parties_nvdec_decode(
    bridge: *mut NvdecBridge,
    data: *const u8,
    len: usize,
    timestamp: i64,
    rgba: *mut u8,
    rgba_len: usize,
  ) -> i32;
  fn parties_nvdec_decode_to_d3d12(
    bridge: *mut NvdecBridge,
    data: *const u8,
    len: usize,
    timestamp: i64,
    y_handle: usize,
    y_size: u64,
    uv_handle: usize,
    uv_size: u64,
    width: u16,
    height: u16,
  ) -> i32;
  fn parties_nvenc_create(codec: u8, width: u16, height: u16, fps: u32, bitrate: u32) -> *mut NvencBridge;
  fn parties_nvenc_destroy(bridge: *mut NvencBridge);
  fn parties_nvenc_force_keyframe(bridge: *mut NvencBridge);
  fn parties_nvenc_encode_rgba(bridge: *mut NvencBridge, rgba: *const u8, rgba_len: usize, timestamp: i64) -> i32;
  fn parties_nvenc_encoded_ptr(bridge: *mut NvencBridge) -> *const u8;
  fn parties_nvenc_encoded_len(bridge: *mut NvencBridge) -> usize;
  fn parties_nvenc_encoded_keyframe(bridge: *mut NvencBridge) -> i32;
  fn parties_amf_create(codec: u8, width: u16, height: u16, fps: u32, bitrate: u32) -> *mut AmfBridge;
  fn parties_amf_destroy(bridge: *mut AmfBridge);
  fn parties_amf_force_keyframe(bridge: *mut AmfBridge);
  fn parties_amf_encode_bgra(bridge: *mut AmfBridge, bgra: *const u8, bgra_len: usize, timestamp: i64) -> i32;
  fn parties_amf_encoded_ptr(bridge: *mut AmfBridge) -> *const u8;
  fn parties_amf_encoded_len(bridge: *mut AmfBridge) -> usize;
  fn parties_amf_encoded_keyframe(bridge: *mut AmfBridge) -> i32;
  fn parties_amf_decoder_create(codec: u8, width: u16, height: u16) -> *mut AmfDecoderBridge;
  fn parties_amf_decoder_destroy(bridge: *mut AmfDecoderBridge);
  fn parties_amf_decode(
    bridge: *mut AmfDecoderBridge,
    data: *const u8,
    len: usize,
    timestamp: i64,
    nv12: *mut u8,
    nv12_len: usize,
  ) -> i32;
  fn parties_amf_decode_to_d3d12(
    bridge: *mut AmfDecoderBridge,
    data: *const u8,
    len: usize,
    timestamp: i64,
    y_handle: usize,
    y_size: u64,
    uv_handle: usize,
    uv_size: u64,
    adapter_luid_low: u32,
    adapter_luid_high: i32,
    width: u16,
    height: u16,
  ) -> i32;
  fn parties_amf_decode_to_shared_nv12_planes(
    bridge: *mut AmfDecoderBridge,
    data: *const u8,
    len: usize,
    timestamp: i64,
    width: u16,
    height: u16,
    y_shared_handle_out: *mut usize,
    uv_shared_handle_out: *mut usize,
  ) -> i32;
  fn parties_mft_h264_decoder_create(width: u32, height: u32) -> *mut MftH264DecoderBridge;
  fn parties_mft_h264_decoder_destroy(decoder: *mut MftH264DecoderBridge);
  fn parties_mft_h264_decoder_decode(
    decoder: *mut MftH264DecoderBridge,
    data: *const u8,
    len: usize,
    timestamp: i64,
    output_requested: i32,
    output: *mut u8,
    output_len: usize,
    width_out: *mut u32,
    height_out: *mut u32,
    error_out: *mut u32,
  ) -> i32;
  fn parties_gpu_stream_create(
    source_kind: u8,
    source_handle: usize,
    codec: u8,
    width: u16,
    height: u16,
    fps: u32,
    bitrate: u32,
  ) -> *mut GpuStreamBridge;
  fn parties_amf_gpu_stream_create(
    source_kind: u8,
    source_handle: usize,
    codec: u8,
    width: u16,
    height: u16,
    fps: u32,
    bitrate: u32,
  ) -> *mut GpuStreamBridge;
  fn parties_gpu_stream_destroy(bridge: *mut GpuStreamBridge);
  fn parties_gpu_stream_force_keyframe(bridge: *mut GpuStreamBridge);
  fn parties_gpu_stream_poll(bridge: *mut GpuStreamBridge) -> i32;
  fn parties_gpu_stream_encoded_ptr(bridge: *mut GpuStreamBridge) -> *const u8;
  fn parties_gpu_stream_encoded_len(bridge: *mut GpuStreamBridge) -> usize;
  fn parties_gpu_stream_encoded_keyframe(bridge: *mut GpuStreamBridge) -> i32;
}

static NATIVE_LOGGER_INIT: Once = Once::new();

fn install_native_logger() {
  NATIVE_LOGGER_INIT.call_once(|| unsafe {
    parties_native_log_set_callback(Some(native_log_callback));
  });
}

extern "C" fn native_log_callback(level: u8, message: *const c_char) {
  if message.is_null() {
    return;
  }

  let message = unsafe { CStr::from_ptr(message) }.to_string_lossy();
  match level {
    0 => tracing::debug!(target: "native::windows", "[native/windows/debug] {message}"),
    1 => tracing::info!(target: "native::windows", "[native/windows/info] {message}"),
    2 => tracing::warn!(target: "native::windows", "[native/windows/warn] {message}"),
    3 => tracing::error!(target: "native::windows", "[native/windows/error] {message}"),
    _ => tracing::warn!(target: "native::windows", "[native/windows/unknown] {message}"),
  }
}

pub(super) fn encode(
  server: Arc<Server>,
  config: VideoBroadcastConfig,
  loopback: Option<VideoFrameLoopback>,
) -> Result<VideoBroadcast, VideoError> {
  install_native_logger();
  let runtime = tokio::runtime::Handle::try_current()
    .map_err(|_| VideoError::new("Video broadcasting must be started from the Tokio runtime."))?;
  let audio_enabled = config.audio_enabled;
  let stream_audio_target = stream_audio_capture_target(&config);
  let stop = Arc::new(AtomicBool::new(false));
  let keyframe_requests = Arc::new(AtomicU64::new(0));
  let thread_stop = Arc::clone(&stop);
  let thread_keyframe_requests = Arc::clone(&keyframe_requests);
  let video_server = Arc::clone(&server);
  let (ready_tx, ready_rx) = mpsc::sync_channel(1);
  let thread = thread::Builder::new()
    .name("parties-video-windows-encode".to_owned())
    .spawn(move || {
      let loop_stop = Arc::clone(&thread_stop);
      if let Err(error) = run_broadcast_loop(
        video_server,
        config,
        runtime,
        loop_stop,
        thread_keyframe_requests,
        loopback,
        Some(ready_tx),
      ) {
        thread_stop.store(true, Ordering::Relaxed);
        tracing::warn!(target: "video::encode::windows", "[video:encode/windows] broadcast loop stopped with error: {error}");
      }
    })
    .map_err(|error| VideoError::new(format!("Failed to start video broadcast thread: {error}")))?;
  let mut threads = Vec::with_capacity(if audio_enabled { 2 } else { 1 });

  let backend = match ready_rx.recv() {
    Ok(Ok(backend)) => backend,
    Ok(Err(error)) => {
      stop.store(true, Ordering::Relaxed);
      let _ = thread.join();
      return Err(VideoError::new(error));
    }
    Err(_) => {
      stop.store(true, Ordering::Relaxed);
      let _ = thread.join();
      return Err(VideoError::new(
        "Video broadcast thread exited before native encoder became ready.",
      ));
    }
  };

  if audio_enabled {
    let audio_thread = match spawn_stream_audio_thread(server, Arc::clone(&stop), stream_audio_target) {
      Ok(thread) => thread,
      Err(error) => {
        stop.store(true, Ordering::Relaxed);
        let _ = thread.join();
        return Err(error);
      }
    };
    threads.push(audio_thread);
  }
  threads.push(thread);

  Ok(VideoBroadcast::from_parts_with_stop_and_keyframes(
    backend,
    stop,
    Some(keyframe_requests),
    threads,
  ))
}

pub(super) enum NativeVideoDecoder {
  Nvdec(NvdecVideoDecoder),
  AmdAmf(AmdAmfVideoDecoder),
  MftH264(MftH264VideoDecoder),
  Software(SoftwareVideoDecoder),
}

pub(super) struct NvdecVideoDecoder {
  handle: NonNull<NvdecBridge>,
}

pub(super) struct AmdAmfVideoDecoder {
  handle: NonNull<AmfDecoderBridge>,
}

pub(super) struct MftH264VideoDecoder {
  handle: NonNull<MftH264DecoderBridge>,
}

#[allow(dead_code)]
fn backend_order_label() -> String {
  BACKEND_ORDER
    .iter()
    .map(|backend| match backend {
      NativeVideoBackend::NvidiaNvenc => "NVENC",
      NativeVideoBackend::NvidiaNvdec => "NVDEC",
      NativeVideoBackend::AmdAmf => "AMF",
      NativeVideoBackend::WindowsMediaFoundation => "Media Foundation",
      NativeVideoBackend::OpenH264 => "OpenH264",
      NativeVideoBackend::SoftwareDecoder => "Software decoder",
      NativeVideoBackend::AppleVideoToolbox => "VideoToolbox",
    })
    .collect::<Vec<_>>()
    .join(" -> ")
}

fn run_broadcast_loop(
  server: Arc<Server>,
  config: VideoBroadcastConfig,
  runtime: tokio::runtime::Handle,
  stop: Arc<AtomicBool>,
  keyframe_requests: Arc<AtomicU64>,
  loopback: Option<VideoFrameLoopback>,
  ready: Option<mpsc::SyncSender<Result<NativeVideoBackend, String>>>,
) -> Result<(), VideoError> {
  tracing::info!(target: "video::encode::windows", "[video:encode/windows] creating native encoder");
  let mut config = config;
  let mut failed_encoder_labels = Vec::new();
  let setup = (|| -> Result<(BroadcastEncoder, Option<CaptureSource>), VideoError> {
    if matches!(config.source_kind, ScreenShareSourceKind::Webcam) {
      tracing::info!(target: "video::encode::windows", "[video:encode/windows] opening CPU capture source");
      let source = CaptureSource::open(&config)?;
      if let Some(capture_fps) = source.capture_fps() {
        if capture_fps != config.fps {
          tracing::info!(target: "video::encode::windows",
            "[video:encode/windows] webcam fps adjusted to selected capture mode: requested={} selected={}",
            config.fps,
            capture_fps
          );
          config.fps = capture_fps;
        }
      }
      let encoder = BroadcastEncoder::new_excluding(&config, &failed_encoder_labels)?;
      log_encoder_ready(&encoder, &config);
      return Ok((encoder, Some(source)));
    }

    let encoder = BroadcastEncoder::new_excluding(&config, &failed_encoder_labels)?;
    log_encoder_ready(&encoder, &config);
    let source = if encoder.owns_capture() {
      None
    } else {
      tracing::info!(target: "video::encode::windows", "[video:encode/windows] opening CPU capture source");
      Some(CaptureSource::open(&config)?)
    };
    Ok((encoder, source))
  })();
  let (mut encoder, mut source) = match setup {
    Ok(setup) => {
      if let Some(ready) = ready {
        let _ = ready.send(Ok(setup.0.backend()));
      }
      setup
    }
    Err(error) => {
      if let Some(ready) = ready {
        let _ = ready.send(Err(error.to_string()));
      }
      return Err(error);
    }
  };
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
    if requested_keyframes != handled_keyframe_requests {
      handled_keyframe_requests = requested_keyframes;
      encoder.force_keyframe();
      tracing::debug!(target: "video::encode::windows", "[video:encode/windows] keyframe requested by PLI");
    }
    let timestamp_100ns = started_at.elapsed().as_nanos().saturating_div(100) as i64;
    let timestamp_ms = started_at.elapsed().as_millis().min(u128::from(u32::MAX)) as u32;
    let samples = if encoder.owns_capture() {
      encoder.encode(&[], frame_number, timestamp_100ns, &config)
    } else {
      let source = source
        .as_mut()
        .ok_or_else(|| VideoError::new("CPU capture source is not initialized."))?;
      let rgba = {
        let _span = profiler::span("video.capture.rgba");
        source.capture_rgba(config.output_width, config.output_height)?
      };
      encoder.encode(&rgba, frame_number, timestamp_100ns, &config)
    };
    let samples = match samples {
      Ok(samples) => samples,
      Err(error) => {
        let failed_backend = encoder.backend_label().to_owned();
        tracing::warn!(target: "video::encode::windows",
          "[video:encode/windows] encoder backend failed at runtime: backend={failed_backend} error={error}; trying fallback"
        );
        failed_encoder_labels.push(failed_backend);
        drop(encoder);
        encoder = BroadcastEncoder::new_excluding(&config, &failed_encoder_labels)?;
        log_encoder_ready(&encoder, &config);
        if !encoder.owns_capture() && source.is_none() {
          tracing::info!(target: "video::encode::windows", "[video:encode/windows] opening CPU capture source");
          source = Some(CaptureSource::open(&config)?);
        }
        frame_number = 0;
        logged_first_frame = false;
        continue;
      }
    };

    if samples.is_empty() {
      if encoder.owns_capture() {
        thread::sleep(Duration::from_millis(1));
      } else {
        let elapsed = loop_started_at.elapsed();
        if elapsed < frame_interval {
          thread::sleep(frame_interval - elapsed);
        }
      }
      continue;
    }

    for sample in samples {
      let sample_len = sample.bytes.len();
      let sample_keyframe = sample.keyframe;
      let sample_probe = if !logged_first_frame {
        Some(bitstream_probe(&sample.bytes))
      } else {
        None
      };
      let frame = VideoFrame {
        frame_number,
        timestamp: timestamp_ms,
        keyframe: sample_keyframe,
        width: config.output_width,
        height: config.output_height,
        codec: config.codec,
        encoded: sample.bytes.into(),
      };
      let send_result = {
        let _span = profiler::span("video.network.send_live_frame");
        runtime
          .block_on(server.send_live_video_frame(&frame))
          .map_err(|error| VideoError::new(format!("Failed to send video frame: {error}")))?
      };
      if send_result == VideoFrameSend::Dropped {
        dropped_live_frames += 1;
        if dropped_live_frames == 1 || dropped_live_frames % 120 == 0 {
          tracing::info!(target: "video::encode::windows",
            "[video:encode/windows] dropped live video frame before network queue: frame={} total_dropped={}",
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
        tracing::warn!(target: "video::encode::windows", "[video:encode/windows] live video datagrams unavailable or too large; using reliable stream fallback");
        logged_stream_fallback = true;
      }
      if !logged_first_frame {
        tracing::info!(target: "video::encode::windows",
          "[video:encode/windows] first encoded frame sent: frame={} bytes={} keyframe={} transport={:?} bitstream={}",
          frame_number,
          sample_len,
          sample_keyframe,
          send_result,
          sample_probe.unwrap_or_else(|| "unavailable".to_owned())
        );
        logged_first_frame = true;
      } else if frame_number > 0 && frame_number % 120 == 0 {
        tracing::debug!(target: "video::encode::windows",
          "[video:encode/windows] encoded frame #{} sent: bytes={} keyframe={} transport={:?}",
          frame_number,
          sample_len,
          sample_keyframe,
          send_result
        );
      }
    }

    frame_number = frame_number.wrapping_add(1);
    if encoder.owns_capture() {
      thread::sleep(Duration::from_millis(1));
    } else {
      let elapsed = loop_started_at.elapsed();
      if elapsed < frame_interval {
        thread::sleep(frame_interval - elapsed);
      }
    }
  }

  tracing::info!(target: "video::encode::windows", "[video:encode/windows] broadcast loop stopped by request");
  Ok(())
}

fn log_encoder_ready(encoder: &BroadcastEncoder, config: &VideoBroadcastConfig) {
  tracing::info!(target: "video::encode::windows",
    "[video:encode/windows] encoder ready: backend={} codec={:?} source={}x{} output={}x{} fps={} bitrate={}kbps",
    encoder.backend_label(),
    config.codec,
    config.source_width,
    config.source_height,
    config.output_width,
    config.output_height,
    config.fps,
    config.bitrate_kbps
  );
}

fn bitstream_probe(bytes: &[u8]) -> String {
  let prefix_len = bytes.len().min(8);
  let prefix = bytes
    .iter()
    .take(prefix_len)
    .map(|byte| format!("{byte:02x}"))
    .collect::<Vec<_>>()
    .join(" ");
  let annexb = bytes.starts_with(&[0, 0, 1]) || bytes.starts_with(&[0, 0, 0, 1]);
  let length_prefix = bytes
    .get(0..4)
    .map(|prefix| u32::from_be_bytes([prefix[0], prefix[1], prefix[2], prefix[3]]) as usize)
    .filter(|len| *len <= bytes.len().saturating_sub(4));
  format!(
    "prefix=[{}] annexb={} length_prefix={}",
    prefix,
    annexb,
    length_prefix
      .map(|len| len.to_string())
      .unwrap_or_else(|| "none".to_owned())
  )
}

#[derive(Clone, Copy, Debug)]
enum StreamAudioCaptureTarget {
  ExcludeProcess(u32),
  IncludeProcess(u32),
}

impl StreamAudioCaptureTarget {
  fn label(self) -> String {
    match self {
      Self::ExcludeProcess(process_id) => format!("process loopback excluding pid {process_id}"),
      Self::IncludeProcess(process_id) => format!("process loopback including pid {process_id}"),
    }
  }
}

fn stream_audio_capture_target(config: &VideoBroadcastConfig) -> StreamAudioCaptureTarget {
  match config.source_kind {
    ScreenShareSourceKind::Window => match find_window_process_id(config.source_id) {
      Ok(process_id) => {
        let current_process_id = unsafe { GetCurrentProcessId() };
        if process_id == current_process_id {
          tracing::warn!(target: "audio::encode::windows",
            "[audio:encode/windows] selected window belongs to current process; capturing Parties process audio: window={} pid={process_id}",
            config.source_id
          );
          return StreamAudioCaptureTarget::IncludeProcess(current_process_id);
        }
        tracing::info!(target: "audio::encode::windows",
          "[audio:encode/windows] selected window audio target: window={} pid={process_id}",
          config.source_id
        );
        StreamAudioCaptureTarget::IncludeProcess(process_id)
      }
      Err(error) => {
        let process_id = unsafe { GetCurrentProcessId() };
        tracing::warn!(target: "audio::encode::windows",
          "[audio:encode/windows] could not resolve selected window pid; using output loopback excluding current process: window={} exclude_pid={process_id} error={error}",
          config.source_id,
        );
        StreamAudioCaptureTarget::ExcludeProcess(process_id)
      }
    },
    ScreenShareSourceKind::Screen | ScreenShareSourceKind::Webcam => {
      let process_id = unsafe { GetCurrentProcessId() };
      StreamAudioCaptureTarget::ExcludeProcess(process_id)
    }
  }
}

fn find_window_process_id(source_id: u32) -> Result<u32, VideoError> {
  find_desktop_window_process_id(source_id)
    .map_err(|error| VideoError::new(format!("Selected window process is no longer available: {error}")))
}

fn spawn_stream_audio_thread(
  server: Arc<Server>,
  stop: Arc<AtomicBool>,
  target: StreamAudioCaptureTarget,
) -> Result<thread::JoinHandle<()>, VideoError> {
  let (ready_tx, ready_rx) = mpsc::sync_channel(1);
  let thread_stop = Arc::clone(&stop);
  let thread = thread::Builder::new()
    .name("parties-stream-audio-windows".to_owned())
    .spawn(move || {
      if let Err(error) = run_stream_audio_loop(server, thread_stop, target, ready_tx) {
        tracing::warn!(target: "audio::encode::windows", "[audio:encode/windows] stream audio capture disabled: {error}");
      }
    })
    .map_err(|error| VideoError::new(format!("Failed to start stream audio capture thread: {error}")))?;

  match ready_rx.recv_timeout(Duration::from_secs(5)) {
    Ok(Ok(())) => Ok(thread),
    Ok(Err(error)) => {
      stop.store(true, Ordering::Relaxed);
      let _ = thread.join();
      Err(VideoError::new(error))
    }
    Err(error) => {
      stop.store(true, Ordering::Relaxed);
      let _ = thread.join();
      Err(VideoError::new(format!(
        "Timed out waiting for stream audio capture to start: {error}"
      )))
    }
  }
}

fn run_stream_audio_loop(
  server: Arc<Server>,
  stop: Arc<AtomicBool>,
  target: StreamAudioCaptureTarget,
  ready_tx: mpsc::SyncSender<Result<(), String>>,
) -> Result<(), VideoError> {
  tracing::info!(target: "audio::encode::windows", "[audio:encode/windows] opening {}", target.label());
  let _com = match ComSession::start("stream audio") {
    Ok(com) => com,
    Err(error) => {
      let _ = ready_tx.send(Err(error.to_string()));
      return Err(error);
    }
  };
  let capture = match WasapiLoopbackCapture::open(target) {
    Ok(capture) => capture,
    Err(error) => {
      let _ = ready_tx.send(Err(error.to_string()));
      return Err(error);
    }
  };
  let mut encoder = match OpusEncoder::new(STREAM_AUDIO_SAMPLE_RATE, OpusChannels::Stereo, OpusApplication::Audio)
    .map_err(|error| VideoError::new(format!("Failed to create stream audio Opus encoder: {error}")))
  {
    Ok(encoder) => encoder,
    Err(error) => {
      let _ = ready_tx.send(Err(error.to_string()));
      return Err(error);
    }
  };
  if let Err(error) = encoder
    .set_bitrate(OpusBitrate::Bits(STREAM_AUDIO_BITRATE))
    .map_err(|error| VideoError::new(format!("Failed to configure stream audio Opus bitrate: {error}")))
  {
    let _ = ready_tx.send(Err(error.to_string()));
    return Err(error);
  }

  let mut pcm_frame = Vec::with_capacity(STREAM_AUDIO_FRAME_SAMPLES);
  let mut opus_packet = vec![0u8; STREAM_AUDIO_MAX_PACKET_BYTES];
  let mut logged_first_packet = false;

  unsafe {
    if let Err(error) = capture
      .audio_client
      .Start()
      .map_err(|error| VideoError::new(format!("Failed to start stream audio capture: {error}")))
    {
      let _ = ready_tx.send(Err(error.to_string()));
      return Err(error);
    }
  }

  tracing::debug!(target: "audio::encode::windows", "[audio:encode/windows] stream audio capture started");
  let _ = ready_tx.send(Ok(()));
  while !stop.load(Ordering::Relaxed) {
    let wait = unsafe { WaitForSingleObject(capture.event, 100) };
    if wait == WAIT_TIMEOUT {
      continue;
    }
    if wait != WAIT_OBJECT_0 {
      return Err(VideoError::new(format!("Stream audio wait failed: {wait:?}")));
    }
    drain_stream_audio_packets(
      &capture.capture_client,
      &server,
      &mut encoder,
      &mut pcm_frame,
      &mut opus_packet,
      &mut logged_first_packet,
    )?;
  }

  tracing::debug!(target: "audio::encode::windows", "[audio:encode/windows] stream audio capture stopped by request");
  Ok(())
}

fn drain_stream_audio_packets(
  capture_client: &IAudioCaptureClient,
  server: &Server,
  encoder: &mut OpusEncoder,
  pcm_frame: &mut Vec<f32>,
  opus_packet: &mut [u8],
  logged_first_packet: &mut bool,
) -> Result<(), VideoError> {
  loop {
    let packet_frames = unsafe {
      capture_client
        .GetNextPacketSize()
        .map_err(|error| VideoError::new(format!("Failed to query stream audio packet size: {error}")))?
    };
    if packet_frames == 0 {
      return Ok(());
    }

    let mut data = ptr::null_mut::<u8>();
    let mut frames_available = 0u32;
    let mut flags = 0u32;
    unsafe {
      capture_client
        .GetBuffer(&mut data, &mut frames_available, &mut flags, None, None)
        .map_err(|error| VideoError::new(format!("Failed to read stream audio buffer: {error}")))?;
    }

    let result = if flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 != 0 {
      encode_silent_stream_audio_frames(
        frames_available,
        server,
        encoder,
        pcm_frame,
        opus_packet,
        logged_first_packet,
      )
    } else {
      let sample_count = frames_available as usize * STREAM_AUDIO_CHANNELS;
      let samples = unsafe { std::slice::from_raw_parts(data.cast::<f32>(), sample_count) };
      encode_stream_audio_samples(samples, server, encoder, pcm_frame, opus_packet, logged_first_packet)
    };

    unsafe {
      capture_client
        .ReleaseBuffer(frames_available)
        .map_err(|error| VideoError::new(format!("Failed to release stream audio buffer: {error}")))?;
    }
    result?;
  }
}

fn encode_silent_stream_audio_frames(
  frames_available: u32,
  server: &Server,
  encoder: &mut OpusEncoder,
  pcm_frame: &mut Vec<f32>,
  opus_packet: &mut [u8],
  logged_first_packet: &mut bool,
) -> Result<(), VideoError> {
  let mut remaining_samples = frames_available as usize * STREAM_AUDIO_CHANNELS;
  while remaining_samples > 0 {
    let space = STREAM_AUDIO_FRAME_SAMPLES - pcm_frame.len();
    let chunk_samples = space.min(remaining_samples);
    let new_len = pcm_frame.len() + chunk_samples;
    pcm_frame.resize(new_len, 0.0);
    remaining_samples -= chunk_samples;
    flush_stream_audio_frame_if_ready(server, encoder, pcm_frame, opus_packet, logged_first_packet)?;
  }
  Ok(())
}

fn encode_stream_audio_samples(
  samples: &[f32],
  server: &Server,
  encoder: &mut OpusEncoder,
  pcm_frame: &mut Vec<f32>,
  opus_packet: &mut [u8],
  logged_first_packet: &mut bool,
) -> Result<(), VideoError> {
  let mut cursor = 0;
  while cursor < samples.len() {
    let space = STREAM_AUDIO_FRAME_SAMPLES - pcm_frame.len();
    let end = (cursor + space).min(samples.len());
    pcm_frame.extend_from_slice(&samples[cursor..end]);
    cursor = end;
    flush_stream_audio_frame_if_ready(server, encoder, pcm_frame, opus_packet, logged_first_packet)?;
  }
  Ok(())
}

fn flush_stream_audio_frame_if_ready(
  server: &Server,
  encoder: &mut OpusEncoder,
  pcm_frame: &mut Vec<f32>,
  opus_packet: &mut [u8],
  logged_first_packet: &mut bool,
) -> Result<(), VideoError> {
  if pcm_frame.len() < STREAM_AUDIO_FRAME_SAMPLES {
    return Ok(());
  }

  let packet_len = encoder
    .encode_float(pcm_frame, opus_packet)
    .map_err(|error| VideoError::new(format!("Failed to encode stream audio packet: {error}")))?;
  server
    .send_stream_audio(&opus_packet[..packet_len])
    .map_err(|error| VideoError::new(format!("Failed to send stream audio packet: {error}")))?;
  pcm_frame.clear();

  if !*logged_first_packet {
    tracing::debug!(target: "audio::encode::windows", "[audio:encode/windows] first stream audio packet sent: bytes={packet_len}");
    *logged_first_packet = true;
  }

  Ok(())
}

struct WasapiLoopbackCapture {
  audio_client: IAudioClient,
  capture_client: IAudioCaptureClient,
  event: HANDLE,
}

impl WasapiLoopbackCapture {
  fn open(target: StreamAudioCaptureTarget) -> Result<Self, VideoError> {
    let audio_client = activate_loopback_client(target)?;
    let format = stream_audio_wave_format();
    let event = unsafe {
      CreateEventW(None, false, false, PCWSTR::null())
        .map_err(|error| VideoError::new(format!("Failed to create stream audio event: {error}")))?
    };

    let open_result = unsafe {
      audio_client
        .Initialize(
          AUDCLNT_SHAREMODE_SHARED,
          AUDCLNT_STREAMFLAGS_EVENTCALLBACK
            | AUDCLNT_STREAMFLAGS_LOOPBACK
            | AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM
            | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY,
          STREAM_AUDIO_FRAME_DURATION_100NS,
          0,
          &format,
          None,
        )
        .and_then(|()| audio_client.SetEventHandle(event))
        .and_then(|()| audio_client.GetService::<IAudioCaptureClient>())
    };

    match open_result {
      Ok(capture_client) => Ok(Self {
        audio_client,
        capture_client,
        event,
      }),
      Err(error) => {
        unsafe {
          let _ = CloseHandle(event);
        }
        Err(VideoError::new(format!(
          "Failed to initialize stream audio loopback capture: {error}"
        )))
      }
    }
  }
}

fn activate_loopback_client(target: StreamAudioCaptureTarget) -> Result<IAudioClient, VideoError> {
  let (process_id, mode, mode_label) = match target {
    StreamAudioCaptureTarget::ExcludeProcess(process_id) => {
      (process_id, PROCESS_LOOPBACK_MODE_EXCLUDE_TARGET_PROCESS_TREE, "exclude")
    }
    StreamAudioCaptureTarget::IncludeProcess(process_id) => {
      (process_id, PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE, "include")
    }
  };

  match activate_process_loopback_client(process_id, mode) {
    Ok(audio_client) => {
      tracing::info!(target: "audio::encode::windows", "[audio:encode/windows] process loopback capture activated: mode={mode_label} pid={process_id}");
      Ok(audio_client)
    }
    Err(error) => {
      tracing::warn!(target: "audio::encode::windows",
        "[audio:encode/windows] process loopback unavailable; no default output fallback because stream audio must not capture unrelated app audio: mode={mode_label} pid={process_id} error={error}"
      );
      Err(VideoError::new(format!(
        "Process loopback unavailable for stream audio ({mode_label} pid {process_id}); refusing default output fallback because stream audio must not capture unrelated app audio: {error}"
      )))
    }
  }
}

fn activate_process_loopback_client(process_id: u32, mode: PROCESS_LOOPBACK_MODE) -> Result<IAudioClient, VideoError> {
  let activation_params = AUDIOCLIENT_ACTIVATION_PARAMS {
    ActivationType: AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK,
    Anonymous: AUDIOCLIENT_ACTIVATION_PARAMS_0 {
      ProcessLoopbackParams: AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS {
        TargetProcessId: process_id,
        ProcessLoopbackMode: mode,
      },
    },
  };
  let blob_size = std::mem::size_of::<AUDIOCLIENT_ACTIVATION_PARAMS>();
  let blob_data = unsafe { CoTaskMemAlloc(blob_size) };
  if blob_data.is_null() {
    return Err(VideoError::new(
      "Failed to allocate process loopback activation params.",
    ));
  }
  unsafe {
    blob_data
      .cast::<AUDIOCLIENT_ACTIVATION_PARAMS>()
      .write(activation_params);
  }
  let propvariant = Box::leak(Box::new(PROPVARIANT {
    Anonymous: PROPVARIANT_0 {
      Anonymous: ManuallyDrop::new(PROPVARIANT_0_0 {
        vt: VT_BLOB,
        wReserved1: 0,
        wReserved2: 0,
        wReserved3: 0,
        Anonymous: PROPVARIANT_0_0_0 {
          blob: BLOB {
            cbSize: blob_size as u32,
            pBlobData: blob_data.cast::<u8>(),
          },
        },
      }),
    },
  }));

  // The process-loopback activation path may retain this blob beyond the
  // completion callback. Keep the PROPVARIANT and payload alive for the process
  // lifetime; this is one small allocation per stream-audio start.
  unsafe { activate_audio_interface_sync(VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK, Some(propvariant)) }
    .map_err(|error| VideoError::new(format!("Failed to activate process loopback capture: {error}")))
}

unsafe fn activate_audio_interface_sync(
  device_interface_path: PCWSTR,
  activation_params: Option<&PROPVARIANT>,
) -> ::windows::core::Result<IAudioClient> {
  #[::windows::core::implement(IActivateAudioInterfaceCompletionHandler)]
  struct CompletionHandler(std::sync::mpsc::Sender<::windows::core::Result<IUnknown>>);

  fn retrieve_activation_result(
    operation: &IActivateAudioInterfaceAsyncOperation,
  ) -> ::windows::core::Result<IUnknown> {
    let mut result = ::windows::core::HRESULT::default();
    let mut interface: Option<IUnknown> = None;
    unsafe {
      operation.GetActivateResult(&mut result, &mut interface)?;
    }
    result.ok()?;
    interface.ok_or_else(|| {
      ::windows::core::Error::new(
        ::windows::Win32::Media::Audio::AUDCLNT_E_DEVICE_INVALIDATED,
        "audio interface not available after activation",
      )
    })
  }

  impl IActivateAudioInterfaceCompletionHandler_Impl for CompletionHandler_Impl {
    fn ActivateCompleted(
      &self,
      operation: ::windows::core::Ref<'_, IActivateAudioInterfaceAsyncOperation>,
    ) -> ::windows::core::Result<()> {
      let result = operation.ok().and_then(retrieve_activation_result);
      let _ = self.0.send(result);
      Ok(())
    }
  }

  let (tx, rx) = std::sync::mpsc::channel();
  let handler: IActivateAudioInterfaceCompletionHandler = CompletionHandler(tx).into();
  unsafe {
    ActivateAudioInterfaceAsync(
      device_interface_path,
      &IAudioClient::IID,
      activation_params.map(|params| params as *const PROPVARIANT),
      &handler,
    )?;
  }
  let result = rx
    .recv()
    .map_err(|_| ::windows::core::Error::from(::windows::core::HRESULT(0x8000FFFFu32 as i32)))?;
  result?.cast()
}

impl Drop for WasapiLoopbackCapture {
  fn drop(&mut self) {
    unsafe {
      let _ = self.audio_client.Stop();
      let _ = CloseHandle(self.event);
    }
  }
}

fn stream_audio_wave_format() -> WAVEFORMATEX {
  WAVEFORMATEX {
    wFormatTag: WAVE_FORMAT_IEEE_FLOAT as u16,
    nChannels: STREAM_AUDIO_CHANNELS as u16,
    nSamplesPerSec: STREAM_AUDIO_SAMPLE_RATE,
    nAvgBytesPerSec: STREAM_AUDIO_SAMPLE_RATE * STREAM_AUDIO_CHANNELS as u32 * std::mem::size_of::<f32>() as u32,
    nBlockAlign: (STREAM_AUDIO_CHANNELS * std::mem::size_of::<f32>()) as u16,
    wBitsPerSample: 32,
    cbSize: 0,
  }
}

struct CaptureSource {
  kind: CaptureSourceKind,
}

enum CaptureSourceKind {
  Desktop(DesktopCaptureSource),
  Webcam(WebcamCapture),
}

impl CaptureSource {
  fn open(config: &VideoBroadcastConfig) -> Result<Self, VideoError> {
    let kind = match config.source_kind {
      ScreenShareSourceKind::Screen | ScreenShareSourceKind::Window => {
        CaptureSourceKind::Desktop(find_desktop_source(config.source_kind, config.source_id)?)
      }
      ScreenShareSourceKind::Webcam => CaptureSourceKind::Webcam(WebcamCapture::open(
        config.source_id,
        config.output_width,
        config.output_height,
        config.fps,
      )?),
    };
    Ok(Self { kind })
  }

  fn capture_fps(&self) -> Option<u32> {
    match &self.kind {
      CaptureSourceKind::Webcam(webcam) => Some(webcam.fps()).filter(|fps| *fps > 0),
      CaptureSourceKind::Desktop(_) => None,
    }
  }

  fn capture_rgba(&mut self, width: u16, height: u16) -> Result<Vec<u8>, VideoError> {
    let frame = match &mut self.kind {
      CaptureSourceKind::Desktop(source) => source
        .capture_frame()
        .map_err(|error| VideoError::new(format!("Failed to capture desktop frame: {error}")))?,
      CaptureSourceKind::Webcam(webcam) => return webcam.capture_rgba(width, height),
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

enum BroadcastEncoder {
  GpuNvenc(GpuNvencStreamEncoder),
  GpuAmf(GpuAmfStreamEncoder),
  Nvenc(NvencVideoEncoder),
  AmdAmf(AmdAmfVideoEncoder),
}

struct GpuNvencStreamEncoder {
  handle: NonNull<GpuStreamBridge>,
  backend_label: String,
}

struct GpuAmfStreamEncoder {
  handle: NonNull<GpuStreamBridge>,
  backend_label: String,
}

struct NvencVideoEncoder {
  handle: NonNull<NvencBridge>,
  backend_label: String,
}

struct AmdAmfVideoEncoder {
  handle: NonNull<AmfBridge>,
  backend_label: String,
}

struct EncodedSample {
  bytes: Vec<u8>,
  keyframe: bool,
}

impl BroadcastEncoder {
  fn new_excluding(config: &VideoBroadcastConfig, excluded_labels: &[String]) -> Result<Self, VideoError> {
    let output_adapter_vendor_id = windows_output_dxgi_adapter_vendor_id();
    let nvidia_output_adapter = output_adapter_vendor_id == Some(NVIDIA_VENDOR_ID);
    let amd_output_adapter = output_adapter_vendor_id == Some(AMD_VENDOR_ID);
    let nvidia_available = nvidia_output_adapter && has_nvidia_adapter().unwrap_or(true);
    let amd_available = amd_output_adapter && has_amd_adapter().unwrap_or(true);
    let gpu_capture_source = matches!(
      config.source_kind,
      ScreenShareSourceKind::Screen | ScreenShareSourceKind::Window
    );

    if nvidia_available && nvidia_output_adapter && !excluded_labels.iter().any(|label| label == "GPU capture + NVENC")
    {
      match GpuNvencStreamEncoder::new(config) {
        Ok(encoder) => return Ok(Self::GpuNvenc(encoder)),
        Err(error) => {
          tracing::warn!(target: "video::encode::windows", "[video:encode/windows] GPU capture + NVENC unavailable: {error}")
        }
      }
    } else if !nvidia_output_adapter && !excluded_labels.iter().any(|label| label == "GPU capture + NVENC") {
      tracing::warn!(target: "video::encode::windows", "[video:encode/windows] selected/output adapter is not NVIDIA; skipping GPU capture + NVENC");
    } else if !nvidia_available && !excluded_labels.iter().any(|label| label == "GPU capture + NVENC") {
      tracing::warn!(target: "video::encode::windows", "[video:encode/windows] NVIDIA adapter not detected; skipping GPU capture + NVENC");
    }

    if nvidia_available && !excluded_labels.iter().any(|label| label == "NVENC") {
      match NvencVideoEncoder::new(config) {
        Ok(encoder) => return Ok(Self::Nvenc(encoder)),
        Err(error) => {
          tracing::warn!(target: "video::encode::windows", "[video:encode/windows] NVENC unavailable: {error}")
        }
      }
    } else if !nvidia_available && !excluded_labels.iter().any(|label| label == "NVENC") {
      if nvidia_output_adapter {
        tracing::warn!(target: "video::encode::windows", "[video:encode/windows] NVIDIA adapter not detected; skipping NVENC");
      } else {
        tracing::warn!(target: "video::encode::windows", "[video:encode/windows] selected/output adapter is not NVIDIA; skipping NVENC");
      }
    }

    if amd_available && gpu_capture_source && !excluded_labels.iter().any(|label| label == "GPU capture + AMF") {
      match GpuAmfStreamEncoder::new(config) {
        Ok(encoder) => return Ok(Self::GpuAmf(encoder)),
        Err(error) => {
          tracing::warn!(target: "video::encode::windows", "[video:encode/windows] GPU capture + AMF unavailable and CPU desktop capture fallback is disabled: {error}");
          return Err(error);
        }
      }
    } else if amd_available && gpu_capture_source && excluded_labels.iter().any(|label| label == "GPU capture + AMF") {
      return Err(VideoError::new(
        "GPU capture + AMF failed and CPU desktop capture fallback is disabled for AMD screen/window streams.",
      ));
    } else if !amd_available && gpu_capture_source && !excluded_labels.iter().any(|label| label == "GPU capture + AMF")
    {
      if amd_output_adapter {
        tracing::warn!(target: "video::encode::windows", "[video:encode/windows] AMD adapter not detected; skipping GPU capture + AMF");
      } else {
        tracing::warn!(target: "video::encode::windows", "[video:encode/windows] selected/output adapter is not AMD; skipping GPU capture + AMF");
      }
    }

    if amd_available && !excluded_labels.iter().any(|label| label == "AMF") {
      match AmdAmfVideoEncoder::new(config) {
        Ok(encoder) => return Ok(Self::AmdAmf(encoder)),
        Err(error) => {
          tracing::warn!(target: "video::encode::windows", "[video:encode/windows] AMF unavailable: {error}")
        }
      }
    } else if !amd_available && !excluded_labels.iter().any(|label| label == "AMF") {
      if amd_output_adapter {
        tracing::warn!(target: "video::encode::windows", "[video:encode/windows] AMD adapter not detected; skipping AMF");
      } else {
        tracing::warn!(target: "video::encode::windows", "[video:encode/windows] selected/output adapter is not AMD; skipping AMF");
      }
    }

    Err(VideoError::new(format!(
      "No native Windows hardware encoder is available for {} at {}x{}.",
      codec_label(config.codec),
      config.output_width,
      config.output_height
    )))
  }

  fn backend_label(&self) -> &str {
    match self {
      Self::GpuNvenc(encoder) => &encoder.backend_label,
      Self::GpuAmf(encoder) => &encoder.backend_label,
      Self::Nvenc(encoder) => &encoder.backend_label,
      Self::AmdAmf(encoder) => &encoder.backend_label,
    }
  }

  fn backend(&self) -> NativeVideoBackend {
    match self {
      Self::GpuNvenc(_) | Self::Nvenc(_) => NativeVideoBackend::NvidiaNvenc,
      Self::GpuAmf(_) | Self::AmdAmf(_) => NativeVideoBackend::AmdAmf,
    }
  }

  fn owns_capture(&self) -> bool {
    matches!(self, Self::GpuNvenc(_) | Self::GpuAmf(_))
  }

  fn encode(
    &mut self,
    rgba: &[u8],
    frame_number: u32,
    timestamp_100ns: i64,
    _config: &VideoBroadcastConfig,
  ) -> Result<Vec<EncodedSample>, VideoError> {
    match self {
      Self::GpuNvenc(encoder) => {
        let _span = profiler::span("video.encode.gpu_nvenc");
        encoder.poll(frame_number)
      }
      Self::GpuAmf(encoder) => {
        let _span = profiler::span("video.encode.gpu_amf");
        encoder.poll(frame_number)
      }
      Self::Nvenc(encoder) => {
        let _span = profiler::span("video.encode.nvenc");
        encoder.encode(rgba, frame_number, timestamp_100ns)
      }
      Self::AmdAmf(encoder) => {
        let _span = profiler::span("video.encode.amf");
        encoder.encode(rgba, frame_number, timestamp_100ns)
      }
    }
  }

  fn force_keyframe(&mut self) {
    match self {
      Self::GpuNvenc(encoder) => encoder.force_keyframe(),
      Self::GpuAmf(encoder) => encoder.force_keyframe(),
      Self::Nvenc(encoder) => encoder.force_keyframe(),
      Self::AmdAmf(encoder) => encoder.force_keyframe(),
    }
  }
}

fn has_nvidia_adapter() -> Option<bool> {
  has_dxgi_adapter_with_vendor(NVIDIA_VENDOR_ID)
}

fn has_amd_adapter() -> Option<bool> {
  has_dxgi_adapter_with_vendor(AMD_VENDOR_ID)
}

fn windows_output_dxgi_adapter_vendor_id() -> Option<u32> {
  let Ok(factory) = (unsafe { CreateDXGIFactory1::<IDXGIFactory1>() }) else {
    return None;
  };
  let mut index = 0;
  loop {
    let adapter = match unsafe { factory.EnumAdapters1(index) } {
      Ok(adapter) => adapter,
      Err(error) if error.code() == DXGI_ERROR_NOT_FOUND => return None,
      Err(_) => return None,
    };
    let has_output = unsafe { adapter.EnumOutputs(0) }.is_ok();
    if has_output {
      return unsafe { adapter.GetDesc1() }.ok().map(|desc| desc.VendorId);
    }
    index += 1;
  }
}

fn has_dxgi_adapter_with_vendor(vendor_id: u32) -> Option<bool> {
  let Ok(factory) = (unsafe { CreateDXGIFactory1::<IDXGIFactory1>() }) else {
    return None;
  };

  let mut adapter_index = 0;
  loop {
    let adapter = match unsafe { factory.EnumAdapters1(adapter_index) } {
      Ok(adapter) => adapter,
      Err(error) if error.code() == DXGI_ERROR_NOT_FOUND => return Some(false),
      Err(_) => return None,
    };
    if let Ok(desc) = unsafe { adapter.GetDesc1() }
      && desc.VendorId == vendor_id
    {
      return Some(true);
    }
    adapter_index += 1;
  }
}

impl GpuNvencStreamEncoder {
  fn new(config: &VideoBroadcastConfig) -> Result<Self, VideoError> {
    let source_kind = match config.source_kind {
      ScreenShareSourceKind::Screen => 0,
      ScreenShareSourceKind::Window => 1,
      ScreenShareSourceKind::Webcam => {
        return Err(VideoError::new(
          "GPU capture + NVENC is only available for screen and window sources.",
        ));
      }
    };
    let handle = {
      let _span = profiler::span("video.ffi.gpu_stream_create");
      unsafe {
        parties_gpu_stream_create(
          source_kind,
          config.source_id as usize,
          config.codec as u8,
          config.output_width,
          config.output_height,
          config.fps.max(1),
          config.bitrate_kbps.saturating_mul(1000),
        )
      }
    };
    let handle = NonNull::new(handle).ok_or_else(|| {
      VideoError::new(format!(
        "GPU capture + NVENC failed for source {:?}/{} at {}x{}.",
        config.source_kind, config.source_id, config.output_width, config.output_height
      ))
    })?;
    Ok(Self {
      handle,
      backend_label: "GPU capture + NVENC".to_owned(),
    })
  }

  fn poll(&mut self, _frame_number: u32) -> Result<Vec<EncodedSample>, VideoError> {
    let result = {
      let _span = profiler::span("video.ffi.gpu_stream_poll");
      unsafe { parties_gpu_stream_poll(self.handle.as_ptr()) }
    };
    if result < 0 {
      return Err(VideoError::new(
        "GPU capture + NVENC failed while polling encoded frames.",
      ));
    }
    if result == 0 {
      return Ok(Vec::new());
    }

    let _span = profiler::span("video.ffi.gpu_stream_read_encoded");
    let len = unsafe { parties_gpu_stream_encoded_len(self.handle.as_ptr()) };
    let ptr = unsafe { parties_gpu_stream_encoded_ptr(self.handle.as_ptr()) };
    if ptr.is_null() || len == 0 {
      return Ok(Vec::new());
    }

    let bytes = unsafe { std::slice::from_raw_parts(ptr, len) }.to_vec();
    let keyframe = unsafe { parties_gpu_stream_encoded_keyframe(self.handle.as_ptr()) != 0 };
    Ok(vec![EncodedSample { bytes, keyframe }])
  }

  fn force_keyframe(&mut self) {
    unsafe {
      parties_gpu_stream_force_keyframe(self.handle.as_ptr());
    }
  }
}

impl Drop for GpuNvencStreamEncoder {
  fn drop(&mut self) {
    unsafe {
      parties_gpu_stream_destroy(self.handle.as_ptr());
    }
  }
}

impl GpuAmfStreamEncoder {
  fn new(config: &VideoBroadcastConfig) -> Result<Self, VideoError> {
    let source_kind = match config.source_kind {
      ScreenShareSourceKind::Screen => 0,
      ScreenShareSourceKind::Window => 1,
      ScreenShareSourceKind::Webcam => {
        return Err(VideoError::new(
          "GPU capture + AMF is only available for screen and window sources.",
        ));
      }
    };
    let handle = {
      let _span = profiler::span("video.ffi.gpu_amf_stream_create");
      unsafe {
        parties_amf_gpu_stream_create(
          source_kind,
          config.source_id as usize,
          config.codec as u8,
          config.output_width,
          config.output_height,
          config.fps.max(1),
          config.bitrate_kbps.saturating_mul(1000),
        )
      }
    };
    let handle = NonNull::new(handle).ok_or_else(|| {
      VideoError::new(format!(
        "GPU capture + AMF failed for source {:?}/{} at {}x{}.",
        config.source_kind, config.source_id, config.output_width, config.output_height
      ))
    })?;
    Ok(Self {
      handle,
      backend_label: "GPU capture + AMF".to_owned(),
    })
  }

  fn poll(&mut self, _frame_number: u32) -> Result<Vec<EncodedSample>, VideoError> {
    let result = {
      let _span = profiler::span("video.ffi.gpu_amf_stream_poll");
      unsafe { parties_gpu_stream_poll(self.handle.as_ptr()) }
    };
    if result < 0 {
      return Err(VideoError::new(
        "GPU capture + AMF failed while polling encoded frames.",
      ));
    }
    if result == 0 {
      return Ok(Vec::new());
    }

    let _span = profiler::span("video.ffi.gpu_amf_stream_read_encoded");
    let len = unsafe { parties_gpu_stream_encoded_len(self.handle.as_ptr()) };
    let ptr = unsafe { parties_gpu_stream_encoded_ptr(self.handle.as_ptr()) };
    if ptr.is_null() || len == 0 {
      return Ok(Vec::new());
    }

    let bytes = unsafe { std::slice::from_raw_parts(ptr, len) }.to_vec();
    let keyframe = unsafe { parties_gpu_stream_encoded_keyframe(self.handle.as_ptr()) != 0 };
    Ok(vec![EncodedSample { bytes, keyframe }])
  }

  fn force_keyframe(&mut self) {
    unsafe {
      parties_gpu_stream_force_keyframe(self.handle.as_ptr());
    }
  }
}

impl Drop for GpuAmfStreamEncoder {
  fn drop(&mut self) {
    unsafe {
      parties_gpu_stream_destroy(self.handle.as_ptr());
    }
  }
}

impl NvencVideoEncoder {
  fn new(config: &VideoBroadcastConfig) -> Result<Self, VideoError> {
    let handle = {
      let _span = profiler::span("video.ffi.nvenc_create");
      unsafe {
        parties_nvenc_create(
          config.codec as u8,
          config.output_width,
          config.output_height,
          config.fps.max(1),
          config.bitrate_kbps.saturating_mul(1000),
        )
      }
    };
    let handle = NonNull::new(handle).ok_or_else(|| {
      VideoError::new(format!(
        "No NVIDIA NVENC encoder is available for {} at {}x{}.",
        codec_label(config.codec),
        config.output_width,
        config.output_height
      ))
    })?;
    Ok(Self {
      handle,
      backend_label: "NVENC".to_owned(),
    })
  }

  fn encode(
    &mut self,
    rgba: &[u8],
    _frame_number: u32,
    timestamp_100ns: i64,
  ) -> Result<Vec<EncodedSample>, VideoError> {
    let bgra = {
      let _span = profiler::span("video.convert.rgba_to_bgra");
      rgba_to_bgra(rgba)?
    };
    let result = {
      let _span = profiler::span("video.ffi.nvenc_encode_rgba");
      unsafe { parties_nvenc_encode_rgba(self.handle.as_ptr(), bgra.as_ptr(), bgra.len(), timestamp_100ns) }
    };
    if result < 0 {
      return Err(VideoError::new("NVENC failed to encode frame."));
    }
    if result == 0 {
      return Ok(Vec::new());
    }

    let _span = profiler::span("video.ffi.nvenc_read_encoded");
    let len = unsafe { parties_nvenc_encoded_len(self.handle.as_ptr()) };
    let ptr = unsafe { parties_nvenc_encoded_ptr(self.handle.as_ptr()) };
    if ptr.is_null() || len == 0 {
      return Ok(Vec::new());
    }

    let bytes = unsafe { std::slice::from_raw_parts(ptr, len) }.to_vec();
    let keyframe = unsafe { parties_nvenc_encoded_keyframe(self.handle.as_ptr()) != 0 };
    Ok(vec![EncodedSample { bytes, keyframe }])
  }

  fn force_keyframe(&mut self) {
    unsafe {
      parties_nvenc_force_keyframe(self.handle.as_ptr());
    }
  }
}

impl Drop for NvencVideoEncoder {
  fn drop(&mut self) {
    unsafe {
      parties_nvenc_destroy(self.handle.as_ptr());
    }
  }
}

impl AmdAmfVideoEncoder {
  fn new(config: &VideoBroadcastConfig) -> Result<Self, VideoError> {
    let handle = {
      let _span = profiler::span("video.ffi.amf_create");
      unsafe {
        parties_amf_create(
          config.codec as u8,
          config.output_width,
          config.output_height,
          config.fps.max(1),
          config.bitrate_kbps.saturating_mul(1000),
        )
      }
    };
    let handle = NonNull::new(handle).ok_or_else(|| {
      VideoError::new(format!(
        "No AMD AMF encoder is available for {} at {}x{}.",
        codec_label(config.codec),
        config.output_width,
        config.output_height
      ))
    })?;
    Ok(Self {
      handle,
      backend_label: "AMF".to_owned(),
    })
  }

  fn encode(
    &mut self,
    rgba: &[u8],
    _frame_number: u32,
    timestamp_100ns: i64,
  ) -> Result<Vec<EncodedSample>, VideoError> {
    let bgra = {
      let _span = profiler::span("video.convert.rgba_to_bgra");
      rgba_to_bgra(rgba)?
    };
    let result = {
      let _span = profiler::span("video.ffi.amf_encode_bgra");
      unsafe { parties_amf_encode_bgra(self.handle.as_ptr(), bgra.as_ptr(), bgra.len(), timestamp_100ns) }
    };
    if result < 0 {
      return Err(VideoError::new("AMF failed to encode frame."));
    }
    if result == 0 {
      return Ok(Vec::new());
    }

    let _span = profiler::span("video.ffi.amf_read_encoded");
    let len = unsafe { parties_amf_encoded_len(self.handle.as_ptr()) };
    let ptr = unsafe { parties_amf_encoded_ptr(self.handle.as_ptr()) };
    if ptr.is_null() || len == 0 {
      return Ok(Vec::new());
    }

    let bytes = unsafe { std::slice::from_raw_parts(ptr, len) }.to_vec();
    let keyframe = unsafe { parties_amf_encoded_keyframe(self.handle.as_ptr()) != 0 };
    Ok(vec![EncodedSample { bytes, keyframe }])
  }

  fn force_keyframe(&mut self) {
    unsafe {
      parties_amf_force_keyframe(self.handle.as_ptr());
    }
  }
}

impl Drop for AmdAmfVideoEncoder {
  fn drop(&mut self) {
    unsafe {
      parties_amf_destroy(self.handle.as_ptr());
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
      Self::Nvdec(decoder) => decoder.decode_frame(frame, output, output_buffer),
      Self::AmdAmf(decoder) => decoder.decode_frame(frame, output, output_buffer),
      Self::MftH264(decoder) => decoder.decode_frame(frame, output, output_buffer),
      Self::Software(decoder) => decoder.decode_frame(frame, output, output_buffer),
    }
  }

  fn decode_frame_to_dx12(
    &mut self,
    frame: &VideoFrame,
    surface: &lurq::app::dx12_render::Dx12Nv12Surface,
  ) -> Result<bool, VideoError> {
    match self {
      Self::Nvdec(decoder) => decoder.decode_frame_to_dx12(frame, surface),
      Self::AmdAmf(decoder) => decoder.decode_frame_to_dx12(frame, surface),
      Self::MftH264(_) => Ok(false),
      Self::Software(_) => Ok(false),
    }
  }

  fn decode_frame_to_shared_nv12_planes(&mut self, frame: &VideoFrame) -> Result<Option<(usize, usize)>, VideoError> {
    match self {
      Self::Nvdec(_) => Ok(None),
      Self::AmdAmf(decoder) => decoder.decode_frame_to_shared_nv12_planes(frame),
      Self::MftH264(_) => Ok(None),
      Self::Software(_) => Ok(None),
    }
  }
}

impl MftH264VideoDecoder {
  fn new(config: &VideoDecodeConfig) -> Result<Self, VideoError> {
    if config.codec != VideoCodecId::H264 {
      return Err(VideoError::new(format!(
        "Windows Media Foundation H.264 decoder cannot decode {}.",
        codec_label(config.codec)
      )));
    }

    let handle = {
      let _span = profiler::span("video.ffi.mft_h264_decoder_create");
      unsafe { parties_mft_h264_decoder_create(u32::from(config.width), u32::from(config.height)) }
    };
    let handle = NonNull::new(handle).ok_or_else(|| {
      VideoError::new(format!(
        "No Windows Media Foundation H.264 decoder is available at {}x{}.",
        config.width, config.height
      ))
    })?;
    Ok(Self { handle })
  }

  fn decode_frame(
    &mut self,
    frame: &VideoFrame,
    output: bool,
    output_buffer: Option<Vec<u8>>,
  ) -> Result<Option<NativeDecodedVideoFrame>, VideoError> {
    let nv12_len = nv12_len(frame.width, frame.height)?;
    let mut nv12 = if output {
      let mut buffer = output_buffer.unwrap_or_default();
      if buffer.capacity() < nv12_len {
        buffer = Vec::with_capacity(nv12_len);
      }
      buffer
    } else {
      Vec::new()
    };
    let (nv12_ptr, nv12_len) = if output {
      (nv12.as_mut_ptr().cast::<u8>(), nv12_len)
    } else {
      (ptr::null_mut(), 0)
    };
    let mut width_out = 0u32;
    let mut height_out = 0u32;
    let mut error_out = 0u32;
    let status = {
      let _span = profiler::span("video.ffi.mft_h264_decode");
      unsafe {
        parties_mft_h264_decoder_decode(
          self.handle.as_ptr(),
          frame.encoded.as_ptr(),
          frame.encoded.len(),
          i64::from(frame.frame_number),
          i32::from(output),
          nv12_ptr,
          nv12_len,
          &mut width_out,
          &mut height_out,
          &mut error_out,
        )
      }
    };

    if status < 0 {
      return Err(VideoError::new(format!(
        "Windows Media Foundation H.264 decoder failed on frame {}: error=0x{error_out:08x}.",
        frame.frame_number
      )));
    }

    if status == 0 || !output {
      return Ok(None);
    }

    if width_out != u32::from(frame.width) || height_out != u32::from(frame.height) {
      return Err(VideoError::new(format!(
        "Windows Media Foundation H.264 decoder output size changed from {}x{} to {}x{} on frame {}.",
        frame.width, frame.height, width_out, height_out, frame.frame_number
      )));
    }

    unsafe {
      nv12.set_len(nv12_len);
    }
    Ok(Some(NativeDecodedVideoFrame {
      format: DecodedVideoPixelFormat::Nv12,
      pixels: nv12,
      native_image: None,
    }))
  }
}

impl Drop for MftH264VideoDecoder {
  fn drop(&mut self) {
    unsafe {
      parties_mft_h264_decoder_destroy(self.handle.as_ptr());
    }
  }
}

impl AmdAmfVideoDecoder {
  fn new(config: &VideoDecodeConfig) -> Result<Self, VideoError> {
    let handle = {
      let _span = profiler::span("video.ffi.amf_decoder_create");
      unsafe { parties_amf_decoder_create(config.codec as u8, config.width, config.height) }
    };
    let handle = NonNull::new(handle).ok_or_else(|| {
      VideoError::new(format!(
        "No AMD AMF decoder is available for {} at {}x{}.",
        codec_label(config.codec),
        config.width,
        config.height
      ))
    })?;
    Ok(Self { handle })
  }

  fn decode_frame(
    &mut self,
    frame: &VideoFrame,
    output: bool,
    output_buffer: Option<Vec<u8>>,
  ) -> Result<Option<NativeDecodedVideoFrame>, VideoError> {
    let nv12_len = nv12_len(frame.width, frame.height)?;
    let mut nv12 = {
      let _span = profiler::span("video.decode.amf_prepare_output");
      if output {
        let mut buffer = output_buffer.unwrap_or_default();
        if buffer.capacity() < nv12_len {
          buffer = Vec::with_capacity(nv12_len);
        }
        buffer
      } else {
        Vec::new()
      }
    };
    let (nv12_ptr, nv12_len) = if output {
      (nv12.as_mut_ptr().cast::<u8>(), nv12_len)
    } else {
      (ptr::null_mut(), 0)
    };
    let status = {
      let _span = profiler::span("video.ffi.amf_decode");
      unsafe {
        parties_amf_decode(
          self.handle.as_ptr(),
          frame.encoded.as_ptr(),
          frame.encoded.len(),
          i64::from(frame.frame_number),
          nv12_ptr,
          nv12_len,
        )
      }
    };

    if status < 0 {
      return Err(VideoError::new(format!(
        "AMD AMF failed to decode {} frame {}.",
        codec_label(frame.codec),
        frame.frame_number
      )));
    }

    if status == 0 || !output {
      return Ok(None);
    }

    unsafe {
      let _span = profiler::span("video.decode.amf_commit_output");
      nv12.set_len(nv12_len);
    }
    Ok(Some(NativeDecodedVideoFrame {
      format: DecodedVideoPixelFormat::Nv12,
      pixels: nv12,
      native_image: None,
    }))
  }
}

impl Drop for AmdAmfVideoDecoder {
  fn drop(&mut self) {
    unsafe {
      parties_amf_decoder_destroy(self.handle.as_ptr());
    }
  }
}

impl AmdAmfVideoDecoder {
  fn decode_frame_to_shared_nv12_planes(&mut self, frame: &VideoFrame) -> Result<Option<(usize, usize)>, VideoError> {
    let mut y_shared_handle = 0usize;
    let mut uv_shared_handle = 0usize;
    let status = {
      let _span = profiler::span("video.ffi.amf_decode_to_shared_nv12_planes");
      unsafe {
        parties_amf_decode_to_shared_nv12_planes(
          self.handle.as_ptr(),
          frame.encoded.as_ptr(),
          frame.encoded.len(),
          i64::from(frame.frame_number),
          frame.width,
          frame.height,
          &mut y_shared_handle,
          &mut uv_shared_handle,
        )
      }
    };

    if status < 0 {
      return Err(VideoError::new(format!(
        "AMD AMF failed to decode {} frame {} into shared NV12 plane textures.",
        codec_label(frame.codec),
        frame.frame_number
      )));
    }

    if status == 0 || y_shared_handle == 0 || uv_shared_handle == 0 {
      return Ok(None);
    }

    Ok(Some((y_shared_handle, uv_shared_handle)))
  }

  fn decode_frame_to_dx12(
    &mut self,
    frame: &VideoFrame,
    surface: &lurq::app::dx12_render::Dx12Nv12Surface,
  ) -> Result<bool, VideoError> {
    let status = {
      let _span = profiler::span("video.ffi.amf_decode_to_d3d12");
      unsafe {
        parties_amf_decode_to_d3d12(
          self.handle.as_ptr(),
          frame.encoded.as_ptr(),
          frame.encoded.len(),
          i64::from(frame.frame_number),
          surface.y_shared_handle_raw() as usize,
          surface.y_allocation_size(),
          surface.uv_shared_handle_raw() as usize,
          surface.uv_allocation_size(),
          surface.adapter_luid_low(),
          surface.adapter_luid_high(),
          frame.width,
          frame.height,
        )
      }
    };

    if status < 0 {
      return Err(VideoError::new(format!(
        "AMD AMF failed to decode {} frame {} into DX12 surface.",
        codec_label(frame.codec),
        frame.frame_number
      )));
    }

    Ok(status > 0)
  }
}

impl NvdecVideoDecoder {
  fn new(config: &VideoDecodeConfig) -> Result<Self, VideoError> {
    let handle = {
      let _span = profiler::span("video.ffi.nvdec_create");
      unsafe { parties_nvdec_create(config.codec as u8, config.width, config.height) }
    };
    let handle = NonNull::new(handle).ok_or_else(|| {
      VideoError::new(format!(
        "No NVIDIA NVDEC decoder is available for {}.",
        codec_label(config.codec)
      ))
    })?;
    Ok(Self { handle })
  }

  fn decode_frame(
    &mut self,
    frame: &VideoFrame,
    output: bool,
    output_buffer: Option<Vec<u8>>,
  ) -> Result<Option<NativeDecodedVideoFrame>, VideoError> {
    let nv12_len = nv12_len(frame.width, frame.height)?;
    let mut nv12 = if output {
      let mut buffer = output_buffer.unwrap_or_default();
      if buffer.capacity() < nv12_len {
        buffer = Vec::with_capacity(nv12_len);
      }
      buffer
    } else {
      Vec::new()
    };
    let (nv12_ptr, nv12_len) = if output {
      (nv12.as_mut_ptr().cast::<u8>(), nv12_len)
    } else {
      (ptr::null_mut(), 0)
    };
    let status = {
      let _span = profiler::span("video.ffi.nvdec_decode");
      unsafe {
        parties_nvdec_decode(
          self.handle.as_ptr(),
          frame.encoded.as_ptr(),
          frame.encoded.len(),
          i64::from(frame.frame_number),
          nv12_ptr,
          nv12_len,
        )
      }
    };

    if status < 0 {
      return Err(VideoError::new(format!(
        "NVIDIA NVDEC failed to decode {} frame {}.",
        codec_label(frame.codec),
        frame.frame_number
      )));
    }

    if status == 0 || !output {
      return Ok(None);
    }

    unsafe {
      nv12.set_len(nv12_len);
    }
    Ok(Some(NativeDecodedVideoFrame {
      format: DecodedVideoPixelFormat::Nv12,
      pixels: nv12,
      native_image: None,
    }))
  }

  fn decode_frame_to_dx12(
    &mut self,
    frame: &VideoFrame,
    surface: &lurq::app::dx12_render::Dx12Nv12Surface,
  ) -> Result<bool, VideoError> {
    let status = {
      let _span = profiler::span("video.ffi.nvdec_decode_to_d3d12");
      unsafe {
        parties_nvdec_decode_to_d3d12(
          self.handle.as_ptr(),
          frame.encoded.as_ptr(),
          frame.encoded.len(),
          i64::from(frame.frame_number),
          surface.y_shared_handle_raw() as usize,
          surface.y_allocation_size(),
          surface.uv_shared_handle_raw() as usize,
          surface.uv_allocation_size(),
          frame.width,
          frame.height,
        )
      }
    };

    if status < 0 {
      return Err(VideoError::new(format!(
        "NVIDIA NVDEC failed to decode {} frame {} into DX12 surface.",
        codec_label(frame.codec),
        frame.frame_number
      )));
    }

    Ok(status > 0)
  }
}

impl Drop for NvdecVideoDecoder {
  fn drop(&mut self) {
    unsafe {
      parties_nvdec_destroy(self.handle.as_ptr());
    }
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

#[allow(dead_code)]
fn rgba_to_nv12(rgba: &[u8], width: u16, height: u16) -> Result<Vec<u8>, VideoError> {
  let width = usize::from(width);
  let height = usize::from(height);
  if width == 0 || height == 0 || width % 2 != 0 || height % 2 != 0 {
    return Err(VideoError::new("NV12 conversion requires non-zero even dimensions."));
  }

  let pixel_count = width * height;
  if rgba.len() != pixel_count * 4 {
    return Err(VideoError::new("RGBA buffer length does not match frame dimensions."));
  }

  let mut out = vec![0u8; pixel_count + pixel_count / 2];
  let (y_plane, uv_plane) = out.split_at_mut(pixel_count);

  for y in 0..height {
    for x in 0..width {
      let offset = (y * width + x) * 4;
      let r = rgba[offset] as i32;
      let g = rgba[offset + 1] as i32;
      let b = rgba[offset + 2] as i32;
      y_plane[y * width + x] = clamp_video_byte(((66 * r + 129 * g + 25 * b + 128) >> 8) + 16);
    }
  }

  for y in (0..height).step_by(2) {
    for x in (0..width).step_by(2) {
      let mut u_sum = 0i32;
      let mut v_sum = 0i32;
      for dy in 0..2 {
        for dx in 0..2 {
          let offset = ((y + dy) * width + (x + dx)) * 4;
          let r = rgba[offset] as i32;
          let g = rgba[offset + 1] as i32;
          let b = rgba[offset + 2] as i32;
          u_sum += ((-38 * r - 74 * g + 112 * b + 128) >> 8) + 128;
          v_sum += ((112 * r - 94 * g - 18 * b + 128) >> 8) + 128;
        }
      }

      let uv_offset = (y / 2) * width + x;
      uv_plane[uv_offset] = clamp_video_byte(u_sum / 4);
      uv_plane[uv_offset + 1] = clamp_video_byte(v_sum / 4);
    }
  }

  Ok(out)
}

fn rgba_to_bgra(rgba: &[u8]) -> Result<Vec<u8>, VideoError> {
  if rgba.len() % 4 != 0 {
    return Err(VideoError::new("RGBA buffer length is not pixel aligned."));
  }

  let mut bgra = Vec::with_capacity(rgba.len());
  for pixel in rgba.chunks_exact(4) {
    bgra.push(pixel[2]);
    bgra.push(pixel[1]);
    bgra.push(pixel[0]);
    bgra.push(pixel[3]);
  }
  Ok(bgra)
}

fn nv12_len(width: u16, height: u16) -> Result<usize, VideoError> {
  let width = usize::from(width);
  let height = usize::from(height);
  if width == 0 || height == 0 || width % 2 != 0 || height % 2 != 0 {
    return Err(VideoError::new("NV12 conversion requires non-zero even dimensions."));
  }
  Ok(width * height + width * height / 2)
}

#[cfg(test)]
fn nv12_to_rgba(nv12: &[u8], width: u16, height: u16) -> Result<Vec<u8>, VideoError> {
  let width = usize::from(width);
  let height = usize::from(height);
  let expected_len = nv12_len(width as u16, height as u16)?;
  if nv12.len() < expected_len {
    return Err(VideoError::new(format!(
      "NV12 buffer is too short: {} bytes, expected at least {expected_len}.",
      nv12.len()
    )));
  }

  let (y_plane, uv_plane) = nv12.split_at(width * height);
  let mut rgba = vec![0u8; width * height * 4];
  for y in 0..height {
    for x in 0..width {
      let y_value = y_plane[y * width + x] as i32;
      let uv_offset = (y / 2) * width + (x & !1);
      let u = uv_plane[uv_offset] as i32;
      let v = uv_plane[uv_offset + 1] as i32;
      let c = (y_value - 16).max(0);
      let d = u - 128;
      let e = v - 128;
      let offset = (y * width + x) * 4;
      rgba[offset] = clamp_video_byte((298 * c + 409 * e + 128) >> 8);
      rgba[offset + 1] = clamp_video_byte((298 * c - 100 * d - 208 * e + 128) >> 8);
      rgba[offset + 2] = clamp_video_byte((298 * c + 516 * d + 128) >> 8);
      rgba[offset + 3] = 255;
    }
  }

  Ok(rgba)
}

#[allow(dead_code)]
fn clamp_video_byte(value: i32) -> u8 {
  value.clamp(0, 255) as u8
}

struct ComSession {
  initialized: bool,
}

impl ComSession {
  fn start(label: &str) -> Result<Self, VideoError> {
    let initialized = unsafe {
      let result = CoInitializeEx(None, COINIT_MULTITHREADED);
      if result == RPC_E_CHANGED_MODE {
        false
      } else if result.is_ok() {
        true
      } else {
        return Err(VideoError::new(format!(
          "Failed to initialize COM for {label}: {result:?}"
        )));
      }
    };

    Ok(Self { initialized })
  }
}

impl Drop for ComSession {
  fn drop(&mut self) {
    if self.initialized {
      unsafe {
        CoUninitialize();
      }
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn windows_broadcast_backend_order_prefers_native_hardware() {
    assert_eq!(
      BACKEND_ORDER,
      [
        NativeVideoBackend::NvidiaNvenc,
        NativeVideoBackend::AmdAmf,
        NativeVideoBackend::OpenH264,
      ]
    );
    assert_eq!(backend_order_label(), "NVENC -> AMF -> OpenH264");
  }

  #[test]
  fn rgba_to_nv12_converts_black_frame_to_video_range_neutral_chroma() {
    let rgba = [0, 0, 0, 255].repeat(4);
    let nv12 = rgba_to_nv12(&rgba, 2, 2).unwrap();

    assert_eq!(&nv12[..4], &[16, 16, 16, 16]);
    assert_eq!(&nv12[4..], &[128, 128]);
  }

  #[test]
  fn rgba_to_nv12_converts_white_frame_to_video_range_neutral_chroma() {
    let rgba = [255, 255, 255, 255].repeat(4);
    let nv12 = rgba_to_nv12(&rgba, 2, 2).unwrap();

    assert_eq!(&nv12[..4], &[235, 235, 235, 235]);
    assert_eq!(&nv12[4..], &[128, 128]);
  }

  #[test]
  fn rgba_to_nv12_rejects_odd_dimensions() {
    let rgba = vec![0; 3 * 2 * 4];
    let error = rgba_to_nv12(&rgba, 3, 2).unwrap_err();

    assert_eq!(error.to_string(), "NV12 conversion requires non-zero even dimensions.");
  }

  #[test]
  fn rgba_to_bgra_swaps_red_and_blue() {
    let rgba = vec![10, 20, 30, 255, 40, 50, 60, 128];
    let bgra = rgba_to_bgra(&rgba).unwrap();

    assert_eq!(bgra, vec![30, 20, 10, 255, 60, 50, 40, 128]);
  }

  #[test]
  fn normalize_rgba_frame_resizes_to_output_dimensions() {
    let rgba = vec![
      1, 2, 3, 255, 4, 5, 6, 255, 7, 8, 9, 255, 10, 11, 12, 255, 13, 14, 15, 255, 16, 17, 18, 255, 19, 20, 21, 255, 22,
      23, 24, 255,
    ];

    let resized = normalize_rgba_frame(rgba, 4, 2, 2, 1).unwrap();

    assert_eq!(resized, vec![1, 2, 3, 255, 7, 8, 9, 255]);
  }

  #[test]
  fn nv12_to_rgba_converts_black_frame() {
    let nv12 = [16, 16, 16, 16, 128, 128].to_vec();
    let rgba = nv12_to_rgba(&nv12, 2, 2).unwrap();

    assert_eq!(rgba, [0, 0, 0, 255].repeat(4));
  }

  #[test]
  fn nv12_to_rgba_converts_white_frame() {
    let nv12 = [235, 235, 235, 235, 128, 128].to_vec();
    let rgba = nv12_to_rgba(&nv12, 2, 2).unwrap();

    assert_eq!(rgba, [255, 255, 255, 255].repeat(4));
  }
}
