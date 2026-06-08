use std::{
  fmt,
  sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
  },
  thread::JoinHandle,
};

use crate::{
  network::{protocol::VideoCodecId, server::Server},
  services::screen_share_sources::ScreenShareSourceKind,
};

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

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
}

#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VideoDecodeConfig {
  pub codec: VideoCodecId,
  pub width: u16,
  pub height: u16,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeVideoBackend {
  NvidiaNvenc,
  AmdAmf,
  WindowsMediaFoundation,
  OpenH264,
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
  threads: Vec<JoinHandle<()>>,
  backend: NativeVideoBackend,
}

#[allow(dead_code)]
pub struct VideoDecoder {
  backend: NativeVideoBackend,
}

impl VideoBroadcast {
  pub fn start(server: Arc<Server>, config: VideoBroadcastConfig) -> Result<Self, VideoError> {
    validate_config(&config)?;
    start_native_backend(server, config)
  }

  #[allow(dead_code)]
  pub fn backend(&self) -> NativeVideoBackend {
    self.backend
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
    Self { stop, threads, backend }
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

  pub(super) fn from_backend(backend: NativeVideoBackend) -> Self {
    Self { backend }
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
fn start_native_backend(server: Arc<Server>, config: VideoBroadcastConfig) -> Result<VideoBroadcast, VideoError> {
  windows::encode(server, config)
}

#[cfg(target_os = "macos")]
fn start_native_backend(server: Arc<Server>, config: VideoBroadcastConfig) -> Result<VideoBroadcast, VideoError> {
  macos::encode(server, config)
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn start_native_backend(_server: Arc<Server>, config: VideoBroadcastConfig) -> Result<VideoBroadcast, VideoError> {
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
    }
  }

  fn valid_decode_config(codec: VideoCodecId) -> VideoDecodeConfig {
    VideoDecodeConfig {
      codec,
      width: 1280,
      height: 720,
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
