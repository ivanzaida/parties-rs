use std::collections::HashSet;

use lurq::{
  app::ctx::Ctx,
  components::{Row, Text},
  layout::{Alignment, layout_kind::Justify},
  node::{BackgroundColor, Element, color::Color},
};

use crate::{
  network::protocol::{ChannelId, UserId, VideoCodecId},
  session::{LobbyChannel, LobbyScreenShare, LobbyState, LobbyUser},
  theme,
  ui::lobby::shared::user_display_name,
};

pub(super) struct ChannelScreenShare<'a> {
  pub(super) share: &'a LobbyScreenShare,
  pub(super) user: Option<&'a LobbyUser>,
}

pub(super) struct WatchedChannelScreenShare<'a> {
  pub(super) channel: &'a LobbyChannel,
  pub(super) stream: ChannelScreenShare<'a>,
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

pub(super) fn watched_stream(lobby: &LobbyState) -> Option<WatchedChannelScreenShare<'_>> {
  let watched_user_id = lobby.watching_user_id?;

  for channel in &lobby.channels {
    let Some(users) = lobby.users_by_channel.get(&channel.id) else {
      continue;
    };
    let Some(user) = users.iter().find(|user| user.user_id == watched_user_id) else {
      continue;
    };
    let Some(share) = lobby
      .screen_shares
      .iter()
      .find(|share| share.sharer_user_id == watched_user_id)
    else {
      continue;
    };

    return Some(WatchedChannelScreenShare {
      channel,
      stream: ChannelScreenShare {
        share,
        user: Some(user),
      },
    });
  }

  None
}

pub(super) fn stream_name(ctx: &mut Ctx, stream: &ChannelScreenShare<'_>, debug_user_ids: bool) -> String {
  stream
    .user
    .map(|user| user_display_name(user.user_id, &user.username, debug_user_ids))
    .unwrap_or_else(|| fallback_user_name(ctx, stream.share.sharer_user_id))
}

pub(super) fn stream_speaking(stream: &ChannelScreenShare<'_>) -> bool {
  stream
    .user
    .is_some_and(|user| user.speaking && !user.muted && !user.deafened)
}

pub(super) fn live_badge(ctx: &mut Ctx) -> Element {
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

pub(super) fn resolution_badge(ctx: &mut Ctx, stream: &LobbyScreenShare) -> Element {
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
        .text_overflow(lurq::components::TextOverflow::Elipsis),
    )
    .into()
}

pub(super) fn stream_footer_meta(ctx: &mut Ctx, name: &str, stream: &LobbyScreenShare) -> String {
  let codec = stream_codec_label(ctx, stream);
  ctx
    .t_args(
      "lobby.stream_browser.watching.footer_meta",
      [("user", name.to_owned()), ("codec", codec)],
    )
    .to_string()
}

pub(super) fn stream_avatar(name: &str, size: f32, active: bool) -> Element {
  Row::new()
    .width(size)
    .height(size)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .rounded(size / 2.0)
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

pub(super) fn initials_for_user(name: &str) -> String {
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

  ctx
    .t_args(
      "lobby.stream_browser.watching.resolution",
      [
        ("width", stream.metadata.width.to_string()),
        ("height", stream.metadata.height.to_string()),
      ],
    )
    .to_string()
}

fn stream_codec_label(ctx: &mut Ctx, stream: &LobbyScreenShare) -> String {
  match stream.metadata.codec {
    VideoCodecId::Unknown => ctx.t("lobby.stream_browser.watching.live").to_string(),
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
