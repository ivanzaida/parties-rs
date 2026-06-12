#![cfg_attr(all(target_os = "windows", not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod i18n;
mod identity;
mod network;
mod routes;
mod services;
mod session;
mod storage;
mod theme;
mod ui;
#[cfg(target_os = "windows")]
mod windows_diagnostics;

#[cfg(target_os = "windows")]
use std::ffi::{c_char, CStr};
use std::{
  panic,
  sync::{Arc, Mutex},
};

use lurq::app::{WindowCornerRadius, WindowIcon};
use session::ServerSession;
use storage::{Storage, WindowState};
use ui::app_chrome::{CUSTOM_MACOS_CHROME, CUSTOM_WINDOW_CHROME};

const DEFAULT_WINDOW_WIDTH: u32 = 1280;
const DEFAULT_WINDOW_HEIGHT: u32 = 900;
const MIN_WINDOW_WIDTH: u32 = 768;
const MIN_WINDOW_HEIGHT: u32 = 640;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ScreenBounds {
  x: i32,
  y: i32,
  width: u32,
  height: u32,
}

fn main() {
  services::logger::init();
  services::updater::start_platform_updater();
  #[cfg(target_os = "windows")]
  log_startup_gpu_info();
  #[cfg(target_os = "windows")]
  install_windows_native_diagnostics();
  let tokio_runtime = tokio::runtime::Builder::new_multi_thread()
    .enable_all()
    .build()
    .expect("failed to create tokio runtime");
  let (startup_storage, window_state, startup_error) = load_startup_storage();
  let window_state = window_state.map(validate_startup_window_state);
  let startup_full_screen = window_state.is_some_and(|state| state.full_screen);
  let window_state_tracker = startup_storage.as_ref().map(|_| app::WindowStateTracker {
    current: Arc::new(Mutex::new(window_state.unwrap_or_else(|| default_window_state(false)))),
    last_saved: Arc::new(Mutex::new(window_state)),
  });
  #[cfg(target_os = "windows")]
  let dx12_video_surfaces = lurq::app::dx12_render::Dx12VideoSurfaceAllocator::new();
  #[cfg(target_os = "windows")]
  let session = ServerSession::with_dx12_video_surface_allocator(dx12_video_surfaces.clone());
  #[cfg(not(target_os = "windows"))]
  let session = ServerSession::default();
  install_shutdown_handlers(&tokio_runtime, session.clone());

  let mut lurq_app = lurq::app::App::new();
  lurq_app.set_tokio_handle(tokio_runtime.handle().clone());

  let assets = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets");
  lurq_app.set_resource_root(assets.clone());
  lurq_app.load_fonts_dir(assets.join("fonts").as_path());
  lurq_app.register_font("Inter", "Inter");
  lurq_app.register_font("JetBrains Mono", "JetBrains Mono");
  lurq_app.register_font("Lucide", "Lucide");
  lurq::app::devtools::load_fonts(&mut lurq_app);
  theme::setup(lurq_app.theme());
  i18n::setup(lurq_app.i18n());

  let mut tree = lurq::app::runtime::Tree::new();
  ui::loader::register_keyframes(&mut tree);
  #[cfg(target_os = "windows")]
  tree.set_render_engine_factory(move || {
    Box::new(lurq::app::dx12_render::Dx12RenderEngine::with_video_surface_allocator(
      dx12_video_surfaces.clone(),
    ))
  });
  #[cfg(not(target_os = "windows"))]
  tree.set_render_engine_factory(|| Box::new(lurq::app::wgpu_render::WgpuRenderEngine::new()));
  tree.mount_root::<app::App>(
    &mut lurq_app,
    app::AppProps {
      tokio: tokio_runtime.handle().clone(),
      startup_storage: startup_storage.clone(),
      startup_error,
      session: session.clone(),
      startup_full_screen,
      window_state_tracker: window_state_tracker.clone(),
    },
  );
  tree.mount_devtools(&mut lurq_app);

  let mut window = lurq::app::winit_shell::WinitWindow::new(lurq_app, tree)
    .with_title("Parties")
    .with_size(
      window_state.map_or(DEFAULT_WINDOW_WIDTH, |state| state.width),
      window_state.map_or(DEFAULT_WINDOW_HEIGHT, |state| state.height),
    )
    .with_min_size(MIN_WINDOW_WIDTH, MIN_WINDOW_HEIGHT)
    .with_icon(app_window_icon())
    .with_decorations(!CUSTOM_WINDOW_CHROME)
    .with_transparent(CUSTOM_MACOS_CHROME)
    .with_corner_radius(if CUSTOM_WINDOW_CHROME {
      WindowCornerRadius::Rounded
    } else {
      WindowCornerRadius::Default
    });

  if let Some(state) = window_state {
    window = window.with_position(state.x, state.y);
  }

  if let (Some(storage), Some(window_state_tracker)) = (startup_storage, window_state_tracker) {
    let current_window_state = window_state_tracker.current.clone();
    let last_saved_state = window_state_tracker.last_saved.clone();
    let move_storage = storage.clone();
    let move_state = current_window_state.clone();
    let move_last_saved_state = last_saved_state.clone();
    window = window.on_position_changed(move |x, y| {
      let state = {
        let mut state = move_state.lock().expect("window state lock poisoned");
        state.x = x;
        state.y = y;
        *state
      };
      let mut last_saved_state = move_last_saved_state.lock().expect("window state lock poisoned");
      if *last_saved_state == Some(state) {
        return;
      }

      if move_storage.save_window_state(state).is_ok() {
        *last_saved_state = Some(state);
      }
    });
    let resize_state = current_window_state;
    let resize_last_saved_state = last_saved_state;
    window = window.on_size_changed(move |width, height| {
      let state = {
        let mut state = resize_state.lock().expect("window state lock poisoned");
        state.width = width;
        state.height = height;
        *state
      };
      let mut last_saved_state = resize_last_saved_state.lock().expect("window state lock poisoned");
      if *last_saved_state == Some(state) {
        return;
      }

      if storage.save_window_state(state).is_ok() {
        *last_saved_state = Some(state);
      }
    });
  }

  window.run();
  session.disconnect_for_shutdown();
}

#[cfg(target_os = "windows")]
fn log_startup_gpu_info() {
  use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIFactory1, DXGI_ERROR_NOT_FOUND};

  let Ok(factory) = (unsafe { CreateDXGIFactory1::<IDXGIFactory1>() }) else {
    tracing::error!(target: "startup::gpu", "[startup/gpu] failed to create DXGI factory");
    return;
  };

  let mut adapter_index = 0;
  let mut logged_any = false;
  loop {
    let adapter = match unsafe { factory.EnumAdapters1(adapter_index) } {
      Ok(adapter) => adapter,
      Err(error) if error.code() == DXGI_ERROR_NOT_FOUND => break,
      Err(error) => {
        tracing::warn!(target: "startup::gpu", "[startup/gpu] failed to enumerate DXGI adapter #{adapter_index}: {error}");
        break;
      }
    };
    log_dxgi_adapter(adapter_index, &adapter);
    logged_any = true;
    adapter_index += 1;
  }

  if !logged_any {
    tracing::warn!(target: "startup::gpu", "[startup/gpu] no DXGI adapters found");
  }
}

#[cfg(target_os = "windows")]
fn log_dxgi_adapter(index: u32, adapter: &windows::Win32::Graphics::Dxgi::IDXGIAdapter1) {
  let desc = match unsafe { adapter.GetDesc1() } {
    Ok(desc) => desc,
    Err(error) => {
      tracing::warn!(target: "startup::gpu", "[startup/gpu] adapter #{index}: failed to read desc: {error}");
      return;
    }
  };
  let name = utf16_null_terminated_to_string(&desc.Description);
  let vendor = gpu_vendor_label(desc.VendorId);
  let dedicated_vram_mb = desc.DedicatedVideoMemory / (1024 * 1024);
  let shared_memory_mb = desc.SharedSystemMemory / (1024 * 1024);
  let output_count = dxgi_output_count(adapter);
  let default_marker = if index == 0 { " default=true" } else { "" };
  tracing::info!(target: "startup::gpu",
    "[startup/gpu] adapter #{index}:{default_marker} vendor={vendor} vendor_id=0x{:04x} device_id=0x{:04x} name='{}' dedicated_vram={}MB shared_memory={}MB outputs={} flags=0x{:x}",
    desc.VendorId,
    desc.DeviceId,
    name,
    dedicated_vram_mb,
    shared_memory_mb,
    output_count,
    desc.Flags
  );
}

#[cfg(target_os = "windows")]
fn dxgi_output_count(adapter: &windows::Win32::Graphics::Dxgi::IDXGIAdapter1) -> u32 {
  use windows::Win32::Graphics::Dxgi::DXGI_ERROR_NOT_FOUND;

  let mut output_index = 0;
  loop {
    match unsafe { adapter.EnumOutputs(output_index) } {
      Ok(_) => output_index + 1,
      Err(error) if error.code() == DXGI_ERROR_NOT_FOUND => return output_index,
      Err(_) => return output_index,
    };
    output_index += 1;
  }
}

#[cfg(target_os = "windows")]
fn utf16_null_terminated_to_string(value: &[u16]) -> String {
  let len = value
    .iter()
    .position(|code_unit| *code_unit == 0)
    .unwrap_or(value.len());
  String::from_utf16_lossy(&value[..len])
}

#[cfg(target_os = "windows")]
fn gpu_vendor_label(vendor_id: u32) -> &'static str {
  match vendor_id {
    0x1002 => "AMD",
    0x10DE => "NVIDIA",
    0x8086 => "Intel",
    0x1414 => "Microsoft",
    _ => "Unknown",
  }
}

fn app_window_icon() -> Option<WindowIcon> {
  lurq::images::ImageData::from_bytes(ui::brand_logo::LOGO_BYTES)
    .ok()
    .map(|image| WindowIcon::from_image_data(&image))
}

fn install_shutdown_handlers(tokio_runtime: &tokio::runtime::Runtime, session: ServerSession) {
  let panic_session = session.clone();
  let default_panic_hook = panic::take_hook();
  panic::set_hook(Box::new(move |info| {
    panic_session.disconnect_for_shutdown();
    default_panic_hook(info);
  }));

  tokio_runtime.spawn(async move {
    if tokio::signal::ctrl_c().await.is_ok() {
      session.disconnect_for_shutdown();
      std::process::exit(0);
    }
  });
}

#[cfg(target_os = "windows")]
unsafe extern "C" {
  fn parties_native_log_set_callback(callback: Option<extern "C" fn(level: u8, message: *const c_char)>);
}

#[cfg(target_os = "windows")]
fn install_windows_native_diagnostics() {
  unsafe {
    parties_native_log_set_callback(Some(windows_native_log_callback));
  }
  windows_diagnostics::install();
}

#[cfg(target_os = "windows")]
extern "C" fn windows_native_log_callback(level: u8, message: *const c_char) {
  if message.is_null() {
    return;
  }

  let message = unsafe { CStr::from_ptr(message) }.to_string_lossy();
  match level {
    0 => tracing::debug!(target: "native::windows", "[native/windows/debug] {message}"),
    1 => tracing::info!(target: "native::windows", "[native/windows/info] {message}"),
    2 => tracing::warn!(target: "native::windows", "[native/windows/warn] {message}"),
    3 => tracing::error!(target: "native::windows", "[native/windows/error] {message}"),
    _ => tracing::warn!(target: "native::windows", "[native/windows/unknown] {message}"),
  }
}

fn load_startup_storage() -> (Option<Storage>, Option<WindowState>, Option<String>) {
  let storage = match Storage::open_default() {
    Ok(storage) => storage,
    Err(error) => return (None, None, Some(error.to_string())),
  };

  match storage.load_window_state() {
    Ok(state) => (Some(storage), state, None),
    Err(error) => (Some(storage), None, Some(error.to_string())),
  }
}

fn default_window_state(full_screen: bool) -> WindowState {
  WindowState {
    x: 0,
    y: 0,
    width: DEFAULT_WINDOW_WIDTH,
    height: DEFAULT_WINDOW_HEIGHT,
    full_screen,
  }
}

fn validate_startup_window_state(state: WindowState) -> WindowState {
  validate_window_state_for_screens(state, &startup_screen_bounds())
}

fn validate_window_state_for_screens(state: WindowState, screens: &[ScreenBounds]) -> WindowState {
  let state = clamp_window_state_size(state);
  if screens.is_empty() {
    return state;
  }
  if !window_size_fits_any_screen(state, screens) || !window_intersects_any_screen(state, screens) {
    return default_window_state(state.full_screen);
  }
  state
}

fn clamp_window_state_size(mut state: WindowState) -> WindowState {
  state.width = state.width.max(MIN_WINDOW_WIDTH);
  state.height = state.height.max(MIN_WINDOW_HEIGHT);
  state
}

fn startup_screen_bounds() -> Vec<ScreenBounds> {
  services::desktop_capture::DesktopCaptureSource::list_screens()
    .unwrap_or_default()
    .into_iter()
    .map(|screen| ScreenBounds {
      x: screen.x,
      y: screen.y,
      width: screen.width,
      height: screen.height,
    })
    .collect()
}

fn window_size_fits_any_screen(state: WindowState, screens: &[ScreenBounds]) -> bool {
  screens
    .iter()
    .any(|screen| state.width <= screen.width && state.height <= screen.height)
}

fn window_intersects_any_screen(state: WindowState, screens: &[ScreenBounds]) -> bool {
  screens
    .iter()
    .any(|screen| rects_intersect(state.x, state.y, state.width, state.height, screen))
}

fn rects_intersect(x: i32, y: i32, width: u32, height: u32, screen: &ScreenBounds) -> bool {
  let left = x as i64;
  let top = y as i64;
  let right = left + width as i64;
  let bottom = top + height as i64;
  let screen_left = screen.x as i64;
  let screen_top = screen.y as i64;
  let screen_right = screen_left + screen.width as i64;
  let screen_bottom = screen_top + screen.height as i64;

  left < screen_right && right > screen_left && top < screen_bottom && bottom > screen_top
}

#[cfg(test)]
mod tests {
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
}
