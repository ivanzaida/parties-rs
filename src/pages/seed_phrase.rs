use lurq::{
  app::{component::Component, ctx::Ctx},
  components::{Column, Row, Text},
  layout::{Alignment, layout_kind::Justify},
  node::{dimension::Dimension, BackgroundColor, Element, color::Color},
};

use crate::theme;

fn word_row(n1: u32, w1: &str, n2: u32, w2: &str, n3: u32, w3: &str) -> Row {
  let word = |n: u32, w: &str| -> Row {
    Row::new()
      .spacing(6.0)
      .flex(1.0)
      .child(Text::new(&format!("{n}.")).variant(theme::TYP_LINK))
      .child(Text::new(w).variant(theme::TYP_BODY))
  };
  Row::new()
    .width(Dimension::Pct(100.0))
    .spacing(8.0)
    .child(word(n1, w1))
    .child(word(n2, w2))
    .child(word(n3, w3))
}

fn btn(label: &str) -> Column {
  Column::new()
    .width(440.0)
    .height(38.0)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .background(BackgroundColor::Palette(theme::BG_SECONDARY))
    .border_inside(1.0, Color::from_hex("#343A50"))
    .rounded(4.0)
    .child(Text::new(label).variant(theme::TYP_BUTTON))
}

pub struct SeedPhrase;

impl Component for SeedPhrase {
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
          .child(Text::new("Your Identity").variant(theme::TYP_TITLE))
          .child(Text::new("This seed phrase is your permanent identity. If you lose it, you cannot recover your account.").variant(theme::TYP_DESC))
          .child(
            Column::new()
              .width(440.0)
              .spacing(12.0)
              .padding(16.0)
              .padding_horizontal(20.0)
              .background(BackgroundColor::Palette(theme::BG_SECONDARY))
              .child(
                Column::new()
                  .width(Dimension::Pct(100.0))
                  .spacing(6.0)
                  .child(word_row(1, "apple", 2, "brave", 3, "coral"))
                  .child(word_row(4, "delta", 5, "eagle", 6, "frost"))
                  .child(word_row(7, "globe", 8, "haven", 9, "ivory"))
                  .child(word_row(10, "jewel", 11, "knack", 12, "lemon")),
              )
              .child(Text::new("copy to clipboard").variant(theme::TYP_LINK)),
          )
          .child(
            Row::new()
              .spacing(8.0)
              .child(Text::new("Fingerprint").variant(theme::TYP_FIELD_LABEL))
              .child(Text::new("a3:f1:7b:02:d4:e8:91:cc:\u{2026}").variant(theme::TYP_MONO)),
          )
          .child(btn("I saved it \u{2014} continue"))
          .child(Text::new("\u{2190} back").variant(theme::TYP_LINK)),
      )
  }
}
