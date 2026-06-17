use super::*;

#[test]
fn control_frame_round_trips() {
  let frame = ControlFrame {
    ty: ControlMessageType::KeepalivePing,
    payload: Vec::new(),
  };

  let encoded = frame.encode().unwrap();
  assert_eq!(encoded, vec![2, 0, 0, 0, 4, 0]);
  assert_eq!(ControlFrame::decode(&encoded).unwrap(), frame);
}

#[test]
fn auth_signed_payload_matches_upstream_shape() {
  let public_key = [7; 32];
  let payload = AuthIdentity::signed_payload(&public_key, "alice", 42).unwrap();
  assert_eq!(&payload[..32], &public_key);
  assert_eq!(&payload[32..39], &[5, 0, b'a', b'l', b'i', b'c', b'e']);
  assert_eq!(&payload[39..], &42_u64.to_le_bytes());
}

#[test]
fn auth_payload_uses_packed_protocol_version() {
  let auth = AuthIdentity {
    protocol_version: crate::network::protocol::PROTOCOL_VERSION,
    public_key: [7; 32],
    display_name: "alice".to_owned(),
    timestamp: 42,
    signature: [9; 64],
    password: "secret".to_owned(),
  };

  let versioned = auth.encode_payload().unwrap();
  assert_eq!(
    &versioned[..2],
    &crate::network::protocol::PROTOCOL_VERSION.to_le_bytes()
  );
  assert_eq!(&versioned[2..34], &[7; 32]);
}

#[test]
fn channel_user_list_defaults_unknown_role_to_user() {
  let mut w = BinaryWriter::new();
  w.write_u32(7);
  w.write_u32(1);
  w.write_u32(42);
  w.write_string("bot").unwrap();
  w.write_u8(0x04);
  w.write_u8(0);
  w.write_u8(0);

  let list = ChannelUserList::decode_payload(w.as_slice()).unwrap();

  assert_eq!(list.channel_id, 7);
  assert_eq!(list.users.len(), 1);
  assert_eq!(list.users[0].role, Role::User);
}

#[test]
fn user_joined_channel_defaults_unknown_role_to_user() {
  let mut w = BinaryWriter::new();
  w.write_u32(42);
  w.write_string("bot").unwrap();
  w.write_u32(7);
  w.write_u8(0x04);

  let joined = UserJoinedChannel::decode_payload(w.as_slice()).unwrap();

  assert_eq!(joined.user_id, 42);
  assert_eq!(joined.role, Role::User);
}

#[test]
fn user_role_changed_defaults_unknown_role_to_user() {
  let mut w = BinaryWriter::new();
  w.write_u32(42);
  w.write_u8(0x04);

  let changed = UserRoleChanged::decode_payload(w.as_slice()).unwrap();

  assert_eq!(changed.user_id, 42);
  assert_eq!(changed.role, Role::User);
}

#[test]
fn screen_share_started_accepts_pending_unknown_codec_metadata() {
  let mut payload = Vec::new();
  payload.extend_from_slice(&42_u32.to_le_bytes());
  payload.push(VideoCodecId::Unknown as u8);
  payload.extend_from_slice(&0_u16.to_le_bytes());
  payload.extend_from_slice(&0_u16.to_le_bytes());

  let started = ScreenShareStarted::decode_payload(&payload).unwrap();

  assert_eq!(started.sharer_user_id, 42);
  assert_eq!(started.metadata.codec, VideoCodecId::Unknown);
  assert_eq!(started.metadata.width, 0);
  assert_eq!(started.metadata.height, 0);
}

#[test]
fn screen_share_started_rejects_invalid_codec_metadata() {
  let mut payload = Vec::new();
  payload.extend_from_slice(&42_u32.to_le_bytes());
  payload.push(0xff);
  payload.extend_from_slice(&0_u16.to_le_bytes());
  payload.extend_from_slice(&0_u16.to_le_bytes());

  let error = ScreenShareStarted::decode_payload(&payload).unwrap_err();

  assert_eq!(
    error,
    DecodeError::InvalidEnumValue {
      field: "video codec",
      value: 0xff,
    }
  );
}

#[test]
fn chat_command_list_decodes_upstream_shape() {
  let mut w = BinaryWriter::new();
  w.write_u16(1);
  w.write_string("botping").unwrap();
  w.write_string("Send a test message as a server bot.").unwrap();
  w.write_string("/botping [text]").unwrap();

  assert_eq!(
    ChatCommandList::decode_payload(w.as_slice()).unwrap(),
    ChatCommandList {
      commands: vec![ChatCommandInfo {
        name: "botping".to_owned(),
        description: "Send a test message as a server bot.".to_owned(),
        usage: "/botping [text]".to_owned(),
      }]
    }
  );
}
