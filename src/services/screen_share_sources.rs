use xcap::{Monitor, Window};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScreenShareSourceKind {
  Screen,
  Window,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScreenShareSource {
  pub kind: ScreenShareSourceKind,
  pub id: u32,
  pub name: String,
  pub description: String,
  pub resolution: Option<String>,
}

pub fn list_screen_sources() -> Vec<ScreenShareSource> {
  let Ok(monitors) = Monitor::all() else {
    return Vec::new();
  };

  monitors
    .into_iter()
    .enumerate()
    .map(|(index, monitor)| monitor_source(index, monitor))
    .collect()
}

pub fn list_window_sources() -> Vec<ScreenShareSource> {
  let Ok(windows) = Window::all() else {
    return Vec::new();
  };

  windows.into_iter().filter_map(window_source).take(12).collect()
}

fn monitor_source(index: usize, monitor: Monitor) -> ScreenShareSource {
  let id = monitor.id().unwrap_or(index as u32);
  let friendly_name = monitor.friendly_name().unwrap_or_default();
  let name = if friendly_name.trim().is_empty() {
    let raw_name = monitor.name().unwrap_or_default();
    if raw_name.trim().is_empty() {
      format!("Display {}", index + 1)
    } else {
      raw_name
    }
  } else {
    friendly_name
  };

  let width = monitor.width().unwrap_or(0);
  let height = monitor.height().unwrap_or(0);
  let resolution = source_resolution(width, height);
  let primary = monitor.is_primary().unwrap_or(false);
  let builtin = monitor.is_builtin().unwrap_or(false);
  let mut details = Vec::new();

  if primary {
    details.push("Primary".to_owned());
  }

  if builtin {
    details.push("Built-in".to_owned());
  }

  ScreenShareSource {
    kind: ScreenShareSourceKind::Screen,
    id,
    name,
    description: details.join(" · "),
    resolution,
  }
}

fn window_source(window: Window) -> Option<ScreenShareSource> {
  if window.is_minimized().unwrap_or(false) {
    return None;
  }

  let id = window.id().ok()?;
  let app_name = window.app_name().unwrap_or_default();
  let title = window.title().unwrap_or_default();
  let name = source_window_name(&app_name, &title)?;
  let width = window.width().unwrap_or(0);
  let height = window.height().unwrap_or(0);
  let resolution = source_resolution(width, height);

  Some(ScreenShareSource {
    kind: ScreenShareSourceKind::Window,
    id,
    name,
    description: app_name,
    resolution,
  })
}

fn source_resolution(width: u32, height: u32) -> Option<String> {
  if width > 0 && height > 0 {
    Some(format!("{width}x{height}"))
  } else {
    None
  }
}

fn source_window_name(app_name: &str, title: &str) -> Option<String> {
  let app_name = app_name.trim();
  let title = title.trim();

  match (app_name.is_empty(), title.is_empty()) {
    (true, true) => None,
    (false, true) => Some(app_name.to_owned()),
    (true, false) => Some(title.to_owned()),
    (false, false) if title == app_name => Some(title.to_owned()),
    (false, false) => Some(format!("{app_name} - {title}")),
  }
}
