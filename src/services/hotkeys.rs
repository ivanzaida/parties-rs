use lurq::app::events::KeyboardEvent;

pub fn event_to_hotkey(event: &KeyboardEvent) -> Option<String> {
  let key = key_label(event)?;
  if is_modifier_label(&key) {
    return None;
  }

  let mut parts = Vec::new();
  if event.ctrl {
    parts.push("Ctrl".to_owned());
  }
  if event.alt {
    parts.push("Alt".to_owned());
  }
  if event.shift {
    parts.push("Shift".to_owned());
  }
  parts.push(key);

  Some(parts.join("+"))
}

pub fn event_matches_hotkey(hotkey: &str, event: &KeyboardEvent) -> bool {
  let hotkey = hotkey.trim();
  !hotkey.is_empty() && event_to_hotkey(event).is_some_and(|event_hotkey| event_hotkey.eq_ignore_ascii_case(hotkey))
}

pub fn is_clear_key(event: &KeyboardEvent) -> bool {
  matches!(
    (event.key.as_str(), event.code.as_str()),
    ("Backspace" | "Delete", _) | (_, "Backspace" | "Delete")
  )
}

pub fn is_cancel_key(event: &KeyboardEvent) -> bool {
  matches!((event.key.as_str(), event.code.as_str()), ("Escape", _) | (_, "Escape"))
}

fn key_label(event: &KeyboardEvent) -> Option<String> {
  let key = event.key.trim();
  let code = event.code.trim();

  let label = match (key, code) {
    ("", "") => return None,
    (" ", _) | (_, "Space") => "Space".to_owned(),
    ("Control" | "Ctrl", _) | (_, "ControlLeft" | "ControlRight") => "Ctrl".to_owned(),
    ("Alt", _) | (_, "AltLeft" | "AltRight") => "Alt".to_owned(),
    ("Shift", _) | (_, "ShiftLeft" | "ShiftRight") => "Shift".to_owned(),
    ("Meta" | "Super" | "OS", _) | (_, "MetaLeft" | "MetaRight") => "Meta".to_owned(),
    _ if key.chars().count() == 1 => key.to_uppercase(),
    _ if !key.is_empty() => key.to_owned(),
    _ => code.to_owned(),
  };

  Some(label)
}

fn is_modifier_label(label: &str) -> bool {
  matches!(label, "Ctrl" | "Alt" | "Shift" | "Meta")
}
