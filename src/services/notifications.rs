use std::{io::Cursor, thread, time::Duration};

use cpal::{
  FromSample, Sample, SampleFormat,
  traits::{DeviceTrait, StreamTrait},
};
use minimp3::{Decoder, Error as Mp3Error, Frame};

use crate::{services::audio_devices, storage::AppSettings};

const JOIN_CHANNEL_MP3: &[u8] = include_bytes!("../../assets/audio/join_channel.mp3");
const LEAVE_CHANNEL_MP3: &[u8] = include_bytes!("../../assets/audio/leave_channel.mp3");
const NEW_MESSAGE_MP3: &[u8] = include_bytes!("../../assets/audio/new_message.mp3");
const USER_KICKED_MP3: &[u8] = include_bytes!("../../assets/audio/user_kicked.mp3");
const PLAYBACK_TAIL_MS: u64 = 80;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotificationSound {
  VoiceJoin,
  VoiceLeave,
  ChatMessage,
  UserKicked,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NotificationAudioSettings {
  pub output_device: String,
  pub volume: i32,
}

impl NotificationAudioSettings {
  pub fn from_app_settings(settings: &AppSettings) -> Self {
    Self {
      output_device: settings.audio_output_device.clone(),
      volume: settings.notification_volume,
    }
  }
}

pub fn play(sound: NotificationSound, settings: NotificationAudioSettings) {
  if settings.volume <= 0 {
    return;
  }

  let _ = thread::Builder::new()
    .name("parties-notification-sound".to_owned())
    .spawn(move || {
      if let Err(error) = play_blocking(sound, settings) {
        super::logger::log(&format!("[audio] notification sound unavailable: {error}"));
      }
    });
}

fn play_blocking(sound: NotificationSound, settings: NotificationAudioSettings) -> Result<(), String> {
  let decoded = decode_notification_sound(sound)?;
  let Some(device) = audio_devices::output_device(&settings.output_device) else {
    return Err("No output device available.".to_owned());
  };

  let supported_config = device
    .default_output_config()
    .map_err(|error| format!("Failed to read output config: {error}"))?;
  let sample_format = supported_config.sample_format();
  let config: cpal::StreamConfig = supported_config.into();
  let duration_ms = decoded.duration_ms();

  let stream = match sample_format {
    SampleFormat::F32 => build_notification_stream::<f32>(&device, config, decoded, settings.volume),
    SampleFormat::F64 => build_notification_stream::<f64>(&device, config, decoded, settings.volume),
    SampleFormat::I8 => build_notification_stream::<i8>(&device, config, decoded, settings.volume),
    SampleFormat::I16 => build_notification_stream::<i16>(&device, config, decoded, settings.volume),
    SampleFormat::I24 => build_notification_stream::<cpal::I24>(&device, config, decoded, settings.volume),
    SampleFormat::I32 => build_notification_stream::<i32>(&device, config, decoded, settings.volume),
    SampleFormat::I64 => build_notification_stream::<i64>(&device, config, decoded, settings.volume),
    SampleFormat::U8 => build_notification_stream::<u8>(&device, config, decoded, settings.volume),
    SampleFormat::U16 => build_notification_stream::<u16>(&device, config, decoded, settings.volume),
    SampleFormat::U24 => build_notification_stream::<cpal::U24>(&device, config, decoded, settings.volume),
    SampleFormat::U32 => build_notification_stream::<u32>(&device, config, decoded, settings.volume),
    SampleFormat::U64 => build_notification_stream::<u64>(&device, config, decoded, settings.volume),
    _ => Err("Unsupported output sample format.".to_owned()),
  }?;

  stream
    .play()
    .map_err(|error| format!("Failed to start output stream: {error}"))?;
  thread::sleep(Duration::from_millis(duration_ms + PLAYBACK_TAIL_MS));
  Ok(())
}

fn build_notification_stream<T>(
  device: &cpal::Device,
  config: cpal::StreamConfig,
  sound: DecodedNotificationSound,
  volume: i32,
) -> Result<cpal::Stream, String>
where
  T: cpal::SizedSample + FromSample<f32>,
{
  let mut state = NotificationRenderState::new(sound, config.channels, config.sample_rate, volume);
  device
    .build_output_stream::<T, _, _>(
      config,
      move |data, _| state.render(data),
      move |error| super::logger::log(&format!("[audio] notification output error: {error}")),
      None,
    )
    .map_err(|error| format!("Failed to build output stream: {error}"))
}

#[derive(Clone)]
struct DecodedNotificationSound {
  samples: Vec<f32>,
  channels: usize,
  sample_rate: u32,
}

impl DecodedNotificationSound {
  fn duration_ms(&self) -> u64 {
    if self.sample_rate == 0 || self.channels == 0 {
      return 0;
    }
    let frames = self.samples.len() / self.channels;
    ((frames as u64) * 1000 / u64::from(self.sample_rate)).max(1)
  }
}

fn decode_notification_sound(sound: NotificationSound) -> Result<DecodedNotificationSound, String> {
  let bytes = match sound {
    NotificationSound::VoiceJoin => JOIN_CHANNEL_MP3,
    NotificationSound::VoiceLeave => LEAVE_CHANNEL_MP3,
    NotificationSound::ChatMessage => NEW_MESSAGE_MP3,
    NotificationSound::UserKicked => USER_KICKED_MP3,
  };

  decode_mp3(bytes)
}

fn decode_mp3(bytes: &[u8]) -> Result<DecodedNotificationSound, String> {
  let mut decoder = Decoder::new(Cursor::new(bytes));
  let mut samples = Vec::new();
  let mut sample_rate = None;
  let mut channels = None;

  loop {
    match decoder.next_frame() {
      Ok(frame) => append_mp3_frame(&mut samples, &mut sample_rate, &mut channels, frame)?,
      Err(Mp3Error::Eof) => break,
      Err(error) => return Err(format!("Failed to decode notification MP3: {error}")),
    }
  }

  let sample_rate = sample_rate.ok_or_else(|| "Notification MP3 did not contain audio.".to_owned())?;
  let channels = channels.ok_or_else(|| "Notification MP3 did not contain audio.".to_owned())?;
  Ok(DecodedNotificationSound {
    samples,
    channels,
    sample_rate,
  })
}

fn append_mp3_frame(
  samples: &mut Vec<f32>,
  sample_rate: &mut Option<u32>,
  channels: &mut Option<usize>,
  frame: Frame,
) -> Result<(), String> {
  let frame_sample_rate =
    u32::try_from(frame.sample_rate).map_err(|_| "Notification MP3 has an invalid sample rate.".to_owned())?;
  if frame_sample_rate == 0 {
    return Err("Notification MP3 has an invalid sample rate.".to_owned());
  }
  if sample_rate.is_none() {
    *sample_rate = Some(frame_sample_rate);
  } else if *sample_rate != Some(frame_sample_rate) {
    return Err("Notification MP3 changes sample rate between frames.".to_owned());
  }

  let frame_channels = frame.channels.max(1);
  if channels.is_none() {
    *channels = Some(frame_channels);
  } else if *channels != Some(frame_channels) {
    return Err("Notification MP3 changes channel count between frames.".to_owned());
  }

  samples.extend(
    frame
      .data
      .iter()
      .map(|sample| (f32::from(*sample) / f32::from(i16::MAX)).clamp(-1.0, 1.0)),
  );
  Ok(())
}

struct NotificationRenderState {
  sound: DecodedNotificationSound,
  output_channels: usize,
  output_rate: u32,
  volume: f32,
  source_position: f64,
}

impl NotificationRenderState {
  fn new(sound: DecodedNotificationSound, channels: u16, output_rate: u32, volume: i32) -> Self {
    Self {
      sound,
      output_channels: usize::from(channels.max(1)),
      output_rate: output_rate.max(1),
      volume: (volume.clamp(0, 100) as f32) / 100.0,
      source_position: 0.0,
    }
  }

  fn render<T>(&mut self, data: &mut [T])
  where
    T: Sample + FromSample<f32>,
  {
    for frame in data.chunks_mut(self.output_channels) {
      for (output_channel, channel) in frame.iter_mut().enumerate() {
        *channel = self.next_sample(output_channel).to_sample::<T>();
      }
      self.advance_source_position();
    }
  }

  fn next_sample(&self, output_channel: usize) -> f32 {
    if self.sound.samples.is_empty() || self.sound.channels == 0 {
      return 0.0;
    }

    let index = self.source_position.floor() as usize;
    let frame_count = self.sound.samples.len() / self.sound.channels;
    if index >= frame_count {
      return 0.0;
    }

    let next_index = (index + 1).min(frame_count - 1);
    let fraction = (self.source_position - index as f64) as f32;

    if self.output_channels == 1 && self.sound.channels > 1 {
      return self.interpolated_mono(index, next_index, fraction) * self.volume;
    }

    let source_channel = if self.sound.channels == 1 {
      0
    } else {
      output_channel.min(self.sound.channels - 1)
    };
    let current = self.sound.samples[index * self.sound.channels + source_channel];
    let next = self.sound.samples[next_index * self.sound.channels + source_channel];
    (current + (next - current) * fraction) * self.volume
  }

  fn interpolated_mono(&self, index: usize, next_index: usize, fraction: f32) -> f32 {
    let current = self.frame_average(index);
    let next = self.frame_average(next_index);
    current + (next - current) * fraction
  }

  fn frame_average(&self, index: usize) -> f32 {
    let start = index * self.sound.channels;
    let end = start + self.sound.channels;
    self.sound.samples[start..end].iter().sum::<f32>() / self.sound.channels as f32
  }

  fn advance_source_position(&mut self) {
    self.source_position += f64::from(self.sound.sample_rate) / f64::from(self.output_rate);
  }
}
