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

#[test]
fn mouse_buttons_can_be_hotkeys() {
  let pressed = HashSet::from([Key::ControlLeft]);
  assert_eq!(
    mouse_event_hotkey(&pressed, "MouseMiddle".to_owned()),
    "ctrl+mousemiddle"
  );
  assert_eq!(mouse_button_label(Button::Middle).as_deref(), Some("MouseMiddle"));
  assert_eq!(mouse_button_label(Button::Unknown(2)).as_deref(), Some("Mouse2"));
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
