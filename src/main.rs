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

use std::sync::{Arc, Mutex};

use storage::{Storage, WindowState};

const DEFAULT_WINDOW_WIDTH: u32 = 1280;
const DEFAULT_WINDOW_HEIGHT: u32 = 900;

fn main() {
  let tokio_runtime = tokio::runtime::Builder::new_multi_thread()
    .enable_all()
    .build()
    .expect("failed to create tokio runtime");
  let (startup_storage, window_state, startup_error) = load_startup_storage();

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
  tree.set_render_engine_factory(|| Box::new(lurq::app::wgpu_render::WgpuRenderEngine::new()));
  tree.mount_root::<app::App>(
    &mut lurq_app,
    app::AppProps {
      tokio: tokio_runtime.handle().clone(),
      startup_storage: startup_storage.clone(),
      startup_error,
    },
  );
  tree.mount_devtools(&mut lurq_app);

  let mut window = lurq::app::winit_shell::WinitWindow::new(lurq_app, tree)
    .with_title("Parties")
    .with_size(
      window_state.map_or(DEFAULT_WINDOW_WIDTH, |state| state.width),
      window_state.map_or(DEFAULT_WINDOW_HEIGHT, |state| state.height),
    )
    .with_decorations(false);

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
