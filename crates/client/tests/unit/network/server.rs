use super::*;

#[test]
fn video_stream_packets_are_len_prefixed() {
  let packet = [0x02, 0x11, 0x22, 0x33];
  let framed = encode_video_stream_packet(&packet).unwrap();

  assert_eq!(&framed[..4], &(packet.len() as u32).to_le_bytes());
  assert_eq!(&framed[4..], &packet);
}

#[test]
fn video_stream_packet_rejects_zero_len() {
  let err = encode_video_stream_packet(&[]).unwrap_err();

  assert_eq!(
    err,
    DecodeError::InvalidLength {
      len: 0,
      max: MAX_VIDEO_FRAME_LEN
    }
  );
}

#[test]
fn video_stream_packet_rejects_oversized_len() {
  let err = validate_video_stream_packet_len(MAX_VIDEO_FRAME_LEN + 1).unwrap_err();

  assert_eq!(
    err,
    DecodeError::InvalidLength {
      len: MAX_VIDEO_FRAME_LEN + 1,
      max: MAX_VIDEO_FRAME_LEN
    }
  );
}

#[test]
fn video_stream_decoder_routes_forwarded_video_packets() {
  let packet = vec![
    PacketType::VideoFrame as u8,
    29,
    0,
    0,
    0,
    7,
    0,
    0,
    0,
    11,
    0,
    0,
    0,
    1,
    128,
    2,
    224,
    1,
    VideoCodecId::H264 as u8,
    1,
    2,
    3,
  ];
  let ReceivedVideoPacket::Frame(decoded) = decode_video_stream_packet(packet).unwrap() else {
    panic!("expected video frame packet");
  };

  assert_eq!(decoded.sender_id, 29);
  assert_eq!(decoded.frame.frame_number, 7);
  assert_eq!(decoded.frame.width, 640);
  assert_eq!(decoded.frame.height, 480);
  assert_eq!(decoded.frame.codec, VideoCodecId::H264);
  assert_eq!(decoded.frame.encoded.as_ref(), &[1, 2, 3]);
}

#[test]
fn video_stream_decoder_routes_video_control_packets() {
  let packet = VideoControl::Pli { user_id: 42 }.encode_datagram();
  let ReceivedVideoPacket::VideoControl(VideoControl::Pli { user_id }) = decode_video_stream_packet(packet).unwrap()
  else {
    panic!("expected video control packet");
  };

  assert_eq!(user_id, 42);
}

#[test]
fn datagram_decoder_routes_forwarded_stream_audio_packets() {
  let DecodedDatagram::StreamAudio(decoded) = decode_datagram(Bytes::from_static(&[
    PacketType::StreamAudio as u8,
    7,
    0,
    0,
    0,
    1,
    2,
    3,
  ]))
  .unwrap() else {
    panic!("expected stream audio datagram");
  };

  assert_eq!(decoded.sender_id, 7);
  assert_eq!(decoded.opus.as_ref(), &[1, 2, 3]);
}

#[test]
fn voice_datagram_decoder_rejects_unknown_packet_type() {
  assert_eq!(
    decode_datagram(Bytes::from_static(&[0xff])).unwrap_err(),
    DecodeError::InvalidPacketType(0xff)
  );
}

#[test]
fn voice_datagram_decoder_accepts_forwarded_voice_packets() {
  let packet = [PacketType::Voice as u8, 42, 0, 0, 0, 9, 0, 1, 2, 3];
  let DecodedDatagram::Voice(decoded) = decode_datagram(Bytes::copy_from_slice(&packet)).unwrap() else {
    panic!("expected voice datagram");
  };

  assert_eq!(decoded.sender_id, 42);
  assert_eq!(decoded.sequence, 9);
  assert_eq!(decoded.opus.as_ref(), &[1, 2, 3]);
}

#[test]
fn datagram_decoder_routes_forwarded_video_packets() {
  let packet = [
    PacketType::VideoFrame as u8,
    29,
    0,
    0,
    0,
    7,
    0,
    0,
    0,
    11,
    0,
    0,
    0,
    1,
    128,
    2,
    224,
    1,
    VideoCodecId::H264 as u8,
    1,
    2,
    3,
  ];
  let DecodedDatagram::Video(decoded) = decode_datagram(Bytes::copy_from_slice(&packet)).unwrap() else {
    panic!("expected video datagram");
  };

  assert_eq!(decoded.sender_id, 29);
  assert_eq!(decoded.frame.frame_number, 7);
  assert_eq!(decoded.frame.width, 640);
  assert_eq!(decoded.frame.height, 480);
  assert_eq!(decoded.frame.codec, VideoCodecId::H264);
  assert_eq!(decoded.frame.encoded.as_ref(), &[1, 2, 3]);
}
