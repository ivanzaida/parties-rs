use std::{
  collections::HashSet,
  sync::{Arc, Mutex},
  thread,
};

#[cfg(target_os = "macos")]
use std::{
  ffi::c_void,
  ptr,
  sync::mpsc::{self, Receiver, Sender},
};

#[cfg(any(not(target_os = "macos"), test))]
use rdev::Key;
#[cfg(not(target_os = "macos"))]
use rdev::listen;
#[cfg(not(target_os = "macos"))]
use rdev::{Event, EventType};

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
  #[cfg(target_os = "macos")]
  macos: Mutex<MacosGlobalHotkeys>,
}

#[derive(Default)]
struct GlobalVoiceHotkeysState {
  enabled: bool,
  push_to_talk_enabled: bool,
  push_to_talk: String,
  toggle_mute: String,
  toggle_deafen: String,
  #[cfg(not(target_os = "macos"))]
  pressed: HashSet<Key>,
  active_toggles: HashSet<VoiceControlAction>,
}

impl GlobalVoiceHotkeys {
  pub fn new(session: ServerSession, tokio: tokio::runtime::Handle) -> Self {
    let inner = Arc::new(GlobalVoiceHotkeysInner {
      session,
      tokio,
      state: Mutex::new(GlobalVoiceHotkeysState::default()),
      #[cfg(target_os = "macos")]
      macos: Mutex::new(MacosGlobalHotkeys::new()),
    });

    #[cfg(not(target_os = "macos"))]
    {
      let listener = inner.clone();
      thread::Builder::new()
        .name("global-key-listener".to_owned())
        .spawn(move || {
          let _ = listen(move |event| {
            listener.handle_event(event);
          });
        })
        .expect("failed to spawn global key listener");
    }

    Self { inner }
  }

  pub fn update_settings(&self, settings: Option<&AppSettings>, enabled: bool) {
    let global_hotkeys_enabled = enabled && settings.is_some();
    let mut state = self.inner.state.lock().expect("global hotkey lock poisoned");
    state.enabled = global_hotkeys_enabled;

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
      #[cfg(not(target_os = "macos"))]
      state.pressed.clear();
      self.inner.session.set_push_to_talk_active(false);
    }

    #[cfg(target_os = "macos")]
    self
      .inner
      .macos
      .lock()
      .expect("macOS global hotkey lock poisoned")
      .update(MacosHotkeyConfig {
        enabled: global_hotkeys_enabled,
        push_to_talk_enabled: state.push_to_talk_enabled,
        push_to_talk: state.push_to_talk.clone(),
        toggle_mute: state.toggle_mute.clone(),
        toggle_deafen: state.toggle_deafen.clone(),
      });
  }

  pub fn poll_events(&self) {
    #[cfg(target_os = "macos")]
    {
      let events = self
        .inner
        .macos
        .lock()
        .expect("macOS global hotkey lock poisoned")
        .drain_events();
      for event in events {
        self.inner.handle_macos_hotkey_event(event);
      }
    }
  }
}

impl GlobalVoiceHotkeysInner {
  #[cfg(not(target_os = "macos"))]
  fn handle_event(&self, event: Event) {
    match event.event_type {
      EventType::KeyPress(key) => self.handle_key_press(key),
      EventType::KeyRelease(key) => self.handle_key_release(key),
      _ => {}
    }
  }

  #[cfg(not(target_os = "macos"))]
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

  #[cfg(not(target_os = "macos"))]
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

  #[cfg(target_os = "macos")]
  fn handle_macos_hotkey_event(&self, event: MacosHotkeyEvent) {
    match (event.action, event.pressed) {
      (MacosHotkeyAction::PushToTalk, true) => {
        let state = self.state.lock().expect("global hotkey lock poisoned");
        if state.enabled && state.push_to_talk_enabled {
          self.session.set_push_to_talk_active(true);
        }
      }
      (MacosHotkeyAction::PushToTalk, false) => {
        self.session.set_push_to_talk_active(false);
      }
      (MacosHotkeyAction::ToggleMute | MacosHotkeyAction::ToggleDeafen, true) => {
        let action = match event.action {
          MacosHotkeyAction::ToggleMute => VoiceControlAction::ToggleMute,
          MacosHotkeyAction::ToggleDeafen => VoiceControlAction::ToggleDeafen,
          MacosHotkeyAction::PushToTalk => return,
        };
        let mut should_run = false;
        {
          let mut state = self.state.lock().expect("global hotkey lock poisoned");
          if state.enabled && state.active_toggles.insert(action) {
            should_run = true;
          }
        }
        if should_run {
          let session = self.session.clone();
          self.tokio.spawn(async move {
            let _ = apply_voice_control(session, action).await;
          });
        }
      }
      (MacosHotkeyAction::ToggleMute | MacosHotkeyAction::ToggleDeafen, false) => {
        let action = match event.action {
          MacosHotkeyAction::ToggleMute => VoiceControlAction::ToggleMute,
          MacosHotkeyAction::ToggleDeafen => VoiceControlAction::ToggleDeafen,
          MacosHotkeyAction::PushToTalk => return,
        };
        self
          .state
          .lock()
          .expect("global hotkey lock poisoned")
          .active_toggles
          .remove(&action);
      }
    }
  }
}

#[cfg(any(not(target_os = "macos"), test))]
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

#[cfg(any(not(target_os = "macos"), test))]
fn hotkey_contains_part(hotkey: &str, part: &str) -> bool {
  !hotkey.is_empty() && hotkey.split('+').any(|hotkey_part| hotkey_part == part)
}

#[cfg(any(not(target_os = "macos"), test))]
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

#[cfg(any(not(target_os = "macos"), test))]
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

#[cfg(any(not(target_os = "macos"), test))]
fn is_modifier_label(label: &str) -> bool {
  matches!(label, "Ctrl" | "Alt" | "Shift" | "Meta")
}

#[cfg(target_os = "macos")]
#[derive(Clone, PartialEq, Eq)]
struct MacosHotkeyConfig {
  enabled: bool,
  push_to_talk_enabled: bool,
  push_to_talk: String,
  toggle_mute: String,
  toggle_deafen: String,
}

#[cfg(target_os = "macos")]
impl Default for MacosHotkeyConfig {
  fn default() -> Self {
    Self {
      enabled: false,
      push_to_talk_enabled: false,
      push_to_talk: String::new(),
      toggle_mute: String::new(),
      toggle_deafen: String::new(),
    }
  }
}

#[cfg(target_os = "macos")]
struct MacosGlobalHotkeys {
  receiver: Receiver<MacosRawKeyEvent>,
  config: MacosHotkeyConfig,
}

#[cfg(target_os = "macos")]
impl MacosGlobalHotkeys {
  fn new() -> Self {
    let (sender, receiver) = mpsc::channel();
    spawn_macos_event_tap(sender);

    Self {
      receiver,
      config: MacosHotkeyConfig::default(),
    }
  }

  fn update(&mut self, config: MacosHotkeyConfig) {
    if self.config == config {
      return;
    }

    self.config = config;
  }

  fn drain_events(&self) -> Vec<MacosHotkeyEvent> {
    let mut events = Vec::new();
    while let Ok(raw_event) = self.receiver.try_recv() {
      if let Some(event) = self.macos_hotkey_event(raw_event) {
        events.push(event);
      }
    }
    events
  }

  fn macos_hotkey_event(&self, raw_event: MacosRawKeyEvent) -> Option<MacosHotkeyEvent> {
    if !self.config.enabled {
      return None;
    }

    if self.config.push_to_talk_enabled && macos_raw_event_matches_hotkey(raw_event, &self.config.push_to_talk) {
      return Some(MacosHotkeyEvent {
        action: MacosHotkeyAction::PushToTalk,
        pressed: raw_event.pressed,
      });
    }

    if macos_raw_event_matches_hotkey(raw_event, &self.config.toggle_mute) {
      return Some(MacosHotkeyEvent {
        action: MacosHotkeyAction::ToggleMute,
        pressed: raw_event.pressed,
      });
    }

    if macos_raw_event_matches_hotkey(raw_event, &self.config.toggle_deafen) {
      return Some(MacosHotkeyEvent {
        action: MacosHotkeyAction::ToggleDeafen,
        pressed: raw_event.pressed,
      });
    }

    None
  }
}

#[cfg(target_os = "macos")]
#[derive(Clone, Copy, Debug)]
struct MacosRawKeyEvent {
  key_code: u32,
  modifiers: u32,
  pressed: bool,
}

#[cfg(target_os = "macos")]
#[derive(Clone, Copy, Debug)]
struct MacosHotkeyEvent {
  action: MacosHotkeyAction,
  pressed: bool,
}

#[cfg(target_os = "macos")]
#[derive(Clone, Copy, Debug)]
#[repr(u32)]
enum MacosHotkeyAction {
  PushToTalk,
  ToggleMute,
  ToggleDeafen,
}

#[cfg(target_os = "macos")]
fn macos_hotkey_parts(hotkey: &str) -> Option<(u32, u32)> {
  let mut modifiers = 0;
  let mut key_code = None;

  for part in hotkey.split('+').map(str::trim).filter(|part| !part.is_empty()) {
    match normalize_hotkey_part(part).as_str() {
      "ctrl" => modifiers |= CONTROL_KEY,
      "alt" => modifiers |= OPTION_KEY,
      "shift" => modifiers |= SHIFT_KEY,
      "meta" => modifiers |= CMD_KEY,
      key if key_code.is_none() => key_code = macos_key_code(key),
      _ => return None,
    }
  }

  key_code.map(|key_code| (key_code, modifiers))
}

#[cfg(target_os = "macos")]
fn macos_raw_event_matches_hotkey(event: MacosRawKeyEvent, hotkey: &str) -> bool {
  let Some((key_code, modifiers)) = macos_hotkey_parts(hotkey) else {
    return false;
  };

  event.key_code == key_code && event.modifiers == modifiers
}

#[cfg(target_os = "macos")]
fn macos_key_code(key: &str) -> Option<u32> {
  match key {
    "a" => Some(0x00),
    "s" => Some(0x01),
    "d" => Some(0x02),
    "f" => Some(0x03),
    "h" => Some(0x04),
    "g" => Some(0x05),
    "z" => Some(0x06),
    "x" => Some(0x07),
    "c" => Some(0x08),
    "v" => Some(0x09),
    "b" => Some(0x0B),
    "q" => Some(0x0C),
    "w" => Some(0x0D),
    "e" => Some(0x0E),
    "r" => Some(0x0F),
    "y" => Some(0x10),
    "t" => Some(0x11),
    "1" => Some(0x12),
    "2" => Some(0x13),
    "3" => Some(0x14),
    "4" => Some(0x15),
    "6" => Some(0x16),
    "5" => Some(0x17),
    "=" => Some(0x18),
    "9" => Some(0x19),
    "7" => Some(0x1A),
    "-" => Some(0x1B),
    "8" => Some(0x1C),
    "0" => Some(0x1D),
    "]" => Some(0x1E),
    "o" => Some(0x1F),
    "u" => Some(0x20),
    "[" => Some(0x21),
    "i" => Some(0x22),
    "p" => Some(0x23),
    "l" => Some(0x25),
    "j" => Some(0x26),
    "'" => Some(0x27),
    "k" => Some(0x28),
    ";" => Some(0x29),
    "\\" => Some(0x2A),
    "," => Some(0x2B),
    "/" => Some(0x2C),
    "n" => Some(0x2D),
    "m" => Some(0x2E),
    "." => Some(0x2F),
    "`" => Some(0x32),
    "enter" => Some(0x24),
    "tab" => Some(0x30),
    "space" => Some(0x31),
    "backspace" => Some(0x33),
    "escape" => Some(0x35),
    "arrowleft" => Some(0x7B),
    "arrowright" => Some(0x7C),
    "arrowdown" => Some(0x7D),
    "arrowup" => Some(0x7E),
    "f1" => Some(0x7A),
    "f2" => Some(0x78),
    "f3" => Some(0x63),
    "f4" => Some(0x76),
    "f5" => Some(0x60),
    "f6" => Some(0x61),
    "f7" => Some(0x62),
    "f8" => Some(0x64),
    "f9" => Some(0x65),
    "f10" => Some(0x6D),
    "f11" => Some(0x67),
    "f12" => Some(0x6F),
    _ => None,
  }
}

#[cfg(target_os = "macos")]
fn spawn_macos_event_tap(sender: Sender<MacosRawKeyEvent>) {
  thread::Builder::new()
    .name("global-key-listener".to_owned())
    .spawn(move || {
      let sender = Box::into_raw(Box::new(sender));
      let mask = (1_u64 << K_CG_EVENT_KEY_DOWN) | (1_u64 << K_CG_EVENT_KEY_UP);
      let tap = unsafe {
        CGEventTapCreate(
          K_CG_SESSION_EVENT_TAP,
          K_CG_HEAD_INSERT_EVENT_TAP,
          K_CG_EVENT_TAP_OPTION_LISTEN_ONLY,
          mask,
          Some(macos_event_tap_callback),
          sender as *mut c_void,
        )
      };

      if tap.is_null() {
        unsafe {
          drop(Box::from_raw(sender));
        }
        return;
      }

      let source = unsafe { CFMachPortCreateRunLoopSource(ptr::null_mut(), tap, 0) };
      if source.is_null() {
        unsafe {
          CFRelease(tap as *const c_void);
          drop(Box::from_raw(sender));
        }
        return;
      }

      unsafe {
        CFRunLoopAddSource(CFRunLoopGetCurrent(), source, kCFRunLoopCommonModes);
        CGEventTapEnable(tap, true);
        CFRunLoopRun();
        CFRelease(source as *const c_void);
        CFRelease(tap as *const c_void);
        drop(Box::from_raw(sender));
      }
    })
    .expect("failed to spawn macOS global key listener");
}

#[cfg(target_os = "macos")]
extern "C" fn macos_event_tap_callback(
  _proxy: CGEventTapProxy,
  event_type: u32,
  event: CGEventRef,
  user_info: *mut c_void,
) -> CGEventRef {
  let pressed = match event_type {
    K_CG_EVENT_KEY_DOWN => true,
    K_CG_EVENT_KEY_UP => false,
    _ => return event,
  };

  let key_code = unsafe { CGEventGetIntegerValueField(event, K_CG_KEYBOARD_EVENT_KEYCODE) as u32 };
  let modifiers = macos_modifiers_from_flags(unsafe { CGEventGetFlags(event) });
  if !user_info.is_null() {
    let sender = unsafe { &*(user_info as *const Sender<MacosRawKeyEvent>) };
    let _ = sender.send(MacosRawKeyEvent {
      key_code,
      modifiers,
      pressed,
    });
  }

  event
}

#[cfg(target_os = "macos")]
fn macos_modifiers_from_flags(flags: u64) -> u32 {
  let mut modifiers = 0;
  if flags & K_CG_EVENT_FLAG_MASK_CONTROL != 0 {
    modifiers |= CONTROL_KEY;
  }
  if flags & K_CG_EVENT_FLAG_MASK_ALTERNATE != 0 {
    modifiers |= OPTION_KEY;
  }
  if flags & K_CG_EVENT_FLAG_MASK_SHIFT != 0 {
    modifiers |= SHIFT_KEY;
  }
  if flags & K_CG_EVENT_FLAG_MASK_COMMAND != 0 {
    modifiers |= CMD_KEY;
  }
  modifiers
}

#[cfg(target_os = "macos")]
#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
  static kCFRunLoopCommonModes: CFStringRef;

  fn CGEventTapCreate(
    tap: u32,
    place: u32,
    options: u32,
    events_of_interest: u64,
    callback: CGEventTapCallBack,
    user_info: *mut c_void,
  ) -> CFMachPortRef;
  fn CGEventTapEnable(tap: CFMachPortRef, enable: bool);
  fn CGEventGetIntegerValueField(event: CGEventRef, field: u32) -> i64;
  fn CGEventGetFlags(event: CGEventRef) -> u64;
  fn CFMachPortCreateRunLoopSource(allocator: CFAllocatorRef, port: CFMachPortRef, order: isize) -> CFRunLoopSourceRef;
  fn CFRunLoopGetCurrent() -> CFRunLoopRef;
  fn CFRunLoopAddSource(rl: CFRunLoopRef, source: CFRunLoopSourceRef, mode: CFStringRef);
  fn CFRunLoopRun();
  fn CFRelease(cf: *const c_void);
}

#[cfg(target_os = "macos")]
type CGEventTapProxy = *mut c_void;
#[cfg(target_os = "macos")]
type CGEventRef = *mut c_void;
#[cfg(target_os = "macos")]
type CFMachPortRef = *mut c_void;
#[cfg(target_os = "macos")]
type CFRunLoopSourceRef = *mut c_void;
#[cfg(target_os = "macos")]
type CFRunLoopRef = *mut c_void;
#[cfg(target_os = "macos")]
type CFStringRef = *const c_void;
#[cfg(target_os = "macos")]
type CFAllocatorRef = *mut c_void;
#[cfg(target_os = "macos")]
type CGEventTapCallBack = Option<extern "C" fn(CGEventTapProxy, u32, CGEventRef, *mut c_void) -> CGEventRef>;

#[cfg(target_os = "macos")]
const K_CG_SESSION_EVENT_TAP: u32 = 1;
#[cfg(target_os = "macos")]
const K_CG_HEAD_INSERT_EVENT_TAP: u32 = 0;
#[cfg(target_os = "macos")]
const K_CG_EVENT_TAP_OPTION_LISTEN_ONLY: u32 = 1;
#[cfg(target_os = "macos")]
const K_CG_EVENT_KEY_DOWN: u32 = 10;
#[cfg(target_os = "macos")]
const K_CG_EVENT_KEY_UP: u32 = 11;
#[cfg(target_os = "macos")]
const K_CG_KEYBOARD_EVENT_KEYCODE: u32 = 9;
#[cfg(target_os = "macos")]
const K_CG_EVENT_FLAG_MASK_SHIFT: u64 = 1 << 17;
#[cfg(target_os = "macos")]
const K_CG_EVENT_FLAG_MASK_CONTROL: u64 = 1 << 18;
#[cfg(target_os = "macos")]
const K_CG_EVENT_FLAG_MASK_ALTERNATE: u64 = 1 << 19;
#[cfg(target_os = "macos")]
const K_CG_EVENT_FLAG_MASK_COMMAND: u64 = 1 << 20;
#[cfg(target_os = "macos")]
const CONTROL_KEY: u32 = 1 << 0;
#[cfg(target_os = "macos")]
const OPTION_KEY: u32 = 1 << 1;
#[cfg(target_os = "macos")]
const SHIFT_KEY: u32 = 1 << 2;
#[cfg(target_os = "macos")]
const CMD_KEY: u32 = 1 << 3;

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
