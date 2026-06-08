use std::process::Command;

use chrono::{DateTime, Datelike, Local, NaiveDate, TimeZone, Timelike, Weekday};
use lurq::{
  app::ctx::Ctx,
  components::{Column, Row, ScrollVertical, Text, TextInput},
  core::Signal,
  layout::{
    Alignment,
    layout_kind::{Justify, ScrollState},
    scrollbar::{ScrollBarPlacement, ScrollBarStyle},
  },
  node::{BackgroundColor, CursorIcon, Element, Style, border::Border, color::Color, dimension::Dimension},
};

use super::{
  ChatHistoryAction, ChatHistoryRequest, SendChatAction, SendChatInput, rail::server_avatar, shared::error_notice,
};
use crate::{
  network::protocol::{ChannelId, control::ChatMessage as ProtocolChatMessage},
  session::{ConnectedServerInfo, LobbyState, LobbyTextChannel, ServerSession},
  theme,
  ui::common::lucide_icon::{LucideIcon, LucideIconProps},
};

pub(super) fn text_channel_detail(
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
