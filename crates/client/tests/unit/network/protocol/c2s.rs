use super::*;

#[test]
fn keepalive_ping_encodes_empty() {
  let frame = C2S::KeepalivePing.encode().unwrap();
  assert_eq!(frame.ty, ControlMessageType::KeepalivePing);
  assert!(frame.payload.is_empty());
}

#[test]
fn channel_join_encodes_id() {
  let frame = C2S::ChannelJoin { channel_id: 42 }.encode().unwrap();
  assert_eq!(frame.ty, ControlMessageType::ChannelJoin);
  assert_eq!(frame.payload, 42_u32.to_le_bytes());
}

#[test]
fn admin_set_user_voice_state_encodes_target_and_state() {
  let frame = C2S::AdminSetUserVoiceState {
    target_user_id: 7,
    muted: true,
    deafened: false,
  }
  .encode()
  .unwrap();
  assert_eq!(frame.ty, ControlMessageType::AdminSetUserVoiceState);
  assert_eq!(frame.payload, [7, 0, 0, 0, 1, 0]);
}

#[test]
fn admin_disconnect_user_encodes_target() {
  let frame = C2S::AdminDisconnectUser { target_user_id: 7 }.encode().unwrap();
  assert_eq!(frame.ty, ControlMessageType::AdminDisconnectUser);
  assert_eq!(frame.payload, 7_u32.to_le_bytes());
}
