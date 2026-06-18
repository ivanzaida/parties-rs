use std::{
  collections::hash_map::DefaultHasher,
  hash::{Hash, Hasher},
  time::Instant,
};

use chrono::Local;
use lurq::{
  app::ctx::{CollisionStrategy, Ctx, Overlay, Placement},
  components::{Column, Row, ScrollVertical, Text},
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
mod scroll_policy;
mod timeline;

pub(super) use channel::ChatChannel;
pub(super) use composer::ChatCommandInvalidFeedback;
use composer::{CHAT_COMMAND_SUGGESTION_BOTTOM_GAP, chat_command_suggestions, chat_composer};
use message::{ChatMessage, ChatMessageProps};
use scroll::{chat_messages_scroll, preserve_chat_scroll_on_prepend, schedule_chat_scroll_to_bottom};
use timeline::{chat_day_divider, local_chat_date};

use super::{ChatHistoryAction, SendChatAction, model::ChatPaneModel, shared::error_notice};
use crate::{
  network::protocol::{ChannelId, control::ChatMessage as ProtocolChatMessage},
  session::ServerSession,
  theme,
  ui::loader::loader,
};

pub(super) fn text_channel_detail(
  ctx: &mut Ctx,
  channel: ChatChannel,
  model: ChatPaneModel<'_>,
  message_input: Signal<String>,
  command_selected_index: Signal<usize>,
  command_scroll_state: ScrollState,
  command_invalid_feedback: ChatCommandInvalidFeedback,
  chat_scroll_state: ScrollState,
  chat_bottom_anchor: Signal<Option<(ChannelId, u64)>>,
  chat_bottom_settle_anchor: Signal<Option<(ChannelId, u64, f32)>>,
  chat_bottom_detached_anchor: Signal<Option<(ChannelId, u64)>>,
  chat_top_anchor: Signal<Option<(ChannelId, u64)>>,
  chat_prepend_settle_anchor: Signal<Option<(ChannelId, u64, f32)>>,
  debug_user_ids: bool,
  session: ServerSession,
  chat_history: &ChatHistoryAction,
  send_chat: &SendChatAction,
) -> Element {
  let render_start = Instant::now();
  let channel_id = channel.id();
  let command_registry = channel.command_registry();
  let commands_enabled = command_registry.has_commands();
  let messages = model.messages;
  let oldest_message_id = messages.first().map(|message| message.id).unwrap_or(0);
  let newest_message_id = messages.last().map(|message| message.id).unwrap_or(0);
  let newest_message_from_local = messages
    .last()
    .is_some_and(|message| message.sender_id == model.local_user_id);
  let initial_history_loading = model.initial_history_loading;
  let can_page = model.can_page;
  let channel_changed = chat_bottom_anchor
    .get_untracked()
    .is_none_or(|(anchor_channel_id, _)| anchor_channel_id != channel_id);
  if channel_changed {
    chat_scroll_state.scroll_to_bottom_pending();
  }
  let _ = preserve_chat_scroll_on_prepend(
    channel_id,
    oldest_message_id,
    chat_scroll_state.clone(),
    chat_top_anchor,
    chat_prepend_settle_anchor.clone(),
  );
  let chat_bottom_settle_for_scroll = chat_bottom_settle_anchor.clone();
  schedule_chat_scroll_to_bottom(
    channel_id,
    newest_message_id,
    newest_message_from_local,
    chat_scroll_state.clone(),
    chat_bottom_anchor.clone(),
    chat_bottom_settle_anchor.clone(),
    chat_bottom_detached_anchor.clone(),
  );
  let list_start = Instant::now();
  let messages_scroll = if initial_history_loading || messages.is_empty() {
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
    } else {
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
    }

    if let Some(error) = model.error {
      messages_column = messages_column.child(error_notice(ctx, error));
    }
    chat_messages_scroll(
      ScrollVertical::new(messages_column),
      chat_scroll_state,
      chat_bottom_settle_for_scroll,
      chat_bottom_anchor,
      chat_bottom_detached_anchor,
      session,
      chat_history,
      channel_id,
      oldest_message_id,
      can_page,
    )
  } else {
    let messages_content = chat_messages_content(ctx, &messages, model.error, model.local_user_id, debug_user_ids);
    chat_messages_scroll(
      ScrollVertical::new(messages_content),
      chat_scroll_state,
      chat_bottom_settle_for_scroll,
      chat_bottom_anchor,
      chat_bottom_detached_anchor,
      session,
      chat_history,
      channel_id,
      oldest_message_id,
      can_page,
    )
  };
  let list_elapsed = list_start.elapsed();

  let composer_ref = ctx.element_ref();
  let mut body = Column::new()
    .width(Dimension::Pct(100.0))
    .height(Dimension::Pct(100.0))
    .flex(1.0)
    .child(messages_scroll)
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

  let element = body.into();
  let total_elapsed = render_start.elapsed();
  tracing::trace!(
    target: "chat-profile",
    "[chat-profile] text_channel_render channel={} messages={} loading={} empty={} can_page={} list_ms={:.3} total_ms={:.3}",
    channel_id,
    messages.len(),
    initial_history_loading,
    messages.is_empty(),
    can_page,
    list_elapsed.as_secs_f64() * 1000.0,
    total_elapsed.as_secs_f64() * 1000.0,
  );
  element
}

fn chat_messages_content(
  ctx: &mut Ctx,
  messages: &[ProtocolChatMessage],
  error: Option<&str>,
  local_user_id: u32,
  debug_user_ids: bool,
) -> Element {
  let today = Local::now().date_naive();
  let mut last_day = None;
  let mut column = Column::new().width(Dimension::Pct(100.0)).spacing(0.0);
  column = column.child(Row::new().width(Dimension::Pct(100.0)).height(24.0));

  for message in messages {
    let message_day = local_chat_date(message.timestamp);
    if last_day != Some(message_day) {
      column = column.child(chat_timeline_row(chat_day_divider(ctx, message_day, today)));
      last_day = Some(message_day);
    }

    let key = format!("message-{}-{:016x}", message.id, chat_message_content_hash(message));
    column = column.child(chat_timeline_row(ctx.mount_keyed::<ChatMessage>(
      &key,
      ChatMessageProps {
        message: message.clone(),
        local_user_id,
        debug_user_ids,
      },
    )));
  }

  if let Some(error) = error {
    column = column.child(chat_timeline_row(error_notice(ctx, error)));
  }
  column = column.child(Row::new().width(Dimension::Pct(100.0)).height(6.0));
  column.into()
}

fn chat_timeline_row(child: impl Into<Element>) -> Element {
  Column::new()
    .width(Dimension::Pct(100.0))
    .padding_horizontal(24.0)
    .padding_bottom(18.0)
    .child(child)
    .into()
}

fn chat_message_content_hash(message: &ProtocolChatMessage) -> u64 {
  let mut hasher = DefaultHasher::new();
  message.channel_id.hash(&mut hasher);
  message.sender_id.hash(&mut hasher);
  message.sender_name.hash(&mut hasher);
  message.timestamp.hash(&mut hasher);
  message.text.hash(&mut hasher);
  message.pinned.hash(&mut hasher);
  message.attachments.len().hash(&mut hasher);
  for attachment in &message.attachments {
    attachment.id.hash(&mut hasher);
    attachment.file_name.hash(&mut hasher);
    attachment.file_size.hash(&mut hasher);
    attachment.mime_type.hash(&mut hasher);
    attachment.uploaded.hash(&mut hasher);
  }
  hasher.finish()
}
