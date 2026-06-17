use std::{
  env, fs,
  io::Cursor,
  path::{Path, PathBuf},
  thread,
  time::Duration,
};

use cpal::{
  FromSample, Sample, SampleFormat,
  traits::{DeviceTrait, StreamTrait},
};
use minimp3::{Decoder, Error as Mp3Error, Frame};

use crate::{services::audio_devices, storage::AppSettings};

const JOIN_CHANNEL_MP3: &[u8] = include_bytes!("../../assets/audio/join_channel.mp3");
const LEAVE_CHANNEL_MP3: &[u8] = include_bytes!("../../assets/audio/leave_channel.mp3");
const NEW_MESSAGE_MP3: &[u8] = include_bytes!("../../assets/audio/new_message.mp3");
const MENTION_MP3: &[u8] = include_bytes!("../../assets/audio/mention.mp3");
const USER_KICKED_MP3: &[u8] = include_bytes!("../../assets/audio/user_kicked.mp3");
const STREAM_STARTED_MP3: &[u8] = include_bytes!("../../assets/audio/stream_started.mp3");
const STREAM_ENDED_MP3: &[u8] = include_bytes!("../../assets/audio/stream_ended.mp3");
const CONNECTION_LOST_MP3: &[u8] = include_bytes!("../../assets/audio/connection_lost.mp3");
const MODERATION_ACTION_MP3: &[u8] = include_bytes!("../../assets/audio/moderation_action.mp3");
const PLAYBACK_TAIL_MS: u64 = 80;
pub const SOUND_CHOICE_DEFAULT: &str = "";
pub const SOUND_CHOICE_JOIN_CHANNEL: &str = "join_channel";
pub const SOUND_CHOICE_LEAVE_CHANNEL: &str = "leave_channel";
pub const SOUND_CHOICE_NEW_MESSAGE: &str = "new_message";
pub const SOUND_CHOICE_MENTION: &str = "mention";
pub const SOUND_CHOICE_USER_KICKED: &str = "user_kicked";
pub const SOUND_CHOICE_STREAM_STARTED: &str = "stream_started";
pub const SOUND_CHOICE_STREAM_ENDED: &str = "stream_ended";
pub const SOUND_CHOICE_CONNECTION_LOST: &str = "connection_lost";
pub const SOUND_CHOICE_MODERATION_ACTION: &str = "moderation_action";
pub const SOUND_CHOICE_CUSTOM: &str = "custom";
pub const OUTGOING_VOICE_JOIN_SOUND_KEY: &str = "outgoing_voice_join";
pub const OUTGOING_VOICE_JOIN_SOUND_FILE_NAME: &str = "outgoing_voice_join.mp3";

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotificationSound {
  VoiceJoin,
  VoiceLeave,
  ChatMessage,
  Mention,
  UserKicked,
  StreamStarted,
  StreamEnded,
  ConnectionLost,
  ModerationAction,
}

#[allow(dead_code)]
impl NotificationSound {
  pub const ALL: [Self; 9] = [
    Self::VoiceJoin,
    Self::VoiceLeave,
    Self::ChatMessage,
    Self::Mention,
    Self::UserKicked,
    Self::StreamStarted,
    Self::StreamEnded,
    Self::ConnectionLost,
    Self::ModerationAction,
  ];
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NotificationSoundAsset {
  pub sound: NotificationSound,
  pub file_name: &'static str,
  pub bytes: &'static [u8],
}

#[allow(dead_code)]
pub fn join_channel_sound() -> NotificationSoundAsset {
  resolve_sound(NotificationSound::VoiceJoin)
}

#[allow(dead_code)]
pub fn leave_channel_sound() -> NotificationSoundAsset {
  resolve_sound(NotificationSound::VoiceLeave)
}

#[allow(dead_code)]
pub fn new_message_sound() -> NotificationSoundAsset {
  resolve_sound(NotificationSound::ChatMessage)
}

#[allow(dead_code)]
pub fn mention_sound() -> NotificationSoundAsset {
  resolve_sound(NotificationSound::Mention)
}

#[allow(dead_code)]
pub fn user_kicked_sound() -> NotificationSoundAsset {
  resolve_sound(NotificationSound::UserKicked)
}

#[allow(dead_code)]
pub fn stream_started_sound() -> NotificationSoundAsset {
  resolve_sound(NotificationSound::StreamStarted)
}

#[allow(dead_code)]
pub fn stream_ended_sound() -> NotificationSoundAsset {
  resolve_sound(NotificationSound::StreamEnded)
}

#[allow(dead_code)]
pub fn connection_lost_sound() -> NotificationSoundAsset {
  resolve_sound(NotificationSound::ConnectionLost)
}

#[allow(dead_code)]
pub fn moderation_action_sound() -> NotificationSoundAsset {
  resolve_sound(NotificationSound::ModerationAction)
}

pub fn resolve_sound(sound: NotificationSound) -> NotificationSoundAsset {
  match sound {
    NotificationSound::VoiceJoin => NotificationSoundAsset {
      sound,
      file_name: "join_channel.mp3",
      bytes: JOIN_CHANNEL_MP3,
    },
    NotificationSound::VoiceLeave => NotificationSoundAsset {
      sound,
      file_name: "leave_channel.mp3",
      bytes: LEAVE_CHANNEL_MP3,
    },
    NotificationSound::ChatMessage => NotificationSoundAsset {
      sound,
      file_name: "new_message.mp3",
      bytes: NEW_MESSAGE_MP3,
    },
    NotificationSound::Mention => NotificationSoundAsset {
      sound,
      file_name: "mention.mp3",
      bytes: MENTION_MP3,
    },
    NotificationSound::UserKicked => NotificationSoundAsset {
      sound,
      file_name: "user_kicked.mp3",
      bytes: USER_KICKED_MP3,
    },
    NotificationSound::StreamStarted => NotificationSoundAsset {
      sound,
      file_name: "stream_started.mp3",
      bytes: STREAM_STARTED_MP3,
    },
    NotificationSound::StreamEnded => NotificationSoundAsset {
      sound,
      file_name: "stream_ended.mp3",
      bytes: STREAM_ENDED_MP3,
    },
    NotificationSound::ConnectionLost => NotificationSoundAsset {
      sound,
      file_name: "connection_lost.mp3",
      bytes: CONNECTION_LOST_MP3,
    },
    NotificationSound::ModerationAction => NotificationSoundAsset {
      sound,
      file_name: "moderation_action.mp3",
      bytes: MODERATION_ACTION_MP3,
    },
  }
}

pub fn notification_sound_key(sound: NotificationSound) -> &'static str {
  match sound {
    NotificationSound::VoiceJoin => "voice_join",
    NotificationSound::VoiceLeave => "voice_leave",
    NotificationSound::ChatMessage => "chat_message",
    NotificationSound::Mention => "mention",
    NotificationSound::UserKicked => "user_kicked",
    NotificationSound::StreamStarted => "stream_started",
    NotificationSound::StreamEnded => "stream_ended",
    NotificationSound::ConnectionLost => "connection_lost",
    NotificationSound::ModerationAction => "moderation_action",
  }
}

pub fn notification_sound_file_name(sound: NotificationSound) -> &'static str {
  match sound {
    NotificationSound::VoiceJoin => "join_channel.mp3",
    NotificationSound::VoiceLeave => "leave_channel.mp3",
    NotificationSound::ChatMessage => "new_message.mp3",
    NotificationSound::Mention => "mention.mp3",
    NotificationSound::UserKicked => "user_kicked.mp3",
    NotificationSound::StreamStarted => "stream_started.mp3",
    NotificationSound::StreamEnded => "stream_ended.mp3",
    NotificationSound::ConnectionLost => "connection_lost.mp3",
    NotificationSound::ModerationAction => "moderation_action.mp3",
  }
}

pub fn custom_audio_dir() -> PathBuf {
  env::current_dir().unwrap_or_else(|_| PathBuf::from(".")).join("audio")
}

pub fn custom_sound_path(sound: NotificationSound) -> PathBuf {
  custom_audio_dir().join(notification_sound_file_name(sound))
}

pub fn outgoing_voice_join_sound_path() -> PathBuf {
  custom_audio_dir().join(OUTGOING_VOICE_JOIN_SOUND_FILE_NAME)
}

pub fn custom_sound_exists(sound: NotificationSound) -> bool {
  custom_sound_path(sound).is_file()
}

pub fn outgoing_voice_join_sound_exists() -> bool {
  outgoing_voice_join_sound_path().is_file()
}

pub fn install_custom_sound(sound: NotificationSound, source: &Path) -> Result<PathBuf, String> {
  install_custom_mp3(source, &custom_sound_path(sound), "notification sound")
}

pub fn install_outgoing_voice_join_sound(source: &Path) -> Result<PathBuf, String> {
  install_custom_mp3(source, &outgoing_voice_join_sound_path(), "outgoing voice join sound")
}

fn install_custom_mp3(source: &Path, target: &Path, label: &str) -> Result<PathBuf, String> {
  if source
    .extension()
    .and_then(|extension| extension.to_str())
    .is_none_or(|extension| !extension.eq_ignore_ascii_case("mp3"))
  {
    return Err(format!("Selected {label} must be an MP3 file."));
  }

  let Some(parent) = target.parent() else {
    return Err("Could not resolve audio directory.".to_owned());
  };
  fs::create_dir_all(parent).map_err(|error| format!("Failed to create audio directory: {error}"))?;

  let same_file = source
    .canonicalize()
    .ok()
    .zip(target.canonicalize().ok())
    .is_some_and(|(source, target)| source == target);
  if !same_file {
    fs::copy(source, target).map_err(|error| format!("Failed to copy {label}: {error}"))?;
  }

  Ok(target.to_path_buf())
}

pub fn resolve_sound_with_overrides(sound: NotificationSound, overrides: &str) -> NotificationSoundAsset {
  let choice = notification_sound_override(overrides, sound);
  resolve_sound_choice(sound, choice.as_deref().unwrap_or(SOUND_CHOICE_DEFAULT))
}

pub fn resolve_sound_choice(sound: NotificationSound, choice: &str) -> NotificationSoundAsset {
  match choice.trim() {
    SOUND_CHOICE_JOIN_CHANNEL => NotificationSoundAsset {
      sound,
      file_name: "join_channel.mp3",
      bytes: JOIN_CHANNEL_MP3,
    },
    SOUND_CHOICE_LEAVE_CHANNEL => NotificationSoundAsset {
      sound,
      file_name: "leave_channel.mp3",
      bytes: LEAVE_CHANNEL_MP3,
    },
    SOUND_CHOICE_NEW_MESSAGE => NotificationSoundAsset {
      sound,
      file_name: "new_message.mp3",
      bytes: NEW_MESSAGE_MP3,
    },
    SOUND_CHOICE_MENTION => NotificationSoundAsset {
      sound,
      file_name: "mention.mp3",
      bytes: MENTION_MP3,
    },
    SOUND_CHOICE_USER_KICKED => NotificationSoundAsset {
      sound,
      file_name: "user_kicked.mp3",
      bytes: USER_KICKED_MP3,
    },
    SOUND_CHOICE_STREAM_STARTED => NotificationSoundAsset {
      sound,
      file_name: "stream_started.mp3",
      bytes: STREAM_STARTED_MP3,
    },
    SOUND_CHOICE_STREAM_ENDED => NotificationSoundAsset {
      sound,
      file_name: "stream_ended.mp3",
      bytes: STREAM_ENDED_MP3,
    },
    SOUND_CHOICE_CONNECTION_LOST => NotificationSoundAsset {
      sound,
      file_name: "connection_lost.mp3",
      bytes: CONNECTION_LOST_MP3,
    },
    SOUND_CHOICE_MODERATION_ACTION => NotificationSoundAsset {
      sound,
      file_name: "moderation_action.mp3",
      bytes: MODERATION_ACTION_MP3,
    },
    _ => resolve_sound(sound),
  }
}

pub fn notification_sound_override(overrides: &str, sound: NotificationSound) -> Option<String> {
  sound_override(overrides, notification_sound_key(sound))
}

pub fn outgoing_voice_join_sound_override(overrides: &str) -> Option<String> {
  sound_override(overrides, OUTGOING_VOICE_JOIN_SOUND_KEY)
}

fn sound_override(overrides: &str, key: &str) -> Option<String> {
  let value = serde_json::from_str::<serde_json::Value>(overrides).ok()?;
  value
    .get(key)
    .and_then(serde_json::Value::as_str)
    .map(str::trim)
    .filter(|choice| !choice.is_empty())
    .map(str::to_owned)
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NotificationAudioSettings {
  pub output_device: String,
  pub volume: i32,
  pub sound_overrides: String,
}

impl NotificationAudioSettings {
  pub fn from_app_settings(settings: &AppSettings) -> Self {
    Self {
      output_device: settings.audio_output_device.clone(),
      volume: settings.notification_volume,
      sound_overrides: settings.notification_sound_overrides.clone(),
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
        tracing::warn!(target: "notifications", "[notifications] notification sound unavailable: {error}");
      }
    });
}

pub fn play_outgoing_voice_join(settings: NotificationAudioSettings) {
  if settings.volume <= 0 {
    return;
  }

  let _ = thread::Builder::new()
    .name("parties-outgoing-join-sound-preview".to_owned())
    .spawn(move || {
      if let Err(error) = play_outgoing_voice_join_blocking(settings) {
        tracing::warn!(target: "notifications", "[notifications] outgoing voice join preview unavailable: {error}");
      }
    });
}

fn play_blocking(sound: NotificationSound, settings: NotificationAudioSettings) -> Result<(), String> {
  let decoded = decode_notification_sound(sound, &settings.sound_overrides)?;
  play_decoded_blocking(decoded, settings)
}

fn play_outgoing_voice_join_blocking(settings: NotificationAudioSettings) -> Result<(), String> {
  let decoded = decode_outgoing_voice_join_sound(&settings.sound_overrides)?
    .ok_or_else(|| "No outgoing voice join sound selected.".to_owned())?;
  play_decoded_blocking(decoded, settings)
}

fn play_decoded_blocking(decoded: DecodedNotificationSound, settings: NotificationAudioSettings) -> Result<(), String> {
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
      move |error| tracing::warn!(target: "notifications", "[notifications] notification output error: {error}"),
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

fn decode_notification_sound(sound: NotificationSound, overrides: &str) -> Result<DecodedNotificationSound, String> {
  if notification_sound_override(overrides, sound).as_deref() == Some(SOUND_CHOICE_CUSTOM) {
    let path = custom_sound_path(sound);
    match fs::read(&path) {
      Ok(bytes) => match decode_mp3(&bytes) {
        Ok(decoded) => return Ok(decoded),
        Err(error) => tracing::warn!(target: "notifications",
          "[notifications] custom notification sound invalid: path={} error={error}",
          path.display()
        ),
      },
      Err(error) => tracing::warn!(target: "notifications",
        "[notifications] custom notification sound unavailable: path={} error={error}",
        path.display()
      ),
    }
  }
  decode_mp3(resolve_sound_with_overrides(sound, overrides).bytes)
}

pub(crate) fn decode_outgoing_voice_join_sound_mono(
  overrides: &str,
  target_sample_rate: u32,
) -> Result<Option<Vec<f32>>, String> {
  Ok(
    decode_outgoing_voice_join_sound(overrides)?
      .map(|sound| sound.resampled_mono(target_sample_rate))
      .filter(|samples| !samples.is_empty()),
  )
}

fn decode_outgoing_voice_join_sound(overrides: &str) -> Result<Option<DecodedNotificationSound>, String> {
  if outgoing_voice_join_sound_override(overrides).as_deref() != Some(SOUND_CHOICE_CUSTOM) {
    return Ok(None);
  }

  let path = outgoing_voice_join_sound_path();
  match fs::read(&path) {
    Ok(bytes) => match decode_mp3(&bytes) {
      Ok(decoded) => Ok(Some(decoded)),
      Err(error) => {
        tracing::warn!(
          target: "notifications",
          "[notifications] outgoing voice join sound invalid: path={} error={error}",
          path.display()
        );
        Err(error)
      }
    },
    Err(error) => {
      tracing::warn!(
        target: "notifications",
        "[notifications] outgoing voice join sound unavailable: path={} error={error}",
        path.display()
      );
      Ok(None)
    }
  }
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

impl DecodedNotificationSound {
  fn resampled_mono(&self, target_sample_rate: u32) -> Vec<f32> {
    if self.samples.is_empty() || self.channels == 0 || self.sample_rate == 0 || target_sample_rate == 0 {
      return Vec::new();
    }

    let frame_count = self.samples.len() / self.channels;
    if frame_count == 0 {
      return Vec::new();
    }

    let target_frames =
      ((frame_count as u64) * u64::from(target_sample_rate)).div_ceil(u64::from(self.sample_rate)) as usize;
    let step = f64::from(self.sample_rate) / f64::from(target_sample_rate);
    let mut mono = Vec::with_capacity(target_frames);
    let mut source_position = 0.0_f64;
    for _ in 0..target_frames {
      let index = source_position.floor() as usize;
      let next_index = (index + 1).min(frame_count - 1);
      let fraction = (source_position - index as f64) as f32;
      let current = self.frame_average(index.min(frame_count - 1));
      let next = self.frame_average(next_index);
      mono.push((current + (next - current) * fraction).clamp(-1.0, 1.0));
      source_position += step;
    }
    mono
  }

  fn frame_average(&self, index: usize) -> f32 {
    let start = index * self.channels;
    let end = start + self.channels;
    self.samples[start..end].iter().sum::<f32>() / self.channels as f32
  }
}

#[cfg(test)]
#[path = "../../tests/unit/services/notifications.rs"]
mod tests;
