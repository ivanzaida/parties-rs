use lurq::{
  app::ctx::Ctx,
  components::{Column, Rect, Row, Text},
  core::Signal,
  layout::{Alignment, layout_kind::Justify},
  node::{BackgroundColor, CursorIcon, Element, Style, dimension::Dimension},
};

use super::{
  AUTO_RECONNECT_MAX_ATTEMPTS, AUTO_RECONNECT_RETRY_DELAY_MS, ReconnectAction, ReconnectRequest,
  layout::lobby_layout_metrics,
};
use crate::{
  routes::ROUTE_CHOOSE_SERVER,
  session::{ConnectedServerInfo, LobbyState, ServerSession},
  theme,
  ui::common::lucide_icon::{LucideIcon, LucideIconProps},
};

pub(super) fn disconnected_lobby(
  ctx: &mut Ctx,
  info: &ConnectedServerInfo,
  lobby: &LobbyState,
  session: ServerSession,
  reconnect: &ReconnectAction,
  reconnect_attempt: Signal<u32>,
) -> Element {
  let metrics = lobby_layout_metrics(ctx);
  let navigator = ctx.navigator();
  let leave_session = session.clone();
  let reconnect_state = reconnect.state().get();
  let auto_attempt = reconnect_attempt.get();
  let should_auto_reconnect =
    !lobby.auto_reconnect_disabled && !reconnect_state.is_pending() && auto_attempt < AUTO_RECONNECT_MAX_ATTEMPTS;
  let display_attempt = if should_auto_reconnect {
    let next_attempt = auto_attempt + 1;
    reconnect_attempt.set(next_attempt);
    reconnect.run(ReconnectRequest {
      address: info.address.clone(),
      delay_ms: if next_attempt == 1 {
        0
      } else {
        AUTO_RECONNECT_RETRY_DELAY_MS
      },
    });
    next_attempt
  } else {
    auto_attempt
  };
  let reconnecting = reconnect_state.is_pending() || should_auto_reconnect;
  let reconnect_exhausted =
    !lobby.auto_reconnect_disabled && !reconnecting && display_attempt >= AUTO_RECONNECT_MAX_ATTEMPTS;
  let reconnect_address = info.address.clone();
  let reconnect_action = reconnect.clone();
  let manual_attempt = reconnect_attempt.clone();
  let server_name = if info.server_name.trim().is_empty() {
    ctx.t("lobby.server.unknown").to_string()
  } else {
    info.server_name.clone()
  };
  let description = ctx.t_args("lobby.disconnected.description", [("server", server_name.clone())]);
  let status = if reconnecting {
    ctx
      .t_args(
        "lobby.disconnected.status.reconnecting",
        [("attempt", display_attempt.max(1).to_string())],
      )
      .to_string()
  } else if reconnect_exhausted {
    ctx.t("lobby.disconnected.status.inaccessible").to_string()
  } else if reconnect_state.error.is_some() {
    ctx.t("lobby.disconnected.status.failed").to_string()
  } else {
    ctx.t("lobby.disconnected.status.ready").to_string()
  };
  let detail = reconnect_state
    .error
    .or_else(|| lobby.last_error.clone())
    .map(|error| {
      ctx
        .t_args("lobby.disconnected.footer_error", [("error", error)])
        .to_string()
    })
    .unwrap_or_else(|| {
      ctx
        .t_args("lobby.disconnected.footer", [("server", server_name.clone())])
        .to_string()
    });

  Column::new()
    .width(Dimension::Pct(100.0))
    .height(Dimension::Pct(100.0))
    .flex(1.0)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .spacing(20.0)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceBase))
    .clip()
    .child(disconnected_icon(ctx))
    .child(
      Column::new()
        .width(Dimension::Pct(100.0))
        .max_width(metrics.copy_max_width)
        .align_items(Alignment::Center)
        .spacing(7.0)
        .child(
          Text::new(&ctx.t("lobby.disconnected.title"))
            .variant(theme::TypographyStyle::Title)
            .color(theme::PaletteColor::TextPrimary)
            .text_align(Alignment::Center),
        )
        .child(
          Text::new(&description)
            .variant(theme::TypographyStyle::Description)
            .color(theme::PaletteColor::TextSecondary)
            .text_align(Alignment::Center)
            .width(Dimension::Pct(100.0)),
        ),
    )
    .child(disconnected_status_pill(&status, reconnecting))
    .child(
      Row::new()
        .align_items(Alignment::Center)
        .spacing(10.0)
        .child(
          disconnected_action_button(
            ctx,
            Some("refresh-cw"),
            &ctx.t(if reconnecting {
              "lobby.disconnected.action.reconnecting"
            } else {
              "lobby.disconnected.action.reconnect"
            }),
            true,
            !reconnecting,
          )
          .on_click(move |_| {
            if !reconnecting {
              let next_attempt = manual_attempt.get_untracked().saturating_add(1);
              manual_attempt.set(next_attempt);
              reconnect_action.run(ReconnectRequest {
                address: reconnect_address.clone(),
                delay_ms: 0,
              });
            }
          }),
        )
        .child(
          disconnected_action_button(
            ctx,
            Some("log-out"),
            &ctx.t("lobby.disconnected.action.leave"),
            false,
            true,
          )
          .on_click(move |_| {
            leave_session.disconnect();
            if let Some(navigator) = navigator.as_ref() {
              navigator.replace(ROUTE_CHOOSE_SERVER);
            }
          }),
        ),
    )
    .child(
      Text::new(&detail)
        .variant(theme::TypographyStyle::Caption)
        .color(theme::PaletteColor::TextMuted)
        .text_align(Alignment::Center),
    )
    .into()
}

fn disconnected_icon(ctx: &mut Ctx) -> Element {
  Row::new()
    .width(92.0)
    .height(92.0)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .rounded(46.0)
    .background(BackgroundColor::Palette(theme::PaletteColor::DangerMuted))
    .border_inside(1.0, theme::PaletteColor::Danger)
    .child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon: "wifi-off",
      size: 38.0,
      color: theme::palette().danger,
    }))
    .into()
}

fn disconnected_status_pill(label: &str, reconnecting: bool) -> Element {
  Row::new()
    .align_items(Alignment::Center)
    .spacing(10.0)
    .padding_vertical(9.0)
    .padding_horizontal(14.0)
    .rounded(20.0)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised))
    .border_inside(1.0, theme::PaletteColor::Border)
    .child(
      Rect::new(7.0, 7.0)
        .rounded(4.0)
        .background(BackgroundColor::Palette(if reconnecting {
          theme::PaletteColor::Accent
        } else {
          theme::PaletteColor::Warning
        })),
    )
    .child(
      Text::new(label)
        .variant(theme::TypographyStyle::Button)
        .color(theme::PaletteColor::TextSecondary),
    )
    .into()
}

fn disconnected_action_button(
  ctx: &mut Ctx,
  icon: Option<&'static str>,
  label: &str,
  primary: bool,
  enabled: bool,
) -> Row {
  let mut button = Row::new()
    .height(34.0)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .spacing(7.0)
    .padding_horizontal(14.0)
    .rounded(theme::RadiusSize::Md)
    .background(BackgroundColor::Palette(if primary {
      theme::PaletteColor::Accent
    } else {
      theme::PaletteColor::SurfaceRaised
    }))
    .border_inside(
      1.0,
      if primary {
        theme::PaletteColor::Accent
      } else {
        theme::PaletteColor::BorderStrong
      },
    );

  if let Some(icon) = icon {
    button = button.child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon,
      size: 15.0,
      color: if primary {
        theme::palette().text_inverse
      } else {
        theme::palette().text_primary
      },
    }));
  }

  button = button.child(
    Text::new(label)
      .variant(theme::TypographyStyle::Button)
      .color(if primary {
        theme::PaletteColor::TextInverse
      } else {
        theme::PaletteColor::TextPrimary
      }),
  );

  if enabled {
    button = button
      .cursor(CursorIcon::Pointer)
      .hovered_style(Style::new().background(BackgroundColor::Palette(if primary {
        theme::PaletteColor::AccentHover
      } else {
        theme::PaletteColor::SurfaceInput
      })));
  }

  button
}
