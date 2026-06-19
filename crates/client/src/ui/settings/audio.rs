use std::{
  sync::{Arc, Mutex},
  thread,
  time::Duration,
};

use lurq::{
  app::{
    component::{Component, ComponentInfo, DevtoolsFormatter, DevtoolsInspectable},
    ctx::Ctx,
    events::{KeyboardEvent, MouseButton, MouseEvent},
  },
  components::{Button, Column, Rect, Row, ScrollVertical, Slider, Stack, Text, TextOverflow},
  core::{Signal, Store},
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
  services::{
    audio_devices,
    global_hotkeys::{GlobalMouseHotkeyCapture, GlobalVoiceHotkeys},
    hotkeys,
  },
  session::ServerSession,
  storage::{AppSettings, Storage, update_app_settings},
  theme,
  ui::{
    common::{
      dropdown_menu::{DropdownOption, dropdown_menu},
      lucide_icon::{LucideIcon, LucideIconProps},
      percent_slider::{PercentSlider, PercentSliderProps, PercentSliderSaveAction},
      slider as app_slider,
    },
    settings::{
      refresh_button::{REFRESH_BUTTON_SIZE, REFRESH_BUTTON_SPACING, refresh_button},
      shell::{SettingsPage, header, page_stack, screen_full, settings_content_padding, settings_section_spacing},
      toggle::settings_toggle,
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
const PUSH_TO_TALK_RELEASE_DELAY_MAX_TENTHS: i32 = 20;
const PUSH_TO_TALK_RELEASE_DELAY_STEP_MS: i32 = 100;

pub struct SettingsAudioScreen {
  input_device: Signal<String>,
  output_device: Signal<String>,
  input_level: Signal<f32>,
  input_level_meter_active: Signal<bool>,
  input_level_meter: Arc<Mutex<Option<audio_devices::InputLevelMeter>>>,
  noise_cancellation: bool,
  voice_normalization: bool,
  voice_normalization_target_level: i32,
  echo_cancellation: bool,
  voice_activation_threshold: i32,
  push_to_talk: bool,
  push_to_talk_release_delay_ms: i32,
  hotkey_push_to_talk: String,
  hotkey_toggle_mute: String,
  hotkey_toggle_deafen: String,
  input_devices: Signal<Vec<String>>,
  output_devices: Signal<Vec<String>>,
}

impl Component for SettingsAudioScreen {
  type Props = ();

  fn create(ctx: &mut Ctx) -> Self {
    let settings = ctx
      .use_context::<Store<AppSettings>>()
      .map(|settings| settings.get())
      .unwrap_or_else(AppSettings::default);
    let input_device = ctx.signal(settings.audio_input_device);
    let output_device = ctx.signal(settings.audio_output_device);
    let input_level = ctx.signal(0.0_f32);
    let input_level_meter_active = ctx.signal(false);
    let noise_cancellation = settings.noise_cancellation;
    let voice_normalization = settings.voice_normalization;
    let voice_normalization_target_level = settings.voice_normalization_target_level.clamp(0, 100);
    let echo_cancellation = settings.echo_cancellation;
    let voice_activation_threshold = settings.voice_activation_threshold.clamp(0, 100);
    let push_to_talk = settings.push_to_talk;
    let push_to_talk_release_delay_ms = settings.push_to_talk_release_delay_ms.clamp(0, 2000);
    let hotkey_push_to_talk = settings.hotkey_push_to_talk;
    let hotkey_toggle_mute = settings.hotkey_toggle_mute;
    let hotkey_toggle_deafen = settings.hotkey_toggle_deafen;
    let input_devices = ctx.signal(audio_devices::input_device_names());
    let output_devices = ctx.signal(audio_devices::output_device_names());
    let input_level_meter = Arc::new(Mutex::new(None));
    replace_input_level_meter(
      &input_level_meter,
      &input_level,
      &input_level_meter_active,
      &input_device.get_untracked(),
    );

    {
      let meter = input_level_meter.clone();
      let level = input_level.clone();
      let active = input_level_meter_active.clone();
      ctx.watch(&input_device, move |device_name| {
        replace_input_level_meter(&meter, &level, &active, device_name);
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

    Self {
      input_device,
      output_device,
      input_level,
      input_level_meter_active,
      input_level_meter,
      noise_cancellation,
      voice_normalization,
      voice_normalization_target_level,
      echo_cancellation,
      voice_activation_threshold,
      push_to_talk,
      push_to_talk_release_delay_ms,
      hotkey_push_to_talk,
      hotkey_toggle_mute,
      hotkey_toggle_deafen,
      input_devices,
      output_devices,
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let storage = ctx.use_context::<Storage>();
    let settings_store = ctx.use_context::<Store<AppSettings>>();
    let session = ctx.use_context::<ServerSession>();
    let (padding_x, padding_y) = settings_content_padding(ctx);
    let section_spacing = settings_section_spacing(ctx);
    let content = ScrollVertical::new(
      page_stack(ctx)
        .padding_vertical(padding_y)
        .padding_horizontal(padding_x)
        .child(header(
          &ctx.t("settings.audio.title"),
          &ctx.t("settings.audio.description"),
        ))
        .child(
          Column::new()
            .width(Dimension::Pct(100.0))
            .spacing(section_spacing)
            .child(
              Column::new()
                .width(Dimension::Pct(100.0))
                .spacing(12.0)
                .child(audio_section_label(&ctx.t("settings.audio.input")))
                .child(ctx.mount::<AudioDeviceSetting>(AudioDeviceSettingProps {
                  kind: AudioDeviceKind::Input,
                  selected: self.input_device.clone(),
                  devices: self.input_devices.clone(),
                }))
                .child(ctx.mount::<AudioInputLevelSetting>(AudioInputLevelSettingProps {
                  level: self.input_level.clone(),
                  active: self.input_level_meter_active.clone(),
                })),
            )
            .child(
              Column::new()
                .width(Dimension::Pct(100.0))
                .spacing(12.0)
                .child(audio_section_label(&ctx.t("settings.audio.output")))
                .child(ctx.mount::<AudioDeviceSetting>(AudioDeviceSettingProps {
                  kind: AudioDeviceKind::Output,
                  selected: self.output_device.clone(),
                  devices: self.output_devices.clone(),
                })),
            )
            .child(
              Column::new()
                .width(Dimension::Pct(100.0))
                .spacing(12.0)
                .child(audio_section_label(&ctx.t("settings.audio.processing")))
                .child(ctx.mount::<AudioToggleSetting>(AudioToggleSettingProps {
                  title_key: "settings.audio.noise_cancellation",
                  description_key: "settings.audio.noise_cancellation.description",
                  initial_enabled: self.noise_cancellation,
                  setting: AudioBoolSetting::NoiseCancellation,
                }))
                .child(ctx.mount::<AudioToggleSetting>(AudioToggleSettingProps {
                  title_key: "settings.audio.voice_normalization",
                  description_key: "settings.audio.voice_normalization.description",
                  initial_enabled: self.voice_normalization,
                  setting: AudioBoolSetting::VoiceNormalization,
                }))
                .child(ctx.mount::<AudioPercentSliderSetting>(AudioPercentSliderSettingProps {
                  title_key: "settings.audio.target_level",
                  description_key: "settings.audio.target_level.description",
                  initial_value: self.voice_normalization_target_level,
                  on_blur: audio_slider_save_action(
                    storage.clone(),
                    settings_store.clone(),
                    session.clone(),
                    AudioSliderSetting::VoiceNormalizationTargetLevel,
                  ),
                }))
                .child(ctx.mount::<AudioToggleSetting>(AudioToggleSettingProps {
                  title_key: "settings.audio.echo_cancellation",
                  description_key: "settings.audio.echo_cancellation.description",
                  initial_enabled: self.echo_cancellation,
                  setting: AudioBoolSetting::EchoCancellation,
                })),
            )
            .child(
              Column::new()
                .width(Dimension::Pct(100.0))
                .spacing(12.0)
                .child(audio_section_label(&ctx.t("settings.audio.transmission")))
                .child(ctx.mount::<AudioThresholdSetting>(AudioThresholdSettingProps {
                  initial_value: self.voice_activation_threshold,
                  input_level: self.input_level.clone(),
                  input_level_active: self.input_level_meter_active.clone(),
                  on_blur: audio_slider_save_action(
                    storage.clone(),
                    settings_store.clone(),
                    session.clone(),
                    AudioSliderSetting::VoiceActivationThreshold,
                  ),
                }))
                .child(ctx.mount::<AudioToggleSetting>(AudioToggleSettingProps {
                  title_key: "settings.audio.push_to_talk",
                  description_key: "settings.audio.push_to_talk.description",
                  initial_enabled: self.push_to_talk,
                  setting: AudioBoolSetting::PushToTalk,
                }))
                .child(ctx.mount::<AudioDelaySliderSetting>(AudioDelaySliderSettingProps {
                  title_key: "settings.audio.push_to_talk_release_delay",
                  description_key: "settings.audio.push_to_talk_release_delay.description",
                  initial_value: delay_ms_to_tenths(self.push_to_talk_release_delay_ms),
                  on_blur: audio_slider_save_action(
                    storage.clone(),
                    settings_store.clone(),
                    session.clone(),
                    AudioSliderSetting::PushToTalkReleaseDelay,
                  ),
                })),
            )
            .child(
              Column::new()
                .width(Dimension::Pct(100.0))
                .spacing(12.0)
                .child(audio_section_label(&ctx.t("settings.audio.hotkeys")))
                .child(ctx.mount::<AudioHotkeySetting>(AudioHotkeySettingProps {
                  title_key: "settings.audio.hotkey.toggle_mute",
                  description_key: "settings.audio.hotkey.toggle_mute.description",
                  initial_value: self.hotkey_toggle_mute.clone(),
                  setting: AudioStringSetting::HotkeyToggleMute,
                }))
                .child(ctx.mount::<AudioHotkeySetting>(AudioHotkeySettingProps {
                  title_key: "settings.audio.hotkey.push_to_talk",
                  description_key: "settings.audio.hotkey.push_to_talk.description",
                  initial_value: self.hotkey_push_to_talk.clone(),
                  setting: AudioStringSetting::HotkeyPushToTalk,
                }))
                .child(ctx.mount::<AudioHotkeySetting>(AudioHotkeySettingProps {
                  title_key: "settings.audio.hotkey.toggle_deafen",
                  description_key: "settings.audio.hotkey.toggle_deafen.description",
                  initial_value: self.hotkey_toggle_deafen.clone(),
                  setting: AudioStringSetting::HotkeyToggleDeafen,
                })),
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
    self.input_level_meter_active.set(false);
  }
}

fn replace_input_level_meter(
  meter: &Arc<Mutex<Option<audio_devices::InputLevelMeter>>>,
  level: &Signal<f32>,
  active: &Signal<bool>,
  device_name: &str,
) {
  level.set(0.0);
  if let Ok(mut meter) = meter.lock() {
    *meter = audio_devices::input_level_meter(device_name);
    active.set(meter.is_some());
  } else {
    active.set(false);
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

#[derive(Clone, Copy, PartialEq, Eq, lurq::DevtoolsInspectable)]
enum AudioDeviceKind {
  Input,
  Output,
}

#[derive(Clone, lurq::DevtoolsInspectable)]
struct AudioDeviceSettingProps {
  kind: AudioDeviceKind,
  selected: Signal<String>,
  devices: Signal<Vec<String>>,
}

impl PartialEq for AudioDeviceSettingProps {
  fn eq(&self, other: &Self) -> bool {
    self.kind == other.kind && self.selected.id() == other.selected.id() && self.devices.id() == other.devices.id()
  }
}

struct AudioDeviceSetting {
  value: Signal<String>,
}

impl Component for AudioDeviceSetting {
  type Props = AudioDeviceSettingProps;

  fn create(ctx: &mut Ctx) -> Self {
    let props = ctx.props::<Self::Props>().clone();
    let value = ctx.signal(props.selected.get_untracked());
    let storage = ctx.use_context::<Storage>();
    let settings_store = ctx.use_context::<Store<AppSettings>>();
    let session = ctx.use_context::<ServerSession>();
    let setting = match props.kind {
      AudioDeviceKind::Input => AudioStringSetting::AudioInputDevice,
      AudioDeviceKind::Output => AudioStringSetting::AudioOutputDevice,
    };
    ctx.watch(&value, move |value| {
      props.selected.set(value.clone());
      if let Some(settings_store) = settings_store.as_ref() {
        let settings = save_audio_string_setting(settings_store, storage.as_ref(), setting, value.clone());
        if matches!(setting, AudioStringSetting::AudioOutputDevice)
          && let Some(session) = session.as_ref()
        {
          session.set_notification_audio_settings(&settings);
        }
        restart_voice_for_audio_setting(settings_store, session.as_ref());
      }
    });
    Self { value }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let props = ctx.props::<Self::Props>().clone();
    let system_default = ctx.t("settings.audio.device.system").to_string();
    let devices = props.devices.get();
    let selected = self.value.get();
    let (title_key, description_key, refresh_devices) = match props.kind {
      AudioDeviceKind::Input => (
        "settings.audio.microphone",
        "settings.audio.microphone.description",
        audio_devices::input_device_names as fn() -> Vec<String>,
      ),
      AudioDeviceKind::Output => (
        "settings.audio.speaker",
        "settings.audio.speaker.description",
        audio_devices::output_device_names as fn() -> Vec<String>,
      ),
    };

    audio_row(
      &ctx.t(title_key),
      &ctx.t(description_key),
      audio_device_control(
        ctx,
        self.value.clone(),
        props.devices,
        device_options(&system_default, &devices, &selected),
        &system_default,
        refresh_devices,
      ),
      true,
    )
  }
}

#[derive(Clone, lurq::DevtoolsInspectable)]
struct AudioInputLevelSettingProps {
  level: Signal<f32>,
  active: Signal<bool>,
}

impl PartialEq for AudioInputLevelSettingProps {
  fn eq(&self, other: &Self) -> bool {
    self.level.id() == other.level.id() && self.active.id() == other.active.id()
  }
}

struct AudioInputLevelSetting;

impl Component for AudioInputLevelSetting {
  type Props = AudioInputLevelSettingProps;

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let props = ctx.props::<Self::Props>().clone();
    let active = props.active.get();
    let level = props.level.get();
    let value = (level * 100.0).round() as i32;
    let label = if active {
      ctx.t_args("settings.audio.input_level.percent", [("value", value.to_string())])
    } else {
      ctx.t("settings.audio.input_level.unavailable")
    };

    audio_row(
      &ctx.t("settings.audio.input_level"),
      &ctx.t("settings.audio.input_level.description"),
      input_level_meter(level, active, &label),
      true,
    )
  }
}

#[derive(Clone, Copy, PartialEq, Eq, lurq::DevtoolsInspectable)]
enum AudioBoolSetting {
  NoiseCancellation,
  VoiceNormalization,
  EchoCancellation,
  PushToTalk,
}

#[derive(Clone, lurq::DevtoolsInspectable)]
struct AudioToggleSettingProps {
  title_key: &'static str,
  description_key: &'static str,
  initial_enabled: bool,
  setting: AudioBoolSetting,
}

impl PartialEq for AudioToggleSettingProps {
  fn eq(&self, other: &Self) -> bool {
    self.title_key == other.title_key
      && self.description_key == other.description_key
      && self.initial_enabled == other.initial_enabled
      && self.setting == other.setting
  }
}

struct AudioToggleSetting {
  enabled: Signal<bool>,
}

impl Component for AudioToggleSetting {
  type Props = AudioToggleSettingProps;

  fn create(ctx: &mut Ctx) -> Self {
    let props = ctx.props::<Self::Props>().clone();
    let enabled = ctx.signal(props.initial_enabled);
    let storage = ctx.use_context::<Storage>();
    if let Some(settings_store) = ctx.use_context::<Store<AppSettings>>() {
      let session = ctx.use_context::<ServerSession>();
      ctx.watch(&enabled, move |enabled| {
        save_audio_bool_setting(&settings_store, storage.as_ref(), props.setting, *enabled);
        if matches!(props.setting, AudioBoolSetting::VoiceNormalization) {
          if let Some(session) = session.as_ref() {
            session.set_voice_normalization(*enabled);
          }
        } else {
          restart_voice_for_audio_setting(&settings_store, session.as_ref());
        }
      });
    }
    Self { enabled }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let props = ctx.props::<Self::Props>().clone();
    let enabled = self.enabled.get();
    let enabled_signal = self.enabled.clone();
    audio_row(
      &ctx.t(props.title_key),
      &ctx.t(props.description_key),
      settings_toggle(enabled, move || {
        enabled_signal.set(!enabled_signal.get_untracked());
      }),
      true,
    )
  }
}

type AudioSliderSaveAction = PercentSliderSaveAction;

#[derive(Clone)]
struct AudioPercentSliderSettingProps {
  title_key: &'static str,
  description_key: &'static str,
  initial_value: i32,
  on_blur: AudioSliderSaveAction,
}

impl PartialEq for AudioPercentSliderSettingProps {
  fn eq(&self, other: &Self) -> bool {
    self.title_key == other.title_key
      && self.description_key == other.description_key
      && self.initial_value == other.initial_value
  }
}

impl DevtoolsInspectable for AudioPercentSliderSettingProps {
  fn inspect(&self, formatter: &mut DevtoolsFormatter<'_>) {
    formatter.buffer_mut().push(ComponentInfo::with_value(
      "title_key",
      std::any::type_name::<&'static str>(),
      self.title_key.to_owned(),
    ));
    formatter.buffer_mut().push(ComponentInfo::with_value(
      "initial_value",
      std::any::type_name::<i32>(),
      self.initial_value.to_string(),
    ));
  }
}

struct AudioPercentSliderSetting {}

impl Component for AudioPercentSliderSetting {
  type Props = AudioPercentSliderSettingProps;

  fn create(_ctx: &mut Ctx) -> Self {
    Self {}
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let props = ctx.props::<Self::Props>().clone();
    audio_row(
      &ctx.t(props.title_key),
      &ctx.t(props.description_key),
      ctx.mount::<PercentSlider>(PercentSliderProps {
        initial_value: props.initial_value,
        control_width: AUDIO_CONTROL_WIDTH,
        track_width: AUDIO_SLIDER_WIDTH,
        value_width: AUDIO_CONTROL_VALUE_WIDTH,
        value_spacing: AUDIO_CONTROL_VALUE_SPACING,
        on_blur: props.on_blur,
      }),
      true,
    )
  }
}

#[derive(Clone)]
struct AudioDelaySliderSettingProps {
  title_key: &'static str,
  description_key: &'static str,
  initial_value: i32,
  on_blur: AudioSliderSaveAction,
}

impl PartialEq for AudioDelaySliderSettingProps {
  fn eq(&self, other: &Self) -> bool {
    self.title_key == other.title_key
      && self.description_key == other.description_key
      && self.initial_value == other.initial_value
  }
}

impl DevtoolsInspectable for AudioDelaySliderSettingProps {
  fn inspect(&self, formatter: &mut DevtoolsFormatter<'_>) {
    formatter.buffer_mut().push(ComponentInfo::with_value(
      "title_key",
      std::any::type_name::<&'static str>(),
      self.title_key.to_owned(),
    ));
    formatter.buffer_mut().push(ComponentInfo::with_value(
      "initial_value",
      std::any::type_name::<i32>(),
      self.initial_value.to_string(),
    ));
  }
}

struct AudioDelaySliderSetting {
  initial_value: Signal<i32>,
  value: Signal<i32>,
}

impl Component for AudioDelaySliderSetting {
  type Props = AudioDelaySliderSettingProps;

  fn create(ctx: &mut Ctx) -> Self {
    let initial_value = ctx
      .props::<Self::Props>()
      .initial_value
      .clamp(0, PUSH_TO_TALK_RELEASE_DELAY_MAX_TENTHS);
    Self {
      initial_value: ctx.signal(initial_value),
      value: ctx.signal(initial_value),
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let props = ctx.props::<Self::Props>().clone();
    let initial_value = props.initial_value.clamp(0, PUSH_TO_TALK_RELEASE_DELAY_MAX_TENTHS);
    if self.initial_value.get_untracked() != initial_value {
      self.initial_value.set(initial_value);
      self.value.set(initial_value);
    }

    audio_row(
      &ctx.t(props.title_key),
      &ctx.t(props.description_key),
      delay_slider_control(self.value.clone(), props.on_blur),
      true,
    )
  }
}

#[derive(Clone)]
struct AudioThresholdSettingProps {
  initial_value: i32,
  input_level: Signal<f32>,
  input_level_active: Signal<bool>,
  on_blur: AudioSliderSaveAction,
}

impl PartialEq for AudioThresholdSettingProps {
  fn eq(&self, other: &Self) -> bool {
    self.initial_value == other.initial_value
      && self.input_level.id() == other.input_level.id()
      && self.input_level_active.id() == other.input_level_active.id()
  }
}

impl DevtoolsInspectable for AudioThresholdSettingProps {
  fn inspect(&self, formatter: &mut DevtoolsFormatter<'_>) {
    formatter.buffer_mut().push(ComponentInfo::with_value(
      "initial_value",
      std::any::type_name::<i32>(),
      self.initial_value.to_string(),
    ));
  }
}

struct AudioThresholdSetting {
  value: Signal<i32>,
}

impl Component for AudioThresholdSetting {
  type Props = AudioThresholdSettingProps;

  fn create(ctx: &mut Ctx) -> Self {
    let props = ctx.props::<Self::Props>().clone();
    let value = ctx.signal(props.initial_value);
    let storage = ctx.use_context::<Storage>();
    let settings_store = ctx.use_context::<Store<AppSettings>>();
    let session = ctx.use_context::<ServerSession>();
    if let Some(settings_store) = settings_store {
      ctx.watch(&value, move |value| {
        let _ = save_slider_setting(
          &settings_store,
          storage.as_ref(),
          AudioSliderSetting::VoiceActivationThreshold,
          *value,
        );
        if let Some(session) = session.as_ref() {
          session.set_voice_activation_threshold(*value);
        }
      });
    }

    Self { value }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let props = ctx.props::<Self::Props>().clone();
    audio_row(
      &ctx.t("settings.audio.threshold"),
      &ctx.t("settings.audio.threshold.description"),
      threshold_slider(
        self.value.clone(),
        props.input_level.get(),
        props.input_level_active.get(),
        props.on_blur,
        &ctx.t("settings.audio.threshold.speaking"),
        &ctx.t("settings.audio.threshold.silent"),
      ),
      true,
    )
  }
}

#[derive(Clone, Copy, PartialEq, Eq, lurq::DevtoolsInspectable)]
enum AudioStringSetting {
  AudioInputDevice,
  AudioOutputDevice,
  HotkeyPushToTalk,
  HotkeyToggleMute,
  HotkeyToggleDeafen,
}

#[derive(Clone, lurq::DevtoolsInspectable)]
struct AudioHotkeySettingProps {
  title_key: &'static str,
  description_key: &'static str,
  initial_value: String,
  setting: AudioStringSetting,
}

impl PartialEq for AudioHotkeySettingProps {
  fn eq(&self, other: &Self) -> bool {
    self.title_key == other.title_key
      && self.description_key == other.description_key
      && self.initial_value == other.initial_value
      && self.setting == other.setting
  }
}

struct AudioHotkeySetting {
  value: Signal<String>,
  recording: Signal<bool>,
  suppress_next_click: Signal<bool>,
  mouse_capture: Arc<Mutex<Option<GlobalMouseHotkeyCapture>>>,
  global_hotkeys: Option<GlobalVoiceHotkeys>,
}

impl Component for AudioHotkeySetting {
  type Props = AudioHotkeySettingProps;

  fn create(ctx: &mut Ctx) -> Self {
    let props = ctx.props::<Self::Props>().clone();
    let value = ctx.signal(props.initial_value.clone());
    let recording = ctx.signal(false);
    let suppress_next_click = ctx.signal(false);
    let mouse_capture = Arc::new(Mutex::new(None));
    let global_hotkeys = ctx.use_context::<GlobalVoiceHotkeys>();
    let storage = ctx.use_context::<Storage>();
    if let Some(settings_store) = ctx.use_context::<Store<AppSettings>>() {
      ctx.watch(&value, move |value| {
        save_audio_string_setting(&settings_store, storage.as_ref(), props.setting, value.clone());
      });
    }

    {
      let capture = mouse_capture.clone();
      let capture_value = value.clone();
      let capture_recording = recording.clone();
      let capture_suppress_next_click = suppress_next_click.clone();
      let interval = ctx.create_interval(Duration::from_millis(16), move || {
        if let Some(hotkey) = take_mouse_hotkey_capture(&capture) {
          capture_value.set(hotkey);
          capture_recording.set(false);
          capture_suppress_next_click.set(true);
        }
      });
      interval.start();
    }

    Self {
      value,
      recording,
      suppress_next_click,
      mouse_capture,
      global_hotkeys,
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let props = ctx.props::<Self::Props>().clone();
    audio_row(
      &ctx.t(props.title_key),
      &ctx.t(props.description_key),
      hotkey_button(
        ctx,
        &ctx.t("settings.audio.hotkey.not_set"),
        &ctx.t("settings.audio.hotkey.recording"),
        self.value.clone(),
        self.recording.clone(),
        self.suppress_next_click.clone(),
        self.mouse_capture.clone(),
        self.global_hotkeys.clone(),
      ),
      true,
    )
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
        .min_width(0.0)
        .spacing(theme::SpacingSize::Xs)
        .child(
          Text::styled(label, row_title_style())
            .width(Dimension::Pct(100.0))
            .min_width(0.0)
            .nowrap()
            .text_overflow(TextOverflow::Elipsis),
        )
        .child(
          Text::styled(description, row_subtitle_style())
            .width(Dimension::Pct(100.0))
            .min_width(0.0)
            .nowrap()
            .text_overflow(TextOverflow::Elipsis),
        ),
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

#[derive(Clone, Copy, PartialEq, Eq, lurq::DevtoolsInspectable)]
enum AudioSliderSetting {
  VoiceNormalizationTargetLevel,
  VoiceActivationThreshold,
  PushToTalkReleaseDelay,
}

fn delay_slider_control(value: Signal<i32>, on_blur: AudioSliderSaveAction) -> Element {
  let current = value.get().clamp(0, PUSH_TO_TALK_RELEASE_DELAY_MAX_TENTHS);
  let label = format!("{:.1}s", current as f32 / 10.0);
  let mut slider = app_slider::slider(
    value.clone(),
    AUDIO_SLIDER_WIDTH,
    0,
    PUSH_TO_TALK_RELEASE_DELAY_MAX_TENTHS,
  );

  slider = slider.on_blur(move || {
    on_blur(value.get_untracked());
  });

  Row::new()
    .width(AUDIO_CONTROL_WIDTH)
    .align_items(Alignment::Center)
    .spacing(AUDIO_CONTROL_VALUE_SPACING)
    .child(slider)
    .child(audio_value_label(&label))
    .into()
}

fn threshold_slider(
  value: Signal<i32>,
  input_level: f32,
  input_level_active: bool,
  on_blur: AudioSliderSaveAction,
  speaking_label: &str,
  silent_label: &str,
) -> Element {
  let current = value.get().clamp(0, 100);
  let level = input_level.clamp(0.0, 1.0);
  let threshold = current as f32 / 100.0;
  let threshold_track_width = THRESHOLD_CONTROL_WIDTH - THRESHOLD_MARKER_WIDTH;
  let threshold_track_offset = THRESHOLD_MARKER_WIDTH * 0.5;
  let level_width = threshold_track_width * level;
  let speaking = input_level_active && level >= threshold;
  let db_label = format!("-{current} dB");
  let status_label = if speaking { speaking_label } else { silent_label };
  let mut slider = Slider::new(value.clone())
    .range(0, 100)
    .width(THRESHOLD_CONTROL_WIDTH)
    .height(THRESHOLD_CONTROL_HEIGHT)
    .track_style(threshold_track_style(
      THRESHOLD_CONTROL_WIDTH,
      Color::from_hex("#00000000"),
    ))
    .track_hovered_style(threshold_track_style(
      THRESHOLD_CONTROL_WIDTH,
      Color::from_hex("#00000000"),
    ))
    .thumb_style(threshold_marker_style(theme::palette().text_primary))
    .thumb_hovered_style(threshold_marker_style(theme::palette().text_primary));

  slider = slider.on_blur(move || {
    on_blur(value.get_untracked());
  });

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
              Rect::new(threshold_track_width, THRESHOLD_TRACK_HEIGHT)
                .absolute_position(threshold_track_offset, 0.0)
                .rounded(7.0)
                .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceInput))
                .border_inside(1.0, theme::PaletteColor::Border),
            )
            .child(
              Rect::new(level_width, THRESHOLD_TRACK_HEIGHT)
                .absolute_position(threshold_track_offset, 0.0)
                .background_gradient(Gradient::linear(
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

fn threshold_track_style(width: f32, color: Color) -> SliderPartStyle {
  SliderPartStyle::new()
    .width(width)
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

fn audio_slider_save_action(
  storage: Option<Storage>,
  settings_store: Option<Store<AppSettings>>,
  session: Option<ServerSession>,
  setting: AudioSliderSetting,
) -> AudioSliderSaveAction {
  Arc::new(move |value| {
    if let Some(settings_store) = settings_store.as_ref() {
      let _ = save_slider_setting(settings_store, storage.as_ref(), setting, value);
      if matches!(setting, AudioSliderSetting::VoiceNormalizationTargetLevel) {
        if let Some(session) = session.as_ref() {
          session.set_voice_normalization_target_level(value);
        }
      } else if matches!(setting, AudioSliderSetting::PushToTalkReleaseDelay)
        && let Some(session) = session.as_ref()
      {
        session.set_push_to_talk_release_delay_ms(tenths_to_delay_ms(value));
      }
    }
  })
}

fn restart_voice_for_audio_setting(settings_store: &Store<AppSettings>, session: Option<&ServerSession>) {
  let Some(session) = session else {
    return;
  };
  if !session.voice_active() {
    return;
  }
  let settings = settings_store.get();
  let session = session.clone();
  if let Err(error) = thread::Builder::new()
    .name("parties-voice-device-restart".to_owned())
    .spawn(move || {
      if !session.voice_active() || session.selected_channel_id().is_none() {
        return;
      }
      if let Err(error) = session.start_voice(settings, "") {
        tracing::debug!(target: "voice", "[voice] failed to restart local voice engine after audio setting change: {error}");
      }
    })
  {
    tracing::debug!(target: "voice", "[voice] failed to spawn audio setting restart thread: {error}");
  }
}

fn save_slider_setting(
  settings_store: &Store<AppSettings>,
  storage: Option<&Storage>,
  setting: AudioSliderSetting,
  value: i32,
) -> AppSettings {
  let value = value.clamp(0, 100);
  update_app_settings(settings_store, storage, |settings| match setting {
    AudioSliderSetting::VoiceNormalizationTargetLevel => settings.voice_normalization_target_level = value,
    AudioSliderSetting::VoiceActivationThreshold => {
      settings.voice_activation_threshold = value;
      settings.voice_activation = true;
    }
    AudioSliderSetting::PushToTalkReleaseDelay => {
      settings.push_to_talk_release_delay_ms = tenths_to_delay_ms(value);
    }
  })
}

fn delay_ms_to_tenths(value: i32) -> i32 {
  (value.clamp(0, 2000) + PUSH_TO_TALK_RELEASE_DELAY_STEP_MS / 2) / PUSH_TO_TALK_RELEASE_DELAY_STEP_MS
}

fn tenths_to_delay_ms(value: i32) -> i32 {
  value.clamp(0, PUSH_TO_TALK_RELEASE_DELAY_MAX_TENTHS) * PUSH_TO_TALK_RELEASE_DELAY_STEP_MS
}

fn save_audio_bool_setting(
  settings_store: &Store<AppSettings>,
  storage: Option<&Storage>,
  setting: AudioBoolSetting,
  value: bool,
) {
  update_app_settings(settings_store, storage, |settings| match setting {
    AudioBoolSetting::NoiseCancellation => settings.noise_cancellation = value,
    AudioBoolSetting::VoiceNormalization => settings.voice_normalization = value,
    AudioBoolSetting::EchoCancellation => settings.echo_cancellation = value,
    AudioBoolSetting::PushToTalk => {
      settings.push_to_talk = value;
      settings.voice_activation = true;
    }
  });
}

fn save_audio_string_setting(
  settings_store: &Store<AppSettings>,
  storage: Option<&Storage>,
  setting: AudioStringSetting,
  value: String,
) -> AppSettings {
  update_app_settings(settings_store, storage, |settings| match setting {
    AudioStringSetting::AudioInputDevice => settings.audio_input_device = value,
    AudioStringSetting::AudioOutputDevice => settings.audio_output_device = value,
    AudioStringSetting::HotkeyPushToTalk => settings.hotkey_push_to_talk = value,
    AudioStringSetting::HotkeyToggleMute => settings.hotkey_toggle_mute = value,
    AudioStringSetting::HotkeyToggleDeafen => settings.hotkey_toggle_deafen = value,
  })
}

fn hotkey_button(
  ctx: &mut Ctx,
  not_set_label: &str,
  recording_label: &str,
  value: Signal<String>,
  recording: Signal<bool>,
  suppress_next_click: Signal<bool>,
  mouse_capture: Arc<Mutex<Option<GlobalMouseHotkeyCapture>>>,
  global_hotkeys: Option<GlobalVoiceHotkeys>,
) -> Element {
  let is_recording = recording.get();
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
  let click_recording = recording.clone();
  let click_suppress_next_click = suppress_next_click.clone();
  let click_value = value.clone();
  let key_recording = recording.clone();
  let key_value = value.clone();
  let click_capture = mouse_capture.clone();
  let click_global_hotkeys = global_hotkeys.clone();
  let key_capture = mouse_capture.clone();
  let key_global_hotkeys = global_hotkeys.clone();
  let clear_capture = mouse_capture.clone();
  let clear_global_hotkeys = global_hotkeys.clone();
  let clear_value = value.clone();
  let clear_recording = recording.clone();
  let has_value = !current.trim().is_empty();

  let hotkey = Button::empty()
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
    .on_click(move |event: MouseEvent| {
      if event.button != MouseButton::Left {
        return;
      }
      if let Some(hotkey) = take_mouse_hotkey_capture(&click_capture) {
        click_value.set(hotkey);
        click_recording.set(false);
        click_suppress_next_click.set(true);
        return;
      }
      if click_suppress_next_click.get_untracked() {
        click_suppress_next_click.set(false);
        return;
      }
      if click_recording.get_untracked() {
        return;
      }
      click_recording.set(true);
      if let Some(global_hotkeys) = click_global_hotkeys.as_ref() {
        let capture = global_hotkeys.begin_mouse_capture();
        *click_capture.lock().expect("audio hotkey capture lock poisoned") = Some(capture);
      }
    })
    .on_key_down(move |event: KeyboardEvent| {
      if !key_recording.get_untracked() {
        return;
      }
      if hotkeys::is_cancel_key(&event) {
        key_recording.set(false);
        cancel_mouse_hotkey_capture(&key_capture, key_global_hotkeys.as_ref());
      } else if hotkeys::is_clear_key(&event) {
        key_value.set(String::new());
        key_recording.set(false);
        cancel_mouse_hotkey_capture(&key_capture, key_global_hotkeys.as_ref());
      } else if let Some(hotkey) = hotkeys::event_to_hotkey(&event) {
        key_value.set(hotkey);
        key_recording.set(false);
        cancel_mouse_hotkey_capture(&key_capture, key_global_hotkeys.as_ref());
      }
    })
    .child(
      Text::styled(label, input_level_label_style())
        .width(Dimension::Pct(100.0))
        .align(Alignment::Center),
    );

  let mut clear = Button::empty()
    .width(36.0)
    .height(36.0)
    .align_items(Alignment::Center)
    .justify(lurq::layout::layout_kind::Justify::Center)
    .rounded(theme::RadiusSize::Md)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceInput))
    .border_inside(1.0, theme::PaletteColor::Border)
    .child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon: "x",
      size: 15.0,
      color: if has_value || is_recording {
        theme::palette().text_secondary
      } else {
        theme::palette().text_muted.with_opacity(0.45)
      },
    }));

  if has_value || is_recording {
    clear = clear
      .cursor(CursorIcon::Pointer)
      .hovered_style(Style::new().background(BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised)))
      .active_style(Style::new().background(BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised)))
      .on_click(move |_| {
        clear_value.set(String::new());
        clear_recording.set(false);
        cancel_mouse_hotkey_capture(&clear_capture, clear_global_hotkeys.as_ref());
      });
  }

  Row::new()
    .width(192.0)
    .height(36.0)
    .align_items(Alignment::Center)
    .spacing(6.0)
    .child(hotkey)
    .child(clear)
    .into()
}

fn cancel_mouse_hotkey_capture(
  capture: &Arc<Mutex<Option<GlobalMouseHotkeyCapture>>>,
  global_hotkeys: Option<&GlobalVoiceHotkeys>,
) {
  *capture.lock().expect("audio hotkey capture lock poisoned") = None;
  if let Some(global_hotkeys) = global_hotkeys {
    global_hotkeys.cancel_mouse_capture();
  }
}

fn take_mouse_hotkey_capture(capture: &Arc<Mutex<Option<GlobalMouseHotkeyCapture>>>) -> Option<String> {
  let mut capture = capture.lock().expect("audio hotkey capture lock poisoned");
  let captured = capture.as_ref().and_then(GlobalMouseHotkeyCapture::take_hotkey);
  if captured.is_some() {
    *capture = None;
  }
  captured
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
