use lurq::{
  components::Slider,
  core::Signal,
  node::{BackgroundColor, SliderPartStyle, color::Color},
};

use crate::theme;

pub const SLIDER_HEIGHT: f32 = 18.0;
pub const TRACK_HEIGHT: f32 = 4.0;
pub const TRACK_RADIUS: f32 = 2.0;
pub const THUMB_SIZE: f32 = 16.0;
pub const THUMB_RADIUS: f32 = 8.0;
pub const THUMB_BORDER: f32 = 2.0;

pub fn slider(value: Signal<i32>, width: f32, min: i32, max: i32) -> Slider {
  let palette = theme::palette();
  Slider::new(value)
    .range(min, max)
    .width(width)
    .height(SLIDER_HEIGHT)
    .track_style(track(width, palette.surface_raised))
    .track_hovered_style(track(width, palette.surface_input))
    .fill_style(fill(palette.accent))
    .fill_hovered_style(fill(palette.accent_hover))
    .thumb_style(thumb(palette.accent))
    .thumb_hovered_style(thumb(palette.accent_hover))
}

pub fn slider_f32(value: Signal<f32>, width: f32, min: f32, max: f32, step: f32) -> Slider {
  let palette = theme::palette();
  Slider::new_f32(value)
    .range_f32(min, max)
    .step(step)
    .width(width)
    .height(SLIDER_HEIGHT)
    .track_style(track(width, palette.surface_raised))
    .track_hovered_style(track(width, palette.surface_input))
    .fill_style(fill(palette.accent))
    .fill_hovered_style(fill(palette.accent_hover))
    .thumb_style(thumb(palette.accent))
    .thumb_hovered_style(thumb(palette.accent_hover))
}

fn track(width: f32, color: Color) -> SliderPartStyle {
  SliderPartStyle::new()
    .width(width)
    .height(TRACK_HEIGHT)
    .rounded(TRACK_RADIUS)
    .background(color)
}

fn fill(color: Color) -> SliderPartStyle {
  SliderPartStyle::new()
    .height(TRACK_HEIGHT)
    .rounded(TRACK_RADIUS)
    .background(color)
}

fn thumb(color: Color) -> SliderPartStyle {
  SliderPartStyle::new()
    .size(THUMB_SIZE, THUMB_SIZE)
    .rounded(THUMB_RADIUS)
    .background(color)
    .border_inside(THUMB_BORDER, BackgroundColor::Palette(theme::PaletteColor::SurfaceBase))
}
