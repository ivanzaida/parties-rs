use std::{process::Command, sync::Arc, time::Duration};

use chrono::{DateTime, Datelike, Local, NaiveDate, TimeZone, Timelike, Weekday};
use lurq::{
  animation::Transition,
  app::ctx::{CollisionStrategy, Ctx, Overlay, Placement, Timeout},
  components::{Column, Row, ScrollVertical, Text, TextInput},
  core::{ElementRef, Signal},
  layout::{
    Alignment,
    layout_kind::{Justify, ScrollState},
    scrollbar::{ScrollBarPlacement, ScrollBarStyle},
  },
  node::{
    BackgroundColor, CursorIcon, Element, HitTestBehavior, Style, border::Border, color::Color, dimension::Dimension,
    transform::Transform2D,
  },
};

use super::{
  ChatHistoryAction, ChatHistoryRequest, SendChatAction, SendChatInput,
  rail::server_avatar,
  shared::{error_notice, user_display_name},
};
use crate::{
  network::protocol::{ChannelId, control::ChatMessage as ProtocolChatMessage},
  session::{
    ConnectedServerInfo, DEBUG_CHAT_CHANNEL_ID, LobbyState, LobbyTextChannel, ServerSession,
    chat_commands::{ChatCommandRegistry, CommandDefinition},
  },
  theme,
  ui::{
    common::lucide_icon::{LucideIcon, LucideIconProps},
    loader::loader,
  },
};

const CHAT_COMMAND_SUGGESTION_BOTTOM_GAP: f32 = 6.0;
const CHAT_COMMAND_SUGGESTION_ROW_HEIGHT: f32 = 68.0;
const CHAT_COMMAND_SUGGESTION_TITLE_HEIGHT: f32 = 32.0;
const CHAT_COMMAND_INVALID_SHAKE_STEP_MS: u64 = 28;
const CHAT_COMMAND_INVALID_SHAKE_TRANSITION_MS: u64 = 20;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ChatChannelKind {
  ServerText,
  Debug,
}

#[derive(Clone, Debug)]
pub(super) struct ChatChannel {
  id: ChannelId,
  name: Arc<str>,
  topic: Arc<str>,
  icon: &'static str,
  kind: ChatChannelKind,
  command_registry: ChatCommandRegistry,
}

impl ChatChannel {
  pub(super) fn server_text(ctx: &mut Ctx, channel: &LobbyTextChannel) -> Self {
    Self {
      id: channel.id,
      name: Arc::from(channel.name.as_str()),
      topic: ctx.t("lobby.text_channel.topic"),
      icon: "hash",
      kind: ChatChannelKind::ServerText,
      command_registry: ChatCommandRegistry::empty(),
    }
  }

  pub(super) fn debug(ctx: &mut Ctx) -> Self {
    Self {
      id: DEBUG_CHAT_CHANNEL_ID,
      name: ctx.t("lobby.debug_channels.chat"),
      topic: ctx.t("lobby.debug_channels.topic"),
      icon: "terminal",
      kind: ChatChannelKind::Debug,
      command_registry: ChatCommandRegistry::from_definitions([
        CommandDefinition {
          name: "/restart-audio-receiver".into(),
          description_key: "lobby.text_channel.commands.description.restart_audio_receiver".into(),
          usage: "/restart-audio-receiver {userId:u32}".into(),
        },
        CommandDefinition {
          name: "/debug-user".into(),
          description_key: "lobby.text_channel.commands.description.debug_user".into(),
          usage: "/debug-user {userId:u32}".into(),
        },
        CommandDefinition {
          name: "/debug-voice".into(),
          description_key: "lobby.text_channel.commands.description.debug_voice".into(),
          usage: "/debug-voice {userId:u32}".into(),
        },
        CommandDefinition {
          name: "/debug-my-voice".into(),
          description_key: "lobby.text_channel.commands.description.debug_my_voice".into(),
          usage: "/debug-my-voice".into(),
        },
        CommandDefinition {
          name: "/debug-stream".into(),
          description_key: "lobby.text_channel.commands.description.debug_stream".into(),
          usage: "/debug-stream {userId:u32}".into(),
        },
        CommandDefinition {
          name: "/debug-my-stream".into(),
          description_key: "lobby.text_channel.commands.description.debug_my_stream".into(),
          usage: "/debug-my-stream".into(),
        },
        CommandDefinition {
          name: "/debug-channel".into(),
          description_key: "lobby.text_channel.commands.description.debug_channel".into(),
          usage: "/debug-channel".into(),
        },
        CommandDefinition {
          name: "/debug-audio-receivers".into(),
          description_key: "lobby.text_channel.commands.description.debug_audio_receivers".into(),
          usage: "/debug-audio-receivers".into(),
        },
        CommandDefinition {
          name: "/debug-video-receivers".into(),
          description_key: "lobby.text_channel.commands.description.debug_video_receivers".into(),
          usage: "/debug-video-receivers".into(),
        },
        CommandDefinition {
          name: "/audio-status".into(),
          description_key: "lobby.text_channel.commands.description.audio_status".into(),
          usage: "/audio-status".into(),
        },
        CommandDefinition {
          name: "/audio-reset-all".into(),
          description_key: "lobby.text_channel.commands.description.audio_reset_all".into(),
          usage: "/audio-reset-all".into(),
        },
        CommandDefinition {
          name: "/audio-clear-queue".into(),
          description_key: "lobby.text_channel.commands.description.audio_clear_queue".into(),
          usage: "/audio-clear-queue {userId:u32}".into(),
        },
      ]),
    }
  }

  pub(super) fn id(&self) -> ChannelId {
    self.id
  }

  pub(super) fn name(&self) -> &str {
    &self.name
  }

  pub(super) fn topic(&self) -> &str {
    &self.topic
  }

  pub(super) fn icon(&self) -> &'static str {
    self.icon
  }

  pub(super) fn command_registry(&self) -> ChatCommandRegistry {
    self.command_registry.clone()
  }

  pub(super) fn server_channel_id(&self) -> Option<ChannelId> {
    self.is_server_backed().then_some(self.id)
  }

  pub(super) fn is_server_backed(&self) -> bool {
    self.kind == ChatChannelKind::ServerText
  }

  pub(super) fn shows_text_tools(&self) -> bool {
    self.kind == ChatChannelKind::ServerText
  }

  fn empty_title_key(&self) -> &'static str {
    match self.kind {
      ChatChannelKind::ServerText => "lobby.text_channel.empty.title",
      ChatChannelKind::Debug => "lobby.debug_channels.empty.title",
    }
  }

  fn empty_description_key(&self) -> &'static str {
    match self.kind {
      ChatChannelKind::ServerText => "lobby.text_channel.empty.description",
      ChatChannelKind::Debug => "lobby.debug_channels.empty.description",
    }
  }
}

#[derive(Clone)]
pub(super) struct ChatCommandInvalidFeedback {
  phase: Signal<u8>,
  step_two: Timeout,
  step_three: Timeout,
  step_four: Timeout,
  step_five: Timeout,
  reset: Timeout,
}

impl ChatCommandInvalidFeedback {
  pub(super) fn new(ctx: &mut Ctx) -> Self {
    let phase = ctx.signal(0_u8);
    let step_two_phase = phase.clone();
    let step_three_phase = phase.clone();
    let step_four_phase = phase.clone();
    let step_five_phase = phase.clone();
    let reset_phase = phase.clone();

    Self {
      phase,
      step_two: ctx.create_timeout(Duration::from_millis(CHAT_COMMAND_INVALID_SHAKE_STEP_MS), move || {
        step_two_phase.set(2);
      }),
      step_three: ctx.create_timeout(
        Duration::from_millis(CHAT_COMMAND_INVALID_SHAKE_STEP_MS * 2),
        move || {
          step_three_phase.set(3);
        },
      ),
      step_four: ctx.create_timeout(
        Duration::from_millis(CHAT_COMMAND_INVALID_SHAKE_STEP_MS * 3),
        move || {
          step_four_phase.set(4);
        },
      ),
      step_five: ctx.create_timeout(
        Duration::from_millis(CHAT_COMMAND_INVALID_SHAKE_STEP_MS * 4),
        move || {
          step_five_phase.set(5);
        },
      ),
      reset: ctx.create_timeout(
        Duration::from_millis(CHAT_COMMAND_INVALID_SHAKE_STEP_MS * 5),
        move || {
          reset_phase.set(0);
        },
      ),
    }
  }

  fn phase(&self) -> u8 {
    self.phase.get()
  }

  fn trigger(&self) {
    self.phase.set(1);
    self.step_two.restart();
    self.step_three.restart();
    self.step_four.restart();
    self.step_five.restart();
    self.reset.restart();
  }
}

pub(super) fn text_channel_detail(
  ctx: &mut Ctx,
  channel: ChatChannel,
  info: &ConnectedServerInfo,
  lobby: &LobbyState,
  message_input: Signal<String>,
  command_selected_index: Signal<usize>,
  command_scroll_state: ScrollState,
  command_invalid_feedback: ChatCommandInvalidFeedback,
  chat_scroll_state: ScrollState,
  chat_bottom_anchor: Signal<Option<(ChannelId, u64)>>,
  chat_top_anchor: Signal<Option<(ChannelId, u64)>>,
  debug_user_ids: bool,
  session: ServerSession,
  chat_history: &ChatHistoryAction,
  send_chat: &SendChatAction,
) -> Element {
  let channel_id = channel.id();
  let command_registry = channel.command_registry();
  let commands_enabled = command_registry.has_commands();
  let messages = if channel.is_server_backed() {
    lobby
      .chat_messages_by_channel
      .get(&channel_id)
      .cloned()
      .unwrap_or_default()
  } else {
    lobby.debug_chat_messages.clone()
  };
  let oldest_message_id = messages.first().map(|message| message.id).unwrap_or(0);
  let newest_message_id = messages.last().map(|message| message.id).unwrap_or(0);
  let newest_message_from_local = messages.last().is_some_and(|message| message.sender_id == info.user_id);
  let initial_history_loading = messages.is_empty()
    && lobby.chat_history_loading.contains(&channel_id)
    && lobby.chat_history_has_more.get(&channel_id).copied().unwrap_or(true);
  let can_page = channel.is_server_backed()
    && oldest_message_id != 0
    && lobby.chat_history_has_more.get(&channel_id).copied().unwrap_or(true)
    && !lobby.chat_history_loading.contains(&channel_id);
  preserve_chat_scroll_on_prepend(
    channel_id,
    oldest_message_id,
    chat_scroll_state.clone(),
    chat_top_anchor,
  );
  schedule_chat_scroll_to_bottom(
    channel_id,
    newest_message_id,
    newest_message_from_local,
    chat_scroll_state.clone(),
    chat_bottom_anchor,
  );
  let mut messages_column = Column::new()
    .width(Dimension::Pct(100.0))
    .spacing(18.0)
    .padding_vertical(theme::SpacingSize::Xl)
    .padding_horizontal(24.0);

  if initial_history_loading {
    messages_column = messages_column.child(
      Column::new()
        .width(Dimension::Pct(100.0))
        .flex(1.0)
        .align_items(Alignment::Center)
        .justify(Justify::Center)
        .child(loader(18.0)),
    );
  } else if messages.is_empty() {
    messages_column = messages_column.child(
      Column::new()
        .width(Dimension::Pct(100.0))
        .flex(1.0)
        .align_items(Alignment::Center)
        .justify(Justify::Center)
        .spacing(theme::SpacingSize::Sm)
        .child(
          Text::new(&ctx.t(channel.empty_title_key()))
            .variant(theme::TypographyStyle::Title)
            .color(theme::PaletteColor::TextPrimary),
        )
        .child(
          Text::new(&ctx.t(channel.empty_description_key()))
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
      messages_column = messages_column.child(chat_message_row(ctx, message, info.user_id, debug_user_ids));
    }
  }

  if let Some(error) = lobby.last_error.as_deref() {
    messages_column = messages_column.child(error_notice(ctx, error));
  }

  let composer_ref = ctx.element_ref();
  let mut body = Column::new()
    .width(Dimension::Pct(100.0))
    .height(Dimension::Pct(100.0))
    .flex(1.0)
    .child(chat_messages_scroll(
      messages_column,
      chat_scroll_state,
      session,
      chat_history,
      channel_id,
      oldest_message_id,
      can_page,
    ))
    .child(chat_composer(
      ctx,
      &channel,
      message_input.clone(),
      command_selected_index.clone(),
      command_invalid_feedback.clone(),
      command_registry.clone(),
      send_chat,
      composer_ref.clone(),
    ));

  if commands_enabled {
    if let Some(suggestions) = chat_command_suggestions(
      ctx,
      message_input,
      command_selected_index,
      &command_registry,
      command_scroll_state,
      command_invalid_feedback,
    ) {
      body = body.child(
        Overlay::new(suggestions)
          .anchor(composer_ref)
          .placement(Placement::TopStart)
          .offset(0.0, CHAT_COMMAND_SUGGESTION_BOTTOM_GAP)
          .match_anchor_width(true)
          .collision(CollisionStrategy::Clamp)
          .hit_test(HitTestBehavior::ContentOnly),
      );
    }
  }

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
    .height(Dimension::Pct(100.0))
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

fn chat_message_row(ctx: &mut Ctx, message: &ProtocolChatMessage, local_user_id: u32, debug_user_ids: bool) -> Element {
  let local = message.sender_id == local_user_id;
  let timestamp = format_chat_time(message.timestamp);
  let sender_name = user_display_name(message.sender_id, &message.sender_name, debug_user_ids);

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
              Text::new(&sender_name)
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
  channel: &ChatChannel,
  message_input: Signal<String>,
  command_selected_index: Signal<usize>,
  command_invalid_feedback: ChatCommandInvalidFeedback,
  command_registry: ChatCommandRegistry,
  send_chat: &SendChatAction,
  composer_ref: ElementRef,
) -> Element {
  let text_style = ctx.theme().typography().description.clone();
  let mut placeholder_style = text_style.clone();
  placeholder_style.color = theme::palette().text_muted.with_opacity(0.65);
  let placeholder = ctx.t_args(
    "lobby.text_channel.composer_placeholder",
    [("channel", channel.name().to_owned())],
  );
  let channel_id = channel.server_channel_id();
  let key_value = message_input.clone();
  let key_command_selected_index = command_selected_index.clone();
  let key_invalid_feedback = command_invalid_feedback.clone();
  let key_command_registry = command_registry.clone();
  let key_action = send_chat.clone();
  let click_value = message_input.clone();
  let click_command_selected_index = command_selected_index.clone();
  let click_invalid_feedback = command_invalid_feedback.clone();
  let click_command_registry = command_registry.clone();
  let click_action = send_chat.clone();

  Row::new()
    .width(Dimension::Pct(100.0))
    .ref_element(composer_ref)
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
              if handle_chat_command_navigation(
                &key_command_registry,
                &key_value,
                &key_command_selected_index,
                &event.key,
                &event.code,
              ) {
                event.prevent_default();
                return;
              }
              if event.key == "Enter" && !event.shift {
                event.prevent_default();
                submit_chat_if_valid(
                  channel_id,
                  &key_value,
                  &key_command_selected_index,
                  &key_invalid_feedback,
                  &key_command_registry,
                  &key_action,
                );
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
            .on_click(move |_| {
              submit_chat_if_valid(
                channel_id,
                &click_value,
                &click_command_selected_index,
                &click_invalid_feedback,
                &click_command_registry,
                &click_action,
              )
            })
            .child(ctx.mount::<LucideIcon>(LucideIconProps {
              icon: "send-horizontal",
              size: 15.0,
              color: theme::palette().text_inverse,
            })),
        ),
    )
    .into()
}

fn handle_chat_command_navigation(
  command_registry: &ChatCommandRegistry,
  message_input: &Signal<String>,
  selected_index: &Signal<usize>,
  key: &str,
  code: &str,
) -> bool {
  if !command_registry.has_commands() {
    return false;
  }

  if matches!((key, code), ("Tab", _) | (_, "Tab")) {
    let input = message_input.get_untracked();
    let Some(query) = command_suggestion_query(&input) else {
      selected_index.set(0);
      return false;
    };
    let commands = matching_chat_command_definitions(command_registry, &query);
    let Some(command) = commands.get(selected_index.get_untracked().min(commands.len().saturating_sub(1))) else {
      selected_index.set(0);
      return false;
    };
    message_input.set(command_fill_text(command.name.as_ref()));
    return true;
  }

  let direction = match (key, code) {
    ("ArrowDown", _) | (_, "ArrowDown") => 1,
    ("ArrowUp", _) | (_, "ArrowUp") => -1,
    _ => return false,
  };
  let input = message_input.get_untracked();
  let Some(query) = command_suggestion_query(&input) else {
    selected_index.set(0);
    return false;
  };
  let count = matching_chat_command_definitions(command_registry, &query).len();
  if count == 0 {
    selected_index.set(0);
    return false;
  }

  let current = selected_index.get_untracked().min(count - 1);
  let next = if direction > 0 {
    (current + 1) % count
  } else {
    current.checked_sub(1).unwrap_or(count - 1)
  };
  selected_index.set(next);
  true
}

fn command_suggestion_query(input: &str) -> Option<String> {
  let trimmed = input.trim_start();
  if !trimmed.starts_with('/') {
    return None;
  }
  Some(
    trimmed
      .split_whitespace()
      .next()
      .unwrap_or(trimmed)
      .to_ascii_lowercase(),
  )
}

fn matching_chat_command_definitions<'a>(
  command_registry: &'a ChatCommandRegistry,
  query: &str,
) -> Vec<&'a CommandDefinition> {
  command_registry
    .definitions()
    .iter()
    .filter(|command| command.name.to_ascii_lowercase().starts_with(query))
    .collect()
}

fn exact_chat_command_definition<'a>(
  command_registry: &'a ChatCommandRegistry,
  input: &str,
) -> Option<&'a CommandDefinition> {
  let command_name = input.trim_start().split_whitespace().next()?;
  command_registry
    .definitions()
    .iter()
    .find(|command| command.name.as_ref() == command_name)
}

fn chat_command_suggestions(
  ctx: &mut Ctx,
  message_input: Signal<String>,
  selected_index: Signal<usize>,
  command_registry: &ChatCommandRegistry,
  scroll_state: ScrollState,
  invalid_feedback: ChatCommandInvalidFeedback,
) -> Option<Element> {
  let input = message_input.get();
  let query = command_suggestion_query(&input)?;
  let commands = matching_chat_command_definitions(command_registry, &query);
  if commands.is_empty() {
    selected_index.set(0);
    return None;
  }

  let active_index = selected_index.get().min(commands.len().saturating_sub(1));
  if active_index != selected_index.get_untracked() {
    selected_index.set(active_index);
  }

  let list_height = command_suggestion_list_height(commands.len());
  ensure_command_selection_visible(&scroll_state, active_index, list_height);
  let invalid_feedback_phase = invalid_feedback.phase();
  let exact_command_name = exact_chat_command_definition(command_registry, &input).map(|command| command.name.as_ref());
  let suggestions_height = CHAT_COMMAND_SUGGESTION_TITLE_HEIGHT + list_height + CHAT_COMMAND_SUGGESTION_BOTTOM_GAP;

  let title = if query == "/" {
    ctx.t("lobby.text_channel.commands.title").to_string()
  } else {
    ctx
      .t_args(
        "lobby.text_channel.commands.matching",
        [("query", query.to_ascii_uppercase())],
      )
      .to_string()
  };

  let mut list = Column::new().width(Dimension::Pct(100.0)).padding_bottom(6.0);

  for (index, command) in commands.into_iter().enumerate() {
    let validate_arguments = invalid_feedback_phase != 0 && exact_command_name == Some(command.name.as_ref());
    list = list.child(command_suggestion_row(
      ctx,
      command,
      input.clone(),
      message_input.clone(),
      selected_index.clone(),
      index,
      index == active_index,
      invalid_feedback_phase,
      validate_arguments,
    ));
  }

  Some(
    Column::new()
      .width(Dimension::Pct(100.0))
      .height(suggestions_height)
      .padding_left(24.0)
      .padding_right(24.0)
      .padding_bottom(CHAT_COMMAND_SUGGESTION_BOTTOM_GAP)
      .child(
        Column::new()
          .width(Dimension::Pct(100.0))
          .rounded(theme::RadiusSize::Lg)
          .clip()
          .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised))
          .border_inside(1.0, theme::PaletteColor::Border)
          .child(command_suggestion_title(&title))
          .child(
            ScrollVertical::new(list)
              .width(Dimension::Pct(100.0))
              .height(list_height)
              .with_scroll_state(scroll_state)
              .scrollbar(chat_scrollbar_style()),
          ),
      )
      .into(),
  )
}

fn command_suggestion_list_height(command_count: usize) -> f32 {
  let visible_rows = command_count.min(7) as f32;
  (visible_rows * CHAT_COMMAND_SUGGESTION_ROW_HEIGHT + 6.0).clamp(
    CHAT_COMMAND_SUGGESTION_ROW_HEIGHT + 6.0,
    CHAT_COMMAND_SUGGESTION_ROW_HEIGHT * 7.0 + 6.0,
  )
}

fn ensure_command_selection_visible(scroll_state: &ScrollState, active_index: usize, fallback_viewport_height: f32) {
  let viewport_height = scroll_state.viewport_height().max(fallback_viewport_height);
  let current_scroll = scroll_state.scroll_y();
  let row_top = active_index as f32 * CHAT_COMMAND_SUGGESTION_ROW_HEIGHT;
  let row_bottom = row_top + CHAT_COMMAND_SUGGESTION_ROW_HEIGHT;
  let viewport_bottom = current_scroll + viewport_height;
  let next_scroll = if row_top < current_scroll {
    row_top
  } else if row_bottom > viewport_bottom {
    row_bottom - viewport_height
  } else {
    return;
  };

  scroll_state.set_scroll_pending(scroll_state.scroll_x(), next_scroll);
}

fn command_suggestion_row(
  ctx: &mut Ctx,
  command: &CommandDefinition,
  input: String,
  message_input: Signal<String>,
  selected_index: Signal<usize>,
  index: usize,
  selected: bool,
  invalid_feedback_phase: u8,
  validate_arguments: bool,
) -> Element {
  let fill = command_fill_text(command.name.as_ref());
  let background = if selected {
    BackgroundColor::Color(Color::from_hex("#232830"))
  } else {
    BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised)
  };

  Row::new()
    .width(Dimension::Pct(100.0))
    .height(CHAT_COMMAND_SUGGESTION_ROW_HEIGHT)
    .align_items(Alignment::Center)
    .spacing(theme::SpacingSize::Lg)
    .padding_vertical(10.0)
    .padding_horizontal(theme::SpacingSize::Lg)
    .background(background)
    .transition(Transition::background_color().duration_ms(120))
    .cursor(CursorIcon::Pointer)
    .hovered_style(Style::new().background(BackgroundColor::Color(Color::from_hex("#2B313A"))))
    .on_mouse_enter(move || selected_index.set(index))
    .on_click(move |_| message_input.set(fill.clone()))
    .child(command_icon(ctx))
    .child(
      Column::new()
        .width(Dimension::Pct(100.0))
        .flex(1.0)
        .spacing(theme::SpacingSize::Xs)
        .child(command_usage_row(
          command_usage_parts(command.usage.as_ref(), &input, validate_arguments),
          invalid_feedback_phase,
        ))
        .child(
          Text::new(&ctx.t(&command.description_key))
            .variant(theme::TypographyStyle::Link)
            .color(theme::PaletteColor::TextSecondary),
        ),
    )
    .into()
}

fn command_icon(ctx: &mut Ctx) -> Element {
  Row::new()
    .width(28.0)
    .height(28.0)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .rounded(14.0)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfacePanel))
    .border_inside(1.0, theme::PaletteColor::BorderStrong)
    .child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon: "terminal",
      size: 14.0,
      color: theme::palette().text_secondary,
    }))
    .into()
}

fn command_suggestion_title(title: &str) -> Element {
  Row::new()
    .width(Dimension::Pct(100.0))
    .height(CHAT_COMMAND_SUGGESTION_TITLE_HEIGHT)
    .align_items(Alignment::Center)
    .padding_horizontal(theme::SpacingSize::Lg)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised))
    .child(
      Text::new(title)
        .variant(theme::TypographyStyle::Caption)
        .color(theme::PaletteColor::TextMuted),
    )
    .into()
}

fn command_usage_row(parts: Vec<CommandUsagePart>, invalid_feedback_phase: u8) -> Element {
  let mut row = Row::new()
    .width(Dimension::Pct(100.0))
    .align_items(Alignment::Center)
    .spacing(theme::SpacingSize::Sm)
    .wrap();

  for part in parts {
    let child: Element = match part {
      CommandUsagePart::Name(name) => Text::new(name.as_ref())
        .variant(theme::TypographyStyle::Heading)
        .color(theme::PaletteColor::TextPrimary)
        .into(),
      CommandUsagePart::Argument { label, invalid } => {
        command_argument_pill(label.as_ref(), invalid, invalid_feedback_phase)
      }
    };
    row = row.child(child);
  }

  row.into()
}

enum CommandUsagePart {
  Name(Arc<str>),
  Argument { label: Arc<str>, invalid: bool },
}

fn command_usage_parts(usage: &str, input: &str, validate_missing: bool) -> Vec<CommandUsagePart> {
  let input_args = command_preview_input_args(input);
  let mut argument_index = 0usize;
  usage
    .split_whitespace()
    .map(|part| {
      if let Some(argument) = part.strip_prefix('{').and_then(|part| part.strip_suffix('}')) {
        let invalid = match input_args.get(argument_index) {
          Some(value) => !command_argument_value_valid(argument, value),
          None => validate_missing && command_argument_required(argument),
        };
        argument_index += 1;
        CommandUsagePart::Argument {
          label: command_argument_display_label(argument),
          invalid,
        }
      } else {
        CommandUsagePart::Name(Arc::from(part))
      }
    })
    .collect()
}

fn command_argument_display_label(argument: &str) -> Arc<str> {
  let Some((name, ty)) = command_argument_parts(argument) else {
    return Arc::from(argument);
  };
  Arc::from(format!("{name}:{}", command_argument_type_label(ty)))
}

fn command_argument_required(argument: &str) -> bool {
  let Some((name, ty)) = argument.split_once(':') else {
    return false;
  };
  !name.ends_with('?') && !ty.starts_with('?')
}

fn command_argument_pill(argument: &str, invalid: bool, invalid_feedback_phase: u8) -> Element {
  let (background, border, text_color) = if invalid {
    (
      BackgroundColor::Palette(theme::PaletteColor::DangerMuted),
      BackgroundColor::Color(theme::palette().danger.with_opacity(0.55)),
      theme::PaletteColor::Danger,
    )
  } else {
    (
      BackgroundColor::Color(Color::from_hex("#0B0C0E")),
      BackgroundColor::Palette(theme::PaletteColor::BorderStrong),
      theme::PaletteColor::TextSecondary,
    )
  };
  let shake_x = if invalid {
    command_invalid_shake_offset(invalid_feedback_phase)
  } else {
    0.0
  };

  Row::new()
    .height(22.0)
    .align_items(Alignment::Center)
    .padding_horizontal(6.0)
    .rounded(theme::RadiusSize::Md)
    .background(background)
    .border_inside(1.0, border)
    .transform(Transform2D::translate(shake_x, 0.0))
    .transition(Transition::transform().duration_ms(CHAT_COMMAND_INVALID_SHAKE_TRANSITION_MS))
    .child(
      Text::new(argument)
        .variant(theme::TypographyStyle::Button)
        .color(text_color),
    )
    .into()
}

fn command_invalid_shake_offset(phase: u8) -> f32 {
  match phase {
    1 => -8.0,
    2 => 7.0,
    3 => -5.0,
    4 => 4.0,
    5 => -2.0,
    _ => 0.0,
  }
}

fn command_preview_input_args(input: &str) -> Vec<String> {
  input
    .trim_start()
    .split_whitespace()
    .skip(1)
    .map(str::to_owned)
    .collect()
}

fn command_argument_value_valid(argument: &str, value: &str) -> bool {
  let Some((name, ty)) = command_argument_parts(argument) else {
    return true;
  };
  match ty {
    "u8" => value.parse::<u8>().is_ok_and(|value| name != "volume" || value <= 100),
    "u16" => value.parse::<u16>().is_ok(),
    "u32" => value.parse::<u32>().is_ok_and(|value| name != "userId" || value > 0),
    "u64" => value.parse::<u64>().is_ok(),
    "role" => matches!(
      value.to_ascii_lowercase().as_str(),
      "owner" | "admin" | "moderator" | "mod" | "user"
    ),
    "choice" | "string" => !value.trim().is_empty(),
    _ => true,
  }
}

fn command_argument_parts(argument: &str) -> Option<(&str, &str)> {
  let (name, ty) = argument.split_once(':')?;
  let ty = ty.strip_prefix('?').unwrap_or(ty);
  let name = name.strip_suffix('?').unwrap_or(name);
  Some((name, ty))
}

fn command_argument_type_label(ty: &str) -> &'static str {
  match ty {
    "u8" | "u16" | "u32" | "u64" => "Number",
    "string" => "String",
    "role" => "Role",
    "choice" => "Choice",
    _ => "Value",
  }
}

fn command_input_has_invalid_argument(
  command_registry: &ChatCommandRegistry,
  input: &str,
  _selected_index: &Signal<usize>,
) -> bool {
  let Some(command) = exact_chat_command_definition(command_registry, input) else {
    return false;
  };

  command_usage_parts(&command.usage, input, true)
    .into_iter()
    .any(|part| matches!(part, CommandUsagePart::Argument { invalid: true, .. }))
}

fn command_fill_text(command_name: &str) -> String {
  format!("{command_name} ")
}

fn submit_chat_if_valid(
  channel_id: Option<ChannelId>,
  message_input: &Signal<String>,
  command_selected_index: &Signal<usize>,
  command_invalid_feedback: &ChatCommandInvalidFeedback,
  command_registry: &ChatCommandRegistry,
  send_chat: &SendChatAction,
) {
  let text = message_input.get_untracked();
  let text = text.trim();
  if text.is_empty() {
    return;
  }

  if command_registry.has_commands()
    && command_input_has_invalid_argument(command_registry, &text, command_selected_index)
  {
    command_invalid_feedback.trigger();
    return;
  }

  run_chat_submission(channel_id, text, command_registry.clone(), send_chat);
  message_input.set(String::new());
}

fn run_chat_submission(
  channel_id: Option<ChannelId>,
  text: &str,
  command_registry: ChatCommandRegistry,
  send_chat: &SendChatAction,
) {
  send_chat.run(SendChatInput {
    channel_id,
    text: text.to_owned(),
    command_registry,
  });
}
