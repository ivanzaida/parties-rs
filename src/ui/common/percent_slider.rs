use std::sync::Arc;

use lurq::{
  app::{
    component::{Component, ComponentInfo, DevtoolsInspectable},
    ctx::Ctx,
  },
  components::{Row, Stack, Text},
  core::Signal,
  layout::{Alignment, StackAlignment},
  node::{dimension::Dimension, Element},
};

use crate::{theme, ui::common::slider as app_slider};

pub type PercentSliderSaveAction = Arc<dyn Fn(i32) + Send + Sync>;

#[derive(Clone)]
pub struct PercentSliderProps {
  pub initial_value: i32,
  pub control_width: f32,
  pub track_width: f32,
  pub value_width: f32,
  pub value_spacing: f32,
  pub on_blur: PercentSliderSaveAction,
}

impl PartialEq for PercentSliderProps {
  fn eq(&self, other: &Self) -> bool {
    self.initial_value == other.initial_value
      && self.control_width == other.control_width
      && self.track_width == other.track_width
      && self.value_width == other.value_width
      && self.value_spacing == other.value_spacing
      && Arc::ptr_eq(&self.on_blur, &other.on_blur)
  }
}

impl DevtoolsInspectable for PercentSliderProps {
  fn write_info(&self, buffer: &mut Vec<ComponentInfo>) {
    buffer.push(ComponentInfo::with_value(
      "initial_value",
      std::any::type_name::<i32>(),
      self.initial_value.to_string(),
    ));
  }
}

pub struct PercentSlider {
  initial_value: Signal<i32>,
  value: Signal<i32>,
}

impl Component for PercentSlider {
  type Props = PercentSliderProps;

  fn create(ctx: &mut Ctx) -> Self {
    let initial_value = ctx.props::<Self::Props>().initial_value.clamp(0, 100);
    Self {
      initial_value: ctx.signal(initial_value),
      value: ctx.signal(initial_value),
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let props = ctx.props::<Self::Props>().clone();
    let initial_value = props.initial_value.clamp(0, 100);

    if self.initial_value.get_untracked() != initial_value {
      self.initial_value.set(initial_value);
      self.value.set(initial_value);
    }

    percent_slider_control(
      self.value.clone(),
      props.control_width,
      props.track_width,
      props.value_width,
      props.value_spacing,
      props.on_blur,
    )
  }
}

fn percent_slider_control(
  value: Signal<i32>,
  control_width: f32,
  track_width: f32,
  value_width: f32,
  value_spacing: f32,
  on_blur: PercentSliderSaveAction,
) -> Element {
  let current = value.get().clamp(0, 100);
  let fill_width = track_width * current as f32 / 100.0;
  let value_label = format!("{current}%");

  let mut slider = app_slider::slider(value.clone(), track_width, 0, 100);

  slider = slider.on_blur(move || {
    on_blur(value.get_untracked());
  });

  Row::new()
    .width(control_width)
    .align_items(Alignment::Center)
    .spacing(value_spacing)
    .child(
      Stack::new()
        .stack_align(StackAlignment::CenterStart)
        .width(track_width)
        .height(app_slider::SLIDER_HEIGHT)
        .child(app_slider::track(track_width))
        .child(app_slider::fill(fill_width))
        .child(slider),
    )
    .child(
      Text::new(&value_label)
        .variant(theme::TypographyStyle::Mono)
        .color(theme::PaletteColor::TextPrimary)
        .width(Dimension::Px(value_width)),
    )
    .into()
}
