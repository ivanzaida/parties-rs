use std::sync::Arc;

use lurq::{
  app::{
    component::{Component, ComponentInfo, DevtoolsInspectable},
    ctx::Ctx,
  },
  components::{Column, Row, Text},
  core::Signal,
  layout::{
    Alignment,
    layout_kind::Justify,
    text_style::{FontWeight, TextStyle},
  },
  node::{BackgroundColor, CursorIcon, Element, Style, color::Color, dimension::Dimension},
};

use crate::{
  theme,
  ui::common::lucide_icon::{LucideIcon, LucideIconProps},
};

pub type ConfirmAction = Arc<dyn Fn() + Send + Sync>;

#[derive(Clone)]
pub struct ConfirmModalProps {
  pub open: Signal<bool>,
  pub icon: &'static str,
  pub title: Arc<str>,
  pub body: Arc<str>,
  pub warning: Option<Arc<str>>,
  pub cancel_label: Arc<str>,
  pub confirm_label: Arc<str>,
  pub on_confirm: ConfirmAction,
}

impl PartialEq for ConfirmModalProps {
  fn eq(&self, other: &Self) -> bool {
    self.icon == other.icon
      && self.title == other.title
      && self.body == other.body
      && self.warning == other.warning
      && self.cancel_label == other.cancel_label
      && self.confirm_label == other.confirm_label
  }
}

impl DevtoolsInspectable for ConfirmModalProps {
  fn write_info(&self, buffer: &mut Vec<ComponentInfo>) {
    buffer.push(ComponentInfo::with_value("title", "Arc<str>", self.title.to_string()));
    buffer.push(ComponentInfo::with_value("body", "Arc<str>", self.body.to_string()));
    buffer.push(ComponentInfo::with_value(
      "confirm_label",
      "Arc<str>",
      self.confirm_label.to_string(),
    ));
  }
}

pub struct ConfirmModal;

impl Component for ConfirmModal {
  type Props = ConfirmModalProps;

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let props = ctx.props::<Self::Props>().clone();
    let window = ctx.window();
    let dialog_width = (window.logical_width() - 32.0).min(420.0).max(280.0);
    let close_signal = props.open.clone();
    let confirm_open = props.open.clone();
    let on_confirm = props.on_confirm.clone();
    let mut panel = Column::new()
      .width(dialog_width)
      .spacing(16.0)
      .padding(24.0)
      .rounded(12.0)
      .background(BackgroundColor::Color(Color::from_hex("#15171A")))
      .border_inside(1.0, BackgroundColor::Color(Color::from_hex("#3A4047")))
      .child(icon_badge(ctx, props.icon))
      .child(copy_block(&props.title, &props.body));

    if let Some(warning) = props.warning.as_deref() {
      panel = panel.child(warning_block(ctx, warning, (dialog_width - 106.0).max(160.0)));
    }

    panel = panel.child(
      Row::new()
        .width(Dimension::Pct(100.0))
        .justify(Justify::End)
        .align_items(Alignment::Center)
        .spacing(10.0)
        .child(
          modal_button(ctx, None, &props.cancel_label, ModalButtonTone::Neutral).on_click(move |_| {
            close_signal.set(false);
          }),
        )
        .child(
          modal_button(ctx, Some(props.icon), &props.confirm_label, ModalButtonTone::Danger).on_click(move |_| {
            confirm_open.set(false);
            on_confirm();
          }),
        ),
    );

    Column::new()
      .width(window.logical_width())
      .height(window.logical_height())
      .align_items(Alignment::Center)
      .justify(Justify::Center)
      .background(BackgroundColor::Color(Color::from_hex("#00000099")))
      .child(panel)
  }
}

fn icon_badge(ctx: &mut Ctx, icon: &'static str) -> Row {
  Row::new()
    .width(44.0)
    .height(44.0)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .rounded(10.0)
    .background(BackgroundColor::Color(Color::from_hex("#2A1A1C")))
    .child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon,
      size: 20.0,
      color: theme::palette().danger,
    }))
}

fn copy_block(title: &str, body: &str) -> Column {
  Column::new()
    .width(Dimension::Pct(100.0))
    .spacing(8.0)
    .child(Text::styled(title, title_style()).width(Dimension::Pct(100.0)))
    .child(Text::styled(body, body_style()).width(Dimension::Pct(100.0)))
}

fn warning_block(ctx: &mut Ctx, warning: &str, text_width: f32) -> Element {
  Row::new()
    .width(Dimension::Pct(100.0))
    .align_items(Alignment::Center)
    .spacing(10.0)
    .padding_vertical(12.0)
    .padding_horizontal(14.0)
    .rounded(8.0)
    .background(BackgroundColor::Color(Color::from_hex("#2B2418")))
    .border_inside(1.0, BackgroundColor::Color(Color::from_hex("#D6B25E59")))
    .child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon: "triangle-alert",
      size: 16.0,
      color: Color::from_hex("#D6B25E"),
    }))
    .child(Text::styled(warning, warning_style()).width(text_width))
    .into()
}

#[derive(Clone, Copy)]
enum ModalButtonTone {
  Neutral,
  Danger,
}

fn modal_button(ctx: &mut Ctx, icon: Option<&'static str>, label: &str, tone: ModalButtonTone) -> Row {
  let (background, border, text_color, icon_color, hover) = match tone {
    ModalButtonTone::Neutral => (
      BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised),
      BackgroundColor::Color(Color::from_hex("#3A4047")),
      theme::palette().text_primary,
      theme::palette().text_secondary,
      BackgroundColor::Palette(theme::PaletteColor::SurfaceInput),
    ),
    ModalButtonTone::Danger => (
      BackgroundColor::Color(theme::palette().danger),
      BackgroundColor::Color(theme::palette().danger),
      theme::palette().surface_base,
      theme::palette().surface_base,
      BackgroundColor::Color(theme::palette().danger.with_opacity(0.86)),
    ),
  };

  let mut button = Row::new()
    .height(34.0)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .spacing(7.0)
    .padding_horizontal(14.0)
    .rounded(5.0)
    .background(background)
    .border_inside(1.0, border)
    .cursor(CursorIcon::Pointer)
    .hovered_style(Style::new().background(hover.clone()))
    .active_style(Style::new().background(hover));

  if let Some(icon) = icon {
    button = button.child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon,
      size: 16.0,
      color: icon_color,
    }));
  }

  button.child(Text::styled(label, button_style(text_color)))
}

fn title_style() -> TextStyle {
  TextStyle {
    font_family: Arc::from("Inter"),
    font_size: 18.0,
    line_height: 1.2,
    weight: FontWeight::Bold,
    color: theme::palette().text_primary,
    ..TextStyle::default()
  }
}

fn body_style() -> TextStyle {
  TextStyle {
    font_family: Arc::from("Inter"),
    font_size: 14.0,
    line_height: 1.45,
    color: theme::palette().text_secondary,
    ..TextStyle::default()
  }
}

fn warning_style() -> TextStyle {
  TextStyle {
    font_family: Arc::from("Inter"),
    font_size: 13.0,
    line_height: 1.35,
    weight: FontWeight::Medium,
    color: Color::from_hex("#D6B25E"),
    ..TextStyle::default()
  }
}

fn button_style(color: Color) -> TextStyle {
  TextStyle {
    font_family: Arc::from("Inter"),
    font_size: 13.0,
    line_height: 1.2,
    weight: FontWeight::Bold,
    color,
    ..TextStyle::default()
  }
}
