use lurq::{
  app::{component::Component, ctx::Ctx},
  components::{Column, Row, Text, Rect},
  layout::Alignment,
  node::{dimension::Dimension, BackgroundColor, Element},
};

use crate::{pages::settings::settings_nav, theme};

pub struct SettingsIdentity;

impl Component for SettingsIdentity {
  type Props = ();
  fn create(_ctx: &mut Ctx) -> Self { Self }

  fn render(&self, _ctx: &mut Ctx) -> impl Into<Element> {
    Row::new()
      .width(Dimension::Pct(100.0))
      .flex(1.0)
      .align_items(Alignment::Stretch)
      .child(settings_nav("Identity"))
      .child(
        Column::new()
          .flex(1.0)
          .padding(32.0)
          .padding_horizontal(48.0)
          .spacing(24.0)
          .child(Text::new("IDENTITY").variant(theme::TYP_SECTION))
          .child(
            Column::new()
              .width(400.0)
              .spacing(16.0)
              .child(
                Row::new()
                  .width(Dimension::Pct(100.0))
                  .spacing(8.0)
                  .child(Text::new("Fingerprint").variant(theme::TYP_FIELD_LABEL))
                  .child(Text::new("a3:f1:7b:02:d4:e8:91:cc:\u{2026}").variant(theme::TYP_MONO)),
              )
              .child(
                Row::new()
                  .width(Dimension::Pct(100.0))
                  .spacing(8.0)
                  .child(Text::new("Public key").variant(theme::TYP_FIELD_LABEL))
                  .child(Text::new("ed25519:abc123\u{2026}").variant(theme::TYP_MONO)),
              ),
          )
          .child(Rect::new(Dimension::Pct(100.0), 1.0).background(BackgroundColor::Palette(theme::BORDER)))
          .child(Text::new("Replace identity\u{2026}").variant(theme::TYP_LINK)),
      )
  }
}
