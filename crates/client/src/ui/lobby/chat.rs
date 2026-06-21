use std::{
  collections::hash_map::DefaultHasher,
  hash::{Hash, Hasher},
  time::{Duration, Instant},
};

use chrono::Local;
use lurq::{
  app::{
    component::{Component, DevtoolsInspectable},
    ctx::{CollisionStrategy, Ctx, Overlay, Placement, Timeout},
  },
  components::{Column, Rect, Row, ScrollVertical, Text},
  core::{Signal, Store},
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

const CHAT_COMMAND_QUERY_DEBOUNCE_MS: u64 = 400;

use super::{
  ChatCommandQueryAction, ChatCommandQueryRequest, ChatHistoryAction, SendChatAction,
  model::{ChatPaneModel, chat_pane_model},
  session_identity::same_session,
  shared::error_notice,
  subscription::{LobbyModelSubscription, apply_current_model, apply_model},
};
use crate::{
  network::protocol::{ChannelId, control::ChatMessage as ProtocolChatMessage},
  session::{ConnectedServerInfo, ServerSession, chat_commands::ChatCommandRegistry},
  theme,
  ui::loader::loader,
};

#[derive(Clone)]
pub(super) struct ChatActions {
  pub history: ChatHistoryAction,
  pub send: SendChatAction,
  pub command_query: ChatCommandQueryAction,
  pub command_query_signature: Signal<Option<String>>,
  pub command_selection_signature: Signal<Option<String>>,
  pub command_query_request_id: Signal<u64>,
}

pub(super) fn text_channel_detail(
  ctx: &mut Ctx,
  channel: ChatChannel,
  info: ConnectedServerInfo,
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
  actions: ChatActions,
) -> Element {
  let key = format!("text-channel-detail-{}", channel.id());
  ctx.mount_keyed::<TextChannelDetail>(
    &key,
    TextChannelDetailProps {
      channel,
      info,
      message_input,
      command_selected_index,
      command_scroll_state,
      command_invalid_feedback,
      chat_scroll_state,
      chat_bottom_anchor,
      chat_bottom_settle_anchor,
      chat_bottom_detached_anchor,
      chat_top_anchor,
      chat_prepend_settle_anchor,
      debug_user_ids,
      session,
      actions,
    },
  )
}

#[derive(Clone)]
struct TextChannelDetailProps {
  channel: ChatChannel,
  info: ConnectedServerInfo,
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
  actions: ChatActions,
}

impl PartialEq for TextChannelDetailProps {
  fn eq(&self, other: &Self) -> bool {
    self.channel == other.channel && self.info == other.info && self.debug_user_ids == other.debug_user_ids
  }
}

impl DevtoolsInspectable for TextChannelDetailProps {}

struct TextChannelDetail {
  model_store: Store<Option<ChatPaneModel>>,
  pending_command_query: Store<Option<PendingChatCommandQuery>>,
  command_query_debounce: Timeout,
}

#[derive(Clone)]
struct PendingChatCommandQuery {
  action: ChatCommandQueryAction,
  request: ChatCommandQueryRequest,
}

impl DevtoolsInspectable for PendingChatCommandQuery {}

impl Component for TextChannelDetail {
  type Props = TextChannelDetailProps;

  fn create(ctx: &mut Ctx) -> Self {
    let pending_command_query = ctx.store(None::<PendingChatCommandQuery>);
    let command_query_debounce = ctx.create_timeout(Duration::from_millis(CHAT_COMMAND_QUERY_DEBOUNCE_MS), {
      let pending_command_query = pending_command_query.clone();
      move || {
        if let Some(pending) = pending_command_query.get() {
          pending.action.run(pending.request);
        }
      }
    });
    Self {
      model_store: ctx.store(None),
      pending_command_query,
      command_query_debounce,
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let props = ctx.props::<Self::Props>().clone();
    if self.model_store.with(Option::is_none) {
      let info = props.info.clone();
      let channel_id = props.channel.id();
      let server_backed = props.channel.is_server_backed();
      apply_current_model(&self.model_store, &props.session, |lobby| {
        chat_pane_model(&info, lobby, channel_id, server_backed)
      });
    }
    let subscriber = ctx.mount::<ChatPaneModelSubscriber>(ChatPaneModelSubscriberProps {
      info: props.info.clone(),
      session: props.session.clone(),
      channel_id: props.channel.id(),
      server_backed: props.channel.is_server_backed(),
      model_store: self.model_store.clone(),
    });
    let Some(model) = self.model_store.get() else {
      return Column::new()
        .width(Dimension::Pct(100.0))
        .flex(1.0)
        .child(subscriber)
        .into();
    };

    text_channel_detail_view(
      ctx,
      subscriber,
      props.channel,
      model,
      props.message_input,
      props.command_selected_index,
      props.command_scroll_state,
      props.command_invalid_feedback,
      props.chat_scroll_state,
      props.chat_bottom_anchor,
      props.chat_bottom_settle_anchor,
      props.chat_bottom_detached_anchor,
      props.chat_top_anchor,
      props.chat_prepend_settle_anchor,
      props.debug_user_ids,
      props.session,
      &props.actions,
      &self.pending_command_query,
      &self.command_query_debounce,
    )
  }
}

#[derive(Clone)]
struct ChatPaneModelSubscriberProps {
  info: ConnectedServerInfo,
  session: ServerSession,
  channel_id: ChannelId,
  server_backed: bool,
  model_store: Store<Option<ChatPaneModel>>,
}

impl PartialEq for ChatPaneModelSubscriberProps {
  fn eq(&self, other: &Self) -> bool {
    self.info == other.info
      && same_session(&self.session, &other.session)
      && self.channel_id == other.channel_id
      && self.server_backed == other.server_backed
  }
}

impl DevtoolsInspectable for ChatPaneModelSubscriberProps {}

struct ChatPaneModelSubscriber {
  subscription: LobbyModelSubscription,
}

impl Component for ChatPaneModelSubscriber {
  type Props = ChatPaneModelSubscriberProps;

  fn create(ctx: &mut Ctx) -> Self {
    Self {
      subscription: LobbyModelSubscription::new(ctx),
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let props = ctx.props::<Self::Props>().clone();

    let info = props.info.clone();
    let channel_id = props.channel_id;
    let server_backed = props.server_backed;
    apply_current_model(&props.model_store, &props.session, |lobby| {
      chat_pane_model(&info, lobby, channel_id, server_backed)
    });

    let info = props.info.clone();
    if let Some((_snapshot_generation, model)) =
      self
        .subscription
        .next_model(ctx, props.session.clone(), move |snapshot| {
          chat_pane_model(&info, &snapshot.lobby, channel_id, server_backed)
        })
    {
      apply_model(&props.model_store, model);
    }

    empty_subscriber_node()
  }
}

fn empty_subscriber_node() -> Element {
  Rect::new(0.0, 0.0).into()
}

fn sync_chat_command_query(
  channel_id: Option<ChannelId>,
  message_input: &Signal<String>,
  command_registry: &ChatCommandRegistry,
  selected_index: &Signal<usize>,
  action: &ChatCommandQueryAction,
  signature: &Signal<Option<String>>,
  selection_signature: &Signal<Option<String>>,
  request_id: &Signal<u64>,
  pending_command_query: &Store<Option<PendingChatCommandQuery>>,
  command_query_debounce: &Timeout,
) {
  let Some(channel_id) = channel_id else {
    clear_chat_command_query_state(signature, selection_signature, pending_command_query, command_query_debounce);
    return;
  };
  let input = message_input.get();
  let Some(query) = command_registry.live_query_for_input(&input) else {
    clear_chat_command_query_state(signature, selection_signature, pending_command_query, command_query_debounce);
    return;
  };
  if query.query.len() < query.input.min_chars as usize {
    clear_chat_command_query_state(signature, selection_signature, pending_command_query, command_query_debounce);
    selected_index.set(0);
    return;
  }

  let next_signature = format!(
    "{}\n{}\n{}\n{}",
    channel_id, query.command_name, query.argument_name, query.query
  );
  if selection_signature.get_untracked().as_deref() != Some(next_signature.as_str()) {
    selected_index.set(0);
    selection_signature.set(Some(next_signature.clone()));
  }
  if signature.get_untracked().as_deref() == Some(next_signature.as_str()) {
    return;
  }

  let next_request_id = request_id.get_untracked().saturating_add(1).max(1);
  request_id.set(next_request_id);
  signature.set(Some(next_signature));
  pending_command_query.set(Some(PendingChatCommandQuery {
    action: action.clone(),
    request: ChatCommandQueryRequest {
      channel_id,
      request_id: next_request_id,
      command_name: query.command_name,
      argument_name: query.argument_name,
      query: query.query,
      cursor_pos: query.cursor_pos,
    },
  }));
  command_query_debounce.restart();
}

fn clear_chat_command_query_state(
  signature: &Signal<Option<String>>,
  selection_signature: &Signal<Option<String>>,
  pending_command_query: &Store<Option<PendingChatCommandQuery>>,
  command_query_debounce: &Timeout,
) {
  clear_chat_command_query_signature(signature);
  clear_chat_command_query_signature(selection_signature);
  if pending_command_query.get().is_some() {
    pending_command_query.set(None);
  }
  command_query_debounce.cancel();
}

fn clear_chat_command_query_signature(signature: &Signal<Option<String>>) {
  if signature.get_untracked().is_some() {
    signature.set(None);
  }
}

fn text_channel_detail_view(
  ctx: &mut Ctx,
  subscriber: Element,
  channel: ChatChannel,
  model: ChatPaneModel,
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
  actions: &ChatActions,
  pending_command_query: &Store<Option<PendingChatCommandQuery>>,
  command_query_debounce: &Timeout,
) -> Element {
  let render_start = Instant::now();
  let channel_id = channel.id();
  let command_registry = channel.command_registry();
  let commands_enabled = command_registry.has_commands();
  sync_chat_command_query(
    channel.server_channel_id(),
    &message_input,
    &command_registry,
    &command_selected_index,
    &actions.command_query,
    &actions.command_query_signature,
    &actions.command_selection_signature,
    &actions.command_query_request_id,
    pending_command_query,
    command_query_debounce,
  );
  let messages = model.messages.as_slice();
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

    if let Some(error) = model.error.as_deref() {
      messages_column = messages_column.child(error_notice(ctx, error));
    }
    chat_messages_scroll(
      ScrollVertical::new(messages_column),
      chat_scroll_state,
      chat_bottom_settle_for_scroll,
      chat_bottom_anchor,
      chat_bottom_detached_anchor,
      session,
      &actions.history,
      channel_id,
      oldest_message_id,
      can_page,
    )
  } else {
    let messages_content = chat_messages_content(
      ctx,
      messages,
      model.error.as_deref(),
      model.local_user_id,
      debug_user_ids,
    );
    chat_messages_scroll(
      ScrollVertical::new(messages_content),
      chat_scroll_state,
      chat_bottom_settle_for_scroll,
      chat_bottom_anchor,
      chat_bottom_detached_anchor,
      session,
      &actions.history,
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
    .child(subscriber)
    .child(messages_scroll)
    .child(chat_composer(
      ctx,
      &channel,
      message_input.clone(),
      command_selected_index.clone(),
      command_invalid_feedback.clone(),
      command_registry.clone(),
      model.command_query_response.clone(),
      actions.command_query_request_id.get(),
      &actions.send,
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
      model.command_query_response.clone(),
      actions.command_query_request_id.get(),
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
