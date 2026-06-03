use lurq::{
  app::{component::Component, ctx::Ctx},
  components::Column,
  core::Signal,
  layout::{Alignment, layout_kind::Justify},
  node::{BackgroundColor, Element, dimension::Dimension},
};

use crate::{screens, theme};

const STEP_IDENTITY_SETUP: u8 = 0;
const STEP_SEED_PHRASE: u8 = 1;

pub struct App {
  step: Signal<u8>,
}

impl Component for App {
  type Props = ();

  fn create(ctx: &mut Ctx) -> Self {
    Self {
      step: ctx.signal(STEP_IDENTITY_SETUP),
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let step = self.step.get();
    ctx.provide(self.step.clone());

    let screen = if step == STEP_SEED_PHRASE {
      ctx.mount::<screens::seed_phrase_display::SeedPhraseDisplay>(())
    } else {
      ctx.mount::<screens::identity_setup::IdentitySetup>(())
    };

    Column::new()
      .width(Dimension::Pct(100.0))
      .height(Dimension::Pct(100.0))
      .background(BackgroundColor::Palette(theme::BG_PRIMARY))
      .align_items(Alignment::Center)
      .justify(Justify::Center)
      .clip()
      .child(screen)
  }
}
