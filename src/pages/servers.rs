use lurq::{
  app::{component::Component, ctx::Ctx},
  components::{Column, Row, Text, Rect},
  layout::{Alignment, layout_kind::Justify},
  node::{dimension::Dimension, BackgroundColor, Element},
};

use crate::theme;

fn server_row(name: &str, addr: &str) -> Row {
  Row::new()
    .width(Dimension::Pct(100.0))
    .height(44.0)
    .align_items(Alignment::Center)
    .padding_horizontal(14.0)
    .justify(Justify::SpaceBetween)
    .child(Text::new(name).variant(theme::TYP_BUTTON))
    .child(Text::new(addr).variant(theme::TYP_LINK))
}

pub struct Servers;

impl Component for Servers {
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

          .child(
            Row::new()
              .width(Dimension::Pct(100.0))
              .justify(Justify::SpaceBetween)
              .align_items(Alignment::Center)
              .child(Text::new("Servers").variant(theme::TYP_TITLE))
              .child(Text::new("+ Add").variant(theme::TYP_LINK)),
          )
          .child(
            Column::new()
              .width(Dimension::Pct(100.0))
              .spacing(1.0)
              .child(server_row("My Server", "192.168.1.10:4433"))
              .child(Rect::new(Dimension::Pct(100.0), 1.0).background(BackgroundColor::Palette(theme::BORDER)))
              .child(server_row("Work", "vpn.corp.local:4433")),
          ),
      )
  }
}
