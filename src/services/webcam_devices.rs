use std::collections::HashSet;

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

pub(crate) fn webcam_device_value(camera: &CameraInfo) -> String {
  clean_device_string(&camera.misc()).unwrap_or_else(|| camera.index().as_string())
}

fn clean_device_string(value: &str) -> Option<String> {
  let value = value
    .chars()
    .filter(|character| !character.is_control())
    .collect::<String>()
    .trim()
    .to_owned();

  if value.is_empty() { None } else { Some(value) }
}

pub(crate) fn initialize_nokhwa() {
  #[cfg(target_os = "macos")]
  nokhwa::nokhwa_initialize(|_| {});
}
