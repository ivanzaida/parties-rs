use lurq::{
  app::{component::Component, ctx::Ctx},
  components::Column,
  layout::Alignment,
  node::{BackgroundColor, Element, dimension::Dimension},
};

use crate::theme;

pub struct App;

impl Component for App {
  type Props = ();

  fn create(ctx: &mut Ctx) -> Self {
    let _ = ctx;
    Self
  }

  fn render(&self, _ctx: &mut Ctx) -> impl Into<Element> {
    Column::new()
      .width(Dimension::Pct(100.0))
      .height(Dimension::Pct(100.0))
      .background(BackgroundColor::Palette(theme::BG_PRIMARY))
      .align_items(Alignment::Stretch)
      .clip()
  }
}
