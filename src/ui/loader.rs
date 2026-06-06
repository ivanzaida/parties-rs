use lurq::{
  animation::{AnimatableProperty, Animation, Keyframes, KeyframesId},
  components,
  layout::{Alignment, layout_kind::Justify},
  node::{Element, dimension::Dimension, transform::Decomposed},
};

const LOADER_ICON: &[u8] = include_bytes!("../../assets/icons/loading_circle.svg");
const LOADER_SPIN_KEYFRAMES: KeyframesId = KeyframesId::new(100);

pub fn register_keyframes(tree: &mut lurq::app::Tree) {
  tree.register_keyframes(
    Keyframes::new(LOADER_SPIN_KEYFRAMES)
      .frame(0.0, |frame| {
        frame.set(AnimatableProperty::Transform, Decomposed::IDENTITY.with_rotate(0.0));
      })
      .frame(1.0, |frame| {
        frame.set(
          AnimatableProperty::Transform,
          Decomposed::IDENTITY.with_rotate(std::f32::consts::TAU),
        );
      }),
  );
}

pub fn loader(size: impl Into<Dimension>) -> impl Into<Element> {
  let size = size.into();

  components::Row::new()
    .width(size)
    .height(size)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .child(components::Svg::from_bytes(LOADER_ICON).width(size).height(size))
    .animation(
      Animation::new(LOADER_SPIN_KEYFRAMES)
        .duration_ms(900)
        .linear()
        .infinite(),
    )
}
