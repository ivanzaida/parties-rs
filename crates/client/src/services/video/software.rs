#[cfg(target_os = "windows")]
use std::ptr::NonNull;
use std::{
  ptr,
  time::{Duration, Instant},
};

use openh264::formats::YUVSource;
use shiguredo_dav1d::{self as dav1d_native, DecoderConfig};

use super::{DecodedVideoPixelFormat, NativeDecodedVideoFrame, NativeVideoBackend, VideoDecodeConfig, VideoError};
use crate::network::protocol::{VideoCodecId, VideoFrame};

const SLOW_SOFTWARE_DECODE_LOG_THRESHOLD: Duration = Duration::from_millis(100);
const AV1_DECODE_SUBMIT_RETRIES: usize = 3;
#[cfg(not(target_os = "windows"))]
const H265_DECODE_PASSES: usize = 8;

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
  #[cfg(target_os = "windows")]
  decoder: WindowsLibhevcDecoder,
  #[cfg(not(target_os = "windows"))]
  decoder: libde265::Decoder,
  width: u16,
  height: u16,
  threads: u32,
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
    #[cfg(target_os = "windows")]
    {
      let threads = h265_decoder_threads(config);
      let decoder = WindowsLibhevcDecoder::new(config.width, config.height, threads)?;
      tracing::info!(target: "video::decode::software",
        "[video:decode/software] native software H.265 decoder started: size={}x{} threads={} backend={} sao=codec-default deblocking=codec-default",
        config.width,
        config.height,
        threads,
        WindowsLibhevcDecoder::version()
      );
      return Ok(Self {
        decoder,
        width: config.width,
        height: config.height,
        threads,
      });
    }

    #[cfg(not(target_os = "windows"))]
    {
      let session = libde265::De265::new()
        .map_err(|error| VideoError::new(format!("Failed to initialize native software H.265 decoder: {error}")))?;
      session.disable_logging();
      let mut decoder = libde265::Decoder::new(session.clone());
      decoder.set_suppress_faulty_pictures(true);
      decoder.set_disable_sao(true);
      decoder.set_disable_deblocking(true);
      let threads = h265_decoder_threads(config);
      decoder.start_worker_threads(threads).map_err(|error| {
        VideoError::new(format!(
          "Failed to start native software H.265 decoder threads: {error}"
        ))
      })?;
      tracing::info!(target: "video::decode::software",
        "[video:decode/software] native software H.265 decoder started: size={}x{} threads={} sao=false deblocking=false libde265_version={}",
        config.width,
        config.height,
        threads,
        session.get_version()
      );
      Ok(Self {
        decoder,
        width: config.width,
        height: config.height,
        threads,
      })
    }
  }

  fn decode_frame(
    &mut self,
    frame: &VideoFrame,
    output: bool,
    output_buffer: Option<Vec<u8>>,
  ) -> Result<Option<NativeDecodedVideoFrame>, VideoError> {
    #[cfg(target_os = "windows")]
    {
      return self.decode_frame_libhevc(frame, output, output_buffer);
    }

    #[cfg(not(target_os = "windows"))]
    {
      let total_start = Instant::now();
      let parse_start = Instant::now();
      let nals = h265_annex_b_nal_count(&frame.encoded);
      let parse_elapsed = parse_start.elapsed();
      if nals == 0 {
        return Err(VideoError::new("Software H.265 decoder expected Annex B NAL units."));
      }

      let mut latest: Option<NativeDecodedVideoFrame> = None;
      let mut reusable_output = output_buffer;
      let mut codec_elapsed = Duration::ZERO;
      let mut convert_elapsed = Duration::ZERO;

      let codec_start = Instant::now();
      self
        .decoder
        .push_data(&frame.encoded, i64::from(frame.frame_number), None)
        .map_err(|error| {
          VideoError::new(format!(
            "Native software H.265 decoder failed to accept frame {}: {error}.",
            frame.frame_number
          ))
        })?;
      self.decoder.push_end_of_frame();
      for _ in 0..H265_DECODE_PASSES {
        match self.decoder.decode() {
          Ok(()) => {}
          Err(libde265::Error::ImageBufferFull) => {}
          Err(error) => {
            codec_elapsed += codec_start.elapsed();
            return Err(VideoError::new(format!(
              "Native software H.265 decoder failed on frame {}: {error}.",
              frame.frame_number
            )));
          }
        }

        while let Some(decoded) = self.decoder.peek_next_picture() {
          if output {
            reusable_output = latest.take().map(|frame| frame.pixels).or(reusable_output);
            let convert_start = Instant::now();
            let converted = h265_image_to_nv12(&decoded, self.width, self.height, reusable_output.take());
            convert_elapsed += convert_start.elapsed();
            latest = Some(converted?);
          }
          self.decoder.release_next_picture();
        }

        if self.decoder.get_number_of_input_bytes_pending() == 0 && self.decoder.get_number_of_nal_units_pending() == 0
        {
          break;
        }
      }
      codec_elapsed += codec_start.elapsed();

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

  #[cfg(target_os = "windows")]
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
  #[cfg(target_os = "windows")]
  {
    let _ = pixels;
    return available_threads.clamp(1, 4) as u32;
  }
  #[cfg(not(target_os = "windows"))]
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

#[cfg(target_os = "windows")]
#[repr(C)]
struct PartiesLibhevcDecoder {
  _private: [u8; 0],
}

#[cfg(target_os = "windows")]
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

#[cfg(target_os = "windows")]
struct WindowsLibhevcDecoder {
  handle: NonNull<PartiesLibhevcDecoder>,
}

#[cfg(target_os = "windows")]
impl WindowsLibhevcDecoder {
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

#[cfg(target_os = "windows")]
impl Drop for WindowsLibhevcDecoder {
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
    let total_start = Instant::now();
    let codec_start = Instant::now();
    let decoded = self.decoder.decode(&frame.encoded).map_err(|error| {
      VideoError::new(format!(
        "Software H.264 decoder failed on frame {}: {error}.",
        frame.frame_number
      ))
    })?;
    let codec_elapsed = codec_start.elapsed();

    if !output {
      log_slow_software_decode(SoftwareDecodeTiming {
        codec: VideoCodecId::H264,
        frame,
        output,
        produced_frame: false,
        total_elapsed: total_start.elapsed(),
        input_copy_elapsed: Duration::ZERO,
        parse_elapsed: Duration::ZERO,
        send_elapsed: Duration::ZERO,
        codec_elapsed,
        convert_elapsed: Duration::ZERO,
        units: usize::from(decoded.is_some()),
        unit_label: "pictures",
        av1_threads: None,
        av1_max_frame_delay: None,
      });
      return Ok(None);
    }

    let mut convert_elapsed = Duration::ZERO;
    let result = if let Some(decoded) = decoded {
      let convert_start = Instant::now();
      let converted = yuv_source_to_nv12(&decoded, self.width, self.height, output_buffer);
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
      parse_elapsed: Duration::ZERO,
      send_elapsed: Duration::ZERO,
      codec_elapsed,
      convert_elapsed,
      units: usize::from(produced_frame),
      unit_label: "pictures",
      av1_threads: None,
      av1_max_frame_delay: None,
    });
    result
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

#[cfg(not(target_os = "windows"))]
fn h265_image_to_nv12(
  image: &libde265::Image,
  width: u16,
  height: u16,
  output_buffer: Option<Vec<u8>>,
) -> Result<NativeDecodedVideoFrame, VideoError> {
  if image.get_bits_per_pixel(0) != 8 || image.get_bits_per_pixel(1) != 8 || image.get_bits_per_pixel(2) != 8 {
    return Err(VideoError::new(format!(
      "Native software H.265 decoder returned unsupported bit depth: y={} u={} v={}.",
      image.get_bits_per_pixel(0),
      image.get_bits_per_pixel(1),
      image.get_bits_per_pixel(2)
    )));
  }
  if image.get_image_width(0) != u32::from(width) || image.get_image_height(0) != u32::from(height) {
    return Err(VideoError::new(format!(
      "Native software H.265 decoder returned unexpected dimensions: got={}x{} expected={}x{}.",
      image.get_image_width(0),
      image.get_image_height(0),
      width,
      height
    )));
  }

  let (y, y_stride) = image.get_image_plane(0);
  let (u, u_stride) = image.get_image_plane(1);
  let (v, v_stride) = image.get_image_plane(2);
  let width = usize::from(width);
  let height = usize::from(height);

  i420_planes_to_nv12(y, u, v, width, height, (y_stride, u_stride, v_stride), output_buffer)
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
}
