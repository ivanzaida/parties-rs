use std::{
  ffi::c_void,
  mem::ManuallyDrop,
  ptr::{self, NonNull},
  sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
  },
  thread,
  time::{Duration, Instant},
};

use ::windows::{
  Win32::{
    Foundation::RPC_E_CHANGED_MODE,
    Media::MediaFoundation::{
      IMFActivate, IMFMediaBuffer, IMFSample, IMFTransform, MF_E_TRANSFORM_NEED_MORE_INPUT,
      MF_E_TRANSFORM_STREAM_CHANGE, MF_MT_AVG_BITRATE, MF_MT_FRAME_RATE, MF_MT_FRAME_SIZE, MF_MT_INTERLACE_MODE,
      MF_MT_MAJOR_TYPE, MF_MT_SUBTYPE, MF_VERSION, MFCreateMediaType, MFCreateMemoryBuffer, MFCreateSample,
      MFMediaType_Video, MFSTARTUP_NOSOCKET, MFShutdown, MFStartup, MFT_CATEGORY_VIDEO_DECODER,
      MFT_CATEGORY_VIDEO_ENCODER, MFT_ENUM_FLAG, MFT_ENUM_FLAG_ALL, MFT_ENUM_FLAG_SORTANDFILTER,
      MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, MFT_MESSAGE_NOTIFY_START_OF_STREAM, MFT_OUTPUT_DATA_BUFFER,
      MFT_OUTPUT_STREAM_PROVIDES_SAMPLES, MFT_REGISTER_TYPE_INFO, MFTEnumEx, MFVideoFormat_AV1, MFVideoFormat_H264,
      MFVideoFormat_HEVC, MFVideoFormat_NV12, MFVideoInterlace_Progressive,
    },
    System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoTaskMemFree, CoUninitialize},
  },
  core::{Error as WindowsError, GUID},
};
use xcap::{Monitor, Window};

use super::{
  DecodedVideoPixelFormat, NativeDecodedVideoFrame, NativeVideoBackend, VideoBroadcast, VideoBroadcastConfig,
  VideoDecodeConfig, VideoDecoder, VideoError,
};
use crate::{
  network::{
    protocol::{VideoCodecId, VideoFrame},
    server::Server,
  },
  services::{logger, screen_share_sources::ScreenShareSourceKind},
};

#[allow(dead_code)]
const BACKEND_ORDER: [NativeVideoBackend; 4] = [
  NativeVideoBackend::NvidiaNvenc,
  NativeVideoBackend::AmdAmf,
  NativeVideoBackend::WindowsMediaFoundation,
  NativeVideoBackend::OpenH264,
];

#[repr(C)]
struct NvdecBridge {
  _private: [u8; 0],
}

unsafe extern "C" {
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
}

pub(super) fn encode(server: Arc<Server>, config: VideoBroadcastConfig) -> Result<VideoBroadcast, VideoError> {
  if config.source_width != config.output_width || config.source_height != config.output_height {
    return Err(VideoError::new(
      "Windows native video scaling is not wired yet. Set stream scale to 100%.",
    ));
  }

  if !media_foundation_encoder_available(config.codec)? {
    return Err(VideoError::new(format!(
      "No Windows Media Foundation encoder is available for {}.",
      codec_label(config.codec)
    )));
  }

  let runtime = tokio::runtime::Handle::try_current()
    .map_err(|_| VideoError::new("Video broadcasting must be started from the Tokio runtime."))?;
  let stop = Arc::new(AtomicBool::new(false));
  let thread_stop = Arc::clone(&stop);
  let thread = thread::Builder::new()
    .name("parties-video-windows-encode".to_owned())
    .spawn(move || {
      if let Err(error) = run_broadcast_loop(server, config, runtime, thread_stop) {
        logger::log(&format!("[video/windows] broadcast loop stopped with error: {error}"));
      }
    })
    .map_err(|error| VideoError::new(format!("Failed to start video broadcast thread: {error}")))?;

  Ok(VideoBroadcast::from_parts_with_stop(
    NativeVideoBackend::WindowsMediaFoundation,
    stop,
    vec![thread],
  ))
}

pub(super) enum NativeVideoDecoder {
  Nvdec(NvdecVideoDecoder),
  MediaFoundation(MediaFoundationVideoDecoder),
}

pub(super) struct NvdecVideoDecoder {
  handle: NonNull<NvdecBridge>,
}

pub(super) struct MediaFoundationVideoDecoder {
  _mf: MediaFoundationSession,
  transform: IMFTransform,
  output_provides_samples: bool,
  output_buffer_size: u32,
  width: u16,
  height: u16,
}

pub(super) fn decode(config: VideoDecodeConfig) -> Result<VideoDecoder, VideoError> {
  match NvdecVideoDecoder::new(&config) {
    Ok(decoder) => {
      logger::log(&format!(
        "[video/windows] decoder ready through NVDEC: codec={:?} size={}x{}",
        config.codec, config.width, config.height
      ));
      Ok(VideoDecoder::from_windows(
        NativeVideoDecoder::Nvdec(decoder),
        config,
        NativeVideoBackend::NvidiaNvdec,
      ))
    }
    Err(nvdec_error) => {
      logger::log(&format!("[video/windows] NVDEC decoder unavailable: {nvdec_error}"));
      let decoder = MediaFoundationVideoDecoder::new(&config)?;
      Ok(VideoDecoder::from_windows(
        NativeVideoDecoder::MediaFoundation(decoder),
        config,
        NativeVideoBackend::WindowsMediaFoundation,
      ))
    }
  }
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
      NativeVideoBackend::AppleVideoToolbox => "VideoToolbox",
    })
    .collect::<Vec<_>>()
    .join(" -> ")
}

fn media_foundation_encoder_available(codec: VideoCodecId) -> Result<bool, VideoError> {
  let _mf = MediaFoundationSession::start()?;
  let subtype = codec_subtype(codec)?;
  let output_type = MFT_REGISTER_TYPE_INFO {
    guidMajorType: MFMediaType_Video,
    guidSubtype: subtype,
  };
  let mut activates: *mut Option<IMFActivate> = ptr::null_mut();
  let mut count = 0u32;
  let flags = MFT_ENUM_FLAG(MFT_ENUM_FLAG_ALL.0 | MFT_ENUM_FLAG_SORTANDFILTER.0);

  let result = unsafe {
    MFTEnumEx(
      MFT_CATEGORY_VIDEO_ENCODER,
      flags,
      None,
      Some(&output_type),
      &mut activates,
      &mut count,
    )
  };

  if !activates.is_null() {
    unsafe {
      release_activates(activates, count);
    }
  }

  match result {
    Ok(()) => Ok(count > 0),
    Err(error) => Err(VideoError::new(format!(
      "Failed to query Windows Media Foundation encoders: {error}"
    ))),
  }
}

fn run_broadcast_loop(
  server: Arc<Server>,
  config: VideoBroadcastConfig,
  runtime: tokio::runtime::Handle,
  stop: Arc<AtomicBool>,
) -> Result<(), VideoError> {
  logger::log("[video/windows] opening capture source");
  let mut source = CaptureSource::open(&config)?;
  logger::log("[video/windows] creating Media Foundation encoder");
  let mut encoder = MftEncoder::new(&config)?;
  logger::log(&format!(
    "[video/windows] encoder ready: codec={:?} size={}x{} fps={} bitrate={}kbps",
    config.codec, config.output_width, config.output_height, config.fps, config.bitrate_kbps
  ));
  let frame_interval = Duration::from_nanos(1_000_000_000u64 / u64::from(config.fps.max(1)));
  let started_at = Instant::now();
  let mut frame_number = 0u32;
  let mut logged_first_frame = false;

  while !stop.load(Ordering::Relaxed) {
    let loop_started_at = Instant::now();
    let rgba = source.capture_rgba(config.output_width, config.output_height)?;
    let nv12 = rgba_to_nv12(&rgba, config.output_width, config.output_height)?;
    let timestamp_100ns = started_at.elapsed().as_nanos().saturating_div(100) as i64;
    let timestamp_ms = started_at.elapsed().as_millis().min(u128::from(u32::MAX)) as u32;
    let samples = encoder.encode(&nv12, frame_number, timestamp_100ns)?;

    for sample in samples {
      let sample_len = sample.bytes.len();
      let sample_keyframe = sample.keyframe;
      runtime
        .block_on(server.send_video_frame(VideoFrame {
          frame_number,
          timestamp: timestamp_ms,
          keyframe: sample_keyframe,
          width: config.output_width,
          height: config.output_height,
          codec: config.codec,
          encoded: sample.bytes,
        }))
        .map_err(|error| VideoError::new(format!("Failed to send video frame: {error}")))?;
      if !logged_first_frame {
        logger::log(&format!(
          "[video/windows] first encoded frame sent: frame={} bytes={} keyframe={}",
          frame_number, sample_len, sample_keyframe
        ));
        logged_first_frame = true;
      }
    }

    frame_number = frame_number.wrapping_add(1);
    let elapsed = loop_started_at.elapsed();
    if elapsed < frame_interval {
      thread::sleep(frame_interval - elapsed);
    }
  }

  logger::log("[video/windows] broadcast loop stopped by request");
  Ok(())
}

struct CaptureSource {
  kind: CaptureSourceKind,
}

enum CaptureSourceKind {
  Screen(Monitor),
  Window(Window),
}

impl CaptureSource {
  fn open(config: &VideoBroadcastConfig) -> Result<Self, VideoError> {
    let kind = match config.source_kind {
      ScreenShareSourceKind::Screen => CaptureSourceKind::Screen(find_monitor(config.source_id)?),
      ScreenShareSourceKind::Window => CaptureSourceKind::Window(find_window(config.source_id)?),
    };
    Ok(Self { kind })
  }

  fn capture_rgba(&mut self, width: u16, height: u16) -> Result<Vec<u8>, VideoError> {
    let image = match &self.kind {
      CaptureSourceKind::Screen(monitor) => monitor
        .capture_image()
        .map_err(|error| VideoError::new(format!("Failed to capture monitor frame: {error}")))?,
      CaptureSourceKind::Window(window) => window
        .capture_image()
        .map_err(|error| VideoError::new(format!("Failed to capture window frame: {error}")))?,
    };

    let frame_width = image.width();
    let frame_height = image.height();
    normalize_rgba_frame(image.into_raw(), frame_width, frame_height, width, height)
  }
}

fn find_monitor(source_id: u32) -> Result<Monitor, VideoError> {
  Monitor::all()
    .map_err(|error| VideoError::new(format!("Failed to list monitors: {error}")))?
    .into_iter()
    .find(|monitor| monitor.id().ok() == Some(source_id))
    .ok_or_else(|| VideoError::new("Selected monitor is no longer available."))
}

fn find_window(source_id: u32) -> Result<Window, VideoError> {
  Window::all()
    .map_err(|error| VideoError::new(format!("Failed to list windows: {error}")))?
    .into_iter()
    .find(|window| window.id().ok() == Some(source_id))
    .ok_or_else(|| VideoError::new("Selected window is no longer available."))
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

  if frame_width < output_width || frame_height < output_height {
    return Err(VideoError::new(format!(
      "Captured frame is {}x{}, expected at least {}x{}.",
      frame_width, frame_height, output_width, output_height
    )));
  }

  let src_stride = frame_width as usize * 4;
  let dst_stride = output_width as usize * 4;
  let mut out = vec![0u8; dst_stride * output_height as usize];
  for row in 0..output_height as usize {
    let src_start = row * src_stride;
    let dst_start = row * dst_stride;
    out[dst_start..dst_start + dst_stride].copy_from_slice(&rgba[src_start..src_start + dst_stride]);
  }
  Ok(out)
}

struct MftEncoder {
  _mf: MediaFoundationSession,
  transform: IMFTransform,
  output_provides_samples: bool,
  output_buffer_size: u32,
  frame_duration_100ns: i64,
  sent_first_sample: bool,
}

struct EncodedSample {
  bytes: Vec<u8>,
  keyframe: bool,
}

impl MftEncoder {
  fn new(config: &VideoBroadcastConfig) -> Result<Self, VideoError> {
    let mf = MediaFoundationSession::start()?;
    let transform = activate_encoder_transform(config.codec)?;
    let output_type = create_video_type(
      codec_subtype(config.codec)?,
      config.output_width,
      config.output_height,
      config.fps,
      Some(config.bitrate_kbps.saturating_mul(1000)),
    )?;
    let input_type = create_video_type(
      MFVideoFormat_NV12,
      config.output_width,
      config.output_height,
      config.fps,
      None,
    )?;

    unsafe {
      transform
        .SetOutputType(0, &output_type, 0)
        .map_err(|error| VideoError::new(format!("Failed to set encoder output type: {error}")))?;
      transform
        .SetInputType(0, &input_type, 0)
        .map_err(|error| VideoError::new(format!("Failed to set encoder input type: {error}")))?;
      transform
        .ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0)
        .map_err(|error| VideoError::new(format!("Failed to begin encoder streaming: {error}")))?;
      transform
        .ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0)
        .map_err(|error| VideoError::new(format!("Failed to start encoder stream: {error}")))?;
    }

    let output_info = unsafe {
      transform
        .GetOutputStreamInfo(0)
        .map_err(|error| VideoError::new(format!("Failed to query encoder output stream: {error}")))?
    };

    Ok(Self {
      _mf: mf,
      transform,
      output_provides_samples: output_info.dwFlags & MFT_OUTPUT_STREAM_PROVIDES_SAMPLES.0 as u32 != 0,
      output_buffer_size: output_info.cbSize.max(1024 * 1024),
      frame_duration_100ns: 10_000_000i64 / i64::from(config.fps.max(1)),
      sent_first_sample: false,
    })
  }

  fn encode(&mut self, nv12: &[u8], frame_number: u32, timestamp_100ns: i64) -> Result<Vec<EncodedSample>, VideoError> {
    let input = sample_from_bytes(nv12, timestamp_100ns, self.frame_duration_100ns)?;
    unsafe {
      self
        .transform
        .ProcessInput(0, &input, 0)
        .map_err(|error| VideoError::new(format!("Failed to submit encoder input frame: {error}")))?;
    }

    let mut samples = Vec::new();
    loop {
      let Some(sample) = self.process_output()? else {
        break;
      };
      let bytes = bytes_from_sample(&sample)?;
      if bytes.is_empty() {
        continue;
      }
      let keyframe = !self.sent_first_sample || frame_number == 0;
      self.sent_first_sample = true;
      samples.push(EncodedSample { bytes, keyframe });
    }

    Ok(samples)
  }

  fn process_output(&mut self) -> Result<Option<IMFSample>, VideoError> {
    let mut output = MFT_OUTPUT_DATA_BUFFER::default();
    if !self.output_provides_samples {
      output.pSample = ManuallyDrop::new(Some(create_empty_output_sample(self.output_buffer_size)?));
    }

    let mut status = 0u32;
    let result = unsafe {
      self
        .transform
        .ProcessOutput(0, std::slice::from_mut(&mut output), &mut status)
    };

    match result {
      Ok(()) => {
        let sample = unsafe { ManuallyDrop::take(&mut output.pSample) };
        let events = unsafe { ManuallyDrop::take(&mut output.pEvents) };
        drop(events);
        Ok(sample)
      }
      Err(error) if error.code() == MF_E_TRANSFORM_NEED_MORE_INPUT => {
        unsafe {
          drop(ManuallyDrop::take(&mut output.pSample));
          drop(ManuallyDrop::take(&mut output.pEvents));
        }
        Ok(None)
      }
      Err(error) if error.code() == MF_E_TRANSFORM_STREAM_CHANGE => Err(VideoError::new(
        "Encoder output stream changed; dynamic video format changes are not wired yet.",
      )),
      Err(error) => {
        unsafe {
          drop(ManuallyDrop::take(&mut output.pSample));
          drop(ManuallyDrop::take(&mut output.pEvents));
        }
        Err(VideoError::new(format!("Failed to read encoder output: {error}")))
      }
    }
  }
}

impl NativeVideoDecoder {
  pub(super) fn decode_frame(
    &mut self,
    frame: &VideoFrame,
    output: bool,
    output_buffer: Option<Vec<u8>>,
  ) -> Result<Option<NativeDecodedVideoFrame>, VideoError> {
    match self {
      Self::Nvdec(decoder) => decoder.decode_frame(frame, output, output_buffer),
      Self::MediaFoundation(decoder) => decoder.decode_frame(frame, output, output_buffer),
    }
  }

  pub(super) fn decode_frame_to_dx12(
    &mut self,
    frame: &VideoFrame,
    surface: &lurq::app::dx12_render::Dx12Nv12Surface,
  ) -> Result<bool, VideoError> {
    match self {
      Self::Nvdec(decoder) => decoder.decode_frame_to_dx12(frame, surface),
      Self::MediaFoundation(_) => Ok(false),
    }
  }
}

impl NvdecVideoDecoder {
  fn new(config: &VideoDecodeConfig) -> Result<Self, VideoError> {
    let handle = unsafe { parties_nvdec_create(config.codec as u8, config.width, config.height) };
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
    let status = unsafe {
      parties_nvdec_decode(
        self.handle.as_ptr(),
        frame.encoded.as_ptr(),
        frame.encoded.len(),
        i64::from(frame.frame_number),
        nv12_ptr,
        nv12_len,
      )
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
    let status = unsafe {
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

impl MediaFoundationVideoDecoder {
  fn new(config: &VideoDecodeConfig) -> Result<Self, VideoError> {
    let mf = MediaFoundationSession::start()?;
    let transform = activate_decoder_transform(config.codec)?;
    let input_type = create_video_type(codec_subtype(config.codec)?, config.width, config.height, 30, None)?;
    let output_type = create_video_type(MFVideoFormat_NV12, config.width, config.height, 30, None)?;

    unsafe {
      transform
        .SetInputType(0, &input_type, 0)
        .map_err(|error| VideoError::new(format!("Failed to set decoder input type: {error}")))?;
      transform
        .SetOutputType(0, &output_type, 0)
        .map_err(|error| VideoError::new(format!("Failed to set decoder output type: {error}")))?;
      transform
        .ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0)
        .map_err(|error| VideoError::new(format!("Failed to begin decoder streaming: {error}")))?;
      transform
        .ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0)
        .map_err(|error| VideoError::new(format!("Failed to start decoder stream: {error}")))?;
    }

    let output_info = unsafe {
      transform
        .GetOutputStreamInfo(0)
        .map_err(|error| VideoError::new(format!("Failed to query decoder output stream: {error}")))?
    };
    let nv12_size = nv12_len(config.width, config.height)? as u32;

    Ok(Self {
      _mf: mf,
      transform,
      output_provides_samples: output_info.dwFlags & MFT_OUTPUT_STREAM_PROVIDES_SAMPLES.0 as u32 != 0,
      output_buffer_size: output_info.cbSize.max(nv12_size),
      width: config.width,
      height: config.height,
    })
  }

  pub(super) fn decode_frame(
    &mut self,
    frame: &VideoFrame,
    output: bool,
    output_buffer: Option<Vec<u8>>,
  ) -> Result<Option<NativeDecodedVideoFrame>, VideoError> {
    let _ = output_buffer;
    let timestamp_100ns = i64::from(frame.timestamp) * 10_000;
    let input = sample_from_bytes(&frame.encoded, timestamp_100ns, 333_333)?;
    unsafe {
      self
        .transform
        .ProcessInput(0, &input, 0)
        .map_err(|error| VideoError::new(format!("Failed to submit decoder input frame: {error}")))?;
    }

    let mut decoded = None;
    loop {
      let Some(sample) = self.process_output()? else {
        break;
      };
      let nv12 = bytes_from_sample(&sample)?;
      if nv12.is_empty() {
        continue;
      }
      if output {
        decoded = Some(NativeDecodedVideoFrame {
          format: DecodedVideoPixelFormat::Rgba8,
          pixels: nv12_to_rgba(&nv12, self.width, self.height)?,
          native_image: None,
        });
      }
    }

    Ok(decoded)
  }

  fn process_output(&mut self) -> Result<Option<IMFSample>, VideoError> {
    let mut output = MFT_OUTPUT_DATA_BUFFER::default();
    if !self.output_provides_samples {
      output.pSample = ManuallyDrop::new(Some(create_empty_output_sample(self.output_buffer_size)?));
    }

    let mut status = 0u32;
    let result = unsafe {
      self
        .transform
        .ProcessOutput(0, std::slice::from_mut(&mut output), &mut status)
    };

    match result {
      Ok(()) => {
        let sample = unsafe { ManuallyDrop::take(&mut output.pSample) };
        let events = unsafe { ManuallyDrop::take(&mut output.pEvents) };
        drop(events);
        Ok(sample)
      }
      Err(error) if error.code() == MF_E_TRANSFORM_NEED_MORE_INPUT => {
        unsafe {
          drop(ManuallyDrop::take(&mut output.pSample));
          drop(ManuallyDrop::take(&mut output.pEvents));
        }
        Ok(None)
      }
      Err(error) if error.code() == MF_E_TRANSFORM_STREAM_CHANGE => Err(VideoError::new(
        "Decoder output stream changed; dynamic video format changes are not wired yet.",
      )),
      Err(error) => {
        unsafe {
          drop(ManuallyDrop::take(&mut output.pSample));
          drop(ManuallyDrop::take(&mut output.pEvents));
        }
        Err(VideoError::new(format!("Failed to read decoder output: {error}")))
      }
    }
  }
}

fn activate_encoder_transform(codec: VideoCodecId) -> Result<IMFTransform, VideoError> {
  let subtype = codec_subtype(codec)?;
  let output_type = MFT_REGISTER_TYPE_INFO {
    guidMajorType: MFMediaType_Video,
    guidSubtype: subtype,
  };

  let mut activates: *mut Option<IMFActivate> = ptr::null_mut();
  let mut count = 0u32;
  let flags = MFT_ENUM_FLAG(MFT_ENUM_FLAG_ALL.0 | MFT_ENUM_FLAG_SORTANDFILTER.0);

  unsafe {
    MFTEnumEx(
      MFT_CATEGORY_VIDEO_ENCODER,
      flags,
      None,
      Some(&output_type),
      &mut activates,
      &mut count,
    )
    .map_err(|error| VideoError::new(format!("Failed to enumerate encoder transforms: {error}")))?;
  }

  if activates.is_null() || count == 0 {
    return Err(VideoError::new(format!(
      "No Windows Media Foundation encoder is available for {}.",
      codec_label(codec)
    )));
  }

  let mut transform = None;
  unsafe {
    for index in 0..count as usize {
      let activate = ptr::read(activates.add(index));
      if transform.is_none() {
        if let Some(activate) = &activate {
          transform = activate.ActivateObject::<IMFTransform>().ok();
        }
      }
      drop(activate);
    }
    CoTaskMemFree(Some(activates.cast::<c_void>()));
  }

  transform.ok_or_else(|| {
    VideoError::new(format!(
      "Windows Media Foundation encoder activation failed for {}.",
      codec_label(codec)
    ))
  })
}

fn activate_decoder_transform(codec: VideoCodecId) -> Result<IMFTransform, VideoError> {
  let subtype = codec_subtype(codec)?;
  let input_type = MFT_REGISTER_TYPE_INFO {
    guidMajorType: MFMediaType_Video,
    guidSubtype: subtype,
  };
  let mut activates: *mut Option<IMFActivate> = ptr::null_mut();
  let mut count = 0u32;
  let flags = MFT_ENUM_FLAG(MFT_ENUM_FLAG_ALL.0 | MFT_ENUM_FLAG_SORTANDFILTER.0);

  unsafe {
    MFTEnumEx(
      MFT_CATEGORY_VIDEO_DECODER,
      flags,
      Some(&input_type),
      None,
      &mut activates,
      &mut count,
    )
    .map_err(|error| VideoError::new(format!("Failed to enumerate decoder transforms: {error}")))?;
  }

  if activates.is_null() || count == 0 {
    return Err(VideoError::new(format!(
      "No Windows Media Foundation decoder is available for {}.",
      codec_label(codec)
    )));
  }

  let mut transform = None;
  unsafe {
    for index in 0..count as usize {
      let activate = ptr::read(activates.add(index));
      if transform.is_none() {
        if let Some(activate) = &activate {
          transform = activate.ActivateObject::<IMFTransform>().ok();
        }
      }
      drop(activate);
    }
    CoTaskMemFree(Some(activates.cast::<c_void>()));
  }

  transform.ok_or_else(|| {
    VideoError::new(format!(
      "Windows Media Foundation decoder activation failed for {}.",
      codec_label(codec)
    ))
  })
}

fn create_video_type(
  subtype: GUID,
  width: u16,
  height: u16,
  fps: u32,
  bitrate: Option<u32>,
) -> Result<::windows::Win32::Media::MediaFoundation::IMFMediaType, VideoError> {
  let media_type = unsafe { MFCreateMediaType().map_err(mf_error("Failed to create media type"))? };
  unsafe {
    media_type
      .SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)
      .map_err(mf_error("Failed to set media major type"))?;
    media_type
      .SetGUID(&MF_MT_SUBTYPE, &subtype)
      .map_err(mf_error("Failed to set media subtype"))?;
    media_type
      .SetUINT64(&MF_MT_FRAME_SIZE, pack_u32_pair(u32::from(width), u32::from(height)))
      .map_err(mf_error("Failed to set media frame size"))?;
    media_type
      .SetUINT64(&MF_MT_FRAME_RATE, pack_u32_pair(fps, 1))
      .map_err(mf_error("Failed to set media frame rate"))?;
    media_type
      .SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)
      .map_err(mf_error("Failed to set media interlace mode"))?;
    if let Some(bitrate) = bitrate {
      media_type
        .SetUINT32(&MF_MT_AVG_BITRATE, bitrate)
        .map_err(mf_error("Failed to set media bitrate"))?;
    }
  }
  Ok(media_type)
}

fn sample_from_bytes(bytes: &[u8], timestamp_100ns: i64, duration_100ns: i64) -> Result<IMFSample, VideoError> {
  let buffer =
    unsafe { MFCreateMemoryBuffer(bytes.len() as u32).map_err(mf_error("Failed to create input media buffer"))? };
  write_buffer(&buffer, bytes)?;
  let sample = unsafe { MFCreateSample().map_err(mf_error("Failed to create input media sample"))? };
  unsafe {
    sample
      .AddBuffer(&buffer)
      .map_err(mf_error("Failed to attach input buffer to sample"))?;
    sample
      .SetSampleTime(timestamp_100ns)
      .map_err(mf_error("Failed to set sample timestamp"))?;
    sample
      .SetSampleDuration(duration_100ns)
      .map_err(mf_error("Failed to set sample duration"))?;
  }
  Ok(sample)
}

fn create_empty_output_sample(buffer_size: u32) -> Result<IMFSample, VideoError> {
  let buffer = unsafe { MFCreateMemoryBuffer(buffer_size).map_err(mf_error("Failed to create output media buffer"))? };
  let sample = unsafe { MFCreateSample().map_err(mf_error("Failed to create output media sample"))? };
  unsafe {
    sample
      .AddBuffer(&buffer)
      .map_err(mf_error("Failed to attach output buffer to sample"))?;
  }
  Ok(sample)
}

fn write_buffer(buffer: &IMFMediaBuffer, bytes: &[u8]) -> Result<(), VideoError> {
  let mut data = ptr::null_mut();
  unsafe {
    buffer
      .Lock(&mut data, None, None)
      .map_err(mf_error("Failed to lock media buffer"))?;
    ptr::copy_nonoverlapping(bytes.as_ptr(), data, bytes.len());
    buffer.Unlock().map_err(mf_error("Failed to unlock media buffer"))?;
    buffer
      .SetCurrentLength(bytes.len() as u32)
      .map_err(mf_error("Failed to set media buffer length"))?;
  }
  Ok(())
}

fn bytes_from_sample(sample: &IMFSample) -> Result<Vec<u8>, VideoError> {
  let buffer = unsafe {
    sample
      .ConvertToContiguousBuffer()
      .map_err(mf_error("Failed to read encoded sample buffer"))?
  };
  let len = unsafe {
    buffer
      .GetCurrentLength()
      .map_err(mf_error("Failed to query encoded sample length"))?
  };
  if len == 0 {
    return Ok(Vec::new());
  }

  let mut data = ptr::null_mut();
  let mut out = vec![0u8; len as usize];
  unsafe {
    buffer
      .Lock(&mut data, None, None)
      .map_err(mf_error("Failed to lock encoded sample buffer"))?;
    ptr::copy_nonoverlapping(data, out.as_mut_ptr(), out.len());
    buffer
      .Unlock()
      .map_err(mf_error("Failed to unlock encoded sample buffer"))?;
  }
  Ok(out)
}

fn mf_error(context: &'static str) -> impl FnOnce(WindowsError) -> VideoError {
  move |error| VideoError::new(format!("{context}: {error}"))
}

fn pack_u32_pair(high: u32, low: u32) -> u64 {
  (u64::from(high) << 32) | u64::from(low)
}

fn codec_subtype(codec: VideoCodecId) -> Result<GUID, VideoError> {
  match codec {
    VideoCodecId::Av1 => Ok(MFVideoFormat_AV1),
    VideoCodecId::H265 => Ok(MFVideoFormat_HEVC),
    VideoCodecId::H264 => Ok(MFVideoFormat_H264),
    VideoCodecId::Unknown => Err(VideoError::new("Unsupported video codec.")),
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

fn nv12_len(width: u16, height: u16) -> Result<usize, VideoError> {
  let width = usize::from(width);
  let height = usize::from(height);
  if width == 0 || height == 0 || width % 2 != 0 || height % 2 != 0 {
    return Err(VideoError::new("NV12 conversion requires non-zero even dimensions."));
  }
  Ok(width * height + width * height / 2)
}

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

struct MediaFoundationSession {
  com_initialized: bool,
}

impl MediaFoundationSession {
  fn start() -> Result<Self, VideoError> {
    let com_initialized = unsafe {
      let result = CoInitializeEx(None, COINIT_MULTITHREADED);
      if result == RPC_E_CHANGED_MODE {
        false
      } else if result.is_ok() {
        true
      } else {
        return Err(VideoError::new(format!(
          "Failed to initialize COM for video: {result:?}"
        )));
      }
    };

    unsafe {
      MFStartup(MF_VERSION, MFSTARTUP_NOSOCKET)
        .map_err(|error| VideoError::new(format!("Failed to start Media Foundation: {error}")))?;
    }
    Ok(Self { com_initialized })
  }
}

impl Drop for MediaFoundationSession {
  fn drop(&mut self) {
    unsafe {
      let _ = MFShutdown();
      if self.com_initialized {
        CoUninitialize();
      }
    }
  }
}

unsafe fn release_activates(activates: *mut Option<IMFActivate>, count: u32) {
  for index in 0..count as usize {
    let activate = unsafe { ptr::read(activates.add(index)) };
    drop(activate);
  }
  unsafe {
    CoTaskMemFree(Some(activates.cast::<c_void>()));
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn windows_backend_order_matches_original_parties() {
    assert_eq!(
      BACKEND_ORDER,
      [
        NativeVideoBackend::NvidiaNvenc,
        NativeVideoBackend::AmdAmf,
        NativeVideoBackend::WindowsMediaFoundation,
        NativeVideoBackend::OpenH264,
      ]
    );
    assert_eq!(backend_order_label(), "NVENC -> AMF -> Media Foundation -> OpenH264");
  }

  #[test]
  fn media_foundation_codec_subtypes_are_known_for_supported_codecs() {
    assert_eq!(codec_subtype(VideoCodecId::Av1).unwrap(), MFVideoFormat_AV1);
    assert_eq!(codec_subtype(VideoCodecId::H265).unwrap(), MFVideoFormat_HEVC);
    assert_eq!(codec_subtype(VideoCodecId::H264).unwrap(), MFVideoFormat_H264);
    assert!(codec_subtype(VideoCodecId::Unknown).is_err());
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
