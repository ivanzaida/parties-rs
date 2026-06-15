use std::{mem::MaybeUninit, ptr, ptr::NonNull};

use openh264::formats::YUVSource;
use rav1d::{
  include::dav1d::{
    data::Dav1dData,
    dav1d::{Dav1dContext, Dav1dSettings},
    headers::DAV1D_PIXEL_LAYOUT_I420,
    picture::Dav1dPicture,
  },
  src::lib::{
    dav1d_close, dav1d_data_create, dav1d_data_unref, dav1d_default_settings, dav1d_get_picture, dav1d_open,
    dav1d_picture_unref, dav1d_send_data,
  },
};

use super::{DecodedVideoPixelFormat, NativeDecodedVideoFrame, NativeVideoBackend, VideoDecodeConfig, VideoError};
use crate::network::protocol::{VideoCodecId, VideoFrame};

const DAV1D_EAGAIN: i32 = -11;

pub(super) enum SoftwareVideoDecoder {
  Av1(Av1SoftwareDecoder),
  H265(H265SoftwareDecoder),
  H264(H264SoftwareDecoder),
}

pub(super) struct Av1SoftwareDecoder {
  context: Option<Dav1dContext>,
  width: u16,
  height: u16,
}

pub(super) struct H265SoftwareDecoder {
  decoder: rust_h265::Decoder,
  width: u16,
  height: u16,
}

pub(super) struct H264SoftwareDecoder {
  decoder: openh264::decoder::Decoder,
  width: u16,
  height: u16,
}

impl SoftwareVideoDecoder {
  pub(super) fn new(config: &VideoDecodeConfig) -> Result<Self, VideoError> {
    match config.codec {
      VideoCodecId::Av1 => Av1SoftwareDecoder::new(config).map(Self::Av1),
      VideoCodecId::H265 => Ok(Self::H265(H265SoftwareDecoder::new(config))),
      VideoCodecId::H264 => H264SoftwareDecoder::new(config).map(Self::H264),
      VideoCodecId::Unknown => Err(VideoError::new(
        "Software decoder does not support unknown video codec.",
      )),
    }
  }

  pub(super) fn backend(&self) -> NativeVideoBackend {
    match self {
      Self::H264(_) => NativeVideoBackend::OpenH264,
      Self::Av1(_) | Self::H265(_) => NativeVideoBackend::SoftwareDecoder,
    }
  }

  pub(super) fn decode_frame(
    &mut self,
    frame: &VideoFrame,
    output: bool,
    output_buffer: Option<Vec<u8>>,
  ) -> Result<Option<NativeDecodedVideoFrame>, VideoError> {
    match self {
      Self::Av1(decoder) => decoder.decode_frame(frame, output, output_buffer),
      Self::H265(decoder) => decoder.decode_frame(frame, output, output_buffer),
      Self::H264(decoder) => decoder.decode_frame(frame, output, output_buffer),
    }
  }
}

impl Av1SoftwareDecoder {
  fn new(config: &VideoDecodeConfig) -> Result<Self, VideoError> {
    let mut settings = MaybeUninit::<Dav1dSettings>::uninit();
    unsafe {
      dav1d_default_settings(NonNull::new_unchecked(settings.as_mut_ptr()));
    }
    let mut settings = unsafe { settings.assume_init() };
    settings.n_threads = 2;
    settings.max_frame_delay = 1;
    settings.apply_grain = 0;

    let mut context = None;
    let result = unsafe { dav1d_open(Some(NonNull::from(&mut context)), Some(NonNull::from(&mut settings))) };
    if result.0 != 0 {
      return Err(VideoError::new(format!(
        "Failed to start software AV1 decoder: dav1d status {}.",
        result.0
      )));
    }

    Ok(Self {
      context,
      width: config.width,
      height: config.height,
    })
  }

  fn decode_frame(
    &mut self,
    frame: &VideoFrame,
    output: bool,
    output_buffer: Option<Vec<u8>>,
  ) -> Result<Option<NativeDecodedVideoFrame>, VideoError> {
    let Some(context) = self.context else {
      return Err(VideoError::new("Software AV1 decoder is closed."));
    };

    let mut data = Dav1dData::default();
    let data_ptr = unsafe { dav1d_data_create(Some(NonNull::from(&mut data)), frame.encoded.len()) };
    if data_ptr.is_null() {
      return Err(VideoError::new("Software AV1 decoder failed to allocate input packet."));
    }
    unsafe {
      ptr::copy_nonoverlapping(frame.encoded.as_ptr(), data_ptr, frame.encoded.len());
    }

    if let Err(error) = send_av1_data(context, &mut data, frame.frame_number) {
      unsafe {
        dav1d_data_unref(Some(NonNull::from(&mut data)));
      }
      return Err(error);
    }

    let mut latest: Option<NativeDecodedVideoFrame> = None;
    let mut reusable_output = output_buffer;
    loop {
      let mut picture = Dav1dPicture::default();
      let status = unsafe { dav1d_get_picture(Some(context), Some(NonNull::from(&mut picture))) };
      if status.0 == DAV1D_EAGAIN {
        break;
      }
      if status.0 != 0 {
        return Err(VideoError::new(format!(
          "Software AV1 decoder failed to produce output for frame {}: dav1d status {}.",
          frame.frame_number, status.0
        )));
      }

      if output {
        reusable_output = latest.take().map(|frame| frame.pixels).or(reusable_output);
        latest = Some(av1_picture_to_nv12(
          &picture,
          self.width,
          self.height,
          reusable_output.take(),
        )?);
      }
      unsafe {
        dav1d_picture_unref(Some(NonNull::from(&mut picture)));
      }
    }

    Ok(latest)
  }
}

fn send_av1_data(context: Dav1dContext, data: &mut Dav1dData, frame_number: u32) -> Result<(), VideoError> {
  for _ in 0..2 {
    let send = unsafe { dav1d_send_data(Some(context), Some(NonNull::from(&mut *data))) };
    if send.0 == 0 {
      return Ok(());
    }
    if send.0 != DAV1D_EAGAIN {
      return Err(VideoError::new(format!(
        "Software AV1 decoder failed to accept frame {frame_number}: dav1d status {}.",
        send.0
      )));
    }

    let mut picture = Dav1dPicture::default();
    let status = unsafe { dav1d_get_picture(Some(context), Some(NonNull::from(&mut picture))) };
    if status.0 == 0 {
      unsafe {
        dav1d_picture_unref(Some(NonNull::from(&mut picture)));
      }
    } else if status.0 != DAV1D_EAGAIN {
      return Err(VideoError::new(format!(
        "Software AV1 decoder failed while draining before frame {frame_number}: dav1d status {}.",
        status.0
      )));
    }
  }

  Err(VideoError::new(format!(
    "Software AV1 decoder could not accept frame {frame_number}: input queue stayed full."
  )))
}

impl Drop for Av1SoftwareDecoder {
  fn drop(&mut self) {
    if self.context.is_some() {
      unsafe {
        dav1d_close(Some(NonNull::from(&mut self.context)));
      }
    }
  }
}

impl H265SoftwareDecoder {
  fn new(config: &VideoDecodeConfig) -> Self {
    Self {
      decoder: rust_h265::Decoder::new(),
      width: config.width,
      height: config.height,
    }
  }

  fn decode_frame(
    &mut self,
    frame: &VideoFrame,
    output: bool,
    output_buffer: Option<Vec<u8>>,
  ) -> Result<Option<NativeDecodedVideoFrame>, VideoError> {
    let nals = rust_h265::parse_annex_b(&frame.encoded);
    if nals.is_empty() {
      return Err(VideoError::new("Software H.265 decoder expected Annex B NAL units."));
    }

    let mut latest: Option<NativeDecodedVideoFrame> = None;
    let mut reusable_output = output_buffer;
    for nal in &nals {
      let decoded = self.decoder.decode_nal(nal).map_err(|error| {
        VideoError::new(format!(
          "Software H.265 decoder failed on frame {}: {error:?}.",
          frame.frame_number
        ))
      })?;
      if output && let Some(decoded) = decoded {
        reusable_output = latest.take().map(|frame| frame.pixels).or(reusable_output);
        latest = Some(h265_frame_to_nv12(
          &decoded,
          self.width,
          self.height,
          reusable_output.take(),
        )?);
      }
    }

    Ok(latest)
  }
}

impl H264SoftwareDecoder {
  fn new(config: &VideoDecodeConfig) -> Result<Self, VideoError> {
    let decoder = openh264::decoder::Decoder::new()
      .map_err(|error| VideoError::new(format!("Failed to start software H.264 decoder: {error}")))?;
    Ok(Self {
      decoder,
      width: config.width,
      height: config.height,
    })
  }

  fn decode_frame(
    &mut self,
    frame: &VideoFrame,
    output: bool,
    output_buffer: Option<Vec<u8>>,
  ) -> Result<Option<NativeDecodedVideoFrame>, VideoError> {
    let decoded = self.decoder.decode(&frame.encoded).map_err(|error| {
      VideoError::new(format!(
        "Software H.264 decoder failed on frame {}: {error}.",
        frame.frame_number
      ))
    })?;

    if !output {
      return Ok(None);
    }

    decoded
      .map(|decoded| yuv_source_to_nv12(&decoded, self.width, self.height, output_buffer))
      .transpose()
  }
}

fn av1_picture_to_nv12(
  picture: &Dav1dPicture,
  width: u16,
  height: u16,
  output_buffer: Option<Vec<u8>>,
) -> Result<NativeDecodedVideoFrame, VideoError> {
  if picture.p.bpc != 8 || picture.p.layout != DAV1D_PIXEL_LAYOUT_I420 {
    return Err(VideoError::new(format!(
      "Software AV1 decoder returned unsupported pixel format: bpc={} layout={}.",
      picture.p.bpc, picture.p.layout
    )));
  }
  if picture.p.w != i32::from(width) || picture.p.h != i32::from(height) {
    return Err(VideoError::new(format!(
      "Software AV1 decoder returned unexpected dimensions: got={}x{} expected={}x{}.",
      picture.p.w, picture.p.h, width, height
    )));
  }

  let y = picture.data[0].ok_or_else(|| VideoError::new("Software AV1 decoder returned no Y plane."))?;
  let u = picture.data[1].ok_or_else(|| VideoError::new("Software AV1 decoder returned no U plane."))?;
  let v = picture.data[2].ok_or_else(|| VideoError::new("Software AV1 decoder returned no V plane."))?;
  let y_stride = usize::try_from(picture.stride[0])
    .map_err(|_| VideoError::new("Software AV1 decoder returned a negative Y stride."))?;
  let uv_stride = usize::try_from(picture.stride[1])
    .map_err(|_| VideoError::new("Software AV1 decoder returned a negative UV stride."))?;
  let width = usize::from(width);
  let height = usize::from(height);
  let y_len = checked_plane_len(y_stride, height)?;
  let uv_len = checked_plane_len(uv_stride, height / 2)?;
  let y = unsafe { std::slice::from_raw_parts(y.as_ptr().cast::<u8>(), y_len) };
  let u = unsafe { std::slice::from_raw_parts(u.as_ptr().cast::<u8>(), uv_len) };
  let v = unsafe { std::slice::from_raw_parts(v.as_ptr().cast::<u8>(), uv_len) };

  i420_planes_to_nv12(y, u, v, width, height, (y_stride, uv_stride, uv_stride), output_buffer)
}

fn h265_frame_to_nv12(
  frame: &rust_h265::Frame,
  width: u16,
  height: u16,
  output_buffer: Option<Vec<u8>>,
) -> Result<NativeDecodedVideoFrame, VideoError> {
  if frame.bit_depth != 8 {
    return Err(VideoError::new(format!(
      "Software H.265 decoder returned unsupported bit depth {}.",
      frame.bit_depth
    )));
  }
  if frame.width != u32::from(width) || frame.height != u32::from(height) {
    return Err(VideoError::new(format!(
      "Software H.265 decoder returned unexpected dimensions: got={}x{} expected={}x{}.",
      frame.width, frame.height, width, height
    )));
  }

  let y = frame
    .y
    .as_u8()
    .ok_or_else(|| VideoError::new("Software H.265 decoder returned non-8-bit Y plane."))?;
  let u = frame
    .u
    .as_u8()
    .ok_or_else(|| VideoError::new("Software H.265 decoder returned non-8-bit U plane."))?;
  let v = frame
    .v
    .as_u8()
    .ok_or_else(|| VideoError::new("Software H.265 decoder returned non-8-bit V plane."))?;
  let width = usize::from(width);
  let height = usize::from(height);

  i420_planes_to_nv12(y, u, v, width, height, (width, width / 2, width / 2), output_buffer)
}

fn yuv_source_to_nv12(
  decoded: &impl YUVSource,
  width: u16,
  height: u16,
  output_buffer: Option<Vec<u8>>,
) -> Result<NativeDecodedVideoFrame, VideoError> {
  let expected = (usize::from(width), usize::from(height));
  if decoded.dimensions() != expected {
    let (got_width, got_height) = decoded.dimensions();
    return Err(VideoError::new(format!(
      "Software H.264 decoder returned unexpected dimensions: got={}x{} expected={}x{}.",
      got_width, got_height, width, height
    )));
  }

  i420_planes_to_nv12(
    decoded.y(),
    decoded.u(),
    decoded.v(),
    expected.0,
    expected.1,
    decoded.strides(),
    output_buffer,
  )
}

fn i420_planes_to_nv12(
  y: &[u8],
  u: &[u8],
  v: &[u8],
  width: usize,
  height: usize,
  strides: (usize, usize, usize),
  output_buffer: Option<Vec<u8>>,
) -> Result<NativeDecodedVideoFrame, VideoError> {
  if width == 0 || height == 0 || width % 2 != 0 || height % 2 != 0 {
    return Err(VideoError::new(
      "Software decoder requires non-zero even NV12 dimensions.",
    ));
  }
  let (y_stride, u_stride, v_stride) = strides;
  validate_plane_len("Y", y.len(), y_stride, width, height)?;
  validate_plane_len("U", u.len(), u_stride, width / 2, height / 2)?;
  validate_plane_len("V", v.len(), v_stride, width / 2, height / 2)?;

  let nv12_len = nv12_len_usize(width, height)?;
  let mut nv12 = full_overwrite_buffer(output_buffer, nv12_len);
  let (y_dst, uv_dst) = nv12.split_at_mut(width * height);

  for row in 0..height {
    let src = row * y_stride;
    let dst = row * width;
    y_dst[dst..dst + width].copy_from_slice(&y[src..src + width]);
  }

  for row in 0..height / 2 {
    let u_src = row * u_stride;
    let v_src = row * v_stride;
    let dst = row * width;
    for column in 0..width / 2 {
      uv_dst[dst + column * 2] = u[u_src + column];
      uv_dst[dst + column * 2 + 1] = v[v_src + column];
    }
  }

  Ok(NativeDecodedVideoFrame {
    format: DecodedVideoPixelFormat::Nv12,
    pixels: nv12,
    native_image: None,
  })
}

fn validate_plane_len(
  plane: &str,
  actual_len: usize,
  stride: usize,
  row_width: usize,
  rows: usize,
) -> Result<(), VideoError> {
  if stride < row_width {
    return Err(VideoError::new(format!(
      "Software decoder returned invalid {plane} stride: {stride} < {row_width}."
    )));
  }
  let expected_len = checked_plane_len(stride, rows)?;
  if actual_len < expected_len {
    return Err(VideoError::new(format!(
      "Software decoder returned short {plane} plane: got={actual_len} expected_at_least={expected_len}."
    )));
  }
  Ok(())
}

fn checked_plane_len(stride: usize, rows: usize) -> Result<usize, VideoError> {
  stride
    .checked_mul(rows)
    .ok_or_else(|| VideoError::new("Software decoder plane dimensions overflowed."))
}

fn nv12_len_usize(width: usize, height: usize) -> Result<usize, VideoError> {
  width
    .checked_mul(height)
    .and_then(|pixels| pixels.checked_add(pixels / 2))
    .ok_or_else(|| VideoError::new("Software decoder output dimensions overflowed."))
}

fn full_overwrite_buffer(buffer: Option<Vec<u8>>, len: usize) -> Vec<u8> {
  let mut buffer = buffer.unwrap_or_default();
  if buffer.capacity() < len {
    buffer = Vec::with_capacity(len);
  }
  unsafe {
    buffer.set_len(len);
  }
  buffer
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn i420_planes_pack_to_nv12() {
    let y = [1, 2, 3, 4];
    let u = [5];
    let v = [6];
    let decoded = i420_planes_to_nv12(&y, &u, &v, 2, 2, (2, 1, 1), None).unwrap();

    assert_eq!(decoded.format, DecodedVideoPixelFormat::Nv12);
    assert_eq!(decoded.pixels, [1, 2, 3, 4, 5, 6]);
  }

  #[test]
  fn i420_planes_reuse_output_buffer() {
    let y = [1, 2, 3, 4];
    let u = [5];
    let v = [6];
    let mut reusable = Vec::with_capacity(16);
    reusable.extend_from_slice(&[9, 9, 9, 9, 9, 9]);
    let original_ptr = reusable.as_ptr();
    let decoded = i420_planes_to_nv12(&y, &u, &v, 2, 2, (2, 1, 1), Some(reusable)).unwrap();

    assert_eq!(decoded.pixels.as_ptr(), original_ptr);
    assert_eq!(decoded.pixels, [1, 2, 3, 4, 5, 6]);
  }
}
