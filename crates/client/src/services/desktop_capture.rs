#[cfg(target_os = "macos")]
use std::ffi::{CStr, c_char};
use std::{
  collections::HashMap,
  sync::{Mutex, OnceLock},
};
#[cfg(target_os = "windows")]
use std::{ffi::c_void, ptr};

#[cfg(target_os = "windows")]
use windows::{
  Win32::{
    Devices::Display::{
      DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME, DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME,
      DISPLAYCONFIG_DEVICE_INFO_HEADER, DISPLAYCONFIG_MODE_INFO, DISPLAYCONFIG_PATH_INFO,
      DISPLAYCONFIG_SOURCE_DEVICE_NAME, DISPLAYCONFIG_TARGET_DEVICE_NAME, DisplayConfigGetDeviceInfo,
      GetDisplayConfigBufferSizes, QDC_ONLY_ACTIVE_PATHS, QueryDisplayConfig,
    },
    Foundation::{CloseHandle, ERROR_SUCCESS, HWND, LPARAM, LUID, RECT},
    Graphics::Gdi::{
      BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS, DeleteDC,
      DeleteObject, EnumDisplayMonitors, GetMonitorInfoW, HBITMAP, HDC, HGDIOBJ, HMONITOR, MONITORINFO, MONITORINFOEXW,
      ReleaseDC, SRCCOPY, SelectObject,
    },
    Storage::Xps::{PRINT_WINDOW_FLAGS, PrintWindow},
    System::Threading::{
      OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
    },
    UI::WindowsAndMessaging::{
      EnumWindows, GetWindowRect, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId, IsIconic,
      IsWindowVisible,
    },
  },
  core::{BOOL, PWSTR},
};

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub enum DesktopCaptureSourceKind {
  Screen,
  Window,
}

#[derive(Clone)]
pub struct DesktopCaptureSource {
  pub kind: DesktopCaptureSourceKind,
  pub id: u32,
  pub name: String,
  pub description: String,
  pub x: i32,
  pub y: i32,
  pub width: u32,
  pub height: u32,
}

#[derive(Clone, Copy)]
struct WindowMetadata {
  process_id: u32,
}

#[cfg(target_os = "windows")]
struct MonitorEnumState {
  screens: Vec<DesktopCaptureSource>,
  friendly_names: HashMap<String, String>,
}

pub struct DesktopFrame {
  pub rgba: Vec<u8>,
  pub width: u32,
  pub height: u32,
}

static WINDOW_CACHE: OnceLock<Mutex<HashMap<u32, WindowMetadata>>> = OnceLock::new();

#[cfg(target_os = "windows")]
unsafe extern "C" {
  fn parties_wgc_snapshot_capture(
    source_kind: u8,
    source_handle: usize,
    timeout_ms: u32,
    out_width: *mut u32,
    out_height: *mut u32,
    out_len: *mut usize,
  ) -> *mut u8;
  fn parties_wgc_snapshot_free(bytes: *mut u8);
}

#[cfg(target_os = "macos")]
unsafe extern "C" {
  fn parties_macos_desktop_sources_json(source_kind: u8) -> *mut c_char;
  fn parties_macos_string_free(text: *mut c_char);
}

pub fn find_window_process_id(id: u32) -> Result<u32, String> {
  cached_window(id)
    .map(|metadata| metadata.process_id)
    .or_else(|| {
      DesktopCaptureSource::list_windows().ok()?;
      cached_window(id).map(|metadata| metadata.process_id)
    })
    .filter(|process_id| *process_id != 0)
    .ok_or_else(|| "window process is no longer available".to_owned())
}

impl DesktopCaptureSource {
  pub fn list_screens() -> Result<Vec<Self>, String> {
    list_platform_screens()
  }

  pub fn list_windows() -> Result<Vec<Self>, String> {
    list_platform_windows()
  }

  pub fn find(kind: DesktopCaptureSourceKind, id: u32) -> Result<Self, String> {
    match kind {
      DesktopCaptureSourceKind::Screen => Self::list_screens()?
        .into_iter()
        .find(|source| source.id == id)
        .ok_or_else(|| "screen source no longer exists".to_owned()),
      DesktopCaptureSourceKind::Window => Self::list_windows()?
        .into_iter()
        .find(|source| source.id == id)
        .ok_or_else(|| "window source no longer exists".to_owned()),
    }
  }

  pub fn capture_frame(&self) -> Result<DesktopFrame, String> {
    #[cfg(target_os = "windows")]
    {
      if let Ok(frame) = capture_wgc_snapshot_frame(self.kind, self.id) {
        return Ok(frame);
      }
      if matches!(self.kind, DesktopCaptureSourceKind::Window) {
        return capture_raw_window_frame(self.id, self.width, self.height);
      }
      return Err("WGC screen snapshot failed".to_owned());
    }

    #[cfg(target_os = "macos")]
    {
      let _ = self;
      Err("CPU desktop capture fallback is unavailable; use native ScreenCaptureKit streaming".to_owned())
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
      let _ = self;
      Err("desktop capture is not supported on this platform".to_owned())
    }
  }

  pub fn capture_snapshot_frame(&self) -> Result<DesktopFrame, String> {
    self.capture_frame()
  }
}

#[cfg(target_os = "windows")]
fn list_platform_screens() -> Result<Vec<DesktopCaptureSource>, String> {
  let mut state = MonitorEnumState {
    screens: Vec::new(),
    friendly_names: monitor_friendly_names_by_gdi_device(),
  };
  let ok = unsafe {
    EnumDisplayMonitors(
      None,
      None,
      Some(enum_monitor_source),
      LPARAM((&mut state as *mut MonitorEnumState) as isize),
    )
  };
  if !ok.as_bool() {
    return Err("EnumDisplayMonitors failed".to_owned());
  }
  Ok(state.screens)
}

#[cfg(target_os = "windows")]
unsafe extern "system" fn enum_monitor_source(monitor: HMONITOR, _dc: HDC, _rect: *mut RECT, state: LPARAM) -> BOOL {
  let state = unsafe { &mut *(state.0 as *mut MonitorEnumState) };
  let mut info = MONITORINFOEXW::default();
  info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
  let ok = unsafe { GetMonitorInfoW(monitor, (&mut info as *mut MONITORINFOEXW).cast::<MONITORINFO>()).as_bool() };
  if ok {
    let rect = info.monitorInfo.rcMonitor;
    let width = rect.right.saturating_sub(rect.left).max(0) as u32;
    let height = rect.bottom.saturating_sub(rect.top).max(0) as u32;
    let device_name = wide_to_string(&info.szDevice);
    let friendly_name = state
      .friendly_names
      .get(&normalize_display_device_name(&device_name))
      .cloned()
      .filter(|name| !name.trim().is_empty());
    let mut details = Vec::new();
    if info.monitorInfo.dwFlags & 1 != 0 {
      details.push("Primary".to_owned());
    }
    state.screens.push(DesktopCaptureSource {
      kind: DesktopCaptureSourceKind::Screen,
      id: monitor.0 as usize as u32,
      name: friendly_name
        .or_else(|| (!device_name.trim().is_empty()).then_some(device_name))
        .unwrap_or_else(|| format!("Display {}", state.screens.len() + 1)),
      description: details.join(" · "),
      x: rect.left,
      y: rect.top,
      width,
      height,
    });
  }
  BOOL(1)
}

#[cfg(target_os = "windows")]
fn monitor_friendly_names_by_gdi_device() -> HashMap<String, String> {
  let mut path_count = 0u32;
  let mut mode_count = 0u32;
  let status = unsafe { GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut path_count, &mut mode_count) };
  if status != ERROR_SUCCESS || path_count == 0 {
    return HashMap::new();
  }

  let mut paths = vec![DISPLAYCONFIG_PATH_INFO::default(); path_count as usize];
  let mut modes = vec![DISPLAYCONFIG_MODE_INFO::default(); mode_count as usize];
  let status = unsafe {
    QueryDisplayConfig(
      QDC_ONLY_ACTIVE_PATHS,
      &mut path_count,
      paths.as_mut_ptr(),
      &mut mode_count,
      modes.as_mut_ptr(),
      None,
    )
  };
  if status != ERROR_SUCCESS {
    return HashMap::new();
  }
  paths.truncate(path_count as usize);

  let mut names = HashMap::new();
  for path in paths {
    let Some(gdi_name) = display_config_source_name(path.sourceInfo.adapterId, path.sourceInfo.id) else {
      continue;
    };
    let Some(friendly_name) = display_config_target_name(path.targetInfo.adapterId, path.targetInfo.id) else {
      continue;
    };
    let friendly_name = friendly_name.trim();
    if friendly_name.is_empty() {
      continue;
    }
    names.insert(normalize_display_device_name(&gdi_name), friendly_name.to_owned());
  }
  names
}

#[cfg(target_os = "windows")]
fn display_config_source_name(adapter_id: LUID, source_id: u32) -> Option<String> {
  let mut source = DISPLAYCONFIG_SOURCE_DEVICE_NAME::default();
  source.header = display_config_header(
    DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME,
    std::mem::size_of::<DISPLAYCONFIG_SOURCE_DEVICE_NAME>(),
    adapter_id,
    source_id,
  );
  let status = unsafe { DisplayConfigGetDeviceInfo((&mut source as *mut DISPLAYCONFIG_SOURCE_DEVICE_NAME).cast()) };
  (status == ERROR_SUCCESS.0 as i32).then(|| wide_to_string(&source.viewGdiDeviceName))
}

#[cfg(target_os = "windows")]
fn display_config_target_name(adapter_id: LUID, target_id: u32) -> Option<String> {
  let mut target = DISPLAYCONFIG_TARGET_DEVICE_NAME::default();
  target.header = display_config_header(
    DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME,
    std::mem::size_of::<DISPLAYCONFIG_TARGET_DEVICE_NAME>(),
    adapter_id,
    target_id,
  );
  let status = unsafe { DisplayConfigGetDeviceInfo((&mut target as *mut DISPLAYCONFIG_TARGET_DEVICE_NAME).cast()) };
  (status == ERROR_SUCCESS.0 as i32).then(|| wide_to_string(&target.monitorFriendlyDeviceName))
}

#[cfg(target_os = "windows")]
fn display_config_header(
  r#type: windows::Win32::Devices::Display::DISPLAYCONFIG_DEVICE_INFO_TYPE,
  size: usize,
  adapter_id: LUID,
  id: u32,
) -> DISPLAYCONFIG_DEVICE_INFO_HEADER {
  DISPLAYCONFIG_DEVICE_INFO_HEADER {
    r#type,
    size: size as u32,
    adapterId: adapter_id,
    id,
  }
}

#[cfg(target_os = "windows")]
fn normalize_display_device_name(name: &str) -> String {
  name.trim().to_ascii_uppercase()
}

#[cfg(target_os = "windows")]
fn list_platform_windows() -> Result<Vec<DesktopCaptureSource>, String> {
  let mut windows = Vec::new();
  unsafe {
    EnumWindows(
      Some(enum_window_source),
      LPARAM((&mut windows as *mut Vec<DesktopCaptureSource>) as isize),
    )
    .map_err(|error| format!("EnumWindows failed: {error}"))?;
  }
  Ok(windows)
}

#[cfg(target_os = "windows")]
unsafe extern "system" fn enum_window_source(hwnd: HWND, state: LPARAM) -> BOOL {
  if !unsafe { IsWindowVisible(hwnd).as_bool() } || unsafe { IsIconic(hwnd).as_bool() } {
    return BOOL(1);
  }

  let mut rect = RECT::default();
  if unsafe { GetWindowRect(hwnd, &mut rect).is_err() } {
    return BOOL(1);
  }
  let width = rect.right.saturating_sub(rect.left);
  let height = rect.bottom.saturating_sub(rect.top);
  if width <= 0 || height <= 0 {
    return BOOL(1);
  }

  let id = hwnd.0 as usize as u32;
  let title = window_title(hwnd);
  let mut process_id = 0u32;
  unsafe {
    GetWindowThreadProcessId(hwnd, Some(&mut process_id));
  }
  let app_name = process_id
    .checked_sub(0)
    .and_then(process_image_name)
    .map(|name| display_window_app_name(&name))
    .unwrap_or_default();
  let Some(name) = source_window_name(&app_name, &title) else {
    return BOOL(1);
  };

  remember_window(id, WindowMetadata { process_id });
  let windows = unsafe { &mut *(state.0 as *mut Vec<DesktopCaptureSource>) };
  windows.push(DesktopCaptureSource {
    kind: DesktopCaptureSourceKind::Window,
    id,
    name,
    description: app_name,
    x: rect.left,
    y: rect.top,
    width: width as u32,
    height: height as u32,
  });
  BOOL(1)
}

#[cfg(target_os = "macos")]
fn list_platform_screens() -> Result<Vec<DesktopCaptureSource>, String> {
  list_macos_desktop_sources(DesktopCaptureSourceKind::Screen)
}

#[cfg(target_os = "macos")]
fn list_platform_windows() -> Result<Vec<DesktopCaptureSource>, String> {
  list_macos_desktop_sources(DesktopCaptureSourceKind::Window)
}

#[cfg(target_os = "macos")]
fn list_macos_desktop_sources(kind: DesktopCaptureSourceKind) -> Result<Vec<DesktopCaptureSource>, String> {
  let source_kind = desktop_source_kind_id(kind);
  let json_ptr = unsafe { parties_macos_desktop_sources_json(source_kind) };
  if json_ptr.is_null() {
    return Err("failed to list macOS desktop sources".to_owned());
  }
  let json = unsafe { CStr::from_ptr(json_ptr) }.to_string_lossy().into_owned();
  unsafe {
    parties_macos_string_free(json_ptr);
  }
  parse_macos_desktop_sources(kind, &json)
}

#[cfg(target_os = "macos")]
fn parse_macos_desktop_sources(
  kind: DesktopCaptureSourceKind,
  json: &str,
) -> Result<Vec<DesktopCaptureSource>, String> {
  let value: serde_json::Value = serde_json::from_str(json).map_err(|error| error.to_string())?;
  let array = value
    .as_array()
    .ok_or_else(|| "macOS source list was not an array".to_owned())?;
  let mut sources = Vec::new();
  for entry in array {
    let id = entry.get("id").and_then(serde_json::Value::as_u64).unwrap_or(0) as u32;
    let width = entry.get("width").and_then(serde_json::Value::as_u64).unwrap_or(0) as u32;
    let height = entry.get("height").and_then(serde_json::Value::as_u64).unwrap_or(0) as u32;
    let x = entry.get("x").and_then(serde_json::Value::as_i64).unwrap_or(0) as i32;
    let y = entry.get("y").and_then(serde_json::Value::as_i64).unwrap_or(0) as i32;
    let name = entry
      .get("name")
      .and_then(serde_json::Value::as_str)
      .unwrap_or_default()
      .trim()
      .to_owned();
    let description = entry
      .get("description")
      .and_then(serde_json::Value::as_str)
      .unwrap_or_default()
      .trim()
      .to_owned();
    if id == 0 || width == 0 || height == 0 || name.is_empty() {
      continue;
    }
    sources.push(DesktopCaptureSource {
      kind,
      id,
      name,
      description,
      x,
      y,
      width,
      height,
    });
  }
  Ok(sources)
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn list_platform_screens() -> Result<Vec<DesktopCaptureSource>, String> {
  Err("screen listing is not supported on this platform".to_owned())
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn list_platform_windows() -> Result<Vec<DesktopCaptureSource>, String> {
  Err("window listing is not supported on this platform".to_owned())
}

#[cfg(target_os = "windows")]
fn capture_wgc_snapshot_frame(kind: DesktopCaptureSourceKind, id: u32) -> Result<DesktopFrame, String> {
  let source_kind = desktop_source_kind_id(kind);
  let mut width = 0u32;
  let mut height = 0u32;
  let mut len = 0usize;
  let bytes =
    unsafe { parties_wgc_snapshot_capture(source_kind, id as usize, 1000, &mut width, &mut height, &mut len) };
  if bytes.is_null() {
    return Err("WGC snapshot returned null".to_owned());
  }

  let expected_len = (width as usize)
    .checked_mul(height as usize)
    .and_then(|pixels| pixels.checked_mul(4))
    .ok_or_else(|| "WGC snapshot dimensions are too large".to_owned())?;
  if width == 0 || height == 0 || len != expected_len {
    unsafe {
      parties_wgc_snapshot_free(bytes);
    }
    return Err(format!(
      "WGC snapshot returned invalid frame: {}x{} len={} expected={}",
      width, height, len, expected_len
    ));
  }

  let rgba = unsafe { std::slice::from_raw_parts(bytes, len).to_vec() };
  unsafe {
    parties_wgc_snapshot_free(bytes);
  }
  Ok(DesktopFrame { rgba, width, height })
}

#[cfg(target_os = "windows")]
fn capture_raw_window_frame(id: u32, width: u32, height: u32) -> Result<DesktopFrame, String> {
  let width = i32::try_from(width).map_err(|_| "window width is too large".to_owned())?;
  let height = i32::try_from(height).map_err(|_| "window height is too large".to_owned())?;
  if width <= 0 || height <= 0 {
    return Err("window has invalid capture dimensions".to_owned());
  }

  let hwnd = HWND(id as usize as *mut c_void);
  let window_dc = unsafe { windows::Win32::Graphics::Gdi::GetWindowDC(Some(hwnd)) };
  if window_dc.is_invalid() {
    return Err("GetWindowDC failed".to_owned());
  }

  let result = unsafe { capture_window_with_dc(hwnd, window_dc, width, height) };
  unsafe {
    ReleaseDC(Some(hwnd), window_dc);
  }
  result
}

#[cfg(target_os = "windows")]
unsafe fn capture_window_with_dc(hwnd: HWND, window_dc: HDC, width: i32, height: i32) -> Result<DesktopFrame, String> {
  let memory_dc = unsafe { CreateCompatibleDC(Some(window_dc)) };
  if memory_dc.is_invalid() {
    return Err("CreateCompatibleDC failed".to_owned());
  }

  let result = unsafe { capture_window_with_memory_dc(hwnd, window_dc, memory_dc, width, height) };
  unsafe {
    let _ = DeleteDC(memory_dc);
  }
  result
}

#[cfg(target_os = "windows")]
unsafe fn capture_window_with_memory_dc(
  hwnd: HWND,
  window_dc: HDC,
  memory_dc: HDC,
  width: i32,
  height: i32,
) -> Result<DesktopFrame, String> {
  let mut bitmap_bits: *mut c_void = ptr::null_mut();
  let bitmap_info = BITMAPINFO {
    bmiHeader: BITMAPINFOHEADER {
      biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
      biWidth: width,
      biHeight: -height,
      biPlanes: 1,
      biBitCount: 32,
      biCompression: BI_RGB.0,
      biSizeImage: (width * height * 4) as u32,
      ..BITMAPINFOHEADER::default()
    },
    ..BITMAPINFO::default()
  };

  let bitmap = unsafe { CreateDIBSection(Some(window_dc), &bitmap_info, DIB_RGB_COLORS, &mut bitmap_bits, None, 0) }
    .map_err(|error| format!("CreateDIBSection failed: {error}"))?;
  if bitmap_bits.is_null() {
    unsafe {
      let _ = DeleteObject(bitmap.into());
    }
    return Err("CreateDIBSection returned null bits".to_owned());
  }

  let result = unsafe { capture_window_into_bitmap(hwnd, window_dc, memory_dc, bitmap, bitmap_bits, width, height) };
  unsafe {
    let _ = DeleteObject(bitmap.into());
  }
  result
}

#[cfg(target_os = "windows")]
unsafe fn capture_window_into_bitmap(
  hwnd: HWND,
  window_dc: HDC,
  memory_dc: HDC,
  bitmap: HBITMAP,
  bitmap_bits: *mut c_void,
  width: i32,
  height: i32,
) -> Result<DesktopFrame, String> {
  let previous = unsafe { SelectObject(memory_dc, HGDIOBJ(bitmap.0)) };
  if previous.is_invalid() {
    return Err("SelectObject failed".to_owned());
  }

  let mut captured = unsafe { PrintWindow(hwnd, memory_dc, PRINT_WINDOW_FLAGS(2)).as_bool() };
  if !captured {
    captured = unsafe { PrintWindow(hwnd, memory_dc, PRINT_WINDOW_FLAGS(0)).as_bool() };
  }
  if !captured {
    captured = unsafe { BitBlt(memory_dc, 0, 0, width, height, Some(window_dc), 0, 0, SRCCOPY).is_ok() };
  }

  unsafe {
    SelectObject(memory_dc, previous);
  }

  if !captured {
    return Err("PrintWindow and BitBlt failed".to_owned());
  }

  let byte_len = (width as usize)
    .checked_mul(height as usize)
    .and_then(|pixels| pixels.checked_mul(4))
    .ok_or_else(|| "window capture buffer is too large".to_owned())?;
  let bgra = unsafe { std::slice::from_raw_parts(bitmap_bits.cast::<u8>(), byte_len) };
  let mut rgba = Vec::with_capacity(byte_len);
  for pixel in bgra.chunks_exact(4) {
    rgba.push(pixel[2]);
    rgba.push(pixel[1]);
    rgba.push(pixel[0]);
    rgba.push(pixel[3]);
  }

  Ok(DesktopFrame {
    rgba,
    width: width as u32,
    height: height as u32,
  })
}

#[cfg(target_os = "windows")]
fn window_title(hwnd: HWND) -> String {
  let text_length = unsafe { GetWindowTextLengthW(hwnd) };
  if text_length <= 0 {
    return String::new();
  }
  let mut buffer = vec![0u16; text_length as usize + 1];
  let copied = unsafe { GetWindowTextW(hwnd, &mut buffer) };
  if copied <= 0 {
    return String::new();
  }
  wide_to_string(&buffer[..copied as usize])
}

#[cfg(target_os = "windows")]
fn process_image_name(process_id: u32) -> Option<String> {
  let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process_id).ok()? };
  let mut buffer = vec![0u16; 32768];
  let mut len = buffer.len() as u32;
  let result = unsafe { QueryFullProcessImageNameW(handle, PROCESS_NAME_WIN32, PWSTR(buffer.as_mut_ptr()), &mut len) };
  unsafe {
    let _ = CloseHandle(handle);
  }
  result.ok()?;
  Some(wide_to_string(&buffer[..len as usize]))
}

#[cfg(target_os = "windows")]
fn wide_to_string(wide: &[u16]) -> String {
  let len = wide.iter().position(|ch| *ch == 0).unwrap_or(wide.len());
  String::from_utf16_lossy(&wide[..len])
}

fn desktop_source_kind_id(kind: DesktopCaptureSourceKind) -> u8 {
  match kind {
    DesktopCaptureSourceKind::Screen => 0,
    DesktopCaptureSourceKind::Window => 1,
  }
}

fn window_cache() -> &'static Mutex<HashMap<u32, WindowMetadata>> {
  WINDOW_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn remember_window(id: u32, metadata: WindowMetadata) {
  let mut cache = window_cache().lock().expect("desktop window cache lock poisoned");
  if cache.len() >= 128 {
    cache.clear();
  }
  cache.insert(id, metadata);
}

fn cached_window(id: u32) -> Option<WindowMetadata> {
  window_cache()
    .lock()
    .expect("desktop window cache lock poisoned")
    .get(&id)
    .copied()
}

fn source_window_name(app_name: &str, title: &str) -> Option<String> {
  let app_name = app_name.trim();
  let title = title.trim();

  match (app_name.is_empty(), title.is_empty()) {
    (true, true) => None,
    (false, true) => Some(app_name.to_owned()),
    (true, false) => Some(title.to_owned()),
    (false, false) if title == app_name => Some(title.to_owned()),
    (false, false) if should_prefer_window_title(app_name, title) => Some(title.to_owned()),
    (false, false) => Some(format!("{app_name} - {title}")),
  }
}

fn should_prefer_window_title(app_name: &str, title: &str) -> bool {
  let app_key = alphanumeric_lowercase(app_name);
  let title_key = alphanumeric_lowercase(title);
  if app_key.is_empty() || title_key.is_empty() {
    return false;
  }

  app_key.contains(&title_key) || is_windows_host_process(&app_key)
}

fn is_windows_host_process(app_key: &str) -> bool {
  matches!(
    app_key,
    "applicationframehost"
      | "shellexperiencehost"
      | "startmenuexperiencehost"
      | "searchhost"
      | "textinputhost"
      | "runtimebroker"
  ) || app_key.ends_with("helper")
}

fn alphanumeric_lowercase(value: &str) -> String {
  value
    .chars()
    .filter(|ch| ch.is_ascii_alphanumeric())
    .flat_map(char::to_lowercase)
    .collect()
}

fn display_window_app_name(app_name: &str) -> String {
  let app_name = app_name.trim().trim_matches('"');
  let Some(file_name) = app_name.rsplit(['\\', '/']).next() else {
    return String::new();
  };
  let file_name = file_name.trim();
  if file_name.len() > 4 && file_name[file_name.len() - 4..].eq_ignore_ascii_case(".exe") {
    file_name[..file_name.len() - 4].to_owned()
  } else {
    file_name.to_owned()
  }
}

#[cfg(test)]
#[path = "../../tests/unit/services/desktop_capture.rs"]
mod tests;
