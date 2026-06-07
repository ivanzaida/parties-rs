use lurq::{
  animation::Transition,
  app::{component::Component, ctx::Ctx},
  components::{Rect, Row},
  core::Signal,
  layout::Alignment,
  node::{BackgroundColor, CursorIcon, Element, color::Color, transform::Transform2D},
};

use crate::theme;

#[derive(Clone, lurq::DevtoolsInspectable)]
pub(super) struct SettingsToggleProps {
  pub enabled: Signal<bool>,
}

impl PartialEq for SettingsToggleProps {
  fn eq(&self, _other: &Self) -> bool {
    true
  }
}

pub(super) struct SettingsToggle;

impl Component for SettingsToggle {
  type Props = SettingsToggleProps;

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let enabled_signal = ctx.props::<Self::Props>().enabled.clone();
    let enabled = enabled_signal.get();
    let palette = theme::palette();
    let knob_translate = if enabled { 18.0 } else { 0.0 };
    let mut track = Row::new()
      .width(40.0)
      .height(22.0)
      .align_items(Alignment::Center)
      .padding_left(2.0)
      .rounded(11.0)
      .background(BackgroundColor::Color(if enabled {
        palette.accent
      } else {
        palette.surface_raised
      }))
      .transition(Transition::background_color().duration_ms(160))
      .child(
        Rect::new(18.0, 18.0)
          .rounded(9.0)
          .background(BackgroundColor::Color(if enabled {
            palette.surface_base
          } else {
            palette.text_muted
          }))
          .transform(Transform2D::translate(knob_translate, 0.0))
          .transition(Transition::background_color().duration_ms(160))
          .transition(Transition::transform().duration_ms(160)),
      );

    if !enabled {
      track = track.border_inside(1.0, BackgroundColor::Color(Color::from_hex("#3A4047")));
    } else {
      track = track.border_inside(1.0, palette.surface_raised);
    }

    track
      .cursor(CursorIcon::Pointer)
      .on_click(move |_| enabled_signal.set(!enabled))
  }
}
