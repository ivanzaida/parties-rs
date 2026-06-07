use std::sync::Arc;

use lurq::{
  app::{component::Component, ctx::Ctx},
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
      shell::{SettingsPage, header, page_stack, screen_full},
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
  codec: Signal<String>,
  scale_percent: Signal<String>,
  fps: Signal<String>,
  bitrate_mbps: Signal<f32>,
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
    let codec = ctx.signal(video_codec_value(&settings.video_codec));
    let scale_percent = ctx.signal(settings.video_scale_percent.clamp(25, 100).to_string());
    let fps = ctx.signal(settings.video_fps.clamp(15, 120).to_string());
    let bitrate_mbps = ctx.signal(settings.video_bitrate_mbps.clamp(VIDEO_BITRATE_MIN, VIDEO_BITRATE_MAX));
    let webcam_devices = ctx.signal(webcam_devices::webcam_devices());

    if let Some(storage) = storage {
      let webcam_device_signal = webcam_device.clone();
      let codec_signal = codec.clone();
      let scale_signal = scale_percent.clone();
      let fps_signal = fps.clone();
      let bitrate_signal = bitrate_mbps.clone();
      ctx.on_effect(move || {
        let mut settings = storage.load_settings().unwrap_or_default();
        settings.video_webcam_device = webcam_device_signal.get();
        settings.video_codec = video_codec_value(&codec_signal.get());
        settings.video_scale_percent = parse_i32(&scale_signal.get(), 100).clamp(25, 100);
        settings.video_fps = parse_i32(&fps_signal.get(), 60).clamp(15, 120);
        settings.video_bitrate_mbps = bitrate_signal.get().clamp(VIDEO_BITRATE_MIN, VIDEO_BITRATE_MAX);
        let _ = storage.save_settings(&settings);
      });
    }

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
    let system_default = ctx.t("settings.video.device.system").to_string();
    let webcam_devices = self.webcam_devices.get();

    let webcam = audio_row(
      &ctx.t("settings.video.webcam"),
      &ctx.t("settings.video.webcam.description"),
      webcam_control(
        ctx,
        self.webcam_device.clone(),
        self.webcam_devices.clone(),
        webcam_options(&system_default, &webcam_devices, &self.webcam_device.get()),
        &system_default,
      ),
      true,
    );
    let codec = audio_row(
      &ctx.t("settings.video.codec"),
      &ctx.t("settings.video.codec.description"),
      dropdown_menu(
        self.codec.clone(),
        codec_options(),
        &ctx.t("settings.video.codec.placeholder"),
        DEVICE_DROPDOWN_WIDTH,
      ),
      true,
    );
    let scale = audio_row(
      &ctx.t("settings.video.scale"),
      &ctx.t("settings.video.scale.description"),
      dropdown_menu(
        self.scale_percent.clone(),
        scale_options(ctx),
        &ctx.t("settings.video.scale.source"),
        DEVICE_DROPDOWN_WIDTH,
      ),
      true,
    );
    let fps = audio_row(
      &ctx.t("settings.video.fps"),
      &ctx.t("settings.video.fps.description"),
      dropdown_menu(
        self.fps.clone(),
        fps_options(ctx),
        &ctx.t("settings.video.fps.placeholder"),
        DEVICE_DROPDOWN_WIDTH,
      ),
      true,
    );
    let bitrate = audio_row(
      &ctx.t("settings.video.bitrate"),
      &ctx.t("settings.video.bitrate.description"),
      bitrate_slider(
        self.bitrate_mbps.clone(),
        storage,
        &ctx.t("settings.video.bitrate.unit"),
      ),
      true,
    );
    let content = ScrollVertical::new(
      page_stack()
        .padding_vertical(40.0)
        .padding_horizontal(40.0)
        .child(header(
          &ctx.t("settings.video.title"),
          &ctx.t("settings.video.description"),
        ))
        .child(
          Column::new()
            .width(Dimension::Pct(100.0))
            .spacing(24.0)
            .child(
              Column::new()
                .width(Dimension::Pct(100.0))
                .spacing(12.0)
                .child(audio_section_label(&ctx.t("settings.video.section.camera")))
                .child(webcam),
            )
            .child(
              Column::new()
                .width(Dimension::Pct(100.0))
                .spacing(12.0)
                .child(audio_section_label(&ctx.t("settings.video.section.screen_sharing")))
                .child(codec)
                .child(scale)
                .child(fps)
                .child(bitrate),
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
  ["AV1", "H.265", "H.264"]
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

fn bitrate_slider(value: Signal<f32>, storage: Option<Storage>, unit: &str) -> Element {
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

  if let Some(storage) = storage {
    slider = slider.on_blur(move || {
      let mut settings = storage.load_settings().unwrap_or_default();
      settings.video_bitrate_mbps = value.get_untracked().clamp(VIDEO_BITRATE_MIN, VIDEO_BITRATE_MAX);
      let _ = storage.save_settings(&settings);
    });
  }

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
  match value.trim() {
    "H.265" | "H.264" => value.trim().to_owned(),
    _ => "AV1".to_owned(),
  }
}

fn parse_i32(value: &str, fallback: i32) -> i32 {
  value.parse().unwrap_or(fallback)
}
