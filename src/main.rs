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
use std::ffi::{CStr, c_char};
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

fn main() {
  services::logger::init();
  services::updater::start_platform_updater();
  #[cfg(target_os = "windows")]
  install_windows_native_diagnostics();
  let tokio_runtime = tokio::runtime::Builder::new_multi_thread()
    .enable_all()
    .build()
    .expect("failed to create tokio runtime");
  let (startup_storage, window_state, startup_error) = load_startup_storage();
  let window_state = window_state.map(clamp_window_state_size);
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

  if let Some(storage) = startup_storage {
    let current_window_state = Arc::new(Mutex::new(window_state.unwrap_or(WindowState {
      x: 0,
      y: 0,
      width: DEFAULT_WINDOW_WIDTH,
      height: DEFAULT_WINDOW_HEIGHT,
    })));
    let last_saved_state = Arc::new(Mutex::new(window_state));
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
  fn parties_native_seh_install();
}

#[cfg(target_os = "windows")]
fn install_windows_native_diagnostics() {
  unsafe {
    parties_native_log_set_callback(Some(windows_native_log_callback));
    parties_native_seh_install();
  }
}

#[cfg(target_os = "windows")]
extern "C" fn windows_native_log_callback(level: u8, message: *const c_char) {
  if message.is_null() {
    return;
  }

  let message = unsafe { CStr::from_ptr(message) }.to_string_lossy();
  let level = match level {
    0 => "debug",
    1 => "info",
    2 => "warn",
    3 => "error",
    _ => "unknown",
  };
  services::logger::log(&format!("[native/windows/{level}] {message}"));
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

fn clamp_window_state_size(mut state: WindowState) -> WindowState {
  state.width = state.width.max(MIN_WINDOW_WIDTH);
  state.height = state.height.max(MIN_WINDOW_HEIGHT);
  state
}
