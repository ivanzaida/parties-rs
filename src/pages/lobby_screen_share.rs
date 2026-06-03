use lurq::{
  app::{component::Component, ctx::Ctx},
  components::{Column, Row, Text, Rect, Spacer},
  layout::{Alignment, layout_kind::Justify},
  node::{dimension::Dimension, BackgroundColor, Element, color::Color},
};

use crate::theme;

pub struct LobbyScreenShare;

impl Component for LobbyScreenShare {
  type Props = ();
  fn create(_ctx: &mut Ctx) -> Self { Self }

  fn render(&self, _ctx: &mut Ctx) -> impl Into<Element> {
    Row::new()
      .width(Dimension::Pct(100.0))
      .flex(1.0)
      .align_items(Alignment::Stretch)
      .child(
        Column::new()
          .width(220.0)
          .flex(1.0)
          .background(BackgroundColor::Palette(theme::BG_SECONDARY))
          .border_inside(1.0, Color::from_hex("#343A50"))
          .child(
            Row::new()
              .width(Dimension::Pct(100.0))
              .height(44.0)
              .align_items(Alignment::Center)
              .padding_horizontal(14.0)
              .child(Text::new("My Server").variant(theme::TYP_BUTTON)),
          )
          .child(
            Column::new()
              .width(Dimension::Pct(100.0))
              .flex(1.0)
              .spacing(2.0)
              .padding_vertical(10.0)
              .child(Text::new("VOICE").variant(theme::TYP_CAPTION).padding_horizontal(14.0))
              .child(
                Row::new()
                  .width(Dimension::Pct(100.0))
                  .height(28.0)
                  .align_items(Alignment::Center)
                  .padding_horizontal(14.0)
                  .background(BackgroundColor::Palette(theme::BG_ELEVATED))
                  .rounded(3.0)
                  .child(Text::new("General").variant(theme::TYP_BODY)),
              ),
          ),
      )
      .child(
        Column::new()
          .flex(1.0)
          .flex(1.0)
          .child(
            Row::new()
              .width(Dimension::Pct(100.0))
              .height(44.0)
              .align_items(Alignment::Center)
              .padding_horizontal(16.0)
              .spacing(8.0)
              .child(Text::new("bob\u{2019}s screen").variant(theme::TYP_BUTTON))
              .child(Spacer::new())
              .child(Text::new("AV1 \u{b7} 1920\u{d7}1080").variant(theme::TYP_MONO)),
          )
          .child(
            Column::new()
              .width(Dimension::Pct(100.0))
              .flex(1.0)
              .align_items(Alignment::Center)
              .justify(Justify::Center)
              .background("#000000")
              .child(
                Rect::new(800.0, 450.0)
                  .background(BackgroundColor::Palette(theme::BG_TERTIARY))
                  .rounded(4.0),
              ),
          )
          .child(
            Row::new()
              .width(Dimension::Pct(100.0))
              .height(40.0)
              .align_items(Alignment::Center)
              .padding_horizontal(16.0)
              .spacing(12.0)
              .child(Text::new("Show chat").variant(theme::TYP_LINK))
              .child(Spacer::new())
              .child(Text::new("28ms").variant(theme::TYP_MONO))
              .child(Text::new("2.1 Mbps").variant(theme::TYP_MONO)),
          ),
      )
  }
}
