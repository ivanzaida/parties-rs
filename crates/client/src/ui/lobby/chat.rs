use chrono::{Local, NaiveDate};
use lurq::{
  app::ctx::{CollisionStrategy, Ctx, Overlay, Placement},
  components::{Column, Text},
  core::Signal,
  layout::{
    Alignment,
    layout_kind::{Justify, ScrollState},
  },
  node::{Element, HitTestBehavior, dimension::Dimension},
};

mod channel;
mod composer;
mod message;
mod scroll;
mod timeline;

pub(super) use channel::ChatChannel;
pub(super) use composer::ChatCommandInvalidFeedback;
use composer::{CHAT_COMMAND_SUGGESTION_BOTTOM_GAP, chat_command_suggestions, chat_composer};
use message::{ChatMessage, ChatMessageProps};
use scroll::{
  chat_messages_scroll, preserve_chat_scroll_on_prepend, request_chat_history_if_at_top, schedule_chat_scroll_to_bottom,
};
use timeline::{chat_day_divider, local_chat_date};

use super::{ChatHistoryAction, SendChatAction, shared::error_notice};
use crate::{
  network::protocol::{ChannelId, control::ChatMessage as ProtocolChatMessage},
  session::{ConnectedServerInfo, LobbyState, ServerSession},
  theme,
  ui::loader::loader,
};

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
  chat_scroll_revision: Signal<u64>,
  chat_bottom_anchor: Signal<Option<(ChannelId, u64)>>,
  chat_bottom_settle_anchor: Signal<Option<(ChannelId, u64)>>,
  chat_top_anchor: Signal<Option<(ChannelId, u64)>>,
  debug_user_ids: bool,
  session: ServerSession,
  chat_history: &ChatHistoryAction,
  send_chat: &SendChatAction,
) -> Element {
  let _chat_scroll_revision = chat_scroll_revision.get();
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
  let channel_changed = chat_bottom_anchor
    .get_untracked()
    .is_none_or(|(anchor_channel_id, _)| anchor_channel_id != channel_id);
  if channel_changed {
    chat_scroll_state.scroll_to_bottom_pending();
    chat_scroll_revision.set(chat_scroll_revision.get_untracked().wrapping_add(1));
  }
  preserve_chat_scroll_on_prepend(
    channel_id,
    oldest_message_id,
    chat_scroll_state.clone(),
    chat_top_anchor,
  );
  let chat_bottom_settle_for_paging = chat_bottom_settle_anchor.clone();
  schedule_chat_scroll_to_bottom(
    channel_id,
    newest_message_id,
    newest_message_from_local,
    chat_scroll_state.clone(),
    chat_bottom_anchor,
    chat_bottom_settle_anchor,
  );
  request_chat_history_if_at_top(
    chat_scroll_state.clone(),
    chat_bottom_settle_for_paging,
    session.clone(),
    chat_history,
    channel_id,
    oldest_message_id,
    can_page,
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
    messages_column = append_chat_messages(ctx, messages_column, &messages, info.user_id, debug_user_ids);
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
      chat_scroll_revision,
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

fn append_chat_messages(
  ctx: &mut Ctx,
  column: Column,
  messages: &[ProtocolChatMessage],
  local_user_id: u32,
  debug_user_ids: bool,
) -> Column {
  let today = Local::now().date_naive();
  let mut last_day = None;
  let mut items = Vec::with_capacity(messages.len().saturating_mul(2));

  for message in messages {
    let message_day = local_chat_date(message.timestamp);
    if last_day != Some(message_day) {
      items.push(ChatTimelineItem::Day(message_day));
      last_day = Some(message_day);
    }

    items.push(ChatTimelineItem::Message(message));
  }

  column.with_children(ctx.for_each(
    items,
    |item| item.key(),
    move |ctx, item| match item {
      ChatTimelineItem::Day(day) => chat_day_divider(ctx, day, today),
      ChatTimelineItem::Message(message) => ctx.mount::<ChatMessage>(ChatMessageProps {
        message: message.clone(),
        local_user_id,
        debug_user_ids,
      }),
    },
  ))
}

#[derive(Clone, Copy)]
enum ChatTimelineItem<'a> {
  Day(NaiveDate),
  Message(&'a ProtocolChatMessage),
}

impl ChatTimelineItem<'_> {
  fn key(&self) -> String {
    match self {
      Self::Day(day) => format!("day-{day}"),
      Self::Message(message) => format!("message-{}", message.id),
    }
  }
}
