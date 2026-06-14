use lurq::app::{ctx::Ctx, theme::Breakpoint};

#[derive(Clone, Copy)]
pub(super) struct LobbyLayoutMetrics {
  pub rail_width: f32,
  pub rail_padding_x: f32,
  pub rail_padding_y: f32,
  pub top_bar_padding_x: f32,
  pub copy_max_width: f32,
}

pub(super) const RAIL_DIVIDER_WIDTH: f32 = 1.0;

pub(super) fn lobby_layout_metrics(ctx: &Ctx) -> LobbyLayoutMetrics {
  match ctx.breakpoint() {
    Some(Breakpoint::Md) => LobbyLayoutMetrics {
      rail_width: 236.0,
      rail_padding_x: 10.0,
      rail_padding_y: 12.0,
      top_bar_padding_x: 18.0,
      copy_max_width: 380.0,
    },
    Some(Breakpoint::Lg) => LobbyLayoutMetrics {
      rail_width: 260.0,
      rail_padding_x: 12.0,
      rail_padding_y: 14.0,
      top_bar_padding_x: 22.0,
      copy_max_width: 440.0,
    },
    Some(Breakpoint::Xl) | Some(Breakpoint::Sm) | None => LobbyLayoutMetrics {
      rail_width: 280.0,
      rail_padding_x: 12.0,
      rail_padding_y: 14.0,
      top_bar_padding_x: 24.0,
      copy_max_width: 480.0,
    },
  }
}
