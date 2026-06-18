use lurq::{
  app::{
    component::Component,
    ctx::{Ctx, Modal, Root},
  },
  components::{Column, Row, Text},
  core::Signal,
  layout::{
    Alignment,
    layout_kind::{Justify, ScrollState},
  },
  node::{BackgroundColor, Element, dimension::Dimension},
};

use crate::{
  network::protocol::{ChannelId, UserId},
  routes::{ROUTE_CHOOSE_SERVER, ROUTE_TOFU_WARNING},
  services::screen_share_sources::ScreenShareSourceKind,
  session::{ConnectedServerInfo, ServerSession, chat_commands::ChatCommandRegistry},
  storage::{AppSettings, Storage},
  theme,
  ui::loader::loader,
};

mod actions;
mod channel_section;
mod chat;
mod content;
mod debug_channels;
mod disconnected;
mod layout;
mod rail;
mod shared;
mod stream_browser;
mod stream_modal;
mod stream_preview;
mod stream_shared;
mod stream_watching;
mod text_channels;
mod user_context_overlay;
mod voice_channels;

use actions::{
  chat_history_action, receiver_action, reconnect_action, send_chat_action, start_stream_action, stop_stream_action,
  stop_watching_action, watch_stream_action,
};
use chat::ChatCommandInvalidFeedback;
use content::main;
use disconnected::disconnected_lobby;
use rail::{LobbyRail, LobbyRailProps};
use stream_modal::start_stream_modal;
use stream_preview::floating_stream_preview;
use stream_shared::watched_stream;

type ReceiverAction = lurq::app::ctx::FutureAction<(), (), String>;
type ChatHistoryAction = lurq::app::ctx::FutureAction<Vec<ChatHistoryRequest>, (), String>;
type SendChatAction = lurq::app::ctx::FutureAction<SendChatInput, (), String>;
type StartStreamAction = lurq::app::ctx::FutureAction<StartStreamInput, (), String>;
type StopStreamAction = lurq::app::ctx::FutureAction<(), (), String>;
type WatchStreamAction = lurq::app::ctx::FutureAction<UserId, (), String>;
type StopWatchingAction = lurq::app::ctx::FutureAction<(), (), String>;
type ReconnectAction = lurq::app::ctx::FutureAction<ReconnectRequest, ConnectedServerInfo, String>;

const AUTO_RECONNECT_MAX_ATTEMPTS: u32 = 5;
const AUTO_RECONNECT_RETRY_DELAY_MS: u64 = 1_500;

#[derive(Clone, Copy)]
struct ChatHistoryRequest {
  channel_id: ChannelId,
  before_id: u64,
}

#[derive(Clone)]
struct SendChatInput {
  channel_id: Option<ChannelId>,
  text: String,
  command_registry: ChatCommandRegistry,
}

#[derive(Clone)]
struct StartStreamInput {
  source_kind: ScreenShareSourceKind,
  source_id: u32,
  width: u16,
  height: u16,
  audio_enabled: bool,
}

#[derive(Clone)]
struct ReconnectRequest {
  address: String,
  delay_ms: u64,
}

pub struct LobbyScreen {
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
  start_stream_modal_open: Signal<bool>,
  stream_start_submitted: Signal<bool>,
  stream_source_kind: Signal<ScreenShareSourceKind>,
  stream_source_index: Signal<usize>,
  stream_audio_enabled: Signal<bool>,
  reconnect_attempt: Signal<u32>,
}

impl Component for LobbyScreen {
  type Props = ();

  fn create(ctx: &mut Ctx) -> Self {
    Self {
      message_input: ctx.signal(String::new()),
      chat_command_selected_index: ctx.signal(0),
      chat_command_scroll_state: ScrollState::new(),
      chat_command_invalid_feedback: ChatCommandInvalidFeedback::new(ctx),
      chat_scroll_state: ScrollState::new(),
      chat_bottom_anchor: ctx.signal(None),
      chat_bottom_settle_anchor: ctx.signal(None),
      chat_bottom_detached_anchor: ctx.signal(None),
      chat_top_anchor: ctx.signal(None),
      chat_prepend_settle_anchor: ctx.signal(None),
      start_stream_modal_open: ctx.signal(false),
      stream_start_submitted: ctx.signal(false),
      stream_source_kind: ctx.signal(ScreenShareSourceKind::Screen),
      stream_source_index: ctx.signal(0),
      stream_audio_enabled: ctx.signal(true),
      reconnect_attempt: ctx.signal(0),
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let Some(session) = ctx.use_context::<ServerSession>() else {
      return empty_lobby(ctx);
    };
    let storage = ctx.use_context::<Storage>();

    let _revision = session.revision().get();

    let Some(info) = session.info() else {
      if let Some(navigator) = ctx.navigator() {
        navigator.replace(ROUTE_CHOOSE_SERVER);
      }
      return empty_lobby(ctx);
    };
    if session.tofu_warning().is_some() {
      if let Some(navigator) = ctx.navigator() {
        navigator.replace(ROUTE_TOFU_WARNING);
      }
      return empty_lobby(ctx);
    }
    let debug_mode_enabled = storage
      .as_ref()
      .and_then(|storage| storage.load_settings().ok())
      .unwrap_or_else(AppSettings::default)
      .debug_mode_enabled;

    let mut lobby = session.lobby();
    let receiver = receiver_action(ctx, session.clone());
    if !session.shutdown_requested()
      && !lobby.disconnected
      && !lobby.receiver_running
      && !receiver.state().get().is_pending()
    {
      receiver.run(());
    }
    let chat_history = chat_history_action(ctx, session.clone());
    if !chat_history.is_active() {
      let mut history_requests = Vec::new();
      if let Some(channel_id) = lobby.selected_text_channel_id
        && lobby
          .chat_messages_by_channel
          .get(&channel_id)
          .is_none_or(Vec::is_empty)
        && session.begin_chat_history_request(channel_id, 0)
      {
        lobby.chat_history_loading.insert(channel_id);
        history_requests.push(ChatHistoryRequest {
          channel_id,
          before_id: 0,
        });
      }
      for channel in &lobby.text_channels {
        if Some(channel.id) == lobby.selected_text_channel_id
          || !lobby
            .chat_messages_by_channel
            .get(&channel.id)
            .is_none_or(Vec::is_empty)
          || !session.begin_chat_history_request(channel.id, 0)
        {
          continue;
        }
        lobby.chat_history_loading.insert(channel.id);
        history_requests.push(ChatHistoryRequest {
          channel_id: channel.id,
          before_id: 0,
        });
      }
      if !history_requests.is_empty() {
        chat_history.run(history_requests);
      }
    }
    let send_chat = send_chat_action(ctx, session.clone());
    let start_stream = start_stream_action(ctx, storage.clone(), session.clone());
    let stop_stream = stop_stream_action(ctx, session.clone());
    let watch_stream = watch_stream_action(ctx, storage.clone(), session.clone());
    let stop_watching = stop_watching_action(ctx, session.clone());
    let reconnect = reconnect_action(ctx, storage.clone(), session.clone());

    if lobby.disconnected {
      return disconnected_lobby(ctx, &info, &lobby, session, &reconnect, self.reconnect_attempt.clone());
    } else if self.reconnect_attempt.get_untracked() != 0 {
      self.reconnect_attempt.set(0);
    }

    if !self.start_stream_modal_open.get_untracked() && self.stream_start_submitted.get_untracked() {
      self.stream_start_submitted.set(false);
    }

    let modal_open = self.start_stream_modal_open.clone();
    let modal_start_stream = start_stream.clone();
    let modal_start_submitted = self.stream_start_submitted.clone();
    let modal_source_kind = self.stream_source_kind.clone();
    let modal_source_index = self.stream_source_index.clone();
    let modal_audio_enabled = self.stream_audio_enabled.clone();
    let modal_stream_codec = stream_modal_codec_label(storage.as_ref());
    let mut body = Row::new()
      .width(Dimension::Pct(100.0))
      .height(Dimension::Pct(100.0))
      .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceBase))
      .clip()
      .child(ctx.mount::<LobbyRail>(LobbyRailProps {
        info: info.clone(),
        lobby: lobby.clone(),
        debug_mode_enabled,
        start_stream_modal_open: self.start_stream_modal_open.clone(),
        stop_stream: stop_stream.clone(),
        watch_stream: watch_stream.clone(),
      }))
      .child(main(
        ctx,
        &info,
        &lobby,
        self.message_input.clone(),
        self.chat_command_selected_index.clone(),
        self.chat_command_scroll_state.clone(),
        self.chat_command_invalid_feedback.clone(),
        self.chat_scroll_state.clone(),
        self.chat_bottom_anchor.clone(),
        self.chat_bottom_settle_anchor.clone(),
        self.chat_bottom_detached_anchor.clone(),
        self.chat_top_anchor.clone(),
        self.chat_prepend_settle_anchor.clone(),
        debug_mode_enabled,
        storage,
        session.clone(),
        &chat_history,
        &send_chat,
        self.start_stream_modal_open.clone(),
        &stop_stream,
        &watch_stream,
        &stop_watching,
      ));

    if modal_open.get() {
      body = body.child(
        Modal::new(start_stream_modal(
          ctx,
          modal_open.clone(),
          modal_source_kind,
          modal_source_index,
          modal_audio_enabled,
          modal_stream_codec,
          modal_start_stream,
          modal_start_submitted,
        ))
        .open(modal_open)
        .target(Root),
      );
    }

    if let Some(watched) = watched_stream(&lobby)
      && let Some(preview) = floating_stream_preview(
        ctx,
        &lobby,
        watched,
        debug_mode_enabled,
        session.clone(),
        &stop_watching,
      )
    {
      body = body.child(Modal::new(preview).target(Root).dismiss_on_escape(false));
    }

    body.into()
  }
}

fn stream_modal_codec_label(storage: Option<&Storage>) -> String {
  let codec = storage
    .and_then(|storage| storage.load_settings().ok())
    .unwrap_or_else(AppSettings::default)
    .video_codec;

  match codec.trim() {
    "H.265" | "H.264" => codec.trim().to_owned(),
    #[cfg(target_os = "macos")]
    _ => "H.265".to_owned(),
    #[cfg(not(target_os = "macos"))]
    _ => "AV1".to_owned(),
  }
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
