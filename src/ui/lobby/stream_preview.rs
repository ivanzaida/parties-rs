use lurq::{
  app::ctx::Ctx,
  components::{Column, Row, Stack, Text, TextOverflow},
  layout::{Alignment, StackAlignment, layout_kind::Justify},
  node::{BackgroundColor, CursorIcon, Element, Style, border::Border, color::Color, dimension::Dimension},
};

use super::stream_shared::{
  WatchedChannelScreenShare, live_badge, resolution_badge, stream_avatar, stream_name, stream_speaking,
};
use crate::{
  network::protocol::ChannelId,
  session::{LobbyState, ServerSession},
  theme,
  ui::common::lucide_icon::{LucideIcon, LucideIconProps},
};

const PREVIEW_WIDTH: f32 = 324.0;
const PREVIEW_HEIGHT: f32 = 206.0;
const PREVIEW_MARGIN: f32 = 18.0;
const PREVIEW_TOP_GAP: f32 = 14.0;
const PREVIEW_FOOTER_HEIGHT: f32 = 54.0;

pub(super) fn floating_stream_preview(
  ctx: &mut Ctx,
  lobby: &LobbyState,
  watched: WatchedChannelScreenShare<'_>,
  debug_user_ids: bool,
  session: ServerSession,
) -> Option<Element> {
  if main_pane_shows_watched_stream(lobby, watched.channel.id) {
    return None;
  }

  let channel_id = watched.channel.id;

  Some(
    preview_card(ctx, watched, debug_user_ids, session.clone())
      .absolute_position(preview_x(ctx), preview_y())
      .on_click(move |_| session.open_stream_browser(channel_id))
      .into(),
  )
}

fn preview_x(ctx: &Ctx) -> f32 {
  let window = ctx.window();
  (window.logical_width() - PREVIEW_WIDTH - PREVIEW_MARGIN).max(PREVIEW_MARGIN)
}

fn preview_y() -> f32 {
  (PREVIEW_TOP_GAP - 1.0).max(0.0)
}

fn main_pane_shows_watched_stream(lobby: &LobbyState, watched_channel_id: ChannelId) -> bool {
  if lobby.debug_chat_selected || lobby.selected_text_channel_id.is_some() {
    return false;
  }

  let visible_voice_channel_id = lobby.stream_browser_channel_id.or(lobby.selected_channel_id);
  visible_voice_channel_id == Some(watched_channel_id)
}

fn preview_card(
  ctx: &mut Ctx,
  watched: WatchedChannelScreenShare<'_>,
  debug_user_ids: bool,
  session: ServerSession,
) -> Row {
  let name = stream_name(ctx, &watched.stream, debug_user_ids);
  let avatar_name = stream_name(ctx, &watched.stream, false);
  let title = ctx.t_args("lobby.stream_browser.watching.screen_name", [("user", name.clone())]);
  let speaking = stream_speaking(&watched.stream);

  Row::new()
    .width(PREVIEW_WIDTH)
    .height(PREVIEW_HEIGHT)
    .rounded(10.0)
    .clip()
    .background(BackgroundColor::Color(Color::from_hex("#0F1013")))
    .border_inside(
      1.0,
      if speaking {
        theme::PaletteColor::Success
      } else {
        theme::PaletteColor::BorderStrong
      },
    )
    .cursor(CursorIcon::Pointer)
    .hovered_style(Style::new().border(Border::inside(
      1.0,
      if speaking {
        theme::PaletteColor::Success
      } else {
        theme::PaletteColor::Accent
      },
    )))
    .child(
      Column::new()
        .width(Dimension::Pct(100.0))
        .height(Dimension::Pct(100.0))
        .child(preview_image(ctx, &watched, &session))
        .child(preview_footer(&avatar_name, &title, &watched.channel.name, speaking)),
    )
}

fn preview_image(ctx: &mut Ctx, watched: &WatchedChannelScreenShare<'_>, session: &ServerSession) -> Element {
  let image = session.video_frame(watched.stream.share.sharer_user_id);
  let video_error = session.video_error(watched.stream.share.sharer_user_id);
  let mut stage = Stack::new()
    .stack_align(StackAlignment::Center)
    .width(Dimension::Pct(100.0))
    .height(PREVIEW_HEIGHT - PREVIEW_FOOTER_HEIGHT)
    .background(BackgroundColor::Color(Color::from_hex("#090A0D")));

  if let Some(image) = image {
    stage = stage.background_image(image).background_cover();
  } else {
    stage = stage.child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon: if video_error.is_some() {
        "triangle-alert"
      } else {
        "monitor"
      },
      size: 34.0,
      color: if video_error.is_some() {
        theme::palette().danger
      } else {
        Color::from_hex("#343943")
      },
    }));
  }

  stage
    .child(
      Column::new()
        .width(Dimension::Pct(100.0))
        .height(Dimension::Pct(100.0))
        .justify(Justify::SpaceBetween)
        .padding(10.0)
        .child(
          Row::new()
            .width(Dimension::Pct(100.0))
            .justify(Justify::SpaceBetween)
            .align_items(Alignment::Center)
            .child(live_badge(ctx))
            .child(resolution_badge(ctx, watched.stream.share)),
        ),
    )
    .into()
}

fn preview_footer(avatar_name: &str, title: &str, channel_name: &str, speaking: bool) -> Element {
  Row::new()
    .width(Dimension::Pct(100.0))
    .height(PREVIEW_FOOTER_HEIGHT)
    .align_items(Alignment::Center)
    .spacing(10.0)
    .padding_horizontal(12.0)
    .background(BackgroundColor::Color(Color::from_hex("#15171AF2")))
    .child(stream_avatar(avatar_name, 30.0, speaking))
    .child(
      Column::new()
        .width(Dimension::Pct(100.0))
        .flex(1.0)
        .spacing(2.0)
        .child(
          Text::new(title)
            .variant(theme::TypographyStyle::Button)
            .color(theme::PaletteColor::TextPrimary)
            .nowrap()
            .text_overflow(TextOverflow::Elipsis),
        )
        .child(
          Text::new(channel_name)
            .variant(theme::TypographyStyle::Caption)
            .color(theme::PaletteColor::TextMuted)
            .nowrap()
            .text_overflow(TextOverflow::Elipsis),
        ),
    )
    .into()
}
