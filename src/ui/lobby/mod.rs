use std::{collections::HashSet, process::Command};

use chrono::{DateTime, Datelike, Local, NaiveDate, TimeZone, Timelike, Weekday};
use lurq::{
  app::{component::Component, ctx::Ctx},
  components::{Column, Row, ScrollVertical, Text, TextInput},
  core::Signal,
  layout::{
    Alignment,
    layout_kind::{Justify, ScrollState},
    scrollbar::{ScrollBarPlacement, ScrollBarStyle},
  },
  node::{BackgroundColor, CursorIcon, Element, Style, border::Border, color::Color, dimension::Dimension},
};

use crate::{
  network::protocol::{ChannelId, control::ChatMessage as ProtocolChatMessage},
  routes::ROUTE_CHOOSE_SERVER,
  session::{ConnectedServerInfo, LobbyChannel, LobbyState, LobbyTextChannel, LobbyUser, ServerSession},
  theme,
  ui::{
    common::lucide_icon::{LucideIcon, LucideIconProps},
    loader::loader,
  },
};

mod channel_section;
mod rail;
mod text_channels;
mod voice_channels;

use rail::{LobbyRail, LobbyRailProps, role_label_lower, server_avatar};

type ReceiverAction = lurq::app::ctx::FutureAction<(), (), String>;
type ChatHistoryAction = lurq::app::ctx::FutureAction<ChatHistoryRequest, (), String>;
type SendChatAction = lurq::app::ctx::FutureAction<SendChatInput, (), String>;

#[derive(Clone, Copy)]
struct ChatHistoryRequest {
  channel_id: ChannelId,
  before_id: u64,
}

#[derive(Clone)]
struct SendChatInput {
  channel_id: ChannelId,
  text: String,
}

pub struct LobbyScreen {
  message_input: Signal<String>,
  chat_scroll_state: ScrollState,
  chat_bottom_anchor: Signal<Option<(ChannelId, u64)>>,
  chat_top_anchor: Signal<Option<(ChannelId, u64)>>,
}

impl Component for LobbyScreen {
  type Props = ();

  fn create(ctx: &mut Ctx) -> Self {
    Self {
      message_input: ctx.signal(String::new()),
      chat_scroll_state: ScrollState::new(),
      chat_bottom_anchor: ctx.signal(None),
      chat_top_anchor: ctx.signal(None),
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let Some(session) = ctx.use_context::<ServerSession>() else {
      return empty_lobby(ctx);
    };

    let _revision = session.revision().get();
    let Some(info) = session.info() else {
      if let Some(navigator) = ctx.navigator() {
        navigator.replace(ROUTE_CHOOSE_SERVER);
      }
      return empty_lobby(ctx);
    };

    let lobby = session.lobby();
    let receiver = receiver_action(ctx, session.clone());
    if !lobby.disconnected && !lobby.receiver_running && !receiver.state().get().is_pending() {
      receiver.run(());
    }
    let chat_history = chat_history_action(ctx, session.clone());
    if let Some(channel_id) = lobby.selected_text_channel_id
      && lobby
        .chat_messages_by_channel
        .get(&channel_id)
        .is_none_or(Vec::is_empty)
      && session.begin_chat_history_request(channel_id)
    {
      chat_history.run(ChatHistoryRequest {
        channel_id,
        before_id: 0,
      });
    }
    let send_chat = send_chat_action(ctx, session.clone());

    Row::new()
      .width(Dimension::Pct(100.0))
      .height(Dimension::Pct(100.0))
      .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceBase))
      .clip()
      .child(ctx.mount::<LobbyRail>(LobbyRailProps {
        info: info.clone(),
        lobby: lobby.clone(),
      }))
      .child(main(
        ctx,
        &info,
        &lobby,
        self.message_input.clone(),
        self.chat_scroll_state.clone(),
        self.chat_bottom_anchor.clone(),
        self.chat_top_anchor.clone(),
        session,
        &chat_history,
        &send_chat,
      ))
      .into()
  }
}

fn receiver_action(ctx: &mut Ctx, session: ServerSession) -> ReceiverAction {
  ctx.future_action(move |()| {
    let session = session.clone();
    async move {
      session.run_lobby_receiver().await;
      Ok(())
    }
  })
}

fn chat_history_action(ctx: &mut Ctx, session: ServerSession) -> ChatHistoryAction {
  ctx.future_action(move |request: ChatHistoryRequest| {
    let session = session.clone();
    async move {
      let server = session.server().ok_or_else(|| "No connected server.".to_owned())?;
      if let Err(error) = server
        .request_chat_history(request.channel_id, request.before_id, 50)
        .await
      {
        session.finish_chat_history_request(request.channel_id, true);
        return Err(error.to_string());
      }
      Ok(())
    }
  })
}

fn send_chat_action(ctx: &mut Ctx, session: ServerSession) -> SendChatAction {
  ctx.future_action(move |input: SendChatInput| {
    let session = session.clone();
    async move {
      let text = input.text.trim().to_owned();
      if text.is_empty() {
        return Ok(());
      }

      let server = session.server().ok_or_else(|| "No connected server.".to_owned())?;
      server
        .send_chat_text(input.channel_id, text)
        .await
        .map_err(|error| error.to_string())?;
      Ok(())
    }
  })
}

fn main(
  ctx: &mut Ctx,
  info: &ConnectedServerInfo,
  lobby: &LobbyState,
  message_input: Signal<String>,
  chat_scroll_state: ScrollState,
  chat_bottom_anchor: Signal<Option<(ChannelId, u64)>>,
  chat_top_anchor: Signal<Option<(ChannelId, u64)>>,
  session: ServerSession,
  chat_history: &ChatHistoryAction,
  send_chat: &SendChatAction,
) -> Element {
  Column::new()
    .width(Dimension::Pct(100.0))
    .height(Dimension::Pct(100.0))
    .flex(1.0)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceBase))
    .child(main_top_bar(ctx, lobby))
    .child(main_body(
      ctx,
      info,
      lobby,
      message_input,
      chat_scroll_state,
      chat_bottom_anchor,
      chat_top_anchor,
      session,
      chat_history,
      send_chat,
    ))
    .into()
}

fn main_top_bar(ctx: &mut Ctx, lobby: &LobbyState) -> Element {
  if let Some(channel) = selected_text_channel(lobby) {
    return text_channel_top_bar(ctx, channel, unique_lobby_member_count(lobby));
  }

  Row::new()
    .width(Dimension::Pct(100.0))
    .height(56.0)
    .align_items(Alignment::Center)
    .spacing(theme::SpacingSize::Md)
    .padding_horizontal(theme::SpacingSize::Xl)
    .border_bottom(Border::inside(1.0, theme::PaletteColor::Border))
    .child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon: "volume-2",
      size: 16.0,
      color: theme::palette().text_secondary,
    }))
    .child(Text::new(&ctx.t("lobby.title")).variant(theme::TypographyStyle::Heading))
    .into()
}

fn text_channel_top_bar(ctx: &mut Ctx, channel: &LobbyTextChannel, member_count: usize) -> Element {
  Row::new()
    .width(Dimension::Pct(100.0))
    .height(56.0)
    .align_items(Alignment::Center)
    .spacing(12.0)
    .padding_horizontal(theme::SpacingSize::Xl)
    .border_bottom(Border::inside(1.0, theme::PaletteColor::Border))
    .child(top_bar_plain_icon(ctx, "hash", 18.0))
    .child(top_bar_label(
      &channel.name,
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
      &ctx.t("lobby.text_channel.topic"),
      theme::TypographyStyle::Caption,
      theme::PaletteColor::TextMuted,
    ))
    .child(Row::new().flex(1.0))
    .child(top_bar_icon(ctx, "search"))
    .child(top_bar_icon(ctx, "pin"))
    .child(
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
  chat_scroll_state: ScrollState,
  chat_bottom_anchor: Signal<Option<(ChannelId, u64)>>,
  chat_top_anchor: Signal<Option<(ChannelId, u64)>>,
  session: ServerSession,
  chat_history: &ChatHistoryAction,
  send_chat: &SendChatAction,
) -> Element {
  if let Some(channel) = selected_text_channel(lobby) {
    return text_channel_detail(
      ctx,
      channel,
      info,
      lobby,
      message_input,
      chat_scroll_state,
      chat_bottom_anchor,
      chat_top_anchor,
      session,
      chat_history,
      send_chat,
    );
  }

  let selected = lobby
    .selected_channel_id
    .and_then(|id| lobby.channels.iter().find(|channel| channel.id == id));

  if lobby.channels.is_empty() {
    return empty_voice_state(ctx, lobby.last_error.as_deref());
  }

  if let Some(channel) = selected {
    return channel_detail(ctx, channel, info, lobby);
  }

  select_channel_state(ctx, lobby.last_error.as_deref())
}

fn text_channel_detail(
  ctx: &mut Ctx,
  channel: &LobbyTextChannel,
  info: &ConnectedServerInfo,
  lobby: &LobbyState,
  message_input: Signal<String>,
  chat_scroll_state: ScrollState,
  chat_bottom_anchor: Signal<Option<(ChannelId, u64)>>,
  chat_top_anchor: Signal<Option<(ChannelId, u64)>>,
  session: ServerSession,
  chat_history: &ChatHistoryAction,
  send_chat: &SendChatAction,
) -> Element {
  let messages = lobby
    .chat_messages_by_channel
    .get(&channel.id)
    .cloned()
    .unwrap_or_default();
  let oldest_message_id = messages.first().map(|message| message.id).unwrap_or(0);
  let newest_message_id = messages.last().map(|message| message.id).unwrap_or(0);
  let newest_message_from_local = messages.last().is_some_and(|message| message.sender_id == info.user_id);
  let can_page = oldest_message_id != 0
    && lobby.chat_history_has_more.get(&channel.id).copied().unwrap_or(true)
    && !lobby.chat_history_loading.contains(&channel.id);
  preserve_chat_scroll_on_prepend(
    channel.id,
    oldest_message_id,
    chat_scroll_state.clone(),
    chat_top_anchor,
  );
  schedule_chat_scroll_to_bottom(
    channel.id,
    newest_message_id,
    newest_message_from_local,
    chat_scroll_state.clone(),
    chat_bottom_anchor,
  );
  let mut body = Column::new()
    .width(Dimension::Pct(100.0))
    .height(Dimension::Pct(100.0))
    .flex(1.0);
  let mut messages_column = Column::new()
    .width(Dimension::Pct(100.0))
    .spacing(18.0)
    .padding_vertical(theme::SpacingSize::Xl)
    .padding_horizontal(24.0);

  if messages.is_empty() {
    messages_column = messages_column.child(
      Column::new()
        .width(Dimension::Pct(100.0))
        .flex(1.0)
        .align_items(Alignment::Center)
        .justify(Justify::Center)
        .spacing(theme::SpacingSize::Sm)
        .child(
          Text::new(&ctx.t("lobby.text_channel.empty.title"))
            .variant(theme::TypographyStyle::Title)
            .color(theme::PaletteColor::TextPrimary),
        )
        .child(
          Text::new(&ctx.t("lobby.text_channel.empty.description"))
            .variant(theme::TypographyStyle::Description)
            .color(theme::PaletteColor::TextMuted),
        ),
    );
  } else {
    let today = Local::now().date_naive();
    let mut last_day = None;
    for message in &messages {
      let message_day = local_chat_date(message.timestamp);
      if last_day != Some(message_day) {
        messages_column = messages_column.child(chat_day_divider(ctx, message_day, today));
        last_day = Some(message_day);
      }
      messages_column = messages_column.child(chat_message_row(ctx, message, info.user_id));
    }
  }

  if let Some(error) = lobby.last_error.as_deref() {
    messages_column = messages_column.child(error_notice(ctx, error));
  }

  body = body
    .child(chat_messages_scroll(
      messages_column,
      chat_scroll_state,
      session,
      chat_history,
      channel.id,
      oldest_message_id,
      can_page,
    ))
    .child(chat_composer(ctx, channel, message_input, send_chat));
  body.into()
}

fn chat_messages_scroll(
  messages: Column,
  scroll_state: ScrollState,
  session: ServerSession,
  chat_history: &ChatHistoryAction,
  channel_id: ChannelId,
  before_id: u64,
  can_page: bool,
) -> Element {
  let history = chat_history.clone();
  ScrollVertical::new(messages)
    .width(Dimension::Pct(100.0))
    .flex(1.0)
    .with_scroll_state(scroll_state)
    .scrollbar(chat_scrollbar_style())
    .scrollbar_hovered(|mut style| {
      let palette = theme::palette();
      style.thumb_color = palette.accent_hover;
      style.track_color = palette.surface_input.with_opacity(0.75);
      style
    })
    .on_scroll(move |event| {
      if can_page && event.y <= 48.0 && session.begin_chat_history_request(channel_id) {
        history.run(ChatHistoryRequest { channel_id, before_id });
      }
    })
    .into()
}

fn schedule_chat_scroll_to_bottom(
  channel_id: ChannelId,
  newest_message_id: u64,
  force_bottom: bool,
  scroll_state: ScrollState,
  bottom_anchor: Signal<Option<(ChannelId, u64)>>,
) {
  if newest_message_id == 0 || bottom_anchor.get_untracked() == Some((channel_id, newest_message_id)) {
    return;
  }

  let previous_anchor = bottom_anchor.get_untracked();
  let should_scroll_to_bottom =
    previous_anchor.is_none() || previous_anchor.is_some_and(|(anchor_channel_id, _)| anchor_channel_id != channel_id);
  bottom_anchor.set(Some((channel_id, newest_message_id)));

  if should_scroll_to_bottom || force_bottom {
    scroll_state.scroll_to_bottom_pending();
  } else {
    scroll_state.stick_to_bottom_if_near_end(64.0);
  }
}

fn preserve_chat_scroll_on_prepend(
  channel_id: ChannelId,
  oldest_message_id: u64,
  scroll_state: ScrollState,
  top_anchor: Signal<Option<(ChannelId, u64)>>,
) {
  if oldest_message_id == 0 {
    return;
  }

  if let Some((anchor_channel_id, previous_oldest_message_id)) = top_anchor.get_untracked()
    && anchor_channel_id == channel_id
    && oldest_message_id < previous_oldest_message_id
  {
    scroll_state.preserve_prepend_anchor_pending();
  }

  top_anchor.set(Some((channel_id, oldest_message_id)));
}

fn chat_scrollbar_style() -> ScrollBarStyle {
  let palette = theme::palette();
  ScrollBarStyle {
    width: 8.0,
    min_thumb_length: 32.0,
    track_color: palette.surface_input.with_opacity(0.55),
    thumb_color: palette.accent,
    thumb_radius: 4.0,
    track_radius: 4.0,
    padding: 2.0,
    placement: ScrollBarPlacement::Reserved,
    ..ScrollBarStyle::default()
  }
}

fn selected_text_channel(lobby: &LobbyState) -> Option<&LobbyTextChannel> {
  lobby
    .selected_text_channel_id
    .and_then(|id| lobby.text_channels.iter().find(|channel| channel.id == id))
}

fn unique_lobby_member_count(lobby: &LobbyState) -> usize {
  let mut users = HashSet::new();

  for user in lobby.users_by_channel.values().flatten() {
    users.insert(user.user_id);
  }

  users.len()
}

fn chat_message_row(ctx: &mut Ctx, message: &ProtocolChatMessage, local_user_id: u32) -> Element {
  let local = message.sender_id == local_user_id;
  let timestamp = format_chat_time(message.timestamp);

  Row::new()
    .width(Dimension::Pct(100.0))
    .align_items(Alignment::Start)
    .spacing(theme::SpacingSize::Md)
    .child(server_avatar(&message.sender_name, 36.0, false))
    .child(
      Column::new()
        .width(Dimension::Pct(100.0))
        .flex(1.0)
        .spacing(theme::SpacingSize::Xs)
        .child(
          Row::new()
            .align_items(Alignment::Center)
            .spacing(theme::SpacingSize::Sm)
            .child(
              Text::new(&message.sender_name)
                .variant(theme::TypographyStyle::Button)
                .color(theme::PaletteColor::TextPrimary)
                .selectable(true),
            )
            .child(chat_sender_badge(ctx, local))
            .child(
              Text::new(&timestamp)
                .variant(theme::TypographyStyle::Caption)
                .color(theme::PaletteColor::TextMuted)
                .selectable(true),
            )
            .child(pinned_badge(ctx, message.pinned)),
        )
        .child(chat_message_text(&message.text)),
    )
    .into()
}

#[derive(Clone, Copy)]
struct MessageTextPart<'a> {
  text: &'a str,
  link: bool,
}

#[derive(Clone, Copy)]
struct MessageTextRange {
  start: usize,
  end: usize,
  link: bool,
}

fn chat_message_text(text: &str) -> Element {
  let parts = message_text_parts(text);
  if parts.len() == 1 && !parts[0].link {
    return Text::new(text)
      .variant(theme::TypographyStyle::Description)
      .color(theme::PaletteColor::TextSecondary)
      .width(Dimension::Pct(100.0))
      .selectable(true)
      .into();
  }

  let mut row = Row::new().width(Dimension::Pct(100.0)).wrap().spacing(0.0);

  for part in parts {
    row = row.child(message_text_part(part));
  }

  row.into()
}

fn message_text_part(part: MessageTextPart<'_>) -> Element {
  let color = if part.link {
    theme::PaletteColor::Accent
  } else {
    theme::PaletteColor::TextSecondary
  };
  let text = Text::new(part.text)
    .variant(theme::TypographyStyle::Description)
    .color(color)
    .selectable(true);

  if !part.link {
    return text.into();
  }

  let url = browser_url_for_link(part.text);

  Row::new()
    .align_items(Alignment::Center)
    .border_bottom(Border::inside(1.0, theme::PaletteColor::Accent))
    .cursor(CursorIcon::Pointer)
    .on_click(move |_| open_link_in_browser(&url))
    .child(text)
    .into()
}

fn message_text_parts(text: &str) -> Vec<MessageTextPart<'_>> {
  let mut ranges = Vec::new();
  let mut emitted_until = 0;
  let mut token_start = None;

  for (index, ch) in text.char_indices() {
    if ch.is_whitespace() {
      if let Some(start) = token_start.take() {
        if emitted_until < start {
          push_message_range(&mut ranges, emitted_until, start, false);
        }
        push_message_token_range(text, start, index, &mut ranges);
        emitted_until = index;
      }
    } else if token_start.is_none() {
      token_start = Some(index);
    }
  }

  if let Some(start) = token_start {
    if emitted_until < start {
      push_message_range(&mut ranges, emitted_until, start, false);
    }
    push_message_token_range(text, start, text.len(), &mut ranges);
    emitted_until = text.len();
  }

  if emitted_until < text.len() {
    push_message_range(&mut ranges, emitted_until, text.len(), false);
  }

  if ranges.is_empty() {
    ranges.push(MessageTextRange {
      start: 0,
      end: text.len(),
      link: false,
    });
  }

  ranges
    .into_iter()
    .map(|range| MessageTextPart {
      text: &text[range.start..range.end],
      link: range.link,
    })
    .collect()
}

fn push_message_range(ranges: &mut Vec<MessageTextRange>, start: usize, end: usize, link: bool) {
  if start == end {
    return;
  }

  if !link
    && let Some(last) = ranges.last_mut()
    && !last.link
    && last.end == start
  {
    last.end = end;
    return;
  }

  ranges.push(MessageTextRange { start, end, link });
}

fn push_message_token_range(text: &str, start: usize, end: usize, ranges: &mut Vec<MessageTextRange>) {
  let token = &text[start..end];
  let link_len = trimmed_link_len(token);

  if link_len > 0 && is_link_candidate(&token[..link_len]) {
    push_message_range(ranges, start, start + link_len, true);
    if link_len < token.len() {
      push_message_range(ranges, start + link_len, end, false);
    }
  } else {
    push_message_range(ranges, start, end, false);
  }
}

fn trimmed_link_len(token: &str) -> usize {
  let mut len = token.len();
  while len > 0 {
    let Some(ch) = token[..len].chars().next_back() else {
      break;
    };
    if matches!(ch, '.' | ',' | ';' | ':' | '!' | '?' | ')' | ']' | '}') {
      len -= ch.len_utf8();
    } else {
      break;
    }
  }
  len
}

fn is_link_candidate(token: &str) -> bool {
  if token.starts_with("http://") || token.starts_with("https://") || token.starts_with("www.") {
    return true;
  }

  let Some(dot) = token.rfind('.') else {
    return false;
  };
  if dot == 0 || dot + 1 >= token.len() {
    return false;
  }

  let host_end = token.find('/').unwrap_or(token.len());
  let host = &token[..host_end];
  let Some(tld) = host.rsplit('.').next() else {
    return false;
  };

  host.contains('.')
    && tld.len() >= 2
    && tld.chars().all(|ch| ch.is_ascii_alphabetic())
    && host
      .chars()
      .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-'))
}

fn browser_url_for_link(link: &str) -> String {
  if link.starts_with("http://") || link.starts_with("https://") {
    link.to_owned()
  } else {
    format!("https://{link}")
  }
}

fn open_link_in_browser(url: &str) {
  #[cfg(target_os = "windows")]
  let _ = Command::new("rundll32")
    .arg("url.dll,FileProtocolHandler")
    .arg(url)
    .spawn();

  #[cfg(target_os = "macos")]
  let _ = Command::new("open").arg(url).spawn();

  #[cfg(all(unix, not(target_os = "macos")))]
  let _ = Command::new("xdg-open").arg(url).spawn();
}

fn chat_day_divider(ctx: &mut Ctx, day: NaiveDate, today: NaiveDate) -> Element {
  Row::new()
    .width(Dimension::Pct(100.0))
    .align_items(Alignment::Center)
    .spacing(theme::SpacingSize::Md)
    .padding_vertical(theme::SpacingSize::Sm)
    .child(day_divider_line())
    .child(
      Text::new(&format_chat_day(ctx, day, today))
        .variant(theme::TypographyStyle::Caption)
        .color(theme::PaletteColor::TextMuted)
        .selectable(true),
    )
    .child(day_divider_line())
    .into()
}

fn day_divider_line() -> Element {
  Row::new()
    .height(1.0)
    .flex(1.0)
    .background(BackgroundColor::Palette(theme::PaletteColor::Border))
    .into()
}

fn format_chat_time(timestamp: u64) -> String {
  let datetime = local_chat_datetime(timestamp);
  format!("{:02}:{:02}", datetime.hour(), datetime.minute())
}

fn format_chat_day(ctx: &mut Ctx, day: NaiveDate, today: NaiveDate) -> String {
  if day == today {
    return ctx.t("date.today").to_string();
  }

  let weekday = ctx.t(weekday_key(day.weekday()));
  let month = ctx.t(month_key(day.month()));
  let day_of_month = day.day().to_string();

  if day.year() == today.year() {
    ctx
      .t_args(
        "date.current_year",
        [
          ("weekday", weekday.to_string()),
          ("month", month.to_string()),
          ("day", day_of_month),
        ],
      )
      .to_string()
  } else {
    ctx
      .t_args(
        "date.other_year",
        [
          ("weekday", weekday.to_string()),
          ("month", month.to_string()),
          ("day", day_of_month),
          ("year", day.year().to_string()),
        ],
      )
      .to_string()
  }
}

fn local_chat_date(timestamp: u64) -> NaiveDate {
  local_chat_datetime(timestamp).date_naive()
}

fn local_chat_datetime(timestamp: u64) -> DateTime<Local> {
  let seconds = if timestamp > 10_000_000_000 {
    (timestamp / 1000) as i64
  } else {
    timestamp as i64
  };
  let millis = if timestamp > 10_000_000_000 {
    (timestamp % 1000) as u32
  } else {
    0
  };

  Local
    .timestamp_opt(seconds, millis * 1_000_000)
    .single()
    .unwrap_or_else(Local::now)
}

fn weekday_key(weekday: Weekday) -> &'static str {
  match weekday {
    Weekday::Mon => "date.weekday.monday",
    Weekday::Tue => "date.weekday.tuesday",
    Weekday::Wed => "date.weekday.wednesday",
    Weekday::Thu => "date.weekday.thursday",
    Weekday::Fri => "date.weekday.friday",
    Weekday::Sat => "date.weekday.saturday",
    Weekday::Sun => "date.weekday.sunday",
  }
}

fn month_key(month: u32) -> &'static str {
  match month {
    1 => "date.month.january",
    2 => "date.month.february",
    3 => "date.month.march",
    4 => "date.month.april",
    5 => "date.month.may",
    6 => "date.month.june",
    7 => "date.month.july",
    8 => "date.month.august",
    9 => "date.month.september",
    10 => "date.month.october",
    11 => "date.month.november",
    12 => "date.month.december",
    _ => "date.month.january",
  }
}

fn chat_sender_badge(ctx: &mut Ctx, local: bool) -> Element {
  if !local {
    return Row::new().into();
  }

  Text::new(&ctx.t("lobby.users.you"))
    .variant(theme::TypographyStyle::Caption)
    .color(theme::PaletteColor::TextMuted)
    .into()
}

fn pinned_badge(ctx: &mut Ctx, pinned: bool) -> Element {
  if !pinned {
    return Row::new().into();
  }

  Row::new()
    .align_items(Alignment::Center)
    .spacing(4.0)
    .padding_vertical(3.0)
    .padding_horizontal(6.0)
    .rounded(theme::RadiusSize::Sm)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised))
    .child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon: "pin",
      size: 11.0,
      color: theme::palette().text_muted,
    }))
    .child(
      Text::new(&ctx.t("lobby.text_channel.pinned"))
        .variant(theme::TypographyStyle::FieldLabel)
        .color(theme::PaletteColor::TextMuted),
    )
    .into()
}

fn chat_composer(
  ctx: &mut Ctx,
  channel: &LobbyTextChannel,
  message_input: Signal<String>,
  send_chat: &SendChatAction,
) -> Element {
  let text_style = ctx.theme().typography().description.clone();
  let mut placeholder_style = text_style.clone();
  placeholder_style.color = theme::palette().text_muted.with_opacity(0.65);
  let placeholder = ctx.t_args(
    "lobby.text_channel.composer_placeholder",
    [("channel", channel.name.clone())],
  );
  let channel_id = channel.id;
  let key_value = message_input.clone();
  let key_action = send_chat.clone();
  let click_value = message_input.clone();
  let click_action = send_chat.clone();

  Row::new()
    .width(Dimension::Pct(100.0))
    .padding_left(24.0)
    .padding_right(24.0)
    .padding_bottom(theme::SpacingSize::Xl)
    .child(
      Row::new()
        .width(Dimension::Pct(100.0))
        .height(64.0)
        .align_items(Alignment::Center)
        .spacing(theme::SpacingSize::Md)
        .padding_vertical(8.0)
        .padding_left(theme::SpacingSize::Lg)
        .padding_right(theme::SpacingSize::Sm)
        .rounded(theme::RadiusSize::Lg)
        .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceInput))
        .border_inside(1.0, theme::PaletteColor::Border)
        .child(
          TextInput::styled(message_input, text_style)
            .placeholder(&placeholder)
            .placeholder_style(placeholder_style)
            .multiline()
            .name("lobby-chat-message")
            .height(Dimension::Pct(100.0))
            .flex(1.0)
            .background(BackgroundColor::Color(Color::from_hex("#00000000")))
            .caret_color(theme::PaletteColor::Accent)
            .on_key_down(move |event| {
              if event.key == "Enter" && !event.shift {
                submit_chat(channel_id, &key_value, &key_action);
              }
            }),
        )
        .child(
          Row::new()
            .width(32.0)
            .height(32.0)
            .align_items(Alignment::Center)
            .justify(Justify::Center)
            .rounded(theme::RadiusSize::Md)
            .background(BackgroundColor::Palette(theme::PaletteColor::Accent))
            .cursor(CursorIcon::Pointer)
            .hovered_style(Style::new().background(BackgroundColor::Palette(theme::PaletteColor::AccentHover)))
            .on_click(move |_| submit_chat(channel_id, &click_value, &click_action))
            .child(ctx.mount::<LucideIcon>(LucideIconProps {
              icon: "send-horizontal",
              size: 15.0,
              color: theme::palette().text_inverse,
            })),
        ),
    )
    .into()
}

fn submit_chat(channel_id: ChannelId, message_input: &Signal<String>, send_chat: &SendChatAction) {
  let text = message_input.get_untracked();
  let text = text.trim();
  if text.is_empty() {
    return;
  }

  send_chat.run(SendChatInput {
    channel_id,
    text: text.to_owned(),
  });
  message_input.set(String::new());
}

fn empty_voice_state(ctx: &mut Ctx, error: Option<&str>) -> Element {
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
        .width(480.0)
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

fn channel_detail(ctx: &mut Ctx, channel: &LobbyChannel, info: &ConnectedServerInfo, lobby: &LobbyState) -> Element {
  let mut users = Column::new().width(520.0).spacing(theme::SpacingSize::Sm);

  if lobby.users.is_empty() {
    users = users.child(
      Text::new(&ctx.t("lobby.users.empty"))
        .variant(theme::TypographyStyle::Description)
        .color(theme::PaletteColor::TextMuted),
    );
  } else {
    for user in &lobby.users {
      users = users.child(user_row(ctx, user, info.user_id));
    }
  }

  let mut body = Column::new()
    .width(Dimension::Pct(100.0))
    .flex(1.0)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .spacing(theme::SpacingSize::Xl)
    .child(
      Column::new()
        .width(520.0)
        .spacing(theme::SpacingSize::Sm)
        .child(Text::new(&channel.name).variant(theme::TypographyStyle::Title))
        .child(
          Text::new(&ctx.t("lobby.channel.description"))
            .variant(theme::TypographyStyle::Description)
            .color(theme::PaletteColor::TextMuted),
        ),
    )
    .child(users);

  if let Some(error) = lobby.last_error.as_deref() {
    body = body.child(error_notice(ctx, error));
  }

  body.into()
}

fn user_row(ctx: &mut Ctx, user: &LobbyUser, local_user_id: u32) -> Element {
  let local = user.user_id == local_user_id;

  Row::new()
    .width(Dimension::Pct(100.0))
    .align_items(Alignment::Center)
    .justify(Justify::SpaceBetween)
    .padding_vertical(8.0)
    .padding_horizontal(10.0)
    .rounded(theme::RadiusSize::Lg)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised))
    .border_inside(1.0, theme::PaletteColor::Border)
    .child(
      Row::new()
        .align_items(Alignment::Center)
        .spacing(theme::SpacingSize::Md)
        .child(server_avatar(&user.username, 32.0, false))
        .child(
          Column::new()
            .spacing(theme::SpacingSize::Xs)
            .child(user_name(ctx, &user.username, local))
            .child(
              Text::new(role_label_lower(user.role))
                .variant(theme::TypographyStyle::Caption)
                .color(theme::PaletteColor::TextMuted),
            ),
        ),
    )
    .child(voice_state(ctx, user))
    .into()
}

fn user_name(ctx: &mut Ctx, username: &str, local: bool) -> Element {
  let name = Text::new(username)
    .variant(theme::TypographyStyle::Description)
    .color(theme::PaletteColor::TextPrimary);

  if !local {
    return name.into();
  }

  Row::new()
    .align_items(Alignment::Center)
    .spacing(theme::SpacingSize::Xs)
    .child(name)
    .child(
      Text::new(&ctx.t("lobby.users.you"))
        .variant(theme::TypographyStyle::Caption)
        .color(theme::PaletteColor::TextMuted),
    )
    .into()
}

fn voice_state(ctx: &mut Ctx, user: &LobbyUser) -> Element {
  let speaking = user.speaking && !user.muted && !user.deafened;
  let icon = if user.deafened {
    "headphone-off"
  } else if user.muted {
    "mic-off"
  } else {
    "mic"
  };
  let label = if user.deafened {
    ctx.t("lobby.voice.deafened")
  } else if user.muted {
    ctx.t("lobby.voice.muted")
  } else {
    ctx.t("lobby.voice.live")
  };
  let color = if user.deafened || user.muted {
    theme::palette().danger
  } else if speaking {
    theme::palette().success
  } else {
    theme::palette().text_muted
  };

  Row::new()
    .align_items(Alignment::Center)
    .spacing(theme::SpacingSize::Xs)
    .padding_vertical(5.0)
    .padding_horizontal(8.0)
    .rounded(theme::RadiusSize::Md)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceInput))
    .child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon,
      size: 13.0,
      color,
    }))
    .child(
      Text::new(&label)
        .variant(theme::TypographyStyle::Caption)
        .color(if user.deafened || user.muted {
          theme::PaletteColor::Danger
        } else if speaking {
          theme::PaletteColor::Success
        } else {
          theme::PaletteColor::TextMuted
        }),
    )
    .into()
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

fn error_notice(ctx: &mut Ctx, message: &str) -> Element {
  Row::new()
    .width(480.0)
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

fn empty_lobby(ctx: &mut Ctx) -> Element {
  Column::new()
    .width(Dimension::Pct(100.0))
    .height(Dimension::Pct(100.0))
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceBase))
    .child(loader(18.0))
    .child(
      Text::new(&ctx.t("lobby.user.disconnected"))
        .variant(theme::TypographyStyle::Description)
        .color(theme::PaletteColor::TextMuted),
    )
    .into()
}
