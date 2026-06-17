use lurq::{app::events::KeyboardEvent, core::NodeId};

use super::*;

fn key_event(key: &str, code: &str, ctrl: bool, alt: bool, shift: bool) -> KeyboardEvent {
  KeyboardEvent::new(key, code, shift, ctrl, alt, false, NodeId::UNASSIGNED)
}

#[test]
fn release_matches_any_hotkey_part() {
  assert!(event_releases_hotkey(
    "Ctrl+P",
    &key_event("P", "KeyP", true, false, false),
  ));
  assert!(event_releases_hotkey(
    "Ctrl+P",
    &key_event("Control", "ControlLeft", false, false, false),
  ));
  assert!(!event_releases_hotkey(
    "Ctrl+P",
    &key_event("M", "KeyM", true, false, false),
  ));
}

#[test]
fn ctrl_shift_f12_matches_physical_function_key() {
  assert!(event_matches_hotkey(
    "Ctrl+Shift+F12",
    &key_event("", "F12", true, false, true),
  ));
}
