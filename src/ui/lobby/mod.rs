use std::{collections::HashSet, process::Command};

use chrono::{DateTime, Datelike, Local, NaiveDate, TimeZone, Timelike, Weekday};
use lurq::{
  animation::Transition,
  app::{
    component::{Component, ComponentInfo, DevtoolsInspectable},
    ctx::Ctx,
  },
  components::{Column, Row, ScrollVertical, Stack, Text, TextInput, TextOverflow},
  core::Signal,
  layout::{
    Alignment,
    layout_kind::{Justify, ScrollState},
    scrollbar::{ScrollBarPlacement, ScrollBarStyle},
  },
  node::{
    BackgroundColor, CursorIcon, Element, Style, border::Border, color::Color, dimension::Dimension,
    transform::Transform2D,
  },
};

use crate::{
  network::protocol::{ChannelId, Role, UserId, VideoCodecId, control::ChatMessage as ProtocolChatMessage},
  routes::ROUTE_CHOOSE_SERVER,
  services::screen_share_sources::{ScreenShareSource, list_screen_sources, list_window_sources},
  session::{
    ConnectedServerInfo, LobbyChannel, LobbyScreenShare, LobbyState, LobbyTextChannel, LobbyUser, ServerSession,
  },
  theme,
  ui::{
    app_chrome::{CHROME_HEIGHT, content_height},
    common::lucide_icon::{LucideIcon, LucideIconProps},
    loader::loader,
  },
};

mod channel_section;
mod channel_management;
mod rail;
mod text_channels;
mod voice_channels;

use channel_management::channel_management_screen;
use rail::{LobbyRail, LobbyRailProps, server_avatar};

type ReceiverAction = lurq::app::ctx::FutureAction<(), (), String>;
type ChatHistoryAction = lurq::app::ctx::FutureAction<ChatHistoryRequest, (), String>;
type SendChatAction = lurq::app::ctx::FutureAction<SendChatInput, (), String>;
type StartStreamAction = lurq::app::ctx::FutureAction<(), (), String>;
type StopStreamAction = lurq::app::ctx::FutureAction<(), (), String>;
type WatchStreamAction = lurq::app::ctx::FutureAction<UserId, (), String>;
type StopWatchingAction = lurq::app::ctx::FutureAction<(), (), String>;
pub(super) type ChannelAdminAction = lurq::app::ctx::FutureAction<ChannelAdminRequest, (), String>;

const STREAM_TOGGLE_TRANSITION_MS: u64 = 240;
const STREAM_SOURCE_CARD_HEIGHT: f32 = 150.0;
const STREAM_SOURCE_PREVIEW_HEIGHT: f32 = 108.0;
const STREAM_SOURCE_GRID_VISIBLE_HEIGHT: f32 = STREAM_SOURCE_CARD_HEIGHT * 2.0 + 12.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ChannelManagerKind {
  Text,
  Voice,
}

impl DevtoolsInspectable for ChannelManagerKind {
  fn write_info(&self, buffer: &mut Vec<ComponentInfo>) {
    buffer.push(ComponentInfo::with_value(
      "kind",
      std::any::type_name::<Self>(),
      match self {
        Self::Text => "Text",
        Self::Voice => "Voice",
      },
    ));
  }
}

#[derive(Clone)]
pub(super) enum ChannelAdminRequest {
  CreateText { name: String },
  DeleteText { channel_id: ChannelId },
  CreateVoice { name: String, max_users: u32 },
  RenameVoice { channel_id: ChannelId, name: String },
  DeleteVoice { channel_id: ChannelId },
}

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
  start_stream_modal_open: Signal<bool>,
  channel_manager: Signal<Option<ChannelManagerKind>>,
  stream_source_screen_tab: Signal<bool>,
  stream_source_index: Signal<usize>,
  stream_audio_enabled: Signal<bool>,
  text_channel_name: Signal<String>,
  voice_channel_name: Signal<String>,
  voice_channel_max_users: Signal<String>,
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
      channel_manager: ctx.signal(None),
      stream_source_screen_tab: ctx.signal(true),
      stream_source_index: ctx.signal(0),
      stream_audio_enabled: ctx.signal(true),
      text_channel_name: ctx.signal(String::new()),
      voice_channel_name: ctx.signal(String::new()),
      voice_channel_max_users: ctx.signal("8".to_owned()),
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
    let start_stream = start_stream_action(ctx, session.clone());
    let stop_stream = stop_stream_action(ctx, session.clone());
    let watch_stream = watch_stream_action(ctx, session.clone());
    let stop_watching = stop_watching_action(ctx, session.clone());
    let channel_admin = channel_admin_action(ctx, session.clone());
    let modal_open = self.start_stream_modal_open.clone();
    let modal_start_stream = start_stream.clone();
    let modal_screen_tab = self.stream_source_screen_tab.clone();
    let modal_source_index = self.stream_source_index.clone();
    let modal_audio_enabled = self.stream_audio_enabled.clone();
    ctx.modal(modal_open.clone(), move |ctx| {
      start_stream_modal(
        ctx,
        modal_open.clone(),
        modal_screen_tab.clone(),
        modal_source_index.clone(),
        modal_audio_enabled.clone(),
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
        channel_manager: self.channel_manager.clone(),
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
        self.start_stream_modal_open.clone(),
        self.channel_manager.clone(),
        self.text_channel_name.clone(),
        self.voice_channel_name.clone(),
        self.voice_channel_max_users.clone(),
        &channel_admin,
        &stop_stream,
        &watch_stream,
        &stop_watching,
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

fn start_stream_action(ctx: &mut Ctx, session: ServerSession) -> StartStreamAction {
  ctx.future_action(move |()| {
    let session = session.clone();
    async move {
      let server = session.server().ok_or_else(|| "No connected server.".to_owned())?;
      server
        .start_screen_share(VideoCodecId::Unknown, 0, 0)
        .await
        .map_err(|error| error.to_string())?;
      Ok(())
    }
  })
}

fn stop_stream_action(ctx: &mut Ctx, session: ServerSession) -> StopStreamAction {
  ctx.future_action(move |()| {
    let session = session.clone();
    async move {
      let server = session.server().ok_or_else(|| "No connected server.".to_owned())?;
      server.stop_screen_share().await.map_err(|error| error.to_string())?;
      Ok(())
    }
  })
}

fn watch_stream_action(ctx: &mut Ctx, session: ServerSession) -> WatchStreamAction {
  ctx.future_action(move |user_id| {
    let session = session.clone();
    async move {
      let server = session.server().ok_or_else(|| "No connected server.".to_owned())?;
      server
        .view_screen_share(user_id)
        .await
        .map_err(|error| error.to_string())?;
      if let Err(error) = server.request_keyframe(user_id) {
        return Err(error.to_string());
      }
      session.set_watching_user(Some(user_id));
      Ok(())
    }
  })
}

fn stop_watching_action(ctx: &mut Ctx, session: ServerSession) -> StopWatchingAction {
  ctx.future_action(move |()| {
    let session = session.clone();
    async move {
      let server = session.server().ok_or_else(|| "No connected server.".to_owned())?;
      server
        .unsubscribe_screen_share()
        .await
        .map_err(|error| error.to_string())?;
      session.set_watching_user(None);
      Ok(())
    }
  })
}

fn channel_admin_action(ctx: &mut Ctx, session: ServerSession) -> ChannelAdminAction {
  ctx.future_action(move |request| {
    let session = session.clone();
    async move {
      let server = session.server().ok_or_else(|| "No connected server.".to_owned())?;
      match request {
        ChannelAdminRequest::CreateText { name } => server.create_text_channel(name).await,
        ChannelAdminRequest::DeleteText { channel_id } => server.delete_text_channel(channel_id).await,
        ChannelAdminRequest::CreateVoice { name, max_users } => server.create_channel(name, max_users).await,
        ChannelAdminRequest::RenameVoice { channel_id, name } => server.rename_channel(channel_id, name).await,
        ChannelAdminRequest::DeleteVoice { channel_id } => server.delete_channel(channel_id).await,
      }
      .map_err(|error| error.to_string())
    }
  })
}

fn start_stream_modal(
  ctx: &mut Ctx,
  open: Signal<bool>,
  screen_tab: Signal<bool>,
  source_index: Signal<usize>,
  audio_enabled: Signal<bool>,
  start_stream: StartStreamAction,
) -> Element {
  let window = ctx.window();
  let window_width = window.logical_width();
  let modal_height = content_height(ctx);
  let dialog_width = (window_width - 32.0).min(560.0).max(320.0);

  Column::new()
    .width(window_width)
    .height(modal_height)
    .absolute(0.0, CHROME_HEIGHT, window_width, modal_height)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .background(BackgroundColor::Color(Color::from_hex("#00000099")))
    .child(
      Column::new()
        .width(dialog_width)
        .spacing(20.0)
        .padding(28.0)
        .rounded(10.0)
        .background(BackgroundColor::Color(Color::from_hex("#15171A")))
        .border_inside(1.0, BackgroundColor::Color(Color::from_hex("#30343A")))
        .child(stream_modal_header(ctx, open.clone()))
        .child(stream_modal_sources(ctx, screen_tab, source_index))
        .child(stream_modal_audio_toggle(ctx, audio_enabled))
        .child(stream_modal_actions(ctx, open, start_stream)),
    )
    .into()
}

fn stream_modal_header(ctx: &mut Ctx, open: Signal<bool>) -> Element {
  let close = open.clone();
  Row::new()
    .width(Dimension::Pct(100.0))
    .align_items(Alignment::Center)
    .spacing(theme::SpacingSize::Lg)
    .child(
      Row::new()
        .width(44.0)
        .height(44.0)
        .align_items(Alignment::Center)
        .justify(Justify::Center)
        .rounded(12.0)
        .background(BackgroundColor::Palette(theme::PaletteColor::AccentMuted))
        .border_inside(1.0, theme::PaletteColor::Accent)
        .child(ctx.mount::<LucideIcon>(LucideIconProps {
          icon: "monitor-up",
          size: 22.0,
          color: theme::palette().accent,
        })),
    )
    .child(
      Column::new()
        .width(Dimension::Pct(100.0))
        .flex(1.0)
        .spacing(3.0)
        .child(
          Text::new(&ctx.t("lobby.stream_modal.title"))
            .variant(theme::TypographyStyle::Title)
            .color(theme::PaletteColor::TextPrimary),
        )
        .child(
          Text::new(&ctx.t("lobby.stream_modal.subtitle"))
            .variant(theme::TypographyStyle::Caption)
            .color(theme::PaletteColor::TextSecondary)
            .width(Dimension::Pct(100.0)),
        ),
    )
    .child(
      Row::new()
        .width(30.0)
        .height(30.0)
        .align_items(Alignment::Center)
        .justify(Justify::Center)
        .rounded(theme::RadiusSize::Lg)
        .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceInput))
        .cursor(CursorIcon::Pointer)
        .hovered_style(Style::new().background(BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised)))
        .on_click(move |_| close.set(false))
        .child(ctx.mount::<LucideIcon>(LucideIconProps {
          icon: "x",
          size: 16.0,
          color: theme::palette().text_muted,
        })),
    )
    .into()
}

fn stream_modal_sources(ctx: &mut Ctx, screen_tab: Signal<bool>, source_index: Signal<usize>) -> Element {
  Column::new()
    .width(Dimension::Pct(100.0))
    .spacing(12.0)
    .child(stream_modal_tabs(ctx, screen_tab.clone()))
    .child(stream_source_grid(ctx, screen_tab, source_index))
    .into()
}

fn stream_modal_tabs(ctx: &mut Ctx, screen_tab: Signal<bool>) -> Element {
  let screen_active = screen_tab.get();
  Row::new()
    .width(Dimension::Pct(100.0))
    .spacing(3.0)
    .padding(3.0)
    .rounded(theme::RadiusSize::Lg)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceInput))
    .child(stream_modal_tab(
      ctx,
      "lobby.stream_modal.tab.screen",
      screen_active,
      screen_tab.clone(),
      true,
    ))
    .child(stream_modal_tab(
      ctx,
      "lobby.stream_modal.tab.window",
      !screen_active,
      screen_tab,
      false,
    ))
    .into()
}

fn stream_modal_tab(
  ctx: &mut Ctx,
  label_key: &'static str,
  active: bool,
  screen_tab: Signal<bool>,
  value: bool,
) -> Element {
  Row::new()
    .height(32.0)
    .width(Dimension::Pct(100.0))
    .flex(1.0)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .rounded(theme::RadiusSize::Md)
    .background(if active {
      BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised)
    } else {
      BackgroundColor::Color(Color::from_hex("#00000000"))
    })
    .cursor(CursorIcon::Pointer)
    .on_click(move |_| screen_tab.set(value))
    .child(
      Text::new(&ctx.t(label_key))
        .variant(theme::TypographyStyle::Button)
        .color(if active {
          theme::PaletteColor::TextPrimary
        } else {
          theme::PaletteColor::TextSecondary
        }),
    )
    .into()
}

fn stream_source_grid(ctx: &mut Ctx, screen_tab: Signal<bool>, source_index: Signal<usize>) -> Element {
  let sources = if screen_tab.get() {
    list_screen_sources()
  } else {
    list_window_sources()
  };
  let selected_index = source_index.get().min(sources.len().saturating_sub(1));

  if sources.is_empty() {
    return stream_source_empty_state(ctx, screen_tab.get());
  }

  let mut grid = Column::new().width(Dimension::Pct(100.0)).spacing(12.0);

  for (row_index, row_sources) in sources.chunks(2).enumerate() {
    grid = grid.child(stream_source_row(
      ctx,
      row_sources,
      row_index * 2,
      selected_index,
      source_index.clone(),
    ));
  }

  ScrollVertical::new(grid)
    .width(Dimension::Pct(100.0))
    .height(STREAM_SOURCE_GRID_VISIBLE_HEIGHT)
    .scrollbar(source_grid_scrollbar_style())
    .scrollbar_hovered(|mut style| {
      let palette = theme::palette();
      style.thumb_color = palette.accent_hover;
      style.track_color = palette.surface_input.with_opacity(0.75);
      style
    })
    .into()
}

fn stream_source_empty_state(ctx: &mut Ctx, screen_tab: bool) -> Element {
  Row::new()
    .width(Dimension::Pct(100.0))
    .height(160.0)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .rounded(8.0)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceInput))
    .border_inside(1.0, theme::PaletteColor::Border)
    .child(
      Text::new(&ctx.t(if screen_tab {
        "lobby.stream_modal.source.empty_screens"
      } else {
        "lobby.stream_modal.source.empty_windows"
      }))
      .variant(theme::TypographyStyle::Caption)
      .color(theme::PaletteColor::TextMuted),
    )
    .into()
}

fn source_grid_scrollbar_style() -> ScrollBarStyle {
  let palette = theme::palette();
  ScrollBarStyle {
    width: 6.0,
    min_thumb_length: 24.0,
    track_color: palette.surface_input.with_opacity(0.55),
    thumb_color: palette.accent,
    thumb_radius: 3.0,
    track_radius: 3.0,
    padding: 2.0,
    placement: ScrollBarPlacement::Reserved,
    ..ScrollBarStyle::default()
  }
}

fn stream_source_row(
  ctx: &mut Ctx,
  sources: &[ScreenShareSource],
  offset: usize,
  selected_index: usize,
  source_index: Signal<usize>,
) -> Element {
  let mut row = Row::new().width(Dimension::Pct(100.0)).spacing(12.0);

  for (column_index, source) in sources.iter().enumerate() {
    row = row.child(stream_source_card(
      ctx,
      source,
      offset + column_index,
      selected_index,
      source_index.clone(),
    ));
  }

  if sources.len() == 1 {
    row = row.child(Row::new().width(Dimension::Pct(100.0)).flex(1.0));
  }

  row.into()
}

fn stream_source_card(
  ctx: &mut Ctx,
  source: &ScreenShareSource,
  index: usize,
  selected_index: usize,
  source_index: Signal<usize>,
) -> Element {
  let selected = selected_index == index;
  let select = source_index.clone();
  Column::new()
    .width(Dimension::Pct(100.0))
    .flex(1.0)
    .height(STREAM_SOURCE_CARD_HEIGHT)
    .spacing(6.0)
    .padding(8.0)
    .rounded(8.0)
    .clip()
    .background(BackgroundColor::Palette(if selected {
      theme::PaletteColor::AccentMuted
    } else {
      theme::PaletteColor::SurfaceInput
    }))
    .border_inside(
      1.0,
      if selected {
        theme::PaletteColor::Accent
      } else {
        theme::PaletteColor::Border
      },
    )
    .cursor(CursorIcon::Pointer)
    .on_click(move |_| select.set(index))
    .child(stream_source_preview(ctx, selected, source.resolution.as_deref()))
    .child(
      Row::new()
        .width(Dimension::Pct(100.0))
        .height(18.0)
        .align_items(Alignment::Center)
        .spacing(8.0)
        .child(ctx.mount::<LucideIcon>(LucideIconProps {
          icon: if selected { "check-circle" } else { "monitor" },
          size: 14.0,
          color: if selected {
            theme::palette().accent
          } else {
            theme::palette().text_muted
          },
        }))
        .child(
          Text::new(&source.name)
            .variant(theme::TypographyStyle::Caption)
            .color(if selected {
              theme::PaletteColor::TextPrimary
            } else {
              theme::PaletteColor::TextSecondary
            })
            .nowrap()
            .text_overflow(TextOverflow::Elipsis)
            .width(Dimension::Pct(100.0))
            .min_width(0.0)
            .flex(1.0),
        ),
    )
    .into()
}

fn stream_source_preview(ctx: &mut Ctx, selected: bool, resolution: Option<&str>) -> Element {
  let mut preview = Stack::new()
    .width(Dimension::Pct(100.0))
    .height(STREAM_SOURCE_PREVIEW_HEIGHT)
    .rounded(theme::RadiusSize::Lg)
    .clip()
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised))
    .child(
      Column::new()
        .width(Dimension::Pct(100.0))
        .height(STREAM_SOURCE_PREVIEW_HEIGHT)
        .align_items(Alignment::Center)
        .justify(Justify::Center)
        .child(ctx.mount::<LucideIcon>(LucideIconProps {
          icon: "monitor",
          size: 26.0,
          color: if selected {
            theme::palette().accent
          } else {
            theme::palette().text_muted
          },
        })),
    );

  if let Some(resolution) = resolution.filter(|value| !value.trim().is_empty()) {
    preview = preview.child(
      Column::new()
        .width(Dimension::Pct(100.0))
        .height(STREAM_SOURCE_PREVIEW_HEIGHT)
        .align_items(Alignment::End)
        .padding_top(8.0)
        .padding_right(8.0)
        .child(stream_source_resolution_badge(resolution)),
    );
  }

  preview.into()
}

fn stream_source_resolution_badge(resolution: &str) -> Element {
  Row::new()
    .height(20.0)
    .align_items(Alignment::Center)
    .padding_horizontal(6.0)
    .rounded(theme::RadiusSize::Sm)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceInput))
    .border_inside(1.0, theme::PaletteColor::Border)
    .child(
      Text::new(resolution)
        .variant(theme::TypographyStyle::FieldLabel)
        .color(theme::PaletteColor::TextMuted)
        .nowrap(),
    )
    .into()
}

fn stream_modal_audio_toggle(ctx: &mut Ctx, audio_enabled: Signal<bool>) -> Element {
  let enabled = audio_enabled.get();
  let palette = theme::palette();
  let knob_translate = if enabled { 16.0 } else { 0.0 };
  let toggle = audio_enabled.clone();
  Row::new()
    .width(Dimension::Pct(100.0))
    .align_items(Alignment::Center)
    .spacing(12.0)
    .padding_vertical(12.0)
    .padding_horizontal(14.0)
    .rounded(8.0)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceInput))
    .border_inside(1.0, theme::PaletteColor::Border)
    .child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon: "volume-2",
      size: 18.0,
      color: theme::palette().text_secondary,
    }))
    .child(
      Column::new()
        .width(Dimension::Pct(100.0))
        .flex(1.0)
        .spacing(2.0)
        .child(
          Text::new(&ctx.t("lobby.stream_modal.audio.title"))
            .variant(theme::TypographyStyle::Button)
            .color(theme::PaletteColor::TextPrimary),
        )
        .child(
          Text::new(&ctx.t("lobby.stream_modal.audio.description"))
            .variant(theme::TypographyStyle::FieldLabel)
            .color(theme::PaletteColor::TextMuted)
            .width(Dimension::Pct(100.0)),
        ),
    )
    .child(
      Row::new()
        .width(38.0)
        .height(22.0)
        .align_items(Alignment::Center)
        .padding_left(2.0)
        .rounded(11.0)
        .background(BackgroundColor::Color(if enabled {
          palette.accent
        } else {
          palette.surface_raised
        }))
        .border_inside(
          1.0,
          BackgroundColor::Color(if enabled {
            palette.surface_raised
          } else {
            Color::from_hex("#3A4047")
          }),
        )
        .transition(Transition::background_color().duration_ms(STREAM_TOGGLE_TRANSITION_MS))
        .cursor(CursorIcon::Pointer)
        .on_click(move |_| toggle.set(!enabled))
        .child(
          Row::new()
            .width(18.0)
            .height(18.0)
            .rounded(9.0)
            .background(BackgroundColor::Color(if enabled {
              palette.surface_base
            } else {
              palette.text_muted
            }))
            .transform(Transform2D::translate(knob_translate, 0.0))
            .transition(Transition::background_color().duration_ms(STREAM_TOGGLE_TRANSITION_MS))
            .transition(Transition::transform().duration_ms(STREAM_TOGGLE_TRANSITION_MS)),
        ),
    )
    .into()
}

fn stream_modal_actions(ctx: &mut Ctx, open: Signal<bool>, start_stream: StartStreamAction) -> Element {
  let close = open.clone();
  let confirm_open = open.clone();
  let pending = start_stream.state().get().is_pending();
  Row::new()
    .width(Dimension::Pct(100.0))
    .align_items(Alignment::Center)
    .justify(Justify::End)
    .spacing(10.0)
    .child(
      stream_modal_button(ctx, None, "common.action.cancel", false).on_click(move |_| {
        close.set(false);
      }),
    )
    .child({
      let mut button = stream_modal_button(ctx, Some("monitor-up"), "lobby.stream_modal.action.start", true);
      if !pending {
        button = button.on_click(move |_| {
          confirm_open.set(false);
          start_stream.run(());
        });
      }
      button
    })
    .into()
}

fn stream_modal_button(ctx: &mut Ctx, icon: Option<&'static str>, label_key: &'static str, primary: bool) -> Row {
  let mut button = Row::new()
    .height(34.0)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .spacing(7.0)
    .padding_horizontal(if primary { 14.0 } else { 16.0 })
    .rounded(theme::RadiusSize::Md)
    .background(BackgroundColor::Palette(if primary {
      theme::PaletteColor::Accent
    } else {
      theme::PaletteColor::SurfaceBase
    }))
    .border_inside(
      1.0,
      if primary {
        theme::PaletteColor::Accent
      } else {
        theme::PaletteColor::Border
      },
    )
    .cursor(CursorIcon::Pointer)
    .hovered_style(Style::new().background(BackgroundColor::Palette(if primary {
      theme::PaletteColor::AccentHover
    } else {
      theme::PaletteColor::SurfaceRaised
    })));

  if let Some(icon) = icon {
    button = button.child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon,
      size: 16.0,
      color: if primary {
        theme::palette().text_inverse
      } else {
        theme::palette().text_secondary
      },
    }));
  }

  button.child(
    Text::new(&ctx.t(label_key))
      .variant(theme::TypographyStyle::Button)
      .color(if primary {
        theme::PaletteColor::TextInverse
      } else {
        theme::PaletteColor::TextSecondary
      }),
  )
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
  start_stream_modal_open: Signal<bool>,
  channel_manager: Signal<Option<ChannelManagerKind>>,
  text_channel_name: Signal<String>,
  voice_channel_name: Signal<String>,
  voice_channel_max_users: Signal<String>,
  channel_admin: &ChannelAdminAction,
  stop_stream: &StopStreamAction,
  watch_stream: &WatchStreamAction,
  stop_watching: &StopWatchingAction,
) -> Element {
  if let Some(kind) = channel_manager.get() {
    return channel_management_screen(
      ctx,
      kind,
      lobby,
      channel_manager,
      text_channel_name,
      voice_channel_name,
      voice_channel_max_users,
      channel_admin,
    );
  }

  Column::new()
    .width(Dimension::Pct(100.0))
    .height(Dimension::Pct(100.0))
    .flex(1.0)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceBase))
    .child(main_top_bar(
      ctx,
      info.user_id,
      lobby,
      start_stream_modal_open.clone(),
      stop_stream,
    ))
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
      start_stream_modal_open,
      stop_stream,
      watch_stream,
      stop_watching,
    ))
    .into()
}

fn main_top_bar(
  ctx: &mut Ctx,
  local_user_id: UserId,
  lobby: &LobbyState,
  start_stream_modal_open: Signal<bool>,
  stop_stream: &StopStreamAction,
) -> Element {
  if let Some(channel) = selected_text_channel(lobby) {
    return text_channel_top_bar(ctx, channel, unique_lobby_member_count(lobby));
  }

  if let Some(channel) = stream_browser_channel(lobby).or_else(|| selected_voice_channel(lobby)) {
    if watched_stream_for_channel(lobby, channel.id).is_some() {
      return watching_top_bar(ctx, channel, local_user_id, lobby, start_stream_modal_open, stop_stream);
    }
    return voice_stream_top_bar(
      ctx,
      channel,
      screen_share_count_for_channel(lobby, channel.id),
      start_stream_modal_open,
    );
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

fn voice_stream_top_bar(
  ctx: &mut Ctx,
  channel: &LobbyChannel,
  stream_count: usize,
  start_stream_modal_open: Signal<bool>,
) -> Element {
  Row::new()
    .width(Dimension::Pct(100.0))
    .height(56.0)
    .align_items(Alignment::Center)
    .justify(Justify::SpaceBetween)
    .spacing(theme::SpacingSize::Md)
    .padding_horizontal(theme::SpacingSize::Xl)
    .border_bottom(Border::inside(1.0, theme::PaletteColor::Border))
    .child(
      Row::new()
        .align_items(Alignment::Center)
        .spacing(10.0)
        .child(ctx.mount::<LucideIcon>(LucideIconProps {
          icon: "volume-2",
          size: 16.0,
          color: theme::palette().text_secondary,
        }))
        .child(
          Text::new(&channel.name)
            .variant(theme::TypographyStyle::Heading)
            .color(theme::PaletteColor::TextPrimary),
        )
        .child(stream_count_chip(ctx, stream_count)),
    )
    .child(top_bar_share_button(ctx, start_stream_modal_open))
    .into()
}

fn stream_count_chip(ctx: &mut Ctx, stream_count: usize) -> Element {
  if stream_count == 0 {
    return Row::new().into();
  }

  Row::new()
    .height(22.0)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .spacing(5.0)
    .padding_vertical(3.0)
    .padding_horizontal(8.0)
    .rounded(theme::RadiusSize::Sm)
    .background(BackgroundColor::Color(Color::from_hex("#2A1A1C")))
    .child(
      Row::new()
        .width(6.0)
        .height(6.0)
        .rounded(3.0)
        .background(BackgroundColor::Color(Color::from_hex("#FF6B5F"))),
    )
    .child(
      Text::new(&ctx.t_args("lobby.stream_browser.live_short", [("count", stream_count.to_string())]))
        .variant(theme::TypographyStyle::FieldLabel)
        .color(theme::PaletteColor::Danger),
    )
    .into()
}

fn watching_top_bar(
  ctx: &mut Ctx,
  channel: &LobbyChannel,
  local_user_id: UserId,
  lobby: &LobbyState,
  start_stream_modal_open: Signal<bool>,
  stop_stream: &StopStreamAction,
) -> Element {
  let local_sharing = screen_shares_for_channel(lobby, channel.id)
    .iter()
    .any(|stream| stream.share.sharer_user_id == local_user_id);

  Row::new()
    .width(Dimension::Pct(100.0))
    .height(56.0)
    .align_items(Alignment::Center)
    .justify(Justify::SpaceBetween)
    .spacing(theme::SpacingSize::Md)
    .padding_horizontal(theme::SpacingSize::Xl)
    .border_bottom(Border::inside(1.0, theme::PaletteColor::Border))
    .child(
      Row::new()
        .align_items(Alignment::Center)
        .spacing(10.0)
        .child(ctx.mount::<LucideIcon>(LucideIconProps {
          icon: "volume-2",
          size: 16.0,
          color: theme::palette().text_secondary,
        }))
        .child(
          Text::new(&channel.name)
            .variant(theme::TypographyStyle::Heading)
            .color(theme::PaletteColor::TextPrimary),
        )
        .child(watching_badge(ctx)),
    )
    .child(if local_sharing {
      top_bar_stop_stream_button(ctx, stop_stream)
    } else {
      top_bar_share_button(ctx, start_stream_modal_open)
    })
    .into()
}

fn watching_badge(ctx: &mut Ctx) -> Element {
  Row::new()
    .height(22.0)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .spacing(5.0)
    .padding_vertical(3.0)
    .padding_horizontal(8.0)
    .rounded(theme::RadiusSize::Sm)
    .background(BackgroundColor::Color(Color::from_hex("#2A1A1C")))
    .child(
      Row::new()
        .width(6.0)
        .height(6.0)
        .rounded(3.0)
        .background(BackgroundColor::Color(Color::from_hex("#FF6B5F"))),
    )
    .child(
      Text::new(&ctx.t("lobby.stream_browser.watching.badge"))
        .variant(theme::TypographyStyle::FieldLabel)
        .color(theme::PaletteColor::Danger),
    )
    .into()
}

fn top_bar_share_button(ctx: &mut Ctx, start_stream_modal_open: Signal<bool>) -> Element {
  let open = start_stream_modal_open.clone();
  let mut button = dark_top_bar_button(ctx, "monitor-up", "lobby.stream_browser.watching.share_screen");
  button = button.on_click(move |_| open.set(true));

  button.into()
}

fn top_bar_stop_stream_button(ctx: &mut Ctx, stop_stream: &StopStreamAction) -> Element {
  let pending = stop_stream.state().get().is_pending();
  let action = stop_stream.clone();
  let mut button = dark_top_bar_button(ctx, "screen-share-off", "lobby.stream_browser.list.stop");

  if !pending {
    button = button.on_click(move |_| action.run(()));
  }

  button.into()
}

fn dark_top_bar_button(ctx: &mut Ctx, icon: &'static str, label_key: &'static str) -> Row {
  Row::new()
    .height(34.0)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .spacing(7.0)
    .padding_horizontal(14.0)
    .rounded(theme::RadiusSize::Md)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised))
    .border_inside(1.0, theme::PaletteColor::Border)
    .cursor(CursorIcon::Pointer)
    .hovered_style(Style::new().background(BackgroundColor::Palette(theme::PaletteColor::SurfaceInput)))
    .child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon,
      size: 16.0,
      color: theme::palette().text_secondary,
    }))
    .child(
      Text::new(&ctx.t(label_key))
        .variant(theme::TypographyStyle::Button)
        .color(theme::PaletteColor::TextPrimary),
    )
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
  start_stream_modal_open: Signal<bool>,
  stop_stream: &StopStreamAction,
  watch_stream: &WatchStreamAction,
  stop_watching: &StopWatchingAction,
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

  if let Some(channel) = stream_browser_channel(lobby).or_else(|| selected_voice_channel(lobby)) {
    return stream_browser(
      ctx,
      channel,
      info.user_id,
      lobby,
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

fn stream_browser_channel(lobby: &LobbyState) -> Option<&LobbyChannel> {
  lobby
    .stream_browser_channel_id
    .and_then(|id| lobby.channels.iter().find(|channel| channel.id == id))
}

fn selected_voice_channel(lobby: &LobbyState) -> Option<&LobbyChannel> {
  lobby
    .selected_channel_id
    .and_then(|id| lobby.channels.iter().find(|channel| channel.id == id))
}

fn screen_share_count_for_channel(lobby: &LobbyState, channel_id: ChannelId) -> usize {
  let Some(users) = lobby.users_by_channel.get(&channel_id) else {
    return 0;
  };
  let user_ids = users.iter().map(|user| user.user_id).collect::<HashSet<_>>();
  lobby
    .screen_shares
    .iter()
    .filter(|share| user_ids.contains(&share.sharer_user_id))
    .count()
}

struct ChannelScreenShare<'a> {
  share: &'a LobbyScreenShare,
  user: Option<&'a LobbyUser>,
}

fn screen_shares_for_channel(lobby: &LobbyState, channel_id: ChannelId) -> Vec<ChannelScreenShare<'_>> {
  let Some(users) = lobby.users_by_channel.get(&channel_id) else {
    return Vec::new();
  };
  let user_ids = users.iter().map(|user| user.user_id).collect::<HashSet<_>>();

  lobby
    .screen_shares
    .iter()
    .filter(|share| user_ids.contains(&share.sharer_user_id))
    .map(|share| ChannelScreenShare {
      share,
      user: users.iter().find(|user| user.user_id == share.sharer_user_id),
    })
    .collect()
}

fn watched_stream_for_channel(lobby: &LobbyState, channel_id: ChannelId) -> Option<ChannelScreenShare<'_>> {
  let watching_user_id = lobby.watching_user_id?;
  screen_shares_for_channel(lobby, channel_id)
    .into_iter()
    .find(|stream| stream.share.sharer_user_id == watching_user_id)
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

fn stream_browser(
  ctx: &mut Ctx,
  channel: &LobbyChannel,
  local_user_id: UserId,
  lobby: &LobbyState,
  start_stream_modal_open: Signal<bool>,
  stop_stream: &StopStreamAction,
  watch_stream: &WatchStreamAction,
  stop_watching: &StopWatchingAction,
) -> Element {
  let mut streams = screen_shares_for_channel(lobby, channel.id);
  let users = lobby
    .users_by_channel
    .get(&channel.id)
    .map(Vec::as_slice)
    .unwrap_or(&[]);

  if let Some(watching_user_id) = lobby.watching_user_id
    && let Some(watched_index) = streams
      .iter()
      .position(|stream| stream.share.sharer_user_id == watching_user_id)
  {
    let watched_stream = streams.remove(watched_index);
    return stream_watching(
      ctx,
      channel,
      watched_stream,
      streams,
      stop_watching,
      watch_stream,
      lobby.last_error.as_deref(),
    );
  }

  if streams.is_empty() {
    return stream_browser_empty_channel(ctx, users, lobby.last_error.as_deref(), start_stream_modal_open);
  }

  stream_browser_streams(
    ctx,
    channel,
    streams,
    users,
    local_user_id,
    lobby.watching_user_id,
    lobby.last_error.as_deref(),
    start_stream_modal_open,
    stop_stream,
    watch_stream,
  )
}

fn stream_watching(
  ctx: &mut Ctx,
  channel: &LobbyChannel,
  stream: ChannelScreenShare<'_>,
  switch_streams: Vec<ChannelScreenShare<'_>>,
  stop_watching: &StopWatchingAction,
  watch_stream: &WatchStreamAction,
  error: Option<&str>,
) -> Element {
  let sharer_id = stream.share.sharer_user_id;
  let name = stream
    .user
    .map(|user| user.username.clone())
    .unwrap_or_else(|| format!("User #{sharer_id}"));
  let title = ctx.t_args("lobby.stream_browser.watching.screen_name", [("user", name.clone())]);
  let metadata = stream_metadata_label(ctx, stream.share);

  let mut body = Column::new()
    .width(Dimension::Pct(100.0))
    .height(Dimension::Pct(100.0))
    .flex(1.0)
    .spacing(10.0)
    .padding(20.0)
    .child(stream_viewer_placeholder(ctx, &metadata))
    .child(stream_info_bar(ctx, &name, &title, channel, stop_watching));

  if !switch_streams.is_empty() {
    body = body.child(stream_switcher(ctx, switch_streams, watch_stream));
  }

  if let Some(error) = error {
    body = body.child(error_notice(ctx, error));
  }

  body.into()
}

fn stream_viewer_placeholder(ctx: &mut Ctx, metadata: &str) -> Element {
  Stack::new()
    .width(Dimension::Pct(100.0))
    .height(Dimension::Pct(100.0))
    .flex(1.0)
    .rounded(theme::RadiusSize::Lg)
    .background(BackgroundColor::Color(Color::from_hex("#0E0F12")))
    .border_inside(1.0, theme::PaletteColor::Border)
    .child(
      Column::new()
        .width(Dimension::Pct(100.0))
        .height(Dimension::Pct(100.0))
        .align_items(Alignment::Center)
        .justify(Justify::Center)
        .spacing(10.0)
        .child(ctx.mount::<LucideIcon>(LucideIconProps {
          icon: "monitor",
          size: 44.0,
          color: theme::palette().border,
        }))
        .child(
          Text::new(metadata)
            .variant(theme::TypographyStyle::Mono)
            .color(theme::PaletteColor::TextMuted),
        ),
    )
    .child(viewer_live_badge(ctx).absolute(16.0, 16.0, 76.0, 24.0))
    .into()
}

fn viewer_live_badge(ctx: &mut Ctx) -> Row {
  Row::new()
    .width(76.0)
    .height(24.0)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .spacing(5.0)
    .padding_vertical(4.0)
    .padding_horizontal(9.0)
    .rounded(theme::RadiusSize::Sm)
    .background(BackgroundColor::Color(Color::from_hex("#2A1A1C")))
    .child(
      Row::new()
        .width(6.0)
        .height(6.0)
        .rounded(3.0)
        .background(BackgroundColor::Color(Color::from_hex("#FF6B5F"))),
    )
    .child(
      Text::new(&ctx.t("lobby.stream_browser.watching.live"))
        .variant(theme::TypographyStyle::FieldLabel)
        .color(theme::PaletteColor::Danger),
    )
}

fn stream_info_bar(
  ctx: &mut Ctx,
  name: &str,
  title: &str,
  channel: &LobbyChannel,
  stop_watching: &StopWatchingAction,
) -> Element {
  Row::new()
    .width(Dimension::Pct(100.0))
    .align_items(Alignment::Center)
    .spacing(10.0)
    .padding_vertical(10.0)
    .padding_horizontal(12.0)
    .child(server_avatar(name, 26.0, false))
    .child(
      Row::new()
        .width(Dimension::Pct(100.0))
        .flex(1.0)
        .align_items(Alignment::Center)
        .spacing(6.0)
        .child(
          Text::new(title)
            .variant(theme::TypographyStyle::Button)
            .color(theme::PaletteColor::TextPrimary),
        )
        .child(
          Text::new(&ctx.t_args(
            "lobby.stream_browser.watching.display",
            [("channel", channel.name.clone())],
          ))
          .variant(theme::TypographyStyle::Caption)
          .color(theme::PaletteColor::TextMuted),
        ),
    )
    .child(stream_icon_button(ctx, "volume-2"))
    .child(stream_icon_button(ctx, "layout-grid"))
    .child(stop_watching_button(ctx, stop_watching))
    .into()
}

fn stream_icon_button(ctx: &mut Ctx, icon: &'static str) -> Element {
  Row::new()
    .width(36.0)
    .height(36.0)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .rounded(theme::RadiusSize::Md)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised))
    .border_inside(1.0, theme::PaletteColor::Border)
    .child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon,
      size: 16.0,
      color: theme::palette().text_secondary,
    }))
    .into()
}

fn stop_watching_button(ctx: &mut Ctx, stop_watching: &StopWatchingAction) -> Element {
  let pending = stop_watching.state().get().is_pending();
  let action = stop_watching.clone();
  let mut button = dark_top_bar_button(ctx, "eye-off", "lobby.stream_browser.watching.stop");

  if !pending {
    button = button.on_click(move |_| action.run(()));
  }

  button.into()
}

fn stream_browser_empty_channel(
  ctx: &mut Ctx,
  users: &[LobbyUser],
  error: Option<&str>,
  start_stream_modal_open: Signal<bool>,
) -> Element {
  let mut body = Column::new()
    .width(Dimension::Pct(100.0))
    .height(Dimension::Pct(100.0))
    .flex(1.0)
    .justify(Justify::SpaceBetween)
    .spacing(theme::SpacingSize::Lg)
    .padding(20.0)
    .child(stream_user_tiles(ctx, users, &[]))
    .child(stream_share_bar(ctx, start_stream_modal_open));

  if let Some(error) = error {
    body = body.child(error_notice(ctx, error));
  }

  body.into()
}

fn stream_user_tiles(ctx: &mut Ctx, users: &[LobbyUser], streaming_user_ids: &[UserId]) -> Element {
  let visible = users.iter().take(3).collect::<Vec<_>>();
  let mut row = Row::new().width(Dimension::Pct(100.0)).spacing(16.0);

  if visible.is_empty() {
    row = row.child(stream_user_tile_placeholder(ctx));
  } else {
    for user in visible {
      row = row.child(stream_user_tile(ctx, user, streaming_user_ids.contains(&user.user_id)));
    }
  }

  row.into()
}

fn stream_user_tile(ctx: &mut Ctx, user: &LobbyUser, streaming: bool) -> Element {
  let speaking = user.speaking && !user.muted && !user.deafened;
  let mut name_row = Row::new()
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .spacing(5.0);

  name_row = name_row.child(
    Text::new(&user.username)
      .variant(theme::TypographyStyle::Button)
      .color(theme::PaletteColor::TextPrimary),
  );

  if speaking {
    name_row = name_row.child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon: "mic",
      size: 12.0,
      color: theme::palette().success,
    }));
  }

  Stack::new()
    .width(260.0)
    .height(150.0)
    .rounded(8.0)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfacePanel))
    .border_inside(
      1.0,
      if speaking {
        theme::PaletteColor::Success
      } else {
        theme::PaletteColor::Border
      },
    )
    .child(
      Column::new()
        .width(Dimension::Pct(100.0))
        .height(Dimension::Pct(100.0))
        .align_items(Alignment::Center)
        .justify(Justify::Center)
        .spacing(theme::SpacingSize::Lg)
        .child(stream_user_avatar(&user.username, speaking, 50.0))
        .child(
          Column::new()
            .align_items(Alignment::Center)
            .spacing(4.0)
            .child(name_row)
            .child(
              Text::new(user_role_label(user.role))
                .variant(theme::TypographyStyle::Caption)
                .color(theme::PaletteColor::TextMuted),
            ),
        ),
    )
    .child(stream_role_badge(ctx, user.role).absolute(12.0, 12.0, 24.0, 24.0))
    .child(stream_user_status_badges(ctx, user, streaming).absolute(164.0, 12.0, 84.0, 24.0))
    .into()
}

fn stream_user_avatar(name: &str, active: bool, size: f32) -> Element {
  Row::new()
    .width(size)
    .height(size)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .rounded(size / 2.0)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised))
    .border_inside(
      1.5,
      BackgroundColor::Palette(if active {
        theme::PaletteColor::Success
      } else {
        theme::PaletteColor::Border
      }),
    )
    .child(
      Text::new(&initials_for_user(name))
        .variant(theme::TypographyStyle::Heading)
        .color(if active {
          theme::PaletteColor::TextPrimary
        } else {
          theme::PaletteColor::TextSecondary
        }),
    )
    .into()
}

fn stream_role_badge(ctx: &mut Ctx, role: Role) -> Row {
  let palette = theme::palette();
  let (icon, color, background) = match role {
    Role::Owner => ("shield-check", palette.warning, palette.warning_muted),
    Role::Admin => ("shield", palette.accent, palette.accent_muted),
    Role::Moderator => ("key-round", palette.info, palette.info_muted),
    Role::User => ("user", palette.text_muted, palette.surface_raised),
  };

  Row::new()
    .width(24.0)
    .height(24.0)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .rounded(12.0)
    .background(BackgroundColor::Color(background))
    .border_inside(1.0, theme::PaletteColor::Border)
    .child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon,
      size: 13.0,
      color,
    }))
}

fn stream_user_status_badges(ctx: &mut Ctx, user: &LobbyUser, streaming: bool) -> Row {
  let mut badges = Row::new()
    .width(84.0)
    .height(24.0)
    .align_items(Alignment::Center)
    .justify(Justify::End)
    .spacing(5.0);

  if streaming {
    badges = badges.child(stream_status_badge(ctx, "monitor-up", theme::palette().accent));
  }

  if user.deafened {
    badges = badges
      .child(stream_status_badge(ctx, "headphone-off", theme::palette().danger))
      .child(stream_status_badge(ctx, "mic-off", theme::palette().danger));
  } else if user.muted {
    badges = badges.child(stream_status_badge(ctx, "mic-off", theme::palette().danger));
  }

  badges
}

fn stream_status_badge(ctx: &mut Ctx, icon: &'static str, color: Color) -> Row {
  Row::new()
    .width(22.0)
    .height(22.0)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .rounded(11.0)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised))
    .border_inside(1.0, theme::PaletteColor::Border)
    .child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon,
      size: 12.0,
      color,
    }))
}

fn stream_user_tile_placeholder(ctx: &mut Ctx) -> Element {
  Column::new()
    .width(260.0)
    .height(150.0)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .spacing(theme::SpacingSize::Md)
    .rounded(8.0)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfacePanel))
    .border_inside(1.0, theme::PaletteColor::Border)
    .child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon: "users",
      size: 28.0,
      color: theme::palette().text_muted,
    }))
    .child(
      Text::new(&ctx.t("lobby.users.empty"))
        .variant(theme::TypographyStyle::Caption)
        .color(theme::PaletteColor::TextMuted),
    )
    .into()
}

fn stream_share_bar(ctx: &mut Ctx, start_stream_modal_open: Signal<bool>) -> Element {
  Row::new()
    .width(Dimension::Pct(100.0))
    .align_items(Alignment::Center)
    .spacing(10.0)
    .padding_vertical(14.0)
    .padding_horizontal(16.0)
    .rounded(8.0)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfacePanel))
    .border_inside(1.0, theme::PaletteColor::Border)
    .child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon: "monitor",
      size: 16.0,
      color: theme::palette().text_muted,
    }))
    .child(
      Text::new(&ctx.t("lobby.stream_browser.empty.share_bar"))
        .variant(theme::TypographyStyle::Caption)
        .color(theme::PaletteColor::TextSecondary)
        .width(Dimension::Pct(100.0))
        .flex(1.0),
    )
    .child(start_stream_button(ctx, start_stream_modal_open))
    .into()
}

fn user_role_label(role: Role) -> &'static str {
  match role {
    Role::Owner => "owner",
    Role::Admin => "admin",
    Role::Moderator => "moderator",
    Role::User => "member",
  }
}

fn initials_for_user(name: &str) -> String {
  let initials = name
    .chars()
    .filter(|ch| ch.is_alphanumeric())
    .flat_map(|ch| ch.to_uppercase())
    .take(1)
    .collect::<String>();

  if initials.is_empty() { "?".to_owned() } else { initials }
}

fn stream_browser_streams(
  ctx: &mut Ctx,
  channel: &LobbyChannel,
  streams: Vec<ChannelScreenShare<'_>>,
  users: &[LobbyUser],
  local_user_id: UserId,
  watching_user_id: Option<UserId>,
  error: Option<&str>,
  start_stream_modal_open: Signal<bool>,
  stop_stream: &StopStreamAction,
  watch_stream: &WatchStreamAction,
) -> Element {
  let local_sharing = streams
    .iter()
    .any(|stream| stream.share.sharer_user_id == local_user_id);
  let stream_count = streams.len();

  if stream_count == 1 {
    let mut streams = streams;
    let stream = streams.remove(0);
    let streaming_user_ids = [stream.share.sharer_user_id];
    let mut body = Column::new()
      .width(Dimension::Pct(100.0))
      .height(Dimension::Pct(100.0))
      .flex(1.0)
      .justify(Justify::SpaceBetween)
      .spacing(20.0)
      .padding(20.0)
      .child(
        Column::new()
          .width(Dimension::Pct(100.0))
          .spacing(14.0)
          .child(
            Text::new(&ctx.t_args("lobby.stream_browser.live_label", [("count", stream_count.to_string())]))
              .variant(theme::TypographyStyle::FieldLabel)
              .color(theme::PaletteColor::TextMuted),
          )
          .child(stream_live_row(
            ctx,
            channel,
            stream,
            local_user_id,
            watching_user_id,
            stop_stream,
            watch_stream,
          ))
          .child(stream_user_tiles(ctx, users, &streaming_user_ids)),
      )
      .child(if local_sharing {
        stop_stream_button(ctx, stop_stream)
      } else {
        start_stream_button(ctx, start_stream_modal_open)
      });

    if let Some(error) = error {
      body = body.child(error_notice(ctx, error));
    }

    return body.into();
  }

  let description = ctx.t_args(
    "lobby.stream_browser.picker.description",
    [("count", stream_count.to_string())],
  );
  let mut cards = Row::new().width(Dimension::Pct(100.0)).wrap().spacing(12.0);

  for stream in streams {
    cards = cards.child(stream_card(
      ctx,
      channel,
      stream,
      local_user_id,
      watching_user_id,
      stop_stream,
      watch_stream,
    ));
  }

  let mut body = Column::new()
    .width(Dimension::Pct(100.0))
    .height(Dimension::Pct(100.0))
    .flex(1.0)
    .spacing(32.0)
    .padding_vertical(40.0)
    .padding_horizontal(24.0)
    .child(
      Column::new()
        .width(Dimension::Pct(100.0))
        .align_items(Alignment::Center)
        .spacing(16.0)
        .child(
          Row::new()
            .width(60.0)
            .height(60.0)
            .align_items(Alignment::Center)
            .justify(Justify::Center)
            .rounded(14.0)
            .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised))
            .border_inside(1.0, theme::PaletteColor::Border)
            .child(ctx.mount::<LucideIcon>(LucideIconProps {
              icon: "monitor-play",
              size: 28.0,
              color: theme::palette().text_secondary,
            })),
        )
        .child(
          Text::new(&ctx.t("lobby.stream_browser.picker.title"))
            .variant(theme::TypographyStyle::Title)
            .color(theme::PaletteColor::TextPrimary),
        )
        .child(
          Text::new(&description)
            .variant(theme::TypographyStyle::Description)
            .color(theme::PaletteColor::TextSecondary)
            .text_align(Alignment::Center)
            .width(440.0),
        ),
    )
    .child(
      Column::new()
        .width(Dimension::Pct(100.0))
        .spacing(12.0)
        .child(
          Row::new()
            .width(Dimension::Pct(100.0))
            .align_items(Alignment::Center)
            .justify(Justify::SpaceBetween)
            .child(
              Text::new(&ctx.t("lobby.stream_browser.picker.live_streams"))
                .variant(theme::TypographyStyle::FieldLabel)
                .color(theme::PaletteColor::TextMuted),
            )
            .child(stream_filter_chip(ctx)),
        )
        .child(cards),
    )
    .child(
      Row::new()
        .width(Dimension::Pct(100.0))
        .align_items(Alignment::Center)
        .justify(Justify::End)
        .child(if local_sharing {
          stop_stream_button(ctx, stop_stream)
        } else {
          start_stream_button(ctx, start_stream_modal_open)
        }),
    );

  if let Some(error) = error {
    body = body.child(error_notice(ctx, error));
  }

  body.into()
}

fn stream_live_row(
  ctx: &mut Ctx,
  channel: &LobbyChannel,
  stream: ChannelScreenShare<'_>,
  local_user_id: UserId,
  watching_user_id: Option<UserId>,
  stop_stream: &StopStreamAction,
  watch_stream: &WatchStreamAction,
) -> Element {
  let sharer_id = stream.share.sharer_user_id;
  let name = stream
    .user
    .map(|user| user.username.clone())
    .unwrap_or_else(|| format!("User #{sharer_id}"));
  let local = sharer_id == local_user_id;
  let watching = watching_user_id == Some(sharer_id);
  let title = ctx.t_args("lobby.stream_browser.watching.screen_name", [("user", name.clone())]);
  let metadata = stream_metadata_label(ctx, stream.share);

  Row::new()
    .width(Dimension::Pct(100.0))
    .height(70.0)
    .align_items(Alignment::Center)
    .spacing(12.0)
    .padding_vertical(10.0)
    .padding_horizontal(12.0)
    .rounded(8.0)
    .background(BackgroundColor::Color(Color::from_hex("#15171A")))
    .border_inside(1.0, theme::PaletteColor::Border)
    .child(stream_thumb(ctx))
    .child(
      Column::new()
        .width(Dimension::Pct(100.0))
        .flex(1.0)
        .spacing(3.0)
        .child(
          Row::new()
            .align_items(Alignment::Center)
            .spacing(8.0)
            .child(
              Text::new(&title)
                .variant(theme::TypographyStyle::Button)
                .color(theme::PaletteColor::TextPrimary),
            )
            .child(if local { local_badge(ctx) } else { Row::new().into() }),
        )
        .child(
          Text::new(&format!(
            "{} · {}",
            ctx.t_args("lobby.stream_browser.list.channel", [("channel", channel.name.clone())],),
            metadata
          ))
          .variant(theme::TypographyStyle::Caption)
          .color(theme::PaletteColor::TextMuted),
        ),
    )
    .child(if local {
      stop_stream_button(ctx, stop_stream)
    } else {
      watch_stream_button(ctx, sharer_id, watching, watch_stream)
    })
    .into()
}

fn stream_card(
  ctx: &mut Ctx,
  _channel: &LobbyChannel,
  stream: ChannelScreenShare<'_>,
  local_user_id: UserId,
  watching_user_id: Option<UserId>,
  stop_stream: &StopStreamAction,
  watch_stream: &WatchStreamAction,
) -> Element {
  let sharer_id = stream.share.sharer_user_id;
  let name = stream
    .user
    .map(|user| user.username.clone())
    .unwrap_or_else(|| format!("User #{sharer_id}"));
  let local = sharer_id == local_user_id;
  let watching = watching_user_id == Some(sharer_id);
  let title = ctx.t_args("lobby.stream_browser.watching.screen_name", [("user", name.clone())]);
  let action = watch_stream.clone();

  let mut card = Row::new()
    .width(226.0)
    .height(58.0)
    .align_items(Alignment::Center)
    .spacing(10.0)
    .padding_vertical(10.0)
    .padding_horizontal(12.0)
    .rounded(8.0)
    .background(BackgroundColor::Color(if watching {
      Color::from_hex("#121A23")
    } else {
      Color::from_hex("#15171A")
    }))
    .border_inside(
      1.0,
      if watching {
        theme::PaletteColor::Accent
      } else {
        theme::PaletteColor::Border
      },
    )
    .cursor(CursorIcon::Pointer)
    .hovered_style(Style::new().background(BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised)))
    .child(stream_thumb(ctx))
    .child(
      Column::new()
        .width(Dimension::Pct(100.0))
        .flex(1.0)
        .spacing(2.0)
        .child(
          Row::new()
            .align_items(Alignment::Center)
            .spacing(theme::SpacingSize::Sm)
            .child(
              Text::new(&title)
                .variant(theme::TypographyStyle::Button)
                .color(theme::PaletteColor::TextPrimary),
            )
            .child(if local { local_badge(ctx) } else { Row::new().into() }),
        )
        .child(
          Text::new(&stream_resolution_label(stream.share))
            .variant(theme::TypographyStyle::Mono)
            .color(theme::PaletteColor::TextMuted),
        ),
    )
    .child(if local {
      stream_card_stop_button(ctx, stop_stream)
    } else {
      stream_card_watch_icon(ctx, watching)
    });

  if !local && !watching && !watch_stream.state().get().is_pending() {
    card = card.on_click(move |_| action.run(sharer_id));
  }

  card.into()
}

fn stream_switcher(ctx: &mut Ctx, streams: Vec<ChannelScreenShare<'_>>, watch_stream: &WatchStreamAction) -> Element {
  let mut cards = Row::new().width(Dimension::Pct(100.0)).spacing(12.0);

  for stream in streams.into_iter().take(3) {
    cards = cards.child(stream_switch_card(ctx, stream, watch_stream));
  }

  Column::new()
    .width(Dimension::Pct(100.0))
    .spacing(10.0)
    .child(
      Row::new()
        .align_items(Alignment::Center)
        .spacing(6.0)
        .child(
          Text::new(&ctx.t("lobby.stream_browser.switch.title"))
            .variant(theme::TypographyStyle::FieldLabel)
            .color(theme::PaletteColor::TextMuted),
        )
        .child(
          Text::new(&ctx.t("lobby.stream_browser.switch.hint"))
            .variant(theme::TypographyStyle::Caption)
            .color(theme::PaletteColor::TextMuted),
        ),
    )
    .child(cards)
    .into()
}

fn stream_switch_card(ctx: &mut Ctx, stream: ChannelScreenShare<'_>, watch_stream: &WatchStreamAction) -> Element {
  let sharer_id = stream.share.sharer_user_id;
  let name = stream
    .user
    .map(|user| user.username.clone())
    .unwrap_or_else(|| format!("User #{sharer_id}"));
  let title = ctx.t_args("lobby.stream_browser.watching.screen_name", [("user", name.clone())]);
  let action = watch_stream.clone();
  let pending = watch_stream.state().get().is_pending();
  let mut card = Row::new()
    .width(Dimension::Pct(100.0))
    .height(58.0)
    .align_items(Alignment::Center)
    .spacing(10.0)
    .padding_vertical(10.0)
    .padding_horizontal(12.0)
    .rounded(8.0)
    .background(BackgroundColor::Color(Color::from_hex("#15171A")))
    .border_inside(1.0, theme::PaletteColor::Border)
    .cursor(CursorIcon::Pointer)
    .hovered_style(Style::new().background(BackgroundColor::Color(Color::from_hex("#121A23"))))
    .child(stream_thumb(ctx))
    .child(
      Column::new()
        .width(Dimension::Pct(100.0))
        .flex(1.0)
        .spacing(2.0)
        .child(
          Text::new(&title)
            .variant(theme::TypographyStyle::Button)
            .color(theme::PaletteColor::TextPrimary),
        )
        .child(
          Text::new(&ctx.t("lobby.stream_browser.watching.display_plain"))
            .variant(theme::TypographyStyle::Caption)
            .color(theme::PaletteColor::TextMuted),
        ),
    );

  if !pending {
    card = card.on_click(move |_| action.run(sharer_id));
  }

  card.into()
}

fn stream_thumb(ctx: &mut Ctx) -> Element {
  Stack::new()
    .width(52.0)
    .height(34.0)
    .rounded(4.0)
    .background(BackgroundColor::Color(Color::from_hex("#0B0C0E")))
    .border_inside(1.0, theme::PaletteColor::Border)
    .child(
      Row::new()
        .width(6.0)
        .height(6.0)
        .rounded(3.0)
        .background(BackgroundColor::Color(Color::from_hex("#FF6B5F")))
        .absolute(6.0, 6.0, 6.0, 6.0),
    )
    .child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon: "monitor",
      size: 14.0,
      color: theme::palette().border,
    }))
    .into()
}

fn stream_filter_chip(ctx: &mut Ctx) -> Element {
  Row::new()
    .width(280.0)
    .height(36.0)
    .align_items(Alignment::Center)
    .spacing(8.0)
    .padding_horizontal(12.0)
    .rounded(theme::RadiusSize::Md)
    .background(BackgroundColor::Color(Color::from_hex("#171A1E")))
    .border_inside(1.0, theme::PaletteColor::Border)
    .child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon: "search",
      size: 14.0,
      color: theme::palette().text_muted,
    }))
    .child(
      Text::new(&ctx.t("lobby.stream_browser.picker.filter"))
        .variant(theme::TypographyStyle::Mono)
        .color(theme::PaletteColor::TextMuted),
    )
    .into()
}

fn stream_card_watch_icon(ctx: &mut Ctx, watching: bool) -> Element {
  Row::new()
    .width(28.0)
    .height(28.0)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .rounded(theme::RadiusSize::Md)
    .background(BackgroundColor::Palette(if watching {
      theme::PaletteColor::AccentMuted
    } else {
      theme::PaletteColor::SurfaceRaised
    }))
    .child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon: if watching { "eye" } else { "play" },
      size: 14.0,
      color: if watching {
        theme::palette().accent
      } else {
        theme::palette().text_secondary
      },
    }))
    .into()
}

fn stream_card_stop_button(ctx: &mut Ctx, stop_stream: &StopStreamAction) -> Element {
  let pending = stop_stream.state().get().is_pending();
  let action = stop_stream.clone();
  let mut button = Row::new()
    .width(28.0)
    .height(28.0)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .rounded(theme::RadiusSize::Md)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised))
    .border_inside(1.0, theme::PaletteColor::Border)
    .cursor(CursorIcon::Pointer)
    .hovered_style(Style::new().background(BackgroundColor::Palette(theme::PaletteColor::SurfaceInput)))
    .child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon: "screen-share-off",
      size: 14.0,
      color: theme::palette().text_secondary,
    }));

  if !pending {
    button = button.on_click(move |_| action.run(()));
  }

  button.into()
}

fn stream_resolution_label(stream: &LobbyScreenShare) -> String {
  if stream.metadata.width == 0 || stream.metadata.height == 0 {
    return "Pending".to_owned();
  }

  format!("{}x{}", stream.metadata.width, stream.metadata.height)
}

fn stream_metadata_label(ctx: &mut Ctx, stream: &LobbyScreenShare) -> String {
  let codec = match stream.metadata.codec {
    VideoCodecId::Unknown => ctx.t("lobby.stream_browser.list.pending").to_string(),
    VideoCodecId::Av1 => "AV1".to_owned(),
    VideoCodecId::H265 => "H.265".to_owned(),
    VideoCodecId::H264 => "H.264".to_owned(),
  };

  if stream.metadata.width == 0 || stream.metadata.height == 0 {
    return codec;
  }

  format!("{} x {} · {}", stream.metadata.width, stream.metadata.height, codec)
}

fn local_badge(ctx: &mut Ctx) -> Element {
  Text::new(&ctx.t("lobby.users.you"))
    .variant(theme::TypographyStyle::Caption)
    .color(theme::PaletteColor::TextMuted)
    .into()
}

fn start_stream_button(ctx: &mut Ctx, start_stream_modal_open: Signal<bool>) -> Element {
  let open = start_stream_modal_open.clone();
  let mut button = Row::new()
    .height(34.0)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .spacing(theme::SpacingSize::Sm)
    .padding_horizontal(theme::SpacingSize::Lg)
    .rounded(theme::RadiusSize::Md)
    .background(BackgroundColor::Palette(theme::PaletteColor::TextPrimary))
    .border_inside(1.0, theme::PaletteColor::TextPrimary)
    .cursor(CursorIcon::Pointer)
    .hovered_style(Style::new().background(BackgroundColor::Palette(theme::PaletteColor::TextSecondary)))
    .child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon: "screen-share",
      size: 16.0,
      color: theme::palette().text_inverse,
    }))
    .child(
      Text::new(&ctx.t("lobby.stream_browser.empty.start"))
        .variant(theme::TypographyStyle::Button)
        .color(theme::PaletteColor::TextInverse),
    );

  button = button.on_click(move |_| open.set(true));

  button.into()
}

fn stop_stream_button(ctx: &mut Ctx, stop_stream: &StopStreamAction) -> Element {
  let pending = stop_stream.state().get().is_pending();
  let action = stop_stream.clone();
  let mut button = Row::new()
    .height(34.0)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .spacing(theme::SpacingSize::Sm)
    .padding_horizontal(theme::SpacingSize::Lg)
    .rounded(theme::RadiusSize::Md)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceInput))
    .border_inside(1.0, theme::PaletteColor::Border)
    .cursor(CursorIcon::Pointer)
    .hovered_style(Style::new().background(BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised)))
    .child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon: "screen-share-off",
      size: 16.0,
      color: theme::palette().text_secondary,
    }))
    .child(
      Text::new(&ctx.t("lobby.stream_browser.list.stop"))
        .variant(theme::TypographyStyle::Button)
        .color(theme::PaletteColor::TextSecondary),
    );

  if !pending {
    button = button.on_click(move |_| action.run(()));
  }

  button.into()
}

fn watch_stream_button(ctx: &mut Ctx, sharer_id: UserId, watching: bool, watch_stream: &WatchStreamAction) -> Element {
  let pending = watch_stream.state().get().is_pending();
  let action = watch_stream.clone();
  let mut button = Row::new()
    .height(34.0)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .spacing(theme::SpacingSize::Sm)
    .padding_horizontal(theme::SpacingSize::Lg)
    .rounded(theme::RadiusSize::Md)
    .background(BackgroundColor::Palette(if watching {
      theme::PaletteColor::SurfaceInput
    } else {
      theme::PaletteColor::Accent
    }))
    .border_inside(
      1.0,
      if watching {
        theme::PaletteColor::Border
      } else {
        theme::PaletteColor::Accent
      },
    )
    .cursor(CursorIcon::Pointer)
    .hovered_style(Style::new().background(BackgroundColor::Palette(if watching {
      theme::PaletteColor::SurfaceInput
    } else {
      theme::PaletteColor::AccentHover
    })))
    .child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon: if watching { "eye" } else { "play" },
      size: 16.0,
      color: if watching {
        theme::palette().text_secondary
      } else {
        theme::palette().text_inverse
      },
    }))
    .child(
      Text::new(&ctx.t(if watching {
        "lobby.stream_browser.list.watching"
      } else {
        "lobby.stream_browser.list.watch"
      }))
      .variant(theme::TypographyStyle::Button)
      .color(if watching {
        theme::PaletteColor::TextSecondary
      } else {
        theme::PaletteColor::TextInverse
      }),
    );

  if !pending && !watching {
    button = button.on_click(move |_| action.run(sharer_id));
  }

  button.into()
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
