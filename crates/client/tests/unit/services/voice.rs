use super::*;

#[test]
fn seq_delta_handles_wraparound() {
  assert_eq!(seq_delta(10, 12), 2);
  assert_eq!(seq_delta(12, 10), -2);
  assert_eq!(seq_delta(u16::MAX, 1), 2);
}

#[test]
fn late_voice_sequence_reset_threshold_requires_consecutive_large_late_packets() {
  assert!(!should_reset_late_voice_sequence(-1, LATE_VOICE_PACKETS_BEFORE_RESET));
  assert!(!should_reset_late_voice_sequence(
    -MAX_LATE_VOICE_FRAMES_BEFORE_RESET,
    LATE_VOICE_PACKETS_BEFORE_RESET
  ));
  assert!(!should_reset_late_voice_sequence(
    -MAX_LATE_VOICE_FRAMES_BEFORE_RESET - 1,
    LATE_VOICE_PACKETS_BEFORE_RESET - 1
  ));
  assert!(should_reset_late_voice_sequence(
    -MAX_LATE_VOICE_FRAMES_BEFORE_RESET - 1,
    LATE_VOICE_PACKETS_BEFORE_RESET
  ));
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
fn mixer_excludes_voice_but_keeps_stream_audio() {
  let mut mixer = VoiceMixer::default();
  mixer.push_frame(AudioStreamId::Voice(1), vec![0.8; OPUS_FRAME_SIZE]);
  mixer.push_frame(AudioStreamId::Voice(1), vec![0.8; OPUS_FRAME_SIZE]);
  mixer.push_frame(AudioStreamId::Stream(2), vec![0.25; OPUS_FRAME_SIZE]);
  mixer.push_frame(AudioStreamId::Stream(2), vec![0.25; OPUS_FRAME_SIZE]);

  let mut output = vec![0.0; OPUS_FRAME_SIZE];
  mixer.mix_samples(&mut output, false);

  assert!(output.iter().all(|sample| (*sample - 0.25).abs() < f32::EPSILON));
}

#[test]
fn clear_voice_audio_preserves_stream_audio() {
  let mut mixer = VoiceMixer::default();
  mixer.push_frame(AudioStreamId::Voice(1), vec![0.8; OPUS_FRAME_SIZE]);
  mixer.push_frame(AudioStreamId::Voice(1), vec![0.8; OPUS_FRAME_SIZE]);
  mixer.push_frame(AudioStreamId::Stream(2), vec![0.25; OPUS_FRAME_SIZE]);
  mixer.push_frame(AudioStreamId::Stream(2), vec![0.25; OPUS_FRAME_SIZE]);

  mixer.clear_voice_audio();
  let mut output = vec![0.0; OPUS_FRAME_SIZE];
  mixer.mix_samples(&mut output, true);

  assert!(output.iter().all(|sample| (*sample - 0.25).abs() < f32::EPSILON));
}

#[test]
fn clear_local_notification_audio_preserves_voice_and_stream_audio() {
  let mut mixer = VoiceMixer::default();
  mixer.push_frame(AudioStreamId::LocalNotification, vec![0.9; OPUS_FRAME_SIZE]);
  mixer.push_frame(AudioStreamId::LocalNotification, vec![0.9; OPUS_FRAME_SIZE]);
  mixer.push_frame(AudioStreamId::Voice(1), vec![0.35; OPUS_FRAME_SIZE]);
  mixer.push_frame(AudioStreamId::Voice(1), vec![0.35; OPUS_FRAME_SIZE]);
  mixer.push_frame(AudioStreamId::Stream(2), vec![0.25; OPUS_FRAME_SIZE]);
  mixer.push_frame(AudioStreamId::Stream(2), vec![0.25; OPUS_FRAME_SIZE]);

  mixer.clear_local_notification_audio();
  let mut output = vec![0.0; OPUS_FRAME_SIZE];
  mixer.mix_samples(&mut output, true);

  assert!(output.iter().all(|sample| (*sample - 0.6).abs() < f32::EPSILON));
}

#[test]
fn local_notification_audio_can_replace_queued_intro_audio() {
  let mut mixer = VoiceMixer::default();
  mixer.push_frame(AudioStreamId::LocalNotification, vec![0.8; OPUS_FRAME_SIZE]);
  mixer.push_frame(AudioStreamId::LocalNotification, vec![0.8; OPUS_FRAME_SIZE]);

  mixer.clear_local_notification_audio();
  mixer.push_frame(AudioStreamId::LocalNotification, vec![0.2; OPUS_FRAME_SIZE]);
  mixer.push_frame(AudioStreamId::LocalNotification, vec![0.2; OPUS_FRAME_SIZE]);

  let mut output = vec![0.0; OPUS_FRAME_SIZE];
  mixer.mix_samples(&mut output, true);

  assert!(output.iter().all(|sample| (*sample - 0.2).abs() < f32::EPSILON));
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
fn aec_delay_defaults_to_low_latency_path() {
  assert_eq!(configured_aec_delay_ms(None), DEFAULT_AEC_DELAY_MS);
  assert_eq!(configured_aec_delay_ms(Some("not-a-number")), DEFAULT_AEC_DELAY_MS);
}

#[test]
fn aec_delay_env_is_clamped_to_supported_range() {
  assert_eq!(configured_aec_delay_ms(Some("-10")), 0);
  assert_eq!(configured_aec_delay_ms(Some("35")), 35);
  assert_eq!(configured_aec_delay_ms(Some("800")), 500);
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
fn outgoing_sound_active_bypasses_voice_activation_gate() {
  let settings = AppSettings {
    voice_activation_threshold: 100,
    ..AppSettings::default()
  };
  let control = VoiceControlState::new(&settings, false, false);
  let mut gate = VoiceActivationGate::default();

  assert!(!gate.should_transmit(&control, &[0.0; OPUS_FRAME_SIZE]));
  control.set_outgoing_sound_active(true);
  assert!(gate.should_transmit(&control, &[0.0; OPUS_FRAME_SIZE]));
}

#[test]
fn push_to_talk_release_delay_keeps_transmit_open_until_deadline() {
  let mut settings = AppSettings {
    push_to_talk: true,
    push_to_talk_release_delay_ms: 500,
    ..AppSettings::default()
  };
  let control = VoiceControlState::new(&settings, false, false);

  assert!(!control.can_transmit());
  control.set_push_to_talk_active(true);
  assert!(control.can_transmit());
  control.set_push_to_talk_active(false);
  assert!(control.can_transmit());

  control
    .push_to_talk_release_until_ms
    .store(monotonic_millis().saturating_sub(1), Ordering::Relaxed);
  assert!(!control.can_transmit());

  settings.push_to_talk_release_delay_ms = 0;
  let control = VoiceControlState::new(&settings, false, false);
  control.set_push_to_talk_active(true);
  assert!(control.can_transmit());
  control.set_push_to_talk_active(false);
  assert!(!control.can_transmit());
}

#[test]
fn push_to_talk_ignores_activation_while_muted_or_deafened() {
  let settings = AppSettings {
    push_to_talk: true,
    ..AppSettings::default()
  };

  let control = VoiceControlState::new(&settings, true, false);
  control.set_push_to_talk_active(true);
  control.set_voice_state(false, false);
  assert!(!control.can_transmit());

  let control = VoiceControlState::new(&settings, false, true);
  control.set_push_to_talk_active(true);
  control.set_voice_state(false, false);
  assert!(!control.can_transmit());
}

#[test]
fn outgoing_sound_transmit_ignores_push_to_talk_but_respects_mute_and_deafen() {
  let settings = AppSettings {
    push_to_talk: true,
    ..AppSettings::default()
  };
  let control = VoiceControlState::new(&settings, false, false);

  assert!(!control.can_transmit());
  assert!(control.can_transmit_outgoing_sound());

  control.set_voice_state(true, false);
  assert!(!control.can_transmit_outgoing_sound());

  control.set_voice_state(false, true);
  assert!(!control.can_transmit_outgoing_sound());
}

#[test]
fn muting_or_deafening_clears_push_to_talk_latch_and_release_delay() {
  let settings = AppSettings {
    push_to_talk: true,
    push_to_talk_release_delay_ms: 500,
    ..AppSettings::default()
  };

  let control = VoiceControlState::new(&settings, false, false);
  control.set_push_to_talk_active(true);
  assert!(control.can_transmit());
  control.set_voice_state(true, false);
  control.set_voice_state(false, false);
  assert!(!control.can_transmit());

  let control = VoiceControlState::new(&settings, false, false);
  control.set_push_to_talk_active(true);
  control.set_push_to_talk_active(false);
  assert!(control.can_transmit());
  control.set_voice_state(false, true);
  control.set_voice_state(false, false);
  assert!(!control.can_transmit());
}

#[test]
fn muting_or_deafening_clears_outgoing_sound_active() {
  let settings = AppSettings::default();

  let control = VoiceControlState::new(&settings, false, false);
  control.set_outgoing_sound_active(true);
  control.set_voice_state(true, false);
  assert!(!control.outgoing_sound_active());

  let control = VoiceControlState::new(&settings, false, false);
  control.set_outgoing_sound_active(true);
  control.set_voice_state(false, true);
  assert!(!control.outgoing_sound_active());
}

#[test]
fn normalization_raises_quiet_frames_toward_target() {
  let mut normalizer = NormalizationState::default();
  let mut frame = vec![0.02; OPUS_FRAME_SIZE];
  normalizer.apply(&mut frame, 0.2);
  assert!(rms(&frame) > 0.02);
}

#[test]
fn voice_normalization_does_not_enable_capture_processing_by_itself() {
  let mut settings = AppSettings::default();
  settings.noise_cancellation = false;
  settings.echo_cancellation = false;
  settings.voice_normalization = true;

  assert!(build_audio_processing(&settings).is_none());
}

#[test]
fn outgoing_sound_volume_clamps_peak_and_percent() {
  let mut samples = vec![2.0, -1.0, 0.5];
  apply_outgoing_sound_volume(&mut samples, 100);
  assert_eq!(samples, vec![OUTGOING_SOUND_MAX_PEAK, -0.25, 0.125]);

  let mut samples = vec![0.5, -0.25];
  apply_outgoing_sound_volume(&mut samples, 50);
  assert_eq!(samples, vec![0.25, -0.125]);

  let mut samples = vec![0.5, -0.25];
  apply_outgoing_sound_volume(&mut samples, -10);
  assert_eq!(samples, vec![0.0, -0.0]);
}

#[test]
fn outgoing_sound_fade_shapes_edges_only() {
  let mut samples = vec![1.0; OUTGOING_SOUND_FADE_SAMPLES * 3];
  apply_outgoing_sound_fade(&mut samples);

  assert!(samples[0] > 0.0);
  assert!(samples[0] < samples[1]);
  assert_eq!(samples[OUTGOING_SOUND_FADE_SAMPLES - 1], 1.0);
  assert_eq!(samples[OUTGOING_SOUND_FADE_SAMPLES], 1.0);
  assert_eq!(samples[samples.len() - OUTGOING_SOUND_FADE_SAMPLES - 1], 1.0);
  assert!(samples[samples.len() - 1] > 0.0);
  assert!(samples[samples.len() - 1] < samples[samples.len() - 2]);
}
