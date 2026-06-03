use lurq::{
  app::{component::Component, ctx::Ctx},
  components::{Column, Row, Text},
  layout::{Alignment, layout_kind::Justify},
  node::{dimension::Dimension, BackgroundColor, Element, color::Color},
};

use crate::theme;

pub struct IdentityImport;

impl Component for IdentityImport {
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
          .child(Text::new("Import Private Key").variant(theme::TYP_TITLE))
          .child(Text::new("Enter your 64-character hex private key.").variant(theme::TYP_DESC))
          .child(
            Column::new()
              .width(400.0)
              .spacing(6.0)
              .child(Text::new("Private key (hex)").variant(theme::TYP_FIELD_LABEL))
              .child(
                Row::new()
                  .width(Dimension::Pct(100.0))
                  .height(38.0)
                  .align_items(Alignment::Center)
                  .padding_horizontal(12.0)
                  .background(BackgroundColor::Palette(theme::BG_SECONDARY))
                  .border_inside(1.0, Color::from_hex("#343A50"))
                  .rounded(4.0)
                  .child(Text::new("0x\u{2026}").variant(theme::TYP_LINK)),
              ),
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
              .child(Text::new("Import").variant(theme::TYP_BUTTON)),
          )
          .child(Text::new("\u{2190} back").variant(theme::TYP_LINK)),
      )
  }
}
