use std::sync::Arc;

use lurq::{
  app::ctx::Ctx,
  components::{Column, Row, Stack, Text, TextOverflow},
  core::Signal,
  layout::{Alignment, StackAlignment, layout_kind::Justify},
  node::{BackgroundColor, CursorIcon, Element, Style, border::Border, color::Color, dimension::Dimension},
};

use super::{
  StopWatchingAction, WatchStreamAction,
  stream_browser::{ChannelScreenShare, screen_shares_for_channel},
};
use crate::{
  network::protocol::{ChannelId, UserId, VideoCodecId},
  session::{LobbyScreenShare, LobbyState, ServerSession},
  storage::Storage,
  theme,
  ui::common::{
    lucide_icon::{LucideIcon, LucideIconProps},
    percent_slider::{PercentSlider, PercentSliderProps},
  },
};

const STREAM_VOLUME_CONTROL_WIDTH: f32 = 168.0;
const STREAM_VOLUME_TRACK_WIDTH: f32 = 104.0;
const STREAM_VOLUME_VALUE_WIDTH: f32 = 36.0;
const STREAM_VOLUME_VALUE_SPACING: f32 = 8.0;

pub(super) fn watched_stream_for_channel(lobby: &LobbyState, channel_id: ChannelId) -> Option<ChannelScreenShare<'_>> {
  let watching_user_id = lobby.watching_user_id?;
  screen_shares_for_channel(lobby, channel_id)
    .into_iter()
    .find(|stream| stream.share.sharer_user_id == watching_user_id)
}

pub(super) fn stream_watching_top_bar(
  ctx: &mut Ctx,
  stream: ChannelScreenShare<'_>,
  start_stream_modal_open: Signal<bool>,
  stop_watching: &StopWatchingAction,
) -> Element {
  let name = stream_name(ctx, &stream);
  let title = ctx.t_args("lobby.stream_browser.watching.screen_name", [("user", name)]);

  Row::new()
    .width(Dimension::Pct(100.0))
    .height(56.0)
    .align_items(Alignment::Center)
    .justify(Justify::SpaceBetween)
    .padding_horizontal(20.0)
    .border_bottom(Border::inside(1.0, theme::PaletteColor::Border))
    .child(
      Row::new()
        .align_items(Alignment::Center)
        .spacing(10.0)
        .child(back_button(ctx, stop_watching))
        .child(
          Text::new(&title)
            .variant(theme::TypographyStyle::Button)
            .color(theme::PaletteColor::TextPrimary),
        )
        .child(live_badge(ctx)),
    )
    .child(start_stream_button(ctx, start_stream_modal_open))
    .into()
}

pub(super) fn stream_watching(
  ctx: &mut Ctx,
  stream: ChannelScreenShare<'_>,
  streams: Vec<ChannelScreenShare<'_>>,
  error: Option<&str>,
  storage: Option<Storage>,
  session: ServerSession,
  watch_stream: &WatchStreamAction,
) -> Element {
  let watched_user_id = stream.share.sharer_user_id;
  let mut body = Column::new()
    .width(Dimension::Pct(100.0))
    .height(Dimension::Pct(100.0))
    .flex(1.0)
    .spacing(16.0)
    .padding(20.0)
    .child(stage(ctx, &stream, storage, &session))
    .child(stream_switcher(ctx, watched_user_id, streams, watch_stream));

  if let Some(error) = error {
    body = body.child(super::shared::error_notice(ctx, error));
  }

  body.into()
}

fn stage(ctx: &mut Ctx, stream: &ChannelScreenShare<'_>, storage: Option<Storage>, session: &ServerSession) -> Element {
  let name = stream_name(ctx, stream);
  let title = ctx.t_args("lobby.stream_browser.watching.screen_name", [("user", name.clone())]);
  let meta = stream_footer_meta(&name, stream.share);
  let speaking = stream_speaking(stream);
  let image = session.video_frame(stream.share.sharer_user_id);

  let mut stage = Stack::new()
    .stack_align(StackAlignment::Center)
    .width(Dimension::Pct(100.0))
    .flex(1.0)
    .rounded(10.0)
    .clip()
    .background(BackgroundColor::Color(Color::from_hex("#0F1013")))
    .border_inside(1.0, theme::PaletteColor::Border);

  if let Some(image) = image {
    stage = stage.background_image(image).background_contain();
  } else {
    stage = stage.child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon: "monitor",
      size: 72.0,
      color: Color::from_hex("#2E333B"),
    }));
  }

  stage
    .child(
      Column::new()
        .width(Dimension::Pct(100.0))
        .height(Dimension::Pct(100.0))
        .justify(Justify::SpaceBetween)
        .padding(14.0)
        .child(
          Row::new()
            .width(Dimension::Pct(100.0))
            .align_items(Alignment::Center)
            .justify(Justify::SpaceBetween)
            .child(live_badge(ctx))
            .child(resolution_badge(ctx, stream.share)),
        )
        .child(
          Row::new()
            .width(Dimension::Pct(100.0))
            .align_items(Alignment::End)
            .justify(Justify::SpaceBetween)
            .child(streamer_label(&name, &title, &meta, speaking))
            .child(stage_controls(ctx, session, storage, stream.share.sharer_user_id)),
        ),
    )
    .into()
}

fn stream_switcher(
  ctx: &mut Ctx,
  watched_user_id: UserId,
  streams: Vec<ChannelScreenShare<'_>>,
  watch_stream: &WatchStreamAction,
) -> Element {
  let mut row = Row::new()
    .width(Dimension::Pct(100.0))
    .height(126.0)
    .align_items(Alignment::Center)
    .spacing(10.0);

  for stream in streams {
    row = row.child(switcher_card(ctx, stream, watched_user_id, watch_stream));
  }

  row.into()
}

fn switcher_card(
  ctx: &mut Ctx,
  stream: ChannelScreenShare<'_>,
  watched_user_id: UserId,
  watch_stream: &WatchStreamAction,
) -> Element {
  let sharer_id = stream.share.sharer_user_id;
  let watching = sharer_id == watched_user_id;
  let name = stream_name(ctx, &stream);
  let title = ctx.t_args("lobby.stream_browser.watching.screen_name", [("user", name.clone())]);
  let speaking = stream_speaking(&stream);
  let action = watch_stream.clone();
  let mut card = Column::new()
    .width(168.0)
    .height(126.0)
    .rounded(8.0)
    .clip()
    .background(BackgroundColor::Color(Color::from_hex("#15171A")))
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
    .child(mini_thumb(ctx, stream.share))
    .child(
      Row::new()
        .width(Dimension::Pct(100.0))
        .height(44.0)
        .align_items(Alignment::Center)
        .padding_vertical(8.0)
        .padding_horizontal(10.0)
        .spacing(8.0)
        .child(avatar(&name, 22.0, speaking))
        .child(
          Text::new(&title)
            .width(Dimension::Pct(100.0))
            .flex(1.0)
            .variant(theme::TypographyStyle::Caption)
            .color(theme::PaletteColor::TextPrimary)
            .nowrap()
            .text_overflow(TextOverflow::Elipsis),
        ),
    );

  if !watching && !watch_stream.state().get().is_pending() {
    card = card.on_click(move |_| action.run(sharer_id));
  }

  card.into()
}

fn mini_thumb(ctx: &mut Ctx, stream: &LobbyScreenShare) -> Element {
  Stack::new()
    .stack_align(StackAlignment::Center)
    .width(Dimension::Pct(100.0))
    .height(82.0)
    .background(BackgroundColor::Color(Color::from_hex("#0F1013")))
    .child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon: "monitor",
      size: 28.0,
      color: Color::from_hex("#2E333B"),
    }))
    .child(
      Column::new()
        .width(Dimension::Pct(100.0))
        .height(Dimension::Pct(100.0))
        .justify(Justify::End)
        .padding(8.0)
        .child(
          Row::new()
            .width(Dimension::Pct(100.0))
            .justify(Justify::End)
            .child(resolution_badge(ctx, stream)),
        ),
    )
    .into()
}

fn streamer_label(name: &str, title: &str, meta: &str, active: bool) -> Element {
  Row::new()
    .align_items(Alignment::Center)
    .spacing(10.0)
    .child(avatar(name, 32.0, active))
    .child(
      Column::new()
        .spacing(2.0)
        .child(
          Text::new(title)
            .variant(theme::TypographyStyle::Button)
            .color(theme::PaletteColor::TextPrimary),
        )
        .child(
          Text::new(meta)
            .variant(theme::TypographyStyle::Caption)
            .color(theme::PaletteColor::TextMuted),
        ),
    )
    .into()
}

fn stage_controls(ctx: &mut Ctx, session: &ServerSession, storage: Option<Storage>, user_id: UserId) -> Element {
  let server_id = session.info().map(|info| info.address);
  let volume = storage
    .as_ref()
    .zip(server_id.as_deref())
    .and_then(|(storage, server_id)| storage.load_volume_override(server_id, user_id).ok().flatten())
    .unwrap_or_else(|| session.user_volume(user_id))
    .clamp(0, 100);
  session.set_user_volume(user_id, volume);

  let save_session = session.clone();
  let save_storage = storage.clone();
  let save_server_id = server_id.clone();

  Row::new()
    .align_items(Alignment::Center)
    .spacing(8.0)
    .padding(6.0)
    .rounded(10.0)
    .background(BackgroundColor::Color(Color::from_hex("#000000A6")))
    .child(stage_control_icon(ctx, "volume-2"))
    .child(ctx.mount::<PercentSlider>(PercentSliderProps {
      initial_value: volume,
      control_width: STREAM_VOLUME_CONTROL_WIDTH,
      track_width: STREAM_VOLUME_TRACK_WIDTH,
      value_width: STREAM_VOLUME_VALUE_WIDTH,
      value_spacing: STREAM_VOLUME_VALUE_SPACING,
      on_blur: Arc::new(move |volume| {
        let volume = volume.clamp(0, 100);
        save_session.set_user_volume(user_id, volume);
        if let (Some(storage), Some(server_id)) = (save_storage.as_ref(), save_server_id.as_deref()) {
          let _ = storage.save_volume_override(server_id, user_id, volume);
        }
      }),
    }))
    .child(stage_control_icon(ctx, "maximize"))
    .into()
}

fn stage_control_icon(ctx: &mut Ctx, icon: &'static str) -> Element {
  Row::new()
    .width(28.0)
    .height(28.0)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .rounded(6.0)
    .child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon,
      size: 16.0,
      color: theme::palette().text_secondary,
    }))
    .into()
}

fn back_button(ctx: &mut Ctx, stop_watching: &StopWatchingAction) -> Element {
  let pending = stop_watching.state().get().is_pending();
  let action = stop_watching.clone();
  let mut button = Row::new()
    .width(28.0)
    .height(28.0)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .rounded(6.0)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised))
    .border_inside(1.0, theme::PaletteColor::Border)
    .cursor(CursorIcon::Pointer)
    .hovered_style(Style::new().background(BackgroundColor::Palette(theme::PaletteColor::SurfaceInput)))
    .child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon: "arrow-left",
      size: 16.0,
      color: theme::palette().text_secondary,
    }));

  if !pending {
    button = button.on_click(move |_| action.run(()));
  }

  button.into()
}

fn start_stream_button(ctx: &mut Ctx, start_stream_modal_open: Signal<bool>) -> Element {
  let open = start_stream_modal_open.clone();
  let mut button = Row::new()
    .height(34.0)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .spacing(7.0)
    .padding_horizontal(14.0)
    .rounded(5.0)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised))
    .border_inside(1.0, theme::PaletteColor::BorderStrong)
    .cursor(CursorIcon::Pointer)
    .hovered_style(Style::new().background(BackgroundColor::Palette(theme::PaletteColor::SurfaceInput)))
    .child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon: "monitor-up",
      size: 16.0,
      color: theme::palette().text_secondary,
    }))
    .child(
      Text::new(&ctx.t("lobby.stream_browser.watching.share_screen"))
        .variant(theme::TypographyStyle::Button)
        .color(theme::PaletteColor::TextPrimary),
    );

  button = button.on_click(move |_| open.set(true));

  button.into()
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

fn avatar(name: &str, size: f32, active: bool) -> Element {
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

fn stream_name(ctx: &mut Ctx, stream: &ChannelScreenShare<'_>) -> String {
  stream
    .user
    .map(|user| user.username.clone())
    .unwrap_or_else(|| fallback_user_name(ctx, stream.share.sharer_user_id))
}

fn stream_speaking(stream: &ChannelScreenShare<'_>) -> bool {
  stream
    .user
    .is_some_and(|user| user.speaking && !user.muted && !user.deafened)
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

fn initials_for_user(name: &str) -> String {
  let initials = name
    .chars()
    .filter(|ch| ch.is_alphanumeric())
    .flat_map(|ch| ch.to_uppercase())
    .take(1)
    .collect::<String>();

  if initials.is_empty() { "?".to_owned() } else { initials }
}
