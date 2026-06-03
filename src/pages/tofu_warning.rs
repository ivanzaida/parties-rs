use lurq::{
  app::{component::Component, ctx::Ctx},
  components::{Column, Row, Text},
  layout::{Alignment, layout_kind::Justify},
  node::{dimension::Dimension, BackgroundColor, Element, color::Color},
};

use crate::theme;

fn fingerprint_block(label: &str, value: &str) -> Column {
  Column::new()
    .spacing(4.0)
    .child(Text::new(label).variant(theme::TYP_FIELD_LABEL))
    .child(Text::new(value).variant(theme::TYP_MONO))
}

fn action_btn(label: &str, border_color: &str) -> Column {
  Column::new()
    .height(38.0)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .padding_horizontal(20.0)
    .background(BackgroundColor::Palette(theme::BG_SECONDARY))
    .border_inside(1.0, Color::from_hex(border_color))
    .rounded(4.0)
    .child(Text::new(label).variant(theme::TYP_BUTTON))
}

pub struct TofuWarning;

impl Component for TofuWarning {
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
          .child(Text::new("Certificate Changed").variant(theme::TYP_TITLE))
          .child(Text::new("The server\u{2019}s TLS fingerprint does not match the one stored from your first connection. This could indicate a MITM attack, or the server regenerated its certificate.").variant(theme::TYP_DESC))
          .child(
            Column::new()
              .width(440.0)
              .spacing(16.0)
              .child(fingerprint_block("Stored", "ab:cd:12:34:ef:56:78:90:..."))
              .child(fingerprint_block("Received", "ff:ee:dd:cc:bb:aa:99:88:...")),
          )
          .child(
            Row::new()
              .spacing(10.0)
              .child(action_btn("Disconnect", "#343A50"))
              .child(action_btn("Trust anyway", "#FF4757")),
          ),
      )
  }
}
