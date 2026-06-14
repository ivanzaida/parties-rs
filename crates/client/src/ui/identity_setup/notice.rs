use lurq::{
  app::ctx::Ctx,
  components::{Row, Text},
  layout::Alignment,
  node::{BackgroundColor, dimension::Dimension},
};

use crate::{
  theme,
  ui::common::lucide_icon::{LucideIcon, LucideIconProps},
};

pub(super) fn notice(ctx: &mut Ctx, message: &str) -> Row {
  Row::new()
    .width(Dimension::Pct(100.0))
    .align_items(Alignment::Center)
    .spacing(10.0)
    .padding_vertical(14.0)
    .padding_horizontal(16.0)
    .rounded(6.0)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfacePanel))
    .border_inside(1.0, theme::PaletteColor::Border)
    .child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon: "info",
      size: 16.0,
      color: theme::palette().text_muted,
    }))
    .child(
      Text::new(message)
        .variant(theme::TypographyStyle::Description)
        .flex(1.0),
    )
}
