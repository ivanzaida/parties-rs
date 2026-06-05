use lurq::{
  components::{Row, Text},
  layout::{Alignment, layout_kind::Justify, text_style::FontWeight},
  node::{BackgroundColor, color::Color},
};

use crate::{
  screens::shared::styled_text,
  theme::{self},
};

pub(super) fn mono_label(content: &str, size: f32, color: impl Into<Color>) -> Text {
  styled_text(content, "JetBrains Mono", size, FontWeight::Bold, color, 1.2)
}

pub(super) fn badge(label: &str, color: impl Into<Color>, background: impl Into<BackgroundColor>) -> Row {
  Row::new()
    .height(22.0)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .padding_horizontal(7.0)
    .rounded(3.0)
    .background(background)
    .border_inside(1.0, theme::PaletteColor::Border)
    .child(mono_label(label, 9.0, color))
}
