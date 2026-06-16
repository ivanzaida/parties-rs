use lurq::{
  app::{ctx::Ctx, events::MouseEvent},
  components::Button,
  layout::{Alignment, layout_kind::Justify},
  node::{BackgroundColor, CursorIcon, Element, Style},
};

use crate::{
  theme,
  ui::common::lucide_icon::{LucideIcon, LucideIconProps},
};

pub(super) const REFRESH_BUTTON_SIZE: f32 = 36.0;
pub(super) const REFRESH_BUTTON_SPACING: f32 = 8.0;

pub(super) fn refresh_button(ctx: &mut Ctx, on_click: impl Fn(&MouseEvent) + Send + Sync + 'static) -> Element {
  Button::empty()
    .button()
    .width(REFRESH_BUTTON_SIZE)
    .height(REFRESH_BUTTON_SIZE)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .rounded(theme::RadiusSize::Md)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceInput))
    .border_inside(1.0, theme::PaletteColor::Border)
    .cursor(CursorIcon::Pointer)
    .hovered_style(Style::new().background(BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised)))
    .active_style(Style::new().background(BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised)))
    .on_click(move |event: MouseEvent| on_click(&event))
    .child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon: "refresh-cw",
      size: 15.0,
      color: theme::palette().text_muted,
    }))
    .into()
}
