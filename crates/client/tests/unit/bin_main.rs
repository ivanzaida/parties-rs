use super::*;

const SCREEN: ScreenBounds = ScreenBounds {
  x: 0,
  y: 0,
  width: 1920,
  height: 1080,
};

fn window_state(x: i32, y: i32, width: u32, height: u32, full_screen: bool) -> WindowState {
  WindowState {
    x,
    y,
    width,
    height,
    full_screen,
  }
}

#[test]
fn startup_window_state_clamps_too_small_size() {
  let state = validate_window_state_for_screens(window_state(20, 30, 320, 240, true), &[SCREEN]);

  assert_eq!(state.x, 20);
  assert_eq!(state.y, 30);
  assert_eq!(state.width, MIN_WINDOW_WIDTH);
  assert_eq!(state.height, MIN_WINDOW_HEIGHT);
  assert!(state.full_screen);
}

#[test]
fn startup_window_state_resets_when_offscreen() {
  let state = validate_window_state_for_screens(window_state(5000, 5000, 1280, 900, false), &[SCREEN]);

  assert_eq!(state, default_window_state(false));
}

#[test]
fn startup_window_state_resets_when_too_large_for_screens() {
  let state = validate_window_state_for_screens(window_state(0, 0, 3840, 2160, true), &[SCREEN]);

  assert_eq!(state, default_window_state(true));
}

#[test]
fn startup_window_state_keeps_valid_secondary_screen_position() {
  let secondary = ScreenBounds {
    x: -1280,
    y: 0,
    width: 1280,
    height: 1024,
  };
  let state = validate_window_state_for_screens(window_state(-1000, 40, 900, 700, false), &[SCREEN, secondary]);

  assert_eq!(state, window_state(-1000, 40, 900, 700, false));
}
