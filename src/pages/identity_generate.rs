use lurq::{
  app::{component::Component, ctx::Ctx},
  components::{Column, Text},
  layout::{Alignment, layout_kind::Justify},
  node::{dimension::Dimension, BackgroundColor, Element},
};

use crate::theme;

fn option_card(title: &str, desc: &str, has_bg: bool) -> Column {
  let card = Column::new()
    .width(Dimension::Pct(100.0))
    .spacing(4.0)
    .padding(14.0)
    .padding_horizontal(16.0)
    .child(Text::new(title).variant(theme::TYP_BUTTON))
    .child(Text::new(desc).variant(theme::TYP_LINK));
  if has_bg {
    card.background(BackgroundColor::Palette(theme::BG_SECONDARY))
  } else {
    card
  }
}

pub struct IdentityGenerate;

impl Component for IdentityGenerate {
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
          .child(Text::new("Identity").variant(theme::TYP_TITLE))
          .child(Text::new("No identity found on this machine.").variant(theme::TYP_DESC))
          .child(
            Column::new()
              .width(400.0)
              .spacing(2.0)
              .child(option_card("Generate new identity", "Create a fresh Ed25519 keypair and seed phrase", true))
              .child(option_card("Restore from seed phrase", "Enter your 12-word mnemonic to recover", false))
              .child(option_card("Import private key", "Paste a 64-character hex key", false)),
          ),
      )
  }
}
