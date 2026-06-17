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
