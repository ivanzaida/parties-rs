use lurq::{
  animation::Transition,
  components::{Rect, Row},
  layout::Alignment,
  node::{BackgroundColor, CursorIcon, Element, color::Color, transform::Transform2D},
};

use crate::theme;

const TOGGLE_TRANSITION_MS: u64 = 240;

pub(super) fn settings_toggle(enabled: bool, on_toggle: impl Fn() + Send + Sync + 'static) -> Element {
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
    .transition(Transition::background_color().duration_ms(TOGGLE_TRANSITION_MS))
    .child(
      Rect::new(18.0, 18.0)
        .rounded(9.0)
        .background(BackgroundColor::Color(if enabled {
          palette.surface_base
        } else {
          palette.text_muted
        }))
        .transform(Transform2D::translate(knob_translate, 0.0))
        .transition(Transition::background_color().duration_ms(TOGGLE_TRANSITION_MS))
        .transition(Transition::transform().duration_ms(TOGGLE_TRANSITION_MS)),
    );

  if !enabled {
    track = track.border_inside(1.0, BackgroundColor::Color(Color::from_hex("#3A4047")));
  } else {
    track = track.border_inside(1.0, palette.surface_raised);
  }

  track.cursor(CursorIcon::Pointer).on_click(move |_| on_toggle()).into()
}
