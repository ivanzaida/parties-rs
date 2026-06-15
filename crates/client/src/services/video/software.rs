#[cfg(any(target_os = "windows", target_os = "macos"))]
use std::ptr::NonNull;
use std::{
  borrow::Cow,
  ops::Range,
  ptr, slice,
  time::{Duration, Instant},
};

use openh264_sys2::API as _;
use shiguredo_dav1d::{self as dav1d_native, DecoderConfig};

use super::{
  DecodedVideoPixelFormat, NativeDecodedVideoFrame, NativeVideoBackend, VideoDecodeConfig, VideoError,
  VideoFrameDecoder,
};
use crate::network::protocol::{VideoCodecId, VideoFrame};

const SLOW_SOFTWARE_DECODE_LOG_THRESHOLD: Duration = Duration::from_millis(100);
const AV1_DECODE_SUBMIT_RETRIES: usize = 3;
const H264_NONFATAL_DECODE_LOG_INTERVAL: Duration = Duration::from_secs(1);
const H264_OPENH264_MAX_LEVEL_IDC: u8 = 52;
pub(super) enum SoftwareVideoDecoder {
  Av1(Av1SoftwareDecoder),
  H265(H265SoftwareDecoder),
  H264(H264SoftwareDecoder),
}

pub(super) struct Av1SoftwareDecoder {
  decoder: dav1d_native::Decoder,
  width: u16,
  height: u16,
  threads: usize,
  max_frame_delay: usize,
}

pub(super) struct H265SoftwareDecoder {
  #[cfg(any(target_os = "windows", target_os = "macos"))]
  decoder: NativeLibhevcDecoder,
  width: u16,
  height: u16,
  threads: u32,
}

pub(super) struct H264SoftwareDecoder {
  decoder: OpenH264RawDecoder,
  width: u16,
  height: u16,
  last_nonfatal_decode_log: Option<Instant>,
}

impl SoftwareVideoDecoder {
  pub(super) fn new(config: &VideoDecodeConfig) -> Result<Self, VideoError> {
    match config.codec {
      VideoCodecId::Av1 => Av1SoftwareDecoder::new(config).map(Self::Av1),
      VideoCodecId::H265 => H265SoftwareDecoder::new(config).map(Self::H265),
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
}

impl VideoFrameDecoder for SoftwareVideoDecoder {
  fn decode_frame(
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
    let threads = av1_decoder_threads(config);
    let max_frame_delay = av1_max_frame_delay(threads);
    let decoder_config = DecoderConfig {
      n_threads: threads,
      max_frame_delay,
      apply_grain: false,
      ..DecoderConfig::new()
    };
    let decoder = dav1d_native::Decoder::new(decoder_config)
      .map_err(|error| VideoError::new(format!("Failed to start native software AV1 decoder: {error}")))?;
    tracing::info!(target: "video::decode::software",
      "[video:decode/software] native software AV1 decoder started: size={}x{} threads={} max_frame_delay={} film_grain=false dav1d_version={}",
      config.width,
      config.height,
      threads,
      max_frame_delay,
      dav1d_native::version()
    );

    Ok(Self {
      decoder,
      width: config.width,
      height: config.height,
      threads,
      max_frame_delay,
    })
  }

  fn decode_frame(
    &mut self,
    frame: &VideoFrame,
    output: bool,
    output_buffer: Option<Vec<u8>>,
  ) -> Result<Option<NativeDecodedVideoFrame>, VideoError> {
    let total_start = Instant::now();
    let input_copy_elapsed = Duration::ZERO;
    let mut send_elapsed = Duration::ZERO;
    let mut get_picture_elapsed = Duration::ZERO;
    let mut convert_elapsed = Duration::ZERO;
    let mut pictures = 0usize;
    let mut latest: Option<NativeDecodedVideoFrame> = None;
    let mut reusable_output = output_buffer;

    let mut submitted = false;
    for _ in 0..AV1_DECODE_SUBMIT_RETRIES {
      let send_start = Instant::now();
      match self.decoder.decode(&frame.encoded) {
        Ok(()) => {
          send_elapsed += send_start.elapsed();
          submitted = true;
          break;
        }
        Err(error) if error.is_eagain() => {
          send_elapsed += send_start.elapsed();
          self.drain_available_frames(
            frame,
            output,
            &mut reusable_output,
            &mut latest,
            &mut pictures,
            &mut get_picture_elapsed,
            &mut convert_elapsed,
          )?;
        }
        Err(error) => {
          send_elapsed += send_start.elapsed();
          return Err(VideoError::new(format!(
            "Native software AV1 decoder failed to accept frame {}: {error}.",
            frame.frame_number
          )));
        }
      }
    }
    if !submitted {
      return Err(VideoError::new(format!(
        "Native software AV1 decoder could not accept frame {}: input queue stayed full.",
        frame.frame_number
      )));
    }
    self.drain_available_frames(
      frame,
      output,
      &mut reusable_output,
      &mut latest,
      &mut pictures,
      &mut get_picture_elapsed,
      &mut convert_elapsed,
    )?;

    log_slow_software_decode(SoftwareDecodeTiming {
      codec: VideoCodecId::Av1,
      frame,
      output,
      produced_frame: latest.is_some(),
      total_elapsed: total_start.elapsed(),
      input_copy_elapsed,
      parse_elapsed: Duration::ZERO,
      send_elapsed,
      codec_elapsed: get_picture_elapsed,
      convert_elapsed,
      units: pictures,
      unit_label: "pictures",
      av1_threads: Some(self.threads),
      av1_max_frame_delay: Some(self.max_frame_delay),
    });
    Ok(latest)
  }

  fn drain_available_frames(
    &mut self,
    input_frame: &VideoFrame,
    output: bool,
    reusable_output: &mut Option<Vec<u8>>,
    latest: &mut Option<NativeDecodedVideoFrame>,
    pictures: &mut usize,
    get_picture_elapsed: &mut Duration,
    convert_elapsed: &mut Duration,
  ) -> Result<(), VideoError> {
    loop {
      let get_picture_start = Instant::now();
      let decoded = self.decoder.next_frame().map_err(|error| {
        VideoError::new(format!(
          "Native software AV1 decoder failed to produce output for frame {}: {error}.",
          input_frame.frame_number
        ))
      })?;
      *get_picture_elapsed += get_picture_start.elapsed();
      let Some(decoded) = decoded else {
        break;
      };

      *pictures += 1;
      if output {
        *reusable_output = latest.take().map(|frame| frame.pixels).or(reusable_output.take());
        let convert_start = Instant::now();
        let converted = dav1d_frame_to_nv12(&decoded, self.width, self.height, reusable_output.take());
        *convert_elapsed += convert_start.elapsed();
        *latest = Some(converted?);
      }
    }
    Ok(())
  }
}

fn av1_decoder_threads(config: &VideoDecodeConfig) -> usize {
  let available_threads = std::thread::available_parallelism().map_or(4, |threads| threads.get());
  let pixels = usize::from(config.width) * usize::from(config.height);
  let target_threads = if pixels >= 3840 * 2160 {
    available_threads
  } else if pixels >= 1920 * 1080 {
    available_threads.min(8).max(4)
  } else {
    available_threads.min(4).max(2)
  };
  target_threads.clamp(2, 16)
}

fn av1_max_frame_delay(threads: usize) -> usize {
  let _ = threads;
  1
}

impl H265SoftwareDecoder {
  fn new(config: &VideoDecodeConfig) -> Result<Self, VideoError> {
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    {
      let threads = h265_decoder_threads(config);
      let decoder = NativeLibhevcDecoder::new(config.width, config.height, threads)?;
      tracing::info!(target: "video::decode::software",
        "[video:decode/software] native software H.265 decoder started: size={}x{} threads={} backend={} sao=codec-default deblocking=codec-default",
        config.width,
        config.height,
        threads,
        NativeLibhevcDecoder::version()
      );
      return Ok(Self {
        decoder,
        width: config.width,
        height: config.height,
        threads,
      });
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
      let _ = config;
      Err(VideoError::new(
        "Software H.265 decoding is only implemented through native libhevc on Windows and macOS.",
      ))
    }
  }

  fn decode_frame(
    &mut self,
    frame: &VideoFrame,
    output: bool,
    output_buffer: Option<Vec<u8>>,
  ) -> Result<Option<NativeDecodedVideoFrame>, VideoError> {
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    {
      return self.decode_frame_libhevc(frame, output, output_buffer);
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
      let _ = (frame, output, output_buffer);
      Err(VideoError::new(
        "Software H.265 decoding is only implemented through native libhevc on Windows and macOS.",
      ))
    }
  }

  #[cfg(any(target_os = "windows", target_os = "macos"))]
  fn decode_frame_libhevc(
    &mut self,
    frame: &VideoFrame,
    output: bool,
    output_buffer: Option<Vec<u8>>,
  ) -> Result<Option<NativeDecodedVideoFrame>, VideoError> {
    let total_start = Instant::now();
    let parse_start = Instant::now();
    let nals = h265_annex_b_nal_count(&frame.encoded);
    let parse_elapsed = parse_start.elapsed();
    if nals == 0 {
      return Err(VideoError::new("Software H.265 decoder expected Annex B NAL units."));
    }

    let mut convert_elapsed = Duration::ZERO;
    let mut output_pixels = if output {
      let convert_start = Instant::now();
      let len = nv12_len_usize(usize::from(self.width), usize::from(self.height))?;
      let buffer = full_overwrite_buffer(output_buffer, len);
      convert_elapsed += convert_start.elapsed();
      Some(buffer)
    } else {
      None
    };

    let codec_start = Instant::now();
    let produced = self.decoder.decode(
      frame,
      output,
      output_pixels.as_mut().map(Vec::as_mut_slice),
      self.width,
      self.height,
      self.threads,
    )?;
    let codec_elapsed = codec_start.elapsed();

    let latest = if produced && output {
      output_pixels.map(|pixels| NativeDecodedVideoFrame {
        format: DecodedVideoPixelFormat::Nv12,
        pixels,
        native_image: None,
      })
    } else {
      None
    };

    log_slow_software_decode(SoftwareDecodeTiming {
      codec: VideoCodecId::H265,
      frame,
      output,
      produced_frame: latest.is_some(),
      total_elapsed: total_start.elapsed(),
      input_copy_elapsed: Duration::ZERO,
      parse_elapsed,
      send_elapsed: Duration::ZERO,
      codec_elapsed,
      convert_elapsed,
      units: nals,
      unit_label: "nals",
      av1_threads: None,
      av1_max_frame_delay: None,
    });
    Ok(latest)
  }
}

fn h265_decoder_threads(config: &VideoDecodeConfig) -> u32 {
  let available_threads = std::thread::available_parallelism().map_or(4, |threads| threads.get());
  let pixels = usize::from(config.width) * usize::from(config.height);
  #[cfg(any(target_os = "windows", target_os = "macos"))]
  {
    let _ = pixels;
    return available_threads.clamp(1, 4) as u32;
  }
  #[cfg(not(any(target_os = "windows", target_os = "macos")))]
  {
    let target_threads = if pixels >= 3840 * 2160 {
      available_threads
    } else if pixels >= 1920 * 1080 {
      available_threads.min(8).max(4)
    } else {
      available_threads.min(4).max(2)
    };
    target_threads.clamp(2, 16) as u32
  }
}

#[cfg(any(target_os = "windows", target_os = "macos"))]
#[repr(C)]
struct PartiesLibhevcDecoder {
  _private: [u8; 0],
}

#[cfg(any(target_os = "windows", target_os = "macos"))]
unsafe extern "C" {
  fn parties_libhevc_decoder_create(width: u32, height: u32, threads: u32) -> *mut PartiesLibhevcDecoder;
  fn parties_libhevc_decoder_destroy(decoder: *mut PartiesLibhevcDecoder);
  fn parties_libhevc_decoder_decode(
    decoder: *mut PartiesLibhevcDecoder,
    data: *const u8,
    len: usize,
    timestamp: i64,
    output_requested: i32,
    output: *mut u8,
    output_len: usize,
    width_out: *mut u32,
    height_out: *mut u32,
    threads: u32,
    error_out: *mut u32,
  ) -> i32;
  fn parties_libhevc_decoder_version() -> *const std::ffi::c_char;
}

#[cfg(any(target_os = "windows", target_os = "macos"))]
struct NativeLibhevcDecoder {
  handle: NonNull<PartiesLibhevcDecoder>,
}

#[cfg(any(target_os = "windows", target_os = "macos"))]
impl NativeLibhevcDecoder {
  fn new(width: u16, height: u16, threads: u32) -> Result<Self, VideoError> {
    let handle = unsafe { parties_libhevc_decoder_create(u32::from(width), u32::from(height), threads) };
    let handle = NonNull::new(handle).ok_or_else(|| VideoError::new("Failed to start native libhevc decoder."))?;
    Ok(Self { handle })
  }

  fn decode(
    &mut self,
    frame: &VideoFrame,
    output: bool,
    output_buffer: Option<&mut [u8]>,
    expected_width: u16,
    expected_height: u16,
    threads: u32,
  ) -> Result<bool, VideoError> {
    let mut width = 0u32;
    let mut height = 0u32;
    let mut error = 0u32;
    let (output_ptr, output_len) = output_buffer
      .map(|buffer| (buffer.as_mut_ptr(), buffer.len()))
      .unwrap_or((ptr::null_mut(), 0));
    let status = unsafe {
      parties_libhevc_decoder_decode(
        self.handle.as_ptr(),
        frame.encoded.as_ptr(),
        frame.encoded.len(),
        i64::from(frame.frame_number),
        i32::from(output),
        output_ptr,
        output_len,
        &mut width,
        &mut height,
        threads,
        &mut error,
      )
    };
    if status < 0 {
      return Err(VideoError::new(format!(
        "Native libhevc decoder failed on frame {}: error=0x{error:08x}.",
        frame.frame_number
      )));
    }
    if status == 0 {
      return Ok(false);
    }
    if width != u32::from(expected_width) || height != u32::from(expected_height) {
      return Err(VideoError::new(format!(
        "Native libhevc decoder returned unexpected dimensions: got={}x{} expected={}x{}.",
        width, height, expected_width, expected_height
      )));
    }
    Ok(true)
  }

  fn version() -> String {
    let ptr = unsafe { parties_libhevc_decoder_version() };
    if ptr.is_null() {
      return "libhevc".to_owned();
    }
    unsafe { std::ffi::CStr::from_ptr(ptr) }.to_string_lossy().into_owned()
  }
}

#[cfg(any(target_os = "windows", target_os = "macos"))]
impl Drop for NativeLibhevcDecoder {
  fn drop(&mut self) {
    unsafe {
      parties_libhevc_decoder_destroy(self.handle.as_ptr());
    }
  }
}

fn h265_annex_b_nal_count(encoded: &[u8]) -> usize {
  split_annex_b_ranges(encoded).len()
}

fn split_annex_b_ranges(encoded: &[u8]) -> Vec<std::ops::Range<usize>> {
  let mut ranges = Vec::new();
  let mut cursor = 0;
  let mut current_start = None;
  while let Some((prefix_start, prefix_len)) = find_annex_b_start_code(&encoded[cursor..]) {
    let prefix_start = cursor + prefix_start;
    if let Some(start) = current_start
      && start < prefix_start
    {
      ranges.push(start..prefix_start);
    }
    current_start = Some(prefix_start + prefix_len);
    cursor = prefix_start + prefix_len;
  }
  if let Some(start) = current_start
    && start < encoded.len()
  {
    ranges.push(start..encoded.len());
  }
  ranges
}

fn find_annex_b_start_code(data: &[u8]) -> Option<(usize, usize)> {
  let mut offset = 0;
  while offset + 3 <= data.len() {
    if data[offset..].starts_with(&[0, 0, 1]) {
      return Some((offset, 3));
    }
    if offset + 4 <= data.len() && data[offset..].starts_with(&[0, 0, 0, 1]) {
      return Some((offset, 4));
    }
    offset += 1;
  }
  None
}

struct OpenH264RawDecoder {
  api: openh264_sys2::DynamicAPI,
  decoder: *mut openh264_sys2::ISVCDecoder,
  threads: usize,
}

struct OpenH264DecodeOutcome {
  frame: Option<OpenH264DecodedFrame>,
  state: openh264_sys2::DECODING_STATE,
}

struct OpenH264DecodedFrame {
  y: *const u8,
  u: *const u8,
  v: *const u8,
  width: usize,
  height: usize,
  y_stride: usize,
  uv_stride: usize,
}

impl OpenH264RawDecoder {
  fn new() -> Result<Self, VideoError> {
    let api = openh264_sys2::DynamicAPI::from_source();
    let mut decoder = ptr::null_mut();
    let create_status = unsafe { api.WelsCreateDecoder(&mut decoder) };
    if create_status != 0 || decoder.is_null() {
      return Err(VideoError::new(format!(
        "WelsCreateDecoder failed: status={create_status} decoder_null={}.",
        decoder.is_null()
      )));
    }

    let decoder = Self {
      api,
      decoder,
      threads: 1,
    };
    let mut params = openh264_sys2::SDecodingParam::default();
    params.uiCpuLoad = 100;
    params.sVideoProperty.eVideoBsType = openh264_sys2::VIDEO_BITSTREAM_AVC;
    params.eEcActiveIdc = openh264_sys2::ERROR_CON_SLICE_COPY;

    let initialize = unsafe { (**decoder.decoder).Initialize }
      .ok_or_else(|| VideoError::new("OpenH264 decoder is missing Initialize."))?;
    let init_status = unsafe { initialize(decoder.decoder, &params) };
    if init_status != 0 {
      return Err(VideoError::new(format!(
        "OpenH264 Initialize failed: status={init_status}."
      )));
    }
    Ok(decoder)
  }

  fn decode_access_unit(
    &mut self,
    input: &H264AnnexBInput<'_>,
    frame_number: u32,
  ) -> Result<OpenH264DecodeOutcome, VideoError> {
    let mut latest_frame = None;
    let mut latest_state = openh264_sys2::dsErrorFree;
    let mut single_nal = Vec::new();
    for range in &input.ranges {
      single_nal.clear();
      single_nal.extend_from_slice(&[0, 0, 0, 1]);
      single_nal.extend_from_slice(&input.data[range.clone()]);
      let outcome = self.decode_nal(&single_nal, frame_number)?;
      latest_state = outcome.state;
      if outcome.frame.is_some() {
        latest_frame = outcome.frame;
      }
    }
    Ok(OpenH264DecodeOutcome {
      frame: latest_frame,
      state: latest_state,
    })
  }

  fn decode_nal(&mut self, data: &[u8], frame_number: u32) -> Result<OpenH264DecodeOutcome, VideoError> {
    let input_len =
      i32::try_from(data.len()).map_err(|_| VideoError::new("OpenH264 input packet is too large for c_int length."))?;
    let decode_frame = unsafe { (**self.decoder).DecodeFrameNoDelay }
      .ok_or_else(|| VideoError::new("OpenH264 decoder is missing DecodeFrameNoDelay."))?;
    let mut dst = [ptr::null_mut(); 3];
    let mut buffer_info = openh264_sys2::SBufferInfo::default();
    let state = unsafe {
      decode_frame(
        self.decoder,
        data.as_ptr(),
        input_len,
        dst.as_mut_ptr(),
        &mut buffer_info,
      )
    };

    if !h264_decode_state_is_recoverable(state) {
      return Err(VideoError::new(format!(
        "OpenH264 failed on frame {frame_number}: state={state} state_label={}.",
        h264_decode_state_label(state)
      )));
    }
    if buffer_info.iBufferStatus != 1 || dst[0].is_null() {
      return Ok(OpenH264DecodeOutcome { frame: None, state });
    }

    let system_buffer = unsafe { buffer_info.UsrData.sSystemBuffer };
    if system_buffer.iFormat != openh264_sys2::videoFormatI420 {
      return Err(VideoError::new(format!(
        "OpenH264 returned unsupported pixel format {}.",
        system_buffer.iFormat
      )));
    }
    let width = usize::try_from(system_buffer.iWidth)
      .map_err(|_| VideoError::new("OpenH264 returned a negative output width."))?;
    let height = usize::try_from(system_buffer.iHeight)
      .map_err(|_| VideoError::new("OpenH264 returned a negative output height."))?;
    let y_stride = usize::try_from(system_buffer.iStride[0])
      .map_err(|_| VideoError::new("OpenH264 returned a negative Y stride."))?;
    let uv_stride = usize::try_from(system_buffer.iStride[1])
      .map_err(|_| VideoError::new("OpenH264 returned a negative UV stride."))?;
    if width == 0 || height == 0 || y_stride < width || uv_stride < width / 2 {
      return Err(VideoError::new(format!(
        "OpenH264 returned invalid output geometry: size={}x{} strides=({}, {}).",
        width, height, y_stride, uv_stride
      )));
    }
    Ok(OpenH264DecodeOutcome {
      frame: Some(OpenH264DecodedFrame {
        y: dst[0],
        u: dst[1],
        v: dst[2],
        width,
        height,
        y_stride,
        uv_stride,
      }),
      state,
    })
  }
}

impl Drop for OpenH264RawDecoder {
  fn drop(&mut self) {
    if self.decoder.is_null() {
      return;
    }
    unsafe {
      if let Some(uninitialize) = (**self.decoder).Uninitialize {
        let _ = uninitialize(self.decoder);
      }
      self.api.WelsDestroyDecoder(self.decoder);
    }
    self.decoder = ptr::null_mut();
  }
}

#[derive(Clone, Copy)]
struct H264AccessUnitSummary {
  nals: usize,
  sps: usize,
  pps: usize,
  idr: usize,
  sps_profile: Option<u8>,
  sps_level: Option<u8>,
  sps_level_clamped_from: Option<u8>,
  length_prefixed: bool,
}

struct H264AnnexBInput<'a> {
  data: Cow<'a, [u8]>,
  ranges: Vec<Range<usize>>,
  summary: H264AccessUnitSummary,
}

fn h264_annex_b_decode_input(encoded: &[u8]) -> Result<H264AnnexBInput<'_>, VideoError> {
  if encoded.is_empty() {
    return Err(VideoError::new("Software H.264 decoder received an empty frame."));
  }

  if looks_like_annex_b(encoded) {
    let ranges = split_annex_b_ranges(encoded);
    if ranges.is_empty() {
      return Err(VideoError::new("Software H.264 decoder expected Annex B NAL units."));
    }
    let mut summary = summarize_h264_nals(encoded, &ranges, false);
    let data = h264_clamp_sps_level_for_openh264(Cow::Borrowed(encoded), &ranges, &mut summary);
    return Ok(H264AnnexBInput { data, ranges, summary });
  }

  let ranges = split_length_prefixed_ranges(encoded)?;
  if ranges.is_empty() {
    return Err(VideoError::new(
      "Software H.264 decoder expected length-prefixed NAL units.",
    ));
  }
  let output_len = ranges.iter().try_fold(0usize, |total, range| {
    total
      .checked_add(4)
      .and_then(|value| value.checked_add(range.end.saturating_sub(range.start)))
      .ok_or_else(|| VideoError::new("Software H.264 Annex B conversion overflowed."))
  })?;
  let mut output = Vec::with_capacity(output_len);
  let mut output_ranges = Vec::with_capacity(ranges.len());
  for range in ranges {
    output.extend_from_slice(&[0, 0, 0, 1]);
    let start = output.len();
    output.extend_from_slice(&encoded[range]);
    output_ranges.push(start..output.len());
  }
  let mut summary = summarize_h264_nals(&output, &output_ranges, true);
  let data = h264_clamp_sps_level_for_openh264(Cow::Owned(output), &output_ranges, &mut summary);
  Ok(H264AnnexBInput {
    data,
    ranges: output_ranges,
    summary,
  })
}

fn looks_like_annex_b(bytes: &[u8]) -> bool {
  bytes.starts_with(&[0, 0, 1]) || bytes.starts_with(&[0, 0, 0, 1])
}

fn split_length_prefixed_ranges(bytes: &[u8]) -> Result<Vec<Range<usize>>, VideoError> {
  let mut out = Vec::new();
  let mut cursor = 0;
  while cursor + 4 <= bytes.len() {
    let len = u32::from_be_bytes(bytes[cursor..cursor + 4].try_into().unwrap()) as usize;
    cursor += 4;
    if len == 0 || cursor + len > bytes.len() {
      return Err(VideoError::new("Invalid length-prefixed H.264 NAL unit."));
    }
    out.push(cursor..cursor + len);
    cursor += len;
  }
  if cursor != bytes.len() {
    return Err(VideoError::new("Trailing bytes after length-prefixed H.264 NAL units."));
  }
  Ok(out)
}

fn summarize_h264_nals(encoded: &[u8], ranges: &[Range<usize>], length_prefixed: bool) -> H264AccessUnitSummary {
  let mut summary = H264AccessUnitSummary {
    nals: 0,
    sps: 0,
    pps: 0,
    idr: 0,
    sps_profile: None,
    sps_level: None,
    sps_level_clamped_from: None,
    length_prefixed,
  };
  for range in ranges {
    let Some(header) = encoded.get(range.start) else {
      continue;
    };
    summary.nals += 1;
    match header & 0x1f {
      5 => summary.idr += 1,
      7 => {
        summary.sps += 1;
        if summary.sps_profile.is_none() && range.start + 3 < range.end {
          summary.sps_profile = Some(encoded[range.start + 1]);
          summary.sps_level = Some(encoded[range.start + 3]);
        }
      }
      8 => summary.pps += 1,
      _ => {}
    }
  }
  summary
}

fn h264_clamp_sps_level_for_openh264<'a>(
  data: Cow<'a, [u8]>,
  ranges: &[Range<usize>],
  summary: &mut H264AccessUnitSummary,
) -> Cow<'a, [u8]> {
  let Some(level) = summary.sps_level else {
    return data;
  };
  if level <= H264_OPENH264_MAX_LEVEL_IDC {
    return data;
  }
  let Some(level_index) = ranges.iter().find_map(|range| {
    let nal_type = data.get(range.start)? & 0x1f;
    if nal_type == 7 && range.start + 3 < range.end {
      Some(range.start + 3)
    } else {
      None
    }
  }) else {
    return data;
  };

  let mut owned = data.into_owned();
  owned[level_index] = H264_OPENH264_MAX_LEVEL_IDC;
  summary.sps_level_clamped_from = Some(level);
  summary.sps_level = Some(H264_OPENH264_MAX_LEVEL_IDC);
  Cow::Owned(owned)
}

fn h264_decode_state_is_recoverable(state: openh264_sys2::DECODING_STATE) -> bool {
  state == openh264_sys2::dsErrorFree
    || state == openh264_sys2::dsFramePending
    || state & openh264_sys2::dsBitstreamError != 0
    || state & openh264_sys2::dsNoParamSets != 0
    || state & openh264_sys2::dsDataErrorConcealed != 0
    || state & openh264_sys2::dsRefLost != 0
    || state & openh264_sys2::dsDepLayerLost != 0
}

fn h264_decode_state_needs_log(state: openh264_sys2::DECODING_STATE) -> bool {
  state != openh264_sys2::dsErrorFree
    && state != openh264_sys2::dsFramePending
    && h264_decode_state_is_recoverable(state)
}

fn h264_decode_state_label(state: openh264_sys2::DECODING_STATE) -> &'static str {
  if state == openh264_sys2::dsErrorFree {
    "ok"
  } else if state == openh264_sys2::dsFramePending {
    "frame_pending"
  } else if state & openh264_sys2::dsNoParamSets != 0 {
    "no_parameter_sets"
  } else if state & openh264_sys2::dsBitstreamError != 0 {
    "bitstream_error"
  } else if state & openh264_sys2::dsDataErrorConcealed != 0 {
    "data_error_concealed"
  } else if state & openh264_sys2::dsRefLost != 0 {
    "reference_lost"
  } else if state & openh264_sys2::dsDepLayerLost != 0 {
    "dependency_layer_lost"
  } else if state & openh264_sys2::dsInvalidArgument != 0 {
    "invalid_argument"
  } else if state & openh264_sys2::dsInitialOptExpected != 0 {
    "initial_option_expected"
  } else if state & openh264_sys2::dsOutOfMemory != 0 {
    "out_of_memory"
  } else if state & openh264_sys2::dsDstBufNeedExpan != 0 {
    "destination_buffer_too_small"
  } else {
    "unknown"
  }
}

impl H264SoftwareDecoder {
  fn new(config: &VideoDecodeConfig) -> Result<Self, VideoError> {
    let decoder = OpenH264RawDecoder::new()
      .map_err(|error| VideoError::new(format!("Failed to start native software H.264 decoder: {error}")))?;
    let threads = decoder.threads;
    tracing::info!(target: "video::decode::software",
      "[video:decode/software] native software H.264 decoder started: size={}x{} backend=OpenH264 raw_api=true threads={}",
      config.width,
      config.height,
      threads
    );
    Ok(Self {
      decoder,
      width: config.width,
      height: config.height,
      last_nonfatal_decode_log: None,
    })
  }

  fn decode_frame(
    &mut self,
    frame: &VideoFrame,
    output: bool,
    output_buffer: Option<Vec<u8>>,
  ) -> Result<Option<NativeDecodedVideoFrame>, VideoError> {
    let total_start = Instant::now();
    let parse_start = Instant::now();
    let input = h264_annex_b_decode_input(&frame.encoded)?;
    let parse_elapsed = parse_start.elapsed();
    let codec_start = Instant::now();
    let decoded = self.decoder.decode_access_unit(&input, frame.frame_number)?;
    let codec_elapsed = codec_start.elapsed();
    if h264_decode_state_needs_log(decoded.state) {
      self.log_nonfatal_decode_state(frame, input.summary, decoded.state);
    }

    if !output {
      log_slow_software_decode(SoftwareDecodeTiming {
        codec: VideoCodecId::H264,
        frame,
        output,
        produced_frame: false,
        total_elapsed: total_start.elapsed(),
        input_copy_elapsed: Duration::ZERO,
        parse_elapsed,
        send_elapsed: Duration::ZERO,
        codec_elapsed,
        convert_elapsed: Duration::ZERO,
        units: input.summary.nals,
        unit_label: "nals",
        av1_threads: None,
        av1_max_frame_delay: None,
      });
      return Ok(None);
    }

    let mut convert_elapsed = Duration::ZERO;
    let result = if let Some(decoded_frame) = decoded.frame {
      let convert_start = Instant::now();
      let converted = h264_decoded_frame_to_nv12(&decoded_frame, self.width, self.height, output_buffer);
      convert_elapsed += convert_start.elapsed();
      converted.map(Some)
    } else {
      Ok(None)
    };
    let produced_frame = result.as_ref().is_ok_and(Option::is_some);
    log_slow_software_decode(SoftwareDecodeTiming {
      codec: VideoCodecId::H264,
      frame,
      output,
      produced_frame,
      total_elapsed: total_start.elapsed(),
      input_copy_elapsed: Duration::ZERO,
      parse_elapsed,
      send_elapsed: Duration::ZERO,
      codec_elapsed,
      convert_elapsed,
      units: input.summary.nals,
      unit_label: "nals",
      av1_threads: None,
      av1_max_frame_delay: None,
    });
    result
  }

  fn log_nonfatal_decode_state(&mut self, frame: &VideoFrame, summary: H264AccessUnitSummary, state: i32) {
    let now = Instant::now();
    if self
      .last_nonfatal_decode_log
      .is_some_and(|last| now.duration_since(last) < H264_NONFATAL_DECODE_LOG_INTERVAL)
    {
      return;
    }
    self.last_nonfatal_decode_log = Some(now);
    tracing::warn!(target: "video::decode::software",
      "[video:decode/software] OpenH264 skipped recoverable frame: frame={} keyframe={} state={} state_label={} bytes={} nals={} sps={} pps={} idr={} sps_profile={} sps_level={} sps_level_clamped_from={} length_prefixed={}",
      frame.frame_number,
      frame.keyframe,
      state,
      h264_decode_state_label(state),
      frame.encoded.len(),
      summary.nals,
      summary.sps,
      summary.pps,
      summary.idr,
      summary.sps_profile.map(i32::from).unwrap_or(-1),
      summary.sps_level.map(i32::from).unwrap_or(-1),
      summary.sps_level_clamped_from.map(i32::from).unwrap_or(-1),
      summary.length_prefixed
    );
  }
}

struct SoftwareDecodeTiming<'a> {
  codec: VideoCodecId,
  frame: &'a VideoFrame,
  output: bool,
  produced_frame: bool,
  total_elapsed: Duration,
  input_copy_elapsed: Duration,
  parse_elapsed: Duration,
  send_elapsed: Duration,
  codec_elapsed: Duration,
  convert_elapsed: Duration,
  units: usize,
  unit_label: &'static str,
  av1_threads: Option<usize>,
  av1_max_frame_delay: Option<usize>,
}

fn log_slow_software_decode(timing: SoftwareDecodeTiming<'_>) {
  if timing.total_elapsed < SLOW_SOFTWARE_DECODE_LOG_THRESHOLD {
    return;
  }

  tracing::warn!(target: "video::decode::software",
    "[video:decode/software] slow software decode detail: codec={:?} size={}x{} frame={} keyframe={} output={} produced_frame={} bytes={} {}={} av1_threads={} av1_max_frame_delay={} input_copy_ms={:.1} parse_ms={:.1} submit_ms={:.1} codec_ms={:.1} convert_ms={:.1} total_ms={:.1}",
    timing.codec,
    timing.frame.width,
    timing.frame.height,
    timing.frame.frame_number,
    timing.frame.keyframe,
    timing.output,
    timing.produced_frame,
    timing.frame.encoded.len(),
    timing.unit_label,
    timing.units,
    timing.av1_threads.unwrap_or(0),
    timing.av1_max_frame_delay.unwrap_or(0),
    duration_ms(timing.input_copy_elapsed),
    duration_ms(timing.parse_elapsed),
    duration_ms(timing.send_elapsed),
    duration_ms(timing.codec_elapsed),
    duration_ms(timing.convert_elapsed),
    duration_ms(timing.total_elapsed),
  );
}

fn duration_ms(duration: Duration) -> f64 {
  duration.as_secs_f64() * 1000.0
}

fn dav1d_frame_to_nv12(
  frame: &dav1d_native::DecodedFrame,
  width: u16,
  height: u16,
  output_buffer: Option<Vec<u8>>,
) -> Result<NativeDecodedVideoFrame, VideoError> {
  if frame.bit_depth() != 8 || frame.pixel_layout() != dav1d_native::PixelLayout::I420 {
    return Err(VideoError::new(format!(
      "Native software AV1 decoder returned unsupported pixel format: bpc={} layout={:?}.",
      frame.bit_depth(),
      frame.pixel_layout()
    )));
  }
  if frame.width() != usize::from(width) || frame.height() != usize::from(height) {
    return Err(VideoError::new(format!(
      "Native software AV1 decoder returned unexpected dimensions: got={}x{} expected={}x{}.",
      frame.width(),
      frame.height(),
      width,
      height
    )));
  }

  let width = usize::from(width);
  let height = usize::from(height);
  i420_planes_to_nv12(
    frame.y_plane(),
    frame.u_plane(),
    frame.v_plane(),
    width,
    height,
    (frame.y_stride(), frame.u_stride(), frame.v_stride()),
    output_buffer,
  )
}

fn h264_decoded_frame_to_nv12(
  decoded: &OpenH264DecodedFrame,
  width: u16,
  height: u16,
  output_buffer: Option<Vec<u8>>,
) -> Result<NativeDecodedVideoFrame, VideoError> {
  let width = usize::from(width);
  let height = usize::from(height);
  if decoded.width != width || decoded.height != height {
    return Err(VideoError::new(format!(
      "Software H.264 decoder returned unexpected dimensions: got={}x{} expected={}x{}.",
      decoded.width, decoded.height, width, height
    )));
  }
  if decoded.y.is_null() || decoded.u.is_null() || decoded.v.is_null() {
    return Err(VideoError::new("Software H.264 decoder returned a null output plane."));
  }

  let y_len = checked_plane_len(decoded.y_stride, height)?;
  let uv_len = checked_plane_len(decoded.uv_stride, height / 2)?;
  let y = unsafe { slice::from_raw_parts(decoded.y, y_len) };
  let u = unsafe { slice::from_raw_parts(decoded.u, uv_len) };
  let v = unsafe { slice::from_raw_parts(decoded.v, uv_len) };

  i420_planes_to_nv12(
    y,
    u,
    v,
    width,
    height,
    (decoded.y_stride, decoded.uv_stride, decoded.uv_stride),
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
    unsafe {
      ptr::copy_nonoverlapping(y.as_ptr().add(src), y_dst.as_mut_ptr().add(dst), width);
    }
  }

  for row in 0..height / 2 {
    let u_src = row * u_stride;
    let v_src = row * v_stride;
    let dst = row * width;
    unsafe {
      let u_row = u.as_ptr().add(u_src);
      let v_row = v.as_ptr().add(v_src);
      let uv_row = uv_dst.as_mut_ptr().add(dst);
      for column in 0..width / 2 {
        *uv_row.add(column * 2) = *u_row.add(column);
        *uv_row.add(column * 2 + 1) = *v_row.add(column);
      }
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

  #[test]
  fn h264_annex_b_input_clamps_high_sps_level_for_openh264() {
    let input = [0, 0, 0, 1, 0x67, 100, 0, 60, 1, 0, 0, 1, 0x68, 2, 0, 0, 1, 0x65, 3];
    let parsed = h264_annex_b_decode_input(&input).unwrap();

    assert!(matches!(parsed.data, Cow::Owned(_)));
    assert_eq!(parsed.data[7], 52);
    assert_eq!(parsed.summary.nals, 3);
    assert_eq!(parsed.summary.sps, 1);
    assert_eq!(parsed.summary.pps, 1);
    assert_eq!(parsed.summary.idr, 1);
    assert_eq!(parsed.summary.sps_profile, Some(100));
    assert_eq!(parsed.summary.sps_level, Some(52));
    assert_eq!(parsed.summary.sps_level_clamped_from, Some(60));
    assert!(!parsed.summary.length_prefixed);
    assert_eq!(parsed.ranges, [4..9, 12..14, 17..19]);
  }

  #[test]
  fn h264_length_prefixed_input_converts_to_annex_b() {
    let input = [0, 0, 0, 4, 0x67, 100, 0, 60, 0, 0, 0, 1, 0x68];
    let parsed = h264_annex_b_decode_input(&input).unwrap();

    assert_eq!(parsed.data.as_ref(), [0, 0, 0, 1, 0x67, 100, 0, 52, 0, 0, 0, 1, 0x68]);
    assert_eq!(parsed.summary.nals, 2);
    assert_eq!(parsed.summary.sps, 1);
    assert_eq!(parsed.summary.pps, 1);
    assert_eq!(parsed.summary.sps_profile, Some(100));
    assert_eq!(parsed.summary.sps_level, Some(52));
    assert_eq!(parsed.summary.sps_level_clamped_from, Some(60));
    assert!(parsed.summary.length_prefixed);
    assert_eq!(parsed.ranges, [4..8, 12..13]);
  }
}
