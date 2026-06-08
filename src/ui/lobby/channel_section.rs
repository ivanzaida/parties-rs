use std::sync::Arc;

use lurq::{
  app::ctx::Ctx,
  components::{Row, Text},
  core::Signal,
  layout::{Alignment, layout_kind::Justify},
  node::{CursorIcon, Element, color::Color, dimension::Dimension, transform::Transform2D},
};

use crate::{
  theme,
  ui::common::lucide_icon::{LucideIcon, LucideIconProps},
};

pub(super) fn section_head(
  ctx: &mut Ctx,
  expanded: Signal<bool>,
  label: &str,
  right_icon: Option<&'static str>,
  right_action: Option<Arc<dyn Fn() + Send + Sync + 'static>>,
) -> Element {
  let toggle = expanded.clone();
  let left_icon = if expanded.get() {
    "chevron-down"
  } else {
    "chevron-right"
  };
  let mut row = Row::new()
    .width(Dimension::Pct(100.0))
    .align_items(Alignment::Center)
    .justify(Justify::SpaceBetween)
    .padding_vertical(0.0)
    .padding_horizontal(8.0)
    .cursor(CursorIcon::Pointer)
    .child(
      Row::new()
        .align_items(Alignment::Center)
        .spacing(theme::SpacingSize::Xs)
        .child(ctx.mount::<LucideIcon>(LucideIconProps {
          icon: left_icon,
          size: 12.0,
          color: theme::palette().text_muted,
        }))
        .child(
          Text::new(label)
            .variant(theme::TypographyStyle::Caption)
            .color(theme::PaletteColor::TextMuted),
        ),
    );

  if let Some(icon) = right_icon {
    let mut action = Row::new()
      .width(20.0)
      .height(20.0)
      .align_items(Alignment::Center)
      .justify(Justify::Center)
      .child(ctx.mount::<LucideIcon>(LucideIconProps {
        icon,
        size: 14.0,
        color: theme::palette().text_muted,
      }));

    if let Some(right_action) = right_action {
      action = action.on_click(move |_| right_action());
    }

    row = row.child(action);
  }

  row.on_click(move |_| toggle.set(!toggle.get())).into()
}

pub(super) fn aligned_channel_icon(ctx: &mut Ctx, icon: &'static str, size: f32) -> Element {
  aligned_channel_icon_with_color(ctx, icon, size, theme::palette().text_muted)
}

pub(super) fn aligned_channel_icon_with_color(ctx: &mut Ctx, icon: &'static str, size: f32, color: Color) -> Element {
  Row::new()
    .width(size)
    .height(size)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .transform(Transform2D::translate(0.0, 1.5))
    .child(ctx.mount::<LucideIcon>(LucideIconProps { icon, size, color }))
    .into()
}
