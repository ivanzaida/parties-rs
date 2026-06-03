use lurq::{
  app::{component::Component, ctx::Ctx},
  components::{Column, Row, Text, Rect, Spacer},
  layout::{Alignment, layout_kind::Justify},
  node::{dimension::Dimension, BackgroundColor, Element, color::Color},
};

use crate::theme;

fn channel_item(name: &str, active: bool, indent: bool) -> Row {
  let row = Row::new()
    .width(Dimension::Pct(100.0))
    .height(28.0)
    .align_items(Alignment::Center)
    .spacing(6.0)
    .padding_horizontal(14.0);
  let row = if indent { row.padding_left(24.0) } else { row };
  let row = if active { row.background(BackgroundColor::Palette(theme::BG_ELEVATED)).rounded(3.0) } else { row };
  row.child(Text::new(name).variant(if indent { theme::TYP_BODY } else { theme::TYP_CAPTION }))
}

fn chat_msg(author: &str, text: &str) -> Row {
  Row::new()
    .width(Dimension::Pct(100.0))
    .spacing(10.0)
    .child(
      Rect::new(28.0, 28.0)
        .background(BackgroundColor::Palette(theme::BG_ELEVATED))
        .rounded(14.0),
    )
    .child(
      Column::new()
        .flex(1.0)
        .spacing(4.0)
        .child(Text::new(author).variant(theme::TYP_BUTTON))
        .child(Text::new(text).variant(theme::TYP_BODY)),
    )
}

pub struct Lobby;

impl Component for Lobby {
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
              .justify(Justify::SpaceBetween)
              .padding_horizontal(14.0)
              .child(Text::new("My Server").variant(theme::TYP_BUTTON)),
          )
          .child(
            Column::new()
              .width(Dimension::Pct(100.0))
              .flex(1.0)
              .spacing(2.0)
              .padding_vertical(10.0)
              .child(channel_item("VOICE", false, false))
              .child(channel_item("general", true, false))
              .child(channel_item("anon", false, true))
              .child(channel_item("alice", false, true))
              .child(channel_item("Gaming", false, false))
              .child(channel_item("AFK", false, false)),
          )
          .child(
            Row::new()
              .width(Dimension::Pct(100.0))
              .height(44.0)
              .align_items(Alignment::Center)
              .padding_horizontal(10.0)
              .child(Text::new("anon").variant(theme::TYP_BODY)),
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
              .child(Text::new("# general").variant(theme::TYP_BUTTON))
              .child(Spacer::new()),
          )
          .child(
            Column::new()
              .width(Dimension::Pct(100.0))
              .flex(1.0)
              .justify(Justify::End)
              .spacing(12.0)
              .padding(16.0)
              .child(chat_msg("alice", "has anyone tested the new relay?"))
              .child(chat_msg("bob", "yeah works great, latency is under 30ms"))
              .child(chat_msg("anon", "nice, pushing the update now")),
          )
          .child(
            Row::new()
              .width(Dimension::Pct(100.0))
              .height(40.0)
              .align_items(Alignment::Center)
              .padding_horizontal(16.0)
              .spacing(8.0)
              .child(
                Row::new()
                  .flex(1.0)
                  .height(28.0)
                  .align_items(Alignment::Center)
                  .padding_horizontal(10.0)
                  .background(BackgroundColor::Palette(theme::BG_INPUT))
                  .border_inside(1.0, Color::from_hex("#343A50"))
                  .rounded(3.0)
                  .child(Text::new("Message #general").variant(theme::TYP_LINK)),
              ),
          ),
      )
  }
}
