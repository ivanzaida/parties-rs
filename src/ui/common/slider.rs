use lurq::{
  components::{Rect, Slider},
  core::Signal,
  node::{BackgroundColor, Element, SliderPartStyle, color::Color},
};

use crate::theme;

pub const SLIDER_HEIGHT: f32 = 18.0;
pub const TRACK_HEIGHT: f32 = 4.0;
pub const TRACK_RADIUS: f32 = 2.0;
pub const THUMB_SIZE: f32 = 16.0;
pub const THUMB_RADIUS: f32 = 8.0;
pub const THUMB_BORDER: f32 = 2.0;

pub fn track(width: f32) -> Element {
  Rect::new(width, TRACK_HEIGHT)
    .rounded(TRACK_RADIUS)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised))
    .into()
}

pub fn fill(width: f32) -> Element {
  Rect::new(width, TRACK_HEIGHT)
    .rounded(TRACK_RADIUS)
    .background(BackgroundColor::Palette(theme::PaletteColor::Accent))
    .into()
}

pub fn slider(value: Signal<i32>, width: f32, min: i32, max: i32) -> Slider {
  Slider::new(value)
    .range(min, max)
    .width(width)
    .height(SLIDER_HEIGHT)
    .track_style(invisible_track(width))
    .track_hovered_style(invisible_track(width))
    .thumb_style(thumb(theme::palette().accent))
    .thumb_hovered_style(thumb(theme::palette().accent_hover))
}

pub fn slider_f32(value: Signal<f32>, width: f32, min: f32, max: f32, step: f32) -> Slider {
  Slider::new_f32(value)
    .range_f32(min, max)
    .step(step)
    .width(width)
    .height(SLIDER_HEIGHT)
    .track_style(invisible_track(width))
    .track_hovered_style(invisible_track(width))
    .thumb_style(thumb(theme::palette().accent))
    .thumb_hovered_style(thumb(theme::palette().accent_hover))
}

fn invisible_track(width: f32) -> SliderPartStyle {
  SliderPartStyle::new()
    .width(width)
    .height(TRACK_HEIGHT)
    .rounded(TRACK_RADIUS)
    .background(Color::from_hex("#00000000"))
}

fn thumb(color: Color) -> SliderPartStyle {
  SliderPartStyle::new()
    .size(THUMB_SIZE, THUMB_SIZE)
    .rounded(THUMB_RADIUS)
    .background(color)
    .border_inside(THUMB_BORDER, BackgroundColor::Palette(theme::PaletteColor::SurfaceBase))
}
