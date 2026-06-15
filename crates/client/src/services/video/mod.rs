use std::{
  fmt,
  sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
  },
  thread::JoinHandle,
  time::{Duration, Instant},
};

use crate::{
  network::{
    protocol::{
      VideoCodecId,
      data::{ForwardedVideoFrame, VideoFrame},
    },
    server::Server,
  },
  services::screen_share_sources::ScreenShareSourceKind,
};

#[cfg(target_os = "macos")]
mod macos;
mod software;
#[cfg(target_os = "windows")]
mod webcam;
#[cfg(target_os = "windows")]
mod windows;

const VIDEO_OUTPUT_DIMENSION_ALIGNMENT: u16 = 16;

#[derive(Clone, Debug)]
pub struct VideoBroadcastConfig {
  pub source_kind: ScreenShareSourceKind,
  pub source_id: u32,
  pub source_width: u16,
  pub source_height: u16,
  pub output_width: u16,
  pub output_height: u16,
  pub codec: VideoCodecId,
  pub fps: u32,
  pub bitrate_kbps: u32,
  pub audio_enabled: bool,
}

pub type VideoFrameLoopback = Arc<dyn Fn(VideoFrame) + Send + Sync + 'static>;
const MIN_KEYFRAME_REQUEST_INTERVAL: Duration = Duration::from_secs(2);

#[allow(dead_code)]
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct VideoDecodeConfig {
  pub codec: VideoCodecId,
  pub width: u16,
  pub height: u16,
  pub hardware_decoding: bool,
}

pub struct DecodedVideoFrame {
  pub sender_id: u32,
  pub codec: VideoCodecId,
  pub width: u16,
  pub height: u16,
  pub format: DecodedVideoPixelFormat,
  pub pixels: Vec<u8>,
  #[allow(dead_code)]
  pub native_image: Option<lurq::images::ImageData>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub enum DecodedVideoPixelFormat {
  Rgba8,
  Nv12,
}

pub(super) struct NativeDecodedVideoFrame {
  pub format: DecodedVideoPixelFormat,
  pub pixels: Vec<u8>,
  pub native_image: Option<lurq::images::ImageData>,
}

pub(super) trait VideoFrameDecoder {
  fn decode_frame(
    &mut self,
    frame: &VideoFrame,
    output: bool,
    output_buffer: Option<Vec<u8>>,
  ) -> Result<Option<NativeDecodedVideoFrame>, VideoError>;

  #[cfg(target_os = "windows")]
  fn decode_frame_to_dx12(
    &mut self,
    _frame: &VideoFrame,
    _surface: &lurq::app::dx12_render::Dx12Nv12Surface,
  ) -> Result<bool, VideoError> {
    Ok(false)
  }

  #[cfg(target_os = "windows")]
  fn decode_frame_to_shared_nv12_planes(&mut self, _frame: &VideoFrame) -> Result<Option<(usize, usize)>, VideoError> {
    Ok(None)
  }
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeVideoBackend {
  NvidiaNvenc,
  NvidiaNvdec,
  AmdAmf,
  WindowsMediaFoundation,
  OpenH264,
  SoftwareDecoder,
  AppleVideoToolbox,
}

#[derive(Debug)]
pub struct VideoError {
  message: String,
}

impl VideoError {
  pub(super) fn new(message: impl Into<String>) -> Self {
    Self {
      message: message.into(),
    }
  }
}

impl fmt::Display for VideoError {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.write_str(&self.message)
  }
}

impl std::error::Error for VideoError {}

#[allow(dead_code)]
pub struct VideoBroadcast {
  stop: Arc<AtomicBool>,
  keyframe_requests: Option<Arc<AtomicU64>>,
  last_keyframe_request: Mutex<Option<Instant>>,
  threads: Vec<JoinHandle<()>>,
  backend: NativeVideoBackend,
}

#[allow(dead_code)]
pub struct VideoDecoder {
  inner: Box<dyn VideoFrameDecoder>,
  config: VideoDecodeConfig,
  backend: NativeVideoBackend,
}

impl VideoBroadcast {
  #[allow(dead_code)]
  pub fn start(server: Arc<Server>, config: VideoBroadcastConfig) -> Result<Self, VideoError> {
    Self::start_with_loopback(server, config, None)
  }

  pub fn start_with_loopback(
    server: Arc<Server>,
    config: VideoBroadcastConfig,
    loopback: Option<VideoFrameLoopback>,
  ) -> Result<Self, VideoError> {
    validate_config(&config)?;
    start_native_backend(server, config, loopback)
  }

  #[allow(dead_code)]
  pub fn backend(&self) -> NativeVideoBackend {
    self.backend
  }

  pub fn request_keyframe(&self) {
    if let Some(requests) = &self.keyframe_requests {
      let now = Instant::now();
      let mut last = self
        .last_keyframe_request
        .lock()
        .expect("video keyframe request lock poisoned");
      if last.is_some_and(|last| now.duration_since(last) < MIN_KEYFRAME_REQUEST_INTERVAL) {
        tracing::debug!(target: "video", "[video] suppressing duplicate local keyframe request");
        return;
      }
      *last = Some(now);
      requests.fetch_add(1, Ordering::Relaxed);
    }
  }

  #[allow(dead_code)]
  pub(super) fn from_parts(backend: NativeVideoBackend, threads: Vec<JoinHandle<()>>) -> Self {
    Self::from_parts_with_stop(backend, Arc::new(AtomicBool::new(false)), threads)
  }

  pub(super) fn from_parts_with_stop(
    backend: NativeVideoBackend,
    stop: Arc<AtomicBool>,
    threads: Vec<JoinHandle<()>>,
  ) -> Self {
    Self::from_parts_with_stop_and_keyframes(backend, stop, None, threads)
  }

  pub(super) fn from_parts_with_stop_and_keyframes(
    backend: NativeVideoBackend,
    stop: Arc<AtomicBool>,
    keyframe_requests: Option<Arc<AtomicU64>>,
    threads: Vec<JoinHandle<()>>,
  ) -> Self {
    Self {
      stop,
      keyframe_requests,
      last_keyframe_request: Mutex::new(None),
      threads,
      backend,
    }
  }
}

#[allow(dead_code)]
impl VideoDecoder {
  pub fn start(config: VideoDecodeConfig) -> Result<Self, VideoError> {
    validate_decode_config(&config)?;
    start_native_decoder(config)
  }

  #[allow(dead_code)]
  pub fn backend(&self) -> NativeVideoBackend {
    self.backend
  }

  pub fn config(&self) -> &VideoDecodeConfig {
    &self.config
  }

  pub fn decode(&mut self, frame: &ForwardedVideoFrame) -> Result<Option<DecodedVideoFrame>, VideoError> {
    self.decode_for_output(frame, true)
  }

  pub fn decode_for_output(
    &mut self,
    frame: &ForwardedVideoFrame,
    output: bool,
  ) -> Result<Option<DecodedVideoFrame>, VideoError> {
    self.decode_with_output_buffer(frame, output, None)
  }

  pub fn decode_with_output_buffer(
    &mut self,
    frame: &ForwardedVideoFrame,
    output: bool,
    output_buffer: Option<Vec<u8>>,
  ) -> Result<Option<DecodedVideoFrame>, VideoError> {
    let frame_config = VideoDecodeConfig {
      codec: frame.frame.codec,
      width: frame.frame.width,
      height: frame.frame.height,
      hardware_decoding: self.config.hardware_decoding,
    };
    if frame_config != self.config {
      return Err(VideoError::new(
        "Video frame format changed; decoder restart is required.",
      ));
    }

    let decoded = self.inner.decode_frame(&frame.frame, output, output_buffer)?;
    Ok(decoded.map(|decoded| DecodedVideoFrame {
      sender_id: frame.sender_id,
      codec: frame.frame.codec,
      width: frame.frame.width,
      height: frame.frame.height,
      format: decoded.format,
      pixels: decoded.pixels,
      native_image: decoded.native_image,
    }))
  }

  #[cfg(target_os = "windows")]
  pub fn decode_to_dx12_surface(
    &mut self,
    frame: &ForwardedVideoFrame,
    surface: &lurq::app::dx12_render::Dx12Nv12Surface,
  ) -> Result<bool, VideoError> {
    let frame_config = VideoDecodeConfig {
      codec: frame.frame.codec,
      width: frame.frame.width,
      height: frame.frame.height,
      hardware_decoding: self.config.hardware_decoding,
    };
    if frame_config != self.config {
      return Err(VideoError::new(
        "Video frame format changed; decoder restart is required.",
      ));
    }

    self.inner.decode_frame_to_dx12(&frame.frame, surface)
  }

  #[cfg(target_os = "windows")]
  pub fn decode_to_shared_nv12_planes(
    &mut self,
    frame: &ForwardedVideoFrame,
  ) -> Result<Option<(usize, usize)>, VideoError> {
    let frame_config = VideoDecodeConfig {
      codec: frame.frame.codec,
      width: frame.frame.width,
      height: frame.frame.height,
      hardware_decoding: self.config.hardware_decoding,
    };
    if frame_config != self.config {
      return Err(VideoError::new(
        "Video frame format changed; decoder restart is required.",
      ));
    }

    self.inner.decode_frame_to_shared_nv12_planes(&frame.frame)
  }

  pub(super) fn from_decoder(
    inner: Box<dyn VideoFrameDecoder>,
    config: VideoDecodeConfig,
    backend: NativeVideoBackend,
  ) -> Self {
    Self { inner, config, backend }
  }
}

impl Drop for VideoBroadcast {
  fn drop(&mut self) {
    self.stop.store(true, Ordering::Relaxed);
    for thread in self.threads.drain(..) {
      let _ = thread.join();
    }
  }
}

fn validate_config(config: &VideoBroadcastConfig) -> Result<(), VideoError> {
  if !config.codec.is_supported_stream_codec() {
    return Err(VideoError::new("Video codec must be AV1, H.265, or H.264."));
  }

  if config.source_width == 0 || config.source_height == 0 || config.output_width == 0 || config.output_height == 0 {
    return Err(VideoError::new("Selected stream source has no capture dimensions."));
  }

  if config.output_width % VIDEO_OUTPUT_DIMENSION_ALIGNMENT != 0
    || config.output_height % VIDEO_OUTPUT_DIMENSION_ALIGNMENT != 0
  {
    return Err(VideoError::new(
      "Video output dimensions must be aligned to 16-pixel codec blocks.",
    ));
  }

  if config.fps == 0 {
    return Err(VideoError::new("Video FPS must be greater than zero."));
  }

  if config.bitrate_kbps == 0 {
    return Err(VideoError::new("Video bitrate must be greater than zero."));
  }

  Ok(())
}

#[allow(dead_code)]
fn validate_decode_config(config: &VideoDecodeConfig) -> Result<(), VideoError> {
  if !config.codec.is_supported_stream_codec() {
    return Err(VideoError::new("Video codec must be AV1, H.265, or H.264."));
  }

  if config.width == 0 || config.height == 0 {
    return Err(VideoError::new("Video decoder dimensions must be greater than zero."));
  }

  Ok(())
}

#[cfg(target_os = "windows")]
fn start_native_backend(
  server: Arc<Server>,
  config: VideoBroadcastConfig,
  loopback: Option<VideoFrameLoopback>,
) -> Result<VideoBroadcast, VideoError> {
  windows::encode(server, config, loopback)
}

#[cfg(target_os = "macos")]
fn start_native_backend(
  server: Arc<Server>,
  config: VideoBroadcastConfig,
  loopback: Option<VideoFrameLoopback>,
) -> Result<VideoBroadcast, VideoError> {
  macos::encode(server, config, loopback)
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn start_native_backend(
  _server: Arc<Server>,
  config: VideoBroadcastConfig,
  _loopback: Option<VideoFrameLoopback>,
) -> Result<VideoBroadcast, VideoError> {
  let _ = (&config.source_kind, config.source_id);
  Err(VideoError::new(
    "Native video backend is not implemented for this platform yet.",
  ))
}

#[cfg(target_os = "windows")]
#[allow(dead_code)]
fn start_native_decoder(config: VideoDecodeConfig) -> Result<VideoDecoder, VideoError> {
  windows::decode(config)
}

#[cfg(target_os = "macos")]
#[allow(dead_code)]
fn start_native_decoder(config: VideoDecodeConfig) -> Result<VideoDecoder, VideoError> {
  macos::decode(config)
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
#[allow(dead_code)]
fn start_native_decoder(_config: VideoDecodeConfig) -> Result<VideoDecoder, VideoError> {
  Err(VideoError::new(
    "Native video decoder is not implemented for this platform yet.",
  ))
}

#[cfg(test)]
mod tests {
  use super::*;

  fn valid_config(codec: VideoCodecId) -> VideoBroadcastConfig {
    VideoBroadcastConfig {
      source_kind: ScreenShareSourceKind::Screen,
      source_id: 1,
      source_width: 1920,
      source_height: 1080,
      output_width: 1280,
      output_height: 720,
      codec,
      fps: 30,
      bitrate_kbps: 2500,
      audio_enabled: true,
    }
  }

  fn valid_decode_config(codec: VideoCodecId) -> VideoDecodeConfig {
    VideoDecodeConfig {
      codec,
      width: 1280,
      height: 720,
      hardware_decoding: true,
    }
  }

  #[test]
  fn config_accepts_supported_codecs() {
    assert!(validate_config(&valid_config(VideoCodecId::Av1)).is_ok());
    assert!(validate_config(&valid_config(VideoCodecId::H265)).is_ok());
    assert!(validate_config(&valid_config(VideoCodecId::H264)).is_ok());
  }

  #[test]
  fn config_rejects_unknown_codec() {
    let error = validate_config(&valid_config(VideoCodecId::Unknown)).unwrap_err();
    assert_eq!(error.to_string(), "Video codec must be AV1, H.265, or H.264.");
  }

  #[test]
  fn config_rejects_zero_dimensions() {
    let mut config = valid_config(VideoCodecId::H264);
    config.source_width = 0;
    let error = validate_config(&config).unwrap_err();
    assert_eq!(error.to_string(), "Selected stream source has no capture dimensions.");
  }

  #[test]
  fn decode_config_accepts_supported_codecs() {
    assert!(validate_decode_config(&valid_decode_config(VideoCodecId::Av1)).is_ok());
    assert!(validate_decode_config(&valid_decode_config(VideoCodecId::H265)).is_ok());
    assert!(validate_decode_config(&valid_decode_config(VideoCodecId::H264)).is_ok());
  }

  #[test]
  fn decode_config_rejects_unknown_codec() {
    let error = validate_decode_config(&valid_decode_config(VideoCodecId::Unknown)).unwrap_err();
    assert_eq!(error.to_string(), "Video codec must be AV1, H.265, or H.264.");
  }
}
