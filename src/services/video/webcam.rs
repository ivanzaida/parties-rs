#[cfg(target_os = "windows")]
mod windows_impl {
  use std::ptr;

  use ::windows::{
    Win32::Media::MediaFoundation::{
      IMFMediaSource, IMFMediaType, IMFSourceReader, MF_E_NO_MORE_TYPES, MF_MT_FRAME_RATE, MF_MT_FRAME_SIZE,
      MF_MT_SUBTYPE, MF_SOURCE_READER_FIRST_VIDEO_STREAM, MF_SOURCE_READERF_ENDOFSTREAM, MF_SOURCE_READERF_ERROR,
      MF_SOURCE_READERF_STREAMTICK, MFCreateSourceReaderFromMediaSource, MFVideoFormat_NV12, MFVideoFormat_RGB32,
      MFVideoFormat_YUY2,
    },
    core::GUID,
  };

  use super::VideoError;
  use crate::services::{
    logger,
    webcam_devices::windows_webcam::{MediaFoundationSession, find_activate_by_id},
  };

  pub(crate) struct WebcamCapture {
    _mf: MediaFoundationSession,
    _source: IMFMediaSource,
    reader: IMFSourceReader,
    frame_format: WebcamFrameFormat,
    frame_width: u32,
    frame_height: u32,
    frame_fps: u32,
  }

  #[derive(Clone, Copy, Debug, Eq, PartialEq)]
  enum WebcamFrameFormat {
    Nv12,
    Yuy2,
    Bgra,
  }

  struct WebcamMediaCandidate {
    media_type: IMFMediaType,
    format: WebcamFrameFormat,
    width: u32,
    height: u32,
    fps: u32,
    score: u64,
  }

  impl WebcamCapture {
    pub(crate) fn open(source_id: u32, width: u16, height: u16, fps: u32) -> Result<Self, VideoError> {
      let (_mf, activate, value) = find_activate_by_id(source_id).map_err(VideoError::new)?;
      let source = unsafe {
        activate
          .ActivateObject::<IMFMediaSource>()
          .map_err(|error| VideoError::new(format!("Failed to activate webcam media source: {error}")))?
      };
      let reader = unsafe {
        MFCreateSourceReaderFromMediaSource(&source, None)
          .map_err(|error| VideoError::new(format!("Failed to create webcam source reader: {error}")))?
      };

      let selected = configure_reader_output(&reader, u32::from(width), u32::from(height), fps)?;
      logger::log(&format!(
        "[video] webcam capture ready: source={} format={} size={}x{} fps={} requested={}x{}@{} backend=MediaFoundation",
        source_id,
        selected.format.label(),
        selected.width,
        selected.height,
        selected.fps,
        width,
        height,
        fps
      ));
      logger::log(&format!("[video] webcam source link selected: {value}"));

      Ok(Self {
        _mf,
        _source: source,
        reader,
        frame_format: selected.format,
        frame_width: selected.width,
        frame_height: selected.height,
        frame_fps: selected.fps,
      })
    }

    pub(crate) fn fps(&self) -> u32 {
      self.frame_fps
    }

    pub(crate) fn capture_rgba(&mut self, width: u16, height: u16) -> Result<Vec<u8>, VideoError> {
      let sample = loop {
        let mut flags = 0u32;
        let mut sample = None;
        unsafe {
          self
            .reader
            .ReadSample(
              MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32,
              0,
              None,
              Some(&mut flags),
              None,
              Some(&mut sample),
            )
            .map_err(|error| VideoError::new(format!("Failed to read webcam frame: {error}")))?;
        }

        if flags & MF_SOURCE_READERF_ERROR.0 as u32 != 0 {
          return Err(VideoError::new("Webcam source reader reported an error."));
        }
        if flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 {
          return Err(VideoError::new("Webcam source reader reached end of stream."));
        }
        if flags & MF_SOURCE_READERF_STREAMTICK.0 as u32 != 0 {
          continue;
        }
        if let Some(sample) = sample {
          break sample;
        }
      };

      let buffer = unsafe {
        sample
          .ConvertToContiguousBuffer()
          .map_err(|error| VideoError::new(format!("Failed to read webcam sample buffer: {error}")))?
      };
      let len = unsafe {
        buffer
          .GetCurrentLength()
          .map_err(|error| VideoError::new(format!("Failed to query webcam sample length: {error}")))?
      };
      let expected_len = self.frame_format.buffer_len(self.frame_width, self.frame_height)?;
      if (len as usize) < expected_len {
        return Err(VideoError::new(format!(
          "Webcam sample is too small: got={} expected_at_least={expected_len}.",
          len
        )));
      }

      let mut data = ptr::null_mut();
      unsafe {
        buffer
          .Lock(&mut data, None, None)
          .map_err(|error| VideoError::new(format!("Failed to lock webcam sample buffer: {error}")))?;
      }
      let bytes = unsafe { std::slice::from_raw_parts(data.cast::<u8>(), expected_len) };
      let result = match self.frame_format {
        WebcamFrameFormat::Nv12 => nv12_to_resized_rgba(
          bytes,
          self.frame_width,
          self.frame_height,
          u32::from(width),
          u32::from(height),
        ),
        WebcamFrameFormat::Yuy2 => yuy2_to_resized_rgba(
          bytes,
          self.frame_width,
          self.frame_height,
          u32::from(width),
          u32::from(height),
        ),
        WebcamFrameFormat::Bgra => bgra_to_resized_rgba(
          bytes,
          self.frame_width,
          self.frame_height,
          u32::from(width),
          u32::from(height),
        ),
      };
      unsafe {
        buffer
          .Unlock()
          .map_err(|error| VideoError::new(format!("Failed to unlock webcam sample buffer: {error}")))?;
      }
      result
    }
  }

  impl Drop for WebcamCapture {
    fn drop(&mut self) {
      unsafe {
        let _ = self._source.Shutdown();
      }
    }
  }

  impl WebcamFrameFormat {
    fn label(self) -> &'static str {
      match self {
        Self::Nv12 => "NV12",
        Self::Yuy2 => "YUY2",
        Self::Bgra => "RGB32",
      }
    }

    fn rank(self) -> u64 {
      match self {
        Self::Nv12 => 0,
        Self::Yuy2 => 1,
        Self::Bgra => 2,
      }
    }

    fn buffer_len(self, width: u32, height: u32) -> Result<usize, VideoError> {
      let width = width as usize;
      let height = height as usize;
      match self {
        Self::Nv12 => {
          if width == 0 || height == 0 || width % 2 != 0 || height % 2 != 0 {
            return Err(VideoError::new(format!(
              "Invalid NV12 webcam frame dimensions: {width}x{height}."
            )));
          }
          Ok(width * height + width * height / 2)
        }
        Self::Yuy2 => {
          if width == 0 || height == 0 || width % 2 != 0 {
            return Err(VideoError::new(format!(
              "Invalid YUY2 webcam frame dimensions: {width}x{height}."
            )));
          }
          Ok(width * height * 2)
        }
        Self::Bgra => Ok(width * height * 4),
      }
    }
  }

  fn configure_reader_output(
    reader: &IMFSourceReader,
    requested_width: u32,
    requested_height: u32,
    requested_fps: u32,
  ) -> Result<WebcamMediaCandidate, VideoError> {
    let mut best = None;
    let mut index = 0u32;
    loop {
      let media_type = match unsafe { reader.GetNativeMediaType(MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32, index) } {
        Ok(media_type) => media_type,
        Err(error) if error.code() == MF_E_NO_MORE_TYPES => break,
        Err(error) => {
          return Err(VideoError::new(format!(
            "Failed to enumerate webcam media type: {error}"
          )));
        }
      };
      index += 1;

      let Some(format) = media_type_format(&media_type) else {
        continue;
      };
      let Ok((width, height)) = media_type_frame_size(&media_type) else {
        continue;
      };
      if format.buffer_len(width, height).is_err() {
        continue;
      }
      let fps = media_type_frame_rate(&media_type).unwrap_or(0);
      let score = webcam_media_score(
        format,
        width,
        height,
        fps,
        requested_width,
        requested_height,
        requested_fps,
      );
      let candidate = WebcamMediaCandidate {
        media_type,
        format,
        width,
        height,
        fps,
        score,
      };
      if best
        .as_ref()
        .is_none_or(|best: &WebcamMediaCandidate| candidate.score < best.score)
      {
        best = Some(candidate);
      }
    }

    let selected = best.ok_or_else(|| {
      VideoError::new("Selected webcam does not expose a supported raw format. Supported formats: NV12, YUY2, RGB32.")
    })?;
    unsafe {
      reader
        .SetCurrentMediaType(MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32, None, &selected.media_type)
        .map_err(|error| {
          VideoError::new(format!(
            "Failed to configure webcam {} output at {}x{}@{}: {error}",
            selected.format.label(),
            selected.width,
            selected.height,
            selected.fps
          ))
        })?;
    }
    Ok(selected)
  }

  fn media_type_format(media_type: &IMFMediaType) -> Option<WebcamFrameFormat> {
    let subtype = unsafe { media_type.GetGUID(&MF_MT_SUBTYPE).ok()? };
    format_from_subtype(&subtype)
  }

  fn format_from_subtype(subtype: &GUID) -> Option<WebcamFrameFormat> {
    if *subtype == MFVideoFormat_NV12 {
      Some(WebcamFrameFormat::Nv12)
    } else if *subtype == MFVideoFormat_YUY2 {
      Some(WebcamFrameFormat::Yuy2)
    } else if *subtype == MFVideoFormat_RGB32 {
      Some(WebcamFrameFormat::Bgra)
    } else {
      None
    }
  }

  fn media_type_frame_size(media_type: &IMFMediaType) -> Result<(u32, u32), VideoError> {
    let size = unsafe { media_type.GetUINT64(&MF_MT_FRAME_SIZE) }
      .map_err(|error| VideoError::new(format!("Failed to query webcam frame size: {error}")))?;
    Ok(unpack_u32_pair(size))
  }

  fn media_type_frame_rate(media_type: &IMFMediaType) -> Result<u32, VideoError> {
    let rate = unsafe { media_type.GetUINT64(&MF_MT_FRAME_RATE) }
      .map_err(|error| VideoError::new(format!("Failed to query webcam frame rate: {error}")))?;
    let (numerator, denominator) = unpack_u32_pair(rate);
    Ok(if denominator == 0 { 0 } else { numerator / denominator })
  }

  fn webcam_media_score(
    format: WebcamFrameFormat,
    width: u32,
    height: u32,
    fps: u32,
    requested_width: u32,
    requested_height: u32,
    requested_fps: u32,
  ) -> u64 {
    let pixel_count = u64::from(width) * u64::from(height);
    let requested_pixel_count = u64::from(requested_width) * u64::from(requested_height);
    let pixel_delta = pixel_count.abs_diff(requested_pixel_count);
    let aspect_delta =
      (u64::from(width) * u64::from(requested_height)).abs_diff(u64::from(requested_width) * u64::from(height));
    let fps_delta = if fps == 0 {
      u64::from(requested_fps.max(1))
    } else {
      u64::from(fps.abs_diff(requested_fps))
    };
    let below_requested_fps = if fps == 0 {
      u64::from(requested_fps.max(1))
    } else if fps >= requested_fps {
      0
    } else {
      u64::from(requested_fps - fps)
    };
    below_requested_fps.saturating_mul(10_000_000_000)
      + fps_delta.saturating_mul(100_000_000)
      + pixel_delta.saturating_mul(1000)
      + aspect_delta.saturating_mul(10)
      + format.rank()
  }

  fn bgra_to_resized_rgba(
    bgra: &[u8],
    frame_width: u32,
    frame_height: u32,
    output_width: u32,
    output_height: u32,
  ) -> Result<Vec<u8>, VideoError> {
    if frame_width == 0 || frame_height == 0 || output_width == 0 || output_height == 0 {
      return Err(VideoError::new(format!(
        "Invalid webcam frame dimensions: captured={}x{} output={}x{}.",
        frame_width, frame_height, output_width, output_height
      )));
    }
    let expected_len = frame_width as usize * frame_height as usize * 4;
    if bgra.len() < expected_len {
      return Err(VideoError::new(format!(
        "Invalid webcam BGRA frame length: got={} expected_at_least={expected_len}.",
        bgra.len()
      )));
    }

    let dst_stride = output_width as usize * 4;
    let mut out = vec![0u8; dst_stride * output_height as usize];
    for row in 0..output_height as usize {
      let src_y = row * frame_height as usize / output_height as usize;
      for column in 0..output_width as usize {
        let src_x = column * frame_width as usize / output_width as usize;
        let src = (src_y * frame_width as usize + src_x) * 4;
        let dst = row * dst_stride + column * 4;
        out[dst] = bgra[src + 2];
        out[dst + 1] = bgra[src + 1];
        out[dst + 2] = bgra[src];
        out[dst + 3] = 255;
      }
    }
    Ok(out)
  }

  fn nv12_to_resized_rgba(
    nv12: &[u8],
    frame_width: u32,
    frame_height: u32,
    output_width: u32,
    output_height: u32,
  ) -> Result<Vec<u8>, VideoError> {
    validate_frame_dimensions(frame_width, frame_height, output_width, output_height)?;
    let expected_len = WebcamFrameFormat::Nv12.buffer_len(frame_width, frame_height)?;
    if nv12.len() < expected_len {
      return Err(VideoError::new(format!(
        "Invalid webcam NV12 frame length: got={} expected_at_least={expected_len}.",
        nv12.len()
      )));
    }

    let frame_width = frame_width as usize;
    let frame_height = frame_height as usize;
    let output_width = output_width as usize;
    let output_height = output_height as usize;
    let (y_plane, uv_plane) = nv12.split_at(frame_width * frame_height);
    let dst_stride = output_width * 4;
    let mut out = vec![0u8; dst_stride * output_height];
    for row in 0..output_height {
      let src_y = row * frame_height / output_height;
      for column in 0..output_width {
        let src_x = column * frame_width / output_width;
        let y_value = y_plane[src_y * frame_width + src_x] as i32;
        let uv_offset = (src_y / 2) * frame_width + (src_x & !1);
        let u = uv_plane[uv_offset] as i32;
        let v = uv_plane[uv_offset + 1] as i32;
        let dst = row * dst_stride + column * 4;
        write_yuv_rgba(&mut out[dst..dst + 4], y_value, u, v);
      }
    }
    Ok(out)
  }

  fn yuy2_to_resized_rgba(
    yuy2: &[u8],
    frame_width: u32,
    frame_height: u32,
    output_width: u32,
    output_height: u32,
  ) -> Result<Vec<u8>, VideoError> {
    validate_frame_dimensions(frame_width, frame_height, output_width, output_height)?;
    let expected_len = WebcamFrameFormat::Yuy2.buffer_len(frame_width, frame_height)?;
    if yuy2.len() < expected_len {
      return Err(VideoError::new(format!(
        "Invalid webcam YUY2 frame length: got={} expected_at_least={expected_len}.",
        yuy2.len()
      )));
    }

    let frame_width = frame_width as usize;
    let frame_height = frame_height as usize;
    let output_width = output_width as usize;
    let output_height = output_height as usize;
    let dst_stride = output_width * 4;
    let mut out = vec![0u8; dst_stride * output_height];
    for row in 0..output_height {
      let src_y = row * frame_height / output_height;
      for column in 0..output_width {
        let src_x = column * frame_width / output_width;
        let pair_x = src_x & !1;
        let pair = (src_y * frame_width + pair_x) * 2;
        let y_offset = if src_x == pair_x { 0 } else { 2 };
        let y_value = yuy2[pair + y_offset] as i32;
        let u = yuy2[pair + 1] as i32;
        let v = yuy2[pair + 3] as i32;
        let dst = row * dst_stride + column * 4;
        write_yuv_rgba(&mut out[dst..dst + 4], y_value, u, v);
      }
    }
    Ok(out)
  }

  fn validate_frame_dimensions(
    frame_width: u32,
    frame_height: u32,
    output_width: u32,
    output_height: u32,
  ) -> Result<(), VideoError> {
    if frame_width == 0 || frame_height == 0 || output_width == 0 || output_height == 0 {
      return Err(VideoError::new(format!(
        "Invalid webcam frame dimensions: captured={}x{} output={}x{}.",
        frame_width, frame_height, output_width, output_height
      )));
    }
    Ok(())
  }

  fn write_yuv_rgba(rgba: &mut [u8], y: i32, u: i32, v: i32) {
    let c = (y - 16).max(0);
    let d = u - 128;
    let e = v - 128;
    rgba[0] = clamp_video_byte((298 * c + 409 * e + 128) >> 8);
    rgba[1] = clamp_video_byte((298 * c - 100 * d - 208 * e + 128) >> 8);
    rgba[2] = clamp_video_byte((298 * c + 516 * d + 128) >> 8);
    rgba[3] = 255;
  }

  fn clamp_video_byte(value: i32) -> u8 {
    value.clamp(0, 255) as u8
  }

  fn unpack_u32_pair(value: u64) -> (u32, u32) {
    ((value >> 32) as u32, value as u32)
  }

  #[cfg(test)]
  mod tests {
    use super::{
      WebcamFrameFormat, bgra_to_resized_rgba, nv12_to_resized_rgba, webcam_media_score, yuy2_to_resized_rgba,
    };

    #[test]
    fn bgra_to_resized_rgba_swaps_blue_and_red() {
      let bgra = vec![30, 20, 10, 255, 60, 50, 40, 255];
      let rgba = bgra_to_resized_rgba(&bgra, 2, 1, 2, 1).unwrap();
      assert_eq!(rgba, vec![10, 20, 30, 255, 40, 50, 60, 255]);
    }

    #[test]
    fn nv12_to_resized_rgba_converts_black_frame() {
      let nv12 = vec![16, 16, 16, 16, 128, 128];
      let rgba = nv12_to_resized_rgba(&nv12, 2, 2, 2, 2).unwrap();
      assert_eq!(rgba, vec![0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255]);
    }

    #[test]
    fn yuy2_to_resized_rgba_converts_black_pair() {
      let yuy2 = vec![16, 128, 16, 128];
      let rgba = yuy2_to_resized_rgba(&yuy2, 2, 1, 2, 1).unwrap();
      assert_eq!(rgba, vec![0, 0, 0, 255, 0, 0, 0, 255]);
    }

    #[test]
    fn webcam_media_score_prefers_requested_fps_over_resolution() {
      let fast_lower_resolution = webcam_media_score(WebcamFrameFormat::Nv12, 640, 480, 120, 1280, 720, 120);
      let slow_exact_resolution = webcam_media_score(WebcamFrameFormat::Nv12, 1280, 720, 30, 1280, 720, 120);

      assert!(fast_lower_resolution < slow_exact_resolution);
    }

    #[test]
    fn webcam_media_score_penalizes_unknown_fps_for_high_fps_request() {
      let known_fps = webcam_media_score(WebcamFrameFormat::Nv12, 640, 480, 120, 1280, 720, 120);
      let unknown_fps = webcam_media_score(WebcamFrameFormat::Nv12, 1280, 720, 0, 1280, 720, 120);

      assert!(known_fps < unknown_fps);
    }
  }
}

#[cfg(target_os = "windows")]
pub(super) use windows_impl::WebcamCapture;

#[cfg(not(target_os = "windows"))]
mod stub_impl {
  use super::VideoError;

  pub(crate) struct WebcamCapture;

  impl WebcamCapture {
    pub(crate) fn open(_source_id: u32, _width: u16, _height: u16, _fps: u32) -> Result<Self, VideoError> {
      Err(VideoError::new("Webcam capture backend is not implemented."))
    }

    pub(crate) fn capture_rgba(&mut self, _width: u16, _height: u16) -> Result<Vec<u8>, VideoError> {
      Err(VideoError::new("Webcam capture backend is not implemented."))
    }
  }
}

#[cfg(not(target_os = "windows"))]
pub(super) use stub_impl::WebcamCapture;

use super::VideoError;
