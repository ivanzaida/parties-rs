use std::{
  ffi::c_void,
  ptr,
  sync::{Arc, mpsc},
};

use core_foundation_sys::{
  base::{CFAllocatorRef, CFRelease, CFTypeRef, OSStatus, kCFAllocatorDefault},
  dictionary::{CFDictionaryCreate, CFDictionaryRef, kCFTypeDictionaryKeyCallBacks, kCFTypeDictionaryValueCallBacks},
  number::{CFNumberCreate, kCFBooleanTrue, kCFNumberSInt32Type},
};
use core_media_sys::{
  block_buffer::{CMBlockBufferCreateWithMemoryBlock, CMBlockBufferRef, kCMBlockBufferAlwaysCopyDataFlag},
  format_description::CMVideoFormatDescriptionRef,
  sample_buffer::{CMSampleBufferRef, CMSampleTimingInfo},
  time::{CMTime, kCMTimeFlags_Valid, kCMTimeInvalid},
};
use core_video_sys::pixel_buffer::{
  CVPixelBufferGetHeight, CVPixelBufferGetPixelFormatType, CVPixelBufferGetWidth, CVPixelBufferRef,
  kCVPixelBufferIOSurfacePropertiesKey, kCVPixelBufferMetalCompatibilityKey, kCVPixelBufferPixelFormatTypeKey,
  kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange,
};

use super::{
  DecodedVideoPixelFormat, NativeDecodedVideoFrame, NativeVideoBackend, VideoBroadcast, VideoBroadcastConfig,
  VideoDecodeConfig, VideoDecoder, VideoError,
};
use crate::{
  network::{protocol::VideoCodecId, protocol::data::VideoFrame, server::Server},
  services::logger,
};

#[allow(dead_code)]
const BACKEND_ORDER: [NativeVideoBackend; 1] = [NativeVideoBackend::AppleVideoToolbox];
const NO_ERR: OSStatus = 0;

#[repr(C)]
struct VTDecompressionSession(c_void);

type VTDecompressionSessionRef = *mut VTDecompressionSession;
type VTDecodeFrameFlags = u32;
type VTDecodeInfoFlags = u32;

type VTDecompressionOutputCallback = extern "C" fn(
  decompression_output_ref_con: *mut c_void,
  source_frame_ref_con: *mut c_void,
  status: OSStatus,
  info_flags: VTDecodeInfoFlags,
  image_buffer: CVPixelBufferRef,
  presentation_time_stamp: CMTime,
  presentation_duration: CMTime,
);

#[repr(C)]
struct VTDecompressionOutputCallbackRecord {
  decompression_output_callback: VTDecompressionOutputCallback,
  decompression_output_ref_con: *mut c_void,
}

unsafe extern "C" {
  fn CMVideoFormatDescriptionCreateFromH264ParameterSets(
    allocator: CFAllocatorRef,
    parameter_set_count: usize,
    parameter_set_pointers: *const *const u8,
    parameter_set_sizes: *const usize,
    nal_unit_header_length: i32,
    format_description_out: *mut CMVideoFormatDescriptionRef,
  ) -> OSStatus;

  fn CMVideoFormatDescriptionCreateFromHEVCParameterSets(
    allocator: CFAllocatorRef,
    parameter_set_count: usize,
    parameter_set_pointers: *const *const u8,
    parameter_set_sizes: *const usize,
    nal_unit_header_length: i32,
    extensions: CFDictionaryRef,
    format_description_out: *mut CMVideoFormatDescriptionRef,
  ) -> OSStatus;

  fn CMSampleBufferCreateReady(
    allocator: CFAllocatorRef,
    data_buffer: CMBlockBufferRef,
    format_description: CMVideoFormatDescriptionRef,
    sample_count: isize,
    sample_timing_entry_count: isize,
    sample_timing_array: *const CMSampleTimingInfo,
    sample_size_entry_count: isize,
    sample_size_array: *const usize,
    sample_buffer_out: *mut CMSampleBufferRef,
  ) -> OSStatus;

  fn VTDecompressionSessionCreate(
    allocator: CFAllocatorRef,
    video_format_description: CMVideoFormatDescriptionRef,
    video_decoder_specification: CFDictionaryRef,
    destination_image_buffer_attributes: CFDictionaryRef,
    output_callback: *const VTDecompressionOutputCallbackRecord,
    decompression_session_out: *mut VTDecompressionSessionRef,
  ) -> OSStatus;

  fn VTDecompressionSessionDecodeFrame(
    session: VTDecompressionSessionRef,
    sample_buffer: CMSampleBufferRef,
    decode_flags: VTDecodeFrameFlags,
    source_frame_ref_con: *mut c_void,
    info_flags_out: *mut VTDecodeInfoFlags,
  ) -> OSStatus;

  fn VTDecompressionSessionInvalidate(session: VTDecompressionSessionRef);
}

pub(super) fn encode(_server: Arc<Server>, config: VideoBroadcastConfig) -> Result<VideoBroadcast, VideoError> {
  let _ = (&config.source_kind, config.source_id);
  Err(VideoError::new(
    "macOS native video encoder is not wired yet. Backend is VideoToolbox.",
  ))
}

pub(super) struct NativeVideoDecoder {
  config: VideoDecodeConfig,
  session: Option<VTSession>,
  output_rx: mpsc::Receiver<DecodedCallbackFrame>,
  output_tx: mpsc::Sender<DecodedCallbackFrame>,
}

unsafe impl Send for NativeVideoDecoder {}

struct VTSession {
  session: VTDecompressionSessionRef,
  format_description: CMVideoFormatDescriptionRef,
  _callback_tx: Box<mpsc::Sender<DecodedCallbackFrame>>,
}

unsafe impl Send for VTSession {}

struct DecodedCallbackFrame {
  status: OSStatus,
  native_image: Option<lurq::images::ImageData>,
}

pub(super) fn decode(config: VideoDecodeConfig) -> Result<VideoDecoder, VideoError> {
  let (output_tx, output_rx) = mpsc::channel();
  let decoder = NativeVideoDecoder {
    config: config.clone(),
    session: None,
    output_rx,
    output_tx,
  };
  logger::log(&format!(
    "[video/macos] decoder ready through VideoToolbox: codec={:?} size={}x{}",
    config.codec, config.width, config.height
  ));
  Ok(VideoDecoder::from_macos(
    decoder,
    config,
    NativeVideoBackend::AppleVideoToolbox,
  ))
}

impl NativeVideoDecoder {
  pub(super) fn decode_frame(
    &mut self,
    frame: &VideoFrame,
    output: bool,
    output_buffer: Option<Vec<u8>>,
  ) -> Result<Option<NativeDecodedVideoFrame>, VideoError> {
    if frame.codec == VideoCodecId::Av1 {
      return Err(VideoError::new(
        "macOS VideoToolbox AV1 decode is not wired yet; H.264 and H.265 are supported.",
      ));
    }

    let access_units = AccessUnits::parse(frame.codec, &frame.encoded)?;
    if self.session.is_none() || access_units.has_parameter_sets() {
      if let Some(session) = self.session.take() {
        drop(session);
      }
      self.session = Some(VTSession::new(&self.config, &access_units, self.output_tx.clone())?);
    }

    let Some(session) = self.session.as_mut() else {
      return Err(VideoError::new(
        "VideoToolbox session is not initialized; waiting for a keyframe with parameter sets.",
      ));
    };
    let sample_data = access_units.sample_data()?;
    let sample = SampleBuffer::new(&sample_data, session.format_description, frame.timestamp)?;
    while self.output_rx.try_recv().is_ok() {}

    let mut info_flags = 0;
    let status =
      unsafe { VTDecompressionSessionDecodeFrame(session.session, sample.ptr, 0, ptr::null_mut(), &mut info_flags) };
    if status != NO_ERR {
      return Err(VideoError::new(format!(
        "VideoToolbox failed to decode {} frame {}: OSStatus {status}.",
        codec_label(frame.codec),
        frame.frame_number
      )));
    }

    let mut latest = None;
    while let Ok(decoded) = self.output_rx.try_recv() {
      if decoded.status != NO_ERR {
        return Err(VideoError::new(format!(
          "VideoToolbox output callback failed for {} frame {}: OSStatus {}.",
          codec_label(frame.codec),
          frame.frame_number,
          decoded.status
        )));
      }
      latest = decoded.native_image;
    }

    if !output {
      return Ok(None);
    }

    match latest {
      Some(native_image) => {
        let _ = output_buffer;
        Ok(Some(NativeDecodedVideoFrame {
          format: DecodedVideoPixelFormat::Nv12,
          pixels: Vec::new(),
          native_image: Some(native_image),
        }))
      }
      None => Ok(None),
    }
  }
}

impl VTSession {
  fn new(
    config: &VideoDecodeConfig,
    access_units: &AccessUnits,
    output_tx: mpsc::Sender<DecodedCallbackFrame>,
  ) -> Result<Self, VideoError> {
    let format_description = create_format_description(config.codec, access_units)?;
    let attributes = PixelBufferAttributes::native_nv12()?;
    let callback_tx = Box::new(output_tx);
    let callback = VTDecompressionOutputCallbackRecord {
      decompression_output_callback,
      decompression_output_ref_con: (&*callback_tx as *const mpsc::Sender<DecodedCallbackFrame>)
        .cast_mut()
        .cast(),
    };
    let mut session = ptr::null_mut();
    let status = unsafe {
      VTDecompressionSessionCreate(
        kCFAllocatorDefault,
        format_description,
        ptr::null(),
        attributes.ptr,
        &callback,
        &mut session,
      )
    };
    if status != NO_ERR || session.is_null() {
      unsafe {
        CFRelease(format_description.cast());
      }
      return Err(VideoError::new(format!(
        "Failed to create VideoToolbox decoder session for {}: OSStatus {status}.",
        codec_label(config.codec)
      )));
    }

    Ok(Self {
      session,
      format_description,
      _callback_tx: callback_tx,
    })
  }
}

impl Drop for VTSession {
  fn drop(&mut self) {
    unsafe {
      VTDecompressionSessionInvalidate(self.session);
      CFRelease(self.session.cast());
      CFRelease(self.format_description.cast());
    }
  }
}

extern "C" fn decompression_output_callback(
  decompression_output_ref_con: *mut c_void,
  _source_frame_ref_con: *mut c_void,
  status: OSStatus,
  _info_flags: VTDecodeInfoFlags,
  image_buffer: CVPixelBufferRef,
  _presentation_time_stamp: CMTime,
  _presentation_duration: CMTime,
) {
  let sender = unsafe { &*(decompression_output_ref_con.cast::<mpsc::Sender<DecodedCallbackFrame>>()) };
  let native_image = if status == NO_ERR && !image_buffer.is_null() {
    native_image_from_pixel_buffer(image_buffer).ok()
  } else {
    None
  };
  let _ = sender.send(DecodedCallbackFrame { status, native_image });
}

fn create_format_description(
  codec: VideoCodecId,
  access_units: &AccessUnits,
) -> Result<CMVideoFormatDescriptionRef, VideoError> {
  let mut format_description = ptr::null_mut();
  let status = match codec {
    VideoCodecId::H264 => {
      let sps = access_units
        .h264_sps
        .as_deref()
        .ok_or_else(|| VideoError::new("H.264 keyframe is missing SPS parameter set."))?;
      let pps = access_units
        .h264_pps
        .as_deref()
        .ok_or_else(|| VideoError::new("H.264 keyframe is missing PPS parameter set."))?;
      let pointers = [sps.as_ptr(), pps.as_ptr()];
      let sizes = [sps.len(), pps.len()];
      unsafe {
        CMVideoFormatDescriptionCreateFromH264ParameterSets(
          kCFAllocatorDefault,
          pointers.len(),
          pointers.as_ptr(),
          sizes.as_ptr(),
          4,
          &mut format_description,
        )
      }
    }
    VideoCodecId::H265 => {
      let vps = access_units
        .h265_vps
        .as_deref()
        .ok_or_else(|| VideoError::new("H.265 keyframe is missing VPS parameter set."))?;
      let sps = access_units
        .h265_sps
        .as_deref()
        .ok_or_else(|| VideoError::new("H.265 keyframe is missing SPS parameter set."))?;
      let pps = access_units
        .h265_pps
        .as_deref()
        .ok_or_else(|| VideoError::new("H.265 keyframe is missing PPS parameter set."))?;
      let pointers = [vps.as_ptr(), sps.as_ptr(), pps.as_ptr()];
      let sizes = [vps.len(), sps.len(), pps.len()];
      unsafe {
        CMVideoFormatDescriptionCreateFromHEVCParameterSets(
          kCFAllocatorDefault,
          pointers.len(),
          pointers.as_ptr(),
          sizes.as_ptr(),
          4,
          ptr::null(),
          &mut format_description,
        )
      }
    }
    VideoCodecId::Av1 | VideoCodecId::Unknown => return Err(VideoError::new("Unsupported macOS video codec.")),
  };

  if status != NO_ERR || format_description.is_null() {
    return Err(VideoError::new(format!(
      "Failed to create {} VideoToolbox format description: OSStatus {status}.",
      codec_label(codec)
    )));
  }

  Ok(format_description)
}

struct PixelBufferAttributes {
  ptr: CFDictionaryRef,
  pixel_format: CFTypeRef,
  iosurface_properties: CFTypeRef,
}

impl PixelBufferAttributes {
  fn native_nv12() -> Result<Self, VideoError> {
    let pixel_format_value = kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange as i32;
    let pixel_format = unsafe {
      CFNumberCreate(
        kCFAllocatorDefault,
        kCFNumberSInt32Type,
        (&pixel_format_value as *const i32).cast(),
      )
    };
    if pixel_format.is_null() {
      return Err(VideoError::new("Failed to create CoreFoundation pixel format number."));
    }

    let iosurface_properties = unsafe {
      CFDictionaryCreate(
        kCFAllocatorDefault,
        ptr::null(),
        ptr::null(),
        0,
        &kCFTypeDictionaryKeyCallBacks,
        &kCFTypeDictionaryValueCallBacks,
      )
    };
    if iosurface_properties.is_null() {
      unsafe {
        CFRelease(pixel_format.cast());
      }
      return Err(VideoError::new(
        "Failed to create empty IOSurface properties dictionary.",
      ));
    }

    let keys = [
      unsafe { kCVPixelBufferPixelFormatTypeKey }.cast::<c_void>(),
      unsafe { kCVPixelBufferMetalCompatibilityKey }.cast::<c_void>(),
      unsafe { kCVPixelBufferIOSurfacePropertiesKey }.cast::<c_void>(),
    ];
    let values = [
      pixel_format.cast::<c_void>(),
      unsafe { kCFBooleanTrue }.cast::<c_void>(),
      iosurface_properties.cast::<c_void>(),
    ];
    let dictionary = unsafe {
      CFDictionaryCreate(
        kCFAllocatorDefault,
        keys.as_ptr(),
        values.as_ptr(),
        keys.len() as isize,
        &kCFTypeDictionaryKeyCallBacks,
        &kCFTypeDictionaryValueCallBacks,
      )
    };
    if dictionary.is_null() {
      unsafe {
        CFRelease(pixel_format.cast());
        CFRelease(iosurface_properties.cast());
      }
      return Err(VideoError::new("Failed to create CoreVideo pixel buffer attributes."));
    }

    Ok(Self {
      ptr: dictionary,
      pixel_format: pixel_format.cast(),
      iosurface_properties: iosurface_properties.cast(),
    })
  }
}

impl Drop for PixelBufferAttributes {
  fn drop(&mut self) {
    unsafe {
      CFRelease(self.ptr.cast());
      CFRelease(self.pixel_format);
      CFRelease(self.iosurface_properties);
    }
  }
}

struct SampleBuffer {
  ptr: CMSampleBufferRef,
  block: CMBlockBufferRef,
}

impl SampleBuffer {
  fn new(
    sample_data: &[u8],
    format_description: CMVideoFormatDescriptionRef,
    timestamp_ms: u32,
  ) -> Result<Self, VideoError> {
    let mut block = ptr::null();
    let mut sample_copy = sample_data.to_vec();
    let status = unsafe {
      CMBlockBufferCreateWithMemoryBlock(
        kCFAllocatorDefault,
        sample_copy.as_mut_ptr().cast(),
        sample_copy.len(),
        kCFAllocatorDefault,
        ptr::null(),
        0,
        sample_copy.len(),
        kCMBlockBufferAlwaysCopyDataFlag,
        &mut block,
      )
    };
    if status != NO_ERR || block.is_null() {
      return Err(VideoError::new(format!(
        "Failed to create CoreMedia block buffer: OSStatus {status}."
      )));
    }
    drop(sample_copy);

    let timing = CMSampleTimingInfo {
      duration: unsafe { kCMTimeInvalid },
      presentation_time_stamp: cm_time_millis(timestamp_ms),
      decode_time_stamp: unsafe { kCMTimeInvalid },
    };
    let sample_size = sample_data.len();
    let mut sample = ptr::null_mut();
    let status = unsafe {
      CMSampleBufferCreateReady(
        kCFAllocatorDefault,
        block,
        format_description,
        1,
        1,
        &timing,
        1,
        &sample_size,
        &mut sample,
      )
    };
    if status != NO_ERR || sample.is_null() {
      unsafe {
        CFRelease(block.cast());
      }
      return Err(VideoError::new(format!(
        "Failed to create CoreMedia sample buffer: OSStatus {status}."
      )));
    }

    Ok(Self { ptr: sample, block })
  }
}

impl Drop for SampleBuffer {
  fn drop(&mut self) {
    unsafe {
      CFRelease(self.ptr.cast());
      CFRelease(self.block.cast());
    }
  }
}

#[derive(Default)]
struct AccessUnits {
  h264_sps: Option<Vec<u8>>,
  h264_pps: Option<Vec<u8>>,
  h265_vps: Option<Vec<u8>>,
  h265_sps: Option<Vec<u8>>,
  h265_pps: Option<Vec<u8>>,
  nals: Vec<Vec<u8>>,
  length_prefixed_input: bool,
}

impl AccessUnits {
  fn parse(codec: VideoCodecId, encoded: &[u8]) -> Result<Self, VideoError> {
    if encoded.is_empty() {
      return Err(VideoError::new("Encoded video frame is empty."));
    }

    let mut units = Self::default();
    let nals = if looks_like_annex_b(encoded) {
      split_annex_b(encoded)
    } else {
      units.length_prefixed_input = true;
      split_length_prefixed(encoded)?
    };

    for nal in nals {
      if nal.is_empty() {
        continue;
      }
      match codec {
        VideoCodecId::H264 => match nal[0] & 0x1f {
          7 => units.h264_sps = Some(nal.clone()),
          8 => units.h264_pps = Some(nal.clone()),
          _ => {}
        },
        VideoCodecId::H265 => match (nal[0] >> 1) & 0x3f {
          32 => units.h265_vps = Some(nal.clone()),
          33 => units.h265_sps = Some(nal.clone()),
          34 => units.h265_pps = Some(nal.clone()),
          _ => {}
        },
        VideoCodecId::Av1 | VideoCodecId::Unknown => {}
      }
      units.nals.push(nal);
    }

    if units.nals.is_empty() {
      return Err(VideoError::new("Encoded video frame contains no NAL units."));
    }

    Ok(units)
  }

  fn has_parameter_sets(&self) -> bool {
    (self.h264_sps.is_some() && self.h264_pps.is_some())
      || (self.h265_vps.is_some() && self.h265_sps.is_some() && self.h265_pps.is_some())
  }

  fn sample_data(&self) -> Result<Vec<u8>, VideoError> {
    let mut out = Vec::new();
    for nal in &self.nals {
      let len = u32::try_from(nal.len()).map_err(|_| VideoError::new("NAL unit is too large."))?;
      out.extend_from_slice(&len.to_be_bytes());
      out.extend_from_slice(nal);
    }
    Ok(out)
  }
}

fn native_image_from_pixel_buffer(pixel_buffer: CVPixelBufferRef) -> Result<lurq::images::ImageData, VideoError> {
  let pixel_format = unsafe { CVPixelBufferGetPixelFormatType(pixel_buffer) };
  if pixel_format != kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange {
    return Err(VideoError::new(format!(
      "VideoToolbox returned unsupported pixel format 0x{pixel_format:08x}; expected NV12/420v."
    )));
  }

  let width = unsafe { CVPixelBufferGetWidth(pixel_buffer) } as u32;
  let height = unsafe { CVPixelBufferGetHeight(pixel_buffer) } as u32;
  if width == 0 || height == 0 || width % 2 != 0 || height % 2 != 0 {
    return Err(VideoError::new(format!(
      "VideoToolbox returned invalid NV12 dimensions {width}x{height}."
    )));
  }

  let pixel_buffer = unsafe { lurq::images::MacosCvPixelBuffer::retain(pixel_buffer) };
  Ok(lurq::images::NativeImageData::from_macos_cv_pixel_buffer_nv12(width, height, pixel_buffer).image_data())
}

fn looks_like_annex_b(bytes: &[u8]) -> bool {
  bytes.starts_with(&[0, 0, 1]) || bytes.starts_with(&[0, 0, 0, 1])
}

fn split_annex_b(bytes: &[u8]) -> Vec<Vec<u8>> {
  let mut out = Vec::new();
  let mut cursor = 0;
  while let Some((start_code, start_code_len)) = find_start_code(bytes, cursor) {
    let nal_start = start_code + start_code_len;
    let next = find_start_code(bytes, nal_start)
      .map(|(index, _)| index)
      .unwrap_or(bytes.len());
    if nal_start < next {
      out.push(bytes[nal_start..next].to_vec());
    }
    cursor = next;
  }
  out
}

fn find_start_code(bytes: &[u8], from: usize) -> Option<(usize, usize)> {
  let mut index = from;
  while index + 3 <= bytes.len() {
    if bytes[index..].starts_with(&[0, 0, 1]) {
      return Some((index, 3));
    }
    if index + 4 <= bytes.len() && bytes[index..].starts_with(&[0, 0, 0, 1]) {
      return Some((index, 4));
    }
    index += 1;
  }
  None
}

fn split_length_prefixed(bytes: &[u8]) -> Result<Vec<Vec<u8>>, VideoError> {
  let mut out = Vec::new();
  let mut cursor = 0;
  while cursor + 4 <= bytes.len() {
    let len = u32::from_be_bytes(bytes[cursor..cursor + 4].try_into().unwrap()) as usize;
    cursor += 4;
    if len == 0 || cursor + len > bytes.len() {
      return Err(VideoError::new("Invalid length-prefixed video NAL unit."));
    }
    out.push(bytes[cursor..cursor + len].to_vec());
    cursor += len;
  }
  if cursor != bytes.len() {
    return Err(VideoError::new("Trailing bytes after length-prefixed video NAL units."));
  }
  Ok(out)
}

fn cm_time_millis(timestamp_ms: u32) -> CMTime {
  CMTime {
    value: i64::from(timestamp_ms),
    timescale: 1000,
    flags: kCMTimeFlags_Valid,
    epoch: 0,
  }
}

fn codec_label(codec: VideoCodecId) -> &'static str {
  match codec {
    VideoCodecId::Av1 => "AV1",
    VideoCodecId::H265 => "H.265",
    VideoCodecId::H264 => "H.264",
    VideoCodecId::Unknown => "Unknown",
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn macos_backend_order_matches_original_parties() {
    assert_eq!(BACKEND_ORDER, [NativeVideoBackend::AppleVideoToolbox]);
  }

  #[test]
  fn splits_annex_b_nals() {
    let nals = split_annex_b(&[0, 0, 0, 1, 0x67, 1, 2, 0, 0, 1, 0x68, 3]);
    assert_eq!(nals, vec![vec![0x67, 1, 2], vec![0x68, 3]]);
  }

  #[test]
  fn splits_length_prefixed_nals() {
    let nals = split_length_prefixed(&[0, 0, 0, 2, 0x67, 1, 0, 0, 0, 1, 0x68]).unwrap();
    assert_eq!(nals, vec![vec![0x67, 1], vec![0x68]]);
  }
}
