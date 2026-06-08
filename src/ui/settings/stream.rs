use std::sync::Arc;

use lurq::{
  app::{
    component::{Component, ComponentInfo, DevtoolsInspectable},
    ctx::Ctx,
  },
  components::{Column, Row, ScrollVertical, Stack, Text},
  core::Signal,
  layout::{Alignment, StackAlignment, text_style::TextStyle},
  node::{Element, dimension::Dimension},
};

use crate::{
  services::webcam_devices::{self, WebcamDevice},
  storage::{AppSettings, Storage},
  theme,
  ui::{
    common::{
      dropdown_menu::{DropdownOption, dropdown_menu},
      slider as app_slider,
    },
    settings::{
      audio::{
        AUDIO_CONTROL_VALUE_SPACING, AUDIO_CONTROL_WIDTH, DEVICE_DROPDOWN_WIDTH, audio_row, audio_scrollbar_style,
        audio_section_label,
      },
      refresh_button::{REFRESH_BUTTON_SIZE, REFRESH_BUTTON_SPACING, refresh_button},
      shell::{SettingsPage, header, page_stack, screen_full, settings_content_padding, settings_section_spacing},
    },
  },
};

const VIDEO_BITRATE_MIN: f32 = 0.0;
const VIDEO_BITRATE_MAX: f32 = 20.0;
const VIDEO_BITRATE_STEP: f32 = 0.5;
const VIDEO_VALUE_WIDTH: f32 = 64.0;
const VIDEO_SLIDER_WIDTH: f32 = AUDIO_CONTROL_WIDTH - VIDEO_VALUE_WIDTH - AUDIO_CONTROL_VALUE_SPACING;

pub struct SettingsStreamScreen {
  webcam_device: Signal<String>,
  codec: String,
  scale_percent: String,
  fps: String,
  bitrate_mbps: f32,
  webcam_devices: Signal<Vec<WebcamDevice>>,
}

impl Component for SettingsStreamScreen {
  type Props = ();

  fn create(ctx: &mut Ctx) -> Self {
    let storage = ctx.use_context::<Storage>();
    let settings = storage
      .as_ref()
      .and_then(|storage| storage.load_settings().ok())
      .unwrap_or_else(AppSettings::default);

    let webcam_device = ctx.signal(settings.video_webcam_device);
    let codec = video_codec_value(&settings.video_codec);
    let scale_percent = settings.video_scale_percent.clamp(25, 100).to_string();
    let fps = settings.video_fps.clamp(15, 120).to_string();
    let bitrate_mbps = settings.video_bitrate_mbps.clamp(VIDEO_BITRATE_MIN, VIDEO_BITRATE_MAX);
    let webcam_devices = ctx.signal(webcam_devices::webcam_devices());

    Self {
      webcam_device,
      codec,
      scale_percent,
      fps,
      bitrate_mbps,
      webcam_devices,
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let storage = ctx.use_context::<Storage>();
    let (padding_x, padding_y) = settings_content_padding(ctx);
    let section_spacing = settings_section_spacing(ctx);
    let content = ScrollVertical::new(
      page_stack(ctx)
        .padding_vertical(padding_y)
        .padding_horizontal(padding_x)
        .child(header(
          &ctx.t("settings.video.title"),
          &ctx.t("settings.video.description"),
        ))
        .child(
          Column::new()
            .width(Dimension::Pct(100.0))
            .spacing(section_spacing)
            .child(
              Column::new()
                .width(Dimension::Pct(100.0))
                .spacing(12.0)
                .child(audio_section_label(&ctx.t("settings.video.section.camera")))
                .child(ctx.mount::<WebcamSetting>(WebcamSettingProps {
                  selected: self.webcam_device.clone(),
                  devices: self.webcam_devices.clone(),
                })),
            )
            .child(
              Column::new()
                .width(Dimension::Pct(100.0))
                .spacing(12.0)
                .child(audio_section_label(&ctx.t("settings.video.section.screen_sharing")))
                .child(ctx.mount::<VideoDropdownSetting>(VideoDropdownSettingProps {
                  kind: VideoDropdownKind::Codec,
                  initial_value: self.codec.clone(),
                }))
                .child(ctx.mount::<VideoDropdownSetting>(VideoDropdownSettingProps {
                  kind: VideoDropdownKind::Scale,
                  initial_value: self.scale_percent.clone(),
                }))
                .child(ctx.mount::<VideoDropdownSetting>(VideoDropdownSettingProps {
                  kind: VideoDropdownKind::Fps,
                  initial_value: self.fps.clone(),
                }))
                .child(ctx.mount::<VideoBitrateSetting>(VideoBitrateSettingProps {
                  initial_value: self.bitrate_mbps,
                  on_blur: video_bitrate_save_action(storage.clone()),
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

    screen_full(ctx, SettingsPage::Stream, content)
  }
}

#[derive(Clone, lurq::DevtoolsInspectable)]
struct WebcamSettingProps {
  selected: Signal<String>,
  devices: Signal<Vec<WebcamDevice>>,
}

impl PartialEq for WebcamSettingProps {
  fn eq(&self, other: &Self) -> bool {
    self.selected.id() == other.selected.id() && self.devices.id() == other.devices.id()
  }
}

struct WebcamSetting {
  value: Signal<String>,
}

impl Component for WebcamSetting {
  type Props = WebcamSettingProps;

  fn create(ctx: &mut Ctx) -> Self {
    let props = ctx.props::<Self::Props>().clone();
    let value = ctx.signal(props.selected.get_untracked());
    let storage = ctx.use_context::<Storage>();

    ctx.watch(&value, move |value| {
      props.selected.set(value.clone());
      if let Some(storage) = storage.as_ref() {
        let mut settings = storage.load_settings().unwrap_or_default();
        settings.video_webcam_device = value.clone();
        let _ = storage.save_settings(&settings);
      }
    });

    Self { value }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let props = ctx.props::<Self::Props>().clone();
    let system_default = ctx.t("settings.video.device.system").to_string();
    let devices = props.devices.get();
    let selected = self.value.get();

    audio_row(
      &ctx.t("settings.video.webcam"),
      &ctx.t("settings.video.webcam.description"),
      webcam_control(
        ctx,
        self.value.clone(),
        props.devices,
        webcam_options(&system_default, &devices, &selected),
        &system_default,
      ),
      true,
    )
  }
}

#[derive(Clone, Copy, PartialEq, Eq, lurq::DevtoolsInspectable)]
enum VideoDropdownKind {
  Codec,
  Scale,
  Fps,
}

#[derive(Clone, lurq::DevtoolsInspectable)]
struct VideoDropdownSettingProps {
  kind: VideoDropdownKind,
  initial_value: String,
}

impl PartialEq for VideoDropdownSettingProps {
  fn eq(&self, other: &Self) -> bool {
    self.kind == other.kind && self.initial_value == other.initial_value
  }
}

struct VideoDropdownSetting {
  value: Signal<String>,
}

impl Component for VideoDropdownSetting {
  type Props = VideoDropdownSettingProps;

  fn create(ctx: &mut Ctx) -> Self {
    let props = ctx.props::<Self::Props>().clone();
    let value = ctx.signal(props.initial_value.clone());
    if let Some(storage) = ctx.use_context::<Storage>() {
      ctx.watch(&value, move |value| {
        save_video_dropdown_setting(&storage, props.kind, value);
      });
    }
    Self { value }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let props = ctx.props::<Self::Props>().clone();
    let (title_key, description_key, placeholder, options) = match props.kind {
      VideoDropdownKind::Codec => (
        "settings.video.codec",
        "settings.video.codec.description",
        ctx.t("settings.video.codec.placeholder").to_string(),
        codec_options(),
      ),
      VideoDropdownKind::Scale => (
        "settings.video.scale",
        "settings.video.scale.description",
        ctx.t("settings.video.scale.source").to_string(),
        scale_options(ctx),
      ),
      VideoDropdownKind::Fps => (
        "settings.video.fps",
        "settings.video.fps.description",
        ctx.t("settings.video.fps.placeholder").to_string(),
        fps_options(ctx),
      ),
    };

    audio_row(
      &ctx.t(title_key),
      &ctx.t(description_key),
      dropdown_menu(self.value.clone(), options, &placeholder, DEVICE_DROPDOWN_WIDTH),
      true,
    )
  }
}

type VideoBitrateSaveAction = Arc<dyn Fn(f32) + Send + Sync>;

#[derive(Clone)]
struct VideoBitrateSettingProps {
  initial_value: f32,
  on_blur: VideoBitrateSaveAction,
}

impl PartialEq for VideoBitrateSettingProps {
  fn eq(&self, other: &Self) -> bool {
    (self.initial_value - other.initial_value).abs() < f32::EPSILON
  }
}

impl DevtoolsInspectable for VideoBitrateSettingProps {
  fn write_info(&self, buffer: &mut Vec<ComponentInfo>) {
    buffer.push(ComponentInfo::with_value(
      "initial_value",
      std::any::type_name::<f32>(),
      format_bitrate_value(self.initial_value),
    ));
  }
}

struct VideoBitrateSetting {
  value: Signal<f32>,
}

impl Component for VideoBitrateSetting {
  type Props = VideoBitrateSettingProps;

  fn create(ctx: &mut Ctx) -> Self {
    Self {
      value: ctx.signal(ctx.props::<Self::Props>().initial_value),
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let props = ctx.props::<Self::Props>().clone();
    audio_row(
      &ctx.t("settings.video.bitrate"),
      &ctx.t("settings.video.bitrate.description"),
      bitrate_slider(self.value.clone(), props.on_blur, &ctx.t("settings.video.bitrate.unit")),
      true,
    )
  }
}

fn webcam_control(
  ctx: &mut Ctx,
  selected: Signal<String>,
  devices: Signal<Vec<WebcamDevice>>,
  options: Vec<DropdownOption>,
  system_default: &str,
) -> Element {
  let dropdown_width = DEVICE_DROPDOWN_WIDTH - REFRESH_BUTTON_SIZE - REFRESH_BUTTON_SPACING;

  Row::new()
    .width(DEVICE_DROPDOWN_WIDTH)
    .align_items(Alignment::Center)
    .spacing(REFRESH_BUTTON_SPACING)
    .child(refresh_button(ctx, move |_| {
      devices.set(webcam_devices::webcam_devices());
    }))
    .child(dropdown_menu(selected, options, system_default, dropdown_width))
    .into()
}

fn webcam_options(system_default: &str, devices: &[WebcamDevice], selected: &str) -> Vec<DropdownOption> {
  let mut options = vec![DropdownOption {
    value: String::new(),
    label: system_default.to_owned(),
  }];

  if !selected.trim().is_empty() && !devices.iter().any(|device| device.value == selected) {
    options.push(DropdownOption {
      value: selected.to_owned(),
      label: selected.to_owned(),
    });
  }

  options.extend(devices.iter().map(|device| DropdownOption {
    value: device.value.clone(),
    label: device.label.clone(),
  }));

  options
}

fn codec_options() -> Vec<DropdownOption> {
  outgoing_codec_options()
    .into_iter()
    .map(|codec| DropdownOption {
      value: codec.to_owned(),
      label: codec.to_owned(),
    })
    .collect()
}

fn scale_options(ctx: &mut Ctx) -> Vec<DropdownOption> {
  [
    ("100", ctx.t("settings.video.scale.source").to_string()),
    ("75", "75%".to_owned()),
    ("50", "50%".to_owned()),
    ("25", "25%".to_owned()),
  ]
  .into_iter()
  .map(|(value, label)| DropdownOption {
    value: value.to_owned(),
    label,
  })
  .collect()
}

fn fps_options(ctx: &mut Ctx) -> Vec<DropdownOption> {
  [15, 30, 60, 120]
    .into_iter()
    .map(|fps| DropdownOption {
      value: fps.to_string(),
      label: ctx
        .t_args("settings.video.fps.value", [("value", fps.to_string())])
        .to_string(),
    })
    .collect()
}

fn bitrate_slider(value: Signal<f32>, on_blur: VideoBitrateSaveAction, unit: &str) -> Element {
  let current = value.get().clamp(VIDEO_BITRATE_MIN, VIDEO_BITRATE_MAX);
  let fill_width = VIDEO_SLIDER_WIDTH * (current - VIDEO_BITRATE_MIN) / (VIDEO_BITRATE_MAX - VIDEO_BITRATE_MIN);
  let value_label = format!("{} {unit}", format_bitrate_value(current));

  let mut slider = app_slider::slider_f32(
    value.clone(),
    VIDEO_SLIDER_WIDTH,
    VIDEO_BITRATE_MIN,
    VIDEO_BITRATE_MAX,
    VIDEO_BITRATE_STEP,
  );

  slider = slider.on_blur(move || {
    on_blur(value.get_untracked());
  });

  Row::new()
    .width(AUDIO_CONTROL_WIDTH)
    .align_items(Alignment::Center)
    .spacing(AUDIO_CONTROL_VALUE_SPACING)
    .child(
      Stack::new()
        .stack_align(StackAlignment::CenterStart)
        .width(VIDEO_SLIDER_WIDTH)
        .height(app_slider::SLIDER_HEIGHT)
        .child(app_slider::track(VIDEO_SLIDER_WIDTH))
        .child(app_slider::fill(fill_width))
        .child(slider),
    )
    .child(video_value_label(&value_label))
    .into()
}

fn video_bitrate_save_action(storage: Option<Storage>) -> VideoBitrateSaveAction {
  Arc::new(move |value| {
    if let Some(storage) = storage.as_ref() {
      let mut settings = storage.load_settings().unwrap_or_default();
      settings.video_bitrate_mbps = value.clamp(VIDEO_BITRATE_MIN, VIDEO_BITRATE_MAX);
      let _ = storage.save_settings(&settings);
    }
  })
}

fn save_video_dropdown_setting(storage: &Storage, setting: VideoDropdownKind, value: &str) {
  let mut settings = storage.load_settings().unwrap_or_default();

  match setting {
    VideoDropdownKind::Codec => settings.video_codec = video_codec_value(value),
    VideoDropdownKind::Scale => settings.video_scale_percent = parse_i32(value, 100).clamp(25, 100),
    VideoDropdownKind::Fps => settings.video_fps = parse_i32(value, 60).clamp(15, 120),
  }

  let _ = storage.save_settings(&settings);
}

fn format_bitrate_value(value: f32) -> String {
  let rounded_half = (value * 2.0).round() / 2.0;
  if rounded_half.fract().abs() < f32::EPSILON {
    format!("{}", rounded_half as i32)
  } else {
    format!("{rounded_half:.1}")
  }
}

fn video_value_label(label: &str) -> Element {
  Text::styled(label, video_value_label_style())
    .width(VIDEO_VALUE_WIDTH)
    .text_align(Alignment::End)
    .nowrap()
    .into()
}

fn video_value_label_style() -> TextStyle {
  TextStyle {
    font_family: Arc::from("JetBrains Mono"),
    font_size: 11.0,
    line_height: 1.2,
    color: theme::palette().text_muted,
    ..TextStyle::default()
  }
}

fn video_codec_value(value: &str) -> String {
  let value = match value.trim() {
    "H.265" | "H.264" => value.trim().to_owned(),
    #[cfg(not(target_os = "macos"))]
    "AV1" => "AV1".to_owned(),
    _ => default_outgoing_codec().to_owned(),
  };
  if outgoing_codec_options().contains(&value.as_str()) {
    value
  } else {
    default_outgoing_codec().to_owned()
  }
}

fn outgoing_codec_options() -> Vec<&'static str> {
  #[cfg(target_os = "macos")]
  {
    vec!["H.265", "H.264"]
  }
  #[cfg(not(target_os = "macos"))]
  {
    vec!["AV1", "H.265", "H.264"]
  }
}

fn default_outgoing_codec() -> &'static str {
  #[cfg(target_os = "macos")]
  {
    "H.265"
  }
  #[cfg(not(target_os = "macos"))]
  {
    "AV1"
  }
}

fn parse_i32(value: &str, fallback: i32) -> i32 {
  value.parse().unwrap_or(fallback)
}
