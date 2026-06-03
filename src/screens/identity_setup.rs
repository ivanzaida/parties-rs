use lurq::{
  app::{component::Component, ctx::Ctx},
  components::{Column, Rect, Row, Spacer, Text},
  layout::{Alignment, layout_kind::Justify},
  node::{BackgroundColor, Element, color::Color, dimension::Dimension},
};

use crate::{screens::icon, theme};

const BORDER: &str = "#30343A";

fn dot(color: impl Into<BackgroundColor>) -> Rect {
  Rect::new(8.0, 8.0).rounded(4.0).background(color)
}

fn option_row(title: &str, desc: &str, active: bool) -> Row {
  let bg = if active {
    BackgroundColor::Palette(theme::GREEN_MUTED)
  } else {
    BackgroundColor::Palette(theme::BG_ELEVATED)
  };
  let stroke = if active { "#42D28B" } else { BORDER };
  let state_color = if active { "#42D28B" } else { "#7D766C" };

  Row::new()
    .width(Dimension::Pct(100.0))
    .align_items(Alignment::Center)
    .spacing(12.0)
    .padding(12.0)
    .rounded(6.0)
    .background(bg)
    .border_inside(1.0, Color::from_hex(stroke))
    .child(dot(state_color))
    .child(
      Column::new()
        .flex(1.0)
        .spacing(3.0)
        .child(Text::new(title).variant(theme::TYP_BUTTON))
        .child(Text::new(desc).variant(theme::TYP_LINK)),
    )
    .child(icon("chevron-right", 14.0, state_color))
}

pub struct IdentitySetup;

impl Component for IdentitySetup {
  type Props = ();

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, _ctx: &mut Ctx) -> impl Into<Element> {
    Row::new()
      .width(960.0)
      .height(640.0)
      .align_items(Alignment::Center)
      .justify(Justify::Center)
      .spacing(40.0)
      .padding_horizontal(72.0)
      .background(BackgroundColor::Palette(theme::BG_PRIMARY))
      .child(
        Column::new()
          .width(280.0)
          .spacing(18.0)
          .child(Text::new("IDENTITY").variant(theme::TYP_CAPTION))
          .child(Text::new("Create identity").variant(theme::TYP_TITLE))
          .child(
            Text::new("Parties uses a local cryptographic identity for names, server trust, and peer verification.")
              .variant(theme::TYP_DESC),
          )
          .child(
            Column::new()
              .width(Dimension::Pct(100.0))
              .spacing(8.0)
              .padding(12.0)
              .rounded(6.0)
              .background(BackgroundColor::Palette(theme::BG_TERTIARY))
              .border_inside(1.0, Color::from_hex(BORDER))
              .child(dot("#F2B84B"))
              .child(Text::new("No local identity found").variant(theme::TYP_BUTTON))
              .child(Text::new("Create one now or restore an existing key from backup.").variant(theme::TYP_LINK)),
          ),
      )
      .child(
        Column::new()
          .width(440.0)
          .spacing(14.0)
          .padding(18.0)
          .rounded(8.0)
          .background(BackgroundColor::Palette(theme::BG_TERTIARY))
          .border_inside(1.0, Color::from_hex(BORDER))
          .child(Text::new("Choose setup method").variant(theme::TYP_HEADING))
          .child(option_row(
            "Generate new identity",
            "Creates a seed phrase and a new peer fingerprint.",
            true,
          ))
          .child(option_row(
            "Restore seed phrase",
            "Use a saved 12-word backup from another install.",
            false,
          ))
          .child(option_row(
            "Import private key",
            "Paste a raw 64-character private key.",
            false,
          ))
          .child(
            Row::new()
              .width(Dimension::Pct(100.0))
              .align_items(Alignment::Center)
              .spacing(8.0)
              .padding(10.0)
              .rounded(5.0)
              .background("#111A14")
              .border_inside(1.0, Color::from_hex("#2D4634"))
              .child(icon("shield-check", 14.0, "#42D28B"))
              .child(Text::new("Seed material stays on this device.").variant(theme::TYP_LINK))
              .child(Spacer::new()),
          ),
      )
  }
}
