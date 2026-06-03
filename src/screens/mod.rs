use lurq::{components::Text, layout::text_style::TextStyle, node::color::Color};

pub mod identity_setup;
pub mod seed_phrase_display;

pub(crate) fn icon(name: &str, size: f32, color: &str) -> Text {
  let ch = match name {
    "chevron-right" => '\u{e06f}',
    "shield-check" => '\u{e1ff}',
    _ => '\u{e06f}',
  };
  let glyph = String::from(ch);

  Text::styled(
    &glyph,
    TextStyle {
      font_family: "lucide".into(),
      font_size: size,
      color: Color::from_hex(color),
      ..TextStyle::default()
    },
  )
}
