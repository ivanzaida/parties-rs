use std::{
  collections::hash_map::DefaultHasher,
  hash::{Hash, Hasher},
  time::Instant,
};

use chrono::Local;
use lurq::{
  app::{
    component::{Component, DevtoolsInspectable},
    ctx::{CollisionStrategy, Ctx, Overlay, Placement},
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

use super::{
  ChatHistoryAction, SendChatAction,
  model::{ChatPaneModel, chat_pane_model},
  session_identity::same_session,
  shared::error_notice,
  subscription::{LobbyModelSubscription, apply_model},
};
use crate::{
  network::protocol::{ChannelId, control::ChatMessage as ProtocolChatMessage},
  session::{ConnectedServerInfo, ServerSession},
  theme,
  ui::loader::loader,
};

#[derive(Clone)]
pub(super) struct ChatActions {
  pub history: ChatHistoryAction,
  pub send: SendChatAction,
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
}

impl Component for TextChannelDetail {
  type Props = TextChannelDetailProps;

  fn create(ctx: &mut Ctx) -> Self {
    Self {
      model_store: ctx.store(None),
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let props = ctx.props::<Self::Props>().clone();
    ctx.provide(self.model_store.clone());
    if self.model_store.with(Option::is_none) {
      apply_model(
        &self.model_store,
        chat_pane_model(
          &props.info,
          &props.session.lobby(),
          props.channel.id(),
          props.channel.is_server_backed(),
        ),
      );
    }
    let subscriber = ctx.mount::<ChatPaneModelSubscriber>(ChatPaneModelSubscriberProps {
      info: props.info.clone(),
      session: props.session.clone(),
      channel_id: props.channel.id(),
      server_backed: props.channel.is_server_backed(),
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
      &props.actions.history,
      &props.actions.send,
    )
  }
}

#[derive(Clone)]
struct ChatPaneModelSubscriberProps {
  info: ConnectedServerInfo,
  session: ServerSession,
  channel_id: ChannelId,
  server_backed: bool,
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
    let Some(model_store) = ctx.use_context::<Store<Option<ChatPaneModel>>>() else {
      return empty_subscriber_node();
    };

    apply_model(
      &model_store,
      chat_pane_model(
        &props.info,
        &props.session.lobby(),
        props.channel_id,
        props.server_backed,
      ),
    );

    let info = props.info.clone();
    let channel_id = props.channel_id;
    let server_backed = props.server_backed;
    if let Some((_snapshot_generation, model)) =
      self
        .subscription
        .next_model(ctx, props.session.clone(), move |snapshot| {
          chat_pane_model(&info, &snapshot.lobby, channel_id, server_backed)
        })
    {
      apply_model(&model_store, model);
    }

    empty_subscriber_node()
  }
}

fn empty_subscriber_node() -> Element {
  Rect::new(0.0, 0.0).into()
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
  chat_history: &ChatHistoryAction,
  send_chat: &SendChatAction,
) -> Element {
  let render_start = Instant::now();
  let channel_id = channel.id();
  let command_registry = channel.command_registry();
  let commands_enabled = command_registry.has_commands();
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
      chat_history,
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
    .child(subscriber)
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
