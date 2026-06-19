use parking_lot::Mutex;

use crate::{
  services::notifications::{self, NotificationAudioSettings, NotificationSound},
  storage::AppAudioSettings,
};

pub(super) struct VoiceState {
  local_voice: Mutex<(bool, bool)>,
  muted_before_deafen: Mutex<Option<bool>>,
  notification_audio_settings: Mutex<NotificationAudioSettings>,
}

impl VoiceState {
  pub(super) fn new() -> Self {
    Self {
      local_voice: Mutex::new((false, false)),
      muted_before_deafen: Mutex::new(None),
      notification_audio_settings: Mutex::new(NotificationAudioSettings::default()),
    }
  }

  pub(super) fn reset_local(&self) {
    *self.local_voice.lock() = (false, false);
    *self.muted_before_deafen.lock() = None;
  }

  pub(super) fn local_voice_state(&self) -> (bool, bool) {
    *self.local_voice.lock()
  }

  pub(super) fn set_local_voice_state(&self, muted: bool, deafened: bool) {
    *self.local_voice.lock() = (muted, deafened);
  }

  pub(super) fn remember_muted_before_deafen(&self, muted: bool) {
    *self.muted_before_deafen.lock() = Some(muted);
  }

  pub(super) fn take_muted_before_deafen(&self) -> Option<bool> {
    self.muted_before_deafen.lock().take()
  }

  pub(super) fn set_notification_audio_settings(&self, settings: &AppAudioSettings) {
    *self.notification_audio_settings.lock() = NotificationAudioSettings::from_audio_settings(settings);
  }

  pub(super) fn play_notification_sound(&self, sound: NotificationSound) {
    let settings = self.notification_audio_settings.lock().clone();
    notifications::play(sound, settings);
  }
}

impl Default for VoiceState {
  fn default() -> Self {
    Self::new()
  }
}

#[cfg(test)]
#[path = "../../tests/unit/session/voice_state.rs"]
mod tests;
