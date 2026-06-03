use lurq::{
  app::{component::Component, ctx::Ctx},
  components::{Column, Row, Text},
  layout::Alignment,
  node::{dimension::Dimension, Element},
};

use crate::{pages::settings::settings_nav, theme};

pub struct SettingsAppearance;

impl Component for SettingsAppearance {
  type Props = ();
  fn create(_ctx: &mut Ctx) -> Self { Self }

  fn render(&self, _ctx: &mut Ctx) -> impl Into<Element> {
    Row::new()
      .width(Dimension::Pct(100.0))
      .flex(1.0)
      .align_items(Alignment::Stretch)
      .child(settings_nav("Appearance"))
      .child(
        Column::new()
          .flex(1.0)
          .padding(32.0)
          .padding_horizontal(48.0)
          .spacing(24.0)
          .child(Text::new("APPEARANCE").variant(theme::TYP_SECTION))
          .child(
            Column::new()
              .width(400.0)
              .spacing(16.0)
              .child(Text::new("Theme and display preferences").variant(theme::TYP_DESC)),
          ),
      )
  }
}
