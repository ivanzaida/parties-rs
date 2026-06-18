#![cfg_attr(all(target_os = "windows", not(debug_assertions)), windows_subsystem = "windows")]

#[cfg(target_os = "windows")]
use std::ffi::{CStr, c_char};
use std::{
  panic,
  path::PathBuf,
  sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
  },
  time::{Duration, Instant},
};

#[cfg(target_os = "windows")]
use client::windows_diagnostics;
use client::{
  app, i18n, services,
  session::ServerSession,
  storage::{Storage, WindowState},
  theme, ui,
  ui::app_chrome::{CUSTOM_MACOS_CHROME, CUSTOM_WINDOW_CHROME, FrameRateSignal},
};
use lurq::{
  app::{WindowCornerRadius, WindowIcon},
  core::Signal,
  persistent_storage::{PersistentStorage, PersistentWrite},
};

const DEFAULT_WINDOW_WIDTH: u32 = 1280;
const DEFAULT_WINDOW_HEIGHT: u32 = 900;
const MIN_WINDOW_WIDTH: u32 = 768;
const MIN_WINDOW_HEIGHT: u32 = 640;
const FPS_SAMPLE_INTERVAL: Duration = Duration::from_millis(500);
const WINDOW_STATE_SAVE_DEBOUNCE: Duration = Duration::from_millis(200);
const WINDOW_X_KEY: &str = "window.x";
const WINDOW_Y_KEY: &str = "window.y";
const WINDOW_WIDTH_KEY: &str = "window.width";
const WINDOW_HEIGHT_KEY: &str = "window.height";
const WINDOW_FULL_SCREEN_KEY: &str = "window.full_screen";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ScreenBounds {
  x: i32,
  y: i32,
  width: u32,
  height: u32,
}

struct FpsSampler {
  signal: Signal<u32>,
  last_sample: Instant,
  frames_since_sample: u32,
}

#[derive(Clone)]
struct WindowStateSaveScheduler {
  tokio: tokio::runtime::Handle,
  storage: PersistentStorage,
  last_saved: Arc<Mutex<Option<WindowState>>>,
  pending: Arc<Mutex<Option<WindowState>>>,
  scheduled: Arc<AtomicBool>,
}

impl WindowStateSaveScheduler {
  fn new(tokio: tokio::runtime::Handle, storage: PersistentStorage, last_saved: Option<WindowState>) -> Self {
    Self {
      tokio,
      storage,
      last_saved: Arc::new(Mutex::new(last_saved)),
      pending: Arc::new(Mutex::new(None)),
      scheduled: Arc::new(AtomicBool::new(false)),
    }
  }

  fn schedule(&self, state: WindowState) {
    *self.pending.lock().expect("window state save lock poisoned") = Some(state);
    if self.scheduled.swap(true, Ordering::AcqRel) {
      return;
    }

    let scheduler = self.clone();
    self.tokio.spawn(async move {
      scheduler.run().await;
    });
  }

  async fn run(self) {
    loop {
      tokio::time::sleep(WINDOW_STATE_SAVE_DEBOUNCE).await;
      if let Some(state) = self.pending.lock().expect("window state save lock poisoned").take() {
        let should_save = {
          let last_saved = self.last_saved.lock().expect("window state lock poisoned");
          !last_saved.is_some_and(|last_saved| window_bounds_equal(last_saved, state))
        };

        if should_save && save_window_bounds_to_persistent(&self.storage, state) {
          *self.last_saved.lock().expect("window state lock poisoned") = Some(state);
        }
      }

      if self.pending.lock().expect("window state save lock poisoned").is_none() {
        self.scheduled.store(false, Ordering::Release);
        if self.pending.lock().expect("window state save lock poisoned").is_none() {
          return;
        }
        if self.scheduled.swap(true, Ordering::AcqRel) {
          return;
        }
      }
    }
  }
}

impl FpsSampler {
  fn new(signal: Signal<u32>) -> Self {
    Self {
      signal,
      last_sample: Instant::now(),
      frames_since_sample: 0,
    }
  }

  fn record_frame(&mut self) -> Option<u32> {
    self.frames_since_sample = self.frames_since_sample.saturating_add(1);
    let now = Instant::now();
    let elapsed = now.duration_since(self.last_sample);
    if elapsed < FPS_SAMPLE_INTERVAL {
      return None;
    }

    let fps = (self.frames_since_sample as f32 / elapsed.as_secs_f32()).round() as u32;
    if self.signal.with_untracked(|current| *current) != fps {
      self.signal.set(fps);
    }
    self.frames_since_sample = 0;
    self.last_sample = now;
    Some(fps)
  }
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
  let (startup_storage, startup_error) = load_startup_storage();
  #[cfg(target_os = "windows")]
  let dx12_video_surfaces = lurq::app::dx12_render::Dx12VideoSurfaceAllocator::new();
  #[cfg(target_os = "windows")]
  let session = ServerSession::with_dx12_video_surface_allocator(dx12_video_surfaces.clone());
  #[cfg(not(target_os = "windows"))]
  let session = ServerSession::default();
  install_shutdown_handlers(&tokio_runtime, session.clone());

  let mut lurq_app = lurq::app::App::new();
  lurq_app.set_tokio_handle(tokio_runtime.handle().clone());
  if let Err(error) = lurq_app.set_persistent_storage_path(persistent_storage_path()) {
    tracing::warn!(target: "window::state", "failed to open persistent storage: {error}");
  }
  let persistent_storage = lurq_app.persistent_storage().clone();
  let window_state =
    load_startup_window_state(&persistent_storage, startup_storage.as_ref()).map(validate_startup_window_state);
  if let Some(state) = window_state {
    save_window_state_to_persistent(&persistent_storage, state);
  }
  let startup_full_screen = window_state.is_some_and(|state| state.full_screen);
  let frame_rate_signal = Signal::new(0);

  let assets = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets");
  lurq_app.set_resource_root(assets.clone());
  lurq_app.load_fonts_dir(assets.join("fonts").as_path());
  lurq::app::devtools::load_fonts(&mut lurq_app);
  lurq_app.register_font("Inter", "Inter");
  lurq_app.register_font("JetBrains Mono", "JetBrains Mono");
  lurq_app.register_font("Lucide", "Lucide");
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
      frame_rate: FrameRateSignal(frame_rate_signal.clone()),
      startup_full_screen,
    },
  );
  tree.mount_devtools(&mut lurq_app);
  let mut fps_sampler = FpsSampler::new(frame_rate_signal);

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
    })
    .on_paint(move |tree, _, report| {
      if let Some(fps) = fps_sampler.record_frame() {
        if tracing::enabled!(target: "frame-profile", tracing::Level::TRACE) {
          tracing::trace!(
            target: "frame-profile",
            "[frame-profile] fps={} cached={} layout_updated={} layout_recalculated={} reasons={:?} {}",
            fps,
            report.used_cached_render_list,
            report.layout_updated,
            report.layout_recalculated,
            report.reasons,
            tree.last_profile()
          );
        }
      }
    });

  if let Some(state) = window_state {
    window = window.with_position(state.x, state.y);
  }

  {
    let current_window_state = Arc::new(Mutex::new(window_state.unwrap_or_else(|| default_window_state(false))));
    let save_scheduler =
      WindowStateSaveScheduler::new(tokio_runtime.handle().clone(), persistent_storage.clone(), window_state);
    let move_state = current_window_state.clone();
    let move_save_scheduler = save_scheduler.clone();
    window = window.on_position_changed(move |x, y| {
      let state = {
        let mut state = move_state.lock().expect("window state lock poisoned");
        state.x = x;
        state.y = y;
        *state
      };
      move_save_scheduler.schedule(state);
    });
    let resize_state = current_window_state;
    let resize_save_scheduler = save_scheduler;
    window = window.on_size_changed(move |width, height| {
      let state = {
        let mut state = resize_state.lock().expect("window state lock poisoned");
        state.width = width;
        state.height = height;
        *state
      };
      resize_save_scheduler.schedule(state);
    });
  }

  window.run();
  session.disconnect_for_shutdown();
}

#[cfg(target_os = "windows")]
fn log_startup_gpu_info() {
  use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, DXGI_ERROR_NOT_FOUND, IDXGIFactory1};

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

  log_dxgi_gpu_preference_adapters(&factory);
  log_dx12_renderer_adapter(&factory);
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
fn log_dxgi_gpu_preference_adapters(factory: &windows::Win32::Graphics::Dxgi::IDXGIFactory1) {
  use windows::Win32::Graphics::Dxgi::{
    DXGI_GPU_PREFERENCE_HIGH_PERFORMANCE, DXGI_GPU_PREFERENCE_MINIMUM_POWER, DXGI_GPU_PREFERENCE_UNSPECIFIED,
    IDXGIAdapter1, IDXGIFactory6,
  };
  use windows_core::Interface;

  let Ok(factory) = factory.cast::<IDXGIFactory6>() else {
    tracing::warn!(target: "startup::gpu", "[startup/gpu] DXGI factory does not support GPU preference enumeration");
    return;
  };

  for (label, preference) in [
    ("unspecified", DXGI_GPU_PREFERENCE_UNSPECIFIED),
    ("high_performance", DXGI_GPU_PREFERENCE_HIGH_PERFORMANCE),
    ("minimum_power", DXGI_GPU_PREFERENCE_MINIMUM_POWER),
  ] {
    match unsafe { factory.EnumAdapterByGpuPreference::<IDXGIAdapter1>(0, preference) } {
      Ok(adapter) => log_selected_dxgi_adapter(format_args!("dxgi-preference-{label}"), &adapter),
      Err(error) => tracing::warn!(
        target: "startup::gpu",
        "[startup/gpu] dxgi-preference-{label}: failed to resolve adapter: {error}"
      ),
    }
  }
}

#[cfg(target_os = "windows")]
fn log_dx12_renderer_adapter(factory: &windows::Win32::Graphics::Dxgi::IDXGIFactory1) {
  use windows::Win32::Graphics::Dxgi::{DXGI_ADAPTER_FLAG_SOFTWARE, DXGI_ERROR_NOT_FOUND};

  if let Some((adapter_index, adapter)) = preferred_dx12_adapter(factory) {
    log_selected_dxgi_adapter(
      format_args!("dx12-renderer-selected source=dxgi-preference-unspecified index={adapter_index}"),
      &adapter,
    );
    return;
  }

  let mut adapter_index = 0;
  loop {
    let adapter = match unsafe { factory.EnumAdapters1(adapter_index) } {
      Ok(adapter) => adapter,
      Err(error) if error.code() == DXGI_ERROR_NOT_FOUND => {
        tracing::warn!(target: "startup::gpu", "[startup/gpu] dx12-renderer-selected: no DX12-capable hardware adapter found");
        return;
      }
      Err(error) => {
        tracing::warn!(target: "startup::gpu", "[startup/gpu] dx12-renderer-selected: failed to enumerate adapter #{adapter_index}: {error}");
        return;
      }
    };

    let desc = match unsafe { adapter.GetDesc1() } {
      Ok(desc) => desc,
      Err(error) => {
        tracing::warn!(target: "startup::gpu", "[startup/gpu] dx12-renderer-selected: failed to read adapter #{adapter_index}: {error}");
        return;
      }
    };

    if desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 == 0 && can_create_dx12_device(&adapter) {
      log_selected_dxgi_adapter(format_args!("dx12-renderer-selected index={adapter_index}"), &adapter);
      return;
    }

    adapter_index += 1;
  }
}

#[cfg(target_os = "windows")]
fn preferred_dx12_adapter(
  factory: &windows::Win32::Graphics::Dxgi::IDXGIFactory1,
) -> Option<(u32, windows::Win32::Graphics::Dxgi::IDXGIAdapter1)> {
  use windows::Win32::Graphics::Dxgi::{
    DXGI_ADAPTER_FLAG_SOFTWARE, DXGI_GPU_PREFERENCE_UNSPECIFIED, IDXGIAdapter1, IDXGIFactory6,
  };
  use windows_core::Interface;

  let factory = factory.cast::<IDXGIFactory6>().ok()?;
  let mut adapter_index = 0;
  loop {
    let adapter =
      unsafe { factory.EnumAdapterByGpuPreference::<IDXGIAdapter1>(adapter_index, DXGI_GPU_PREFERENCE_UNSPECIFIED) }
        .ok()?;
    let desc = unsafe { adapter.GetDesc1() }.ok()?;
    if desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 == 0 && can_create_dx12_device(&adapter) {
      return Some((adapter_index, adapter));
    }
    adapter_index += 1;
  }
}

#[cfg(target_os = "windows")]
fn can_create_dx12_device(adapter: &windows::Win32::Graphics::Dxgi::IDXGIAdapter1) -> bool {
  use windows::Win32::Graphics::{
    Direct3D::D3D_FEATURE_LEVEL_11_0,
    Direct3D12::{D3D12CreateDevice, ID3D12Device},
  };

  let mut device = None::<ID3D12Device>;
  unsafe { D3D12CreateDevice(adapter, D3D_FEATURE_LEVEL_11_0, &mut device) }.is_ok()
}

#[cfg(target_os = "windows")]
fn log_selected_dxgi_adapter(label: std::fmt::Arguments<'_>, adapter: &windows::Win32::Graphics::Dxgi::IDXGIAdapter1) {
  let desc = match unsafe { adapter.GetDesc1() } {
    Ok(desc) => desc,
    Err(error) => {
      tracing::warn!(target: "startup::gpu", "[startup/gpu] {label}: failed to read desc: {error}");
      return;
    }
  };

  let name = utf16_null_terminated_to_string(&desc.Description);
  let vendor = gpu_vendor_label(desc.VendorId);
  let dedicated_vram_mb = desc.DedicatedVideoMemory / (1024 * 1024);
  let output_count = dxgi_output_count(adapter);
  tracing::info!(
    target: "startup::gpu",
    "[startup/gpu] {label}: vendor={vendor} vendor_id=0x{:04x} device_id=0x{:04x} luid={:08x}:{:08x} name='{}' dedicated_vram={}MB outputs={} flags=0x{:x}",
    desc.VendorId,
    desc.DeviceId,
    desc.AdapterLuid.HighPart as u32,
    desc.AdapterLuid.LowPart,
    name,
    dedicated_vram_mb,
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
    if services::voice::is_catching_input_capture_callback_panic() {
      return;
    }
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

fn load_startup_storage() -> (Option<Storage>, Option<String>) {
  let storage = match Storage::open_default() {
    Ok(storage) => storage,
    Err(error) => return (None, Some(error.to_string())),
  };

  (Some(storage), None)
}

fn persistent_storage_path() -> PathBuf {
  Storage::default_data_dir().join("lurq.redb")
}

fn load_startup_window_state(
  persistent_storage: &PersistentStorage,
  fallback_storage: Option<&Storage>,
) -> Option<WindowState> {
  load_window_state_from_persistent(persistent_storage).or_else(|| {
    fallback_storage
      .and_then(|storage| storage.load_window_state().ok().flatten())
      .map(validate_startup_window_state)
  })
}

fn load_window_state_from_persistent(storage: &PersistentStorage) -> Option<WindowState> {
  let values = storage
    .read_bulk([
      WINDOW_X_KEY,
      WINDOW_Y_KEY,
      WINDOW_WIDTH_KEY,
      WINDOW_HEIGHT_KEY,
      WINDOW_FULL_SCREEN_KEY,
    ])
    .ok()?;

  Some(WindowState {
    x: values.value(WINDOW_X_KEY)?,
    y: values.value(WINDOW_Y_KEY)?,
    width: values.value(WINDOW_WIDTH_KEY)?,
    height: values.value(WINDOW_HEIGHT_KEY)?,
    full_screen: values.value(WINDOW_FULL_SCREEN_KEY).unwrap_or(false),
  })
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

fn window_bounds_equal(a: WindowState, b: WindowState) -> bool {
  a.x == b.x && a.y == b.y && a.width == b.width && a.height == b.height
}

fn save_window_bounds_to_persistent(storage: &PersistentStorage, state: WindowState) -> bool {
  match storage.write_bulk([
    PersistentWrite::new(WINDOW_X_KEY, state.x),
    PersistentWrite::new(WINDOW_Y_KEY, state.y),
    PersistentWrite::new(WINDOW_WIDTH_KEY, state.width),
    PersistentWrite::new(WINDOW_HEIGHT_KEY, state.height),
  ]) {
    Ok(()) => true,
    Err(error) => {
      tracing::warn!(target: "window::state", "failed to save window bounds: {error}");
      false
    }
  }
}

fn save_window_state_to_persistent(storage: &PersistentStorage, state: WindowState) -> bool {
  match storage.write_bulk([
    PersistentWrite::new(WINDOW_X_KEY, state.x),
    PersistentWrite::new(WINDOW_Y_KEY, state.y),
    PersistentWrite::new(WINDOW_WIDTH_KEY, state.width),
    PersistentWrite::new(WINDOW_HEIGHT_KEY, state.height),
    PersistentWrite::new(WINDOW_FULL_SCREEN_KEY, state.full_screen),
  ]) {
    Ok(()) => true,
    Err(error) => {
      tracing::warn!(target: "window::state", "failed to save window state: {error}");
      false
    }
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
#[path = "../tests/unit/bin_main.rs"]
mod tests;
