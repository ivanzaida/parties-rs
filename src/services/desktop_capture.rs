use std::{
  collections::HashMap,
  sync::{Mutex, OnceLock},
};
#[cfg(target_os = "windows")]
use std::{ffi::c_void, ptr};

#[cfg(target_os = "windows")]
use windows::Win32::{
  Foundation::HWND,
  Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS, DeleteDC,
    DeleteObject, HBITMAP, HDC, HGDIOBJ, ReleaseDC, SRCCOPY, SelectObject,
  },
  Storage::Xps::{PRINT_WINDOW_FLAGS, PrintWindow},
};
use xcap::{Monitor, Window};

static WINDOW_CACHE: OnceLock<Mutex<HashMap<u32, Window>>> = OnceLock::new();

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
  pub width: u32,
  pub height: u32,
  inner: DesktopCaptureSourceInner,
}

#[derive(Clone)]
enum DesktopCaptureSourceInner {
  Screen(Monitor),
  Window(Window),
}

pub struct DesktopFrame {
  pub rgba: Vec<u8>,
  pub width: u32,
  pub height: u32,
}

pub fn find_window_process_id(id: u32) -> Result<u32, String> {
  cached_window(id)
    .or_else(|| {
      Window::all()
        .ok()?
        .into_iter()
        .find(|window| window.id().ok() == Some(id))
    })
    .and_then(|window| window.pid().ok())
    .filter(|process_id| *process_id != 0)
    .ok_or_else(|| "window process is no longer available".to_owned())
}

impl DesktopCaptureSource {
  pub fn list_screens() -> Result<Vec<Self>, String> {
    Monitor::all().map_err(|error| error.to_string()).map(|monitors| {
      monitors
        .into_iter()
        .enumerate()
        .map(|(index, monitor)| screen_source(index, monitor))
        .collect()
    })
  }

  pub fn list_windows() -> Result<Vec<Self>, String> {
    Window::all()
      .map_err(|error| error.to_string())
      .map(|windows| windows.into_iter().filter_map(window_source).collect())
  }

  pub fn find(kind: DesktopCaptureSourceKind, id: u32) -> Result<Self, String> {
    match kind {
      DesktopCaptureSourceKind::Screen => Self::list_screens()?
        .into_iter()
        .find(|source| source.id == id)
        .ok_or_else(|| "screen source no longer exists".to_owned()),
      DesktopCaptureSourceKind::Window => cached_window(id)
        .and_then(window_source)
        .or_else(|| Self::list_windows().ok()?.into_iter().find(|source| source.id == id))
        .ok_or_else(|| "window source no longer exists".to_owned()),
    }
  }

  pub fn capture_frame(&self) -> Result<DesktopFrame, String> {
    #[cfg(target_os = "windows")]
    if matches!(&self.inner, DesktopCaptureSourceInner::Window(_)) {
      if let Ok(frame) = capture_raw_window_frame(self.id, self.width, self.height) {
        return Ok(frame);
      }
    }

    let image = match &self.inner {
      DesktopCaptureSourceInner::Screen(monitor) => monitor.capture_image().map_err(|error| error.to_string())?,
      DesktopCaptureSourceInner::Window(window) => window.capture_image().map_err(|error| error.to_string())?,
    };
    Ok(DesktopFrame {
      width: image.width(),
      height: image.height(),
      rgba: image.into_raw(),
    })
  }
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

fn screen_source(index: usize, monitor: Monitor) -> DesktopCaptureSource {
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
  let primary = monitor.is_primary().unwrap_or(false);
  let builtin = monitor.is_builtin().unwrap_or(false);
  let mut details = Vec::new();

  if primary {
    details.push("Primary".to_owned());
  }

  if builtin {
    details.push("Built-in".to_owned());
  }

  DesktopCaptureSource {
    kind: DesktopCaptureSourceKind::Screen,
    id,
    name,
    description: details.join(" · "),
    width,
    height,
    inner: DesktopCaptureSourceInner::Screen(monitor),
  }
}

fn window_source(window: Window) -> Option<DesktopCaptureSource> {
  if window.is_minimized().unwrap_or(false) {
    return None;
  }

  let id = window.id().ok()?;
  remember_window(id, window.clone());
  let app_name = window.app_name().unwrap_or_default();
  let title = window.title().unwrap_or_default();
  let name = source_window_name(&app_name, &title)?;
  let width = window.width().unwrap_or(0);
  let height = window.height().unwrap_or(0);

  Some(DesktopCaptureSource {
    kind: DesktopCaptureSourceKind::Window,
    id,
    name,
    description: app_name,
    width,
    height,
    inner: DesktopCaptureSourceInner::Window(window),
  })
}

fn window_cache() -> &'static Mutex<HashMap<u32, Window>> {
  WINDOW_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn remember_window(id: u32, window: Window) {
  let mut cache = window_cache().lock().expect("desktop window cache lock poisoned");
  if cache.len() >= 128 {
    cache.clear();
  }
  cache.insert(id, window);
}

fn cached_window(id: u32) -> Option<Window> {
  window_cache()
    .lock()
    .expect("desktop window cache lock poisoned")
    .get(&id)
    .cloned()
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
