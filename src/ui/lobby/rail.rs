use lurq::{
  app::{
    component::{Component, ComponentInfo, DevtoolsInspectable},
    ctx::Ctx,
  },
  components::{Column, Rect, Row, Text},
  layout::{Alignment, layout_kind::Justify},
  node::{BackgroundColor, CursorIcon, Element, Style, border::Border, color::Color, dimension::Dimension},
};

use crate::{
  network::protocol::{ChannelId, Role},
  routes::ROUTE_CHOOSE_SERVER,
  session::{ConnectedServerInfo, LobbyState, ServerSession},
  storage::{AppSettings, Storage},
  theme,
  ui::{
    common::lucide_icon::{LucideIcon, LucideIconProps},
    lobby::{
      text_channels::{TextChannels, TextChannelsProps},
      voice_channels::{JoinChannelAction, VoiceChannels, VoiceChannelsProps},
    },
    settings::SettingsPopupHandle,
  },
};

const RAIL_WIDTH: f32 = 280.0;
const RAIL_DIVIDER_WIDTH: f32 = 1.0;

type VoiceControlAction = lurq::app::ctx::FutureAction<VoiceControl, (), String>;

#[derive(Clone, Copy, PartialEq, Eq)]
enum VoiceControl {
  ToggleMute,
  ToggleDeafen,
}

#[derive(Clone, PartialEq)]
pub(super) struct LobbyRailProps {
  pub info: ConnectedServerInfo,
  pub lobby: LobbyState,
}

impl DevtoolsInspectable for LobbyRailProps {
  fn write_info(&self, buffer: &mut Vec<ComponentInfo>) {
    buffer.push(ComponentInfo::with_value(
      "server_name",
      std::any::type_name::<String>(),
      self.info.server_name.clone(),
    ));
    buffer.push(ComponentInfo::with_value(
      "channels",
      std::any::type_name::<usize>(),
      self.lobby.channels.len().to_string(),
    ));
    buffer.push(ComponentInfo::with_value(
      "text_channels",
      std::any::type_name::<usize>(),
      self.lobby.text_channels.len().to_string(),
    ));
    buffer.push(ComponentInfo::with_value(
      "selected_channel_id",
      std::any::type_name::<Option<ChannelId>>(),
      format!("{:?}", self.lobby.selected_channel_id),
    ));
    buffer.push(ComponentInfo::with_value(
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
      join_channel.as_ref(),
      voice_control.as_ref(),
    )
  }
}

fn join_channel_action(ctx: &mut Ctx, session: ServerSession, storage: Option<Storage>) -> JoinChannelAction {
  ctx.future_action(move |channel_id| {
    let session = session.clone();
    let storage = storage.clone();
    async move {
      let server = session.server().ok_or_else(|| "No connected server.".to_owned())?;
      let (muted, deafened) = session.local_voice_state().unwrap_or((false, false));
      server
        .join_channel(channel_id)
        .await
        .map_err(|error| error.to_string())?;
      session.select_channel(channel_id);
      server
        .update_voice_state(muted, deafened)
        .await
        .map_err(|error| error.to_string())?;
      session.set_local_voice_state(muted, deafened);
      let settings = storage
        .as_ref()
        .and_then(|storage| storage.load_settings().ok())
        .unwrap_or_else(AppSettings::default);
      let _ = session.start_voice(settings);
      Ok(())
    }
  })
}

fn voice_control_action(ctx: &mut Ctx, session: ServerSession) -> VoiceControlAction {
  ctx.future_action(move |control| {
    let session = session.clone();
    async move {
      let server = session.server().ok_or_else(|| "No connected server.".to_owned())?;
      let (mut muted, mut deafened) = session.local_voice_state().unwrap_or((false, false));

      match control {
        VoiceControl::ToggleMute => {
          if deafened && muted {
            return Ok(());
          }
          muted = !muted;
        }
        VoiceControl::ToggleDeafen => {
          if deafened {
            deafened = false;
            muted = session.take_muted_before_deafen().unwrap_or(muted);
          } else {
            session.remember_muted_before_deafen(muted);
            deafened = true;
            muted = true;
          }
        }
      }

      server
        .update_voice_state(muted, deafened)
        .await
        .map_err(|error| error.to_string())?;
      session.set_local_voice_state(muted, deafened);
      Ok(())
    }
  })
}

fn rail(
  ctx: &mut Ctx,
  info: &ConnectedServerInfo,
  lobby: &LobbyState,
  join_channel: Option<&JoinChannelAction>,
  voice_control: Option<&VoiceControlAction>,
) -> Element {
  Row::new()
    .border_right(Border::inside(1.0, theme::PaletteColor::Border))
    .width(RAIL_WIDTH)
    .height(Dimension::Pct(100.0))
    .background(BackgroundColor::Color(Color::from_hex("#0C0D0F")))
    .child(
      Column::new()
        .width(RAIL_WIDTH - RAIL_DIVIDER_WIDTH)
        .height(Dimension::Pct(100.0))
        .background(BackgroundColor::Color(Color::from_hex("#0C0D0F")))
        .child(rail_header(ctx, info, lobby))
        .child(rail_channels(ctx, info, lobby, join_channel))
        .child(rail_bottom(ctx, info, lobby, voice_control)),
    )
    .child(
      Rect::new(RAIL_DIVIDER_WIDTH, 1.0)
        .height(Dimension::Pct(100.0))
        .background(BackgroundColor::Palette(theme::PaletteColor::Border)),
    )
    .into()
}

fn rail_header(ctx: &mut Ctx, info: &ConnectedServerInfo, lobby: &LobbyState) -> Element {
  let unknown_server = ctx.t("lobby.server.unknown");
  let server_name = server_name(info, unknown_server.as_ref());
  let user_label = local_user_label(lobby, info);
  let sub = format!("{} · {}", user_label, role_label_lower(info.role));

  Row::new()
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
    )
    .child(leave_button(ctx))
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
  join_channel: Option<&JoinChannelAction>,
) -> Element {
  Column::new()
    .width(Dimension::Pct(100.0))
    .flex(1.0)
    .spacing(18.0)
    .padding_vertical(14.0)
    .padding_horizontal(12.0)
    .child(ctx.mount::<TextChannels>(TextChannelsProps {
      channels: lobby.text_channels.clone(),
      selected_channel_id: lobby.selected_text_channel_id,
    }))
    .child(ctx.mount::<VoiceChannels>(VoiceChannelsProps {
      channels: lobby.channels.clone(),
      users_by_channel: lobby.users_by_channel.clone(),
      streaming_user_ids: lobby.screen_shares.iter().map(|share| share.sharer_user_id).collect(),
      selected_channel_id: lobby.selected_channel_id,
      local_user_id: info.user_id,
      join_channel: join_channel.cloned(),
    }))
    .into()
}

fn rail_bottom(
  ctx: &mut Ctx,
  info: &ConnectedServerInfo,
  lobby: &LobbyState,
  voice_control: Option<&VoiceControlAction>,
) -> Element {
  Column::new()
    .width(Dimension::Pct(100.0))
    .spacing(theme::SpacingSize::Md)
    .padding(12.0)
    .child(connection_status(ctx, lobby))
    .child(local_user_row(info, lobby))
    .child(control_row(ctx, info, lobby, voice_control))
    .into()
}

fn connection_status(ctx: &mut Ctx, lobby: &LobbyState) -> Element {
  let selected = lobby
    .selected_channel_id
    .and_then(|id| lobby.channels.iter().find(|channel| channel.id == id));
  let fallback_title = ctx.t("lobby.connection.not_in_channel");
  let connected_to_voice = selected.is_some() && !lobby.disconnected;
  let title = if lobby.disconnected {
    ctx.t("lobby.status.disconnected")
  } else if connected_to_voice {
    ctx.t("lobby.connection.voice_connected")
  } else {
    fallback_title
  };
  let sub = if let Some(channel) = selected {
    format!("{} · {}", channel.name, ctx.t("lobby.connection.connected"))
  } else if lobby.disconnected {
    ctx.t("lobby.connection.not_in_channel").to_string()
  } else {
    ctx.t("lobby.connection.connected").to_string()
  };
  let (status_icon, icon_color) = if lobby.disconnected {
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
        .child(status_sub(connected_to_voice, &sub)),
    )
    .child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon: status_icon,
      size: 16.0,
      color: icon_color,
    }))
    .into()
}

fn status_sub(connected_to_voice: bool, label: &str) -> Element {
  let mut row = Row::new().align_items(Alignment::Center).spacing(6.0);

  if connected_to_voice {
    row = row.child(
      Rect::new(7.0, 7.0)
        .rounded(4.0)
        .background(BackgroundColor::Palette(theme::PaletteColor::Success)),
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

fn local_user_row(info: &ConnectedServerInfo, lobby: &LobbyState) -> Element {
  let username = local_user_label(lobby, info);

  Row::new()
    .width(Dimension::Pct(100.0))
    .align_items(Alignment::Center)
    .spacing(theme::SpacingSize::Md)
    .padding(4.0)
    .child(local_avatar(&username))
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
          Text::new(role_label_lower(info.role))
            .variant(theme::TypographyStyle::Caption)
            .color(theme::PaletteColor::TextMuted),
        ),
    )
    .into()
}

fn control_row(
  ctx: &mut Ctx,
  info: &ConnectedServerInfo,
  lobby: &LobbyState,
  voice_control: Option<&VoiceControlAction>,
) -> Element {
  let (muted, deafened) = ctx
    .use_context::<ServerSession>()
    .and_then(|session| session.local_voice_state())
    .unwrap_or_else(|| local_voice_state(lobby, info));
  let mic_icon = if muted { "mic-off" } else { "mic" };
  let headphones_icon = if deafened { "headphone-off" } else { "headphones" };
  let mute_locked = muted && deafened;
  let mut row = Row::new()
    .width(Dimension::Pct(100.0))
    .align_items(Alignment::Center)
    .spacing(8.0);

  row = row.child(control_button(
    ctx,
    mic_icon,
    muted,
    voice_control,
    Some(VoiceControl::ToggleMute),
    mute_locked,
  ));
  row = row.child(control_button(
    ctx,
    headphones_icon,
    deafened,
    voice_control,
    Some(VoiceControl::ToggleDeafen),
    false,
  ));
  row = row.child(control_button(ctx, "monitor-up", false, None, None, false));

  let settings_popup = ctx.use_context::<SettingsPopupHandle>();
  if let Some(settings_popup) = settings_popup {
    row = row.child(icon_button(ctx, "settings", false, false).on_click(move |_| settings_popup.open()));
  } else {
    row = row.child(icon_button(ctx, "settings", false, false));
  }

  row.into()
}

fn control_button(
  ctx: &mut Ctx,
  icon: &'static str,
  active: bool,
  voice_control: Option<&VoiceControlAction>,
  action: Option<VoiceControl>,
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
      if active {
        theme::PaletteColor::Danger
      } else {
        theme::PaletteColor::Border
      },
    )
    .child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon,
      size: 16.0,
      color: if active {
        theme::palette().danger
      } else {
        theme::palette().text_secondary
      },
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
    .border_inside(1.5, BackgroundColor::Palette(theme::PaletteColor::Success))
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

fn local_user_label(lobby: &LobbyState, info: &ConnectedServerInfo) -> String {
  local_user_name(lobby, info).unwrap_or_else(|| {
    let display_name = info.display_name.trim();
    if display_name.is_empty() {
      format!("user #{}", info.user_id)
    } else {
      display_name.to_owned()
    }
  })
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

pub(super) fn role_label_lower(role: Role) -> &'static str {
  match role {
    Role::Owner => "owner",
    Role::Admin => "admin",
    Role::Moderator => "moderator",
    Role::User => "member",
  }
}
