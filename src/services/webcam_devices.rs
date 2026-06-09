#[cfg(target_os = "macos")]
use std::ffi::CStr;

#[cfg(target_os = "windows")]
use std::collections::HashSet;

#[cfg(target_os = "windows")]
use nokhwa::{native_api_backend, query, utils::CameraInfo};

#[derive(Clone, Debug, PartialEq, Eq, lurq::DevtoolsInspectable)]
pub struct WebcamDevice {
  pub value: String,
  pub label: String,
}

pub fn webcam_device_id(value: &str) -> u32 {
  value.as_bytes().iter().fold(0x811C_9DC5_u32, |hash, byte| {
    (hash ^ u32::from(*byte)).wrapping_mul(0x0100_0193)
  })
}

pub fn webcam_devices() -> Vec<WebcamDevice> {
  #[cfg(target_os = "macos")]
  {
    return native_macos_webcam_devices();
  }

  #[cfg(target_os = "windows")]
  {
    return nokhwa_webcam_devices();
  }

  #[cfg(not(any(target_os = "macos", target_os = "windows")))]
  {
    Vec::new()
  }
}

#[cfg(target_os = "windows")]
fn nokhwa_webcam_devices() -> Vec<WebcamDevice> {
  initialize_nokhwa();

  let Some(api) = native_api_backend() else {
    return Vec::new();
  };

  let mut devices = query(api)
    .unwrap_or_default()
    .into_iter()
    .enumerate()
    .filter_map(|(ordinal, camera)| webcam_device(camera, ordinal))
    .collect::<Vec<_>>();

  devices.sort_by(|left, right| {
    left
      .label
      .to_lowercase()
      .cmp(&right.label.to_lowercase())
      .then_with(|| left.value.cmp(&right.value))
  });

  let mut seen_values = HashSet::new();
  devices.retain(|device| seen_values.insert(device.value.clone()));
  devices
}

#[cfg(target_os = "windows")]
fn webcam_device(camera: CameraInfo, ordinal: usize) -> Option<WebcamDevice> {
  let label = clean_device_string(&camera.human_name())
    .or_else(|| clean_device_string(camera.description()))
    .unwrap_or_else(|| format!("Camera {}", ordinal + 1));
  let value = webcam_device_value(&camera);

  if value.trim().is_empty() {
    return None;
  }

  Some(WebcamDevice { value, label })
}

#[cfg(target_os = "windows")]
pub(crate) fn webcam_device_value(camera: &CameraInfo) -> String {
  clean_device_string(&camera.misc()).unwrap_or_else(|| camera.index().as_string())
}

#[cfg(target_os = "windows")]
fn clean_device_string(value: &str) -> Option<String> {
  let value = value
    .chars()
    .filter(|character| !character.is_control())
    .collect::<String>()
    .trim()
    .to_owned();

  if value.is_empty() { None } else { Some(value) }
}

#[cfg(target_os = "windows")]
pub(crate) fn initialize_nokhwa() {}

#[cfg(target_os = "macos")]
unsafe extern "C" {
  fn parties_macos_camera_refresh() -> usize;
  fn parties_macos_camera_unique_id(index: usize) -> *const std::ffi::c_char;
  fn parties_macos_camera_name(index: usize) -> *const std::ffi::c_char;
}

#[cfg(target_os = "macos")]
fn native_macos_webcam_devices() -> Vec<WebcamDevice> {
  let count = unsafe { parties_macos_camera_refresh() };
  let mut devices = (0..count)
    .filter_map(|index| {
      let value = unsafe { c_string(parties_macos_camera_unique_id(index))? };
      let label = unsafe { c_string(parties_macos_camera_name(index))? };
      if value.trim().is_empty() {
        return None;
      }
      Some(WebcamDevice {
        value,
        label: if label.trim().is_empty() {
          format!("Camera {}", index + 1)
        } else {
          label
        },
      })
    })
    .collect::<Vec<_>>();
  devices.sort_by(|left, right| {
    left
      .label
      .to_lowercase()
      .cmp(&right.label.to_lowercase())
      .then_with(|| left.value.cmp(&right.value))
  });

  let mut seen_values = std::collections::HashSet::new();
  devices.retain(|device| seen_values.insert(device.value.clone()));
  devices
}

#[cfg(target_os = "macos")]
unsafe fn c_string(ptr: *const std::ffi::c_char) -> Option<String> {
  if ptr.is_null() {
    return None;
  }
  unsafe { CStr::from_ptr(ptr) }.to_str().ok().map(str::to_owned)
}
