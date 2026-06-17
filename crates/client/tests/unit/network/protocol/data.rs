use super::*;

#[test]
fn video_control_pli_round_trips() {
  let pkt = VideoControl::Pli { user_id: 42 };
  let encoded = pkt.encode_datagram();
  assert_eq!(
    encoded,
    vec![PacketType::VideoControl as u8, VIDEO_CTL_PLI, 42, 0, 0, 0]
  );
  assert_eq!(VideoControl::decode_datagram(&encoded).unwrap(), pkt);
}

#[test]
fn forwarded_video_frame_decode_owned_reuses_payload_buffer() {
  let mut encoded = Vec::new();
  encoded.push(PacketType::VideoFrame as u8);
  encoded.extend_from_slice(&7u32.to_le_bytes());
  encoded.extend_from_slice(&11u32.to_le_bytes());
  encoded.extend_from_slice(&12u32.to_le_bytes());
  encoded.push(VIDEO_FLAG_KEYFRAME);
  encoded.extend_from_slice(&1920u16.to_le_bytes());
  encoded.extend_from_slice(&1080u16.to_le_bytes());
  encoded.push(VideoCodecId::Av1 as u8);
  encoded.extend_from_slice(&[1, 2, 3, 4]);

  let decoded = ForwardedVideoFrame::decode_owned(encoded).unwrap();

  assert_eq!(decoded.sender_id, 7);
  assert_eq!(decoded.frame.frame_number, 11);
  assert_eq!(decoded.frame.timestamp, 12);
  assert!(decoded.frame.keyframe);
  assert_eq!(decoded.frame.width, 1920);
  assert_eq!(decoded.frame.height, 1080);
  assert_eq!(decoded.frame.codec, VideoCodecId::Av1);
  assert_eq!(decoded.frame.encoded.as_ref(), &[1, 2, 3, 4]);
}
