use std::io::Cursor;

use minimp3::{Decoder as Mp3Decoder, Error as Mp3Error};
use opus::{Application, Bitrate, Channels, Encoder};
use server_plugin::{
  BOT_VOICE_BITRATE_BPS, BOT_VOICE_FRAME_SAMPLES, BOT_VOICE_MAX_OPUS_PACKET_BYTES, BOT_VOICE_SAMPLE_RATE_HZ,
};
use symphonia::core::{
  audio::GenericAudioBufferRef,
  codecs::audio::AudioDecoderOptions,
  errors::Error as SymphoniaError,
  formats::{FormatOptions, TrackType, probe::Hint},
  io::MediaSourceStream,
  meta::MetadataOptions,
};

use crate::{
  queue::Track,
  sources::{
    model::{ResolvedAudio, ResolvedAudioPayload},
    registry::SourceRegistry,
  },
};

pub(crate) struct AudioFrames {
  source: AudioFrameSource,
}

impl AudioFrames {
  pub(crate) fn open(track: &mut Track, sources: &SourceRegistry) -> Result<Self, String> {
    let resolved = sources.resolve(&track.source)?;
    let opened = PcmFrameReader::from_resolved_audio(resolved)?;
    track.title = opened.title;
    Ok(Self {
      source: AudioFrameSource::DecodedPcm(opened.reader),
    })
  }

  pub(crate) fn next_frame(&mut self) -> Result<Option<Vec<f32>>, String> {
    match &mut self.source {
      AudioFrameSource::DecodedPcm(reader) => Ok(reader.next_frame()),
    }
  }
}

enum AudioFrameSource {
  DecodedPcm(PcmFrameReader),
}

struct PcmFrameReader {
  samples: Vec<f32>,
  offset: usize,
}

impl PcmFrameReader {
  fn from_resolved_audio(resolved: ResolvedAudio) -> Result<OpenedAudio, String> {
    let _source_kind = resolved.source_kind;
    let _source_url = &resolved.source_url;
    let ResolvedAudioPayload::Buffered { bytes, container_hint } = resolved.payload;
    let decoded = decode_audio_bytes(bytes, container_hint.as_deref())?;
    let samples = resample_linear(&decoded.samples, decoded.sample_rate, BOT_VOICE_SAMPLE_RATE_HZ);
    Ok(OpenedAudio {
      title: resolved.title,
      reader: Self { samples, offset: 0 },
    })
  }

  fn next_frame(&mut self) -> Option<Vec<f32>> {
    if self.offset >= self.samples.len() {
      return None;
    }

    let end = (self.offset + BOT_VOICE_FRAME_SAMPLES).min(self.samples.len());
    let mut frame = vec![0.0; BOT_VOICE_FRAME_SAMPLES];
    frame[..end - self.offset].copy_from_slice(&self.samples[self.offset..end]);
    self.offset = end;
    Some(frame)
  }
}

struct OpenedAudio {
  title: String,
  reader: PcmFrameReader,
}

struct DecodedAudio {
  samples: Vec<f32>,
  sample_rate: u32,
}

fn decode_audio_bytes(bytes: Vec<u8>, extension: Option<&str>) -> Result<DecodedAudio, String> {
  if extension.is_some_and(|extension| extension.eq_ignore_ascii_case("mp3")) {
    return decode_mp3_bytes(bytes);
  }

  let cursor = Cursor::new(bytes);
  let mss = MediaSourceStream::new(Box::new(cursor), Default::default());
  let mut hint = Hint::new();
  if let Some(extension) = extension {
    hint.with_extension(extension);
  }

  let mut format = symphonia::default::get_probe()
    .probe(&hint, mss, FormatOptions::default(), MetadataOptions::default())
    .map_err(|error| format!("failed to probe audio stream: {error}"))?;
  let track = format
    .default_track(TrackType::Audio)
    .ok_or_else(|| "audio stream did not contain a decodable track.".to_owned())?;
  let track_id = track.id;
  let codec_params = track
    .codec_params
    .as_ref()
    .ok_or_else(|| "audio track did not contain codec parameters.".to_owned())?
    .audio()
    .ok_or_else(|| "audio track did not contain audio codec parameters.".to_owned())?;
  let mut decoder = symphonia::default::get_codecs()
    .make_audio_decoder(codec_params, &AudioDecoderOptions::default())
    .map_err(|error| format!("failed to create audio decoder: {error}"))?;

  let mut output = Vec::new();
  let mut sample_rate = None;

  loop {
    let packet = match format.next_packet() {
      Ok(Some(packet)) => packet,
      Ok(None) => break,
      Err(error) => return Err(format!("failed to read audio packet: {error}")),
    };

    if packet.track_id != track_id {
      continue;
    }

    let decoded = match decoder.decode(&packet) {
      Ok(decoded) => decoded,
      Err(SymphoniaError::DecodeError(_)) => continue,
      Err(error) => return Err(format!("failed to decode audio packet: {error}")),
    };

    append_mono_samples(decoded, &mut output, &mut sample_rate)?;
  }

  let sample_rate = sample_rate.ok_or_else(|| "audio stream did not produce decoded samples.".to_owned())?;
  Ok(DecodedAudio {
    samples: output,
    sample_rate,
  })
}

fn decode_mp3_bytes(bytes: Vec<u8>) -> Result<DecodedAudio, String> {
  let mut decoder = Mp3Decoder::new(Cursor::new(bytes));
  let mut output = Vec::new();
  let mut sample_rate = None;

  loop {
    match decoder.next_frame() {
      Ok(frame) => append_mp3_frame(frame, &mut output, &mut sample_rate)?,
      Err(Mp3Error::Eof) => break,
      Err(error) => return Err(format!("failed to decode MP3 frame: {error}")),
    }
  }

  let sample_rate = sample_rate.ok_or_else(|| "MP3 stream did not produce decoded samples.".to_owned())?;
  Ok(DecodedAudio {
    samples: output,
    sample_rate,
  })
}

fn append_mp3_frame(frame: minimp3::Frame, output: &mut Vec<f32>, sample_rate: &mut Option<u32>) -> Result<(), String> {
  let frame_sample_rate = u32::try_from(frame.sample_rate).map_err(|_| "MP3 frame reported an invalid sample rate.")?;
  if let Some(existing_sample_rate) = *sample_rate {
    if frame_sample_rate != existing_sample_rate {
      return Err("MP3 stream changed sample rate mid-track.".to_owned());
    }
  } else {
    *sample_rate = Some(frame_sample_rate);
  }

  if frame_sample_rate == 0 {
    return Err("MP3 stream did not report a sample rate.".to_owned());
  }

  let channels = frame.channels.max(1);
  output.extend(frame.data.chunks(channels).map(|frame| {
    frame
      .iter()
      .copied()
      .map(|sample| sample as f32 / i16::MAX as f32)
      .sum::<f32>()
      / frame.len() as f32
  }));
  Ok(())
}

pub(crate) fn probe_decoded_sample_count(bytes: Vec<u8>, extension: Option<&str>) -> Result<usize, String> {
  decode_audio_bytes(bytes, extension).map(|decoded| decoded.samples.len())
}

fn append_mono_samples(
  decoded: GenericAudioBufferRef<'_>,
  output: &mut Vec<f32>,
  sample_rate: &mut Option<u32>,
) -> Result<(), String> {
  let spec = decoded.spec();
  if let Some(existing_sample_rate) = *sample_rate {
    if spec.rate() != existing_sample_rate {
      return Err("audio stream changed sample rate mid-track.".to_owned());
    }
  } else {
    *sample_rate = Some(spec.rate());
  }

  if spec.rate() == 0 {
    return Err("audio stream did not report a sample rate.".to_owned());
  }

  let channels = spec.channels().count().max(1);
  let mut samples = vec![0.0; decoded.samples_interleaved()];
  decoded.copy_to_slice_interleaved::<f32, _>(&mut samples);
  output.extend(
    samples
      .chunks(channels)
      .map(|frame| frame.iter().copied().sum::<f32>() / frame.len() as f32),
  );
  Ok(())
}

fn resample_linear(samples: &[f32], input_rate: u32, output_rate: u32) -> Vec<f32> {
  if input_rate == output_rate || samples.is_empty() {
    return samples.to_vec();
  }

  let output_len = samples.len() * output_rate as usize / input_rate as usize;
  let ratio = input_rate as f64 / output_rate as f64;
  (0..output_len)
    .map(|index| {
      let position = index as f64 * ratio;
      let left = position.floor() as usize;
      let right = (left + 1).min(samples.len() - 1);
      let fraction = (position - left as f64) as f32;
      samples[left] + (samples[right] - samples[left]) * fraction
    })
    .collect()
}

pub(crate) struct VoiceEncoder {
  inner: Encoder,
  output: [u8; BOT_VOICE_MAX_OPUS_PACKET_BYTES],
}

impl VoiceEncoder {
  pub(crate) fn new() -> Result<Self, opus::Error> {
    let mut inner = Encoder::new(BOT_VOICE_SAMPLE_RATE_HZ, Channels::Mono, Application::Audio)?;
    inner.set_bitrate(Bitrate::Bits(BOT_VOICE_BITRATE_BPS))?;
    Ok(Self {
      inner,
      output: [0; BOT_VOICE_MAX_OPUS_PACKET_BYTES],
    })
  }

  pub(crate) fn encode(&mut self, frame: &[f32]) -> Result<Vec<u8>, opus::Error> {
    let len = self.inner.encode_float(frame, &mut self.output)?;
    Ok(self.output[..len].to_vec())
  }
}
