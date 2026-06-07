use std::{
  sync::{Arc, Mutex},
  time::Duration,
};

use lurq::{
  app::{component::Component, ctx::Ctx},
  components::{Button, Column, Rect, Row, ScrollVertical, Slider, Stack, Text},
  core::Signal,
  layout::{
    Alignment, StackAlignment,
    scrollbar::ScrollBarStyle,
    text_style::{FontWeight, TextStyle},
  },
  node::{
    BackgroundColor, CursorIcon, Element, Gradient, GradientStop, SliderPartStyle, Style, border::Border, color::Color,
    dimension::Dimension,
  },
};

use crate::{
  services::{audio_devices, hotkeys},
  storage::{AppSettings, Storage},
  theme,
  ui::{
    common::{
      dropdown_menu::{DropdownOption, dropdown_menu},
      slider as app_slider,
    },
    settings::{
      refresh_button::{REFRESH_BUTTON_SIZE, REFRESH_BUTTON_SPACING, refresh_button},
      shell::{SettingsPage, header, page_stack, screen_full},
      toggle::{SettingsToggle, SettingsToggleProps},
    },
  },
};

pub(super) const DEVICE_DROPDOWN_WIDTH: f32 = 284.0;
pub(super) const AUDIO_CONTROL_WIDTH: f32 = DEVICE_DROPDOWN_WIDTH;
pub(super) const AUDIO_CONTROL_VALUE_WIDTH: f32 = 36.0;
pub(super) const AUDIO_CONTROL_VALUE_SPACING: f32 = 12.0;
pub(super) const AUDIO_CONTROL_TRACK_WIDTH: f32 =
  AUDIO_CONTROL_WIDTH - AUDIO_CONTROL_VALUE_WIDTH - AUDIO_CONTROL_VALUE_SPACING;
const INPUT_LEVEL_METER_WIDTH: f32 = AUDIO_CONTROL_TRACK_WIDTH;
const INPUT_LEVEL_METER_HEIGHT: f32 = 8.0;
const AUDIO_SLIDER_WIDTH: f32 = AUDIO_CONTROL_TRACK_WIDTH;
const THRESHOLD_CONTROL_WIDTH: f32 = AUDIO_CONTROL_WIDTH;
const THRESHOLD_CONTROL_HEIGHT: f32 = 22.0;
const THRESHOLD_TRACK_HEIGHT: f32 = 14.0;
const THRESHOLD_MARKER_WIDTH: f32 = 6.0;

pub struct SettingsAudioScreen {
  input_device: Signal<String>,
  output_device: Signal<String>,
  input_level: Signal<f32>,
  input_level_meter: Arc<Mutex<Option<audio_devices::InputLevelMeter>>>,
  notification_volume: Signal<i32>,
  noise_cancellation: Signal<bool>,
  voice_normalization: Signal<bool>,
  voice_normalization_target_level: Signal<i32>,
  echo_cancellation: Signal<bool>,
  voice_activation: Signal<bool>,
  voice_activation_threshold: Signal<i32>,
  push_to_talk: Signal<bool>,
  hotkey_push_to_talk: Signal<String>,
  hotkey_toggle_mute: Signal<String>,
  hotkey_toggle_deafen: Signal<String>,
  capturing_hotkey: Signal<Option<&'static str>>,
  input_devices: Signal<Vec<String>>,
  output_devices: Signal<Vec<String>>,
}

const HOTKEY_TOGGLE_MUTE: &str = "toggle_mute";
const HOTKEY_TOGGLE_DEAFEN: &str = "toggle_deafen";
const HOTKEY_PUSH_TO_TALK: &str = "push_to_talk";

impl Component for SettingsAudioScreen {
  type Props = ();

  fn create(ctx: &mut Ctx) -> Self {
    let storage = ctx.use_context::<Storage>();
    let settings = storage
      .as_ref()
      .and_then(|storage| storage.load_settings().ok())
      .unwrap_or_else(AppSettings::default);
    let input_device = ctx.signal(settings.audio_input_device);
    let output_device = ctx.signal(settings.audio_output_device);
    let input_level = ctx.signal(0.0_f32);
    let notification_volume = ctx.signal(settings.notification_volume.clamp(0, 100));
    let noise_cancellation = ctx.signal(settings.noise_cancellation);
    let voice_normalization = ctx.signal(settings.voice_normalization);
    let voice_normalization_target_level = ctx.signal(settings.voice_normalization_target_level.clamp(0, 100));
    let echo_cancellation = ctx.signal(settings.echo_cancellation);
    let voice_activation = ctx.signal(settings.voice_activation);
    let voice_activation_threshold = ctx.signal(settings.voice_activation_threshold.clamp(0, 100));
    let push_to_talk = ctx.signal(settings.push_to_talk);
    let hotkey_push_to_talk = ctx.signal(settings.hotkey_push_to_talk);
    let hotkey_toggle_mute = ctx.signal(settings.hotkey_toggle_mute);
    let hotkey_toggle_deafen = ctx.signal(settings.hotkey_toggle_deafen);
    let capturing_hotkey = ctx.signal(None::<&'static str>);
    let input_devices = ctx.signal(audio_devices::input_device_names());
    let output_devices = ctx.signal(audio_devices::output_device_names());
    let input_level_meter = Arc::new(Mutex::new(None));
    replace_input_level_meter(&input_level_meter, &input_level, &input_device.get_untracked());

    {
      let meter = input_level_meter.clone();
      let level = input_level.clone();
      ctx.watch(&input_device, move |device_name| {
        replace_input_level_meter(&meter, &level, device_name);
      });
    }

    {
      let meter = input_level_meter.clone();
      let level = input_level.clone();
      let interval = ctx.create_interval(Duration::from_millis(33), move || {
        poll_input_level_meter(&meter, &level);
      });
      interval.start();
    }

    if let Some(storage) = storage {
      let input_signal = input_device.clone();
      let output_signal = output_device.clone();
      let noise_cancellation_signal = noise_cancellation.clone();
      let voice_normalization_signal = voice_normalization.clone();
      let echo_cancellation_signal = echo_cancellation.clone();
      let voice_activation_signal = voice_activation.clone();
      let push_to_talk_signal = push_to_talk.clone();
      let hotkey_push_to_talk_signal = hotkey_push_to_talk.clone();
      let hotkey_toggle_mute_signal = hotkey_toggle_mute.clone();
      let hotkey_toggle_deafen_signal = hotkey_toggle_deafen.clone();
      ctx.on_effect(move || {
        let mut settings = storage.load_settings().unwrap_or_default();
        settings.audio_input_device = input_signal.get();
        settings.audio_output_device = output_signal.get();
        settings.noise_cancellation = noise_cancellation_signal.get();
        settings.voice_normalization = voice_normalization_signal.get();
        settings.echo_cancellation = echo_cancellation_signal.get();
        settings.voice_activation = voice_activation_signal.get();
        settings.push_to_talk = push_to_talk_signal.get();
        settings.hotkey_push_to_talk = hotkey_push_to_talk_signal.get();
        settings.hotkey_toggle_mute = hotkey_toggle_mute_signal.get();
        settings.hotkey_toggle_deafen = hotkey_toggle_deafen_signal.get();
        let _ = storage.save_settings(&settings);
      });
    }

    Self {
      input_device,
      output_device,
      input_level,
      input_level_meter,
      notification_volume,
      noise_cancellation,
      voice_normalization,
      voice_normalization_target_level,
      echo_cancellation,
      voice_activation,
      voice_activation_threshold,
      push_to_talk,
      hotkey_push_to_talk,
      hotkey_toggle_mute,
      hotkey_toggle_deafen,
      capturing_hotkey,
      input_devices,
      output_devices,
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let storage = ctx.use_context::<Storage>();
    let system_default = ctx.t("settings.audio.device.system").to_string();
    let input_devices = self.input_devices.get();
    let output_devices = self.output_devices.get();
    let input_level_active = self.input_level_meter_active();
    let input_level_value = (self.input_level.get() * 100.0).round() as i32;
    let input_level_label = if input_level_active {
      ctx.t_args(
        "settings.audio.input_level.percent",
        [("value", input_level_value.to_string())],
      )
    } else {
      ctx.t("settings.audio.input_level.unavailable")
    };

    let input_device = audio_row(
      &ctx.t("settings.audio.microphone"),
      &ctx.t("settings.audio.microphone.description"),
      audio_device_control(
        ctx,
        self.input_device.clone(),
        self.input_devices.clone(),
        device_options(&system_default, &input_devices, &self.input_device.get()),
        &system_default,
        audio_devices::input_device_names,
      ),
      true,
    );
    let input_gain = audio_row(
      &ctx.t("settings.audio.input_level"),
      &ctx.t("settings.audio.input_level.description"),
      input_level_meter(self.input_level.get(), input_level_active, &input_level_label),
      true,
    );
    let output_device = audio_row(
      &ctx.t("settings.audio.speaker"),
      &ctx.t("settings.audio.speaker.description"),
      audio_device_control(
        ctx,
        self.output_device.clone(),
        self.output_devices.clone(),
        device_options(&system_default, &output_devices, &self.output_device.get()),
        &system_default,
        audio_devices::output_device_names,
      ),
      true,
    );
    let notification_volume = audio_row(
      &ctx.t("settings.audio.notification_volume"),
      &ctx.t("settings.audio.notification_volume.description"),
      percent_slider(
        self.notification_volume.clone(),
        storage.clone(),
        AudioSliderSetting::NotificationVolume,
      ),
      true,
    );
    let noise_cancellation = audio_row(
      &ctx.t("settings.audio.noise_cancellation"),
      &ctx.t("settings.audio.noise_cancellation.description"),
      ctx.mount::<SettingsToggle>(SettingsToggleProps {
        enabled: self.noise_cancellation.clone(),
      }),
      true,
    );
    let voice_normalization = audio_row(
      &ctx.t("settings.audio.voice_normalization"),
      &ctx.t("settings.audio.voice_normalization.description"),
      ctx.mount::<SettingsToggle>(SettingsToggleProps {
        enabled: self.voice_normalization.clone(),
      }),
      true,
    );
    let normalization_target = audio_row(
      &ctx.t("settings.audio.target_level"),
      &ctx.t("settings.audio.target_level.description"),
      percent_slider(
        self.voice_normalization_target_level.clone(),
        storage.clone(),
        AudioSliderSetting::VoiceNormalizationTargetLevel,
      ),
      true,
    );
    let echo_cancellation = audio_row(
      &ctx.t("settings.audio.echo_cancellation"),
      &ctx.t("settings.audio.echo_cancellation.description"),
      ctx.mount::<SettingsToggle>(SettingsToggleProps {
        enabled: self.echo_cancellation.clone(),
      }),
      true,
    );
    let voice_activation = audio_row(
      &ctx.t("settings.audio.voice_activation"),
      &ctx.t("settings.audio.voice_activation.description"),
      ctx.mount::<SettingsToggle>(SettingsToggleProps {
        enabled: self.voice_activation.clone(),
      }),
      true,
    );
    let threshold = audio_row(
      &ctx.t("settings.audio.threshold"),
      &ctx.t("settings.audio.threshold.description"),
      threshold_slider(
        self.voice_activation_threshold.clone(),
        self.input_level.get(),
        input_level_active,
        storage.clone(),
        AudioSliderSetting::VoiceActivationThreshold,
        &ctx.t("settings.audio.threshold.speaking"),
        &ctx.t("settings.audio.threshold.silent"),
      ),
      true,
    );
    let push_to_talk = audio_row(
      &ctx.t("settings.audio.push_to_talk"),
      &ctx.t("settings.audio.push_to_talk.description"),
      ctx.mount::<SettingsToggle>(SettingsToggleProps {
        enabled: self.push_to_talk.clone(),
      }),
      true,
    );
    let toggle_mute = audio_row(
      &ctx.t("settings.audio.hotkey.toggle_mute"),
      &ctx.t("settings.audio.hotkey.toggle_mute.description"),
      hotkey_button(
        &ctx.t("settings.audio.hotkey.not_set"),
        &ctx.t("settings.audio.hotkey.recording"),
        self.hotkey_toggle_mute.clone(),
        self.capturing_hotkey.clone(),
        HOTKEY_TOGGLE_MUTE,
      ),
      true,
    );
    let push_to_talk_hotkey = audio_row(
      &ctx.t("settings.audio.hotkey.push_to_talk"),
      &ctx.t("settings.audio.hotkey.push_to_talk.description"),
      hotkey_button(
        &ctx.t("settings.audio.hotkey.not_set"),
        &ctx.t("settings.audio.hotkey.recording"),
        self.hotkey_push_to_talk.clone(),
        self.capturing_hotkey.clone(),
        HOTKEY_PUSH_TO_TALK,
      ),
      true,
    );
    let toggle_deafen = audio_row(
      &ctx.t("settings.audio.hotkey.toggle_deafen"),
      &ctx.t("settings.audio.hotkey.toggle_deafen.description"),
      hotkey_button(
        &ctx.t("settings.audio.hotkey.not_set"),
        &ctx.t("settings.audio.hotkey.recording"),
        self.hotkey_toggle_deafen.clone(),
        self.capturing_hotkey.clone(),
        HOTKEY_TOGGLE_DEAFEN,
      ),
      true,
    );
    let content = ScrollVertical::new(
      page_stack()
        .padding_vertical(40.0)
        .padding_horizontal(40.0)
        .child(header(
          &ctx.t("settings.audio.title"),
          &ctx.t("settings.audio.description"),
        ))
        .child(
          Column::new()
            .width(Dimension::Pct(100.0))
            .spacing(24.0)
            .child(
              Column::new()
                .width(Dimension::Pct(100.0))
                .spacing(12.0)
                .child(audio_section_label(&ctx.t("settings.audio.input")))
                .child(input_device)
                .child(input_gain),
            )
            .child(
              Column::new()
                .width(Dimension::Pct(100.0))
                .spacing(12.0)
                .child(audio_section_label(&ctx.t("settings.audio.output")))
                .child(output_device)
                .child(notification_volume),
            )
            .child(
              Column::new()
                .width(Dimension::Pct(100.0))
                .spacing(12.0)
                .child(audio_section_label(&ctx.t("settings.audio.processing")))
                .child(noise_cancellation)
                .child(voice_normalization)
                .child(normalization_target)
                .child(echo_cancellation),
            )
            .child(
              Column::new()
                .width(Dimension::Pct(100.0))
                .spacing(12.0)
                .child(audio_section_label(&ctx.t("settings.audio.transmission")))
                .child(voice_activation)
                .child(threshold)
                .child(push_to_talk),
            )
            .child(
              Column::new()
                .width(Dimension::Pct(100.0))
                .spacing(12.0)
                .child(audio_section_label(&ctx.t("settings.audio.hotkeys")))
                .child(toggle_mute)
                .child(push_to_talk_hotkey)
                .child(toggle_deafen),
            ),
        ),
    )
    .width(Dimension::Pct(100.0))
    .height(Dimension::Pct(100.0))
    .scrollbar(audio_scrollbar_style())
    .scrollbar_hovered(|mut style| {
      let palette = theme::palette();
      style.thumb_color = palette.accent_hover;
      style.track_color = palette.surface_input.with_opacity(0.75);
      style
    });

    screen_full(ctx, SettingsPage::Audio, content)
  }

  fn on_unmounted(&self) {
    if let Ok(mut meter) = self.input_level_meter.lock() {
      *meter = None;
    }
    self.input_level.set(0.0);
  }
}

impl SettingsAudioScreen {
  fn input_level_meter_active(&self) -> bool {
    self.input_level_meter.lock().is_ok_and(|meter| meter.is_some())
  }
}

fn replace_input_level_meter(
  meter: &Arc<Mutex<Option<audio_devices::InputLevelMeter>>>,
  level: &Signal<f32>,
  device_name: &str,
) {
  level.set(0.0);
  if let Ok(mut meter) = meter.lock() {
    *meter = audio_devices::input_level_meter(device_name);
  }
}

fn poll_input_level_meter(meter: &Arc<Mutex<Option<audio_devices::InputLevelMeter>>>, level: &Signal<f32>) {
  let measured = meter
    .lock()
    .ok()
    .and_then(|meter| meter.as_ref().map(audio_devices::InputLevelMeter::level))
    .unwrap_or(0.0);
  let current = level.get_untracked();
  let next = if measured > current {
    measured
  } else {
    current * 0.82 + measured * 0.18
  };

  if (next - current).abs() >= 0.004 {
    level.set(next);
  }
}

fn audio_device_control(
  ctx: &mut Ctx,
  selected: Signal<String>,
  devices: Signal<Vec<String>>,
  options: Vec<DropdownOption>,
  system_default: &str,
  refresh_devices: fn() -> Vec<String>,
) -> Element {
  let dropdown_width = DEVICE_DROPDOWN_WIDTH - REFRESH_BUTTON_SIZE - REFRESH_BUTTON_SPACING;

  Row::new()
    .width(DEVICE_DROPDOWN_WIDTH)
    .align_items(Alignment::Center)
    .spacing(REFRESH_BUTTON_SPACING)
    .child(refresh_button(ctx, move |_| {
      devices.set(refresh_devices());
    }))
    .child(dropdown_menu(selected, options, system_default, dropdown_width))
    .into()
}

fn device_options(system_default: &str, devices: &[String], selected: &str) -> Vec<DropdownOption> {
  let mut options = vec![DropdownOption {
    value: String::new(),
    label: system_default.to_owned(),
  }];

  if !selected.trim().is_empty() && !devices.iter().any(|device| device == selected) {
    options.push(DropdownOption {
      value: selected.to_owned(),
      label: selected.to_owned(),
    });
  }

  options.extend(devices.iter().map(|device| DropdownOption {
    value: device.clone(),
    label: device.clone(),
  }));

  options
}

pub(super) fn audio_row(label: &str, description: &str, trailing: Element, divider: bool) -> Element {
  let row = Row::new()
    .width(Dimension::Pct(100.0))
    .align_items(Alignment::Center)
    .spacing(16.0)
    .padding_vertical(18.0);
  let row = if divider {
    row.border_bottom(Border::inside(1.0, theme::PaletteColor::Border))
  } else {
    row
  };

  row
    .child(
      Column::new()
        .flex(1.0)
        .spacing(theme::SpacingSize::Xs)
        .child(Text::styled(label, row_title_style()))
        .child(Text::styled(description, row_subtitle_style())),
    )
    .child(trailing)
    .into()
}

pub(super) fn audio_section_label(label: &str) -> Element {
  Text::styled(
    label,
    TextStyle {
      font_family: Arc::from("Inter"),
      font_size: 11.0,
      line_height: 1.2,
      weight: FontWeight::Bold,
      color: theme::palette().text_muted,
      ..TextStyle::default()
    },
  )
  .into()
}

pub(super) fn audio_scrollbar_style() -> ScrollBarStyle {
  let palette = theme::palette();
  ScrollBarStyle {
    width: 8.0,
    min_thumb_length: 32.0,
    track_color: palette.surface_input.with_opacity(0.55),
    thumb_color: palette.accent,
    thumb_radius: 4.0,
    track_radius: 4.0,
    padding: 2.0,
    ..ScrollBarStyle::default()
  }
}

#[derive(Clone, Copy)]
enum AudioSliderSetting {
  NotificationVolume,
  VoiceNormalizationTargetLevel,
  VoiceActivationThreshold,
}

fn percent_slider(value: Signal<i32>, storage: Option<Storage>, setting: AudioSliderSetting) -> Element {
  slider_control(value, storage, setting, None)
}

fn threshold_slider(
  value: Signal<i32>,
  input_level: f32,
  input_level_active: bool,
  storage: Option<Storage>,
  setting: AudioSliderSetting,
  speaking_label: &str,
  silent_label: &str,
) -> Element {
  let current = value.get().clamp(0, 100);
  let level = input_level.clamp(0.0, 1.0);
  let threshold = current as f32 / 100.0;
  let level_width = THRESHOLD_CONTROL_WIDTH * level;
  let speaking = input_level_active && level >= threshold;
  let db_label = format!("-{current} dB");
  let status_label = if speaking { speaking_label } else { silent_label };

  let mut slider = Slider::new(value.clone())
    .range(0, 100)
    .width(THRESHOLD_CONTROL_WIDTH)
    .height(THRESHOLD_CONTROL_HEIGHT)
    .track_style(threshold_track_style(Color::from_hex("#00000000")))
    .track_hovered_style(threshold_track_style(Color::from_hex("#00000000")))
    .thumb_style(threshold_marker_style(theme::palette().text_primary))
    .thumb_hovered_style(threshold_marker_style(theme::palette().text_primary));

  if let Some(storage) = storage {
    slider = slider.on_blur(move || {
      save_slider_setting(&storage, setting, value.get_untracked());
    });
  }

  Column::new()
    .width(THRESHOLD_CONTROL_WIDTH)
    .spacing(8.0)
    .child(
      Row::new()
        .width(THRESHOLD_CONTROL_WIDTH)
        .align_items(Alignment::Center)
        .justify(lurq::layout::layout_kind::Justify::SpaceBetween)
        .child(threshold_status(status_label, speaking))
        .child(Text::styled(&db_label, threshold_db_label_style())),
    )
    .child(
      Stack::new()
        .stack_align(StackAlignment::CenterStart)
        .width(THRESHOLD_CONTROL_WIDTH)
        .height(THRESHOLD_CONTROL_HEIGHT)
        .child(
          Stack::new()
            .stack_align(StackAlignment::CenterStart)
            .width(THRESHOLD_CONTROL_WIDTH)
            .height(THRESHOLD_TRACK_HEIGHT)
            .rounded(7.0)
            .clip()
            .child(
              Rect::new(THRESHOLD_CONTROL_WIDTH, THRESHOLD_TRACK_HEIGHT)
                .rounded(7.0)
                .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceInput))
                .border_inside(1.0, theme::PaletteColor::Border),
            )
            .child(
              Rect::new(level_width, THRESHOLD_TRACK_HEIGHT).background_gradient(Gradient::linear(
                90.0,
                [
                  GradientStop::at(theme::palette().accent, 0.0),
                  GradientStop::at(theme::palette().accent_hover, 1.0),
                ],
              )),
            ),
        )
        .child(slider),
    )
    .into()
}

fn threshold_track_style(color: Color) -> SliderPartStyle {
  SliderPartStyle::new()
    .width(THRESHOLD_CONTROL_WIDTH)
    .height(THRESHOLD_TRACK_HEIGHT)
    .rounded(7.0)
    .background(color)
}

fn threshold_marker_style(color: Color) -> SliderPartStyle {
  SliderPartStyle::new()
    .size(THRESHOLD_MARKER_WIDTH, THRESHOLD_CONTROL_HEIGHT)
    .rounded(3.0)
    .background(color)
    .border_inside(1.0, BackgroundColor::Palette(theme::PaletteColor::SurfaceBase))
}

fn threshold_status(label: &str, active: bool) -> Element {
  let color = if active {
    theme::palette().accent
  } else {
    theme::palette().text_muted
  };

  Row::new()
    .height(20.0)
    .align_items(Alignment::Center)
    .spacing(5.0)
    .padding_horizontal(8.0)
    .rounded(10.0)
    .background(BackgroundColor::Palette(theme::PaletteColor::AccentMuted))
    .child(
      Rect::new(6.0, 6.0)
        .rounded(3.0)
        .background(BackgroundColor::Color(color)),
    )
    .child(Text::styled(label, threshold_status_label_style(color)))
    .into()
}

fn slider_control(
  value: Signal<i32>,
  storage: Option<Storage>,
  setting: AudioSliderSetting,
  input_level: Option<(f32, bool)>,
) -> Element {
  let current = value.get().clamp(0, 100);
  let fill_width = AUDIO_SLIDER_WIDTH * current as f32 / 100.0;
  let value_label = format!("{current}%");

  let stack = Stack::new()
    .stack_align(StackAlignment::CenterStart)
    .width(AUDIO_SLIDER_WIDTH)
    .height(app_slider::SLIDER_HEIGHT)
    .child(app_slider::track(AUDIO_SLIDER_WIDTH))
    .child(app_slider::fill(fill_width));
  let stack = if let Some((input_level, input_level_active)) = input_level {
    let input_level = input_level.clamp(0.0, 1.0);
    let input_width = AUDIO_SLIDER_WIDTH * input_level;
    stack.child(app_slider::meter_fill(
      input_width,
      input_level_fill_color(input_level, input_level_active),
    ))
  } else {
    stack
  };

  let mut slider = app_slider::slider(value.clone(), AUDIO_SLIDER_WIDTH, 0, 100);

  if let Some(storage) = storage {
    slider = slider.on_blur(move || {
      save_slider_setting(&storage, setting, value.get_untracked());
    });
  }

  Row::new()
    .width(AUDIO_CONTROL_WIDTH)
    .align_items(Alignment::Center)
    .spacing(AUDIO_CONTROL_VALUE_SPACING)
    .child(stack.child(slider))
    .child(audio_value_label(&value_label))
    .into()
}

fn save_slider_setting(storage: &Storage, setting: AudioSliderSetting, value: i32) {
  let mut settings = storage.load_settings().unwrap_or_default();
  let value = value.clamp(0, 100);

  match setting {
    AudioSliderSetting::NotificationVolume => settings.notification_volume = value,
    AudioSliderSetting::VoiceNormalizationTargetLevel => settings.voice_normalization_target_level = value,
    AudioSliderSetting::VoiceActivationThreshold => settings.voice_activation_threshold = value,
  }

  let _ = storage.save_settings(&settings);
}

fn hotkey_button(
  not_set_label: &str,
  recording_label: &str,
  value: Signal<String>,
  capturing: Signal<Option<&'static str>>,
  target: &'static str,
) -> Element {
  let is_recording = capturing.get() == Some(target);
  let current = value.get();
  let label = if is_recording {
    recording_label
  } else if current.trim().is_empty() {
    not_set_label
  } else {
    current.trim()
  };
  let border = if is_recording {
    theme::PaletteColor::Accent
  } else {
    theme::PaletteColor::Border
  };
  let click_capturing = capturing.clone();
  let key_capturing = capturing.clone();
  let key_value = value.clone();

  Button::empty()
    .width(150.0)
    .height(36.0)
    .align_items(Alignment::Center)
    .justify(lurq::layout::layout_kind::Justify::Center)
    .padding_horizontal(theme::SpacingSize::Lg)
    .rounded(theme::RadiusSize::Md)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceInput))
    .border_inside(1.0, border)
    .cursor(CursorIcon::Pointer)
    .hovered_style(Style::new().background(BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised)))
    .active_style(Style::new().background(BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised)))
    .on_click(move |_| {
      click_capturing.set(Some(target));
    })
    .on_key_down(move |event| {
      if key_capturing.get_untracked() != Some(target) {
        return;
      }
      if hotkeys::is_cancel_key(event) {
        key_capturing.set(None);
      } else if hotkeys::is_clear_key(event) {
        key_value.set(String::new());
        key_capturing.set(None);
      } else if let Some(hotkey) = hotkeys::event_to_hotkey(event) {
        key_value.set(hotkey);
        key_capturing.set(None);
      }
    })
    .child(
      Text::styled(label, input_level_label_style())
        .width(Dimension::Pct(100.0))
        .align(Alignment::Center),
    )
    .into()
}

fn input_level_meter(level: f32, active: bool, label: &str) -> Element {
  let level = level.clamp(0.0, 1.0);
  let fill_width = INPUT_LEVEL_METER_WIDTH * level;

  Row::new()
    .width(AUDIO_CONTROL_WIDTH)
    .align_items(Alignment::Center)
    .spacing(AUDIO_CONTROL_VALUE_SPACING)
    .child(
      Stack::new()
        .stack_align(StackAlignment::CenterStart)
        .width(INPUT_LEVEL_METER_WIDTH)
        .height(INPUT_LEVEL_METER_HEIGHT)
        .rounded(theme::RadiusSize::Sm)
        .clip()
        .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceInput))
        .border(Border::inside(1.0, theme::PaletteColor::Border))
        .child(
          Rect::new(fill_width, INPUT_LEVEL_METER_HEIGHT)
            .rounded(theme::RadiusSize::Sm)
            .background(BackgroundColor::Color(input_level_fill_color(level, active))),
        ),
    )
    .child(audio_value_label(label))
    .into()
}

fn audio_value_label(label: &str) -> Element {
  Text::styled(label, input_level_label_style())
    .width(AUDIO_CONTROL_VALUE_WIDTH)
    .text_align(Alignment::End)
    .nowrap()
    .into()
}

fn input_level_fill_color(level: f32, active: bool) -> lurq::node::color::Color {
  if !active {
    return theme::palette().border;
  }
  if level >= 0.9 {
    return theme::palette().danger;
  }
  if level >= 0.75 {
    return theme::palette().warning;
  }
  theme::palette().success
}

fn threshold_status_label_style(color: Color) -> TextStyle {
  TextStyle {
    font_family: Arc::from("Inter"),
    font_size: 10.0,
    line_height: 1.2,
    weight: FontWeight::Bold,
    color,
    ..TextStyle::default()
  }
}

fn threshold_db_label_style() -> TextStyle {
  TextStyle {
    font_family: Arc::from("JetBrains Mono"),
    font_size: 12.0,
    line_height: 1.2,
    color: theme::palette().text_muted,
    ..TextStyle::default()
  }
}

fn input_level_label_style() -> TextStyle {
  TextStyle {
    font_family: Arc::from("JetBrains Mono"),
    font_size: 11.0,
    line_height: 1.2,
    color: theme::palette().text_muted,
    ..TextStyle::default()
  }
}

fn row_title_style() -> TextStyle {
  TextStyle {
    font_family: Arc::from("Inter"),
    font_size: 14.0,
    line_height: 1.2,
    weight: FontWeight::Bold,
    color: theme::palette().text_secondary,
    ..TextStyle::default()
  }
}

fn row_subtitle_style() -> TextStyle {
  TextStyle {
    font_family: Arc::from("Inter"),
    font_size: 13.0,
    line_height: 1.2,
    color: theme::palette().text_muted,
    ..TextStyle::default()
  }
}
