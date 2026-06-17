use std::{
  cell::Cell,
  collections::{HashMap, HashSet, VecDeque},
  env, fmt,
  panic::{AssertUnwindSafe, catch_unwind},
  sync::{
    Arc, LazyLock, Mutex,
    atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
    mpsc::{self, Receiver, SyncSender, TrySendError},
  },
  thread::{self, JoinHandle},
  time::{Duration, Instant},
};

use cpal::{
  BufferSize, Sample, SampleFormat, SupportedBufferSize,
  traits::{DeviceTrait, StreamTrait},
};
use opus::{Application, Bitrate, Channels, Decoder, Encoder, Signal};
use sonora::{
  AudioProcessing, Config, StreamConfig,
  config::{EchoCanceller, HighPassFilter, MaxProcessingRate, NoiseSuppression, NoiseSuppressionLevel, Pipeline},
};

use super::{audio_devices, notifications};
use crate::{
  network::{
    protocol::{
      UserId,
      data::{ForwardedStreamAudioPacket, ForwardedVoicePacket},
    },
    server::Server,
  },
  storage::AppSettings,
};

#[cfg(target_os = "macos")]
unsafe extern "C" {
  fn parties_macos_microphone_authorize() -> i32;
  fn parties_macos_last_error() -> *const std::ffi::c_char;
}

const SAMPLE_RATE: u32 = 48_000;
const CHANNELS: usize = 1;
const STREAM_CHANNELS: usize = 2;
const PROCESS_FRAME_SIZE: usize = 480;
const OPUS_FRAME_SIZE: usize = 960;
const OPUS_BITRATE: i32 = 32_000;
const MAX_OPUS_PACKET: usize = 512;
const INPUT_FRAME_QUEUE: usize = 8;
const INPUT_FRAME_POOL: usize = INPUT_FRAME_QUEUE + 2;
const MAX_PCM_FRAMES_PER_USER: usize = 12;
const MAX_LOCAL_NOTIFICATION_FRAMES: usize = MAX_OUTGOING_SOUND_SAMPLES.div_ceil(OPUS_FRAME_SIZE) + 2;
const MIN_PCM_FRAMES_BEFORE_PLAYOUT: usize = 2;
const MAX_PLC_FRAMES: i16 = 3;
const MAX_LATE_VOICE_FRAMES_BEFORE_RESET: i16 = 32;
const LATE_VOICE_PACKETS_BEFORE_RESET: u8 = 3;
const MAX_CAPTURE_QUEUE_SAMPLES: usize = OPUS_FRAME_SIZE * 5;
const MAX_OPUS_QUEUE_SAMPLES: usize = OPUS_FRAME_SIZE * INPUT_FRAME_POOL;
const MAX_RENDER_QUEUE_SAMPLES: usize = PROCESS_FRAME_SIZE * 50;
const MAX_OUTGOING_SOUND_SAMPLES: usize = SAMPLE_RATE as usize * 2;
const OUTGOING_SOUND_FADE_SAMPLES: usize = SAMPLE_RATE as usize / 50;
const OUTGOING_SOUND_MAX_PEAK: f32 = 0.5;
const STREAM_BUFFER_TARGET_MS: u32 = 20;
const VOICE_ACTIVATION_HOLD_FRAMES: u8 = 12;
const DEFAULT_AEC_DELAY_MS: i32 = 20;
const AEC_DELAY_ENV: &str = "PARTIES_AEC_DELAY_MS";
const MAX_PUSH_TO_TALK_RELEASE_DELAY_MS: i32 = 2_000;
const VOICE_SEND_LOG_INTERVAL: u64 = 100;
const LOCAL_VOICE_INPUT_IDLE_WARN_AFTER: Duration = Duration::from_secs(10);
const LOCAL_VOICE_SEND_IDLE_WARN_AFTER: Duration = Duration::from_secs(10);
const LOCAL_VOICE_IDLE_WARN_REPEAT: Duration = Duration::from_secs(30);
static VOICE_CLOCK_START: LazyLock<Instant> = LazyLock::new(Instant::now);
thread_local! {
  static CATCHING_INPUT_CAPTURE_CALLBACK_PANIC: Cell<u32> = const { Cell::new(0) };
}

pub type LocalVoiceCallback = Arc<dyn Fn() + Send + Sync + 'static>;
pub type LocalSpeakingActivityCallback = Arc<dyn Fn(bool) + Send + Sync + 'static>;

pub fn is_catching_input_capture_callback_panic() -> bool {
  CATCHING_INPUT_CAPTURE_CALLBACK_PANIC.with(|depth| depth.get() > 0)
}

#[derive(Debug)]
pub struct VoiceError {
  message: String,
}

impl VoiceError {
  fn new(message: impl Into<String>) -> Self {
    Self {
      message: message.into(),
    }
  }
}

impl fmt::Display for VoiceError {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.write_str(&self.message)
  }
}

impl std::error::Error for VoiceError {}

fn should_log_voice_send_count(count: u64) -> bool {
  count == 1 || count % VOICE_SEND_LOG_INTERVAL == 0
}

pub struct VoiceEngine {
  _input_stream: Option<cpal::Stream>,
  _output_stream: Option<cpal::Stream>,
  encoder_thread: Option<JoinHandle<()>>,
  stop: Arc<AtomicBool>,
  control: Arc<VoiceControlState>,
  mixer: Arc<Mutex<VoiceMixer>>,
  outgoing_frame_tx: Option<SyncSender<EncodeFrame>>,
  outgoing_sound_generation: Arc<AtomicU64>,
  decoders: HashMap<UserId, DecodeStream>,
  stream_decoders: HashMap<UserId, DecodeStream>,
  normalized_users: HashSet<UserId>,
  captures_voice: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct VoicePacketStatus {
  pub queued: bool,
  pub speaking: bool,
}

impl VoiceEngine {
  pub fn start(
    server: Arc<Server>,
    settings: AppSettings,
    muted: bool,
    deafened: bool,
    on_local_voice: LocalVoiceCallback,
  ) -> Result<Self, VoiceError> {
    let stop = Arc::new(AtomicBool::new(false));
    let control = Arc::new(VoiceControlState::new(&settings, muted, deafened));
    let mixer = Arc::new(Mutex::new(VoiceMixer::default()));
    let outgoing_sound_generation = Arc::new(AtomicU64::new(0));

    let output_stream = match build_output_stream(&settings, control.clone(), mixer.clone()) {
      Ok(stream) => Some(stream),
      Err(error) => {
        if settings.echo_cancellation {
          tracing::warn!(target: "audio::decode", "[audio:decode] echo cancellation enabled, but render reference output stream failed: {error}");
        }
        None
      }
    };
    let input_result = build_input_path(server, &settings, control.clone(), stop.clone(), on_local_voice);
    let input_error = input_result.as_ref().err().map(ToString::to_string);
    let input_path = input_result.unwrap_or_default();

    if input_path.stream.is_none() && output_stream.is_none() {
      return Err(VoiceError::new(
        input_error.unwrap_or_else(|| "No usable audio input or output device.".to_owned()),
      ));
    }
    let captures_voice = input_path.stream.is_some() || input_path.encoder_thread.is_some();

    Ok(Self {
      _input_stream: input_path.stream,
      _output_stream: output_stream,
      encoder_thread: input_path.encoder_thread,
      stop,
      control,
      mixer,
      outgoing_frame_tx: input_path.frame_tx,
      outgoing_sound_generation,
      decoders: HashMap::new(),
      stream_decoders: HashMap::new(),
      normalized_users: HashSet::new(),
      captures_voice,
    })
  }

  pub fn start_playback(settings: AppSettings, deafened: bool) -> Result<Self, VoiceError> {
    let stop = Arc::new(AtomicBool::new(false));
    let control = Arc::new(VoiceControlState::new(&settings, true, deafened));
    let mixer = Arc::new(Mutex::new(VoiceMixer::default()));
    let output_stream = build_output_stream(&settings, control.clone(), mixer.clone())?;

    Ok(Self {
      _input_stream: None,
      _output_stream: Some(output_stream),
      encoder_thread: None,
      stop,
      control,
      mixer,
      outgoing_frame_tx: None,
      outgoing_sound_generation: Arc::new(AtomicU64::new(0)),
      decoders: HashMap::new(),
      stream_decoders: HashMap::new(),
      normalized_users: HashSet::new(),
      captures_voice: false,
    })
  }

  pub fn captures_voice(&self) -> bool {
    self.captures_voice
  }

  pub fn set_voice_state(&self, muted: bool, deafened: bool) {
    self.control.set_voice_state(muted, deafened);
    if muted || deafened {
      self.outgoing_sound_generation.fetch_add(1, Ordering::Relaxed);
      self.control.set_outgoing_sound_active(false);
    }
    if deafened {
      self
        .mixer
        .lock()
        .expect("voice mixer lock poisoned")
        .clear_voice_audio();
    }
  }

  pub fn set_voice_activation_threshold(&self, value: i32) {
    self.control.set_voice_activation_threshold(value);
  }

  pub fn set_voice_normalization(&self, value: bool) {
    self.control.set_voice_normalization(value);
  }

  pub fn set_voice_normalization_target_level(&self, value: i32) {
    self.control.set_voice_normalization_target_level(value);
  }

  pub fn set_user_normalization(&mut self, user_id: UserId, enabled: bool) {
    if enabled {
      self.normalized_users.insert(user_id);
    } else {
      self.normalized_users.remove(&user_id);
      if let Some(stream) = self.decoders.get_mut(&user_id) {
        stream.reset_normalization();
      }
    }
  }

  pub fn set_push_to_talk_active(&self, active: bool) {
    self.control.set_push_to_talk_active(active);
  }

  pub fn push_to_talk_release_delay_ms(&self) -> u64 {
    self.control.push_to_talk_release_delay_ms()
  }

  pub fn set_push_to_talk_release_delay_ms(&self, value: i32) {
    self.control.set_push_to_talk_release_delay_ms(value);
  }

  pub fn queue_outgoing_voice_join_sound(
    &self,
    sound_overrides: &str,
    volume_percent: i32,
    on_local_intro_activity: LocalSpeakingActivityCallback,
  ) -> Result<bool, String> {
    if !self.captures_voice {
      return Ok(false);
    }
    let Some(frame_tx) = self.outgoing_frame_tx.clone() else {
      return Ok(false);
    };
    if !self.control.can_transmit_outgoing_sound() {
      return Ok(false);
    }
    if volume_percent <= 0 {
      return Ok(false);
    }

    let Some(mut samples) = notifications::decode_outgoing_voice_join_sound_mono(sound_overrides, SAMPLE_RATE)? else {
      return Ok(false);
    };
    if samples.len() > MAX_OUTGOING_SOUND_SAMPLES {
      samples.truncate(MAX_OUTGOING_SOUND_SAMPLES);
    }
    apply_outgoing_sound_volume(&mut samples, volume_percent);
    apply_outgoing_sound_fade(&mut samples);
    on_local_intro_activity(true);
    self.play_local_outgoing_sound(&samples);

    let generation = self.outgoing_sound_generation.fetch_add(1, Ordering::Relaxed) + 1;
    let active_generation = self.outgoing_sound_generation.clone();
    let control = self.control.clone();
    let finish_local_intro_activity = on_local_intro_activity.clone();
    control.set_outgoing_sound_active(true);
    if let Err(error) = thread::Builder::new()
      .name("parties-outgoing-join-sound".to_owned())
      .spawn(move || {
        let frame_duration = Duration::from_millis(20);
        let mut next_frame_at = Instant::now();
        for chunk in samples.chunks(OPUS_FRAME_SIZE) {
          if active_generation.load(Ordering::Relaxed) != generation || !control.can_transmit_outgoing_sound() {
            break;
          }
          let mut frame = Vec::with_capacity(OPUS_FRAME_SIZE);
          frame.extend_from_slice(chunk);
          frame.resize(OPUS_FRAME_SIZE, 0.0);
          if frame_tx
            .send(EncodeFrame {
              samples: frame,
              force_transmit: true,
            })
            .is_err()
          {
            break;
          }
          next_frame_at = next_frame_at.checked_add(frame_duration).unwrap_or_else(Instant::now);
          if let Some(delay) = next_frame_at.checked_duration_since(Instant::now()) {
            thread::sleep(delay);
          }
        }
        finish_local_intro_activity(false);
        if active_generation.load(Ordering::Relaxed) == generation {
          control.set_outgoing_sound_active(false);
        }
      })
    {
      self.outgoing_sound_generation.fetch_add(1, Ordering::Relaxed);
      self.control.set_outgoing_sound_active(false);
      on_local_intro_activity(false);
      return Err(format!("Failed to start outgoing voice join sound thread: {error}"));
    }
    Ok(true)
  }

  fn play_local_outgoing_sound(&self, samples: &[f32]) {
    let mut mixer = self.mixer.lock().expect("voice mixer lock poisoned");
    mixer.clear_local_notification_audio();
    for chunk in samples.chunks(OPUS_FRAME_SIZE) {
      let mut frame = Vec::with_capacity(OPUS_FRAME_SIZE);
      frame.extend_from_slice(chunk);
      frame.resize(OPUS_FRAME_SIZE, 0.0);
      mixer.push_frame(AudioStreamId::LocalNotification, frame);
    }
  }

  pub fn set_user_volume(&self, user_id: UserId, volume_percent: i32) {
    self
      .mixer
      .lock()
      .expect("voice mixer lock poisoned")
      .set_user_volume(user_id, volume_percent);
  }

  pub fn restart_audio_receiver(&mut self, user_id: UserId) -> bool {
    let had_decoder = self.decoders.remove(&user_id).is_some();
    let cleared_audio = self
      .mixer
      .lock()
      .expect("voice mixer lock poisoned")
      .clear_voice_audio_for_user(user_id);
    had_decoder || cleared_audio
  }

  pub fn set_stream_volume(&self, user_id: UserId, volume_percent: i32) {
    self
      .mixer
      .lock()
      .expect("voice mixer lock poisoned")
      .set_stream_volume(user_id, volume_percent);
  }

  pub fn push_packet(&mut self, packet: ForwardedVoicePacket) -> VoicePacketStatus {
    if self.control.deafened.load(Ordering::Relaxed) {
      return VoicePacketStatus::default();
    }

    let mut pcm = self.take_pcm_buffer(OPUS_FRAME_SIZE * CHANNELS);
    let stream = match self.decoders.entry(packet.sender_id) {
      std::collections::hash_map::Entry::Occupied(entry) => entry.into_mut(),
      std::collections::hash_map::Entry::Vacant(entry) => match DecodeStream::new(Channels::Mono) {
        Ok(stream) => entry.insert(stream),
        Err(_) => {
          self.recycle_pcm_buffer(pcm);
          return VoicePacketStatus::default();
        }
      },
    };

    if stream.decode_into(packet.sequence, &packet.opus, &mut pcm).is_err() {
      self.recycle_pcm_buffer(pcm);
      return VoicePacketStatus::default();
    }

    if self.control.voice_normalization.load(Ordering::Relaxed) && self.normalized_users.contains(&packet.sender_id) {
      let target = f32::from_bits(self.control.voice_normalization_target.load(Ordering::Relaxed));
      stream.apply_normalization(&mut pcm, target);
    }

    let speaking = rms(&pcm) > 0.001;
    let queued = !pcm.is_empty();

    if queued {
      self
        .mixer
        .lock()
        .expect("voice mixer lock poisoned")
        .push_frame(AudioStreamId::Voice(packet.sender_id), pcm);
    }

    VoicePacketStatus { queued, speaking }
  }

  pub fn push_stream_audio_packet(&mut self, packet: ForwardedStreamAudioPacket) -> bool {
    let mut pcm = self.take_pcm_buffer(OPUS_FRAME_SIZE * CHANNELS);
    let stream = match self.stream_decoders.entry(packet.sender_id) {
      std::collections::hash_map::Entry::Occupied(entry) => entry.into_mut(),
      std::collections::hash_map::Entry::Vacant(entry) => match DecodeStream::new(Channels::Stereo) {
        Ok(stream) => entry.insert(stream),
        Err(_) => {
          self.recycle_pcm_buffer(pcm);
          return false;
        }
      },
    };

    if stream.decode_stereo_downmix_into(&packet.opus, &mut pcm).is_err() {
      self.recycle_pcm_buffer(pcm);
      return false;
    }
    if pcm.is_empty() {
      self.recycle_pcm_buffer(pcm);
      return false;
    }

    self
      .mixer
      .lock()
      .expect("voice mixer lock poisoned")
      .push_frame(AudioStreamId::Stream(packet.sender_id), pcm);
    true
  }

  fn take_pcm_buffer(&self, capacity: usize) -> Vec<f32> {
    self
      .mixer
      .lock()
      .expect("voice mixer lock poisoned")
      .take_frame_buffer(capacity)
  }

  fn recycle_pcm_buffer(&self, frame: Vec<f32>) {
    self
      .mixer
      .lock()
      .expect("voice mixer lock poisoned")
      .recycle_frame(frame);
  }

  pub fn clear_stream_audio(&self, user_id: UserId) {
    self
      .mixer
      .lock()
      .expect("voice mixer lock poisoned")
      .clear_stream_audio(user_id);
  }

  pub fn clear_all_stream_audio(&self) {
    self
      .mixer
      .lock()
      .expect("voice mixer lock poisoned")
      .clear_all_stream_audio();
  }
}

impl Drop for VoiceEngine {
  fn drop(&mut self) {
    self.stop.store(true, Ordering::Relaxed);
    self._input_stream.take();
    self._output_stream.take();
    if let Some(thread) = self.encoder_thread.take() {
      let _ = thread.join();
    }
  }
}

struct VoiceControlState {
  muted: AtomicBool,
  deafened: AtomicBool,
  voice_normalization: AtomicBool,
  voice_normalization_target: AtomicU32,
  voice_activation_threshold: AtomicU32,
  push_to_talk: bool,
  push_to_talk_release_delay_ms: AtomicU64,
  push_to_talk_active: AtomicBool,
  push_to_talk_release_until_ms: AtomicU64,
  outgoing_sound_active: AtomicBool,
  audio_processing: Option<Arc<Mutex<AudioProcessing>>>,
}

impl VoiceControlState {
  fn new(settings: &AppSettings, muted: bool, deafened: bool) -> Self {
    Self {
      muted: AtomicBool::new(muted),
      deafened: AtomicBool::new(deafened),
      voice_normalization: AtomicBool::new(settings.voice_normalization),
      voice_normalization_target: AtomicU32::new(normalize_target(settings.voice_normalization_target_level).to_bits()),
      voice_activation_threshold: AtomicU32::new(activation_threshold(settings.voice_activation_threshold).to_bits()),
      push_to_talk: settings.push_to_talk,
      push_to_talk_release_delay_ms: AtomicU64::new(push_to_talk_release_delay_ms(
        settings.push_to_talk_release_delay_ms,
      )),
      push_to_talk_active: AtomicBool::new(false),
      push_to_talk_release_until_ms: AtomicU64::new(0),
      outgoing_sound_active: AtomicBool::new(false),
      audio_processing: build_audio_processing(settings),
    }
  }

  fn can_transmit(&self) -> bool {
    !self.muted.load(Ordering::Relaxed)
      && !self.deafened.load(Ordering::Relaxed)
      && (!self.push_to_talk
        || self.push_to_talk_active.load(Ordering::Relaxed)
        || self.push_to_talk_release_until_ms.load(Ordering::Relaxed) > monotonic_millis())
  }

  fn can_transmit_outgoing_sound(&self) -> bool {
    !self.muted.load(Ordering::Relaxed) && !self.deafened.load(Ordering::Relaxed)
  }

  fn outgoing_sound_active(&self) -> bool {
    self.outgoing_sound_active.load(Ordering::Relaxed)
  }

  fn set_outgoing_sound_active(&self, active: bool) {
    self.outgoing_sound_active.store(active, Ordering::Relaxed);
  }

  fn audio_processing(&self) -> Option<&Arc<Mutex<AudioProcessing>>> {
    self.audio_processing.as_ref()
  }

  fn set_voice_state(&self, muted: bool, deafened: bool) {
    self.muted.store(muted, Ordering::Relaxed);
    self.deafened.store(deafened, Ordering::Relaxed);
    if muted || deafened {
      self.push_to_talk_active.store(false, Ordering::Relaxed);
      self.push_to_talk_release_until_ms.store(0, Ordering::Relaxed);
      self.outgoing_sound_active.store(false, Ordering::Relaxed);
    }
  }

  fn set_voice_activation_threshold(&self, value: i32) {
    self
      .voice_activation_threshold
      .store(activation_threshold(value).to_bits(), Ordering::Relaxed);
  }

  fn set_voice_normalization(&self, value: bool) {
    self.voice_normalization.store(value, Ordering::Relaxed);
  }

  fn set_voice_normalization_target_level(&self, value: i32) {
    self
      .voice_normalization_target
      .store(normalize_target(value).to_bits(), Ordering::Relaxed);
  }

  fn set_push_to_talk_active(&self, active: bool) {
    if active && (self.muted.load(Ordering::Relaxed) || self.deafened.load(Ordering::Relaxed)) {
      self.push_to_talk_active.store(false, Ordering::Relaxed);
      self.push_to_talk_release_until_ms.store(0, Ordering::Relaxed);
      return;
    }

    self.push_to_talk_active.store(active, Ordering::Relaxed);
    let release_delay_ms = self.push_to_talk_release_delay_ms();
    let release_until_ms = if active || release_delay_ms == 0 {
      0
    } else {
      monotonic_millis().saturating_add(release_delay_ms)
    };
    self
      .push_to_talk_release_until_ms
      .store(release_until_ms, Ordering::Relaxed);
  }

  fn set_push_to_talk_release_delay_ms(&self, value: i32) {
    let value = push_to_talk_release_delay_ms(value);
    self.push_to_talk_release_delay_ms.store(value, Ordering::Relaxed);
    if value == 0 && !self.push_to_talk_active.load(Ordering::Relaxed) {
      self.push_to_talk_release_until_ms.store(0, Ordering::Relaxed);
    }
  }

  fn push_to_talk_release_delay_ms(&self) -> u64 {
    self.push_to_talk_release_delay_ms.load(Ordering::Relaxed)
  }

  fn voice_activation_threshold(&self) -> f32 {
    f32::from_bits(self.voice_activation_threshold.load(Ordering::Relaxed))
  }
}

fn build_audio_processing(settings: &AppSettings) -> Option<Arc<Mutex<AudioProcessing>>> {
  if !settings.noise_cancellation && !settings.echo_cancellation {
    return None;
  }

  let config = Config {
    pipeline: Pipeline {
      maximum_internal_processing_rate: MaxProcessingRate::Rate48kHz,
      ..Default::default()
    },
    high_pass_filter: Some(HighPassFilter::default()),
    echo_canceller: settings.echo_cancellation.then(EchoCanceller::default),
    noise_suppression: settings.noise_cancellation.then(|| NoiseSuppression {
      level: NoiseSuppressionLevel::High,
      ..Default::default()
    }),
    ..Default::default()
  };

  let mut audio_processing = AudioProcessing::builder()
    .config(config)
    .capture_config(StreamConfig::new(SAMPLE_RATE, CHANNELS as u16))
    .render_config(StreamConfig::new(SAMPLE_RATE, CHANNELS as u16))
    .build();

  if settings.echo_cancellation {
    let delay_ms = aec_delay_ms();
    let _ = audio_processing.set_stream_delay_ms(delay_ms);
    tracing::info!(target: "audio::encode",
      "[audio:encode] echo cancellation enabled: stream_delay_ms={}",
      audio_processing.stream_delay_ms()
    );
  }

  Some(Arc::new(Mutex::new(audio_processing)))
}

fn aec_delay_ms() -> i32 {
  configured_aec_delay_ms(env::var(AEC_DELAY_ENV).ok().as_deref())
}

fn configured_aec_delay_ms(value: Option<&str>) -> i32 {
  value
    .and_then(|value| value.parse::<i32>().ok())
    .unwrap_or(DEFAULT_AEC_DELAY_MS)
    .clamp(0, 500)
}

fn push_to_talk_release_delay_ms(value: i32) -> u64 {
  value.clamp(0, MAX_PUSH_TO_TALK_RELEASE_DELAY_MS) as u64
}

fn monotonic_millis() -> u64 {
  VOICE_CLOCK_START.elapsed().as_millis().min(u128::from(u64::MAX)) as u64
}

fn low_latency_stream_config(supported: &cpal::SupportedStreamConfig) -> cpal::StreamConfig {
  let mut config = supported.config();

  if let SupportedBufferSize::Range { min, max } = *supported.buffer_size() {
    let target = frames_for_duration(config.sample_rate, STREAM_BUFFER_TARGET_MS);
    config.buffer_size = BufferSize::Fixed(target.clamp(min.max(1), max.max(1)));
  }

  config
}

fn frames_for_duration(sample_rate: u32, duration_ms: u32) -> u32 {
  let frames = (u64::from(sample_rate.max(1)) * u64::from(duration_ms.max(1))).div_ceil(1000);
  frames.clamp(1, u64::from(u32::MAX)) as u32
}

fn build_input_path(
  server: Arc<Server>,
  settings: &AppSettings,
  control: Arc<VoiceControlState>,
  stop: Arc<AtomicBool>,
  on_local_voice: LocalVoiceCallback,
) -> Result<VoiceInputPath, VoiceError> {
  ensure_microphone_authorized()?;

  let Some(device) = audio_devices::input_device(&settings.audio_input_device) else {
    return Ok(VoiceInputPath::default());
  };

  let supported_config = device
    .default_input_config()
    .map_err(|error| VoiceError::new(format!("Failed to read input config: {error}")))?;
  let sample_format = supported_config.sample_format();
  let config = low_latency_stream_config(&supported_config);
  let (frame_tx, frame_rx) = mpsc::sync_channel(INPUT_FRAME_QUEUE);
  let outgoing_frame_tx = frame_tx.clone();
  let (free_frame_tx, free_frame_rx) = mpsc::sync_channel(INPUT_FRAME_POOL);
  for _ in 0..INPUT_FRAME_POOL {
    let _ = free_frame_tx.try_send(Vec::with_capacity(OPUS_FRAME_SIZE));
  }

  let input_stream = match sample_format {
    SampleFormat::F32 => {
      build_input_stream::<f32>(&device, config, frame_tx, free_frame_rx, control.clone(), stop.clone())
    }
    SampleFormat::F64 => {
      build_input_stream::<f64>(&device, config, frame_tx, free_frame_rx, control.clone(), stop.clone())
    }
    SampleFormat::I8 => {
      build_input_stream::<i8>(&device, config, frame_tx, free_frame_rx, control.clone(), stop.clone())
    }
    SampleFormat::I16 => {
      build_input_stream::<i16>(&device, config, frame_tx, free_frame_rx, control.clone(), stop.clone())
    }
    SampleFormat::I24 => {
      build_input_stream::<cpal::I24>(&device, config, frame_tx, free_frame_rx, control.clone(), stop.clone())
    }
    SampleFormat::I32 => {
      build_input_stream::<i32>(&device, config, frame_tx, free_frame_rx, control.clone(), stop.clone())
    }
    SampleFormat::I64 => {
      build_input_stream::<i64>(&device, config, frame_tx, free_frame_rx, control.clone(), stop.clone())
    }
    SampleFormat::U8 => {
      build_input_stream::<u8>(&device, config, frame_tx, free_frame_rx, control.clone(), stop.clone())
    }
    SampleFormat::U16 => {
      build_input_stream::<u16>(&device, config, frame_tx, free_frame_rx, control.clone(), stop.clone())
    }
    SampleFormat::U24 => {
      build_input_stream::<cpal::U24>(&device, config, frame_tx, free_frame_rx, control.clone(), stop.clone())
    }
    SampleFormat::U32 => {
      build_input_stream::<u32>(&device, config, frame_tx, free_frame_rx, control.clone(), stop.clone())
    }
    SampleFormat::U64 => {
      build_input_stream::<u64>(&device, config, frame_tx, free_frame_rx, control.clone(), stop.clone())
    }
    _ => Err(VoiceError::new("Unsupported input sample format.")),
  }?;

  input_stream
    .play()
    .map_err(|error| VoiceError::new(format!("Failed to start input stream: {error}")))?;

  let encoder_thread = spawn_encoder_thread(server, frame_rx, free_frame_tx, stop, on_local_voice, control)?;
  Ok(VoiceInputPath {
    stream: Some(input_stream),
    encoder_thread: Some(encoder_thread),
    frame_tx: Some(outgoing_frame_tx),
  })
}

#[derive(Default)]
struct VoiceInputPath {
  stream: Option<cpal::Stream>,
  encoder_thread: Option<JoinHandle<()>>,
  frame_tx: Option<SyncSender<EncodeFrame>>,
}

#[cfg(target_os = "macos")]
fn ensure_microphone_authorized() -> Result<(), VoiceError> {
  let authorized = unsafe { parties_macos_microphone_authorize() != 0 };
  if authorized {
    return Ok(());
  }

  let error = unsafe {
    let ptr = parties_macos_last_error();
    if ptr.is_null() {
      None
    } else {
      std::ffi::CStr::from_ptr(ptr)
        .to_str()
        .ok()
        .filter(|message| !message.is_empty())
        .map(str::to_owned)
    }
  }
  .unwrap_or_else(|| "microphone permission was not granted".to_owned());

  Err(VoiceError::new(error))
}

#[cfg(not(target_os = "macos"))]
fn ensure_microphone_authorized() -> Result<(), VoiceError> {
  Ok(())
}

fn build_input_stream<T>(
  device: &cpal::Device,
  config: cpal::StreamConfig,
  frame_tx: SyncSender<EncodeFrame>,
  free_frame_rx: Receiver<Vec<f32>>,
  control: Arc<VoiceControlState>,
  stop: Arc<AtomicBool>,
) -> Result<cpal::Stream, VoiceError>
where
  T: cpal::SizedSample,
  f32: cpal::FromSample<T>,
{
  let channels = usize::from(config.channels.max(1));
  let sample_rate = config.sample_rate;
  let mut state = InputCaptureState::new(channels, sample_rate, frame_tx, free_frame_rx, control, stop);

  device
    .build_input_stream::<T, _, _>(
      config,
      move |data, _| state.push_catching(data),
      move |error| tracing::warn!(target: "audio::encode", "[audio:encode] input stream error: {error}"),
      None,
    )
    .map_err(|error| VoiceError::new(format!("Failed to build input stream: {error}")))
}

fn spawn_encoder_thread(
  server: Arc<Server>,
  frame_rx: Receiver<EncodeFrame>,
  free_frame_tx: SyncSender<Vec<f32>>,
  stop: Arc<AtomicBool>,
  on_local_voice: LocalVoiceCallback,
  control: Arc<VoiceControlState>,
) -> Result<JoinHandle<()>, VoiceError> {
  let mut encoder = Encoder::new(SAMPLE_RATE, Channels::Mono, Application::Voip)
    .map_err(|error| VoiceError::new(format!("Failed to create Opus encoder: {error}")))?;
  let _ = encoder.set_bitrate(Bitrate::Bits(OPUS_BITRATE));
  let _ = encoder.set_signal(Signal::Voice);
  let _ = encoder.set_force_channels(Some(Channels::Mono));

  thread::Builder::new()
    .name("parties-voice-encoder".to_owned())
    .spawn(move || {
      tracing::info!(target: "audio::encode", "[audio:encode] voice encoder thread started");
      let started_at = Instant::now();
      let mut sequence = 0u16;
      let mut opus = vec![0u8; MAX_OPUS_PACKET];
      let mut voice_gate = VoiceActivationGate::default();
      let mut encoded_packets = 0_u64;
      let mut sent_packets = 0_u64;
      let mut send_errors = 0_u64;
      let mut suppressed_frames = 0_u64;
      let mut encode_errors = 0_u64;
      let mut input_frames = 0_u64;
      let mut last_input_frame_at = started_at;
      let mut last_sent_at = started_at;
      let mut last_input_idle_warn_at = None;
      let mut last_send_idle_warn_at = None;
      while !stop.load(Ordering::Relaxed) {
        let mut frame = match frame_rx.recv_timeout(Duration::from_millis(50)) {
          Ok(frame) => frame,
          Err(mpsc::RecvTimeoutError::Timeout) => {
            warn_local_voice_input_idle(
              &control,
              Instant::now(),
              last_input_frame_at,
              input_frames,
              sent_packets,
              suppressed_frames,
              &mut last_input_idle_warn_at,
            );
            continue;
          }
          Err(mpsc::RecvTimeoutError::Disconnected) => break,
        };
        let now = Instant::now();
        input_frames = input_frames.saturating_add(1);
        last_input_frame_at = now;

        if !frame.force_transmit && control.outgoing_sound_active() {
          frame.samples.clear();
          let _ = free_frame_tx.try_send(frame.samples);
          continue;
        }

        if !frame.force_transmit && !voice_gate.should_transmit(&control, &frame.samples) {
          suppressed_frames = suppressed_frames.saturating_add(1);
          warn_local_voice_send_idle(
            &control,
            now,
            last_sent_at,
            input_frames,
            encoded_packets,
            sent_packets,
            suppressed_frames,
            voice_gate.hold_frames,
            &mut last_send_idle_warn_at,
          );
          frame.samples.clear();
          let _ = free_frame_tx.try_send(frame.samples);
          continue;
        }

        let len = match encoder.encode_float(&frame.samples, &mut opus) {
          Ok(len) => len,
          Err(error) => {
            encode_errors = encode_errors.saturating_add(1);
            tracing::warn!(
              target: "audio::encode",
              "[audio:encode] voice opus encode failed #{}: {error}",
              encode_errors
            );
            frame.samples.clear();
            let _ = free_frame_tx.try_send(frame.samples);
            continue;
          }
        };
        if len == 0 {
          frame.samples.clear();
          let _ = free_frame_tx.try_send(frame.samples);
          continue;
        }

        encoded_packets = encoded_packets.saturating_add(1);
        match server.send_voice(sequence, &opus[..len]) {
          Ok(()) => {
            sent_packets = sent_packets.saturating_add(1);
            last_sent_at = Instant::now();
            last_send_idle_warn_at = None;
            if should_log_voice_send_count(sent_packets) {
              tracing::debug!(
                target: "audio::encode",
                "[audio:encode] sent voice packet #{sent_packets}: sequence={sequence} bytes={len} encoded_packets={encoded_packets} suppressed_frames={suppressed_frames}"
              );
            }
            on_local_voice();
          }
          Err(error) => {
            send_errors = send_errors.saturating_add(1);
            tracing::warn!(
              target: "audio::encode",
              "[audio:encode] voice datagram send failed #{}: sequence={sequence} bytes={len} encoded_packets={encoded_packets} sent_packets={sent_packets} error={error}",
              send_errors
            );
          }
        }
        sequence = sequence.wrapping_add(1);
        frame.samples.clear();
        let _ = free_frame_tx.try_send(frame.samples);
      }
      tracing::info!(
        target: "audio::encode",
        "[audio:encode] voice encoder thread stopped: uptime_ms={} encoded_packets={encoded_packets} sent_packets={sent_packets} send_errors={send_errors} encode_errors={encode_errors} suppressed_frames={suppressed_frames} next_sequence={sequence}",
        started_at.elapsed().as_millis()
      );
    })
    .map_err(|error| VoiceError::new(format!("Failed to start encoder thread: {error}")))
}

fn warn_local_voice_input_idle(
  control: &VoiceControlState,
  now: Instant,
  last_input_frame_at: Instant,
  input_frames: u64,
  sent_packets: u64,
  suppressed_frames: u64,
  last_warn_at: &mut Option<Instant>,
) {
  if !control.can_transmit() {
    return;
  }
  let idle = now.duration_since(last_input_frame_at);
  if idle < LOCAL_VOICE_INPUT_IDLE_WARN_AFTER {
    return;
  }
  if last_warn_at.is_some_and(|warned_at| now.duration_since(warned_at) < LOCAL_VOICE_IDLE_WARN_REPEAT) {
    return;
  }
  *last_warn_at = Some(now);
  tracing::warn!(
    target: "audio::encode",
    "[audio:encode] no local voice input frames reached encoder for {}ms while transmit is allowed: input_frames={} sent_packets={} suppressed_frames={} muted={} deafened={} push_to_talk={} push_to_talk_active={} threshold={:.3}",
    idle.as_millis(),
    input_frames,
    sent_packets,
    suppressed_frames,
    control.muted.load(Ordering::Relaxed),
    control.deafened.load(Ordering::Relaxed),
    control.push_to_talk,
    control.push_to_talk_active.load(Ordering::Relaxed),
    control.voice_activation_threshold()
  );
}

#[allow(clippy::too_many_arguments)]
fn warn_local_voice_send_idle(
  control: &VoiceControlState,
  now: Instant,
  last_sent_at: Instant,
  input_frames: u64,
  encoded_packets: u64,
  sent_packets: u64,
  suppressed_frames: u64,
  vad_hold_frames: u8,
  last_warn_at: &mut Option<Instant>,
) {
  let idle = now.duration_since(last_sent_at);
  if idle < LOCAL_VOICE_SEND_IDLE_WARN_AFTER {
    return;
  }
  if last_warn_at.is_some_and(|warned_at| now.duration_since(warned_at) < LOCAL_VOICE_IDLE_WARN_REPEAT) {
    return;
  }
  *last_warn_at = Some(now);
  tracing::warn!(
    target: "audio::encode",
    "[audio:encode] no local voice packets sent for {}ms while input frames are arriving: input_frames={} encoded_packets={} sent_packets={} suppressed_frames={} muted={} deafened={} push_to_talk={} push_to_talk_active={} threshold={:.3} vad_hold_frames={} note=\"frames are being suppressed before Opus encode, usually by voice activation\"",
    idle.as_millis(),
    input_frames,
    encoded_packets,
    sent_packets,
    suppressed_frames,
    control.muted.load(Ordering::Relaxed),
    control.deafened.load(Ordering::Relaxed),
    control.push_to_talk,
    control.push_to_talk_active.load(Ordering::Relaxed),
    control.voice_activation_threshold(),
    vad_hold_frames
  );
}

struct InputCaptureState {
  channels: usize,
  resampler: NearestResampler,
  capture_frame: VecDeque<f32>,
  process_frame: Vec<f32>,
  opus_frame: VecDeque<f32>,
  frame_tx: SyncSender<EncodeFrame>,
  free_frame_rx: Receiver<Vec<f32>>,
  spare_frame: Option<Vec<f32>>,
  control: Arc<VoiceControlState>,
  stop: Arc<AtomicBool>,
  callback_failed: bool,
  processor: CaptureProcessor,
}

struct EncodeFrame {
  samples: Vec<f32>,
  force_transmit: bool,
}

impl InputCaptureState {
  fn new(
    channels: usize,
    sample_rate: u32,
    frame_tx: SyncSender<EncodeFrame>,
    free_frame_rx: Receiver<Vec<f32>>,
    control: Arc<VoiceControlState>,
    stop: Arc<AtomicBool>,
  ) -> Self {
    Self {
      channels,
      resampler: NearestResampler::new(sample_rate, SAMPLE_RATE),
      capture_frame: VecDeque::with_capacity(PROCESS_FRAME_SIZE * CHANNELS),
      process_frame: Vec::with_capacity(PROCESS_FRAME_SIZE * CHANNELS),
      opus_frame: VecDeque::with_capacity(OPUS_FRAME_SIZE * CHANNELS),
      frame_tx,
      free_frame_rx,
      spare_frame: None,
      control,
      stop,
      callback_failed: false,
      processor: CaptureProcessor::default(),
    }
  }

  fn push_catching<T>(&mut self, data: &[T])
  where
    T: Sample,
    f32: cpal::FromSample<T>,
  {
    if self.callback_failed || self.stop.load(Ordering::Relaxed) {
      return;
    }

    CATCHING_INPUT_CAPTURE_CALLBACK_PANIC.with(|depth| depth.set(depth.get().saturating_add(1)));
    let result = catch_unwind(AssertUnwindSafe(|| self.push(data)));
    CATCHING_INPUT_CAPTURE_CALLBACK_PANIC.with(|depth| depth.set(depth.get().saturating_sub(1)));

    if result.is_err() {
      self.callback_failed = true;
      self.capture_frame.clear();
      self.process_frame.clear();
      self.opus_frame.clear();
      tracing::error!(target: "audio::encode", "[audio:encode] input capture callback panicked; disabling voice capture until restart");
    }
  }

  fn push<T>(&mut self, data: &[T])
  where
    T: Sample,
    f32: cpal::FromSample<T>,
  {
    if self.stop.load(Ordering::Relaxed) {
      self.capture_frame.clear();
      self.process_frame.clear();
      self.opus_frame.clear();
      return;
    }

    let can_transmit_voice = self.control.can_transmit();
    if !can_transmit_voice {
      self.capture_frame.clear();
      self.opus_frame.clear();
      return;
    }

    for input_frame in data.chunks(self.channels) {
      let mono = if can_transmit_voice {
        input_frame.iter().map(|sample| sample.to_sample::<f32>()).sum::<f32>() / input_frame.len().max(1) as f32
      } else {
        0.0
      };

      self.resampler.push(mono.clamp(-1.0, 1.0), |sample| {
        self.capture_frame.push_back(sample);
      });
      trim_front_samples(&mut self.capture_frame, MAX_CAPTURE_QUEUE_SAMPLES);

      while self.capture_frame.len() >= PROCESS_FRAME_SIZE {
        self.process_frame.clear();
        self
          .process_frame
          .extend(self.capture_frame.iter().take(PROCESS_FRAME_SIZE).copied());
        if !self
          .processor
          .process_capture_frame(&mut self.process_frame, &self.control)
        {
          break;
        }
        pop_front_samples(&mut self.capture_frame, PROCESS_FRAME_SIZE);
        self.opus_frame.extend(self.process_frame.iter().copied());
        trim_front_samples(&mut self.opus_frame, MAX_OPUS_QUEUE_SAMPLES);
      }

      while self.opus_frame.len() >= OPUS_FRAME_SIZE {
        let Some(mut frame) = self.take_frame_buffer() else {
          break;
        };
        frame.extend(self.opus_frame.iter().take(OPUS_FRAME_SIZE).copied());

        match self.frame_tx.try_send(EncodeFrame {
          samples: frame,
          force_transmit: false,
        }) {
          Ok(()) => {
            pop_front_samples(&mut self.opus_frame, OPUS_FRAME_SIZE);
          }
          Err(TrySendError::Disconnected(frame)) => {
            self.recycle_frame_buffer(frame.samples);
            pop_front_samples(&mut self.opus_frame, OPUS_FRAME_SIZE);
            break;
          }
          Err(TrySendError::Full(frame)) => {
            self.recycle_frame_buffer(frame.samples);
            break;
          }
        }
      }
    }
  }

  fn take_frame_buffer(&mut self) -> Option<Vec<f32>> {
    let mut frame = match self.spare_frame.take() {
      Some(frame) => frame,
      None => self.free_frame_rx.try_recv().ok()?,
    };
    frame.clear();
    Some(frame)
  }

  fn recycle_frame_buffer(&mut self, mut frame: Vec<f32>) {
    frame.clear();
    if self.spare_frame.is_none() {
      self.spare_frame = Some(frame);
    }
  }
}

fn pop_front_samples(samples: &mut VecDeque<f32>, count: usize) {
  pop_front_values(samples, count);
}

fn pop_front_values<T>(values: &mut VecDeque<T>, count: usize) {
  for _ in 0..count.min(values.len()) {
    values.pop_front();
  }
}

fn trim_front_samples(samples: &mut VecDeque<f32>, max_len: usize) {
  trim_front_values(samples, max_len);
}

fn trim_front_values<T>(values: &mut VecDeque<T>, max_len: usize) {
  let overflow = values.len().saturating_sub(max_len);
  pop_front_values(values, overflow);
}

#[derive(Default)]
struct CaptureProcessor {
  processed_frame: Vec<f32>,
}

#[derive(Default)]
struct VoiceActivationGate {
  hold_frames: u8,
}

impl VoiceActivationGate {
  fn should_transmit(&mut self, control: &VoiceControlState, frame: &[f32]) -> bool {
    if control.outgoing_sound_active() {
      return true;
    }

    self.should_transmit_level(
      !control.push_to_talk,
      rms_to_perceptual(rms(frame)),
      control.voice_activation_threshold(),
    )
  }

  fn should_transmit_level(&mut self, enabled: bool, level: f32, threshold: f32) -> bool {
    if !enabled {
      return true;
    }

    let active = level >= threshold;
    if active {
      self.hold_frames = VOICE_ACTIVATION_HOLD_FRAMES;
      return true;
    }

    if self.hold_frames > 0 {
      self.hold_frames -= 1;
      return true;
    }

    false
  }
}

impl CaptureProcessor {
  fn process_capture_frame(&mut self, frame: &mut [f32], control: &VoiceControlState) -> bool {
    if let Some(audio_processing) = control.audio_processing()
      && let Ok(mut audio_processing) = audio_processing.try_lock()
    {
      let src = [&frame[..]];
      self.processed_frame.resize(frame.len(), 0.0);
      let mut dest = [&mut self.processed_frame[..]];
      if audio_processing.process_capture_f32(&src, &mut dest).is_ok() {
        frame.copy_from_slice(&self.processed_frame);
      }
    } else if control.audio_processing().is_some() {
      return false;
    }

    for sample in frame {
      *sample = sample.clamp(-1.0, 1.0);
    }

    true
  }
}

#[derive(Default)]
struct NormalizationState {
  gain: f32,
}

impl NormalizationState {
  fn apply(&mut self, frame: &mut [f32], target: f32) {
    let level = rms(frame);
    if level < 0.001 || target <= 0.0 {
      self.gain = lerp(self.gain.max(1.0), 1.0, 0.08);
      return;
    }

    let desired = (target / level).clamp(0.25, 8.0);
    let current = if self.gain <= 0.0 {
      desired
    } else {
      lerp(self.gain, desired, 0.12)
    };
    self.gain = current;
    apply_gain(frame, current);
  }
}

fn build_output_stream(
  settings: &AppSettings,
  control: Arc<VoiceControlState>,
  mixer: Arc<Mutex<VoiceMixer>>,
) -> Result<cpal::Stream, VoiceError> {
  let Some(device) = audio_devices::output_device(&settings.audio_output_device) else {
    return Err(VoiceError::new("No output device available."));
  };

  let supported_config = device
    .default_output_config()
    .map_err(|error| VoiceError::new(format!("Failed to read output config: {error}")))?;
  let sample_format = supported_config.sample_format();
  let config = low_latency_stream_config(&supported_config);

  let stream = match sample_format {
    SampleFormat::F32 => build_output_stream_for::<f32>(&device, config, control, mixer),
    SampleFormat::F64 => build_output_stream_for::<f64>(&device, config, control, mixer),
    SampleFormat::I8 => build_output_stream_for::<i8>(&device, config, control, mixer),
    SampleFormat::I16 => build_output_stream_for::<i16>(&device, config, control, mixer),
    SampleFormat::I24 => build_output_stream_for::<cpal::I24>(&device, config, control, mixer),
    SampleFormat::I32 => build_output_stream_for::<i32>(&device, config, control, mixer),
    SampleFormat::I64 => build_output_stream_for::<i64>(&device, config, control, mixer),
    SampleFormat::U8 => build_output_stream_for::<u8>(&device, config, control, mixer),
    SampleFormat::U16 => build_output_stream_for::<u16>(&device, config, control, mixer),
    SampleFormat::U24 => build_output_stream_for::<cpal::U24>(&device, config, control, mixer),
    SampleFormat::U32 => build_output_stream_for::<u32>(&device, config, control, mixer),
    SampleFormat::U64 => build_output_stream_for::<u64>(&device, config, control, mixer),
    _ => Err(VoiceError::new("Unsupported output sample format.")),
  }?;

  stream
    .play()
    .map_err(|error| VoiceError::new(format!("Failed to start output stream: {error}")))?;
  Ok(stream)
}

fn build_output_stream_for<T>(
  device: &cpal::Device,
  config: cpal::StreamConfig,
  control: Arc<VoiceControlState>,
  mixer: Arc<Mutex<VoiceMixer>>,
) -> Result<cpal::Stream, VoiceError>
where
  T: cpal::SizedSample + cpal::FromSample<f32>,
{
  let channels = usize::from(config.channels.max(1));
  let sample_rate = config.sample_rate;
  let mut state = OutputRenderState::new(channels, sample_rate, control, mixer);

  device
    .build_output_stream::<T, _, _>(
      config,
      move |data, _| state.render(data),
      move |error| tracing::warn!(target: "audio::decode", "[audio:decode] output stream error: {error}"),
      None,
    )
    .map_err(|error| VoiceError::new(format!("Failed to build output stream: {error}")))
}

struct OutputRenderState {
  channels: usize,
  output_rate: u32,
  control: Arc<VoiceControlState>,
  mixer: Arc<Mutex<VoiceMixer>>,
  output: Vec<f32>,
  source: Vec<f32>,
  render_frame: VecDeque<f32>,
  render_process_frame: Vec<f32>,
  render_processed_frame: Vec<f32>,
  source_cache: VecDeque<f32>,
  source_phase: f64,
  last_deafened: bool,
}

impl OutputRenderState {
  fn new(channels: usize, output_rate: u32, control: Arc<VoiceControlState>, mixer: Arc<Mutex<VoiceMixer>>) -> Self {
    let last_deafened = control.deafened.load(Ordering::Relaxed);
    Self {
      channels,
      output_rate,
      control,
      mixer,
      output: Vec::new(),
      source: Vec::new(),
      render_frame: VecDeque::with_capacity(PROCESS_FRAME_SIZE),
      render_process_frame: Vec::with_capacity(PROCESS_FRAME_SIZE),
      render_processed_frame: Vec::with_capacity(PROCESS_FRAME_SIZE),
      source_cache: VecDeque::new(),
      source_phase: 0.0,
      last_deafened,
    }
  }

  fn render<T>(&mut self, data: &mut [T])
  where
    T: Sample + cpal::FromSample<f32>,
  {
    let deafened = self.control.deafened.load(Ordering::Relaxed);
    if deafened != self.last_deafened {
      self.source_cache.clear();
      self.render_frame.clear();
      self.last_deafened = deafened;
    }
    let include_voice = !deafened;

    let frames = data.len() / self.channels.max(1);
    if frames == 0 {
      return;
    }

    self.render_mono(frames, include_voice);
    for (output_frame, sample) in data.chunks_mut(self.channels).zip(self.output.iter().copied()) {
      let converted = sample.to_sample::<T>();
      for channel in output_frame {
        *channel = converted;
      }
    }
  }

  fn render_mono(&mut self, frames: usize, include_voice: bool) {
    self.output.resize(frames, 0.0);

    if self.output_rate == SAMPLE_RATE {
      mix_samples_nonblocking(&self.mixer, &mut self.output, include_voice);
      self.queue_render_output();
      return;
    }

    let step = SAMPLE_RATE as f64 / self.output_rate.max(1) as f64;
    let needed_index = (self.source_phase + step * frames.saturating_sub(1) as f64).floor() as usize + 1;
    while self.source_cache.len() <= needed_index {
      let needed = needed_index + 1 - self.source_cache.len();
      self.source.resize(needed, 0.0);
      mix_samples_nonblocking(&self.mixer, &mut self.source, include_voice);
      self.queue_render_source();
      self.source_cache.extend(self.source.iter().copied());
    }

    for sample in &mut self.output {
      let index = self.source_phase.floor() as usize;
      let fraction = (self.source_phase - index as f64) as f32;
      let current = *self.source_cache.get(index).unwrap_or(&0.0);
      let next = *self.source_cache.get(index + 1).unwrap_or(&current);
      *sample = lerp(current, next, fraction);
      self.source_phase += step;
    }

    let consumed = self.source_phase.floor() as usize;
    for _ in 0..consumed.min(self.source_cache.len()) {
      self.source_cache.pop_front();
    }
    self.source_phase -= consumed as f64;
  }

  fn queue_render_output(&mut self) {
    if self.control.audio_processing().is_none() {
      return;
    }

    self.render_frame.extend(self.output.iter().copied());
    trim_front_samples(&mut self.render_frame, MAX_RENDER_QUEUE_SAMPLES);
    self.process_queued_render_frames();
  }

  fn queue_render_source(&mut self) {
    if self.control.audio_processing().is_none() {
      return;
    }

    self.render_frame.extend(self.source.iter().copied());
    trim_front_samples(&mut self.render_frame, MAX_RENDER_QUEUE_SAMPLES);
    self.process_queued_render_frames();
  }

  fn process_queued_render_frames(&mut self) {
    let Some(audio_processing) = self.control.audio_processing() else {
      return;
    };

    while self.render_frame.len() >= PROCESS_FRAME_SIZE {
      let Ok(mut audio_processing) = audio_processing.try_lock() else {
        break;
      };

      self.render_process_frame.clear();
      self
        .render_process_frame
        .extend(self.render_frame.iter().take(PROCESS_FRAME_SIZE).copied());

      let src = [&self.render_process_frame[..]];
      self.render_processed_frame.resize(self.render_process_frame.len(), 0.0);
      let mut dest = [&mut self.render_processed_frame[..]];
      let _ = audio_processing.process_render_f32(&src, &mut dest);
      pop_front_samples(&mut self.render_frame, PROCESS_FRAME_SIZE);
    }
  }
}

fn mix_samples_nonblocking(mixer: &Arc<Mutex<VoiceMixer>>, output: &mut [f32], include_voice: bool) {
  let Ok(mut mixer) = mixer.try_lock() else {
    output.fill(0.0);
    return;
  };

  mixer.mix_samples(output, include_voice);
}

#[derive(Default)]
struct VoiceMixer {
  streams: HashMap<AudioStreamId, PcmStream>,
  volumes: HashMap<AudioStreamId, f32>,
  frame_pool: Vec<Vec<f32>>,
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
enum AudioStreamId {
  Voice(UserId),
  Stream(UserId),
  LocalNotification,
}

impl VoiceMixer {
  fn set_user_volume(&mut self, user_id: UserId, volume_percent: i32) {
    self.set_volume_for_stream_id(AudioStreamId::Voice(user_id), volume_percent);
  }

  fn set_stream_volume(&mut self, user_id: UserId, volume_percent: i32) {
    self.set_volume_for_stream_id(AudioStreamId::Stream(user_id), volume_percent);
  }

  fn set_volume_for_stream_id(&mut self, stream_id: AudioStreamId, volume_percent: i32) {
    let volume_percent = volume_percent.clamp(0, 100);
    if volume_percent == 100 {
      self.volumes.remove(&stream_id);
    } else {
      self.volumes.insert(stream_id, volume_percent as f32 / 100.0);
    }
  }

  fn push_frame(&mut self, stream_id: AudioStreamId, pcm: Vec<f32>) {
    let max_frames = max_frames_for_stream_id(stream_id);
    let dropped = {
      let stream = self.streams.entry(stream_id).or_default();
      let dropped = if stream.frames.len() >= max_frames {
        stream.frames.pop_front()
      } else {
        None
      };
      stream.frames.push_back(pcm);
      dropped
    };
    if let Some(frame) = dropped {
      self.recycle_frame(frame);
    }
  }

  fn mix_samples(&mut self, output: &mut [f32], include_voice: bool) {
    output.fill(0.0);

    for (stream_id, stream) in &mut self.streams {
      if !include_voice && matches!(stream_id, AudioStreamId::Voice(_)) {
        continue;
      }
      let volume = self.volumes.get(stream_id).copied().unwrap_or(1.0);
      for sample in output.iter_mut() {
        if let Some(next) = stream.next_sample() {
          *sample += next * volume;
        }
      }
    }

    let mut recycled = Vec::new();
    for stream in self.streams.values_mut() {
      stream.drain_recycled_frames(&mut recycled);
    }
    self.streams.retain(|_, stream| stream.has_audio());
    for frame in recycled {
      self.recycle_frame(frame);
    }

    for sample in output {
      *sample = sample.clamp(-1.0, 1.0);
    }
  }

  fn clear_voice_audio(&mut self) {
    let stream_ids = self
      .streams
      .keys()
      .copied()
      .filter(|stream_id| matches!(stream_id, AudioStreamId::Voice(_)))
      .collect::<Vec<_>>();
    for stream_id in stream_ids {
      if let Some(stream) = self.streams.remove(&stream_id) {
        self.recycle_stream(stream);
      }
    }
  }

  fn clear_voice_audio_for_user(&mut self, user_id: UserId) -> bool {
    if let Some(stream) = self.streams.remove(&AudioStreamId::Voice(user_id)) {
      self.recycle_stream(stream);
      true
    } else {
      false
    }
  }

  fn clear_stream_audio(&mut self, user_id: UserId) {
    if let Some(stream) = self.streams.remove(&AudioStreamId::Stream(user_id)) {
      self.recycle_stream(stream);
    }
  }

  fn clear_local_notification_audio(&mut self) {
    if let Some(stream) = self.streams.remove(&AudioStreamId::LocalNotification) {
      self.recycle_stream(stream);
    }
  }

  fn clear_all_stream_audio(&mut self) {
    let stream_ids = self
      .streams
      .keys()
      .copied()
      .filter(|stream_id| matches!(stream_id, AudioStreamId::Stream(_)))
      .collect::<Vec<_>>();
    for stream_id in stream_ids {
      if let Some(stream) = self.streams.remove(&stream_id) {
        self.recycle_stream(stream);
      }
    }
  }

  fn take_frame_buffer(&mut self, capacity: usize) -> Vec<f32> {
    let capacity = capacity.max(OPUS_FRAME_SIZE * CHANNELS);
    if let Some(index) = self.frame_pool.iter().position(|frame| frame.capacity() >= capacity) {
      let mut frame = self.frame_pool.swap_remove(index);
      frame.clear();
      frame
    } else {
      Vec::with_capacity(capacity)
    }
  }

  fn recycle_frame(&mut self, mut frame: Vec<f32>) {
    frame.clear();
    self.frame_pool.push(frame);
  }

  fn recycle_stream(&mut self, mut stream: PcmStream) {
    for frame in stream.frames.drain(..) {
      self.recycle_frame(frame);
    }
    if !stream.current.is_empty() {
      self.recycle_frame(stream.current);
    }
    for frame in stream.recycled.drain(..) {
      self.recycle_frame(frame);
    }
  }
}

fn max_frames_for_stream_id(stream_id: AudioStreamId) -> usize {
  match stream_id {
    AudioStreamId::LocalNotification => MAX_LOCAL_NOTIFICATION_FRAMES,
    AudioStreamId::Voice(_) | AudioStreamId::Stream(_) => MAX_PCM_FRAMES_PER_USER,
  }
}

#[derive(Default)]
struct PcmStream {
  frames: VecDeque<Vec<f32>>,
  current: Vec<f32>,
  position: usize,
  started: bool,
  recycled: Vec<Vec<f32>>,
}

impl PcmStream {
  fn next_sample(&mut self) -> Option<f32> {
    if self.position >= self.current.len() {
      if !self.current.is_empty() {
        self.recycled.push(std::mem::take(&mut self.current));
      }
      if !self.started && self.frames.len() < MIN_PCM_FRAMES_BEFORE_PLAYOUT {
        return None;
      }

      self.current = self.frames.pop_front()?;
      self.position = 0;
      self.started = true;
    }

    let sample = self.current[self.position];
    self.position += 1;
    Some(sample)
  }

  fn has_audio(&self) -> bool {
    self.position < self.current.len() || !self.frames.is_empty()
  }

  fn drain_recycled_frames(&mut self, recycled: &mut Vec<Vec<f32>>) {
    recycled.append(&mut self.recycled);
  }
}

struct DecodeStream {
  decoder: Decoder,
  channels: usize,
  next_sequence: Option<u16>,
  late_sequence_drop_count: u8,
  normalizer: NormalizationState,
  scratch: Vec<f32>,
}

impl DecodeStream {
  fn new(channels: Channels) -> Result<Self, opus::Error> {
    Ok(Self {
      decoder: Decoder::new(SAMPLE_RATE, channels)?,
      channels: match channels {
        Channels::Mono => CHANNELS,
        Channels::Stereo => STREAM_CHANNELS,
      },
      next_sequence: None,
      late_sequence_drop_count: 0,
      normalizer: NormalizationState::default(),
      scratch: Vec::new(),
    })
  }

  fn decode_into(&mut self, sequence: u16, opus: &[u8], decoded: &mut Vec<f32>) -> Result<(), opus::Error> {
    decoded.clear();

    if let Some(expected) = self.next_sequence {
      let delta = seq_delta(expected, sequence);
      if delta < 0 {
        self.late_sequence_drop_count = self.late_sequence_drop_count.saturating_add(1);
        if should_reset_late_voice_sequence(delta, self.late_sequence_drop_count) {
          self.decoder.reset_state()?;
          self.next_sequence = None;
        } else {
          return Ok(());
        }
      } else if delta > MAX_PLC_FRAMES {
        self.decoder.reset_state()?;
      } else {
        for _ in 0..delta {
          self.decode_frame_into(&[], decoded)?;
        }
      }

      if self.next_sequence.is_none() {
        self.decode_frame_into(opus, decoded)?;
        self.next_sequence = Some(sequence.wrapping_add(1));
        self.late_sequence_drop_count = 0;
        return Ok(());
      }
    }

    self.decode_frame_into(opus, decoded)?;
    self.next_sequence = Some(sequence.wrapping_add(1));
    self.late_sequence_drop_count = 0;
    Ok(())
  }

  fn decode_stereo_downmix_into(&mut self, opus: &[u8], mono: &mut Vec<f32>) -> Result<(), opus::Error> {
    mono.clear();
    self.scratch.resize(OPUS_FRAME_SIZE * self.channels, 0.0);
    let samples = self.decoder.decode_float(opus, &mut self.scratch, false)?;
    let len = samples * self.channels;
    mono.reserve(samples);
    for frame in self.scratch[..len].chunks(STREAM_CHANNELS) {
      let left = frame.first().copied().unwrap_or(0.0);
      let right = frame.get(1).copied().unwrap_or(left);
      mono.push(((left + right) * 0.5).clamp(-1.0, 1.0));
    }
    Ok(())
  }

  fn decode_frame_into(&mut self, opus: &[u8], decoded: &mut Vec<f32>) -> Result<(), opus::Error> {
    let start = decoded.len();
    decoded.resize(start + OPUS_FRAME_SIZE * self.channels, 0.0);
    let samples = self.decoder.decode_float(opus, &mut decoded[start..], false)?;
    decoded.truncate(start + samples * self.channels);
    Ok(())
  }

  fn apply_normalization(&mut self, pcm: &mut [f32], target: f32) {
    self.normalizer.apply(pcm, target);
  }

  fn reset_normalization(&mut self) {
    self.normalizer = NormalizationState::default();
  }
}

struct NearestResampler {
  source_rate: u32,
  target_rate: u32,
  credit: u32,
}

impl NearestResampler {
  fn new(source_rate: u32, target_rate: u32) -> Self {
    Self {
      source_rate: source_rate.max(1),
      target_rate: target_rate.max(1),
      credit: 0,
    }
  }

  fn push(&mut self, sample: f32, mut emit: impl FnMut(f32)) {
    self.credit = self.credit.saturating_add(self.target_rate);
    while self.credit >= self.source_rate {
      emit(sample);
      self.credit -= self.source_rate;
    }
  }
}

fn seq_delta(expected: u16, actual: u16) -> i16 {
  actual.wrapping_sub(expected) as i16
}

fn should_reset_late_voice_sequence(delta: i16, consecutive_late_packets: u8) -> bool {
  delta < -MAX_LATE_VOICE_FRAMES_BEFORE_RESET && consecutive_late_packets >= LATE_VOICE_PACKETS_BEFORE_RESET
}

fn rms(samples: &[f32]) -> f32 {
  if samples.is_empty() {
    return 0.0;
  }

  let sum = samples.iter().map(|sample| sample * sample).sum::<f32>();
  (sum / samples.len() as f32).sqrt()
}

fn rms_to_perceptual(rms: f32) -> f32 {
  if rms < 0.001 {
    return 0.0;
  }

  ((20.0 * rms.log10() + 60.0) / 60.0).clamp(0.0, 1.0)
}

fn activation_threshold(level: i32) -> f32 {
  (level.clamp(0, 100) as f32) / 100.0
}

fn normalize_target(level: i32) -> f32 {
  let value = (level.clamp(0, 100) as f32) / 100.0;
  (value * value * value).clamp(0.0, 1.0)
}

fn apply_gain(frame: &mut [f32], gain: f32) {
  for sample in frame {
    *sample *= gain;
  }
}

fn apply_outgoing_sound_volume(samples: &mut [f32], volume_percent: i32) {
  let volume_gain = (volume_percent.clamp(0, 100) as f32) / 100.0;
  let peak = samples.iter().map(|sample| sample.abs()).fold(0.0_f32, f32::max);
  let peak_limit = OUTGOING_SOUND_MAX_PEAK * volume_gain;
  let gain = if peak > peak_limit && peak > 0.0 {
    peak_limit / peak
  } else {
    volume_gain
  };
  apply_gain(samples, gain);
}

fn apply_outgoing_sound_fade(samples: &mut [f32]) {
  let fade_samples = OUTGOING_SOUND_FADE_SAMPLES.min(samples.len() / 2);
  if fade_samples == 0 {
    return;
  }

  for index in 0..fade_samples {
    let fade_in = (index + 1) as f32 / fade_samples as f32;
    samples[index] *= fade_in;
    let tail_index = samples.len() - 1 - index;
    samples[tail_index] *= fade_in;
  }
}

fn lerp(from: f32, to: f32, amount: f32) -> f32 {
  from + (to - from) * amount.clamp(0.0, 1.0)
}

#[cfg(test)]
#[path = "../../tests/unit/services/voice.rs"]
mod tests;
