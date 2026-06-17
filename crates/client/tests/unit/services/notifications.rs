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

#[test]
fn outgoing_voice_join_override_uses_own_key() {
  let overrides = r#"{"voice_join":"custom","outgoing_voice_join":" custom "}"#;

  assert_eq!(
    outgoing_voice_join_sound_override(overrides).as_deref(),
    Some(SOUND_CHOICE_CUSTOM)
  );
  assert_eq!(
    notification_sound_override(overrides, NotificationSound::VoiceJoin).as_deref(),
    Some(SOUND_CHOICE_CUSTOM)
  );
}

#[test]
fn outgoing_voice_join_empty_or_missing_override_is_not_selected() {
  assert_eq!(
    outgoing_voice_join_sound_override(r#"{"outgoing_voice_join":""}"#),
    None
  );
  assert_eq!(outgoing_voice_join_sound_override(r#"{"voice_join":"custom"}"#), None);
  assert_eq!(outgoing_voice_join_sound_override("not-json"), None);

  assert!(
    decode_outgoing_voice_join_sound_mono(r#"{"voice_join":"custom"}"#, 48_000)
      .unwrap()
      .is_none()
  );
}

#[test]
fn decoded_notification_sound_resamples_to_mono() {
  let sound = DecodedNotificationSound {
    samples: vec![1.0, -1.0, 0.5, 0.25, -0.5, -1.0],
    channels: 2,
    sample_rate: 3,
  };

  let mono = sound.resampled_mono(6);

  assert_eq!(mono.len(), 6);
  assert_eq!(mono[0], 0.0);
  assert_eq!(mono[1], 0.1875);
  assert_eq!(mono[2], 0.375);
  assert_eq!(mono[3], -0.1875);
  assert_eq!(mono[4], -0.75);
  assert_eq!(mono[5], -0.75);
}
