use std::collections::HashSet;

use lurq::{
  app::ctx::Ctx,
  components::{Column, Row, ScrollVertical, Stack, Text, TextOverflow},
  core::Signal,
  layout::{
    Alignment, StackAlignment,
    layout_kind::Justify,
    scrollbar::{ScrollBarPlacement, ScrollBarStyle},
  },
  node::{BackgroundColor, CursorIcon, Element, Style, color::Color, dimension::Dimension},
};

use super::{
  StopStreamAction, StopWatchingAction, WatchStreamAction, layout::lobby_layout_metrics, shared::error_notice,
};
use crate::{
  network::protocol::{ChannelId, UserId, VideoCodecId},
  session::{LobbyChannel, LobbyScreenShare, LobbyState, LobbyUser},
  theme,
  ui::common::lucide_icon::{LucideIcon, LucideIconProps},
};

const LOBBY_GRID_PADDING: f32 = 20.0;
const LOBBY_GRID_GAP: f32 = 16.0;
const LOBBY_GRID_MIN_CARD_WIDTH: f32 = 300.0;
const LOBBY_GRID_MAX_CARD_WIDTH: f32 = 380.0;
const LOBBY_STREAM_CARD_HEIGHT: f32 = 208.0;
const LOBBY_STREAM_FOOTER_HEIGHT: f32 = 58.0;

pub(super) struct ChannelScreenShare<'a> {
  pub(super) share: &'a LobbyScreenShare,
  pub(super) user: Option<&'a LobbyUser>,
}

pub(super) fn screen_shares_for_channel(lobby: &LobbyState, channel_id: ChannelId) -> Vec<ChannelScreenShare<'_>> {
  let Some(users) = lobby.users_by_channel.get(&channel_id) else {
    return Vec::new();
  };
  let user_ids = users.iter().map(|user| user.user_id).collect::<HashSet<_>>();

  lobby
    .screen_shares
    .iter()
    .filter(|share| user_ids.contains(&share.sharer_user_id))
    .map(|share| ChannelScreenShare {
      share,
      user: users.iter().find(|user| user.user_id == share.sharer_user_id),
    })
    .collect()
}

pub(super) fn stream_browser(
  ctx: &mut Ctx,
  channel: &LobbyChannel,
  local_user_id: UserId,
  lobby: &LobbyState,
  _start_stream_modal_open: Signal<bool>,
  stop_stream: &StopStreamAction,
  watch_stream: &WatchStreamAction,
  _stop_watching: &StopWatchingAction,
) -> Element {
  let streams = screen_shares_for_channel(lobby, channel.id);
  let users = lobby
    .users_by_channel
    .get(&channel.id)
    .map(Vec::as_slice)
    .unwrap_or(&[]);
  let mut content = Column::new()
    .width(Dimension::Pct(100.0))
    .spacing(12.0)
    .padding(LOBBY_GRID_PADDING)
    .child(merged_lobby_grid(
      ctx,
      channel,
      users,
      streams,
      local_user_id,
      lobby.watching_user_id,
      stop_stream,
      watch_stream,
    ));

  if let Some(error) = lobby.last_error.as_deref() {
    content = content.child(error_notice(ctx, error));
  }

  ScrollVertical::new(content)
    .width(Dimension::Pct(100.0))
    .height(Dimension::Pct(100.0))
    .flex(1.0)
    .scrollbar(lobby_stream_scrollbar_style())
    .scrollbar_hovered(|mut style| {
      let palette = theme::palette();
      style.thumb_color = palette.accent_hover;
      style.track_color = palette.surface_input.with_opacity(0.75);
      style
    })
    .into()
}

fn lobby_stream_scrollbar_style() -> ScrollBarStyle {
  let palette = theme::palette();
  ScrollBarStyle {
    width: 8.0,
    min_thumb_length: 32.0,
    track_color: palette.surface_input.with_opacity(0.55),
    thumb_color: palette.accent,
    thumb_radius: 4.0,
    track_radius: 4.0,
    padding: 0.0,
    placement: ScrollBarPlacement::Reserved,
    ..ScrollBarStyle::default()
  }
}

fn merged_lobby_grid(
  ctx: &mut Ctx,
  channel: &LobbyChannel,
  users: &[LobbyUser],
  streams: Vec<ChannelScreenShare<'_>>,
  local_user_id: UserId,
  watching_user_id: Option<UserId>,
  stop_stream: &StopStreamAction,
  watch_stream: &WatchStreamAction,
) -> Element {
  let columns = lobby_grid_columns(ctx);
  let card_width = lobby_card_width(ctx, columns);
  let mut stream_by_user = streams
    .into_iter()
    .map(|stream| (stream.share.sharer_user_id, stream))
    .collect::<std::collections::HashMap<_, _>>();
  let mut cards = Vec::new();

  for user in users {
    if let Some(stream) = stream_by_user.remove(&user.user_id) {
      cards.push(merged_stream_card(
        ctx,
        channel,
        stream,
        watching_user_id,
        stop_stream,
        watch_stream,
        card_width,
      ));
    } else {
      cards.push(merged_user_card(ctx, user, user.user_id == local_user_id, card_width));
    }
  }

  for stream in stream_by_user.into_values() {
    cards.push(merged_stream_card(
      ctx,
      channel,
      stream,
      watching_user_id,
      stop_stream,
      watch_stream,
      card_width,
    ));
  }

  if cards.is_empty() {
    cards.push(merged_empty_card(ctx, card_width));
  }

  let mut grid = Column::new().width(Dimension::Pct(100.0)).spacing(LOBBY_GRID_GAP);
  let mut cards = cards.into_iter();

  loop {
    let mut row = Row::new().width(Dimension::Pct(100.0)).spacing(LOBBY_GRID_GAP);
    let mut has_card = false;

    for _ in 0..columns {
      if let Some(card) = cards.next() {
        has_card = true;
        row = row.child(card);
      } else if columns > 1 {
        row = row.child(Row::new().width(card_width));
      }
    }

    if !has_card {
      break;
    }

    grid = grid.child(row);
  }

  grid.into()
}

fn lobby_grid_columns(ctx: &Ctx) -> usize {
  let content_width = lobby_grid_content_width(ctx);
  let columns = ((content_width + LOBBY_GRID_GAP) / (LOBBY_GRID_MIN_CARD_WIDTH + LOBBY_GRID_GAP)).floor() as usize;

  columns.max(1)
}

fn lobby_card_width(ctx: &Ctx, columns: usize) -> f32 {
  let gaps = (columns.saturating_sub(1) as f32) * LOBBY_GRID_GAP;

  ((lobby_grid_content_width(ctx) - gaps) / columns.max(1) as f32).clamp(0.0, LOBBY_GRID_MAX_CARD_WIDTH)
}

fn lobby_grid_content_width(ctx: &Ctx) -> f32 {
  let metrics = lobby_layout_metrics(ctx);

  (ctx.window().logical_width() - metrics.rail_width - LOBBY_GRID_PADDING * 2.0).max(0.0)
}

fn merged_stream_card(
  ctx: &mut Ctx,
  _channel: &LobbyChannel,
  stream: ChannelScreenShare<'_>,
  watching_user_id: Option<UserId>,
  _stop_stream: &StopStreamAction,
  watch_stream: &WatchStreamAction,
  card_width: f32,
) -> Element {
  let sharer_id = stream.share.sharer_user_id;
  let name = stream
    .user
    .map(|user| user.username.clone())
    .unwrap_or_else(|| fallback_user_name(ctx, sharer_id));
  let watching = watching_user_id == Some(sharer_id);
  let speaking = stream
    .user
    .is_some_and(|user| user.speaking && !user.muted && !user.deafened);
  let title = ctx.t_args("lobby.stream_browser.watching.screen_name", [("user", name.clone())]);
  let footer_meta = stream_footer_meta(&name, stream.share);
  let action = watch_stream.clone();

  let mut card = Column::new()
    .width(card_width)
    .height(LOBBY_STREAM_CARD_HEIGHT)
    .rounded(8.0)
    .clip()
    .background(BackgroundColor::Color(if watching {
      Color::from_hex("#121A23")
    } else {
      Color::from_hex("#15171A")
    }))
    .border_inside(
      1.0,
      if speaking {
        theme::PaletteColor::Success
      } else if watching {
        theme::PaletteColor::Accent
      } else {
        theme::PaletteColor::Border
      },
    )
    .cursor(CursorIcon::Pointer)
    .hovered_style(Style::new().background(BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised)))
    .child(stream_thumbnail(ctx, stream.share))
    .child(
      Row::new()
        .width(Dimension::Pct(100.0))
        .height(LOBBY_STREAM_FOOTER_HEIGHT)
        .align_items(Alignment::Center)
        .padding_vertical(10.0)
        .padding_horizontal(14.0)
        .spacing(10.0)
        .child(stream_footer_avatar(&name, speaking))
        .child(
          Column::new()
            .width(Dimension::Pct(100.0))
            .flex(1.0)
            .spacing(2.0)
            .child(
              Text::new(&title)
                .variant(theme::TypographyStyle::Button)
                .color(theme::PaletteColor::TextPrimary),
            )
            .child(
              Text::new(&footer_meta)
                .variant(theme::TypographyStyle::Caption)
                .color(theme::PaletteColor::TextMuted),
            ),
        ),
    );

  if !watching && !watch_stream.state().get().is_pending() {
    card = card.on_click(move |_| action.run(sharer_id));
  }

  card.into()
}

fn stream_thumbnail(ctx: &mut Ctx, stream: &LobbyScreenShare) -> Element {
  Stack::new()
    .stack_align(StackAlignment::Center)
    .width(Dimension::Pct(100.0))
    .flex(1.0)
    .background(BackgroundColor::Color(Color::from_hex("#0F1013")))
    .child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon: "monitor",
      size: 40.0,
      color: Color::from_hex("#2E333B"),
    }))
    .child(
      Column::new()
        .width(Dimension::Pct(100.0))
        .height(Dimension::Pct(100.0))
        .justify(Justify::SpaceBetween)
        .padding(12.0)
        .child(
          Row::new()
            .width(Dimension::Pct(100.0))
            .height(22.0)
            .align_items(Alignment::Center)
            .child(live_badge(ctx)),
        )
        .child(
          Row::new()
            .width(Dimension::Pct(100.0))
            .height(20.0)
            .align_items(Alignment::Center)
            .justify(Justify::End)
            .child(resolution_badge(ctx, stream)),
        ),
    )
    .into()
}

fn stream_footer_avatar(name: &str, active: bool) -> Element {
  Row::new()
    .width(28.0)
    .height(28.0)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .rounded(14.0)
    .background(BackgroundColor::Color(Color::from_hex("#1B1E23")))
    .border_inside(
      1.5,
      if active {
        theme::PaletteColor::Success
      } else {
        theme::PaletteColor::Border
      },
    )
    .child(
      Text::new(&initials_for_user(name))
        .variant(theme::TypographyStyle::Mono)
        .color(theme::PaletteColor::TextMuted),
    )
    .into()
}

fn merged_user_card(ctx: &mut Ctx, user: &LobbyUser, _local: bool, card_width: f32) -> Element {
  let active = user.speaking && !user.muted && !user.deafened;
  let name_max_width = (card_width - 74.0).max(60.0);

  Column::new()
    .width(card_width)
    .height(LOBBY_STREAM_CARD_HEIGHT)
    .padding(12.0)
    .rounded(8.0)
    .background(BackgroundColor::Color(Color::from_hex("#15171A")))
    .border_inside(
      1.0,
      if active {
        theme::PaletteColor::Success
      } else {
        theme::PaletteColor::Border
      },
    )
    .child(Row::new().width(Dimension::Pct(100.0)).height(22.0))
    .child(
      Row::new()
        .width(Dimension::Pct(100.0))
        .flex(1.0)
        .align_items(Alignment::Center)
        .justify(Justify::Center)
        .child(stream_user_avatar(&user.username, active, 56.0)),
    )
    .child(
      Column::new()
        .width(Dimension::Pct(100.0))
        .height(42.0)
        .align_items(Alignment::Center)
        .justify(Justify::Center)
        .child(
          Row::new()
            .width(Dimension::Pct(100.0))
            .height(22.0)
            .align_items(Alignment::Center)
            .justify(Justify::Center)
            .spacing(6.0)
            .child(
              Text::new(&user.username)
                .max_width(name_max_width)
                .variant(theme::TypographyStyle::Button)
                .color(theme::PaletteColor::TextPrimary)
                .nowrap()
                .text_overflow(TextOverflow::Elipsis),
            )
            .child(merged_voice_icons(ctx, user)),
        ),
    )
    .into()
}

fn merged_empty_card(ctx: &mut Ctx, card_width: f32) -> Element {
  Column::new()
    .width(card_width)
    .height(LOBBY_STREAM_CARD_HEIGHT)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .spacing(10.0)
    .rounded(8.0)
    .background(BackgroundColor::Color(Color::from_hex("#15171A")))
    .border_inside(1.0, theme::PaletteColor::Border)
    .child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon: "users",
      size: 28.0,
      color: theme::palette().text_muted,
    }))
    .child(
      Text::new(&ctx.t("lobby.users.empty"))
        .variant(theme::TypographyStyle::Caption)
        .color(theme::PaletteColor::TextMuted),
    )
    .into()
}

fn live_badge(ctx: &mut Ctx) -> Element {
  Row::new()
    .height(22.0)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .spacing(5.0)
    .padding_horizontal(8.0)
    .rounded(4.0)
    .background(BackgroundColor::Palette(theme::PaletteColor::DangerMuted))
    .child(
      Row::new()
        .width(6.0)
        .height(6.0)
        .rounded(3.0)
        .background(BackgroundColor::Palette(theme::PaletteColor::Danger)),
    )
    .child(
      Text::new(&ctx.t("lobby.stream_browser.watching.live"))
        .variant(theme::TypographyStyle::FieldLabel)
        .color(theme::PaletteColor::Danger),
    )
    .into()
}

fn resolution_badge(ctx: &mut Ctx, stream: &LobbyScreenShare) -> Element {
  Row::new()
    .width(96.0)
    .height(20.0)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .padding_vertical(3.0)
    .padding_horizontal(7.0)
    .rounded(4.0)
    .background(BackgroundColor::Color(Color::from_hex("#000000A6")))
    .child(
      Text::new(&stream_resolution_label(ctx, stream))
        .variant(theme::TypographyStyle::Mono)
        .color(theme::PaletteColor::TextSecondary)
        .nowrap()
        .text_overflow(TextOverflow::Elipsis),
    )
    .into()
}

fn merged_voice_icons(ctx: &mut Ctx, user: &LobbyUser) -> Element {
  let mut icons = Row::new()
    .height(18.0)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .spacing(6.0);

  if user.deafened {
    icons = icons
      .child(ctx.mount::<LucideIcon>(LucideIconProps {
        icon: "headphone-off",
        size: 14.0,
        color: theme::palette().danger,
      }))
      .child(ctx.mount::<LucideIcon>(LucideIconProps {
        icon: "mic-off",
        size: 14.0,
        color: theme::palette().danger,
      }));
  } else if user.muted {
    icons = icons.child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon: "mic-off",
      size: 14.0,
      color: theme::palette().danger,
    }));
  } else if user.speaking {
    icons = icons.child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon: "mic",
      size: 14.0,
      color: theme::palette().success,
    }));
  }

  icons.into()
}

fn stream_user_avatar(name: &str, active: bool, size: f32) -> Element {
  Row::new()
    .width(size)
    .height(size)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .rounded(size / 2.0)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised))
    .border_inside(
      1.5,
      BackgroundColor::Palette(if active {
        theme::PaletteColor::Success
      } else {
        theme::PaletteColor::Border
      }),
    )
    .child(
      Text::new(&initials_for_user(name))
        .variant(theme::TypographyStyle::Heading)
        .color(if active {
          theme::PaletteColor::TextPrimary
        } else {
          theme::PaletteColor::TextSecondary
        }),
    )
    .into()
}

fn initials_for_user(name: &str) -> String {
  let initials = name
    .chars()
    .filter(|ch| ch.is_alphanumeric())
    .flat_map(|ch| ch.to_uppercase())
    .take(1)
    .collect::<String>();

  if initials.is_empty() { "?".to_owned() } else { initials }
}

fn stream_resolution_label(ctx: &mut Ctx, stream: &LobbyScreenShare) -> String {
  if stream.metadata.width == 0 || stream.metadata.height == 0 {
    return ctx.t("lobby.stream_browser.watching.live").to_string();
  }

  format!("{}x{}", stream.metadata.width, stream.metadata.height)
}

fn stream_footer_meta(name: &str, stream: &LobbyScreenShare) -> String {
  format!("{name} · {}", stream_codec_label(stream))
}

fn stream_codec_label(stream: &LobbyScreenShare) -> String {
  match stream.metadata.codec {
    VideoCodecId::Unknown => "Live".to_owned(),
    VideoCodecId::Av1 => "AV1".to_owned(),
    VideoCodecId::H265 => "H.265".to_owned(),
    VideoCodecId::H264 => "H.264".to_owned(),
  }
}

fn fallback_user_name(ctx: &mut Ctx, user_id: UserId) -> String {
  ctx
    .t_args("lobby.user.fallback", [("id", user_id.to_string())])
    .to_string()
}
