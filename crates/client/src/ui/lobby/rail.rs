use lurq::{
  app::{
    component::{Component, ComponentInfo, DevtoolsFormatter, DevtoolsInspectable},
    ctx::Ctx,
  },
  components::{Column, Rect, Row, Text},
  core::Signal,
  layout::{Alignment, layout_kind::Justify},
  node::{BackgroundColor, CursorIcon, Element, Style, border::Border, color::Color, dimension::Dimension},
};

use super::{StopStreamAction, WatchStreamAction};
use crate::{
  network::protocol::{ChannelId, Role, UserId},
  routes::{ROUTE_CHOOSE_SERVER, ROUTE_SERVER_SETTINGS},
  services::voice_controls::{VoiceControlAction, apply_voice_control},
  session::{ConnectedServerInfo, LobbyConnectionWarningKind, LobbyState, ServerSession},
  storage::{AppSettings, Storage},
  theme,
  ui::{
    common::lucide_icon::{LucideIcon, LucideIconProps},
    lobby::{
      debug_channels::{DebugChannels, DebugChannelsProps},
      layout::{RAIL_DIVIDER_WIDTH, lobby_layout_metrics},
      text_channels::{TextChannels, TextChannelsProps},
      voice_channels::{JoinChannelAction, VoiceChannels, VoiceChannelsProps},
    },
    settings::SettingsPopupHandle,
  },
};

type VoiceControlFuture = lurq::app::ctx::FutureAction<VoiceControlAction, (), String>;

#[derive(Clone)]
pub(super) struct LobbyRailProps {
  pub info: ConnectedServerInfo,
  pub lobby: LobbyState,
  pub debug_mode_enabled: bool,
  pub start_stream_modal_open: Signal<bool>,
  pub stop_stream: StopStreamAction,
  pub watch_stream: WatchStreamAction,
}

impl PartialEq for LobbyRailProps {
  fn eq(&self, other: &Self) -> bool {
    self.info == other.info && self.lobby == other.lobby && self.debug_mode_enabled == other.debug_mode_enabled
  }
}

impl DevtoolsInspectable for LobbyRailProps {
  fn inspect(&self, formatter: &mut DevtoolsFormatter<'_>) {
    formatter.buffer_mut().push(ComponentInfo::with_value(
      "server_name",
      std::any::type_name::<String>(),
      self.info.server_name.clone(),
    ));
    formatter.buffer_mut().push(ComponentInfo::with_value(
      "channels",
      std::any::type_name::<usize>(),
      self.lobby.channels.len().to_string(),
    ));
    formatter.buffer_mut().push(ComponentInfo::with_value(
      "text_channels",
      std::any::type_name::<usize>(),
      self.lobby.text_channels.len().to_string(),
    ));
    formatter.buffer_mut().push(ComponentInfo::with_value(
      "selected_channel_id",
      std::any::type_name::<Option<ChannelId>>(),
      format!("{:?}", self.lobby.selected_channel_id),
    ));
    formatter.buffer_mut().push(ComponentInfo::with_value(
      "selected_text_channel_id",
      std::any::type_name::<Option<ChannelId>>(),
      format!("{:?}", self.lobby.selected_text_channel_id),
    ));
  }
}

pub(super) struct LobbyRail;

impl Component for LobbyRail {
  type Props = LobbyRailProps;

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let props = ctx.props::<Self::Props>().clone();
    let session = ctx.use_context::<ServerSession>();
    let storage = ctx.use_context::<Storage>();
    let join_channel = session
      .clone()
      .map(|session| join_channel_action(ctx, session, storage.clone()));
    let voice_control = session.map(|session| voice_control_action(ctx, session));

    rail(
      ctx,
      &props.info,
      &props.lobby,
      props.debug_mode_enabled,
      props.start_stream_modal_open.clone(),
      &props.stop_stream,
      &props.watch_stream,
      join_channel.as_ref(),
      voice_control.as_ref(),
    )
  }
}

fn join_channel_action(ctx: &mut Ctx, session: ServerSession, storage: Option<Storage>) -> JoinChannelAction {
  let no_connected_server = ctx.t("lobby.error.no_connected_server").to_string();
  ctx.future_action(move |channel_id| {
    let session = session.clone();
    let storage = storage.clone();
    let no_connected_server = no_connected_server.clone();
    async move {
      let server = session.server().ok_or(no_connected_server.clone())?;
      let settings = storage
        .as_ref()
        .and_then(|storage| storage.load_settings().ok())
        .unwrap_or_else(AppSettings::default);
      let already_in_voice = session.lobby().selected_channel_id.is_some();
      let (mut muted, deafened) = session.local_voice_state().unwrap_or((false, false));
      if !already_in_voice {
        muted = settings.start_muted_when_joining || deafened;
      }
      tracing::info!(target: "lobby", 
        "[lobby] join channel requested: channel={channel_id} already_in_voice={already_in_voice} muted={muted} deafened={deafened}"
      );
      server
        .join_channel(channel_id)
        .await
        .map_err(|error| error.to_string())?;
      tracing::info!(target: "lobby", "[lobby] join channel accepted: channel={channel_id}");
      session.select_channel(channel_id);
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
  })
}

fn voice_control_action(ctx: &mut Ctx, session: ServerSession) -> VoiceControlFuture {
  let no_connected_server = ctx.t("lobby.error.no_connected_server").to_string();
  ctx.future_action(move |control| {
    let session = session.clone();
    let no_connected_server = no_connected_server.clone();
    async move { apply_voice_control(session, control, no_connected_server).await }
  })
}

fn rail(
  ctx: &mut Ctx,
  info: &ConnectedServerInfo,
  lobby: &LobbyState,
  debug_mode_enabled: bool,
  start_stream_modal_open: Signal<bool>,
  stop_stream: &StopStreamAction,
  watch_stream: &WatchStreamAction,
  join_channel: Option<&JoinChannelAction>,
  voice_control: Option<&VoiceControlFuture>,
) -> Element {
  let metrics = lobby_layout_metrics(ctx);
  Row::new()
    .border_right(Border::inside(1.0, theme::PaletteColor::Border))
    .width(metrics.rail_width)
    .height(Dimension::Pct(100.0))
    .background(BackgroundColor::Color(Color::from_hex("#0C0D0F")))
    .child(
      Column::new()
        .width(metrics.rail_width - RAIL_DIVIDER_WIDTH)
        .height(Dimension::Pct(100.0))
        .background(BackgroundColor::Color(Color::from_hex("#0C0D0F")))
        .child(rail_header(ctx, info, lobby, debug_mode_enabled))
        .child(rail_channels(
          ctx,
          info,
          lobby,
          debug_mode_enabled,
          join_channel,
          watch_stream,
        ))
        .child(rail_bottom(
          ctx,
          info,
          lobby,
          debug_mode_enabled,
          start_stream_modal_open,
          stop_stream,
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

fn rail_header(ctx: &mut Ctx, info: &ConnectedServerInfo, lobby: &LobbyState, debug_user_ids: bool) -> Element {
  let unknown_server = ctx.t("lobby.server.unknown");
  let server_name = server_name(info, unknown_server.as_ref());
  let user_label = local_user_label(ctx, lobby, info, debug_user_ids);
  let role = ctx.t(role_label_lower_key(info.role));
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

  if info.role.can_edit_server_settings() {
    row = row.child(server_settings_button(ctx));
  }

  row.child(leave_button(ctx)).into()
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

fn leave_button(ctx: &mut Ctx) -> Element {
  let navigator = ctx.navigator();
  let session = ctx.use_context::<ServerSession>();
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
  info: &ConnectedServerInfo,
  lobby: &LobbyState,
  debug_mode_enabled: bool,
  join_channel: Option<&JoinChannelAction>,
  watch_stream: &WatchStreamAction,
) -> Element {
  let metrics = lobby_layout_metrics(ctx);
  let mut channels = Column::new()
    .width(Dimension::Pct(100.0))
    .flex(1.0)
    .spacing(18.0)
    .padding_vertical(metrics.rail_padding_y)
    .padding_horizontal(metrics.rail_padding_x)
    .child(ctx.mount::<TextChannels>(TextChannelsProps {
      channels: lobby.text_channels.clone(),
      selected_channel_id: lobby.selected_text_channel_id,
      unread_channel_ids: lobby.unread_text_channel_ids.clone(),
    }));

  channels = channels.child(ctx.mount::<VoiceChannels>(VoiceChannelsProps {
    channels: lobby.channels.clone(),
    users_by_channel: lobby.users_by_channel.clone(),
    streaming_user_ids: lobby.screen_shares.iter().map(|share| share.sharer_user_id).collect(),
    selected_channel_id: lobby.selected_channel_id,
    disconnected: lobby.disconnected,
    local_user_id: info.user_id,
    local_role: info.role,
    debug_user_ids: debug_mode_enabled,
    join_channel: join_channel.cloned(),
    watch_stream: Some(watch_stream.clone()),
  }));

  if debug_mode_enabled {
    channels = channels.child(ctx.mount::<DebugChannels>(DebugChannelsProps {
      selected: lobby.debug_chat_selected,
    }));
  }

  channels.into()
}

fn rail_bottom(
  ctx: &mut Ctx,
  info: &ConnectedServerInfo,
  lobby: &LobbyState,
  debug_user_ids: bool,
  start_stream_modal_open: Signal<bool>,
  stop_stream: &StopStreamAction,
  voice_control: Option<&VoiceControlFuture>,
) -> Element {
  let metrics = lobby_layout_metrics(ctx);
  Column::new()
    .width(Dimension::Pct(100.0))
    .spacing(theme::SpacingSize::Md)
    .padding_vertical(metrics.rail_padding_y)
    .padding_horizontal(metrics.rail_padding_x)
    .child(connection_status(ctx, lobby))
    .child(local_user_row(ctx, info, lobby, debug_user_ids))
    .child(control_row(
      ctx,
      info,
      lobby,
      start_stream_modal_open,
      stop_stream,
      voice_control,
    ))
    .into()
}

fn connection_status(ctx: &mut Ctx, lobby: &LobbyState) -> Element {
  let selected = lobby
    .selected_channel_id
    .and_then(|id| lobby.channels.iter().find(|channel| channel.id == id));
  let warning = (!lobby.disconnected)
    .then_some(lobby.connection_warning.as_ref())
    .flatten();
  let fallback_title = ctx.t("lobby.connection.not_in_channel");
  let connected_to_voice = selected.is_some() && !lobby.disconnected;
  let title = if let Some(warning) = warning {
    ctx.t(connection_warning_title_key(&warning.kind)).to_string()
  } else if lobby.disconnected {
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
  } else if lobby.disconnected {
    ctx.t("lobby.connection.not_in_channel").to_string()
  } else {
    ctx.t("lobby.connection.connected").to_string()
  };
  let (status_icon, icon_color) = if warning.is_some() {
    ("triangle-alert", theme::palette().warning)
  } else if lobby.disconnected {
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

fn local_user_row(ctx: &mut Ctx, info: &ConnectedServerInfo, lobby: &LobbyState, debug_user_ids: bool) -> Element {
  let avatar_name = local_user_name(lobby, info).unwrap_or_else(|| {
    let display_name = info.display_name.trim();
    if display_name.is_empty() {
      ctx
        .t_args("lobby.user.fallback", [("id", info.user_id.to_string())])
        .to_string()
    } else {
      display_name.to_owned()
    }
  });
  let username = local_user_label(ctx, lobby, info, debug_user_ids);
  let role = ctx.t(role_label_lower_key(info.role));
  let ping_label = lobby
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
  info: &ConnectedServerInfo,
  lobby: &LobbyState,
  start_stream_modal_open: Signal<bool>,
  stop_stream: &StopStreamAction,
  voice_control: Option<&VoiceControlFuture>,
) -> Element {
  let (muted, deafened) = ctx
    .use_context::<ServerSession>()
    .and_then(|session| session.local_voice_state())
    .unwrap_or_else(|| local_voice_state(lobby, info));
  let mic_icon = if muted { "mic-off" } else { "mic" };
  let headphones_icon = if deafened { "headphone-off" } else { "headphones" };
  let mute_locked = muted && deafened;
  let connected_to_voice = local_user_in_voice(lobby, info.user_id) && !lobby.disconnected;
  let local_streaming = lobby
    .screen_shares
    .iter()
    .any(|share| share.sharer_user_id == info.user_id);
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

  let settings_popup = ctx.use_context::<SettingsPopupHandle>();
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

fn local_user_in_voice(lobby: &LobbyState, local_user_id: UserId) -> bool {
  lobby
    .users_by_channel
    .values()
    .any(|users| users.iter().any(|user| user.user_id == local_user_id))
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

fn server_name<'a>(info: &'a ConnectedServerInfo, fallback: &'a str) -> &'a str {
  if info.server_name.trim().is_empty() {
    fallback
  } else {
    info.server_name.as_str()
  }
}

fn local_user_name(lobby: &LobbyState, info: &ConnectedServerInfo) -> Option<String> {
  lobby
    .users
    .iter()
    .chain(lobby.users_by_channel.values().flatten())
    .find(|user| user.user_id == info.user_id)
    .map(|user| user.username.clone())
}

fn local_user_label(ctx: &mut Ctx, lobby: &LobbyState, info: &ConnectedServerInfo, debug_user_ids: bool) -> String {
  let name = local_user_name(lobby, info).unwrap_or_else(|| {
    let display_name = info.display_name.trim();
    if display_name.is_empty() {
      ctx
        .t_args("lobby.user.fallback", [("id", info.user_id.to_string())])
        .to_string()
    } else {
      display_name.to_owned()
    }
  });
  super::shared::user_display_name(info.user_id, &name, debug_user_ids)
}

fn local_voice_state(lobby: &LobbyState, info: &ConnectedServerInfo) -> (bool, bool) {
  lobby
    .users
    .iter()
    .chain(lobby.users_by_channel.values().flatten())
    .find(|user| user.user_id == info.user_id)
    .map(|user| (user.muted, user.deafened))
    .unwrap_or((false, false))
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
