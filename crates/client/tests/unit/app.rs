use lurq::{app::events::KeyboardEvent, core::NodeId};

use super::*;

fn key_event(key: &str, code: &str, ctrl: bool) -> KeyboardEvent {
  KeyboardEvent::new(key, code, false, ctrl, false, false, NodeId::UNASSIGNED)
}

#[test]
fn toggle_hotkey_activation_is_released_by_key_up() {
  let active = Signal::new(Vec::new());

  assert!(activate_toggle_hotkey(&active, "Ctrl+M"));
  assert!(!activate_toggle_hotkey(&active, "Ctrl+M"));

  release_toggle_hotkey(&active, "Ctrl+M", &key_event("M", "KeyM", true));
  assert!(activate_toggle_hotkey(&active, "Ctrl+M"));
}
