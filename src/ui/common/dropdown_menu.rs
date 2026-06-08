use std::sync::Arc;

use lurq::{
  components::{Row, Select, Text, TextOverflow},
  core::Signal,
  layout::{layout_kind::Justify, text_style::TextStyle, Alignment},
  node::{color::Color, dimension::Dimension, padding::Padding, Element, SelectPartStyle, SelectStyle},
};

use crate::theme;

pub struct DropdownOption {
  pub value: String,
  pub label: String,
}

pub fn dropdown_menu(value: Signal<String>, options: Vec<DropdownOption>, placeholder: &str, width: f32) -> Element {
  Select::new(value)
    .options(options.into_iter().map(|option| (option.value, option.label)))
    .placeholder(placeholder)
    .width(width)
    .height(36.0)
    .style(dropdown_style())
    .trigger(move |state| dropdown_trigger(state.label.or(state.placeholder)))
    .into()
}

fn dropdown_trigger(label: Option<Arc<str>>) -> Element {
  let label = label.unwrap_or_default();

  Row::new()
    .width(Dimension::Pct(100.0))
    .align_items(Alignment::Center)
    .justify(Justify::SpaceBetween)
    .spacing(theme::SpacingSize::Sm)
    .child(
      Text::styled(label.as_ref(), text_style(theme::palette().text_muted))
        .nowrap()
        .text_overflow(TextOverflow::Elipsis)
        .min_width(0.0)
        .flex(1.0),
    )
    .child(
      Row::new()
        .width(15.0)
        .height(15.0)
        .align_items(Alignment::Center)
        .justify(Justify::Center)
        .child(Text::styled(
          "\u{e06d}",
          TextStyle {
            font_family: Arc::from("lucide"),
            font_size: 15.0,
            line_height: 1.0,
            color: theme::palette().text_muted,
            ..TextStyle::default()
          },
        )),
    )
    .into()
}

fn dropdown_style() -> SelectStyle {
  SelectStyle::new()
    .trigger(
      SelectPartStyle::new()
        .background(theme::PaletteColor::SurfaceInput)
        .border_inside(1.0, theme::PaletteColor::Border)
        .rounded(theme::RadiusSize::Md)
        .padding(Padding::symmetric(14.0, 7.0))
        .text(text_style(theme::palette().text_muted))
        .min_height(36.0),
    )
    .trigger_hovered(SelectPartStyle::new().background(theme::PaletteColor::SurfaceRaised))
    .trigger_focused(SelectPartStyle::new().border_inside(1.0, theme::PaletteColor::BorderFocus))
    .trigger_open(
      SelectPartStyle::new()
        .rounded(theme::RadiusSize::Md)
        .border_inside(1.0, theme::PaletteColor::Border),
    )
    .placeholder_text(text_style(theme::palette().text_muted))
    .menu(
      SelectPartStyle::new()
        .background(theme::PaletteColor::SurfaceRaised)
        .border_inside(1.0, Color::from_hex("#3A4047"))
        .rounded(10.0),
    )
    .option(
      SelectPartStyle::new()
        .padding(Padding::symmetric(9.0, 10.0))
        .text(text_style(theme::palette().text_secondary))
        .min_height(36.0),
    )
    .option_hovered(SelectPartStyle::new().background(Color::from_hex("#232830")))
    .option_selected(
      SelectPartStyle::new()
        .background(Color::from_hex("#232830"))
        .text(text_style(theme::palette().text_primary)),
    )
    .option_selected_hovered(SelectPartStyle::new().background(Color::from_hex("#232830")))
    .chevron_color(theme::palette().text_muted)
    .chevron_size(12.0)
    .checkmark_color(theme::palette().warning)
    .menu_gap(6.0)
    .max_menu_height(260.0)
}

fn text_style(color: Color) -> lurq::layout::text_style::TextStyle {
  lurq::layout::text_style::TextStyle {
    font_family: Arc::from("Inter"),
    font_size: 13.0,
    line_height: 1.2,
    color,
    ..lurq::layout::text_style::TextStyle::default()
  }
}
