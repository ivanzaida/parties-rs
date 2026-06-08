use lurq::{
  app::{component::Component, ctx::Ctx},
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
  routes::ROUTE_CHOOSE_SERVER,
  services::screen_share_sources::ScreenShareSourceKind,
  session::{ConnectedServerInfo, ServerSession},
  storage::{AppSettings, Storage},
  theme,
  ui::loader::loader,
};

mod actions;
mod channel_section;
mod chat;
mod content;
mod disconnected;
mod layout;
mod rail;
mod shared;
mod stream_browser;
mod stream_modal;
mod stream_watching;
mod text_channels;
mod voice_channels;

use actions::{
  chat_history_action, receiver_action, reconnect_action, send_chat_action, start_stream_action, stop_stream_action,
  stop_watching_action, watch_stream_action,
};
use content::main;
use disconnected::disconnected_lobby;
use rail::{LobbyRail, LobbyRailProps};
use stream_modal::start_stream_modal;

type ReceiverAction = lurq::app::ctx::FutureAction<(), (), String>;
type ChatHistoryAction = lurq::app::ctx::FutureAction<ChatHistoryRequest, (), String>;
type SendChatAction = lurq::app::ctx::FutureAction<SendChatInput, (), String>;
type StartStreamAction = lurq::app::ctx::FutureAction<StartStreamInput, (), String>;
type StopStreamAction = lurq::app::ctx::FutureAction<(), (), String>;
type WatchStreamAction = lurq::app::ctx::FutureAction<UserId, (), String>;
type StopWatchingAction = lurq::app::ctx::FutureAction<(), (), String>;
type ReconnectAction = lurq::app::ctx::FutureAction<ReconnectRequest, ConnectedServerInfo, String>;

const AUTO_RECONNECT_MAX_ATTEMPTS: u32 = 3;
const AUTO_RECONNECT_RETRY_DELAY_MS: u64 = 1_500;

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
  chat_scroll_state: ScrollState,
  chat_bottom_anchor: Signal<Option<(ChannelId, u64)>>,
  chat_top_anchor: Signal<Option<(ChannelId, u64)>>,
  start_stream_modal_open: Signal<bool>,
  stream_source_screen_tab: Signal<bool>,
  stream_source_index: Signal<usize>,
  stream_audio_enabled: Signal<bool>,
  reconnect_attempt: Signal<u32>,
}

impl Component for LobbyScreen {
  type Props = ();

  fn create(ctx: &mut Ctx) -> Self {
    Self {
      message_input: ctx.signal(String::new()),
      chat_scroll_state: ScrollState::new(),
      chat_bottom_anchor: ctx.signal(None),
      chat_top_anchor: ctx.signal(None),
      start_stream_modal_open: ctx.signal(false),
      stream_source_screen_tab: ctx.signal(true),
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

    let modal_open = self.start_stream_modal_open.clone();
    let modal_start_stream = start_stream.clone();
    let modal_screen_tab = self.stream_source_screen_tab.clone();
    let modal_source_index = self.stream_source_index.clone();
    let modal_audio_enabled = self.stream_audio_enabled.clone();
    let modal_stream_codec = stream_modal_codec_label(storage.as_ref());
    ctx.modal(modal_open.clone(), move |ctx| {
      start_stream_modal(
        ctx,
        modal_open.clone(),
        modal_screen_tab.clone(),
        modal_source_index.clone(),
        modal_audio_enabled.clone(),
        &modal_stream_codec,
        modal_start_stream.clone(),
      )
    });

    Row::new()
      .width(Dimension::Pct(100.0))
      .height(Dimension::Pct(100.0))
      .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceBase))
      .clip()
      .child(ctx.mount::<LobbyRail>(LobbyRailProps {
        info: info.clone(),
        lobby: lobby.clone(),
        start_stream_modal_open: self.start_stream_modal_open.clone(),
        stop_stream: stop_stream.clone(),
      }))
      .child(main(
        ctx,
        &info,
        &lobby,
        self.message_input.clone(),
        self.chat_scroll_state.clone(),
        self.chat_bottom_anchor.clone(),
        self.chat_top_anchor.clone(),
        storage,
        session,
        &chat_history,
        &send_chat,
        self.start_stream_modal_open.clone(),
        &stop_stream,
        &watch_stream,
        &stop_watching,
      ))
      .into()
  }
}

fn stream_modal_codec_label(storage: Option<&Storage>) -> String {
  let codec = storage
    .and_then(|storage| storage.load_settings().ok())
    .unwrap_or_else(AppSettings::default)
    .video_codec;

  match codec.trim() {
    "H.265" | "H.264" => codec.trim().to_owned(),
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
