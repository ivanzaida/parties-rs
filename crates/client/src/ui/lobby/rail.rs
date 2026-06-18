use std::sync::Arc;

use lurq::{
  app::{
    component::{Component, ComponentInfo, DevtoolsFormatter, DevtoolsInspectable},
    ctx::Ctx,
  },
  components::{Column, Rect, Row, ScrollVertical, Text},
  core::{Signal, Store},
  layout::{
    Alignment,
    layout_kind::Justify,
    scrollbar::{ScrollBarPlacement, ScrollBarStyle},
  },
  node::{BackgroundColor, CursorIcon, Element, Style, border::Border, color::Color, dimension::Dimension},
};
use parking_lot::Mutex;
use tokio::sync::{Mutex as AsyncMutex, watch};

use super::{StopStreamAction, WatchStreamAction};
use crate::{
  network::protocol::Role,
  routes::{ROUTE_CHOOSE_SERVER, ROUTE_SERVER_SETTINGS},
  services::voice_controls::{VoiceControlAction, apply_voice_control},
  session::{ConnectedServerInfo, LobbyConnectionWarningKind, LobbySnapshot, ServerSession},
  storage::{AppSettings, Storage},
  theme,
  ui::{
    common::lucide_icon::{LucideIcon, LucideIconProps},
    lobby::{
      debug_channels::{DebugChannels, DebugChannelsProps, SelectDebugChatAction},
      layout::{RAIL_DIVIDER_WIDTH, lobby_layout_metrics},
      model::{LobbyRailModel, lobby_rail_model},
      text_channels::{SelectTextChannelAction, TextChannels, TextChannelsProps},
      voice_channels::{JoinChannelAction, JoinChannelRequest, VoiceChannelActions, VoiceChannels, VoiceChannelsProps},
    },
    settings::SettingsPopupHandle,
  },
};

type VoiceControlTask = lurq::app::ctx::FutureAction<VoiceControlAction, (), String>;

#[derive(Clone)]
struct VoiceControlFuture {
  session: ServerSession,
  task: VoiceControlTask,
}

impl VoiceControlFuture {
  fn run(&self, action: VoiceControlAction) {
    if action == VoiceControlAction::LeaveChannel {
      self.session.leave_channel_locally();
    }
    self.task.run(action);
  }
}

#[derive(Clone)]
pub(super) struct RailStreamActions {
  pub start_stream_modal_open: Signal<bool>,
  pub stop_stream: StopStreamAction,
  pub watch_stream: WatchStreamAction,
}

#[derive(Clone)]
pub(super) struct LobbyRailProps {
  pub info: ConnectedServerInfo,
  pub debug_mode_enabled: bool,
  pub session: ServerSession,
  pub storage: Option<Storage>,
  pub settings_popup: Option<SettingsPopupHandle>,
  pub stream_actions: RailStreamActions,
}

impl PartialEq for LobbyRailProps {
  fn eq(&self, other: &Self) -> bool {
    self.info == other.info
      && self.debug_mode_enabled == other.debug_mode_enabled
      && self.session.info().map(|info| info.address) == other.session.info().map(|info| info.address)
      && self.storage.is_some() == other.storage.is_some()
      && self.settings_popup.is_some() == other.settings_popup.is_some()
  }
}

impl DevtoolsInspectable for LobbyRailProps {
  fn inspect(&self, formatter: &mut DevtoolsFormatter<'_>) {
    formatter.buffer_mut().push(ComponentInfo::with_value(
      "server_name",
      std::any::type_name::<String>(),
      self.info.server_name.clone(),
    ));
  }
}

pub(super) struct LobbyRail {
  model_store: Store<Option<LobbyRailModel>>,
}

impl Component for LobbyRail {
  type Props = LobbyRailProps;

  fn create(ctx: &mut Ctx) -> Self {
    Self {
      model_store: ctx.store(None),
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let props = ctx.props::<Self::Props>().clone();
    let join_channel = join_channel_action(ctx, props.session.clone(), props.storage.clone());
    let voice_control = voice_control_action(ctx, props.session.clone());
    let select_text_channel = SelectTextChannelAction::new(props.session.clone());
    let select_debug_chat = SelectDebugChatAction::new(props.session.clone());
    let local_voice_state = props.session.local_voice_state();
    ctx.provide(self.model_store.clone());
    let subscriber = ctx.mount::<LobbyRailModelSubscriber>(LobbyRailModelSubscriberProps {
      info: props.info.clone(),
      session: props.session.clone(),
    });
    let Some(model) = self.model_store.get() else {
      return empty_rail(ctx, subscriber);
    };

    rail(
      ctx,
      subscriber,
      &model,
      props.debug_mode_enabled,
      props.settings_popup,
      &props.stream_actions,
      props.storage,
      Some(props.session),
      local_voice_state,
      Some(select_text_channel),
      Some(select_debug_chat),
      Some(&join_channel),
      Some(&voice_control),
    )
  }
}

#[derive(Clone)]
struct LobbyRailModelSubscriberProps {
  info: ConnectedServerInfo,
  session: ServerSession,
}

impl PartialEq for LobbyRailModelSubscriberProps {
  fn eq(&self, other: &Self) -> bool {
    self.info == other.info
      && self.session.info().map(|info| info.address) == other.session.info().map(|info| info.address)
  }
}

impl DevtoolsInspectable for LobbyRailModelSubscriberProps {}

struct LobbyRailModelSubscriber {
  generation: Signal<u64>,
  applied_generation: Signal<Option<u64>>,
  receiver: Mutex<Option<Arc<AsyncMutex<watch::Receiver<LobbySnapshot>>>>>,
}

impl Component for LobbyRailModelSubscriber {
  type Props = LobbyRailModelSubscriberProps;

  fn create(ctx: &mut Ctx) -> Self {
    Self {
      generation: ctx.signal(0),
      applied_generation: ctx.signal(None),
      receiver: Mutex::new(None),
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let props = ctx.props::<Self::Props>().clone();
    let Some(model_store) = ctx.use_context::<Store<Option<LobbyRailModel>>>() else {
      return empty_subscriber_node();
    };

    apply_rail_model(&model_store, lobby_rail_model(&props.info, &props.session.lobby()));

    let receiver = {
      let mut receiver = self.receiver.lock();
      receiver
        .get_or_insert_with(|| Arc::new(AsyncMutex::new(props.session.subscribe_lobby_updates())))
        .clone()
    };
    let session = props.session.clone();
    let info = props.info.clone();
    let wait_generation = self.generation.get();
    let update = ctx.future(wait_generation, move |wait_generation| {
      let receiver = receiver.clone();
      let session = session.clone();
      let info = info.clone();
      async move {
        let mut receiver = receiver.lock().await;
        let snapshot = match receiver.changed().await {
          Ok(()) => receiver.borrow().clone(),
          Err(_) => LobbySnapshot {
            generation: wait_generation,
            lobby: session.lobby(),
          },
        };
        Ok::<_, String>((snapshot.generation, Some(lobby_rail_model(&info, &snapshot.lobby))))
      }
    });
    let state = update.state().get();
    if state.is_fulfilled()
      && let Some((snapshot_generation, Some(model))) = state.data
      && self.applied_generation.get_untracked() != Some(snapshot_generation)
    {
      apply_rail_model(&model_store, model);
      self.applied_generation.set(Some(snapshot_generation));
      self.generation.set(wait_generation.wrapping_add(1));
    }

    empty_subscriber_node()
  }
}

fn apply_rail_model(model_store: &Store<Option<LobbyRailModel>>, model: LobbyRailModel) {
  if model_store.with(|current| current.as_ref() != Some(&model)) {
    model_store.set(Some(model));
  }
}

fn join_channel_action(ctx: &mut Ctx, session: ServerSession, storage: Option<Storage>) -> JoinChannelAction {
  let no_connected_server = ctx.t("lobby.error.no_connected_server").to_string();
  let task_session = session.clone();
  let task = ctx.future_action(move |request: JoinChannelRequest| {
    let session = task_session.clone();
    let storage = storage.clone();
    let no_connected_server = no_connected_server.clone();
    async move {
      let channel_id = request.channel_id;
      let server = session.server().ok_or(no_connected_server.clone())?;
      let settings = storage
        .as_ref()
        .and_then(|storage| storage.load_settings().ok())
        .unwrap_or_else(AppSettings::default);
      let already_in_voice = request.previous_channel_id.is_some();
      let (mut muted, deafened) = session.local_voice_state().unwrap_or((false, false));
      if !already_in_voice {
        muted = settings.start_muted_when_joining || deafened;
      }
      tracing::info!(target: "lobby", 
        "[lobby] join channel requested: channel={channel_id} already_in_voice={already_in_voice} muted={muted} deafened={deafened}"
      );
      if let Err(error) = server.join_channel(channel_id).await {
        match request.previous_channel_id {
          Some(previous_channel_id) => session.select_channel(previous_channel_id),
          None => session.clear_channel_selection_locally(),
        }
        return Err(error.to_string());
      }
      tracing::info!(target: "lobby", "[lobby] join channel accepted: channel={channel_id}");
      session.play_voice_join_notification();
      server
        .update_voice_state(muted, deafened)
        .await
        .map_err(|error| error.to_string())?;
      tracing::info!(target: "voice", 
        "[voice] local voice state announced after join: channel={channel_id} muted={muted} deafened={deafened}"
      );
      session.set_local_voice_state(muted, deafened);
      match session.start_voice(settings.clone(), &no_connected_server) {
        Ok(()) => {
          tracing::info!(target: "voice", "[voice] local voice engine started after join");
          session.queue_voice_join_sound_to_channel(&settings);
        }
        Err(error) => tracing::warn!(target: "voice", "[voice] local voice engine failed after join: {error}"),
      }
      Ok(())
    }
  });
  JoinChannelAction::new(session, task)
}

fn voice_control_action(ctx: &mut Ctx, session: ServerSession) -> VoiceControlFuture {
  let no_connected_server = ctx.t("lobby.error.no_connected_server").to_string();
  let task_session = session.clone();
  let task = ctx.future_action(move |control| {
    let session = task_session.clone();
    let no_connected_server = no_connected_server.clone();
    async move { apply_voice_control(session, control, no_connected_server).await }
  });
  VoiceControlFuture { session, task }
}

fn rail(
  ctx: &mut Ctx,
  subscriber: Element,
  model: &LobbyRailModel,
  debug_mode_enabled: bool,
  settings_popup: Option<SettingsPopupHandle>,
  stream_actions: &RailStreamActions,
  storage: Option<Storage>,
  leave_session: Option<ServerSession>,
  local_voice_state: Option<(bool, bool)>,
  select_text_channel: Option<SelectTextChannelAction>,
  select_debug_chat: Option<SelectDebugChatAction>,
  join_channel: Option<&JoinChannelAction>,
  voice_control: Option<&VoiceControlFuture>,
) -> Element {
  let metrics = lobby_layout_metrics(ctx);
  Row::new()
    .border_right(Border::inside(1.0, theme::PaletteColor::Border))
    .width(metrics.rail_width)
    .height(Dimension::Pct(100.0))
    .background(BackgroundColor::Color(Color::from_hex("#0C0D0F")))
    .child(subscriber)
    .child(
      Column::new()
        .width(metrics.rail_width - RAIL_DIVIDER_WIDTH)
        .height(Dimension::Pct(100.0))
        .background(BackgroundColor::Color(Color::from_hex("#0C0D0F")))
        .child(rail_header(ctx, model, debug_mode_enabled, leave_session.clone()))
        .child(rail_channels(
          ctx,
          model,
          debug_mode_enabled,
          storage,
          leave_session.clone(),
          select_text_channel,
          select_debug_chat,
          join_channel,
          &stream_actions.watch_stream,
        ))
        .child(rail_bottom(
          ctx,
          model,
          debug_mode_enabled,
          settings_popup,
          stream_actions,
          local_voice_state,
          voice_control,
        )),
    )
    .child(
      Rect::new(RAIL_DIVIDER_WIDTH, 1.0)
        .height(Dimension::Pct(100.0))
        .background(BackgroundColor::Palette(theme::PaletteColor::Border)),
    )
    .into()
}

fn empty_rail(ctx: &mut Ctx, subscriber: Element) -> Element {
  let metrics = lobby_layout_metrics(ctx);
  Row::new()
    .border_right(Border::inside(1.0, theme::PaletteColor::Border))
    .width(metrics.rail_width)
    .height(Dimension::Pct(100.0))
    .background(BackgroundColor::Color(Color::from_hex("#0C0D0F")))
    .child(subscriber)
    .into()
}

fn empty_subscriber_node() -> Element {
  Rect::new(0.0, 0.0).into()
}

fn rail_header(
  ctx: &mut Ctx,
  model: &LobbyRailModel,
  debug_user_ids: bool,
  leave_session: Option<ServerSession>,
) -> Element {
  let unknown_server = ctx.t("lobby.server.unknown");
  let server_name = server_name(&model.server_name, unknown_server.as_ref());
  let user_label = local_user_label(ctx, model, debug_user_ids);
  let role = ctx.t(role_label_lower_key(model.role));
  let sub = ctx.t_args(
    "lobby.rail.user_meta",
    [("user", user_label.clone()), ("role", role.to_string())],
  );

  let mut row = Row::new()
    .width(Dimension::Pct(100.0))
    .align_items(Alignment::Center)
    .spacing(theme::SpacingSize::Md)
    .padding(14.0)
    .background(BackgroundColor::Color(Color::from_hex("#0E0F11")))
    .border_bottom(Border::inside(1.0, theme::PaletteColor::Border))
    .child(server_avatar(server_name, 36.0, true))
    .child(
      Column::new()
        .width(Dimension::Pct(100.0))
        .flex(1.0)
        .spacing(theme::SpacingSize::Xs)
        .child(Text::new(server_name).variant(theme::TypographyStyle::Heading))
        .child(
          Text::new(&sub)
            .variant(theme::TypographyStyle::Caption)
            .color(theme::PaletteColor::TextMuted),
        ),
    );

  if model.role.can_edit_server_settings() {
    row = row.child(server_settings_button(ctx));
  }

  row.child(leave_button(ctx, leave_session)).into()
}

fn server_settings_button(ctx: &mut Ctx) -> Element {
  let navigator = ctx.navigator();
  Row::new()
    .width(30.0)
    .height(30.0)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .rounded(theme::RadiusSize::Lg)
    .cursor(CursorIcon::Pointer)
    .hovered_style(Style::new().background(BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised)))
    .child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon: "settings",
      size: 17.0,
      color: theme::palette().text_muted,
    }))
    .on_click(move |_| {
      if let Some(navigator) = navigator.as_ref() {
        navigator.push(ROUTE_SERVER_SETTINGS);
      }
    })
    .into()
}

fn leave_button(ctx: &mut Ctx, session: Option<ServerSession>) -> Element {
  let navigator = ctx.navigator();
  let mut button = Row::new()
    .width(30.0)
    .height(30.0)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .rounded(theme::RadiusSize::Lg)
    .cursor(CursorIcon::Pointer)
    .hovered_style(Style::new().background(BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised)))
    .child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon: "log-out",
      size: 17.0,
      color: theme::palette().text_muted,
    }));

  if let Some(session) = session {
    button = button.on_click(move |_| {
      session.disconnect();
      if let Some(navigator) = navigator.as_ref() {
        navigator.replace(ROUTE_CHOOSE_SERVER);
      }
    });
  }

  button.into()
}

fn rail_channels(
  ctx: &mut Ctx,
  model: &LobbyRailModel,
  debug_mode_enabled: bool,
  storage: Option<Storage>,
  session: Option<ServerSession>,
  select_text_channel: Option<SelectTextChannelAction>,
  select_debug_chat: Option<SelectDebugChatAction>,
  join_channel: Option<&JoinChannelAction>,
  watch_stream: &WatchStreamAction,
) -> Element {
  let metrics = lobby_layout_metrics(ctx);
  let mut channels = Column::new()
    .width(Dimension::Pct(100.0))
    .spacing(18.0)
    .padding_vertical(metrics.rail_padding_y)
    .padding_horizontal(metrics.rail_padding_x)
    .child(ctx.mount::<TextChannels>(TextChannelsProps {
      channels: model.text_channels.clone(),
      select_channel: select_text_channel,
    }));

  channels = channels.child(ctx.mount::<VoiceChannels>(VoiceChannelsProps {
    channels: model.voice_channels.clone(),
    disconnected: model.disconnected,
    local_user_id: model.user_id,
    local_role: model.role,
    debug_user_ids: debug_mode_enabled,
    storage,
    actions: VoiceChannelActions {
      session,
      join_channel: join_channel.cloned(),
      watch_stream: Some(watch_stream.clone()),
    },
  }));

  if debug_mode_enabled {
    channels = channels.child(ctx.mount::<DebugChannels>(DebugChannelsProps {
      selected: model.debug_chat_selected,
      select_debug_chat,
    }));
  }

  ScrollVertical::new(channels)
    .width(Dimension::Pct(100.0))
    .height(Dimension::Pct(100.0))
    .flex(1.0)
    .scrollbar(rail_scrollbar_style())
    .scrollbar_hovered(|mut style| {
      let palette = theme::palette();
      style.thumb_color = palette.accent_hover;
      style.track_color = palette.surface_input.with_opacity(0.7);
      style
    })
    .into()
}

fn rail_scrollbar_style() -> ScrollBarStyle {
  let palette = theme::palette();
  ScrollBarStyle {
    width: 6.0,
    min_thumb_length: 28.0,
    track_color: palette.surface_input.with_opacity(0.35),
    thumb_color: palette.border_strong,
    thumb_radius: 3.0,
    track_radius: 3.0,
    padding: 2.0,
    placement: ScrollBarPlacement::Overlay,
    ..ScrollBarStyle::default()
  }
}

fn rail_bottom(
  ctx: &mut Ctx,
  model: &LobbyRailModel,
  debug_user_ids: bool,
  settings_popup: Option<SettingsPopupHandle>,
  stream_actions: &RailStreamActions,
  local_voice_state: Option<(bool, bool)>,
  voice_control: Option<&VoiceControlFuture>,
) -> Element {
  let metrics = lobby_layout_metrics(ctx);
  Column::new()
    .width(Dimension::Pct(100.0))
    .spacing(theme::SpacingSize::Md)
    .padding_vertical(metrics.rail_padding_y)
    .padding_horizontal(metrics.rail_padding_x)
    .child(connection_status(ctx, model))
    .child(local_user_row(ctx, model, debug_user_ids))
    .child(control_row(
      ctx,
      model,
      stream_actions.start_stream_modal_open.clone(),
      settings_popup,
      &stream_actions.stop_stream,
      local_voice_state,
      voice_control,
    ))
    .into()
}

fn connection_status(ctx: &mut Ctx, model: &LobbyRailModel) -> Element {
  let selected = model.selected_voice_channel.as_ref();
  let warning = (!model.disconnected)
    .then_some(model.connection_warning.as_ref())
    .flatten();
  let fallback_title = ctx.t("lobby.connection.not_in_channel");
  let connected_to_voice = selected.is_some() && !model.disconnected;
  let title = if let Some(warning) = warning {
    ctx.t(connection_warning_title_key(&warning.kind)).to_string()
  } else if model.disconnected {
    ctx.t("lobby.status.disconnected").to_string()
  } else if connected_to_voice {
    ctx.t("lobby.connection.voice_connected").to_string()
  } else {
    fallback_title.to_string()
  };
  let sub = if let Some(warning) = warning {
    if let Some(channel) = selected {
      ctx
        .t_args(
          "lobby.connection.warning_channel",
          [("channel", channel.name.clone()), ("message", warning.message.clone())],
        )
        .to_string()
    } else {
      warning.message.clone()
    }
  } else if let Some(channel) = selected {
    let status = ctx.t("lobby.connection.connected");
    ctx
      .t_args(
        "lobby.connection.channel_connected",
        [("channel", channel.name.clone()), ("status", status.to_string())],
      )
      .to_string()
  } else if model.disconnected {
    ctx.t("lobby.connection.not_in_channel").to_string()
  } else {
    ctx.t("lobby.connection.connected").to_string()
  };
  let (status_icon, icon_color) = if warning.is_some() {
    ("triangle-alert", theme::palette().warning)
  } else if model.disconnected {
    ("unplug", theme::palette().danger)
  } else if connected_to_voice {
    ("audio-lines", theme::palette().success)
  } else {
    ("audio-lines", theme::palette().text_muted)
  };

  Row::new()
    .width(Dimension::Pct(100.0))
    .align_items(Alignment::Center)
    .spacing(theme::SpacingSize::Md)
    .padding_vertical(10.0)
    .padding_horizontal(12.0)
    .rounded(theme::RadiusSize::Lg)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised))
    .border_inside(1.0, theme::PaletteColor::Border)
    .child(
      Column::new()
        .width(Dimension::Pct(100.0))
        .flex(1.0)
        .spacing(theme::SpacingSize::Xs)
        .child(
          Text::new(&title)
            .variant(theme::TypographyStyle::Button)
            .color(theme::PaletteColor::TextPrimary),
        )
        .child(status_sub(
          if warning.is_some() {
            Some(theme::PaletteColor::Warning)
          } else if connected_to_voice {
            Some(theme::PaletteColor::Success)
          } else {
            None
          },
          &sub,
        )),
    )
    .child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon: status_icon,
      size: 16.0,
      color: icon_color,
    }))
    .into()
}

fn connection_warning_title_key(kind: &LobbyConnectionWarningKind) -> &'static str {
  match kind {
    LobbyConnectionWarningKind::KeepalivePongOverdue => "lobby.connection.warning_keepalive",
    LobbyConnectionWarningKind::VoiceReceiverStopped => "lobby.connection.warning_voice",
    LobbyConnectionWarningKind::VideoReceiverStopped => "lobby.connection.warning_video",
  }
}

fn status_sub(dot_color: Option<theme::PaletteColor>, label: &str) -> Element {
  let mut row = Row::new().align_items(Alignment::Center).spacing(6.0);

  if let Some(dot_color) = dot_color {
    row = row.child(
      Rect::new(7.0, 7.0)
        .rounded(4.0)
        .background(BackgroundColor::Palette(dot_color)),
    );
  }

  row
    .child(
      Text::new(label)
        .variant(theme::TypographyStyle::Caption)
        .color(theme::PaletteColor::TextMuted),
    )
    .into()
}

fn local_user_row(ctx: &mut Ctx, model: &LobbyRailModel, debug_user_ids: bool) -> Element {
  let avatar_name = model.local_user_name.clone().unwrap_or_else(|| {
    let display_name = model.display_name.trim();
    if display_name.is_empty() {
      ctx
        .t_args("lobby.user.fallback", [("id", model.user_id.to_string())])
        .to_string()
    } else {
      display_name.to_owned()
    }
  });
  let username = local_user_label(ctx, model, debug_user_ids);
  let role = ctx.t(role_label_lower_key(model.role));
  let ping_label = model
    .ping_ms
    .map(|ping_ms| ctx.t_args("lobby.rail.ping_ms", [("value", ping_ms.to_string())]));

  let mut row = Row::new()
    .width(Dimension::Pct(100.0))
    .align_items(Alignment::Center)
    .spacing(theme::SpacingSize::Md)
    .padding(4.0)
    .child(local_avatar(&avatar_name))
    .child(
      Column::new()
        .width(Dimension::Pct(100.0))
        .flex(1.0)
        .spacing(1.0)
        .child(
          Text::new(&username)
            .variant(theme::TypographyStyle::Button)
            .color(theme::PaletteColor::TextPrimary),
        )
        .child(
          Text::new(&role)
            .variant(theme::TypographyStyle::Caption)
            .color(theme::PaletteColor::TextMuted),
        ),
    );

  if let Some(ping_label) = ping_label {
    row = row.child(
      Text::new(&ping_label)
        .variant(theme::TypographyStyle::Caption)
        .color(theme::PaletteColor::TextMuted),
    );
  }

  row.into()
}

fn control_row(
  ctx: &mut Ctx,
  model: &LobbyRailModel,
  start_stream_modal_open: Signal<bool>,
  settings_popup: Option<SettingsPopupHandle>,
  stop_stream: &StopStreamAction,
  local_voice_state: Option<(bool, bool)>,
  voice_control: Option<&VoiceControlFuture>,
) -> Element {
  let (muted, deafened) = local_voice_state.unwrap_or(model.local_voice_state);
  let mic_icon = if muted { "mic-off" } else { "mic" };
  let headphones_icon = if deafened { "headphone-off" } else { "headphones" };
  let mute_locked = muted && deafened;
  let connected_to_voice = model.local_user_in_voice && !model.disconnected;
  let local_streaming = model.local_streaming;
  let mut row = Row::new()
    .width(Dimension::Pct(100.0))
    .align_items(Alignment::Center)
    .spacing(8.0);

  row = row.child(control_button(
    ctx,
    mic_icon,
    muted,
    voice_control,
    Some(VoiceControlAction::ToggleMute),
    mute_locked,
  ));
  row = row.child(control_button(
    ctx,
    headphones_icon,
    deafened,
    voice_control,
    Some(VoiceControlAction::ToggleDeafen),
    false,
  ));
  row = row.child(stream_control_button(
    ctx,
    start_stream_modal_open,
    stop_stream,
    local_streaming,
    !connected_to_voice && !local_streaming,
  ));
  row = row.child(control_button(
    ctx,
    "phone-off",
    connected_to_voice,
    voice_control,
    Some(VoiceControlAction::LeaveChannel),
    !connected_to_voice,
  ));

  if let Some(settings_popup) = settings_popup {
    row = row.child(icon_button(ctx, "settings", false, false).on_click(move |_| settings_popup.open()));
  } else {
    row = row.child(icon_button(ctx, "settings", false, false));
  }

  row.into()
}

fn stream_control_button(
  ctx: &mut Ctx,
  start_stream_modal_open: Signal<bool>,
  stop_stream: &StopStreamAction,
  streaming: bool,
  disabled: bool,
) -> Element {
  let open = start_stream_modal_open.clone();
  let pending = stop_stream.state().get().is_pending();
  let mut button = icon_button(
    ctx,
    if streaming { "screen-share-off" } else { "monitor-up" },
    streaming,
    disabled,
  );

  if !disabled && !pending {
    if streaming {
      let stop_stream = stop_stream.clone();
      button = button.on_click(move |_| stop_stream.run(()));
    } else {
      button = button.on_click(move |_| open.set(true));
    }
  }

  button.into()
}

fn control_button(
  ctx: &mut Ctx,
  icon: &'static str,
  active: bool,
  voice_control: Option<&VoiceControlFuture>,
  action: Option<VoiceControlAction>,
  disabled: bool,
) -> Element {
  let mut button = icon_button(ctx, icon, active, disabled);

  if !disabled {
    if let (Some(voice_control), Some(action)) = (voice_control, action) {
      let voice_control = voice_control.clone();
      button = button.on_click(move |_| voice_control.run(action));
    }
  }

  button.into()
}

fn icon_button(ctx: &mut Ctx, icon: &'static str, active: bool, disabled: bool) -> Row {
  let palette = theme::palette();
  let icon_color = if disabled {
    palette.text_muted.with_opacity(0.45)
  } else if active {
    palette.danger
  } else {
    palette.text_secondary
  };

  let mut button = Row::new()
    .width(Dimension::Pct(100.0))
    .height(38.0)
    .flex(1.0)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .rounded(theme::RadiusSize::Lg)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised))
    .border_inside(
      1.0,
      if disabled {
        theme::PaletteColor::Border
      } else if active {
        theme::PaletteColor::Danger
      } else {
        theme::PaletteColor::Border
      },
    )
    .child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon,
      size: 16.0,
      color: icon_color,
    }));

  if !disabled {
    button = button
      .cursor(CursorIcon::Pointer)
      .hovered_style(Style::new().background(BackgroundColor::Palette(theme::PaletteColor::SurfaceInput)));
  }

  button
}

fn local_avatar(name: &str) -> Element {
  Row::new()
    .width(30.0)
    .height(30.0)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .rounded(15.0)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised))
    .border_inside(1.5, BackgroundColor::Palette(theme::PaletteColor::Border))
    .child(
      Text::new(&initials_for(name))
        .variant(theme::TypographyStyle::Mono)
        .color(theme::PaletteColor::TextSecondary),
    )
    .into()
}

pub(super) fn server_avatar(name: &str, size: f32, accent: bool) -> Element {
  Row::new()
    .width(size)
    .height(size)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .rounded(theme::RadiusSize::Lg)
    .background(if accent {
      BackgroundColor::Palette(theme::PaletteColor::Accent)
    } else {
      BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised)
    })
    .child(
      Text::new(&initials_for(name))
        .variant(theme::TypographyStyle::Mono)
        .color(if accent {
          theme::PaletteColor::TextInverse
        } else {
          theme::PaletteColor::TextSecondary
        }),
    )
    .into()
}

fn server_name<'a>(name: &'a str, fallback: &'a str) -> &'a str {
  if name.trim().is_empty() { fallback } else { name }
}

fn local_user_label(ctx: &mut Ctx, model: &LobbyRailModel, debug_user_ids: bool) -> String {
  let name = model.local_user_name.clone().unwrap_or_else(|| {
    let display_name = model.display_name.trim();
    if display_name.is_empty() {
      ctx
        .t_args("lobby.user.fallback", [("id", model.user_id.to_string())])
        .to_string()
    } else {
      display_name.to_owned()
    }
  });
  super::shared::user_display_name(model.user_id, &name, debug_user_ids)
}

fn initials_for(name: &str) -> String {
  let initials = name
    .chars()
    .filter(|ch| ch.is_alphanumeric())
    .flat_map(|ch| ch.to_uppercase())
    .take(2)
    .collect::<String>();

  if initials.is_empty() { "?".to_owned() } else { initials }
}

fn role_label_lower_key(role: Role) -> &'static str {
  match role {
    Role::Owner => "lobby.role_meta.owner",
    Role::Admin => "lobby.role_meta.admin",
    Role::Moderator => "lobby.role_meta.moderator",
    Role::User => "lobby.role_meta.member",
  }
}
