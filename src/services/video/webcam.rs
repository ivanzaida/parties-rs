use nokhwa::{
  Camera, native_api_backend,
  pixel_format::RgbAFormat,
  query,
  utils::{CameraIndex, RequestedFormat, RequestedFormatType},
};

use super::VideoError;
use crate::services::{
  logger,
  webcam_devices::{initialize_nokhwa, webcam_device_id, webcam_device_value},
};

pub(super) struct WebcamCapture {
  camera: Camera,
}

impl WebcamCapture {
  pub(super) fn open(source_id: u32) -> Result<Self, VideoError> {
    initialize_nokhwa();
    let camera_index = find_camera_index(source_id)?;
    let request = RequestedFormat::new::<RgbAFormat>(RequestedFormatType::None);
    let mut camera =
      Camera::new(camera_index, request).map_err(|error| VideoError::new(format!("Failed to open webcam: {error}")))?;
    camera
      .open_stream()
      .map_err(|error| VideoError::new(format!("Failed to start webcam stream: {error}")))?;
    let resolution = camera.resolution();
    logger::log(&format!(
      "[video] webcam capture ready: source={} size={}x{} fps={}",
      source_id,
      resolution.width(),
      resolution.height(),
      camera.frame_rate()
    ));
    Ok(Self { camera })
  }

  pub(super) fn capture_rgba(&mut self, width: u16, height: u16) -> Result<Vec<u8>, VideoError> {
    let frame = self
      .camera
      .frame()
      .map_err(|error| VideoError::new(format!("Failed to capture webcam frame: {error}")))?;
    let resolution = frame.resolution();
    let image = frame
      .decode_image::<RgbAFormat>()
      .map_err(|error| VideoError::new(format!("Failed to decode webcam frame: {error}")))?;
    normalize_rgba_frame(image.into_raw(), resolution.width(), resolution.height(), width, height)
  }
}

impl Drop for WebcamCapture {
  fn drop(&mut self) {
    let _ = self.camera.stop_stream();
  }
}

fn find_camera_index(source_id: u32) -> Result<CameraIndex, VideoError> {
  let Some(api) = native_api_backend() else {
    return Err(VideoError::new("No native webcam backend is available."));
  };

  query(api)
    .map_err(|error| VideoError::new(format!("Failed to list webcams: {error}")))?
    .into_iter()
    .find(|camera| webcam_device_id(&webcam_device_value(camera)) == source_id)
    .map(|camera| camera.index().clone())
    .ok_or_else(|| VideoError::new("Selected webcam is no longer available."))
}

fn normalize_rgba_frame(
  rgba: Vec<u8>,
  frame_width: u32,
  frame_height: u32,
  output_width: u16,
  output_height: u16,
) -> Result<Vec<u8>, VideoError> {
  let output_width = u32::from(output_width);
  let output_height = u32::from(output_height);
  if frame_width == output_width && frame_height == output_height {
    return Ok(rgba);
  }

  if frame_width == 0 || frame_height == 0 || output_width == 0 || output_height == 0 {
    return Err(VideoError::new(format!(
      "Invalid webcam frame dimensions: captured={}x{} output={}x{}.",
      frame_width, frame_height, output_width, output_height
    )));
  }

  let src_stride = frame_width as usize * 4;
  let dst_stride = output_width as usize * 4;
  let mut out = vec![0u8; dst_stride * output_height as usize];
  for row in 0..output_height as usize {
    let src_y = row * frame_height as usize / output_height as usize;
    let dst_start = row * dst_stride;
    for column in 0..output_width as usize {
      let src_x = column * frame_width as usize / output_width as usize;
      let src_start = src_y * src_stride + src_x * 4;
      let dst_start = dst_start + column * 4;
      out[dst_start..dst_start + 4].copy_from_slice(&rgba[src_start..src_start + 4]);
    }
  }
  Ok(out)
}
