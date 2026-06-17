use super::*;

#[test]
fn named_sound_helpers_resolve_bundled_files() {
  assert_eq!(join_channel_sound().file_name, "join_channel.mp3");
  assert_eq!(leave_channel_sound().file_name, "leave_channel.mp3");
  assert_eq!(new_message_sound().file_name, "new_message.mp3");
  assert_eq!(user_kicked_sound().file_name, "user_kicked.mp3");
}

#[test]
fn planned_sound_helpers_resolve_bundled_files() {
  assert_eq!(mention_sound().file_name, "mention.mp3");
  assert_eq!(stream_started_sound().file_name, "stream_started.mp3");
  assert_eq!(stream_ended_sound().file_name, "stream_ended.mp3");
  assert_eq!(connection_lost_sound().file_name, "connection_lost.mp3");
  assert_eq!(moderation_action_sound().file_name, "moderation_action.mp3");
}

#[test]
fn every_notification_sound_resolves_to_audio_bytes() {
  for sound in NotificationSound::ALL {
    let asset = resolve_sound(sound);
    assert_eq!(asset.sound, sound);
    assert!(!asset.file_name.is_empty());
    assert!(!asset.bytes.is_empty());
    assert!(decode_mp3(asset.bytes).is_ok(), "{} should decode", asset.file_name);
  }
}

#[test]
fn every_notification_sound_uses_a_unique_bundled_file() {
  let mut file_names = std::collections::HashSet::new();
  for sound in NotificationSound::ALL {
    let asset = resolve_sound(sound);
    assert!(file_names.insert(asset.file_name), "{} is reused", asset.file_name);
  }
}

#[test]
fn sound_overrides_resolve_selected_bundled_asset() {
  let overrides = r#"{"chat_message":"user_kicked"}"#;
  let asset = resolve_sound_with_overrides(NotificationSound::ChatMessage, overrides);

  assert_eq!(asset.sound, NotificationSound::ChatMessage);
  assert_eq!(asset.file_name, "user_kicked.mp3");
}

#[test]
fn empty_sound_override_uses_default_asset() {
  let overrides = r#"{"chat_message":""}"#;
  let asset = resolve_sound_with_overrides(NotificationSound::ChatMessage, overrides);

  assert_eq!(asset.file_name, "new_message.mp3");
}
