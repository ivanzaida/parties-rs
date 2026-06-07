use std::{
  collections::{HashMap, VecDeque},
  fmt,
  sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU32, Ordering},
    mpsc::{self, Receiver, SyncSender, TrySendError},
  },
  thread::{self, JoinHandle},
  time::Duration,
};

use cpal::{
  BufferSize, Sample, SampleFormat, SupportedBufferSize,
  traits::{DeviceTrait, StreamTrait},
};
use opus::{Application, Bitrate, Channels, Decoder, Encoder, Signal};
use sonora::{
  AudioProcessing, Config, StreamConfig,
  config::{
    AdaptiveDigital, CaptureLevelAdjustment, EchoCanceller, GainController2, HighPassFilter, MaxProcessingRate,
    NoiseSuppression, NoiseSuppressionLevel, Pipeline,
  },
};

use super::audio_devices;
use crate::{
  network::{
    protocol::{UserId, data::ForwardedVoicePacket},
    server::Server,
  },
  storage::AppSettings,
};

const SAMPLE_RATE: u32 = 48_000;
const CHANNELS: usize = 1;
const PROCESS_FRAME_SIZE: usize = 480;
const OPUS_FRAME_SIZE: usize = 960;
const OPUS_BITRATE: i32 = 32_000;
const MAX_OPUS_PACKET: usize = 512;
const INPUT_FRAME_QUEUE: usize = 8;
const INPUT_FRAME_POOL: usize = INPUT_FRAME_QUEUE + 2;
const MAX_PCM_FRAMES_PER_USER: usize = 12;
const MIN_PCM_FRAMES_BEFORE_PLAYOUT: usize = 2;
const MAX_PLC_FRAMES: i16 = 3;
const MAX_CAPTURE_QUEUE_SAMPLES: usize = OPUS_FRAME_SIZE * 5;
const MAX_OPUS_QUEUE_SAMPLES: usize = OPUS_FRAME_SIZE * INPUT_FRAME_POOL;
const MAX_RENDER_QUEUE_SAMPLES: usize = PROCESS_FRAME_SIZE * 10;
const STREAM_BUFFER_TARGET_MS: u32 = 20;
const VOICE_ACTIVATION_HOLD_FRAMES: u8 = 12;

pub type LocalVoiceCallback = Arc<dyn Fn() + Send + Sync + 'static>;

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

pub struct VoiceEngine {
  _input_stream: Option<cpal::Stream>,
  _output_stream: Option<cpal::Stream>,
  encoder_thread: Option<JoinHandle<()>>,
  stop: Arc<AtomicBool>,
  control: Arc<VoiceControlState>,
  mixer: Arc<Mutex<VoiceMixer>>,
  decoders: HashMap<UserId, DecodeStream>,
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

    let output_stream = build_output_stream(&settings, control.clone(), mixer.clone()).ok();
    let input_result = build_input_path(server, &settings, control.clone(), stop.clone(), on_local_voice);
    let input_error = input_result.as_ref().err().map(ToString::to_string);
    let (input_stream, encoder_thread) = input_result.unwrap_or((None, None));

    if input_stream.is_none() && output_stream.is_none() {
      return Err(VoiceError::new(
        input_error.unwrap_or_else(|| "No usable audio input or output device.".to_owned()),
      ));
    }

    Ok(Self {
      _input_stream: input_stream,
      _output_stream: output_stream,
      encoder_thread,
      stop,
      control,
      mixer,
      decoders: HashMap::new(),
    })
  }

  pub fn set_voice_state(&self, muted: bool, deafened: bool) {
    self.control.muted.store(muted, Ordering::Relaxed);
    self.control.deafened.store(deafened, Ordering::Relaxed);
    if deafened {
      self.mixer.lock().expect("voice mixer lock poisoned").clear();
    }
  }

  pub fn set_voice_activation_threshold(&self, value: i32) {
    self.control.set_voice_activation_threshold(value);
  }

  pub fn set_push_to_talk_active(&self, active: bool) {
    self.control.set_push_to_talk_active(active);
  }

  pub fn push_packet(&mut self, packet: ForwardedVoicePacket) -> bool {
    if self.control.deafened.load(Ordering::Relaxed) {
      return false;
    }

    let stream = match self.decoders.entry(packet.sender_id) {
      std::collections::hash_map::Entry::Occupied(entry) => entry.into_mut(),
      std::collections::hash_map::Entry::Vacant(entry) => match DecodeStream::new() {
        Ok(stream) => entry.insert(stream),
        Err(_) => return false,
      },
    };

    let Ok((pcm, rms)) = stream.decode(packet.sequence, &packet.opus) else {
      return false;
    };

    if !pcm.is_empty() {
      self
        .mixer
        .lock()
        .expect("voice mixer lock poisoned")
        .push_frame(packet.sender_id, pcm);
    }

    rms > 0.001
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
  voice_normalization: bool,
  voice_normalization_target: f32,
  voice_activation: bool,
  voice_activation_threshold: AtomicU32,
  push_to_talk: bool,
  push_to_talk_active: AtomicBool,
  audio_processing: Option<Arc<Mutex<AudioProcessing>>>,
}

impl VoiceControlState {
  fn new(settings: &AppSettings, muted: bool, deafened: bool) -> Self {
    Self {
      muted: AtomicBool::new(muted),
      deafened: AtomicBool::new(deafened),
      voice_normalization: settings.voice_normalization,
      voice_normalization_target: normalize_target(settings.voice_normalization_target_level),
      voice_activation: settings.voice_activation,
      voice_activation_threshold: AtomicU32::new(activation_threshold(settings.voice_activation_threshold).to_bits()),
      push_to_talk: settings.push_to_talk,
      push_to_talk_active: AtomicBool::new(false),
      audio_processing: build_audio_processing(settings),
    }
  }

  fn can_transmit(&self) -> bool {
    !self.muted.load(Ordering::Relaxed)
      && !self.deafened.load(Ordering::Relaxed)
      && (!self.push_to_talk || self.push_to_talk_active.load(Ordering::Relaxed))
  }

  fn audio_processing(&self) -> Option<&Arc<Mutex<AudioProcessing>>> {
    self.audio_processing.as_ref()
  }

  fn set_voice_activation_threshold(&self, value: i32) {
    self
      .voice_activation_threshold
      .store(activation_threshold(value).to_bits(), Ordering::Relaxed);
  }

  fn set_push_to_talk_active(&self, active: bool) {
    self.push_to_talk_active.store(active, Ordering::Relaxed);
  }

  fn voice_activation_threshold(&self) -> f32 {
    f32::from_bits(self.voice_activation_threshold.load(Ordering::Relaxed))
  }
}

fn build_audio_processing(settings: &AppSettings) -> Option<Arc<Mutex<AudioProcessing>>> {
  if !settings.noise_cancellation && !settings.voice_normalization && !settings.echo_cancellation {
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
    gain_controller2: settings.voice_normalization.then(|| GainController2 {
      adaptive_digital: Some(AdaptiveDigital {
        max_gain_db: 30.0,
        initial_gain_db: normalization_initial_gain_db(settings.voice_normalization_target_level),
        ..Default::default()
      }),
      ..Default::default()
    }),
    capture_level_adjustment: settings.voice_normalization.then(|| CaptureLevelAdjustment {
      post_gain_factor: normalization_post_gain(settings.voice_normalization_target_level),
      ..Default::default()
    }),
    ..Default::default()
  };

  Some(Arc::new(Mutex::new(
    AudioProcessing::builder()
      .config(config)
      .capture_config(StreamConfig::new(SAMPLE_RATE, CHANNELS as u16))
      .render_config(StreamConfig::new(SAMPLE_RATE, CHANNELS as u16))
      .build(),
  )))
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
) -> Result<(Option<cpal::Stream>, Option<JoinHandle<()>>), VoiceError> {
  let Some(device) = audio_devices::input_device(&settings.audio_input_device) else {
    return Ok((None, None));
  };

  let supported_config = device
    .default_input_config()
    .map_err(|error| VoiceError::new(format!("Failed to read input config: {error}")))?;
  let sample_format = supported_config.sample_format();
  let config = low_latency_stream_config(&supported_config);
  let (frame_tx, frame_rx) = mpsc::sync_channel(INPUT_FRAME_QUEUE);
  let (free_frame_tx, free_frame_rx) = mpsc::sync_channel(INPUT_FRAME_POOL);
  for _ in 0..INPUT_FRAME_POOL {
    let _ = free_frame_tx.try_send(Vec::with_capacity(OPUS_FRAME_SIZE));
  }

  let input_stream = match sample_format {
    SampleFormat::F32 => build_input_stream::<f32>(&device, config, frame_tx, free_frame_rx, control.clone()),
    SampleFormat::F64 => build_input_stream::<f64>(&device, config, frame_tx, free_frame_rx, control.clone()),
    SampleFormat::I8 => build_input_stream::<i8>(&device, config, frame_tx, free_frame_rx, control.clone()),
    SampleFormat::I16 => build_input_stream::<i16>(&device, config, frame_tx, free_frame_rx, control.clone()),
    SampleFormat::I24 => build_input_stream::<cpal::I24>(&device, config, frame_tx, free_frame_rx, control.clone()),
    SampleFormat::I32 => build_input_stream::<i32>(&device, config, frame_tx, free_frame_rx, control.clone()),
    SampleFormat::I64 => build_input_stream::<i64>(&device, config, frame_tx, free_frame_rx, control.clone()),
    SampleFormat::U8 => build_input_stream::<u8>(&device, config, frame_tx, free_frame_rx, control.clone()),
    SampleFormat::U16 => build_input_stream::<u16>(&device, config, frame_tx, free_frame_rx, control.clone()),
    SampleFormat::U24 => build_input_stream::<cpal::U24>(&device, config, frame_tx, free_frame_rx, control.clone()),
    SampleFormat::U32 => build_input_stream::<u32>(&device, config, frame_tx, free_frame_rx, control.clone()),
    SampleFormat::U64 => build_input_stream::<u64>(&device, config, frame_tx, free_frame_rx, control.clone()),
    _ => Err(VoiceError::new("Unsupported input sample format.")),
  }?;

  input_stream
    .play()
    .map_err(|error| VoiceError::new(format!("Failed to start input stream: {error}")))?;

  let encoder_thread = spawn_encoder_thread(server, frame_rx, free_frame_tx, stop, on_local_voice, control)?;
  Ok((Some(input_stream), Some(encoder_thread)))
}

fn build_input_stream<T>(
  device: &cpal::Device,
  config: cpal::StreamConfig,
  frame_tx: SyncSender<Vec<f32>>,
  free_frame_rx: Receiver<Vec<f32>>,
  control: Arc<VoiceControlState>,
) -> Result<cpal::Stream, VoiceError>
where
  T: cpal::SizedSample,
  f32: cpal::FromSample<T>,
{
  let channels = usize::from(config.channels.max(1));
  let sample_rate = config.sample_rate;
  let mut state = InputCaptureState::new(channels, sample_rate, frame_tx, free_frame_rx, control);

  device
    .build_input_stream::<T, _, _>(config, move |data, _| state.push(data), move |_| {}, None)
    .map_err(|error| VoiceError::new(format!("Failed to build input stream: {error}")))
}

fn spawn_encoder_thread(
  server: Arc<Server>,
  frame_rx: Receiver<Vec<f32>>,
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
      let mut sequence = 0u16;
      let mut opus = vec![0u8; MAX_OPUS_PACKET];
      let mut voice_gate = VoiceActivationGate::default();
      while !stop.load(Ordering::Relaxed) {
        let mut frame = match frame_rx.recv_timeout(Duration::from_millis(50)) {
          Ok(frame) => frame,
          Err(mpsc::RecvTimeoutError::Timeout) => continue,
          Err(mpsc::RecvTimeoutError::Disconnected) => break,
        };

        if !voice_gate.should_transmit(&control, &frame) {
          frame.clear();
          let _ = free_frame_tx.try_send(frame);
          continue;
        }

        let len = match encoder.encode_float(&frame, &mut opus) {
          Ok(len) => len,
          Err(_) => {
            frame.clear();
            let _ = free_frame_tx.try_send(frame);
            continue;
          }
        };
        if len == 0 {
          frame.clear();
          let _ = free_frame_tx.try_send(frame);
          continue;
        }

        if server.send_voice(sequence, opus[..len].to_vec()).is_ok() {
          on_local_voice();
        }
        sequence = sequence.wrapping_add(1);
        frame.clear();
        let _ = free_frame_tx.try_send(frame);
      }
    })
    .map_err(|error| VoiceError::new(format!("Failed to start encoder thread: {error}")))
}

struct InputCaptureState {
  channels: usize,
  resampler: NearestResampler,
  capture_frame: VecDeque<f32>,
  process_frame: Vec<f32>,
  opus_frame: VecDeque<f32>,
  frame_tx: SyncSender<Vec<f32>>,
  free_frame_rx: Receiver<Vec<f32>>,
  spare_frame: Option<Vec<f32>>,
  control: Arc<VoiceControlState>,
  processor: CaptureProcessor,
}

impl InputCaptureState {
  fn new(
    channels: usize,
    sample_rate: u32,
    frame_tx: SyncSender<Vec<f32>>,
    free_frame_rx: Receiver<Vec<f32>>,
    control: Arc<VoiceControlState>,
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
      processor: CaptureProcessor::default(),
    }
  }

  fn push<T>(&mut self, data: &[T])
  where
    T: Sample,
    f32: cpal::FromSample<T>,
  {
    if !self.control.can_transmit() {
      self.capture_frame.clear();
      self.opus_frame.clear();
      return;
    }

    for input_frame in data.chunks(self.channels) {
      let mono =
        input_frame.iter().map(|sample| sample.to_sample::<f32>()).sum::<f32>() / input_frame.len().max(1) as f32;

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

        match self.frame_tx.try_send(frame) {
          Ok(()) => {}
          Err(TrySendError::Disconnected(frame)) => {
            self.recycle_frame_buffer(frame);
            pop_front_samples(&mut self.opus_frame, OPUS_FRAME_SIZE);
            break;
          }
          Err(TrySendError::Full(frame)) => self.recycle_frame_buffer(frame),
        }
        pop_front_samples(&mut self.opus_frame, OPUS_FRAME_SIZE);
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
  for _ in 0..count.min(samples.len()) {
    samples.pop_front();
  }
}

fn trim_front_samples(samples: &mut VecDeque<f32>, max_len: usize) {
  let overflow = samples.len().saturating_sub(max_len);
  pop_front_samples(samples, overflow);
}

#[derive(Default)]
struct CaptureProcessor {
  normalize_gain: f32,
  processed_frame: Vec<f32>,
}

#[derive(Default)]
struct VoiceActivationGate {
  hold_frames: u8,
}

impl VoiceActivationGate {
  fn should_transmit(&mut self, control: &VoiceControlState, frame: &[f32]) -> bool {
    self.should_transmit_level(
      control.voice_activation,
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

    if control.voice_normalization {
      self.apply_normalization(frame, control.voice_normalization_target);
    }

    for sample in frame {
      *sample = sample.clamp(-1.0, 1.0);
    }

    true
  }

  fn apply_normalization(&mut self, frame: &mut [f32], target: f32) {
    let level = rms(frame);
    if level < 0.001 || target <= 0.0 {
      self.normalize_gain = lerp(self.normalize_gain.max(1.0), 1.0, 0.08);
      return;
    }

    let desired = (target / level).clamp(0.25, 8.0);
    let current = if self.normalize_gain <= 0.0 {
      desired
    } else {
      lerp(self.normalize_gain, desired, 0.12)
    };
    self.normalize_gain = current;
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
    .build_output_stream::<T, _, _>(config, move |data, _| state.render(data), move |_| {}, None)
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
}

impl OutputRenderState {
  fn new(channels: usize, output_rate: u32, control: Arc<VoiceControlState>, mixer: Arc<Mutex<VoiceMixer>>) -> Self {
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
    }
  }

  fn render<T>(&mut self, data: &mut [T])
  where
    T: Sample + cpal::FromSample<f32>,
  {
    if self.control.deafened.load(Ordering::Relaxed) {
      self.source_cache.clear();
      self.render_frame.clear();
      self.write_silence(data);
      return;
    }

    let frames = data.len() / self.channels.max(1);
    if frames == 0 {
      return;
    }

    self.render_mono(frames);
    for (output_frame, sample) in data.chunks_mut(self.channels).zip(self.output.iter().copied()) {
      let converted = sample.to_sample::<T>();
      for channel in output_frame {
        *channel = converted;
      }
    }
  }

  fn render_mono(&mut self, frames: usize) {
    self.output.resize(frames, 0.0);

    if self.output_rate == SAMPLE_RATE {
      mix_samples_nonblocking(&self.mixer, &mut self.output);
      self.queue_render_output();
      return;
    }

    let step = SAMPLE_RATE as f64 / self.output_rate.max(1) as f64;
    let needed_index = (self.source_phase + step * frames.saturating_sub(1) as f64).floor() as usize + 1;
    while self.source_cache.len() <= needed_index {
      let needed = needed_index + 1 - self.source_cache.len();
      self.source.resize(needed, 0.0);
      mix_samples_nonblocking(&self.mixer, &mut self.source);
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

  fn write_silence<T>(&self, data: &mut [T])
  where
    T: Sample + cpal::FromSample<f32>,
  {
    let silence = 0.0_f32.to_sample::<T>();
    for sample in data {
      *sample = silence;
    }
  }
}

fn mix_samples_nonblocking(mixer: &Arc<Mutex<VoiceMixer>>, output: &mut [f32]) {
  let Ok(mut mixer) = mixer.try_lock() else {
    output.fill(0.0);
    return;
  };

  mixer.mix_samples(output);
}

#[derive(Default)]
struct VoiceMixer {
  streams: HashMap<UserId, PcmStream>,
}

impl VoiceMixer {
  fn push_frame(&mut self, user_id: UserId, pcm: Vec<f32>) {
    let stream = self.streams.entry(user_id).or_default();
    if stream.frames.len() >= MAX_PCM_FRAMES_PER_USER {
      stream.frames.pop_front();
    }
    stream.frames.push_back(pcm);
  }

  fn mix_samples(&mut self, output: &mut [f32]) {
    output.fill(0.0);

    for stream in self.streams.values_mut() {
      for sample in output.iter_mut() {
        if let Some(next) = stream.next_sample() {
          *sample += next;
        }
      }
    }

    self.streams.retain(|_, stream| stream.has_audio());

    for sample in output {
      *sample = sample.clamp(-1.0, 1.0);
    }
  }

  fn clear(&mut self) {
    self.streams.clear();
  }
}

#[derive(Default)]
struct PcmStream {
  frames: VecDeque<Vec<f32>>,
  current: Vec<f32>,
  position: usize,
  started: bool,
}

impl PcmStream {
  fn next_sample(&mut self) -> Option<f32> {
    if self.position >= self.current.len() {
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
}

struct DecodeStream {
  decoder: Decoder,
  next_sequence: Option<u16>,
}

impl DecodeStream {
  fn new() -> Result<Self, opus::Error> {
    Ok(Self {
      decoder: Decoder::new(SAMPLE_RATE, Channels::Mono)?,
      next_sequence: None,
    })
  }

  fn decode(&mut self, sequence: u16, opus: &[u8]) -> Result<(Vec<f32>, f32), opus::Error> {
    let mut decoded = Vec::new();

    if let Some(expected) = self.next_sequence {
      let delta = seq_delta(expected, sequence);
      if delta < 0 {
        return Ok((Vec::new(), 0.0));
      }

      if delta > MAX_PLC_FRAMES {
        self.decoder.reset_state()?;
      } else {
        for _ in 0..delta {
          let mut plc = vec![0.0; OPUS_FRAME_SIZE];
          let samples = self.decoder.decode_float(&[], &mut plc, false)?;
          plc.truncate(samples);
          decoded.extend(plc);
        }
      }
    }

    let mut pcm = vec![0.0; OPUS_FRAME_SIZE];
    let samples = self.decoder.decode_float(opus, &mut pcm, false)?;
    pcm.truncate(samples);
    let level = rms(&pcm);
    decoded.extend(pcm);
    self.next_sequence = Some(sequence.wrapping_add(1));
    Ok((decoded, level))
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

fn normalization_initial_gain_db(level: i32) -> f32 {
  let value = (level.clamp(0, 100) as f32) / 100.0;
  lerp(3.0, 18.0, value)
}

fn normalization_post_gain(level: i32) -> f32 {
  let value = (level.clamp(0, 100) as f32) / 100.0;
  lerp(0.5, 1.5, value)
}

fn apply_gain(frame: &mut [f32], gain: f32) {
  for sample in frame {
    *sample *= gain;
  }
}

fn lerp(from: f32, to: f32, amount: f32) -> f32 {
  from + (to - from) * amount.clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn seq_delta_handles_wraparound() {
    assert_eq!(seq_delta(10, 12), 2);
    assert_eq!(seq_delta(12, 10), -2);
    assert_eq!(seq_delta(u16::MAX, 1), 2);
  }

  #[test]
  fn nearest_resampler_keeps_nominal_rate() {
    let mut resampler = NearestResampler::new(24_000, 48_000);
    let mut output = Vec::new();
    for _ in 0..4 {
      resampler.push(1.0, |sample| output.push(sample));
    }
    assert_eq!(output.len(), 8);
  }

  #[test]
  fn pcm_stream_waits_for_playout_cushion() {
    let mut stream = PcmStream::default();
    stream.frames.push_back(vec![0.5; OPUS_FRAME_SIZE]);
    assert_eq!(stream.next_sample(), None);

    stream.frames.push_back(vec![0.25; OPUS_FRAME_SIZE]);
    assert_eq!(stream.next_sample(), Some(0.5));
  }

  #[test]
  fn low_latency_config_uses_supported_buffer_range() {
    let supported = cpal::SupportedStreamConfig::new(
      1,
      SAMPLE_RATE,
      SupportedBufferSize::Range { min: 128, max: 960 },
      SampleFormat::F32,
    );
    let config = low_latency_stream_config(&supported);

    assert_eq!(config.buffer_size, BufferSize::Fixed(960));
  }

  #[test]
  fn voice_activation_gate_holds_after_speech() {
    let mut gate = VoiceActivationGate::default();

    assert!(!gate.should_transmit_level(true, 0.1, 0.5));
    assert!(gate.should_transmit_level(true, 0.8, 0.5));

    for _ in 0..VOICE_ACTIVATION_HOLD_FRAMES {
      assert!(gate.should_transmit_level(true, 0.1, 0.5));
    }
    assert!(!gate.should_transmit_level(true, 0.1, 0.5));
  }

  #[test]
  fn normalization_raises_quiet_frames_toward_target() {
    let mut processor = CaptureProcessor::default();
    let mut frame = vec![0.02; OPUS_FRAME_SIZE];
    processor.apply_normalization(&mut frame, 0.2);
    assert!(rms(&frame) > 0.02);
  }

  #[test]
  fn normalization_apm_mappings_are_bounded() {
    assert_eq!(normalization_initial_gain_db(-1), 3.0);
    assert_eq!(normalization_initial_gain_db(101), 18.0);
    assert_eq!(normalization_post_gain(-1), 0.5);
    assert_eq!(normalization_post_gain(101), 1.5);
  }
}
