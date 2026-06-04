use std::sync::Arc;

use lurq::{
  components::{Column, Rect, Row, Text},
  core::Signal,
  layout::{
    Alignment,
    layout_kind::Justify,
    text_style::{FontWeight, TextStyle},
  },
  node::{BackgroundColor, CursorIcon, Element, Style, color::Color, dimension::Dimension},
};

use crate::theme;

pub const BORDER: &str = "#30343A";
pub const STEP_IDENTITY_SETUP: u8 = 0;
pub const STEP_SEED_PHRASE: u8 = 1;
pub const STEP_IMPORT_PRIVATE_KEY: u8 = 2;
pub const STEP_RESTORE_IDENTITY: u8 = 3;
pub const STEP_CHOOSE_SERVER: u8 = 4;
pub const STEP_CONNECT_SERVER: u8 = 5;
pub const CONTENT_HEIGHT: f32 = 520.0;
pub const INTRO_WIDTH: f32 = 280.0;
pub const CARD_WIDTH: f32 = 440.0;

pub fn identity_screen(intro: impl Into<Element>, card: impl Into<Element>) -> Column {
  Column::new()
    .width(Dimension::Pct(100.0))
    .height(Dimension::Pct(100.0))
    .justify(Justify::Center)
    .background(BackgroundColor::Palette(theme::BG_PRIMARY))
    .clip()
    .child(
      Column::new()
        .width(Dimension::Pct(100.0))
        .background(BackgroundColor::Palette(theme::BG_PRIMARY))
        .child(
          Row::new()
            .width(Dimension::Pct(100.0))
            .height(CONTENT_HEIGHT)
            .align_items(Alignment::Start)
            .justify(Justify::Center)
            .spacing(40.0)
            .padding_horizontal(72.0)
            .background(BackgroundColor::Palette(theme::BG_PRIMARY))
            .child(intro)
            .child(card),
        ),
    )
}

pub(crate) fn icon(name: &str, size: f32, color: impl Into<Color>) -> Text {
  let ch = match name {
    "arrow-left" => '\u{e048}',
    "check" => '\u{e06c}',
    "chevron-right" => '\u{e06f}',
    "copy" => '\u{e09e}',
    "info" => '\u{e0f9}',
    "shield-check" => '\u{e1ff}',
    "trash-2" => '\u{e18e}',
    "alert-triangle" | "triangle-alert" => '\u{e193}',
    _ => '\u{e06f}',
  };
  let glyph = String::from(ch);

  Text::styled(
    &glyph,
    TextStyle {
      font_family: "lucide".into(),
      font_size: size,
      color: color.into(),
      ..TextStyle::default()
    },
  )
}

pub fn styled_text(
  content: &str,
  family: &str,
  size: f32,
  weight: FontWeight,
  color: impl Into<Color>,
  line_height: f32,
) -> Text {
  Text::styled(
    content,
    TextStyle {
      font_family: Arc::from(family),
      font_size: size,
      line_height,
      weight,
      color: color.into(),
      ..TextStyle::default()
    },
  )
}

pub fn text_style(family: &str, size: f32, weight: FontWeight, color: &str, line_height: f32) -> TextStyle {
  TextStyle {
    font_family: Arc::from(family),
    font_size: size,
    line_height,
    weight,
    color: Color::from_hex(color),
    ..TextStyle::default()
  }
}

pub fn dot(color: impl Into<BackgroundColor>) -> Rect {
  Rect::new(8.0, 8.0).rounded(4.0).background(color)
}

pub fn action_button(label: &str, primary: bool) -> Row {
  let background = if primary {
    BackgroundColor::Palette(theme::ACCENT)
  } else {
    BackgroundColor::Palette(theme::BG_ELEVATED)
  };
  let border = if primary { "#42D28B" } else { BORDER };
  let hover_bg = if primary {
    BackgroundColor::Palette(theme::ACCENT_HOVER)
  } else {
    BackgroundColor::Palette(theme::BG_INPUT)
  };
  let label = if primary {
    styled_text(label, "Inter", 13.0, FontWeight::Bold, theme::TEXT_INVERSE_COLOR, 1.2)
  } else {
    Text::new(label).variant(theme::TYP_BUTTON)
  };

  Row::new()
    .width(Dimension::Pct(100.0))
    .height(34.0)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .rounded(5.0)
    .background(background)
    .border_inside(1.0, Color::from_hex(border))
    .cursor(CursorIcon::Pointer)
    .hovered_style(Style::new().background(hover_bg))
    .child(label)
}

pub fn back_button(step: Option<Signal<u8>>, label: &str) -> Row {
  let row = action_button(label, false);

  if let Some(step) = step {
    row.on_click(move |_| step.set(STEP_IDENTITY_SETUP))
  } else {
    row
  }
}
