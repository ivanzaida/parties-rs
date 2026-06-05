use std::sync::Arc;

pub use lurq::components::{FormPrimaryButton, FormPrimaryButtonProps, FormTextInput, FormTextInputProps};
use lurq::{
  components::{Column, Rect, Row, Text},
  layout::{
    Alignment,
    layout_kind::Justify,
    text_style::{FontWeight, TextStyle},
  },
  node::{BackgroundColor, CursorIcon, Element, Style, color::Color, dimension::Dimension},
};

use crate::theme;

pub const ROUTE_LOADING: &str = "/loading";
pub const ROUTE_IDENTITY_SETUP: &str = "/identity/setup";
pub const ROUTE_SEED_PHRASE: &str = "/identity/seed";
pub const ROUTE_IMPORT_PRIVATE_KEY: &str = "/identity/import";
pub const ROUTE_RESTORE_IDENTITY: &str = "/identity/restore";
pub const ROUTE_CHOOSE_SERVER: &str = "/servers";
pub const ROUTE_CONNECT_SERVER: &str = "/servers/connect";
pub const ROUTE_LOBBY: &str = "/lobby";
pub const ROUTE_TOFU_WARNING: &str = "/servers/tofu";
pub const CONTENT_HEIGHT: f32 = 520.0;
pub const INTRO_WIDTH: f32 = 280.0;
pub const CARD_WIDTH: f32 = 440.0;

pub fn identity_screen(intro: impl Into<Element>, card: impl Into<Element>) -> Column {
  Column::new()
    .width(Dimension::Pct(100.0))
    .height(Dimension::Pct(100.0))
    .justify(Justify::Center)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceBase))
    .clip()
    .child(
      Column::new()
        .width(Dimension::Pct(100.0))
        .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceBase))
        .child(
          Row::new()
            .width(Dimension::Pct(100.0))
            .height(CONTENT_HEIGHT)
            .align_items(Alignment::Center)
            .justify(Justify::Center)
            .spacing(40.0)
            .padding_horizontal(72.0)
            .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceBase))
            .child(intro)
            .child(card),
        ),
    )
}

pub(crate) fn icon(name: &str, size: f32, color: impl Into<Color>) -> Text {
  let ch = match name {
    "arrow-left" => '\u{e048}',
    "alert-circle" | "circle-alert" => '\u{e077}',
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

pub fn dot(color: impl Into<BackgroundColor>) -> Rect {
  Rect::new(8.0, 8.0).rounded(4.0).background(color)
}

pub fn notice_row(
  message: &str,
  icon_name: &str,
  icon_color: impl Into<Color>,
  background: impl Into<BackgroundColor>,
  border: impl Into<BackgroundColor>,
) -> Row {
  Row::new()
    .width(Dimension::Pct(100.0))
    .align_items(Alignment::Center)
    .spacing(8.0)
    .padding(10.0)
    .rounded(5.0)
    .background(background)
    .border_inside(1.0, border)
    .child(icon(icon_name, 14.0, icon_color))
    .child(
      styled_text(
        message,
        "Inter",
        11.0,
        FontWeight::Medium,
        theme::palette().text_secondary,
        1.2,
      )
      .flex(1.0),
    )
}

pub fn action_button(label: &str, primary: bool) -> Row {
  let background = if primary {
    BackgroundColor::Palette(theme::PaletteColor::Accent)
  } else {
    BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised)
  };
  let border = if primary {
    theme::PaletteColor::Accent
  } else {
    theme::PaletteColor::Border
  };
  let hover_bg = if primary {
    BackgroundColor::Palette(theme::PaletteColor::AccentHover)
  } else {
    BackgroundColor::Palette(theme::PaletteColor::SurfaceInput)
  };
  let label = if primary {
    styled_text(
      label,
      "Inter",
      13.0,
      FontWeight::Bold,
      theme::palette().text_inverse,
      1.2,
    )
  } else {
    Text::new(label).variant(theme::TypographyStyle::Button)
  };

  Row::new()
    .width(Dimension::Pct(100.0))
    .height(34.0)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .rounded(5.0)
    .background(background)
    .border_inside(1.0, border)
    .cursor(CursorIcon::Pointer)
    .hovered_style(Style::new().background(hover_bg))
    .child(label)
}

pub fn back_button(navigator: Option<lurq::router::Navigator>, label: &str) -> Row {
  let row = action_button(label, false);

  if let Some(navigator) = navigator {
    row.on_click(move |_| navigator.push(ROUTE_IDENTITY_SETUP))
  } else {
    row
  }
}
