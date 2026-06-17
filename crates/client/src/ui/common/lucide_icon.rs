use lurq::{
  app::{component::Component, ctx::Ctx},
  components::{Row, Text},
  layout::{Alignment, layout_kind::Justify, text_style::TextStyle},
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

    Row::new()
      .width(props.size)
      .height(props.size)
      .align_items(Alignment::Center)
      .justify(Justify::Center)
      .child(lucide_icon_text(props.icon, props.size, props.color))
  }
}

pub(crate) fn lucide_icon_text(icon: &'static str, size: f32, color: Color) -> Text {
  let glyph = String::from(name_to_char(icon));
  Text::styled(
    &glyph,
    TextStyle {
      font_family: "lucide".into(),
      font_size: size,
      line_height: 1.0,
      color,
      ..TextStyle::default()
    },
  )
}

fn name_to_char(name: &str) -> char {
  match name {
    "arrow-left" => '\u{e048}',
    "arrow-right" => '\u{e049}',
    "alert-circle" | "circle-alert" => '\u{e077}',
    "activity" => '\u{e038}',
    "app-window" => '\u{e426}',
    "camera" => '\u{e064}',
    "check" => '\u{e06c}',
    "check-circle" => '\u{e07c}',
    "chevron-down" => '\u{e06d}',
    "chevron-right" => '\u{e06f}',
    "circle" => '\u{e076}',
    "corner-down-right" => '\u{e0a2}',
    "copy" => '\u{e09e}',
    "database" => '\u{e0ad}',
    "ellipsis" => '\u{e0b6}',
    "eye" => '\u{e0ba}',
    "eye-off" => '\u{e0bb}',
    "gamepad-2" => '\u{e0df}',
    "globe" => '\u{e0e8}',
    "hash" => '\u{e0ef}',
    "headphones" => '\u{e0f1}',
    "headphone-off" => '\u{e629}',
    "info" => '\u{e0f9}',
    "key" => '\u{e0fd}',
    "key-round" => '\u{e4a3}',
    "layout-grid" => '\u{e0ff}',
    "list-tree" => '\u{e408}',
    "loader" => '\u{e109}',
    "lock" => '\u{e10b}',
    "log-out" => '\u{e10e}',
    "mic" => '\u{e118}',
    "mic-off" => '\u{e119}',
    "megaphone" => '\u{e580}',
    "maximize" => '\u{e112}',
    "minimize-2" => '\u{e11b}',
    "minus" => '\u{e11c}',
    "monitor" => '\u{e11d}',
    "monitor-play" => '\u{e485}',
    "monitor-up" => '\u{e422}',
    "moon" => '\u{e11e}',
    "palette" => '\u{e1dd}',
    "phone-off" => '\u{e138}',
    "pin" => '\u{e259}',
    "play" => '\u{e13c}',
    "plug" => '\u{e37f}',
    "plus" => '\u{e13d}',
    "power" => '\u{e140}',
    "radar" => '\u{e497}',
    "radio" => '\u{e142}',
    "refresh-cw" => '\u{e145}',
    "rotate-cw" => '\u{e149}',
    "screen-share" => '\u{e14f}',
    "screen-share-off" => '\u{e150}',
    "search" => '\u{e151}',
    "send" => '\u{e152}',
    "send-horizontal" => '\u{e4f2}',
    "settings" => '\u{e154}',
    "server" => '\u{e153}',
    "shield" => '\u{e158}',
    "shield-alert" => '\u{e1fe}',
    "shield-check" => '\u{e1ff}',
    "sliders-horizontal" => '\u{e29a}',
    "sprout" => '\u{e1eb}',
    "terminal" => '\u{e181}',
    "trash-2" => '\u{e18e}',
    "unlock" => '\u{e4a3}',
    "user" => '\u{e19f}',
    "user-x" => '\u{e1a3}',
    "users" => '\u{e1a4}',
    "unplug" => '\u{e45d}',
    "video" => '\u{e1a5}',
    "volume-2" => '\u{e1ab}',
    "volume-x" => '\u{e1ae}',
    "wifi-off" => '\u{e1af}',
    "x" => '\u{e1b2}',
    "audio-lines" => '\u{e55a}',
    "alert-triangle" | "triangle-alert" => '\u{e193}',
    "bug" => '\u{e20c}',
    _ => {
      crate::log_once!(warn, target: "ui::icons", "Unknown icon name: {name}");
      '\u{e06f}'
    }
  }
}
