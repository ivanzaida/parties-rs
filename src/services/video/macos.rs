use std::{
  ffi::c_void,
  mem::MaybeUninit,
  ptr,
  ptr::NonNull,
  slice,
  sync::{Arc, mpsc},
};

use core_foundation_sys::{
  base::{CFAllocatorRef, CFRelease, CFTypeRef, OSStatus, kCFAllocatorDefault},
  data::CFDataCreate,
  dictionary::{CFDictionaryCreate, CFDictionaryRef, kCFTypeDictionaryKeyCallBacks, kCFTypeDictionaryValueCallBacks},
  number::{CFNumberCreate, kCFBooleanFalse, kCFBooleanTrue, kCFNumberSInt32Type},
  string::{CFStringCreateWithBytes, CFStringRef, kCFStringEncodingUTF8},
};
use core_media_sys::{
  block_buffer::{CMBlockBufferCreateWithMemoryBlock, CMBlockBufferRef, CMBlockBufferReplaceDataBytes},
  format_description::CMVideoFormatDescriptionRef,
  sample_buffer::{CMSampleBufferRef, CMSampleTimingInfo},
  time::{CMTime, kCMTimeFlags_Valid, kCMTimeInvalid},
};
use core_video_sys::pixel_buffer::{
  CVPixelBufferGetHeight, CVPixelBufferGetPixelFormatType, CVPixelBufferGetWidth, CVPixelBufferRef,
  kCVPixelBufferIOSurfacePropertiesKey, kCVPixelBufferMetalCompatibilityKey, kCVPixelBufferPixelFormatTypeKey,
  kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange,
};
use rav1d::{
  Dav1dResult,
  include::dav1d::{
    data::Dav1dData,
    dav1d::{Dav1dContext, Dav1dSettings},
    headers::{DAV1D_PIXEL_LAYOUT_I400, DAV1D_PIXEL_LAYOUT_I420, DAV1D_PIXEL_LAYOUT_I422, DAV1D_PIXEL_LAYOUT_I444},
    picture::Dav1dPicture,
  },
  src::lib::{
    dav1d_close, dav1d_data_create, dav1d_data_unref, dav1d_default_settings, dav1d_get_picture, dav1d_open,
    dav1d_picture_unref, dav1d_send_data,
  },
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
const K_CM_VIDEO_CODEC_TYPE_AV1: u32 = 0x6176_3031; // 'av01'
const OBU_SEQUENCE_HEADER: u8 = 1;
const DAV1D_EAGAIN: i32 = -35;
const MAX_SOFTWARE_AV1_PIXELS: u32 = 1920 * 1080;
const SOFTWARE_AV1_THREADS: i32 = 2;
const SOFTWARE_AV1_ENV: &str = "PARTIES_MACOS_SOFTWARE_AV1";
const SIMULATE_UNSUPPORTED_AV1_ENV: &str = "PARTIES_SIMULATE_UNSUPPORTED_AV1";

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

  fn CMVideoFormatDescriptionCreate(
    allocator: CFAllocatorRef,
    codec_type: u32,
    width: i32,
    height: i32,
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
  software_av1: Option<SoftwareAv1Decoder>,
  av1_videotoolbox_unavailable: bool,
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
    software_av1: None,
    av1_videotoolbox_unavailable: false,
    output_rx,
    output_tx,
  };
  logger::log(&format!(
    "[video/macos] decoder ready: codec={:?} size={}x{} av1_videotoolbox_unavailable={}",
    config.codec, config.width, config.height, decoder.av1_videotoolbox_unavailable
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
    if frame.codec == VideoCodecId::Av1 && simulate_unsupported_av1() {
      return Err(unsupported_av1_error());
    }

    if frame.codec == VideoCodecId::Av1 && self.av1_videotoolbox_unavailable {
      return self.decode_av1_software(frame, output, output_buffer);
    }

    let access_units = AccessUnits::parse(frame.codec, &frame.encoded)?;
    if frame.keyframe && self.session.is_none() && !access_units.can_initialize_session(frame.codec) {
      logger::log(&format!(
        "[video/macos] keyframe missing VideoToolbox parameter sets: codec={:?} {}",
        frame.codec,
        access_units.parameter_set_summary()
      ));
    }

    let should_initialize_session = match frame.codec {
      VideoCodecId::Av1 => self.session.is_none(),
      VideoCodecId::H264 | VideoCodecId::H265 => access_units.can_initialize_session(frame.codec),
      VideoCodecId::Unknown => false,
    };

    if should_initialize_session {
      if let Some(session) = self.session.take() {
        drop(session);
      }
      logger::log(&format!(
        "[video/macos] initializing VideoToolbox session from parameter sets: codec={:?} {}",
        frame.codec,
        access_units.parameter_set_summary()
      ));
      match VTSession::new(&self.config, &access_units, self.output_tx.clone()) {
        Ok(session) => self.session = Some(session),
        Err(error) if frame.codec == VideoCodecId::Av1 => {
          self.av1_videotoolbox_unavailable = true;
          logger::log(&format!(
            "[video/macos] VideoToolbox AV1 unavailable; falling back to rav1d software decode: {error}"
          ));
          return self.decode_av1_software(frame, output, output_buffer);
        }
        Err(error) => return Err(error),
      }
    }

    let Some(session) = self.session.as_mut() else {
      return Ok(None);
    };
    let sample_data = access_units.sample_data(frame.codec)?;
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

  fn decode_av1_software(
    &mut self,
    frame: &VideoFrame,
    output: bool,
    output_buffer: Option<Vec<u8>>,
  ) -> Result<Option<NativeDecodedVideoFrame>, VideoError> {
    if !software_av1_enabled() {
      return Err(unsupported_av1_error());
    }

    let pixels = u32::from(frame.width) * u32::from(frame.height);
    if pixels > MAX_SOFTWARE_AV1_PIXELS {
      return Err(VideoError::new(format!(
        "macOS VideoToolbox AV1 is unavailable and software AV1 is disabled for {}x{} streams to avoid excessive CPU usage. Use H.265/H.264 or lower AV1 resolution, or raise the software AV1 limit in macos.rs.",
        frame.width, frame.height
      )));
    }

    let decoder = match self.software_av1.as_mut() {
      Some(decoder) => decoder,
      None => {
        self.software_av1 = Some(SoftwareAv1Decoder::new()?);
        self.software_av1.as_mut().unwrap()
      }
    };
    decoder.decode(frame, output, output_buffer)
  }
}

fn software_av1_enabled() -> bool {
  std::env::var_os(SOFTWARE_AV1_ENV).is_some_and(|value| value == "1" || value == "true")
}

fn simulate_unsupported_av1() -> bool {
  std::env::var_os(SIMULATE_UNSUPPORTED_AV1_ENV).is_some_and(|value| value == "1" || value == "true")
}

fn unsupported_av1_error() -> VideoError {
  VideoError::new(format!(
    "macOS VideoToolbox AV1 is unavailable and software AV1 is disabled because it is too CPU-heavy for realtime playback. Use H.265/H.264, or set {SOFTWARE_AV1_ENV}=1 to force software AV1."
  ))
}

struct SoftwareAv1Decoder {
  context: Option<Dav1dContext>,
}

impl SoftwareAv1Decoder {
  fn new() -> Result<Self, VideoError> {
    let mut settings = MaybeUninit::<Dav1dSettings>::uninit();
    unsafe {
      dav1d_default_settings(NonNull::new(settings.as_mut_ptr()).unwrap());
    }
    let mut settings = unsafe { settings.assume_init() };
    settings.n_threads = SOFTWARE_AV1_THREADS;
    settings.max_frame_delay = 1;
    settings.apply_grain = 0;

    let mut context = None;
    let result = unsafe { dav1d_open(Some(NonNull::from(&mut context)), Some(NonNull::from(&mut settings))) };
    dav1d_result(result, "open rav1d decoder")?;
    if context.is_none() {
      return Err(VideoError::new("rav1d returned no decoder context."));
    }
    Ok(Self { context })
  }

  fn decode(
    &mut self,
    frame: &VideoFrame,
    output: bool,
    output_buffer: Option<Vec<u8>>,
  ) -> Result<Option<NativeDecodedVideoFrame>, VideoError> {
    let Some(context) = self.context else {
      return Err(VideoError::new("rav1d decoder context is closed."));
    };

    let mut data = Dav1dData::default();
    let data_ptr = unsafe { dav1d_data_create(Some(NonNull::from(&mut data)), frame.encoded.len()) };
    if data_ptr.is_null() {
      return Err(VideoError::new("rav1d failed to allocate encoded input buffer."));
    }
    unsafe {
      ptr::copy_nonoverlapping(frame.encoded.as_ptr(), data_ptr, frame.encoded.len());
    }
    data.m.timestamp = i64::from(frame.timestamp);

    let result = unsafe { dav1d_send_data(Some(context), Some(NonNull::from(&mut data))) };
    if result.0 != 0 {
      unsafe {
        dav1d_data_unref(Some(NonNull::from(&mut data)));
      }
      return Err(VideoError::new(format!(
        "rav1d failed to consume AV1 frame {}: {}.",
        frame.frame_number,
        dav1d_error_label(result)
      )));
    }

    let mut latest = None;
    loop {
      let mut picture = Dav1dPicture::default();
      let result = unsafe { dav1d_get_picture(Some(context), Some(NonNull::from(&mut picture))) };
      if result.0 == DAV1D_EAGAIN {
        break;
      }
      if result.0 != 0 {
        return Err(VideoError::new(format!(
          "rav1d failed to output AV1 frame {}: {}.",
          frame.frame_number,
          dav1d_error_label(result)
        )));
      }

      let conversion_result = if output {
        picture_to_nv12(&picture, output_buffer.clone()).map(Some)
      } else {
        Ok(None)
      };
      unsafe {
        dav1d_picture_unref(Some(NonNull::from(&mut picture)));
      }
      let Some(pixels) = conversion_result? else {
        continue;
      };
      if output {
        latest = Some(pixels);
      }
    }

    Ok(latest.map(|pixels| NativeDecodedVideoFrame {
      format: DecodedVideoPixelFormat::Nv12,
      pixels,
      native_image: None,
    }))
  }
}

impl Drop for SoftwareAv1Decoder {
  fn drop(&mut self) {
    let mut context = self.context.take();
    unsafe {
      dav1d_close(Some(NonNull::from(&mut context)));
    }
  }
}

impl VTSession {
  fn new(
    config: &VideoDecodeConfig,
    access_units: &AccessUnits,
    output_tx: mpsc::Sender<DecodedCallbackFrame>,
  ) -> Result<Self, VideoError> {
    let format_description = create_format_description(config, access_units)?;
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
  config: &VideoDecodeConfig,
  access_units: &AccessUnits,
) -> Result<CMVideoFormatDescriptionRef, VideoError> {
  let codec = config.codec;
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
    VideoCodecId::Av1 => {
      let sequence_header = access_units
        .av1_sequence_header
        .as_deref()
        .ok_or_else(|| VideoError::new("AV1 keyframe is missing sequence-header OBU."))?;
      let extensions = Av1FormatExtensions::new(sequence_header)?;
      unsafe {
        CMVideoFormatDescriptionCreate(
          kCFAllocatorDefault,
          K_CM_VIDEO_CODEC_TYPE_AV1,
          i32::from(config.width),
          i32::from(config.height),
          extensions.ptr,
          &mut format_description,
        )
      }
    }
    VideoCodecId::Unknown => return Err(VideoError::new("Unsupported macOS video codec.")),
  };

  if status != NO_ERR || format_description.is_null() {
    return Err(VideoError::new(format!(
      "Failed to create {} VideoToolbox format description: OSStatus {status}.",
      codec_label(codec)
    )));
  }

  Ok(format_description)
}

struct Av1FormatExtensions {
  ptr: CFDictionaryRef,
}

impl Av1FormatExtensions {
  fn new(sequence_header_obu: &[u8]) -> Result<Self, VideoError> {
    let av1c = build_av1c(sequence_header_obu)?;
    let sample_description_atoms_key = cf_string("SampleDescriptionExtensionAtoms")?;
    let av1c_key = cf_string("av1C")?;
    let av1c_data = unsafe { CFDataCreate(kCFAllocatorDefault, av1c.as_ptr(), av1c.len() as isize) };
    if av1c_data.is_null() {
      unsafe {
        CFRelease(sample_description_atoms_key.cast());
        CFRelease(av1c_key.cast());
      }
      return Err(VideoError::new("Failed to create AV1 VideoToolbox av1C data."));
    }

    let atoms_keys = [av1c_key.cast::<c_void>()];
    let atoms_values = [av1c_data.cast::<c_void>()];
    let atoms = unsafe {
      CFDictionaryCreate(
        kCFAllocatorDefault,
        atoms_keys.as_ptr(),
        atoms_values.as_ptr(),
        atoms_keys.len() as isize,
        &kCFTypeDictionaryKeyCallBacks,
        &kCFTypeDictionaryValueCallBacks,
      )
    };
    if atoms.is_null() {
      unsafe {
        CFRelease(sample_description_atoms_key.cast());
        CFRelease(av1c_key.cast());
        CFRelease(av1c_data.cast());
      }
      return Err(VideoError::new("Failed to create AV1 VideoToolbox extension atoms."));
    }

    let format_name_key = cf_string("FormatName")?;
    let format_name = cf_string("av01")?;
    let depth_key = cf_string("Depth")?;
    let depth = cf_number_i32(24)?;
    let bits_per_component_key = cf_string("BitsPerComponent")?;
    let bits_per_component = cf_number_i32(8)?;
    let color_primaries_key = cf_string("CVImageBufferColorPrimaries")?;
    let color_primaries = cf_string("ITU_R_709_2")?;
    let transfer_key = cf_string("CVImageBufferTransferFunction")?;
    let transfer = cf_string("ITU_R_709_2")?;
    let matrix_key = cf_string("CVImageBufferYCbCrMatrix")?;
    let matrix = cf_string("ITU_R_709_2")?;
    let full_range_key = cf_string("FullRangeVideo")?;

    let keys = [
      sample_description_atoms_key.cast::<c_void>(),
      format_name_key.cast::<c_void>(),
      depth_key.cast::<c_void>(),
      bits_per_component_key.cast::<c_void>(),
      color_primaries_key.cast::<c_void>(),
      transfer_key.cast::<c_void>(),
      matrix_key.cast::<c_void>(),
      full_range_key.cast::<c_void>(),
    ];
    let values = [
      atoms.cast::<c_void>(),
      format_name.cast::<c_void>(),
      depth.cast::<c_void>(),
      bits_per_component.cast::<c_void>(),
      color_primaries.cast::<c_void>(),
      transfer.cast::<c_void>(),
      matrix.cast::<c_void>(),
      unsafe { kCFBooleanFalse }.cast::<c_void>(),
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
        CFRelease(sample_description_atoms_key.cast());
        CFRelease(av1c_key.cast());
        CFRelease(av1c_data.cast());
        CFRelease(atoms.cast());
        CFRelease(format_name_key.cast());
        CFRelease(format_name.cast());
        CFRelease(depth_key.cast());
        CFRelease(depth.cast());
        CFRelease(bits_per_component_key.cast());
        CFRelease(bits_per_component.cast());
        CFRelease(color_primaries_key.cast());
        CFRelease(color_primaries.cast());
        CFRelease(transfer_key.cast());
        CFRelease(transfer.cast());
        CFRelease(matrix_key.cast());
        CFRelease(matrix.cast());
        CFRelease(full_range_key.cast());
      }
      return Err(VideoError::new("Failed to create AV1 VideoToolbox format extensions."));
    }

    unsafe {
      CFRelease(sample_description_atoms_key.cast());
      CFRelease(av1c_key.cast());
      CFRelease(av1c_data.cast());
      CFRelease(atoms.cast());
      CFRelease(format_name_key.cast());
      CFRelease(format_name.cast());
      CFRelease(depth_key.cast());
      CFRelease(depth.cast());
      CFRelease(bits_per_component_key.cast());
      CFRelease(bits_per_component.cast());
      CFRelease(color_primaries_key.cast());
      CFRelease(color_primaries.cast());
      CFRelease(transfer_key.cast());
      CFRelease(transfer.cast());
      CFRelease(matrix_key.cast());
      CFRelease(matrix.cast());
      CFRelease(full_range_key.cast());
    }

    Ok(Self { ptr: dictionary })
  }
}

impl Drop for Av1FormatExtensions {
  fn drop(&mut self) {
    unsafe {
      CFRelease(self.ptr.cast());
    }
  }
}

fn cf_string(value: &str) -> Result<CFStringRef, VideoError> {
  let string = unsafe {
    CFStringCreateWithBytes(
      kCFAllocatorDefault,
      value.as_ptr(),
      value.len() as isize,
      kCFStringEncodingUTF8,
      0,
    )
  };
  if string.is_null() {
    return Err(VideoError::new(format!(
      "Failed to create CoreFoundation string {value}."
    )));
  }
  Ok(string)
}

fn cf_number_i32(value: i32) -> Result<CFTypeRef, VideoError> {
  let number = unsafe { CFNumberCreate(kCFAllocatorDefault, kCFNumberSInt32Type, (&value as *const i32).cast()) };
  if number.is_null() {
    return Err(VideoError::new("Failed to create CoreFoundation number."));
  }
  Ok(number.cast())
}

fn build_av1c(sequence_header_obu: &[u8]) -> Result<Vec<u8>, VideoError> {
  let sequence = parse_av1_sequence_header(sequence_header_obu)?.sequence;
  let sequence_header_obu = av1_obu_with_size_field(sequence_header_obu)?;
  let mut av1c = Vec::with_capacity(4 + sequence_header_obu.len());
  av1c.push(0x80 | 1);
  av1c.push((sequence.profile << 5) | (sequence.level_idx & 0x1f));
  av1c.push(
    (sequence.tier << 7)
      | (u8::from(sequence.high_bitdepth) << 6)
      | (u8::from(sequence.twelve_bit) << 5)
      | (u8::from(sequence.monochrome) << 4)
      | (u8::from(sequence.chroma_subsampling_x) << 3)
      | (u8::from(sequence.chroma_subsampling_y) << 2)
      | (sequence.chroma_sample_position & 0x03),
  );
  av1c.push(0);
  av1c.extend_from_slice(&sequence_header_obu);
  Ok(av1c)
}

#[derive(Debug)]
struct ParsedAv1SequenceHeader {
  sequence: Av1SequenceHeader,
  payload_bytes: usize,
}

#[derive(Debug)]
struct Av1SequenceHeader {
  profile: u8,
  level_idx: u8,
  tier: u8,
  high_bitdepth: bool,
  twelve_bit: bool,
  monochrome: bool,
  chroma_subsampling_x: bool,
  chroma_subsampling_y: bool,
  chroma_sample_position: u8,
}

fn parse_av1_sequence_header(sequence_header_obu: &[u8]) -> Result<ParsedAv1SequenceHeader, VideoError> {
  let payload =
    av1_obu_payload(sequence_header_obu)?.ok_or_else(|| VideoError::new("AV1 sequence-header OBU is malformed."))?;
  let mut bits = BitReader::new(payload);

  let profile = bits.read_bits(3)? as u8;
  let _still_picture = bits.read_bool()?;
  let reduced_still_picture_header = bits.read_bool()?;

  let mut level_idx = 0;
  let mut tier = 0;
  let mut decoder_model_info = None;
  let initial_display_delay_present;
  let operating_points;

  if reduced_still_picture_header {
    operating_points = 1;
    level_idx = bits.read_bits(5)? as u8;
    initial_display_delay_present = false;
  } else {
    let timing_info_present = bits.read_bool()?;
    let decoder_model_info_present = if timing_info_present {
      bits.skip_bits(32)?; // num_units_in_display_tick
      bits.skip_bits(32)?; // time_scale
      if bits.read_bool()? {
        bits.read_uvlc()?;
      }
      bits.read_bool()?
    } else {
      false
    };

    if decoder_model_info_present {
      let buffer_delay_length = bits.read_bits(5)? + 1;
      bits.skip_bits(32)?; // num_units_in_decoding_tick
      let buffer_removal_time_length = bits.read_bits(5)? + 1;
      let frame_presentation_time_length = bits.read_bits(5)? + 1;
      decoder_model_info = Some(Av1DecoderModelInfo {
        buffer_delay_length,
        buffer_removal_time_length,
        frame_presentation_time_length,
      });
    }

    initial_display_delay_present = bits.read_bool()?;
    operating_points = bits.read_bits(5)? + 1;
  }

  for i in 0..operating_points {
    bits.skip_bits(12)?; // operating_point_idc
    let current_level_idx = bits.read_bits(5)? as u8;
    let current_tier = if current_level_idx > 7 {
      u8::from(bits.read_bool()?)
    } else {
      0
    };
    if i == 0 {
      level_idx = current_level_idx;
      tier = current_tier;
    }

    if let Some(model_info) = decoder_model_info
      && bits.read_bool()?
    {
      bits.skip_bits(model_info.buffer_delay_length)?;
      bits.skip_bits(model_info.buffer_delay_length)?;
      bits.skip_bits(1)?; // low_delay_mode_flag
    }

    if initial_display_delay_present && bits.read_bool()? {
      bits.skip_bits(4)?;
    }
  }

  let frame_width_bits = bits.read_bits(4)? + 1;
  let frame_height_bits = bits.read_bits(4)? + 1;
  bits.skip_bits(frame_width_bits)?;
  bits.skip_bits(frame_height_bits)?;

  if !reduced_still_picture_header {
    if bits.read_bool()? {
      bits.skip_bits(4)?;
      bits.skip_bits(3)?;
    }
  }

  bits.skip_bits(1)?; // use_128x128_superblock
  bits.skip_bits(1)?; // enable_filter_intra
  bits.skip_bits(1)?; // enable_intra_edge_filter

  if !reduced_still_picture_header {
    bits.skip_bits(1)?; // enable_interintra_compound
    bits.skip_bits(1)?; // enable_masked_compound
    bits.skip_bits(1)?; // enable_warped_motion
    bits.skip_bits(1)?; // enable_dual_filter
    let enable_order_hint = bits.read_bool()?;
    if enable_order_hint {
      bits.skip_bits(1)?; // enable_jnt_comp
      bits.skip_bits(1)?; // enable_ref_frame_mvs
    }
    let seq_force_screen_content_tools = bits.read_select_or_bit()?;
    if seq_force_screen_content_tools > 0 {
      bits.read_select_or_bit()?;
    }
    if enable_order_hint {
      bits.skip_bits(3)?;
    }
  }

  bits.skip_bits(1)?; // enable_superres
  bits.skip_bits(1)?; // enable_cdef
  bits.skip_bits(1)?; // enable_restoration

  let high_bitdepth = bits.read_bool()?;
  let twelve_bit = profile == 2 && high_bitdepth && bits.read_bool()?;
  let monochrome = profile != 1 && bits.read_bool()?;

  let mut color_primaries = 2;
  let mut transfer_characteristics = 2;
  let mut matrix_coefficients = 2;
  if bits.read_bool()? {
    color_primaries = bits.read_bits(8)?;
    transfer_characteristics = bits.read_bits(8)?;
    matrix_coefficients = bits.read_bits(8)?;
  }

  let (chroma_subsampling_x, chroma_subsampling_y, chroma_sample_position) = if monochrome {
    bits.skip_bits(1)?; // color_range
    (false, false, 0)
  } else if color_primaries == 1 && transfer_characteristics == 13 && matrix_coefficients == 0 {
    (false, false, 0)
  } else {
    bits.skip_bits(1)?; // color_range
    let (x, y) = match profile {
      0 => (true, true),
      1 => (false, false),
      2 if twelve_bit => (bits.read_bool()?, false),
      2 => {
        let x = bits.read_bool()?;
        let y = if x { bits.read_bool()? } else { false };
        (x, y)
      }
      _ => return Err(VideoError::new(format!("Unsupported AV1 profile {profile}."))),
    };
    let sample_position = if x && y { bits.read_bits(2)? as u8 } else { 0 };
    (x, y, sample_position)
  };

  bits.skip_bits(1)?; // separate_uv_delta_q
  bits.skip_bits(1)?; // film_grain_params_present
  bits.skip_trailing_bits()?;

  Ok(ParsedAv1SequenceHeader {
    sequence: Av1SequenceHeader {
      profile,
      level_idx,
      tier,
      high_bitdepth,
      twelve_bit,
      monochrome,
      chroma_subsampling_x,
      chroma_subsampling_y,
      chroma_sample_position,
    },
    payload_bytes: bits.bytes_consumed(),
  })
}

#[derive(Clone, Copy)]
struct Av1DecoderModelInfo {
  buffer_delay_length: u32,
  #[allow(dead_code)]
  buffer_removal_time_length: u32,
  #[allow(dead_code)]
  frame_presentation_time_length: u32,
}

fn find_av1_sequence_header_obu(encoded: &[u8]) -> Option<Vec<u8>> {
  let mut offset = 0;
  while offset < encoded.len() {
    let start = offset;
    let header = *encoded.get(offset)?;
    offset += 1;
    if header & 0x80 != 0 {
      return None;
    }
    let obu_type = (header >> 3) & 0x0f;
    let has_extension = header & 0x04 != 0;
    let has_size_field = header & 0x02 != 0;
    if has_extension {
      offset = offset.checked_add(1)?;
      encoded.get(offset - 1)?;
    }
    let payload_size = if has_size_field {
      let (value, bytes_read) = read_leb128(&encoded[offset..])?;
      offset = offset.checked_add(bytes_read)?;
      usize::try_from(value).ok()?
    } else if obu_type == OBU_SEQUENCE_HEADER {
      let header_len = offset.checked_sub(start)?;
      let parsed = parse_av1_sequence_header(&encoded[start..]).ok()?;
      let end = start.checked_add(header_len)?.checked_add(parsed.payload_bytes)?;
      if end > encoded.len() {
        return None;
      }
      return Some(encoded[start..end].to_vec());
    } else {
      encoded.len().checked_sub(offset)?
    };
    let end = offset.checked_add(payload_size)?;
    if end > encoded.len() {
      return None;
    }
    if obu_type == OBU_SEQUENCE_HEADER {
      return Some(encoded[start..end].to_vec());
    }
    offset = end;
  }
  None
}

fn av1_obu_payload(obu: &[u8]) -> Result<Option<&[u8]>, VideoError> {
  if obu.is_empty() {
    return Ok(None);
  }
  let header = obu[0];
  if header & 0x80 != 0 {
    return Err(VideoError::new("AV1 OBU forbidden bit is set."));
  }
  if ((header >> 3) & 0x0f) != OBU_SEQUENCE_HEADER {
    return Ok(None);
  }
  let has_extension = header & 0x04 != 0;
  let has_size_field = header & 0x02 != 0;
  let mut offset = 1usize;
  if has_extension {
    if offset >= obu.len() {
      return Err(VideoError::new("AV1 sequence-header OBU extension is truncated."));
    }
    offset += 1;
  }
  if has_size_field {
    let (payload_size, bytes_read) =
      read_leb128(&obu[offset..]).ok_or_else(|| VideoError::new("AV1 sequence-header OBU size is truncated."))?;
    offset += bytes_read;
    let payload_size =
      usize::try_from(payload_size).map_err(|_| VideoError::new("AV1 sequence-header OBU size is too large."))?;
    let end = offset
      .checked_add(payload_size)
      .ok_or_else(|| VideoError::new("AV1 sequence-header OBU size overflow."))?;
    if end > obu.len() {
      return Err(VideoError::new("AV1 sequence-header OBU payload is truncated."));
    }
    Ok(Some(&obu[offset..end]))
  } else {
    Ok(Some(&obu[offset..]))
  }
}

fn av1_obu_with_size_field(obu: &[u8]) -> Result<Vec<u8>, VideoError> {
  let payload = av1_obu_payload(obu)?.ok_or_else(|| VideoError::new("AV1 sequence-header OBU is malformed."))?;
  if obu[0] & 0x02 != 0 {
    return Ok(obu.to_vec());
  }

  let has_extension = obu[0] & 0x04 != 0;
  let payload_len = parse_av1_sequence_header(obu)?.payload_bytes;
  if payload_len > payload.len() {
    return Err(VideoError::new("AV1 sequence-header OBU payload length is invalid."));
  }
  let payload = &payload[..payload_len];

  let mut out = Vec::with_capacity(obu.len() + 8);
  out.push(obu[0] | 0x02);
  if has_extension {
    let extension = *obu
      .get(1)
      .ok_or_else(|| VideoError::new("AV1 sequence-header OBU extension is truncated."))?;
    out.push(extension);
  }
  write_leb128(payload.len() as u64, &mut out);
  out.extend_from_slice(payload);
  Ok(out)
}

fn read_leb128(input: &[u8]) -> Option<(u64, usize)> {
  let mut value = 0u64;
  for (i, byte) in input.iter().copied().enumerate().take(8) {
    value |= u64::from(byte & 0x7f) << (i * 7);
    if byte & 0x80 == 0 {
      return Some((value, i + 1));
    }
  }
  None
}

fn write_leb128(mut value: u64, out: &mut Vec<u8>) {
  loop {
    let mut byte = (value & 0x7f) as u8;
    value >>= 7;
    if value != 0 {
      byte |= 0x80;
    }
    out.push(byte);
    if value == 0 {
      break;
    }
  }
}

struct BitReader<'a> {
  bytes: &'a [u8],
  bit_offset: usize,
}

impl<'a> BitReader<'a> {
  fn new(bytes: &'a [u8]) -> Self {
    Self { bytes, bit_offset: 0 }
  }

  fn read_bool(&mut self) -> Result<bool, VideoError> {
    Ok(self.read_bits(1)? != 0)
  }

  fn read_bits(&mut self, count: u32) -> Result<u32, VideoError> {
    if count > 32 {
      return Err(VideoError::new("AV1 bit reader cannot read more than 32 bits."));
    }
    let mut value = 0u32;
    for _ in 0..count {
      let byte_index = self.bit_offset / 8;
      let bit_index = 7 - (self.bit_offset % 8);
      let byte = *self
        .bytes
        .get(byte_index)
        .ok_or_else(|| VideoError::new("AV1 sequence-header OBU is truncated."))?;
      value = (value << 1) | u32::from((byte >> bit_index) & 1);
      self.bit_offset += 1;
    }
    Ok(value)
  }

  fn skip_bits(&mut self, count: u32) -> Result<(), VideoError> {
    self.read_bits(count).map(|_| ())
  }

  fn read_select_or_bit(&mut self) -> Result<u32, VideoError> {
    if self.read_bool()? { Ok(2) } else { self.read_bits(1) }
  }

  fn read_uvlc(&mut self) -> Result<u32, VideoError> {
    let mut leading_zeroes = 0u32;
    while !self.read_bool()? {
      leading_zeroes += 1;
      if leading_zeroes >= 32 {
        return Err(VideoError::new("AV1 uvlc value is too large."));
      }
    }
    if leading_zeroes == 0 {
      return Ok(0);
    }
    Ok((1 << leading_zeroes) - 1 + self.read_bits(leading_zeroes)?)
  }

  fn skip_trailing_bits(&mut self) -> Result<(), VideoError> {
    if !self.read_bool()? {
      return Err(VideoError::new("AV1 sequence-header trailing bit is missing."));
    }
    while !self.bit_offset.is_multiple_of(8) {
      if self.read_bool()? {
        return Err(VideoError::new("AV1 sequence-header trailing padding bit is non-zero."));
      }
    }
    Ok(())
  }

  fn bytes_consumed(&self) -> usize {
    self.bit_offset.div_ceil(8)
  }
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
}

impl SampleBuffer {
  fn new(
    sample_data: &[u8],
    format_description: CMVideoFormatDescriptionRef,
    timestamp_ms: u32,
  ) -> Result<Self, VideoError> {
    let mut block = ptr::null();
    let status = unsafe {
      CMBlockBufferCreateWithMemoryBlock(
        kCFAllocatorDefault,
        ptr::null_mut(),
        sample_data.len(),
        kCFAllocatorDefault,
        ptr::null(),
        0,
        sample_data.len(),
        0,
        &mut block,
      )
    };
    if status != NO_ERR || block.is_null() {
      return Err(VideoError::new(format!(
        "Failed to create CoreMedia block buffer: OSStatus {status}."
      )));
    }

    let status = unsafe { CMBlockBufferReplaceDataBytes(sample_data.as_ptr().cast(), block, 0, sample_data.len()) };
    if status != NO_ERR {
      unsafe {
        CFRelease(block.cast());
      }
      return Err(VideoError::new(format!(
        "Failed to copy encoded video bytes into CoreMedia block buffer: OSStatus {status}."
      )));
    }

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

    unsafe {
      CFRelease(block.cast());
    }

    Ok(Self { ptr: sample })
  }
}

impl Drop for SampleBuffer {
  fn drop(&mut self) {
    unsafe {
      CFRelease(self.ptr.cast());
    }
  }
}

#[derive(Default)]
struct AccessUnits {
  raw_sample: Option<Vec<u8>>,
  av1_sequence_header: Option<Vec<u8>>,
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
    if codec == VideoCodecId::Av1 {
      units.raw_sample = Some(encoded.to_vec());
      units.av1_sequence_header = find_av1_sequence_header_obu(encoded);
      return Ok(units);
    }

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

  fn can_initialize_session(&self, codec: VideoCodecId) -> bool {
    match codec {
      VideoCodecId::Av1 => self.av1_sequence_header.is_some(),
      VideoCodecId::H264 => self.h264_sps.is_some() && self.h264_pps.is_some(),
      VideoCodecId::H265 => self.h265_vps.is_some() && self.h265_sps.is_some() && self.h265_pps.is_some(),
      VideoCodecId::Unknown => false,
    }
  }

  fn parameter_set_summary(&self) -> String {
    format!(
      "av1_raw={} h264_sps={} h264_pps={} h265_vps={} h265_sps={} h265_pps={} nal_count={}",
      self.raw_sample.is_some(),
      self.h264_sps.is_some(),
      self.h264_pps.is_some(),
      self.h265_vps.is_some(),
      self.h265_sps.is_some(),
      self.h265_pps.is_some(),
      self.nals.len()
    )
  }

  fn sample_data(&self, codec: VideoCodecId) -> Result<Vec<u8>, VideoError> {
    if codec == VideoCodecId::Av1 {
      return self
        .raw_sample
        .clone()
        .ok_or_else(|| VideoError::new("Encoded AV1 frame is empty."));
    }

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

fn picture_to_nv12(picture: &Dav1dPicture, output_buffer: Option<Vec<u8>>) -> Result<Vec<u8>, VideoError> {
  if picture.p.bpc != 8 {
    return Err(VideoError::new(format!(
      "rav1d returned unsupported AV1 bit depth {}; only 8-bit output is wired.",
      picture.p.bpc
    )));
  }

  let width = usize::try_from(picture.p.w).map_err(|_| VideoError::new("rav1d returned invalid AV1 width."))?;
  let height = usize::try_from(picture.p.h).map_err(|_| VideoError::new("rav1d returned invalid AV1 height."))?;
  if width == 0 || height == 0 {
    return Err(VideoError::new("rav1d returned an empty AV1 picture."));
  }
  if width % 2 != 0 || height % 2 != 0 {
    return Err(VideoError::new(format!(
      "rav1d returned AV1 dimensions {width}x{height}; NV12 output requires even dimensions."
    )));
  }

  let y_ptr = picture.data[0].ok_or_else(|| VideoError::new("rav1d AV1 picture is missing the Y plane."))?;
  let chroma = chroma_layout(picture.p.layout)?;
  let stride_y =
    usize::try_from(picture.stride[0]).map_err(|_| VideoError::new("rav1d returned unsupported negative Y stride."))?;
  let y_plane = unsafe { slice::from_raw_parts(y_ptr.as_ptr().cast::<u8>(), stride_y * height) };

  let mut output = output_buffer.unwrap_or_default();
  let y_len = width
    .checked_mul(height)
    .ok_or_else(|| VideoError::new("AV1 NV12 output buffer size overflow."))?;
  let nv12_len = y_len
    .checked_add(y_len / 2)
    .ok_or_else(|| VideoError::new("AV1 NV12 output buffer size overflow."))?;
  output.resize(nv12_len, 128);

  for y in 0..height {
    let src_start = y * stride_y;
    let dst_start = y * width;
    output[dst_start..dst_start + width].copy_from_slice(&y_plane[src_start..src_start + width]);
  }

  match chroma {
    ChromaLayout::Monochrome => {}
    ChromaLayout::Yuv {
      width_shift,
      height_shift,
    } => {
      let u_ptr = picture.data[1].ok_or_else(|| VideoError::new("rav1d AV1 picture is missing the U plane."))?;
      let v_ptr = picture.data[2].ok_or_else(|| VideoError::new("rav1d AV1 picture is missing the V plane."))?;
      let stride_uv = usize::try_from(picture.stride[1])
        .map_err(|_| VideoError::new("rav1d returned unsupported negative UV stride."))?;
      let uv_height = (height + (1 << height_shift) - 1) >> height_shift;
      let u_plane = unsafe { slice::from_raw_parts(u_ptr.as_ptr().cast::<u8>(), stride_uv * uv_height) };
      let v_plane = unsafe { slice::from_raw_parts(v_ptr.as_ptr().cast::<u8>(), stride_uv * uv_height) };
      yuv_to_nv12_chroma(
        u_plane,
        v_plane,
        stride_uv,
        width,
        height,
        width_shift,
        height_shift,
        &mut output[y_len..],
      );
    }
  }

  Ok(output)
}

enum ChromaLayout {
  Monochrome,
  Yuv { width_shift: usize, height_shift: usize },
}

fn chroma_layout(layout: u32) -> Result<ChromaLayout, VideoError> {
  match layout {
    DAV1D_PIXEL_LAYOUT_I400 => Ok(ChromaLayout::Monochrome),
    DAV1D_PIXEL_LAYOUT_I420 => Ok(ChromaLayout::Yuv {
      width_shift: 1,
      height_shift: 1,
    }),
    DAV1D_PIXEL_LAYOUT_I422 => Ok(ChromaLayout::Yuv {
      width_shift: 1,
      height_shift: 0,
    }),
    DAV1D_PIXEL_LAYOUT_I444 => Ok(ChromaLayout::Yuv {
      width_shift: 0,
      height_shift: 0,
    }),
    _ => Err(VideoError::new(format!(
      "rav1d returned unsupported AV1 pixel layout {layout}."
    ))),
  }
}

#[allow(clippy::too_many_arguments)]
fn yuv_to_nv12_chroma(
  u_plane: &[u8],
  v_plane: &[u8],
  stride_uv: usize,
  width: usize,
  height: usize,
  width_shift: usize,
  height_shift: usize,
  uv_output: &mut [u8],
) {
  for y in (0..height).step_by(2) {
    let uv_row = (y / 2) * width;
    let source_y = y >> height_shift;
    for x in (0..width).step_by(2) {
      let source_x = x >> width_shift;
      let offset = uv_row + x;
      uv_output[offset] = u_plane[source_y * stride_uv + source_x];
      uv_output[offset + 1] = v_plane[source_y * stride_uv + source_x];
    }
  }
}

fn dav1d_result(result: Dav1dResult, action: &str) -> Result<(), VideoError> {
  if result.0 == 0 {
    Ok(())
  } else {
    Err(VideoError::new(format!(
      "Failed to {action}: {}.",
      dav1d_error_label(result)
    )))
  }
}

fn dav1d_error_label(result: Dav1dResult) -> String {
  match result.0 {
    DAV1D_EAGAIN => "EAGAIN".to_string(),
    code => format!("Dav1dResult {code}"),
  }
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
