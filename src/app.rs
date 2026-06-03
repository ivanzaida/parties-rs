use lurq::{
  app::{component::Component, ctx::Ctx},
  components::Column,
  layout::{Alignment, layout_kind::Justify},
  node::{BackgroundColor, Element, dimension::Dimension},
};

use crate::{screens, theme};

pub struct App;

impl Component for App {
  type Props = ();

  fn create(ctx: &mut Ctx) -> Self {
    let _ = ctx;
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    Column::new()
      .width(Dimension::Pct(100.0))
      .height(Dimension::Pct(100.0))
      .background(BackgroundColor::Palette(theme::BG_PRIMARY))
      .align_items(Alignment::Center)
      .justify(Justify::Center)
      .clip()
      .child(ctx.mount::<screens::seed_phrase_display::SeedPhraseDisplay>(()))
  }
}
