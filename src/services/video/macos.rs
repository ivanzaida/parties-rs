use std::sync::Arc;

use super::{NativeVideoBackend, VideoBroadcast, VideoBroadcastConfig, VideoDecodeConfig, VideoDecoder, VideoError};
use crate::network::server::Server;

const BACKEND_ORDER: [NativeVideoBackend; 1] = [NativeVideoBackend::AppleVideoToolbox];

pub(super) fn encode(_server: Arc<Server>, config: VideoBroadcastConfig) -> Result<VideoBroadcast, VideoError> {
  let _ = (&config.source_kind, config.source_id);
  Err(VideoError::new(
    "macOS native video encoder is not wired yet. Backend is VideoToolbox.",
  ))
}

#[allow(dead_code)]
pub(super) fn decode(config: VideoDecodeConfig) -> Result<VideoDecoder, VideoError> {
  let _ = config;
  Err(VideoError::new(
    "macOS native video decoder is not wired yet. Backend is VideoToolbox.",
  ))
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn macos_backend_order_matches_original_parties() {
    assert_eq!(BACKEND_ORDER, [NativeVideoBackend::AppleVideoToolbox]);
  }
}
