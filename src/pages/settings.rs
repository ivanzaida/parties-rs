use lurq::{
  app::{component::Component, ctx::Ctx},
  components::{Column, Row, Text, Rect},
  layout::Alignment,
  node::{dimension::Dimension, BackgroundColor, Element, color::Color},
};

use crate::theme;

pub fn settings_nav(active: &str) -> Column {
  let tab = |label: &str, is_active: bool| -> Row {
    let row = Row::new()
      .width(Dimension::Pct(100.0))
      .height(32.0)
      .align_items(Alignment::Center)
      .padding_horizontal(10.0)
      .rounded(4.0)
      .child(Text::new(label).variant(theme::TYP_BODY));
    if is_active { row.background(BackgroundColor::Palette(theme::BG_ELEVATED)) } else { row }
  };

  Column::new()
    .width(200.0)
    .flex(1.0)
    .background(BackgroundColor::Palette(theme::BG_SECONDARY))
    .border_inside(1.0, Color::from_hex("#343A50"))
    .padding(32.0)
    .padding_horizontal(20.0)
    .spacing(4.0)
    .child(Text::new("Settings").variant(theme::TYP_HEADING))
    .child(Rect::new(1.0, 12.0))
    .child(tab("Audio", active == "Audio"))
    .child(tab("Identity", active == "Identity"))
    .child(tab("Appearance", active == "Appearance"))
    .child(tab("About", active == "About"))
}

pub struct Settings;

impl Component for Settings {
  type Props = ();
  fn create(_ctx: &mut Ctx) -> Self { Self }

  fn render(&self, _ctx: &mut Ctx) -> impl Into<Element> {
    Row::new()
      .width(Dimension::Pct(100.0))
      .flex(1.0)
      .align_items(Alignment::Stretch)
      .child(settings_nav("Audio"))
      .child(
        Column::new()
          .flex(1.0)
          .padding(32.0)
          .padding_horizontal(48.0)
          .spacing(28.0)
          .child(
            Column::new()
              .width(Dimension::Pct(100.0))
              .spacing(16.0)
              .child(Text::new("AUDIO").variant(theme::TYP_SECTION))
              .child(Text::new("Input / output device configuration").variant(theme::TYP_DESC)),
          )
          .child(Rect::new(Dimension::Pct(100.0), 1.0).background(BackgroundColor::Palette(theme::BORDER)))
          .child(
            Column::new()
              .width(Dimension::Pct(100.0))
              .spacing(16.0)
              .child(Text::new("HOTKEYS").variant(theme::TYP_SECTION))
              .child(Text::new("Push-to-talk and mute bindings").variant(theme::TYP_DESC)),
          )
          .child(Rect::new(Dimension::Pct(100.0), 1.0).background(BackgroundColor::Palette(theme::BORDER)))
          .child(
            Column::new()
              .width(Dimension::Pct(100.0))
              .spacing(16.0)
              .child(Text::new("SCREEN SHARING").variant(theme::TYP_SECTION))
              .child(Text::new("Capture and encoding settings").variant(theme::TYP_DESC)),
          ),
      )
  }
}
