#[cfg(target_os = "macos")]
use std::ffi::CStr;

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

pub fn webcam_devices_with_fallbacks(
  default_label: &str,
  indexed_label: &dyn Fn(usize) -> String,
) -> Vec<WebcamDevice> {
  #[cfg(not(target_os = "windows"))]
  let _ = default_label;
  #[cfg(not(target_os = "macos"))]
  let _ = indexed_label;

  #[cfg(target_os = "macos")]
  {
    return native_macos_webcam_devices(indexed_label);
  }

  #[cfg(target_os = "windows")]
  {
    return windows_webcam::webcam_devices(default_label).unwrap_or_default();
  }

  #[cfg(not(any(target_os = "macos", target_os = "windows")))]
  {
    Vec::new()
  }
}

#[cfg(target_os = "windows")]
pub(crate) mod windows_webcam {
  use std::{ffi::c_void, ptr};

  use windows::{
    Win32::{
      Media::MediaFoundation::{
        IMFActivate, MF_DEVSOURCE_ATTRIBUTE_FRIENDLY_NAME, MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE,
        MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_GUID, MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_SYMBOLIC_LINK,
        MF_VERSION, MFCreateAttributes, MFEnumDeviceSources, MFSTARTUP_NOSOCKET, MFShutdown, MFStartup,
      },
      System::Com::CoTaskMemFree,
    },
    core::PWSTR,
  };

  use super::{WebcamDevice, webcam_device_id};

  pub(crate) struct MediaFoundationSession;

  impl MediaFoundationSession {
    pub(crate) fn start() -> Result<Self, String> {
      unsafe {
        MFStartup(MF_VERSION, MFSTARTUP_NOSOCKET)
          .map_err(|error| format!("Failed to start Media Foundation: {error}"))?;
      }
      Ok(Self)
    }
  }

  impl Drop for MediaFoundationSession {
    fn drop(&mut self) {
      unsafe {
        let _ = MFShutdown();
      }
    }
  }

  pub(crate) fn webcam_devices(default_label: &str) -> Result<Vec<WebcamDevice>, String> {
    let _mf = MediaFoundationSession::start()?;
    let activates = enumerate_video_activates()?;
    Ok(
      activates
        .iter()
        .filter_map(|activate| {
          let value = activate_string(activate, &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_SYMBOLIC_LINK).ok()?;
          let label = activate_string(activate, &MF_DEVSOURCE_ATTRIBUTE_FRIENDLY_NAME)
            .ok()
            .filter(|label| !label.trim().is_empty())
            .unwrap_or_else(|| default_label.to_owned());
          Some(WebcamDevice { value, label })
        })
        .collect(),
    )
  }

  pub(crate) fn find_activate_by_id(source_id: u32) -> Result<(MediaFoundationSession, IMFActivate, String), String> {
    let mf = MediaFoundationSession::start()?;
    let activates = enumerate_video_activates()?;
    for activate in activates {
      let value = activate_string(&activate, &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_SYMBOLIC_LINK)?;
      if webcam_device_id(&value) == source_id {
        return Ok((mf, activate, value));
      }
    }
    Err("Selected webcam is no longer available.".to_owned())
  }

  pub(crate) fn activate_string(activate: &IMFActivate, key: &windows::core::GUID) -> Result<String, String> {
    let mut value = PWSTR::null();
    let mut len = 0u32;
    unsafe {
      activate
        .GetAllocatedString(key, &mut value, &mut len)
        .map_err(|error| format!("Failed to read webcam attribute: {error}"))?;
    }
    if value.is_null() {
      return Ok(String::new());
    }

    let string = unsafe { String::from_utf16_lossy(std::slice::from_raw_parts(value.as_ptr(), len as usize)) };
    unsafe {
      CoTaskMemFree(Some(value.as_ptr().cast::<c_void>()));
    }
    Ok(string)
  }

  fn enumerate_video_activates() -> Result<Vec<IMFActivate>, String> {
    let attributes = unsafe {
      let mut attributes = None;
      MFCreateAttributes(&mut attributes, 1)
        .map_err(|error| format!("Failed to create webcam source attributes: {error}"))?;
      attributes.ok_or_else(|| "Media Foundation returned null webcam source attributes".to_owned())?
    };
    unsafe {
      attributes
        .SetGUID(
          &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE,
          &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_GUID,
        )
        .map_err(|error| format!("Failed to configure webcam source attributes: {error}"))?;
    }

    let mut activates: *mut Option<IMFActivate> = ptr::null_mut();
    let mut count = 0u32;
    unsafe {
      MFEnumDeviceSources(&attributes, &mut activates, &mut count)
        .map_err(|error| format!("Failed to enumerate webcams: {error}"))?;
    }
    if activates.is_null() || count == 0 {
      return Ok(Vec::new());
    }

    let mut out = Vec::with_capacity(count as usize);
    unsafe {
      for index in 0..count as usize {
        if let Some(activate) = ptr::read(activates.add(index)) {
          out.push(activate);
        }
      }
      CoTaskMemFree(Some(activates.cast::<c_void>()));
    }
    Ok(out)
  }
}

#[cfg(target_os = "macos")]
unsafe extern "C" {
  fn parties_macos_camera_refresh() -> usize;
  fn parties_macos_camera_unique_id(index: usize) -> *const std::ffi::c_char;
  fn parties_macos_camera_name(index: usize) -> *const std::ffi::c_char;
}

#[cfg(target_os = "macos")]
fn native_macos_webcam_devices(indexed_label: &dyn Fn(usize) -> String) -> Vec<WebcamDevice> {
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
          indexed_label(index)
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
