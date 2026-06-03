use lurq::{
  app::{component::Component, ctx::Ctx},
  components::{Column, Row, Text, Rect},
  layout::Alignment,
  node::{dimension::Dimension, BackgroundColor, Element},
};

use crate::{pages::settings::settings_nav, theme};

fn info_row(label: &str, value: &str) -> Row {
  Row::new()
    .width(Dimension::Pct(100.0))
    .spacing(8.0)
    .child(Text::new(label).variant(theme::TYP_FIELD_LABEL))
    .child(Text::new(value).variant(theme::TYP_MONO))
}

pub struct SettingsAbout;

impl Component for SettingsAbout {
  type Props = ();
  fn create(_ctx: &mut Ctx) -> Self { Self }

  fn render(&self, _ctx: &mut Ctx) -> impl Into<Element> {
    Row::new()
      .width(Dimension::Pct(100.0))
      .flex(1.0)
      .align_items(Alignment::Stretch)
      .child(settings_nav("About"))
      .child(
        Column::new()
          .flex(1.0)
          .padding(32.0)
          .padding_horizontal(48.0)
          .spacing(24.0)
          .child(Text::new("ABOUT").variant(theme::TYP_SECTION))
          .child(
            Column::new()
              .width(400.0)
              .spacing(14.0)
              .child(info_row("Version", "0.1.0"))
              .child(info_row("Protocol", "parties/1"))
              .child(info_row("Runtime", "Rust")),
          )
          .child(Rect::new(Dimension::Pct(100.0), 1.0).background(BackgroundColor::Palette(theme::BORDER)))
          .child(
            Row::new()
              .width(Dimension::Pct(100.0))
              .spacing(20.0)
              .child(Text::new("Source code").variant(theme::TYP_LINK))
              .child(Text::new("Report issue").variant(theme::TYP_LINK)),
          ),
      )
  }
}
