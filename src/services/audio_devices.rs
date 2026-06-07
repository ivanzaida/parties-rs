use std::sync::{
  Arc,
  atomic::{AtomicU32, Ordering},
};

use cpal::{
  SampleFormat,
  traits::{DeviceTrait, HostTrait, StreamTrait},
};

pub fn input_device_names() -> Vec<String> {
  cpal::default_host()
    .input_devices()
    .map(device_names)
    .unwrap_or_default()
}

pub fn output_device_names() -> Vec<String> {
  cpal::default_host()
    .output_devices()
    .map(device_names)
    .unwrap_or_default()
}

pub struct InputLevelMeter {
  _stream: cpal::Stream,
  level_bits: Arc<AtomicU32>,
}

impl InputLevelMeter {
  pub fn level(&self) -> f32 {
    f32::from_bits(self.level_bits.load(Ordering::Relaxed))
  }
}

pub fn input_level_meter(selected_device: &str) -> Option<InputLevelMeter> {
  let device = input_device(selected_device)?;
  let supported_config = device.default_input_config().ok()?;
  let sample_format = supported_config.sample_format();
  let config = supported_config.config();
  let level_bits = Arc::new(AtomicU32::new(0.0_f32.to_bits()));
  let stream = match sample_format {
    SampleFormat::F32 => build_input_level_stream::<f32>(&device, config, level_bits.clone()),
    SampleFormat::F64 => build_input_level_stream::<f64>(&device, config, level_bits.clone()),
    SampleFormat::I8 => build_input_level_stream::<i8>(&device, config, level_bits.clone()),
    SampleFormat::I16 => build_input_level_stream::<i16>(&device, config, level_bits.clone()),
    SampleFormat::I24 => build_input_level_stream::<cpal::I24>(&device, config, level_bits.clone()),
    SampleFormat::I32 => build_input_level_stream::<i32>(&device, config, level_bits.clone()),
    SampleFormat::I64 => build_input_level_stream::<i64>(&device, config, level_bits.clone()),
    SampleFormat::U8 => build_input_level_stream::<u8>(&device, config, level_bits.clone()),
    SampleFormat::U16 => build_input_level_stream::<u16>(&device, config, level_bits.clone()),
    SampleFormat::U24 => build_input_level_stream::<cpal::U24>(&device, config, level_bits.clone()),
    SampleFormat::U32 => build_input_level_stream::<u32>(&device, config, level_bits.clone()),
    SampleFormat::U64 => build_input_level_stream::<u64>(&device, config, level_bits.clone()),
    _ => return None,
  }
  .ok()?;

  stream.play().ok()?;
  Some(InputLevelMeter {
    _stream: stream,
    level_bits,
  })
}

fn device_names(devices: impl Iterator<Item = cpal::Device>) -> Vec<String> {
  let mut names = devices
    .map(|device| device.to_string())
    .map(|name: String| name.trim().to_owned())
    .filter(|name| !name.is_empty())
    .collect::<Vec<_>>();

  names.sort();
  names.dedup();
  names
}

fn input_device(selected_device: &str) -> Option<cpal::Device> {
  let host = cpal::default_host();
  let selected_device = selected_device.trim();

  if !selected_device.is_empty()
    && let Ok(mut devices) = host.input_devices()
    && let Some(device) = devices.find(|device| device_name_matches(device, selected_device))
  {
    return Some(device);
  }

  host.default_input_device()
}

fn device_name_matches(device: &cpal::Device, selected_device: &str) -> bool {
  device.to_string() == selected_device
}

fn build_input_level_stream<T>(
  device: &cpal::Device,
  config: cpal::StreamConfig,
  level_bits: Arc<AtomicU32>,
) -> Result<cpal::Stream, cpal::Error>
where
  T: cpal::SizedSample,
  f32: cpal::FromSample<T>,
{
  let channels = usize::from(config.channels.max(1));
  let error_level_bits = level_bits.clone();

  device.build_input_stream::<T, _, _>(
    config,
    move |data, _| {
      let level = input_buffer_level(data, channels);
      level_bits.store(level.to_bits(), Ordering::Relaxed);
    },
    move |_| {
      error_level_bits.store(0.0_f32.to_bits(), Ordering::Relaxed);
    },
    None,
  )
}

fn input_buffer_level<T>(data: &[T], channels: usize) -> f32
where
  T: cpal::Sample,
  f32: cpal::FromSample<T>,
{
  let peak = data
    .chunks(channels)
    .flat_map(|frame| frame.iter())
    .map(|sample| sample.to_sample::<f32>().abs())
    .fold(0.0_f32, f32::max)
    .clamp(0.0, 1.0);

  dbfs_to_meter_level(peak)
}

fn dbfs_to_meter_level(amplitude: f32) -> f32 {
  if amplitude <= 0.000_001 {
    return 0.0;
  }

  ((20.0 * amplitude.log10() + 60.0) / 60.0).clamp(0.0, 1.0)
}
