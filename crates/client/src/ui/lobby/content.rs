use lurq::{
  app::ctx::Ctx,
  components::{Column, Row, Text},
  core::Signal,
  layout::{
    Alignment,
    layout_kind::{Justify, ScrollState},
  },
  node::{BackgroundColor, CursorIcon, Element, Style, border::Border, dimension::Dimension},
};

use super::{
  ChatHistoryAction, SendChatAction, StopStreamAction, StopWatchingAction, WatchStreamAction,
  chat::{ChatChannel, ChatCommandInvalidFeedback, text_channel_detail},
  layout::lobby_layout_metrics,
  model::{
    chat_pane_model, selected_text_channel, stream_browser_channel, stream_browser_model, stream_watching_model,
    unique_lobby_member_count,
  },
  shared::error_notice,
  stream_browser::stream_browser,
  stream_watching::{stream_watching, stream_watching_top_bar},
};
use crate::{
  network::protocol::{ChannelId, UserId},
  session::{ConnectedServerInfo, LobbyChannel, LobbyState, ServerSession},
  storage::Storage,
  theme,
  ui::common::lucide_icon::{LucideIcon, LucideIconProps},
};

pub(super) fn main(
  ctx: &mut Ctx,
  info: &ConnectedServerInfo,
  lobby: &LobbyState,
  message_input: Signal<String>,
  chat_command_selected_index: Signal<usize>,
  chat_command_scroll_state: ScrollState,
  chat_command_invalid_feedback: ChatCommandInvalidFeedback,
  chat_scroll_state: ScrollState,
  chat_bottom_anchor: Signal<Option<(ChannelId, u64)>>,
  chat_bottom_settle_anchor: Signal<Option<(ChannelId, u64, f32)>>,
  chat_bottom_detached_anchor: Signal<Option<(ChannelId, u64)>>,
  chat_top_anchor: Signal<Option<(ChannelId, u64)>>,
  chat_prepend_settle_anchor: Signal<Option<(ChannelId, u64, f32)>>,
  debug_mode_enabled: bool,
  storage: Option<Storage>,
  session: ServerSession,
  chat_history: &ChatHistoryAction,
  send_chat: &SendChatAction,
  start_stream_modal_open: Signal<bool>,
  stop_stream: &StopStreamAction,
  watch_stream: &WatchStreamAction,
  stop_watching: &StopWatchingAction,
) -> Element {
  Column::new()
    .width(Dimension::Pct(100.0))
    .height(Dimension::Pct(100.0))
    .flex(1.0)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceBase))
    .child(main_top_bar(
      ctx,
      info.user_id,
      lobby,
      debug_mode_enabled,
      start_stream_modal_open.clone(),
      stop_stream,
      stop_watching,
    ))
    .child(main_body(
      ctx,
      info,
      lobby,
      message_input,
      chat_command_selected_index,
      chat_command_scroll_state,
      chat_command_invalid_feedback,
      chat_scroll_state,
      chat_bottom_anchor,
      chat_bottom_settle_anchor,
      chat_bottom_detached_anchor,
      chat_top_anchor,
      chat_prepend_settle_anchor,
      debug_mode_enabled,
      storage,
      session,
      chat_history,
      send_chat,
      start_stream_modal_open,
      stop_stream,
      watch_stream,
      stop_watching,
    ))
    .into()
}

fn main_top_bar(
  ctx: &mut Ctx,
  _local_user_id: UserId,
  lobby: &LobbyState,
  debug_mode_enabled: bool,
  start_stream_modal_open: Signal<bool>,
  _stop_stream: &StopStreamAction,
  stop_watching: &StopWatchingAction,
) -> Element {
  let metrics = lobby_layout_metrics(ctx);
  if debug_mode_enabled && lobby.debug_chat_selected {
    let channel = ChatChannel::debug(ctx);
    return chat_channel_top_bar(ctx, &channel, None);
  }

  if let Some(channel) = selected_text_channel(lobby) {
    let channel = ChatChannel::server_text(ctx, channel, lobby.chat_command_registry.clone());
    return chat_channel_top_bar(ctx, &channel, Some(unique_lobby_member_count(lobby)));
  }

  if let Some(channel) = stream_browser_channel(lobby) {
    if let Some(model) = stream_watching_model(lobby, channel.id) {
      return stream_watching_top_bar(
        ctx,
        model.stream,
        debug_mode_enabled,
        start_stream_modal_open,
        stop_watching,
      );
    }

    let user_count = lobby.users_by_channel.get(&channel.id).map(Vec::len).unwrap_or(0);
    return voice_stream_top_bar(ctx, channel, user_count);
  }

  Row::new()
    .width(Dimension::Pct(100.0))
    .height(56.0)
    .align_items(Alignment::Center)
    .spacing(theme::SpacingSize::Md)
    .padding_horizontal(metrics.top_bar_padding_x)
    .border_bottom(Border::inside(1.0, theme::PaletteColor::Border))
    .child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon: "volume-2",
      size: 16.0,
      color: theme::palette().text_secondary,
    }))
    .child(Text::new(&ctx.t("lobby.title")).variant(theme::TypographyStyle::Heading))
    .into()
}

fn chat_channel_top_bar(ctx: &mut Ctx, channel: &ChatChannel, member_count: Option<usize>) -> Element {
  let metrics = lobby_layout_metrics(ctx);
  let mut row = Row::new()
    .width(Dimension::Pct(100.0))
    .height(56.0)
    .align_items(Alignment::Center)
    .spacing(12.0)
    .padding_horizontal(metrics.top_bar_padding_x)
    .border_bottom(Border::inside(1.0, theme::PaletteColor::Border))
    .child(top_bar_plain_icon(ctx, channel.icon(), 18.0))
    .child(top_bar_label(
      channel.name(),
      theme::TypographyStyle::Heading,
      theme::PaletteColor::TextPrimary,
    ))
    .child(
      Row::new()
        .height(Dimension::Pct(100.0))
        .align_items(Alignment::Center)
        .child(
          Row::new()
            .width(1.0)
            .height(20.0)
            .background(BackgroundColor::Palette(theme::PaletteColor::Border)),
        ),
    )
    .child(top_bar_label(
      channel.topic(),
      theme::TypographyStyle::Caption,
      theme::PaletteColor::TextMuted,
    ))
    .child(Row::new().flex(1.0));

  if channel.shows_text_tools() {
    row = row.child(top_bar_icon(ctx, "search")).child(top_bar_icon(ctx, "pin"));
    if let Some(member_count) = member_count {
      row = row.child(
        Row::new()
          .height(Dimension::Pct(100.0))
          .align_items(Alignment::Center)
          .spacing(6.0)
          .child(top_bar_icon(ctx, "users"))
          .child(top_bar_label(
            &member_count.to_string(),
            theme::TypographyStyle::Mono,
            theme::PaletteColor::TextMuted,
          )),
      );
    }
  }

  row.into()
}

fn voice_stream_top_bar(ctx: &mut Ctx, channel: &LobbyChannel, user_count: usize) -> Element {
  let metrics = lobby_layout_metrics(ctx);
  Row::new()
    .width(Dimension::Pct(100.0))
    .height(56.0)
    .align_items(Alignment::Center)
    .spacing(10.0)
    .padding_horizontal(metrics.top_bar_padding_x)
    .border_bottom(Border::inside(1.0, theme::PaletteColor::Border))
    .child(top_bar_plain_icon(ctx, "volume-2", 16.0))
    .child(top_bar_label(
      &channel.name,
      theme::TypographyStyle::Heading,
      theme::PaletteColor::TextPrimary,
    ))
    .child(user_count_chip(ctx, user_count))
    .child(Row::new().flex(1.0))
    .into()
}

fn user_count_chip(ctx: &mut Ctx, user_count: usize) -> Element {
  Row::new()
    .height(Dimension::Pct(100.0))
    .align_items(Alignment::Center)
    .child(
      Row::new()
        .height(22.0)
        .align_items(Alignment::Center)
        .justify(Justify::Center)
        .spacing(5.0)
        .padding_horizontal(4.0)
        .child(ctx.mount::<LucideIcon>(LucideIconProps {
          icon: "users",
          size: 14.0,
          color: theme::palette().text_muted,
        }))
        .child(
          Text::new(&user_count.to_string())
            .variant(theme::TypographyStyle::Caption)
            .color(theme::PaletteColor::TextMuted),
        ),
    )
    .into()
}

fn top_bar_label(text: &str, variant: theme::TypographyStyle, color: theme::PaletteColor) -> Element {
  Row::new()
    .height(Dimension::Pct(100.0))
    .align_items(Alignment::Center)
    .child(Text::new(text).variant(variant).color(color))
    .into()
}

fn top_bar_plain_icon(ctx: &mut Ctx, icon: &'static str, size: f32) -> Element {
  Row::new()
    .height(Dimension::Pct(100.0))
    .align_items(Alignment::Center)
    .child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon,
      size,
      color: theme::palette().text_muted,
    }))
    .into()
}

fn top_bar_icon(ctx: &mut Ctx, icon: &'static str) -> Element {
  Row::new()
    .width(28.0)
    .height(28.0)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .rounded(theme::RadiusSize::Md)
    .child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon,
      size: 17.0,
      color: theme::palette().text_muted,
    }))
    .into()
}

fn main_body(
  ctx: &mut Ctx,
  info: &ConnectedServerInfo,
  lobby: &LobbyState,
  message_input: Signal<String>,
  chat_command_selected_index: Signal<usize>,
  chat_command_scroll_state: ScrollState,
  chat_command_invalid_feedback: ChatCommandInvalidFeedback,
  chat_scroll_state: ScrollState,
  chat_bottom_anchor: Signal<Option<(ChannelId, u64)>>,
  chat_bottom_settle_anchor: Signal<Option<(ChannelId, u64, f32)>>,
  chat_bottom_detached_anchor: Signal<Option<(ChannelId, u64)>>,
  chat_top_anchor: Signal<Option<(ChannelId, u64)>>,
  chat_prepend_settle_anchor: Signal<Option<(ChannelId, u64, f32)>>,
  debug_mode_enabled: bool,
  storage: Option<Storage>,
  session: ServerSession,
  chat_history: &ChatHistoryAction,
  send_chat: &SendChatAction,
  start_stream_modal_open: Signal<bool>,
  stop_stream: &StopStreamAction,
  watch_stream: &WatchStreamAction,
  stop_watching: &StopWatchingAction,
) -> Element {
  if debug_mode_enabled && lobby.debug_chat_selected {
    let channel = ChatChannel::debug(ctx);
    let model = chat_pane_model(info, lobby, channel.id(), channel.is_server_backed());
    return text_channel_detail(
      ctx,
      channel,
      model,
      message_input,
      chat_command_selected_index,
      chat_command_scroll_state,
      chat_command_invalid_feedback,
      chat_scroll_state,
      chat_bottom_anchor,
      chat_bottom_settle_anchor,
      chat_bottom_detached_anchor,
      chat_top_anchor,
      chat_prepend_settle_anchor,
      debug_mode_enabled,
      session,
      chat_history,
      send_chat,
    );
  }

  if let Some(channel) = selected_text_channel(lobby) {
    let channel = ChatChannel::server_text(ctx, channel, lobby.chat_command_registry.clone());
    let model = chat_pane_model(info, lobby, channel.id(), channel.is_server_backed());
    return text_channel_detail(
      ctx,
      channel,
      model,
      message_input,
      chat_command_selected_index,
      chat_command_scroll_state,
      chat_command_invalid_feedback,
      chat_scroll_state,
      chat_bottom_anchor,
      chat_bottom_settle_anchor,
      chat_bottom_detached_anchor,
      chat_top_anchor,
      chat_prepend_settle_anchor,
      debug_mode_enabled,
      session,
      chat_history,
      send_chat,
    );
  }

  if let Some(channel) = stream_browser_channel(lobby) {
    if let Some(model) = stream_watching_model(lobby, channel.id) {
      return stream_watching(ctx, model, debug_mode_enabled, storage, session, watch_stream);
    }

    return stream_browser(
      ctx,
      stream_browser_model(lobby, channel),
      info.user_id,
      debug_mode_enabled,
      start_stream_modal_open,
      stop_stream,
      watch_stream,
      stop_watching,
    );
  }

  if lobby.channels.is_empty() {
    return empty_voice_state(ctx, lobby.last_error.as_deref());
  }

  select_channel_state(ctx, lobby.last_error.as_deref())
}

fn empty_voice_state(ctx: &mut Ctx, error: Option<&str>) -> Element {
  let metrics = lobby_layout_metrics(ctx);
  let mut body = Column::new()
    .width(Dimension::Pct(100.0))
    .flex(1.0)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .spacing(theme::SpacingSize::Xl)
    .child(
      Row::new()
        .width(64.0)
        .height(64.0)
        .align_items(Alignment::Center)
        .justify(Justify::Center)
        .rounded(16.0)
        .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised))
        .border_inside(1.0, theme::PaletteColor::Border)
        .child(ctx.mount::<LucideIcon>(LucideIconProps {
          icon: "volume-2",
          size: 28.0,
          color: theme::palette().text_secondary,
        })),
    )
    .child(
      Column::new()
        .width(Dimension::Pct(100.0))
        .max_width(metrics.copy_max_width)
        .align_items(Alignment::Center)
        .spacing(theme::SpacingSize::Md)
        .child(Text::new(&ctx.t("lobby.empty.title")).variant(theme::TypographyStyle::Title))
        .child(
          Text::new(&ctx.t("lobby.empty.description"))
            .variant(theme::TypographyStyle::Description)
            .text_align(Alignment::Center)
            .width(Dimension::Pct(100.0)),
        ),
    )
    .child(create_voice_button(ctx));

  if let Some(error) = error {
    body = body.child(error_notice(ctx, error));
  }

  body.into()
}

fn select_channel_state(ctx: &mut Ctx, error: Option<&str>) -> Element {
  let mut body = Column::new()
    .width(Dimension::Pct(100.0))
    .flex(1.0)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .spacing(theme::SpacingSize::Lg)
    .child(
      Text::new(&ctx.t("lobby.select.title"))
        .variant(theme::TypographyStyle::Title)
        .color(theme::PaletteColor::TextPrimary),
    )
    .child(
      Text::new(&ctx.t("lobby.select.description"))
        .variant(theme::TypographyStyle::Description)
        .color(theme::PaletteColor::TextSecondary),
    );

  if let Some(error) = error {
    body = body.child(error_notice(ctx, error));
  }

  body.into()
}

fn create_voice_button(ctx: &mut Ctx) -> Element {
  Row::new()
    .height(34.0)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .spacing(theme::SpacingSize::Sm)
    .padding_horizontal(theme::SpacingSize::Lg)
    .rounded(theme::RadiusSize::Md)
    .background(BackgroundColor::Palette(theme::PaletteColor::Accent))
    .cursor(CursorIcon::Pointer)
    .hovered_style(Style::new().background(BackgroundColor::Palette(theme::PaletteColor::AccentHover)))
    .child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon: "plus",
      size: 16.0,
      color: theme::palette().text_inverse,
    }))
    .child(
      Text::new(&ctx.t("lobby.empty.create"))
        .variant(theme::TypographyStyle::Button)
        .color(theme::PaletteColor::TextInverse),
    )
    .into()
}
