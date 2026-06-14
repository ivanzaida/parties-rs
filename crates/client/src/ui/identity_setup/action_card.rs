use std::sync::Arc;

use lurq::{
  app::{component::Component, ctx::Ctx, theme::Breakpoint},
  components::{Column, Row, Text},
  layout::{Alignment, layout_kind::Justify},
  node::{BackgroundColor, CursorIcon, Element, Style, dimension::Dimension},
};

use crate::{
  theme,
  ui::common::lucide_icon::{LucideIcon, LucideIconProps},
};

#[derive(Clone, Copy)]
struct ActionCardLayoutMetrics {
  icon_size: f32,
  row_spacing: f32,
  row_padding_x: f32,
  row_padding_y: f32,
}

fn action_card_layout_metrics(ctx: &Ctx) -> ActionCardLayoutMetrics {
  match ctx.breakpoint() {
    Some(Breakpoint::Md) => ActionCardLayoutMetrics {
      icon_size: 38.0,
      row_spacing: 12.0,
      row_padding_x: 16.0,
      row_padding_y: 14.0,
    },
    Some(Breakpoint::Lg) => ActionCardLayoutMetrics {
      icon_size: 40.0,
      row_spacing: 14.0,
      row_padding_x: 18.0,
      row_padding_y: 14.0,
    },
    Some(Breakpoint::Xl) | Some(Breakpoint::Sm) | None => ActionCardLayoutMetrics {
      icon_size: 42.0,
      row_spacing: 14.0,
      row_padding_x: 20.0,
      row_padding_y: 14.0,
    },
  }
}

#[derive(Clone, PartialEq, lurq::DevtoolsInspectable)]
pub(super) struct IdentityActionCardProps {
  pub icon: &'static str,
  pub title: Arc<str>,
  pub description: Arc<str>,
  pub target_route: Option<&'static str>,
}

pub(super) struct IdentityActionCard;

impl Component for IdentityActionCard {
  type Props = IdentityActionCardProps;

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let props = ctx.props::<Self::Props>().clone();
    let navigator = ctx.navigator();
    let icon_color = theme::palette().text_secondary;
    let metrics = action_card_layout_metrics(ctx);

    let mut row = Row::new()
      .width(Dimension::Pct(100.0))
      .align_items(Alignment::Center)
      .spacing(metrics.row_spacing)
      .padding_vertical(metrics.row_padding_y)
      .padding_horizontal(metrics.row_padding_x)
      .rounded(theme::RadiusSize::Lg)
      .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised))
      .border_inside(1.0, theme::PaletteColor::Border)
      .cursor(CursorIcon::Pointer)
      .hovered_style(action_state_style())
      .active_style(action_state_style())
      .child(
        Row::new()
          .width(metrics.icon_size)
          .height(metrics.icon_size)
          .align_items(Alignment::Center)
          .justify(Justify::Center)
          .rounded(theme::RadiusSize::Lg)
          .background(BackgroundColor::Palette(theme::PaletteColor::SurfacePanel))
          .border_inside(1.0, theme::PaletteColor::Border)
          .child(ctx.mount::<LucideIcon>(LucideIconProps {
            icon: props.icon,
            size: 20.0,
            color: icon_color,
          })),
      )
      .child(
        Column::new()
          .flex(1.0)
          .spacing(theme::SpacingSize::Xs)
          .child(Text::new(&props.title).variant(theme::TypographyStyle::Heading))
          .child(Text::new(&props.description).variant(theme::TypographyStyle::Link)),
      )
      .child(ctx.mount::<LucideIcon>(LucideIconProps {
        icon: "chevron-right",
        size: 18.0,
        color: icon_color,
      }));

    if let (Some(navigator), Some(target_route)) = (navigator, props.target_route) {
      row = row.on_click(move |_| navigator.push(target_route));
    }

    row
  }
}

fn action_state_style() -> Style {
  Style::new()
    .background(BackgroundColor::Palette(theme::PaletteColor::AccentMuted))
    .border_inside(1.0, theme::PaletteColor::Accent)
}
