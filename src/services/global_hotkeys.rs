use std::{
  collections::HashSet,
  sync::{Arc, Mutex},
  thread,
};

use rdev::{Event, EventType, Key, listen};

use crate::{
  services::voice_controls::{VoiceControlAction, apply_voice_control},
  session::ServerSession,
  storage::AppSettings,
};

#[derive(Clone)]
pub struct GlobalVoiceHotkeys {
  inner: Arc<GlobalVoiceHotkeysInner>,
}

struct GlobalVoiceHotkeysInner {
  session: ServerSession,
  tokio: tokio::runtime::Handle,
  state: Mutex<GlobalVoiceHotkeysState>,
}

#[derive(Default)]
struct GlobalVoiceHotkeysState {
  enabled: bool,
  push_to_talk_enabled: bool,
  push_to_talk: String,
  toggle_mute: String,
  toggle_deafen: String,
  pressed: HashSet<Key>,
  active_toggles: HashSet<VoiceControlAction>,
}

impl GlobalVoiceHotkeys {
  pub fn new(session: ServerSession, tokio: tokio::runtime::Handle) -> Self {
    let inner = Arc::new(GlobalVoiceHotkeysInner {
      session,
      tokio,
      state: Mutex::new(GlobalVoiceHotkeysState::default()),
    });

    let listener = inner.clone();
    thread::Builder::new()
      .name("global-key-listener".to_owned())
      .spawn(move || {
        let _ = listen(move |event| {
          listener.handle_event(event);
        });
      })
      .expect("failed to spawn global key listener");

    Self { inner }
  }

  pub fn update_settings(&self, settings: Option<&AppSettings>, enabled: bool) {
    let mut state = self.inner.state.lock().expect("global hotkey lock poisoned");
    state.enabled = enabled && settings.is_some();

    if let Some(settings) = settings {
      state.push_to_talk_enabled = settings.push_to_talk;
      state.push_to_talk = normalize_hotkey(&settings.hotkey_push_to_talk);
      state.toggle_mute = normalize_hotkey(&settings.hotkey_toggle_mute);
      state.toggle_deafen = normalize_hotkey(&settings.hotkey_toggle_deafen);
    } else {
      state.push_to_talk_enabled = false;
      state.push_to_talk.clear();
      state.toggle_mute.clear();
      state.toggle_deafen.clear();
    }

    if !state.enabled {
      state.active_toggles.clear();
      state.pressed.clear();
      self.inner.session.set_push_to_talk_active(false);
    }
  }

  pub fn poll_events(&self) {}
}

impl GlobalVoiceHotkeysInner {
  fn handle_event(&self, event: Event) {
    match event.event_type {
      EventType::KeyPress(key) => self.handle_key_press(key),
      EventType::KeyRelease(key) => self.handle_key_release(key),
      _ => {}
    }
  }

  fn handle_key_press(&self, key: Key) {
    let mut pending_action = None;
    {
      let mut state = self.state.lock().expect("global hotkey lock poisoned");
      state.pressed.insert(key);

      if !state.enabled {
        return;
      }

      let Some(event_hotkey) = event_hotkey(&state.pressed, key) else {
        return;
      };

      if state.push_to_talk_enabled && event_hotkey == state.push_to_talk {
        self.session.set_push_to_talk_active(true);
      } else if event_hotkey == state.toggle_mute {
        if state.active_toggles.insert(VoiceControlAction::ToggleMute) {
          pending_action = Some(VoiceControlAction::ToggleMute);
        }
      } else if event_hotkey == state.toggle_deafen && state.active_toggles.insert(VoiceControlAction::ToggleDeafen) {
        pending_action = Some(VoiceControlAction::ToggleDeafen);
      }
    }

    if let Some(action) = pending_action {
      let session = self.session.clone();
      self.tokio.spawn(async move {
        let _ = apply_voice_control(session, action).await;
      });
    }
  }

  fn handle_key_release(&self, key: Key) {
    let mut stop_push_to_talk = false;
    {
      let mut state = self.state.lock().expect("global hotkey lock poisoned");
      state.pressed.remove(&key);

      let Some(label) = key_label(key) else {
        return;
      };
      let label = label.to_ascii_lowercase();

      if state.push_to_talk_enabled && hotkey_contains_part(&state.push_to_talk, &label) {
        stop_push_to_talk = true;
      }
      if hotkey_contains_part(&state.toggle_mute, &label) {
        state.active_toggles.remove(&VoiceControlAction::ToggleMute);
      }
      if hotkey_contains_part(&state.toggle_deafen, &label) {
        state.active_toggles.remove(&VoiceControlAction::ToggleDeafen);
      }
    }

    if stop_push_to_talk {
      self.session.set_push_to_talk_active(false);
    }
  }
}

fn event_hotkey(pressed: &HashSet<Key>, key: Key) -> Option<String> {
  let key = key_label(key)?;
  if is_modifier_label(&key) {
    return None;
  }

  let mut parts = Vec::new();
  if pressed.contains(&Key::ControlLeft) || pressed.contains(&Key::ControlRight) {
    parts.push("ctrl".to_owned());
  }
  if pressed.contains(&Key::Alt) || pressed.contains(&Key::AltGr) {
    parts.push("alt".to_owned());
  }
  if pressed.contains(&Key::ShiftLeft) || pressed.contains(&Key::ShiftRight) {
    parts.push("shift".to_owned());
  }
  if pressed.contains(&Key::MetaLeft) || pressed.contains(&Key::MetaRight) {
    parts.push("meta".to_owned());
  }
  parts.push(key.to_ascii_lowercase());
  Some(parts.join("+"))
}

fn normalize_hotkey(hotkey: &str) -> String {
  hotkey
    .split('+')
    .map(str::trim)
    .filter(|part| !part.is_empty())
    .map(normalize_hotkey_part)
    .collect::<Vec<_>>()
    .join("+")
}

fn normalize_hotkey_part(part: &str) -> String {
  match part.to_ascii_lowercase().as_str() {
    "control" => "ctrl".to_owned(),
    "uparrow" => "arrowup".to_owned(),
    "downarrow" => "arrowdown".to_owned(),
    "leftarrow" => "arrowleft".to_owned(),
    "rightarrow" => "arrowright".to_owned(),
    "cmd" | "command" | "super" | "os" => "meta".to_owned(),
    normalized => normalized.to_owned(),
  }
}

fn hotkey_contains_part(hotkey: &str, part: &str) -> bool {
  !hotkey.is_empty() && hotkey.split('+').any(|hotkey_part| hotkey_part == part)
}

fn key_label(key: Key) -> Option<String> {
  let label = match key {
    Key::Alt | Key::AltGr => "Alt",
    Key::Backspace => "Backspace",
    Key::CapsLock => "CapsLock",
    Key::ControlLeft | Key::ControlRight => "Ctrl",
    Key::Delete => "Delete",
    Key::DownArrow => "ArrowDown",
    Key::End => "End",
    Key::Escape => "Escape",
    Key::F1 => "F1",
    Key::F2 => "F2",
    Key::F3 => "F3",
    Key::F4 => "F4",
    Key::F5 => "F5",
    Key::F6 => "F6",
    Key::F7 => "F7",
    Key::F8 => "F8",
    Key::F9 => "F9",
    Key::F10 => "F10",
    Key::F11 => "F11",
    Key::F12 => "F12",
    Key::Home => "Home",
    Key::Insert => "Insert",
    Key::LeftArrow => "ArrowLeft",
    Key::MetaLeft | Key::MetaRight => "Meta",
    Key::PageDown => "PageDown",
    Key::PageUp => "PageUp",
    Key::Return | Key::KpReturn => "Enter",
    Key::RightArrow => "ArrowRight",
    Key::ShiftLeft | Key::ShiftRight => "Shift",
    Key::Space => "Space",
    Key::Tab => "Tab",
    Key::UpArrow => "ArrowUp",
    Key::PrintScreen => "PrintScreen",
    Key::ScrollLock => "ScrollLock",
    Key::Pause => "Pause",
    Key::NumLock => "NumLock",
    Key::BackQuote => "`",
    Key::Num0 => "0",
    Key::Num1 => "1",
    Key::Num2 => "2",
    Key::Num3 => "3",
    Key::Num4 => "4",
    Key::Num5 => "5",
    Key::Num6 => "6",
    Key::Num7 => "7",
    Key::Num8 => "8",
    Key::Num9 => "9",
    Key::Minus => "-",
    Key::Equal => "=",
    Key::KeyA => "A",
    Key::KeyB => "B",
    Key::KeyC => "C",
    Key::KeyD => "D",
    Key::KeyE => "E",
    Key::KeyF => "F",
    Key::KeyG => "G",
    Key::KeyH => "H",
    Key::KeyI => "I",
    Key::KeyJ => "J",
    Key::KeyK => "K",
    Key::KeyL => "L",
    Key::KeyM => "M",
    Key::KeyN => "N",
    Key::KeyO => "O",
    Key::KeyP => "P",
    Key::KeyQ => "Q",
    Key::KeyR => "R",
    Key::KeyS => "S",
    Key::KeyT => "T",
    Key::KeyU => "U",
    Key::KeyV => "V",
    Key::KeyW => "W",
    Key::KeyX => "X",
    Key::KeyY => "Y",
    Key::KeyZ => "Z",
    Key::LeftBracket => "[",
    Key::RightBracket => "]",
    Key::SemiColon => ";",
    Key::Quote => "'",
    Key::BackSlash | Key::IntlBackslash => "\\",
    Key::Comma => ",",
    Key::Dot => ".",
    Key::Slash => "/",
    Key::Kp0 => "Numpad0",
    Key::Kp1 => "Numpad1",
    Key::Kp2 => "Numpad2",
    Key::Kp3 => "Numpad3",
    Key::Kp4 => "Numpad4",
    Key::Kp5 => "Numpad5",
    Key::Kp6 => "Numpad6",
    Key::Kp7 => "Numpad7",
    Key::Kp8 => "Numpad8",
    Key::Kp9 => "Numpad9",
    Key::KpDelete => "NumpadDecimal",
    Key::KpMinus => "NumpadSubtract",
    Key::KpPlus => "NumpadAdd",
    Key::KpMultiply => "NumpadMultiply",
    Key::KpDivide => "NumpadDivide",
    Key::Unknown(code) => return unknown_key_label(code).map(ToOwned::to_owned),
    Key::Function => return None,
  };

  Some(label.to_owned())
}

fn unknown_key_label(code: u32) -> Option<&'static str> {
  match code {
    #[cfg(target_os = "windows")]
    37 => Some("ArrowLeft"),
    #[cfg(target_os = "windows")]
    38 => Some("ArrowUp"),
    #[cfg(target_os = "windows")]
    39 => Some("ArrowRight"),
    #[cfg(target_os = "windows")]
    40 => Some("ArrowDown"),
    #[cfg(target_os = "linux")]
    111 => Some("ArrowUp"),
    #[cfg(target_os = "linux")]
    113 => Some("ArrowLeft"),
    #[cfg(target_os = "linux")]
    114 => Some("ArrowRight"),
    #[cfg(target_os = "linux")]
    116 => Some("ArrowDown"),
    #[cfg(target_os = "macos")]
    123 => Some("ArrowLeft"),
    #[cfg(target_os = "macos")]
    124 => Some("ArrowRight"),
    #[cfg(target_os = "macos")]
    125 => Some("ArrowDown"),
    #[cfg(target_os = "macos")]
    126 => Some("ArrowUp"),
    _ => None,
  }
}

fn is_modifier_label(label: &str) -> bool {
  matches!(label, "Ctrl" | "Alt" | "Shift" | "Meta")
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn builds_hotkey_from_pressed_keys_without_consuming_keys() {
    let pressed = HashSet::from([Key::ShiftLeft, Key::KeyS]);
    assert_eq!(event_hotkey(&pressed, Key::KeyS).as_deref(), Some("shift+s"));
  }

  #[test]
  fn caps_lock_can_be_a_standalone_hotkey() {
    let pressed = HashSet::from([Key::CapsLock]);
    assert_eq!(event_hotkey(&pressed, Key::CapsLock).as_deref(), Some("capslock"));
  }

  #[test]
  fn arrows_can_be_standalone_hotkeys() {
    let pressed = HashSet::from([Key::DownArrow]);
    assert_eq!(event_hotkey(&pressed, Key::DownArrow).as_deref(), Some("arrowdown"));

    let pressed = HashSet::from([Key::UpArrow]);
    assert_eq!(event_hotkey(&pressed, Key::UpArrow).as_deref(), Some("arrowup"));
  }

  #[test]
  fn normalizes_arrow_aliases() {
    assert_eq!(normalize_hotkey("UpArrow"), "arrowup");
    assert_eq!(normalize_hotkey("Ctrl+DownArrow"), "ctrl+arrowdown");
  }

  #[cfg(target_os = "windows")]
  #[test]
  fn windows_unknown_arrow_codes_are_mapped() {
    assert_eq!(key_label(Key::Unknown(38)).as_deref(), Some("ArrowUp"));
    assert_eq!(key_label(Key::Unknown(40)).as_deref(), Some("ArrowDown"));
  }

  #[test]
  fn release_matches_any_hotkey_part() {
    assert!(hotkey_contains_part("ctrl+m", "ctrl"));
    assert!(hotkey_contains_part("ctrl+m", "m"));
    assert!(!hotkey_contains_part("ctrl+m", "shift"));
  }
}
