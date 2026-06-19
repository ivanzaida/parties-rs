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

use super::{StopStreamAction, WatchStreamAction};
use crate::{
  network::protocol::{Role, UserId},
  routes::{ROUTE_CHOOSE_SERVER, ROUTE_SERVER_SETTINGS},
  services::voice_controls::{VoiceControlAction, apply_voice_control},
  session::{ConnectedServerInfo, LobbyConnectionWarningKind, ServerSession},
  storage::{AppAudioSettings, Storage, UserAudioPreferences},
  theme,
  ui::{
    common::lucide_icon::{LucideIcon, LucideIconProps},
    lobby::{
      debug_channels::{DebugChannels, DebugChannelsProps, SelectDebugChatAction},
      layout::{RAIL_DIVIDER_WIDTH, lobby_layout_metrics},
      model::{
        RailBottomModel, RailChannelsModel, RailHeaderModel, rail_bottom_model, rail_channels_model, rail_header_model,
      },
      session_identity::{same_optional_session, same_session},
      subscription::{LobbyModelSubscription, apply_current_model, apply_model, current_model},
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

impl PartialEq for VoiceControlFuture {
  fn eq(&self, other: &Self) -> bool {
    same_session(&self.session, &other.session)
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
  pub user_audio_preferences: Option<Store<UserAudioPreferences>>,
  pub settings_popup: Option<SettingsPopupHandle>,
  pub stream_actions: RailStreamActions,
}

impl PartialEq for LobbyRailProps {
  fn eq(&self, other: &Self) -> bool {
    self.info == other.info
      && self.debug_mode_enabled == other.debug_mode_enabled
      && same_session(&self.session, &other.session)
      && self.storage.is_some() == other.storage.is_some()
      && self.user_audio_preferences.is_some() == other.user_audio_preferences.is_some()
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
  header_store: Store<Option<RailHeaderModel>>,
  channels_store: Store<Option<RailChannelsModel>>,
  bottom_store: Store<Option<RailBottomModel>>,
}

impl Component for LobbyRail {
  type Props = LobbyRailProps;

  fn create(ctx: &mut Ctx) -> Self {
    Self {
      header_store: ctx.store(None),
      channels_store: ctx.store(None),
      bottom_store: ctx.store(None),
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let props = ctx.props::<Self::Props>().clone();
    let audio_settings_store = ctx.use_context::<Store<AppAudioSettings>>();
    let join_channel = join_channel_action(ctx, props.session.clone(), audio_settings_store);
    let voice_control = voice_control_action(ctx, props.session.clone());
    let select_text_channel = SelectTextChannelAction::new(props.session.clone());
    let select_debug_chat = SelectDebugChatAction::new(props.session.clone());
    let local_voice_state = props.session.local_voice_state();
    let (header_model, channels_model, bottom_model) = current_model(&props.session, |lobby| {
      (
        rail_header_model(&props.info, lobby),
        rail_channels_model(&props.info, lobby),
        rail_bottom_model(&props.info, lobby),
      )
    });
    apply_model(&self.header_store, header_model);
    apply_model(&self.channels_store, channels_model);
    apply_model(&self.bottom_store, bottom_model);
    let subscribers = RailSubscribers {
      header: ctx.mount::<RailHeaderModelSubscriber>(RailHeaderModelSubscriberProps {
        info: props.info.clone(),
        session: props.session.clone(),
        model_store: self.header_store.clone(),
      }),
      channels: ctx.mount::<RailChannelsModelSubscriber>(RailChannelsModelSubscriberProps {
        info: props.info.clone(),
        session: props.session.clone(),
        model_store: self.channels_store.clone(),
      }),
      bottom: ctx.mount::<RailBottomModelSubscriber>(RailBottomModelSubscriberProps {
        info: props.info.clone(),
        session: props.session.clone(),
        model_store: self.bottom_store.clone(),
      }),
    };

    rail(
      ctx,
      subscribers,
      self.header_store.clone(),
      self.channels_store.clone(),
      self.bottom_store.clone(),
      props.debug_mode_enabled,
      props.settings_popup,
      &props.stream_actions,
      props.storage,
      props.user_audio_preferences,
      Some(props.session),
      local_voice_state,
      Some(select_text_channel),
      Some(select_debug_chat),
      Some(&join_channel),
      Some(&voice_control),
    )
  }
}

fn join_channel_action(
  ctx: &mut Ctx,
  session: ServerSession,
  audio_settings_store: Option<Store<AppAudioSettings>>,
) -> JoinChannelAction {
  let no_connected_server = ctx.t("lobby.error.no_connected_server").to_string();
  let task_session = session.clone();
  let task = ctx.future_action(move |request: JoinChannelRequest| {
    let session = task_session.clone();
    let audio_settings_store = audio_settings_store.clone();
    let no_connected_server = no_connected_server.clone();
    async move {
      let channel_id = request.channel_id;
      let server = session.server().ok_or(no_connected_server.clone())?;
      let settings = audio_settings_store.as_ref().map(Store::get).unwrap_or_default();
      let already_in_voice = request.previous_channel_id.is_some();
      let (mut muted, deafened) = session.local_voice_state().unwrap_or((false, false));
      if !already_in_voice {
        muted = settings.start_muted_when_joining || deafened;
      }
      tracing::debug!(target: "lobby",
        "[lobby] join channel requested: channel={channel_id} already_in_voice={already_in_voice} muted={muted} deafened={deafened}"
      );
      if let Err(error) = server.join_channel(channel_id).await {
        match request.previous_channel_id {
          Some(previous_channel_id) => session.select_channel(previous_channel_id),
          None => session.clear_channel_selection_locally(),
        }
        return Err(error.to_string());
      }
      tracing::debug!(target: "lobby", "[lobby] join channel accepted: channel={channel_id}");
      let summary = session.voice_channel_debug_summary(channel_id);
      tracing::debug!(target: "lobby::voice",
        "[lobby:voice] after join command send: channel={channel_id} selected={:?} local_user={local_user_id:?} local_visible={local_visible} cached_users={} selected_users={}",
        summary.selected_channel_id,
        summary.cached_users,
        summary.selected_users,
        local_user_id = summary.local_user_id,
        local_visible = summary.local_visible,
      );
      session.play_voice_join_notification();
      server
        .update_voice_state(muted, deafened)
        .await
        .map_err(|error| error.to_string())?;
      tracing::debug!(target: "voice",
        "[voice] local voice state announced after join: channel={channel_id} muted={muted} deafened={deafened}"
      );
      session.set_local_voice_state(muted, deafened);
      match session.start_voice(settings.clone(), &no_connected_server) {
        Ok(()) => {
          tracing::debug!(target: "voice", "[voice] local voice engine started after join");
          session.queue_voice_join_sound_to_channel(&settings);
        }
        Err(error) => tracing::debug!(target: "voice", "[voice] local voice engine failed after join: {error}"),
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

struct RailSubscribers {
  header: Element,
  channels: Element,
  bottom: Element,
}

fn rail(
  ctx: &mut Ctx,
  subscribers: RailSubscribers,
  header_store: Store<Option<RailHeaderModel>>,
  channels_store: Store<Option<RailChannelsModel>>,
  bottom_store: Store<Option<RailBottomModel>>,
  debug_mode_enabled: bool,
  settings_popup: Option<SettingsPopupHandle>,
  stream_actions: &RailStreamActions,
  storage: Option<Storage>,
  user_audio_preferences: Option<Store<UserAudioPreferences>>,
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
    .child(subscribers.header)
    .child(subscribers.channels)
    .child(subscribers.bottom)
    .child(
      Column::new()
        .width(metrics.rail_width - RAIL_DIVIDER_WIDTH)
        .height(Dimension::Pct(100.0))
        .background(BackgroundColor::Color(Color::from_hex("#0C0D0F")))
        .child(ctx.mount::<RailHeader>(RailHeaderProps {
          model_store: header_store,
          debug_user_ids: debug_mode_enabled,
          leave_session: leave_session.clone(),
        }))
        .child(ctx.mount::<RailChannels>(RailChannelsProps {
          model_store: channels_store,
          debug_mode_enabled,
          storage,
          user_audio_preferences,
          session: leave_session.clone(),
          select_text_channel,
          select_debug_chat,
          join_channel: join_channel.cloned(),
          watch_stream: stream_actions.watch_stream.clone(),
        }))
        .child(ctx.mount::<RailBottom>(RailBottomProps {
          model_store: bottom_store,
          debug_user_ids: debug_mode_enabled,
          settings_popup,
          start_stream_modal_open: stream_actions.start_stream_modal_open.clone(),
          stop_stream: stream_actions.stop_stream.clone(),
          local_voice_state,
          voice_control: voice_control.cloned(),
        })),
    )
    .child(
      Rect::new(RAIL_DIVIDER_WIDTH, 1.0)
        .height(Dimension::Pct(100.0))
        .background(BackgroundColor::Palette(theme::PaletteColor::Border)),
    )
    .into()
}

fn empty_subscriber_node() -> Element {
  Rect::new(0.0, 0.0).into()
}

#[derive(Clone)]
struct RailHeaderModelSubscriberProps {
  info: ConnectedServerInfo,
  session: ServerSession,
  model_store: Store<Option<RailHeaderModel>>,
}

impl PartialEq for RailHeaderModelSubscriberProps {
  fn eq(&self, other: &Self) -> bool {
    self.info == other.info && same_session(&self.session, &other.session)
  }
}

impl DevtoolsInspectable for RailHeaderModelSubscriberProps {}

struct RailHeaderModelSubscriber {
  subscription: LobbyModelSubscription,
}

impl Component for RailHeaderModelSubscriber {
  type Props = RailHeaderModelSubscriberProps;

  fn create(ctx: &mut Ctx) -> Self {
    Self {
      subscription: LobbyModelSubscription::new(ctx),
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let props = ctx.props::<Self::Props>().clone();
    let info = props.info.clone();
    apply_current_model(&props.model_store, &props.session, |lobby| {
      rail_header_model(&info, lobby)
    });

    let info = props.info.clone();
    if let Some((_snapshot_generation, model)) =
      self
        .subscription
        .next_model(ctx, props.session.clone(), move |snapshot| {
          rail_header_model(&info, &snapshot.lobby)
        })
    {
      apply_model(&props.model_store, model);
    }

    empty_subscriber_node()
  }
}

#[derive(Clone)]
struct RailChannelsModelSubscriberProps {
  info: ConnectedServerInfo,
  session: ServerSession,
  model_store: Store<Option<RailChannelsModel>>,
}

impl PartialEq for RailChannelsModelSubscriberProps {
  fn eq(&self, other: &Self) -> bool {
    self.info == other.info && same_session(&self.session, &other.session)
  }
}

impl DevtoolsInspectable for RailChannelsModelSubscriberProps {}

struct RailChannelsModelSubscriber {
  subscription: LobbyModelSubscription,
}

impl Component for RailChannelsModelSubscriber {
  type Props = RailChannelsModelSubscriberProps;

  fn create(ctx: &mut Ctx) -> Self {
    Self {
      subscription: LobbyModelSubscription::new(ctx),
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let props = ctx.props::<Self::Props>().clone();
    let info = props.info.clone();
    apply_current_model(&props.model_store, &props.session, |lobby| {
      rail_channels_model(&info, lobby)
    });

    let info = props.info.clone();
    if let Some((snapshot_generation, model)) =
      self
        .subscription
        .next_model(ctx, props.session.clone(), move |snapshot| {
          rail_channels_model(&info, &snapshot.lobby)
        })
    {
      log_rail_channels_model(snapshot_generation, &model);
      apply_model(&props.model_store, model);
    }

    empty_subscriber_node()
  }
}

fn log_rail_channels_model(snapshot_generation: u64, model: &RailChannelsModel) {
  let selected_channel_id = model
    .voice_channels
    .iter()
    .find(|channel| channel.selected)
    .map(|channel| channel.channel.id);
  let selected_channel_users = selected_channel_id
    .and_then(|channel_id| {
      model
        .voice_channels
        .iter()
        .find(|channel| channel.channel.id == channel_id)
        .map(|channel| channel.users.len())
    })
    .unwrap_or(0);
  tracing::debug!(
    target: "lobby::voice",
    "[lobby:voice] rail channels model applied: generation={snapshot_generation} selected={selected_channel_id:?} selected_channel_users={} voice_channels={}",
    selected_channel_users,
    model.voice_channels.len()
  );
}

#[derive(Clone)]
struct RailBottomModelSubscriberProps {
  info: ConnectedServerInfo,
  session: ServerSession,
  model_store: Store<Option<RailBottomModel>>,
}

impl PartialEq for RailBottomModelSubscriberProps {
  fn eq(&self, other: &Self) -> bool {
    self.info == other.info && same_session(&self.session, &other.session)
  }
}

impl DevtoolsInspectable for RailBottomModelSubscriberProps {}

struct RailBottomModelSubscriber {
  subscription: LobbyModelSubscription,
}

impl Component for RailBottomModelSubscriber {
  type Props = RailBottomModelSubscriberProps;

  fn create(ctx: &mut Ctx) -> Self {
    Self {
      subscription: LobbyModelSubscription::new(ctx),
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let props = ctx.props::<Self::Props>().clone();
    let info = props.info.clone();
    apply_current_model(&props.model_store, &props.session, |lobby| {
      rail_bottom_model(&info, lobby)
    });

    let info = props.info.clone();
    if let Some((_snapshot_generation, model)) =
      self
        .subscription
        .next_model(ctx, props.session.clone(), move |snapshot| {
          rail_bottom_model(&info, &snapshot.lobby)
        })
    {
      apply_model(&props.model_store, model);
    }

    empty_subscriber_node()
  }
}

#[derive(Clone)]
struct RailHeaderProps {
  model_store: Store<Option<RailHeaderModel>>,
  debug_user_ids: bool,
  leave_session: Option<ServerSession>,
}

impl PartialEq for RailHeaderProps {
  fn eq(&self, other: &Self) -> bool {
    self.debug_user_ids == other.debug_user_ids
      && same_optional_session(self.leave_session.as_ref(), other.leave_session.as_ref())
  }
}

impl DevtoolsInspectable for RailHeaderProps {}

struct RailHeader;

impl Component for RailHeader {
  type Props = RailHeaderProps;

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let props = ctx.props::<Self::Props>().clone();
    let Some(model) = props.model_store.get() else {
      return empty_subscriber_node();
    };
    rail_header(ctx, &model, props.debug_user_ids, props.leave_session)
  }
}

fn rail_header(
  ctx: &mut Ctx,
  model: &RailHeaderModel,
  debug_user_ids: bool,
  leave_session: Option<ServerSession>,
) -> Element {
  let unknown_server = ctx.t("lobby.server.unknown");
  let server_name = server_name(&model.server_name, unknown_server.as_ref());
  let user_label = local_user_label_fields(
    ctx,
    model.user_id,
    &model.display_name,
    model.local_user_name.as_deref(),
    debug_user_ids,
  );
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

#[derive(Clone)]
struct RailChannelsProps {
  model_store: Store<Option<RailChannelsModel>>,
  debug_mode_enabled: bool,
  storage: Option<Storage>,
  user_audio_preferences: Option<Store<UserAudioPreferences>>,
  session: Option<ServerSession>,
  select_text_channel: Option<SelectTextChannelAction>,
  select_debug_chat: Option<SelectDebugChatAction>,
  join_channel: Option<JoinChannelAction>,
  watch_stream: WatchStreamAction,
}

impl PartialEq for RailChannelsProps {
  fn eq(&self, other: &Self) -> bool {
    self.debug_mode_enabled == other.debug_mode_enabled
      && self.storage.is_some() == other.storage.is_some()
      && self.user_audio_preferences.is_some() == other.user_audio_preferences.is_some()
      && same_optional_session(self.session.as_ref(), other.session.as_ref())
      && self.select_text_channel == other.select_text_channel
      && self.select_debug_chat == other.select_debug_chat
      && self.join_channel == other.join_channel
  }
}

impl DevtoolsInspectable for RailChannelsProps {}

struct RailChannels;

impl Component for RailChannels {
  type Props = RailChannelsProps;

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let props = ctx.props::<Self::Props>().clone();
    let Some(model) = props.model_store.get() else {
      return empty_subscriber_node();
    };
    rail_channels(ctx, &model, props)
  }
}

fn rail_channels(ctx: &mut Ctx, model: &RailChannelsModel, props: RailChannelsProps) -> Element {
  let metrics = lobby_layout_metrics(ctx);
  let mut channels = Column::new()
    .width(Dimension::Pct(100.0))
    .spacing(18.0)
    .padding_vertical(metrics.rail_padding_y)
    .padding_horizontal(metrics.rail_padding_x)
    .child(ctx.mount::<TextChannels>(TextChannelsProps {
      channels: model.text_channels.clone(),
      select_channel: props.select_text_channel,
    }));

  channels = channels.child(ctx.mount::<VoiceChannels>(VoiceChannelsProps {
    channels: model.voice_channels.clone(),
    disconnected: model.disconnected,
    local_user_id: model.user_id,
    local_role: model.role,
    debug_user_ids: props.debug_mode_enabled,
    storage: props.storage,
    user_audio_preferences: props.user_audio_preferences,
    actions: VoiceChannelActions {
      session: props.session,
      join_channel: props.join_channel,
      watch_stream: Some(props.watch_stream),
    },
  }));

  if props.debug_mode_enabled {
    channels = channels.child(ctx.mount::<DebugChannels>(DebugChannelsProps {
      selected: model.debug_chat_selected,
      select_debug_chat: props.select_debug_chat,
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

#[derive(Clone)]
struct RailBottomProps {
  model_store: Store<Option<RailBottomModel>>,
  debug_user_ids: bool,
  settings_popup: Option<SettingsPopupHandle>,
  start_stream_modal_open: Signal<bool>,
  stop_stream: StopStreamAction,
  local_voice_state: Option<(bool, bool)>,
  voice_control: Option<VoiceControlFuture>,
}

impl PartialEq for RailBottomProps {
  fn eq(&self, other: &Self) -> bool {
    self.debug_user_ids == other.debug_user_ids
      && self.settings_popup.is_some() == other.settings_popup.is_some()
      && self.local_voice_state == other.local_voice_state
      && self.voice_control == other.voice_control
  }
}

impl DevtoolsInspectable for RailBottomProps {}

struct RailBottom;

impl Component for RailBottom {
  type Props = RailBottomProps;

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let props = ctx.props::<Self::Props>().clone();
    let Some(model) = props.model_store.get() else {
      return empty_subscriber_node();
    };
    rail_bottom(
      ctx,
      &model,
      props.debug_user_ids,
      props.settings_popup,
      props.start_stream_modal_open,
      &props.stop_stream,
      props.local_voice_state,
      props.voice_control.as_ref(),
    )
  }
}

fn rail_bottom(
  ctx: &mut Ctx,
  model: &RailBottomModel,
  debug_user_ids: bool,
  settings_popup: Option<SettingsPopupHandle>,
  start_stream_modal_open: Signal<bool>,
  stop_stream: &StopStreamAction,
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
    .child(ctx.mount::<LocalUserRow>(LocalUserRowProps {
      display_name: model.display_name.clone(),
      user_id: model.user_id,
      role: model.role,
      local_user_name: model.local_user_name.clone(),
      ping_ms: model.ping_ms,
      debug_user_ids,
    }))
    .child(ctx.mount::<ControlRow>(ControlRowProps {
      disconnected: model.disconnected,
      model_local_voice_state: model.local_voice_state,
      local_user_in_voice: model.local_user_in_voice,
      local_streaming: model.local_streaming,
      start_stream_modal_open,
      settings_popup,
      stop_stream: stop_stream.clone(),
      local_voice_state,
      voice_control: voice_control.cloned(),
    }))
    .into()
}

fn connection_status(ctx: &mut Ctx, model: &RailBottomModel) -> Element {
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

#[derive(Clone, PartialEq, Eq)]
struct LocalUserRowProps {
  display_name: String,
  user_id: UserId,
  role: Role,
  local_user_name: Option<String>,
  ping_ms: Option<u32>,
  debug_user_ids: bool,
}

impl DevtoolsInspectable for LocalUserRowProps {}

struct LocalUserRow;

impl Component for LocalUserRow {
  type Props = LocalUserRowProps;

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let props = ctx.props::<Self::Props>().clone();
    local_user_row(ctx, props)
  }
}

fn local_user_row(ctx: &mut Ctx, model: LocalUserRowProps) -> Element {
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
  let username = local_user_label_fields(
    ctx,
    model.user_id,
    &model.display_name,
    model.local_user_name.as_deref(),
    model.debug_user_ids,
  );
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

#[derive(Clone)]
struct ControlRowProps {
  disconnected: bool,
  model_local_voice_state: (bool, bool),
  local_user_in_voice: bool,
  local_streaming: bool,
  start_stream_modal_open: Signal<bool>,
  settings_popup: Option<SettingsPopupHandle>,
  stop_stream: StopStreamAction,
  local_voice_state: Option<(bool, bool)>,
  voice_control: Option<VoiceControlFuture>,
}

impl PartialEq for ControlRowProps {
  fn eq(&self, other: &Self) -> bool {
    self.disconnected == other.disconnected
      && self.model_local_voice_state == other.model_local_voice_state
      && self.local_user_in_voice == other.local_user_in_voice
      && self.local_streaming == other.local_streaming
      && self.settings_popup.is_some() == other.settings_popup.is_some()
      && self.local_voice_state == other.local_voice_state
      && self.voice_control == other.voice_control
  }
}

impl DevtoolsInspectable for ControlRowProps {}

struct ControlRow;

impl Component for ControlRow {
  type Props = ControlRowProps;

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let props = ctx.props::<Self::Props>().clone();
    control_row(ctx, props)
  }
}

fn control_row(ctx: &mut Ctx, props: ControlRowProps) -> Element {
  let (muted, deafened) = props.local_voice_state.unwrap_or(props.model_local_voice_state);
  let mic_icon = if muted { "mic-off" } else { "mic" };
  let headphones_icon = if deafened { "headphone-off" } else { "headphones" };
  let mute_locked = muted && deafened;
  let connected_to_voice = props.local_user_in_voice && !props.disconnected;
  let local_streaming = props.local_streaming;
  let mut row = Row::new()
    .width(Dimension::Pct(100.0))
    .align_items(Alignment::Center)
    .spacing(8.0);

  row = row.child(control_button(
    ctx,
    mic_icon,
    muted,
    props.voice_control.as_ref(),
    Some(VoiceControlAction::ToggleMute),
    mute_locked,
  ));
  row = row.child(control_button(
    ctx,
    headphones_icon,
    deafened,
    props.voice_control.as_ref(),
    Some(VoiceControlAction::ToggleDeafen),
    false,
  ));
  row = row.child(stream_control_button(
    ctx,
    props.start_stream_modal_open,
    &props.stop_stream,
    local_streaming,
    !connected_to_voice && !local_streaming,
  ));
  row = row.child(control_button(
    ctx,
    "phone-off",
    connected_to_voice,
    props.voice_control.as_ref(),
    Some(VoiceControlAction::LeaveChannel),
    !connected_to_voice,
  ));

  if let Some(settings_popup) = props.settings_popup {
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
  ctx.mount::<StreamControlButton>(StreamControlButtonProps {
    start_stream_modal_open,
    stop_stream: stop_stream.clone(),
    streaming,
    disabled,
  })
}

#[derive(Clone)]
struct StreamControlButtonProps {
  start_stream_modal_open: Signal<bool>,
  stop_stream: StopStreamAction,
  streaming: bool,
  disabled: bool,
}

impl PartialEq for StreamControlButtonProps {
  fn eq(&self, other: &Self) -> bool {
    self.streaming == other.streaming && self.disabled == other.disabled
  }
}

impl DevtoolsInspectable for StreamControlButtonProps {}

struct StreamControlButton;

impl Component for StreamControlButton {
  type Props = StreamControlButtonProps;

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let props = ctx.props::<Self::Props>().clone();
    stream_control_button_content(
      ctx,
      props.start_stream_modal_open,
      props.stop_stream,
      props.streaming,
      props.disabled,
    )
  }
}

fn stream_control_button_content(
  ctx: &mut Ctx,
  start_stream_modal_open: Signal<bool>,
  stop_stream: StopStreamAction,
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

fn local_user_label_fields(
  ctx: &mut Ctx,
  user_id: UserId,
  display_name: &str,
  local_user_name: Option<&str>,
  debug_user_ids: bool,
) -> String {
  let name = local_user_name.map(str::to_owned).unwrap_or_else(|| {
    let display_name = display_name.trim();
    if display_name.is_empty() {
      ctx
        .t_args("lobby.user.fallback", [("id", user_id.to_string())])
        .to_string()
    } else {
      display_name.to_owned()
    }
  });
  super::shared::user_display_name(user_id, &name, debug_user_ids)
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
