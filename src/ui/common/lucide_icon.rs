use lurq::{
  app::{component::Component, ctx::Ctx},
  components::Text,
  layout::text_style::TextStyle,
  node::{Element, color::Color},
};

#[derive(Clone, PartialEq, lurq::DevtoolsInspectable)]
pub struct LucideIconProps {
  pub icon: &'static str,
  pub size: f32,
  pub color: Color,
}

pub struct LucideIcon;

impl Component for LucideIcon {
  type Props = LucideIconProps;

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let props = ctx.props::<Self::Props>();
    let glyph = String::from(name_to_char(props.icon));

    Text::styled(
      &glyph,
      TextStyle {
        font_family: "lucide".into(),
        font_size: props.size,
        color: props.color,
        ..TextStyle::default()
      },
    )
  }
}

fn name_to_char(name: &str) -> char {
  match name {
    "arrow-left" => '\u{e048}',
    "arrow-right" => '\u{e04a}',
    "alert-circle" | "circle-alert" => '\u{e077}',
    "check" => '\u{e06c}',
    "chevron-down" => '\u{e06d}',
    "chevron-right" => '\u{e06f}',
    "copy" => '\u{e09e}',
    "database" => '\u{e0ad}',
    "eye-off" => '\u{e0d0}',
    "gamepad-2" => '\u{e0df}',
    "headphones" => '\u{e0f1}',
    "info" => '\u{e0f9}',
    "key" => '\u{e0fd}',
    "list-tree" => '\u{e408}',
    "loader" => '\u{e109}',
    "lock" => '\u{e10b}',
    "mic" => '\u{e118}',
    "mic-off" => '\u{e119}',
    "monitor" => '\u{e11d}',
    "moon" => '\u{e11e}',
    "plus" => '\u{e13d}',
    "refresh-cw" => '\u{e145}',
    "rotate-cw" => '\u{e149}',
    "settings" => '\u{e154}',
    "shield" => '\u{e1fe}',
    "shield-check" => '\u{e1ff}',
    "sprout" => '\u{e1eb}',
    "trash-2" => '\u{e18e}',
    "video" => '\u{e1a5}',
    "volume-2" => '\u{e1ab}',
    "alert-triangle" | "triangle-alert" => '\u{e193}',
    _ => '\u{e06f}',
  }
}
