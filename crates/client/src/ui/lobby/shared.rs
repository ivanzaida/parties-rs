use lurq::{
  app::ctx::Ctx,
  components::{Row, Text},
  layout::Alignment,
  node::{BackgroundColor, Element, dimension::Dimension},
};

use super::layout::lobby_layout_metrics;
use crate::{
  network::protocol::UserId,
  theme,
  ui::common::lucide_icon::{LucideIcon, LucideIconProps},
};

pub(super) fn user_display_name(user_id: UserId, name: &str, debug_user_ids: bool) -> String {
  if debug_user_ids {
    format!("[id:{user_id}] {name}")
  } else {
    name.to_owned()
  }
}

pub(super) fn error_notice(ctx: &mut Ctx, message: &str) -> Element {
  let metrics = lobby_layout_metrics(ctx);

  Row::new()
    .width(Dimension::Pct(100.0))
    .max_width(metrics.copy_max_width)
    .align_items(Alignment::Center)
    .spacing(theme::SpacingSize::Sm)
    .padding(theme::SpacingSize::Md)
    .rounded(theme::RadiusSize::Md)
    .background(BackgroundColor::Palette(theme::PaletteColor::DangerMuted))
    .border_inside(1.0, theme::PaletteColor::Danger)
    .child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon: "triangle-alert",
      size: 14.0,
      color: theme::palette().danger,
    }))
    .child(
      Text::new(message)
        .variant(theme::TypographyStyle::Description)
        .color(theme::PaletteColor::Danger)
        .width(Dimension::Pct(100.0)),
    )
    .into()
}

#[cfg(test)]
#[path = "../../../tests/unit/ui/lobby/shared.rs"]
mod tests;
