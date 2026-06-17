use super::VoiceState;

#[test]
fn local_voice_state_defaults_to_unmuted_and_undeafened() {
  let state = VoiceState::new();

  assert_eq!(state.local_voice_state(), (false, false));
}

#[test]
fn local_voice_state_tracks_mute_and_deafen_together() {
  let state = VoiceState::new();

  state.set_local_voice_state(true, false);
  assert_eq!(state.local_voice_state(), (true, false));

  state.set_local_voice_state(false, true);
  assert_eq!(state.local_voice_state(), (false, true));
}

#[test]
fn reset_local_clears_voice_state_and_saved_pre_deafen_mute() {
  let state = VoiceState::new();
  state.set_local_voice_state(true, true);
  state.remember_muted_before_deafen(true);

  state.reset_local();

  assert_eq!(state.local_voice_state(), (false, false));
  assert_eq!(state.take_muted_before_deafen(), None);
}

#[test]
fn muted_before_deafen_is_consumed_once() {
  let state = VoiceState::new();
  state.remember_muted_before_deafen(false);

  assert_eq!(state.take_muted_before_deafen(), Some(false));
  assert_eq!(state.take_muted_before_deafen(), None);
}
