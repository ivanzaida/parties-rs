use lurq::{
  app::{component::Component, ctx::Ctx},
  components::{Column, Text},
  layout::{Alignment, layout_kind::Justify},
  node::{dimension::Dimension, BackgroundColor, Element, color::Color},
};

use crate::theme;

pub struct IdentityRestore;

impl Component for IdentityRestore {
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
          .child(Text::new("Restore Identity").variant(theme::TYP_TITLE))
          .child(Text::new("Enter your 12-word seed phrase to restore your identity.").variant(theme::TYP_DESC))
          .child(
            Column::new()
              .width(400.0)
              .spacing(6.0)
              .child(Text::new("Seed phrase").variant(theme::TYP_FIELD_LABEL))
              .child(
                Column::new()
                  .width(Dimension::Pct(100.0))
                  .height(64.0)
                  .padding(10.0)
                  .padding_horizontal(12.0)
                  .background(BackgroundColor::Palette(theme::BG_SECONDARY))
                  .border_inside(1.0, Color::from_hex("#343A50"))
                  .rounded(4.0)
                  .child(Text::new("enter words\u{2026}").variant(theme::TYP_LINK)),
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
              .child(Text::new("Restore").variant(theme::TYP_BUTTON)),
          )
          .child(Text::new("\u{2190} back").variant(theme::TYP_LINK)),
      )
  }
}
