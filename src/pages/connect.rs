use lurq::{
  app::{component::Component, ctx::Ctx},
  components::{Column, Row, Text},
  layout::{Alignment, layout_kind::Justify},
  node::{dimension::Dimension, BackgroundColor, Element, color::Color},
};

use crate::theme;

fn field(label: &str, placeholder: &str) -> Column {
  Column::new()
    .width(Dimension::Pct(100.0))
    .spacing(6.0)
    .child(Text::new(label).variant(theme::TYP_FIELD_LABEL))
    .child(
      Row::new()
        .width(Dimension::Pct(100.0))
        .height(38.0)
        .align_items(Alignment::Center)
        .padding_horizontal(12.0)
        .background(BackgroundColor::Palette(theme::BG_SECONDARY))
        .border_inside(1.0, Color::from_hex("#343A50"))
        .rounded(4.0)
        .child(Text::new(placeholder).variant(theme::TYP_LINK)),
    )
}

pub struct Connect;

impl Component for Connect {
  type Props = ();
  fn create(_ctx: &mut Ctx) -> Self { Self }

  fn render(&self, _ctx: &mut Ctx) -> impl Into<Element> {
    Column::new()
      .width(Dimension::Pct(100.0))
      .flex(1.0)
      .align_items(Alignment::Center)
      .justify(Justify::Center)
      .child(
        Column::new()
          .width(480.0)
          .spacing(24.0)
          .child(Text::new("Connect").variant(theme::TYP_TITLE))
          .child(
            Column::new()
              .width(400.0)
              .spacing(16.0)
              .child(field("Server address", "host:port"))
              .child(field("Display name", "anonymous"))
              .child(field("Invite seed (optional)", "paste invite\u{2026}")),
          )
          .child(
            Column::new()
              .width(400.0)
              .height(38.0)
              .align_items(Alignment::Center)
              .justify(Justify::Center)
              .background(BackgroundColor::Palette(theme::BG_SECONDARY))
              .border_inside(1.0, Color::from_hex("#343A50"))
              .rounded(4.0)
              .child(Text::new("Connect").variant(theme::TYP_BUTTON)),
          ),
      )
  }
}
