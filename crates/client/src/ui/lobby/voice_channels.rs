use std::{
  collections::HashMap,
  sync::{Arc, Mutex},
  time::Duration,
};

use lurq::{
  app::{
    component::{Component, ComponentInfo, DevtoolsFormatter, DevtoolsInspectable},
    ctx::{Ctx, Interval, Modal, Root},
    events::{MouseButton, MouseEvent},
  },
  components::{Column, Row, Stack, Text},
  core::Signal,
  layout::{
    Alignment,
    layout_kind::Justify,
    text_style::{FontWeight, TextStyle},
  },
  node::{BackgroundColor, CursorIcon, Element, Style, color::Color, dimension::Dimension},
};

use super::WatchStreamAction;
use crate::{
  network::protocol::{ChannelId, Permission, Role, UserId},
  session::{LobbyChannel, LobbyUser, ServerSession},
  storage::Storage,
  theme,
  ui::{
    app_chrome::{CHROME_HEIGHT, content_height, modal_y},
    common::percent_slider::{PercentSliderSaveAction, percent_slider_control},
    lobby::{
      channel_section::{aligned_channel_icon, aligned_channel_icon_with_color, section_head},
      shared::user_display_name,
    },
    settings::settings_toggle_track,
  },
};

pub(super) type JoinChannelAction = lurq::app::ctx::FutureAction<ChannelId, (), String>;
type SetRoleAction = lurq::app::ctx::FutureAction<(UserId, Role), (), String>;
type SetUserVoiceStateAction = lurq::app::ctx::FutureAction<(UserId, bool, bool), (), String>;
type DisconnectUserAction = lurq::app::ctx::FutureAction<UserId, (), String>;
type KickUserAction = lurq::app::ctx::FutureAction<UserId, (), String>;

const USER_CONTEXT_MENU_WIDTH: f32 = 286.0;
const USER_CONTEXT_MENU_HORIZONTAL_PADDING: f32 = 10.0;
const USER_VOLUME_SLIDER_WIDTH: f32 = USER_CONTEXT_MENU_WIDTH - USER_CONTEXT_MENU_HORIZONTAL_PADDING * 2.0;
const USER_VOLUME_VALUE_WIDTH: f32 = 42.0;
const USER_VOLUME_VALUE_SPACING: f32 = 10.0;
const USER_VOLUME_TRACK_WIDTH: f32 = USER_VOLUME_SLIDER_WIDTH - USER_VOLUME_VALUE_WIDTH - USER_VOLUME_VALUE_SPACING;
const DEFAULT_USER_VOLUME: i32 = 100;
const ASSIGNABLE_ROLES: [Role; 3] = [Role::Admin, Role::Moderator, Role::User];

#[derive(Clone)]
pub(super) struct VoiceChannelsProps {
  pub channels: Vec<LobbyChannel>,
  pub users_by_channel: HashMap<ChannelId, Vec<LobbyUser>>,
  pub streaming_user_ids: Vec<UserId>,
  pub selected_channel_id: Option<ChannelId>,
  pub disconnected: bool,
  pub local_user_id: UserId,
  pub local_role: Role,
  pub debug_user_ids: bool,
  pub join_channel: Option<JoinChannelAction>,
  pub watch_stream: Option<WatchStreamAction>,
}

impl PartialEq for VoiceChannelsProps {
  fn eq(&self, other: &Self) -> bool {
    self.channels == other.channels
      && self.users_by_channel == other.users_by_channel
      && self.streaming_user_ids == other.streaming_user_ids
      && self.selected_channel_id == other.selected_channel_id
      && self.disconnected == other.disconnected
      && self.local_user_id == other.local_user_id
      && self.local_role == other.local_role
      && self.debug_user_ids == other.debug_user_ids
      && self.join_channel.is_some() == other.join_channel.is_some()
      && self.watch_stream.is_some() == other.watch_stream.is_some()
  }
}

impl DevtoolsInspectable for VoiceChannelsProps {
  fn inspect(&self, formatter: &mut DevtoolsFormatter<'_>) {
    formatter.buffer_mut().push(ComponentInfo::with_value(
      "channels",
      std::any::type_name::<usize>(),
      self.channels.len().to_string(),
    ));
    formatter.buffer_mut().push(ComponentInfo::with_value(
      "selected_channel_id",
      std::any::type_name::<Option<ChannelId>>(),
      format!("{:?}", self.selected_channel_id),
    ));
    formatter.buffer_mut().push(ComponentInfo::with_value(
      "users_by_channel",
      std::any::type_name::<usize>(),
      self.users_by_channel.values().map(Vec::len).sum::<usize>().to_string(),
    ));
    formatter.buffer_mut().push(ComponentInfo::with_value(
      "streaming_user_ids",
      std::any::type_name::<usize>(),
      self.streaming_user_ids.len().to_string(),
    ));
  }
}

pub(super) struct VoiceChannels {
  expanded: Signal<bool>,
  context_user_id: Signal<Option<UserId>>,
  context_menu_open: Signal<bool>,
  context_menu_anchor: Signal<Option<(f32, f32)>>,
  role_menu_user_id: Signal<Option<UserId>>,
}

impl Component for VoiceChannels {
  type Props = VoiceChannelsProps;

  fn create(ctx: &mut Ctx) -> Self {
    Self {
      expanded: ctx.signal(true),
      context_user_id: ctx.signal(None),
      context_menu_open: ctx.signal(false),
      context_menu_anchor: ctx.signal(None),
      role_menu_user_id: ctx.signal(None),
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let props = ctx.props::<Self::Props>().clone();
    let window_focused = ctx.window().is_focused;
    if self.context_menu_open.get_untracked()
      && (!window_focused
        || props.disconnected
        || self.context_user_id.get_untracked().is_none_or(|user_id| {
          user_id == props.local_user_id
            || !props
              .users_by_channel
              .values()
              .any(|users| users.iter().any(|user| user.user_id == user_id))
        }))
    {
      close_user_context_menu(
        self.context_user_id.clone(),
        self.context_menu_open.clone(),
        self.context_menu_anchor.clone(),
        self.role_menu_user_id.clone(),
      );
    }
    let is_expanded = self.expanded.get();
    let session = ctx.use_context::<ServerSession>();
    let set_role = session.clone().map(|session| set_role_action(ctx, session));
    let set_user_voice_state = session.clone().map(|session| set_user_voice_state_action(ctx, session));
    let disconnect_user = session.clone().map(|session| disconnect_user_action(ctx, session));
    let kick_user = session.clone().map(|session| kick_user_action(ctx, session));
    let channels_for_menu = props.channels.clone();
    let users_by_channel_for_menu = props.users_by_channel.clone();
    let mut body = Column::new()
      .width(Dimension::Pct(100.0))
      .spacing(theme::SpacingSize::Xs)
      .child(section_head(
        ctx,
        self.expanded.clone(),
        &ctx.t("lobby.voice_channels.title"),
        None,
        None,
        false,
      ));

    if !is_expanded {
      return body;
    }

    if props.channels.is_empty() {
      body = body.child(
        Row::new()
          .width(Dimension::Pct(100.0))
          .padding_vertical(6.0)
          .padding_horizontal(8.0)
          .child(
            Text::new(&ctx.t("lobby.voice_channels.empty"))
              .variant(theme::TypographyStyle::Link)
              .color(theme::PaletteColor::TextMuted),
          ),
      );
    } else {
      let users_by_channel = props.users_by_channel.clone();
      let selected_channel_id = props.selected_channel_id;
      let local_user_id = props.local_user_id;
      let join_channel = props.join_channel.clone();
      let watch_stream = props.watch_stream.clone();
      let streaming_user_ids = props.streaming_user_ids.clone();
      let debug_user_ids = props.debug_user_ids;
      let context_user_id = self.context_user_id.clone();
      let context_menu_open = self.context_menu_open.clone();
      let context_menu_anchor = self.context_menu_anchor.clone();
      let role_menu_user_id = self.role_menu_user_id.clone();
      let session_for_channels = session.clone();
      let channel_groups = ctx.for_each(
        props.channels,
        |channel| channel.id,
        move |ctx, channel| {
          let users = users_by_channel.get(&channel.id).cloned().unwrap_or_default();
          channel_group(
            ctx,
            &channel,
            selected_channel_id,
            local_user_id,
            join_channel.as_ref(),
            watch_stream.as_ref(),
            users,
            streaming_user_ids.clone(),
            session_for_channels.clone(),
            context_user_id.clone(),
            context_menu_open.clone(),
            context_menu_anchor.clone(),
            role_menu_user_id.clone(),
            debug_user_ids,
          )
        },
      );
      body = body.with_children(channel_groups);
    }

    if let Some((modal_user, modal_channel_name)) = context_menu_target(
      &channels_for_menu,
      &users_by_channel_for_menu,
      self.context_user_id.get_untracked(),
    ) {
      let modal_context_user_id = self.context_user_id.clone();
      let modal_context_menu_open = self.context_menu_open.clone();
      let modal_anchor = self.context_menu_anchor.clone();
      let modal_role_menu_user_id = self.role_menu_user_id.clone();
      let modal_session = session.clone();
      let modal_set_role = set_role.clone();
      let modal_set_user_voice_state = set_user_voice_state.clone();
      let modal_disconnect_user = disconnect_user.clone();
      let modal_kick_user = kick_user.clone();
      let local_user_id = props.local_user_id;
      let local_role = props.local_role;
      body = body.child(
        Modal::new(user_context_overlay(
          ctx,
          &modal_user,
          &modal_channel_name,
          local_user_id,
          local_role,
          modal_context_user_id,
          modal_context_menu_open,
          modal_anchor,
          modal_role_menu_user_id,
          modal_session,
          modal_set_role,
          modal_set_user_voice_state,
          modal_disconnect_user,
          modal_kick_user,
          props.debug_user_ids,
        ))
        .open(self.context_menu_open.clone())
        .target(Root),
      );
    }

    body
  }
}

fn context_menu_target(
  channels: &[LobbyChannel],
  users_by_channel: &HashMap<ChannelId, Vec<LobbyUser>>,
  target_user_id: Option<UserId>,
) -> Option<(LobbyUser, String)> {
  let target_user_id = target_user_id?;
  channels.iter().find_map(|channel| {
    users_by_channel.get(&channel.id).and_then(|users| {
      users
        .iter()
        .find(|user| user.user_id == target_user_id)
        .cloned()
        .map(|user| (user, channel.name.clone()))
    })
  })
}

fn close_user_context_menu(
  context_user_id: Signal<Option<UserId>>,
  context_menu_open: Signal<bool>,
  context_menu_anchor: Signal<Option<(f32, f32)>>,
  role_menu_user_id: Signal<Option<UserId>>,
) {
  context_user_id.set(None);
  context_menu_open.set(false);
  context_menu_anchor.set(None);
  role_menu_user_id.set(None);
}

fn set_role_action(ctx: &mut Ctx, session: ServerSession) -> SetRoleAction {
  let no_connected_server = ctx.t("lobby.error.no_connected_server").to_string();
  ctx.future_action(move |(target_user_id, role)| {
    let session = session.clone();
    let no_connected_server = no_connected_server.clone();
    async move {
      let server = session.server().ok_or(no_connected_server)?;
      server
        .set_role(target_user_id, role)
        .await
        .map_err(|error| error.to_string())
    }
  })
}

fn set_user_voice_state_action(ctx: &mut Ctx, session: ServerSession) -> SetUserVoiceStateAction {
  let no_connected_server = ctx.t("lobby.error.no_connected_server").to_string();
  ctx.future_action(move |(target_user_id, muted, deafened)| {
    let session = session.clone();
    let no_connected_server = no_connected_server.clone();
    async move {
      let server = session.server().ok_or(no_connected_server)?;
      server
        .set_user_voice_state(target_user_id, muted, deafened)
        .await
        .map_err(|error| error.to_string())
    }
  })
}

fn disconnect_user_action(ctx: &mut Ctx, session: ServerSession) -> DisconnectUserAction {
  let no_connected_server = ctx.t("lobby.error.no_connected_server").to_string();
  ctx.future_action(move |target_user_id| {
    let session = session.clone();
    let no_connected_server = no_connected_server.clone();
    async move {
      let server = session.server().ok_or(no_connected_server)?;
      server
        .disconnect_user_from_voice(target_user_id)
        .await
        .map_err(|error| error.to_string())
    }
  })
}

fn kick_user_action(ctx: &mut Ctx, session: ServerSession) -> KickUserAction {
  let no_connected_server = ctx.t("lobby.error.no_connected_server").to_string();
  ctx.future_action(move |target_user_id| {
    let session = session.clone();
    let no_connected_server = no_connected_server.clone();
    async move {
      let server = session.server().ok_or(no_connected_server)?;
      server
        .kick_user(target_user_id)
        .await
        .map_err(|error| error.to_string())
    }
  })
}

fn channel_group(
  ctx: &mut Ctx,
  channel: &LobbyChannel,
  selected_channel_id: Option<ChannelId>,
  local_user_id: UserId,
  join_channel: Option<&JoinChannelAction>,
  watch_stream: Option<&WatchStreamAction>,
  mut users: Vec<LobbyUser>,
  streaming_user_ids: Vec<UserId>,
  session: Option<ServerSession>,
  context_user_id: Signal<Option<UserId>>,
  context_menu_open: Signal<bool>,
  context_menu_anchor: Signal<Option<(f32, f32)>>,
  role_menu_user_id: Signal<Option<UserId>>,
  debug_user_ids: bool,
) -> Element {
  users.sort_by(|left, right| {
    left
      .username
      .to_lowercase()
      .cmp(&right.username.to_lowercase())
      .then_with(|| left.username.cmp(&right.username))
      .then_with(|| left.user_id.cmp(&right.user_id))
  });

  let user_rows = ctx.for_each(
    users,
    |user| user.user_id,
    move |ctx, user| {
      let streaming = streaming_user_ids.contains(&user.user_id);
      channel_user_row(
        ctx,
        &user,
        user.user_id == local_user_id,
        streaming,
        watch_stream,
        context_user_id.clone(),
        context_menu_open.clone(),
        context_menu_anchor.clone(),
        role_menu_user_id.clone(),
        debug_user_ids,
      )
    },
  );

  Column::new()
    .width(Dimension::Pct(100.0))
    .spacing(2.0)
    .child(channel_row(ctx, channel, selected_channel_id, join_channel, session))
    .with_children(user_rows)
    .into()
}

fn channel_row(
  ctx: &mut Ctx,
  channel: &LobbyChannel,
  selected_channel_id: Option<ChannelId>,
  join_channel: Option<&JoinChannelAction>,
  session: Option<ServerSession>,
) -> Element {
  let selected = selected_channel_id == Some(channel.id);
  let channel_id = channel.id;
  let count = channel.user_count.to_string();
  let channel_color = if selected {
    theme::palette().accent
  } else {
    theme::palette().text_muted
  };
  let mut row = Row::new()
    .width(Dimension::Pct(100.0))
    .align_items(Alignment::Center)
    .justify(Justify::SpaceBetween)
    .spacing(theme::SpacingSize::Sm)
    .padding_vertical(10.0)
    .padding_horizontal(12.0)
    .rounded(theme::RadiusSize::Md)
    .background(if selected {
      BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised)
    } else {
      BackgroundColor::Color(Color::from_hex("#00000000"))
    })
    .cursor(CursorIcon::Pointer)
    .hovered_style(Style::new().background(BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised)))
    .child(
      Row::new()
        .flex(1.0)
        .align_items(Alignment::Center)
        .spacing(theme::SpacingSize::Sm)
        .child(aligned_channel_icon_with_color(ctx, "volume-2", 16.0, channel_color))
        .child(
          Text::new(&channel.name)
            .variant(theme::TypographyStyle::Description)
            .color(if selected {
              theme::PaletteColor::TextPrimary
            } else {
              theme::PaletteColor::TextSecondary
            })
            .width(Dimension::Pct(100.0)),
        ),
    )
    .child(
      Row::new()
        .align_items(Alignment::Center)
        .spacing(theme::SpacingSize::Xs)
        .child(aligned_channel_icon(ctx, "user", 12.0))
        .child(
          Text::new(&count)
            .variant(theme::TypographyStyle::Mono)
            .color(theme::PaletteColor::TextMuted),
        ),
    );

  if selected {
    if let Some(session) = session {
      row = row.on_click(move |_| session.open_stream_browser(channel_id));
    }
  } else if let Some(join_channel) = join_channel {
    let action = join_channel.clone();
    row = row.on_click(move |_| action.run(channel_id));
  }

  row.into()
}

fn channel_user_row(
  ctx: &mut Ctx,
  user: &LobbyUser,
  local: bool,
  streaming: bool,
  watch_stream: Option<&WatchStreamAction>,
  context_user_id: Signal<Option<UserId>>,
  context_menu_open: Signal<bool>,
  context_menu_anchor: Signal<Option<(f32, f32)>>,
  role_menu_user_id: Signal<Option<UserId>>,
  debug_user_ids: bool,
) -> Element {
  let speaking = user.speaking && !user.muted && !user.deafened;
  let user_id = user.user_id;
  let open_context = context_user_id.clone();
  let open_menu = context_menu_open.clone();
  let open_anchor = context_menu_anchor.clone();
  let open_role_menu = role_menu_user_id.clone();
  let watch_action = streaming
    .then(|| watch_stream)
    .flatten()
    .filter(|action| !action.state().get().is_pending())
    .cloned();
  let close_context = context_user_id.clone();
  let close_menu = context_menu_open.clone();
  let close_anchor = context_menu_anchor.clone();
  let close_role_menu = role_menu_user_id.clone();
  let menu_open = context_user_id.get() == Some(user.user_id);
  let scale = ctx.window().scale_factor.max(f32::EPSILON);
  let username = user_display_name(user.user_id, &user.username, debug_user_ids);

  let mut row = Row::new()
    .width(Dimension::Pct(100.0))
    .align_items(Alignment::Center)
    .spacing(theme::SpacingSize::Sm)
    .padding_vertical(5.0)
    .padding_horizontal(16.0)
    .rounded(theme::RadiusSize::Md)
    .background(if menu_open {
      BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised)
    } else {
      BackgroundColor::Color(Color::from_hex("#00000000"))
    })
    .cursor(CursorIcon::Pointer)
    .hovered_style(Style::new().background(BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised)));

  if !local {
    row = row.on_mouse_click(MouseButton::Right, move |event: MouseEvent| {
      let anchor = (event.x / scale, event.y / scale);
      open_anchor.set(Some(anchor));
      open_context.set(Some(user_id));
      open_role_menu.set(None);
      open_menu.set(true);
    });
  }

  row = row
    .on_click(move |_| {
      if close_context.get_untracked() == Some(user_id) {
        close_user_context_menu(
          close_context.clone(),
          close_menu.clone(),
          close_anchor.clone(),
          close_role_menu.clone(),
        );
      }
      if let Some(action) = watch_action.as_ref() {
        action.run(user_id);
      }
    })
    .child(user_avatar(&user.username, speaking))
    .child(
      Text::new(&username)
        .flex(1.0)
        .variant(if speaking {
          theme::TypographyStyle::Button
        } else {
          theme::TypographyStyle::Description
        })
        .color(if speaking {
          theme::PaletteColor::TextPrimary
        } else {
          theme::PaletteColor::TextSecondary
        }),
    )
    .child(user_voice_icons(ctx, user, streaming));

  Column::new().width(Dimension::Pct(100.0)).child(row).into()
}

fn user_context_overlay(
  ctx: &mut Ctx,
  user: &LobbyUser,
  channel_name: &str,
  local_user_id: UserId,
  local_role: Role,
  context_user_id: Signal<Option<UserId>>,
  context_menu_open: Signal<bool>,
  context_menu_anchor: Signal<Option<(f32, f32)>>,
  role_menu_user_id: Signal<Option<UserId>>,
  session: Option<ServerSession>,
  set_role: Option<SetRoleAction>,
  set_user_voice_state: Option<SetUserVoiceStateAction>,
  disconnect_user: Option<DisconnectUserAction>,
  kick_user: Option<KickUserAction>,
  debug_user_ids: bool,
) -> Element {
  let window = ctx.window();
  let window_width = window.logical_width();
  let modal_height = content_height(ctx);
  let (anchor_x, anchor_y) = context_menu_anchor
    .get_untracked()
    .unwrap_or((250.0, CHROME_HEIGHT + 96.0));
  let menu_left = (anchor_x + 8.0).clamp(8.0, (window_width - USER_CONTEXT_MENU_WIDTH - 8.0).max(8.0));
  let menu_top = modal_y(anchor_y).clamp(8.0, (modal_height - 8.0).max(8.0));
  let close_left_user_id = context_user_id.clone();
  let close_left_menu = context_menu_open.clone();
  let close_left_anchor = context_menu_anchor.clone();
  let close_left_role_menu = role_menu_user_id.clone();
  let close_right_user_id = context_user_id.clone();
  let close_right_menu = context_menu_open.clone();
  let close_right_anchor = context_menu_anchor.clone();
  let close_right_role_menu = role_menu_user_id.clone();
  let close_middle_user_id = context_user_id.clone();
  let close_middle_menu = context_menu_open.clone();
  let close_middle_anchor = context_menu_anchor.clone();
  let close_middle_role_menu = role_menu_user_id.clone();

  Stack::new()
    .width(window_width)
    .height(modal_height)
    .absolute(0.0, CHROME_HEIGHT, window_width, modal_height)
    .child(
      Row::new()
        .width(window_width)
        .height(modal_height)
        .on_click(move |_| {
          close_user_context_menu(
            close_left_user_id.clone(),
            close_left_menu.clone(),
            close_left_anchor.clone(),
            close_left_role_menu.clone(),
          );
        })
        .on_mouse_click(MouseButton::Right, move |_| {
          close_user_context_menu(
            close_right_user_id.clone(),
            close_right_menu.clone(),
            close_right_anchor.clone(),
            close_right_role_menu.clone(),
          );
        })
        .on_mouse_click(MouseButton::Middle, move |_| {
          close_user_context_menu(
            close_middle_user_id.clone(),
            close_middle_menu.clone(),
            close_middle_anchor.clone(),
            close_middle_role_menu.clone(),
          );
        }),
    )
    .child(
      user_context_menu(
        ctx,
        user,
        channel_name,
        local_user_id,
        local_role,
        context_user_id,
        context_menu_open,
        context_menu_anchor,
        role_menu_user_id,
        session,
        set_role,
        set_user_voice_state,
        disconnect_user,
        kick_user,
        debug_user_ids,
      )
      .absolute_position(menu_left, menu_top),
    )
    .into()
}

fn user_context_menu(
  ctx: &mut Ctx,
  user: &LobbyUser,
  channel_name: &str,
  local_user_id: UserId,
  local_role: Role,
  context_user_id: Signal<Option<UserId>>,
  context_menu_open: Signal<bool>,
  context_menu_anchor: Signal<Option<(f32, f32)>>,
  role_menu_user_id: Signal<Option<UserId>>,
  session: Option<ServerSession>,
  set_role: Option<SetRoleAction>,
  set_user_voice_state: Option<SetUserVoiceStateAction>,
  disconnect_user: Option<DisconnectUserAction>,
  kick_user: Option<KickUserAction>,
  debug_user_ids: bool,
) -> Column {
  let target_user_id = user.user_id;
  let can_moderate = target_user_id != local_user_id && local_role.can_moderate(user.role);
  let can_set_role = can_moderate && local_role.has_permission(Permission::ManageRoles) && set_role.is_some();
  let can_mute = can_moderate && local_role.has_permission(Permission::MuteOthers) && set_user_voice_state.is_some();
  let can_deafen =
    can_moderate && local_role.has_permission(Permission::DeafenOthers) && set_user_voice_state.is_some();
  let can_disconnect =
    can_moderate && local_role.has_permission(Permission::KickFromChannel) && disconnect_user.is_some();
  let can_kick = can_moderate && local_role.has_permission(Permission::KickFromServer) && kick_user.is_some();
  let role_menu_open = role_menu_user_id.get() == Some(target_user_id);
  let volume_control_key = format!("user-volume-{target_user_id}");
  let normalization_control_key = format!("user-normalization-{target_user_id}");
  let session_for_volume = session.clone();
  let mut menu = Column::new()
    .width(USER_CONTEXT_MENU_WIDTH)
    .spacing(0.0)
    .padding_vertical(8.0)
    .rounded(6.0)
    .background(BackgroundColor::Color(Color::from_hex("#15171A")))
    .border_inside(1.0, BackgroundColor::Color(Color::from_hex("#3A4047")))
    .child(user_context_header(ctx, user, channel_name, debug_user_ids))
    .child(menu_separator())
    .child(ctx.mount_keyed::<UserVolumeControl>(
      &volume_control_key,
      UserVolumeControlProps {
        user_id: target_user_id,
        session: session_for_volume,
      },
    ))
    .child(ctx.mount_keyed::<UserNormalizationToggle>(
      &normalization_control_key,
      UserNormalizationToggleProps {
        user_id: target_user_id,
        session,
      },
    ));

  if can_set_role || can_mute || can_deafen || can_disconnect || can_kick {
    menu = menu.child(menu_separator()).child(admin_section_label(ctx));
  }

  if let Some(set_role) = set_role.filter(|_| can_set_role) {
    let open_role_menu = role_menu_user_id.clone();
    let label = ctx.t("lobby.voice_menu.set_role");
    menu = menu.child(menu_item(ctx, "shield", &label, false).on_click(move |_| {
      if open_role_menu.get_untracked() == Some(target_user_id) {
        open_role_menu.set(None);
      } else {
        open_role_menu.set(Some(target_user_id));
      }
    }));

    if role_menu_open {
      menu = menu.child(role_submenu(
        ctx,
        target_user_id,
        user.role,
        local_role,
        set_role,
        context_user_id.clone(),
        context_menu_open.clone(),
        context_menu_anchor.clone(),
        role_menu_user_id.clone(),
      ));
    }
  }

  if let Some(set_user_voice_state) = set_user_voice_state.clone().filter(|_| can_mute) {
    let close_context = context_user_id.clone();
    let close_menu = context_menu_open.clone();
    let close_anchor = context_menu_anchor.clone();
    let close_role_menu = role_menu_user_id.clone();
    let muted = !user.muted;
    let deafened = user.deafened;
    let label = if user.muted {
      ctx.t("lobby.voice_menu.unmute")
    } else {
      ctx.t("lobby.voice_menu.mute")
    };
    let icon = if user.muted { "mic" } else { "mic-off" };
    menu = menu.child(menu_item(ctx, icon, &label, false).on_click(move |_| {
      set_user_voice_state.run((target_user_id, muted, deafened));
      close_user_context_menu(
        close_context.clone(),
        close_menu.clone(),
        close_anchor.clone(),
        close_role_menu.clone(),
      );
    }));
  }

  if let Some(set_user_voice_state) = set_user_voice_state.filter(|_| can_deafen) {
    let close_context = context_user_id.clone();
    let close_menu = context_menu_open.clone();
    let close_anchor = context_menu_anchor.clone();
    let close_role_menu = role_menu_user_id.clone();
    let muted = user.muted;
    let deafened = !user.deafened;
    let label = if user.deafened {
      ctx.t("lobby.voice_menu.undeafen")
    } else {
      ctx.t("lobby.voice_menu.deafen")
    };
    let icon = if user.deafened { "headphones" } else { "headphone-off" };
    menu = menu.child(menu_item(ctx, icon, &label, false).on_click(move |_| {
      set_user_voice_state.run((target_user_id, muted, deafened));
      close_user_context_menu(
        close_context.clone(),
        close_menu.clone(),
        close_anchor.clone(),
        close_role_menu.clone(),
      );
    }));
  }

  if let Some(disconnect_user) = disconnect_user.filter(|_| can_disconnect) {
    let close_context = context_user_id.clone();
    let close_menu = context_menu_open.clone();
    let close_anchor = context_menu_anchor.clone();
    let close_role_menu = role_menu_user_id.clone();
    let label = ctx.t("lobby.voice_menu.disconnect");
    menu = menu.child(menu_item(ctx, "phone-off", &label, false).on_click(move |_| {
      disconnect_user.run(target_user_id);
      close_user_context_menu(
        close_context.clone(),
        close_menu.clone(),
        close_anchor.clone(),
        close_role_menu.clone(),
      );
    }));
  }

  if let Some(kick_user) = kick_user.filter(|_| can_kick) {
    let close_context = context_user_id.clone();
    let close_menu = context_menu_open.clone();
    let close_anchor = context_menu_anchor.clone();
    let close_role_menu = role_menu_user_id.clone();
    let label = ctx.t("lobby.voice_menu.kick");
    menu = menu.child(menu_item(ctx, "user-x", &label, true).on_click(move |_| {
      kick_user.run(target_user_id);
      close_user_context_menu(
        close_context.clone(),
        close_menu.clone(),
        close_anchor.clone(),
        close_role_menu.clone(),
      );
    }));
  }

  menu
}

fn role_submenu(
  ctx: &mut Ctx,
  target_user_id: UserId,
  current_role: Role,
  local_role: Role,
  set_role: SetRoleAction,
  context_user_id: Signal<Option<UserId>>,
  context_menu_open: Signal<bool>,
  context_menu_anchor: Signal<Option<(f32, f32)>>,
  role_menu_user_id: Signal<Option<UserId>>,
) -> Element {
  let mut submenu = Column::new()
    .width(Dimension::Pct(100.0))
    .spacing(0.0)
    .padding_left(22.0)
    .padding_right(6.0);

  for role in ASSIGNABLE_ROLES {
    if role == current_role || !can_assign_role(local_role, role) {
      continue;
    }

    let close_context = context_user_id.clone();
    let close_menu = context_menu_open.clone();
    let close_anchor = context_menu_anchor.clone();
    let close_role_menu = role_menu_user_id.clone();
    let action = set_role.clone();
    let label = ctx.t(role_label_key(role));
    submenu = submenu.child(menu_item(ctx, "corner-down-right", &label, false).on_click(move |_| {
      action.run((target_user_id, role));
      close_user_context_menu(
        close_context.clone(),
        close_menu.clone(),
        close_anchor.clone(),
        close_role_menu.clone(),
      );
    }));
  }

  submenu.into()
}

fn can_assign_role(actor_role: Role, target_role: Role) -> bool {
  target_role != Role::Owner && (actor_role == Role::Owner || (target_role as u8) > actor_role as u8)
}

fn user_context_header(ctx: &mut Ctx, user: &LobbyUser, channel_name: &str, debug_user_ids: bool) -> Element {
  let role = ctx.t(role_meta_label_key(user.role));
  let username = user_display_name(user.user_id, &user.username, debug_user_ids);
  let meta = ctx.t_args(
    "lobby.voice_menu.user_meta",
    [("role", role.to_string()), ("channel", channel_name.to_owned())],
  );
  Row::new()
    .width(Dimension::Pct(100.0))
    .align_items(Alignment::Center)
    .spacing(10.0)
    .padding_vertical(10.0)
    .padding_horizontal(12.0)
    .child(user_avatar_sized(
      &user.username,
      user.speaking && !user.muted && !user.deafened,
      34.0,
    ))
    .child(
      Column::new()
        .width(Dimension::Pct(100.0))
        .flex(1.0)
        .spacing(2.0)
        .child(
          Text::new(&username)
            .variant(theme::TypographyStyle::Button)
            .color(theme::PaletteColor::TextPrimary),
        )
        .child(
          Text::new(&meta)
            .variant(theme::TypographyStyle::FieldLabel)
            .color(theme::PaletteColor::TextMuted),
        ),
    )
    .child(aligned_channel_icon_with_color(
      ctx,
      if user.deafened {
        "headphone-off"
      } else if user.muted {
        "mic-off"
      } else {
        "mic"
      },
      14.0,
      if user.deafened || user.muted {
        theme::palette().danger
      } else {
        theme::palette().success
      },
    ))
    .into()
}

#[derive(Clone)]
struct UserVolumeControlProps {
  user_id: UserId,
  session: Option<ServerSession>,
}

impl PartialEq for UserVolumeControlProps {
  fn eq(&self, other: &Self) -> bool {
    self.user_id == other.user_id && self.session.is_some() == other.session.is_some()
  }
}

impl DevtoolsInspectable for UserVolumeControlProps {
  fn inspect(&self, formatter: &mut DevtoolsFormatter<'_>) {
    formatter.buffer_mut().push(ComponentInfo::with_value(
      "user_id",
      std::any::type_name::<UserId>(),
      self.user_id.to_string(),
    ));
  }
}

struct UserVolumeControl {
  user_id: Signal<UserId>,
  server_id: Signal<Option<String>>,
  value: Signal<i32>,
  apply_session: Arc<Mutex<Option<ServerSession>>>,
  last_applied_volume: Arc<Mutex<i32>>,
  apply_interval: Interval,
}

#[derive(Clone)]
struct UserNormalizationToggleProps {
  user_id: UserId,
  session: Option<ServerSession>,
}

impl PartialEq for UserNormalizationToggleProps {
  fn eq(&self, other: &Self) -> bool {
    self.user_id == other.user_id && self.session.is_some() == other.session.is_some()
  }
}

impl DevtoolsInspectable for UserNormalizationToggleProps {
  fn inspect(&self, formatter: &mut DevtoolsFormatter<'_>) {
    formatter.buffer_mut().push(ComponentInfo::with_value(
      "user_id",
      std::any::type_name::<UserId>(),
      self.user_id.to_string(),
    ));
  }
}

struct UserNormalizationToggle {
  user_id: Signal<UserId>,
  server_id: Signal<Option<String>>,
  enabled: Signal<bool>,
}

impl Component for UserNormalizationToggle {
  type Props = UserNormalizationToggleProps;

  fn create(ctx: &mut Ctx) -> Self {
    let props = ctx.props::<Self::Props>().clone();
    let storage = ctx.use_context::<Storage>();
    let server_id = props
      .session
      .as_ref()
      .and_then(|session| session.info().map(|info| info.address));
    let initial = load_user_normalization(
      storage.as_ref(),
      props.session.as_ref(),
      server_id.as_deref(),
      props.user_id,
    );
    if let Some(session) = props.session.as_ref() {
      session.set_user_normalization(props.user_id, initial);
    }

    Self {
      user_id: ctx.signal(props.user_id),
      server_id: ctx.signal(server_id),
      enabled: ctx.signal(initial),
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let props = ctx.props::<Self::Props>().clone();
    let storage = ctx.use_context::<Storage>();
    let server_id = props
      .session
      .as_ref()
      .and_then(|session| session.info().map(|info| info.address));

    if self.user_id.get_untracked() != props.user_id || self.server_id.get_untracked() != server_id {
      let enabled = load_user_normalization(
        storage.as_ref(),
        props.session.as_ref(),
        server_id.as_deref(),
        props.user_id,
      );
      self.user_id.set(props.user_id);
      self.server_id.set(server_id.clone());
      self.enabled.set(enabled);
      if let Some(session) = props.session.as_ref() {
        session.set_user_normalization(props.user_id, enabled);
      }
    }

    user_normalization_toggle(
      ctx,
      self.enabled.clone(),
      props.session.clone(),
      storage,
      server_id,
      props.user_id,
    )
  }
}

impl Component for UserVolumeControl {
  type Props = UserVolumeControlProps;

  fn create(ctx: &mut Ctx) -> Self {
    let props = ctx.props::<Self::Props>().clone();
    let storage = ctx.use_context::<Storage>();
    let initial_server_id = props
      .session
      .as_ref()
      .and_then(|session| session.info().map(|info| info.address));
    let initial = load_user_volume(
      storage.as_ref(),
      props.session.as_ref(),
      initial_server_id.as_deref(),
      props.user_id,
    );
    if let Some(session) = props.session.as_ref() {
      session.set_user_volume(props.user_id, initial);
    }
    let user_id = ctx.signal(props.user_id);
    let server_id = ctx.signal(initial_server_id);
    let value = ctx.signal(initial);
    let apply_session = Arc::new(Mutex::new(props.session.clone()));
    let last_applied_volume = Arc::new(Mutex::new(initial));
    let apply_interval = {
      let apply_session = apply_session.clone();
      let user_id = user_id.clone();
      let value = value.clone();
      let last_applied_volume = last_applied_volume.clone();
      let interval = ctx.create_interval(Duration::from_millis(16), move || {
        let volume = value.get_untracked().clamp(0, 100);
        let mut last_applied_volume = last_applied_volume
          .lock()
          .expect("user volume last-applied lock poisoned");
        if *last_applied_volume != volume {
          if let Some(session) = apply_session
            .lock()
            .expect("user volume session lock poisoned")
            .as_ref()
          {
            session.set_user_volume(user_id.get_untracked(), volume);
          }
          *last_applied_volume = volume;
        }
      });
      interval.start();
      interval
    };

    Self {
      user_id,
      server_id,
      value,
      apply_session,
      last_applied_volume,
      apply_interval,
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let props = ctx.props::<Self::Props>().clone();
    let storage = ctx.use_context::<Storage>();
    let server_id = props
      .session
      .as_ref()
      .and_then(|session| session.info().map(|info| info.address));

    *self.apply_session.lock().expect("user volume session lock poisoned") = props.session.clone();

    if self.user_id.get_untracked() != props.user_id || self.server_id.get_untracked() != server_id {
      let value = load_user_volume(
        storage.as_ref(),
        props.session.as_ref(),
        server_id.as_deref(),
        props.user_id,
      );
      self.user_id.set(props.user_id);
      self.server_id.set(server_id.clone());
      self.value.set(value);
      if let Some(session) = props.session.as_ref() {
        session.set_user_volume(props.user_id, value);
      }
      *self
        .last_applied_volume
        .lock()
        .expect("user volume last-applied lock poisoned") = value;
    }

    let save_storage = storage.clone();
    let save_session = props.session.clone();
    let save_server_id = server_id.clone();
    let save_user_id = props.user_id;
    user_volume_control(
      ctx,
      self.value.clone(),
      Arc::new(move |volume| {
        let volume = volume.clamp(0, 100);
        if let Some(session) = save_session.as_ref() {
          session.set_user_volume(save_user_id, volume);
        }
        if let (Some(storage), Some(server_id)) = (save_storage.as_ref(), save_server_id.as_deref()) {
          let _ = storage.save_volume_override(server_id, save_user_id, volume);
        }
      }),
    )
  }

  fn on_unmounted(&self) {
    self.apply_interval.stop();
  }
}

fn load_user_volume(
  storage: Option<&Storage>,
  session: Option<&ServerSession>,
  server_id: Option<&str>,
  user_id: UserId,
) -> i32 {
  storage
    .zip(server_id)
    .and_then(|(storage, server_id)| storage.load_volume_override(server_id, user_id).ok().flatten())
    .or_else(|| session.map(|session| session.user_volume(user_id)))
    .unwrap_or(DEFAULT_USER_VOLUME)
}

fn load_user_normalization(
  storage: Option<&Storage>,
  session: Option<&ServerSession>,
  server_id: Option<&str>,
  user_id: UserId,
) -> bool {
  storage
    .zip(server_id)
    .and_then(|(storage, server_id)| storage.load_user_normalization(server_id, user_id).ok())
    .or_else(|| session.map(|session| session.user_normalization(user_id)))
    .unwrap_or(false)
}

fn user_normalization_toggle(
  ctx: &mut Ctx,
  enabled: Signal<bool>,
  session: Option<ServerSession>,
  storage: Option<Storage>,
  server_id: Option<String>,
  user_id: UserId,
) -> Element {
  let label = ctx.t("lobby.voice_menu.normalize_voice");
  let currently_enabled = enabled.get();
  let toggle_enabled = enabled.clone();

  Row::new()
    .width(Dimension::Pct(100.0))
    .height(38.0)
    .align_items(Alignment::Center)
    .justify(Justify::SpaceBetween)
    .spacing(8.0)
    .padding_horizontal(USER_CONTEXT_MENU_HORIZONTAL_PADDING)
    .rounded(5.0)
    .cursor(CursorIcon::Pointer)
    .hovered_style(Style::new().background(BackgroundColor::Color(Color::from_hex("#232830"))))
    .on_click(move |_| {
      let next = !toggle_enabled.get_untracked();
      toggle_enabled.set(next);
      if let Some(session) = session.as_ref() {
        session.set_user_normalization(user_id, next);
      }
      if let (Some(storage), Some(server_id)) = (storage.as_ref(), server_id.as_deref()) {
        let _ = storage.save_user_normalization(server_id, user_id, next);
      }
    })
    .child(
      Row::new()
        .width(Dimension::Pct(100.0))
        .flex(1.0)
        .align_items(Alignment::Center)
        .spacing(8.0)
        .child(aligned_channel_icon_with_color(
          ctx,
          "waves",
          14.0,
          if currently_enabled {
            theme::palette().accent
          } else {
            theme::palette().text_secondary
          },
        ))
        .child(
          Text::new(&label)
            .variant(theme::TypographyStyle::Caption)
            .color(theme::PaletteColor::TextSecondary),
        ),
    )
    .child(settings_toggle_track(currently_enabled))
    .into()
}

fn user_volume_control(ctx: &mut Ctx, value: Signal<i32>, on_blur: PercentSliderSaveAction) -> Element {
  let label = ctx.t("lobby.voice_menu.user_volume");

  Column::new()
    .width(Dimension::Pct(100.0))
    .spacing(8.0)
    .padding_vertical(8.0)
    .padding_horizontal(USER_CONTEXT_MENU_HORIZONTAL_PADDING)
    .child(
      Row::new()
        .width(Dimension::Pct(100.0))
        .align_items(Alignment::Center)
        .spacing(8.0)
        .child(aligned_channel_icon_with_color(
          ctx,
          "volume-2",
          14.0,
          theme::palette().text_secondary,
        ))
        .child(
          Text::new(&label)
            .variant(theme::TypographyStyle::Caption)
            .color(theme::PaletteColor::TextSecondary)
            .width(Dimension::Pct(100.0))
            .flex(1.0),
        ),
    )
    .child(percent_slider_control(
      ctx,
      value,
      USER_VOLUME_SLIDER_WIDTH,
      USER_VOLUME_TRACK_WIDTH,
      USER_VOLUME_VALUE_WIDTH,
      USER_VOLUME_VALUE_SPACING,
      on_blur,
    ))
    .into()
}

fn menu_item(ctx: &mut Ctx, icon: &'static str, label: &str, danger: bool) -> Row {
  let color = if danger {
    theme::palette().danger
  } else {
    theme::palette().text_secondary
  };

  Row::new()
    .width(Dimension::Pct(100.0))
    .height(34.0)
    .align_items(Alignment::Center)
    .spacing(10.0)
    .padding_horizontal(12.0)
    .rounded(5.0)
    .cursor(CursorIcon::Pointer)
    .hovered_style(Style::new().background(BackgroundColor::Color(Color::from_hex("#232830"))))
    .child(aligned_channel_icon_with_color(ctx, icon, 14.0, color))
    .child(
      Text::new(label)
        .variant(theme::TypographyStyle::Caption)
        .color(if danger {
          theme::PaletteColor::Danger
        } else {
          theme::PaletteColor::TextSecondary
        })
        .width(Dimension::Pct(100.0))
        .flex(1.0),
    )
}

fn menu_separator() -> Element {
  Row::new()
    .width(Dimension::Pct(100.0))
    .height(1.0)
    .background(BackgroundColor::Palette(theme::PaletteColor::Border))
    .into()
}

fn admin_section_label(ctx: &mut Ctx) -> Element {
  let label = ctx.t("lobby.voice_menu.admin_actions");

  Row::new()
    .width(Dimension::Pct(100.0))
    .padding_top(9.0)
    .padding_bottom(4.0)
    .padding_horizontal(10.0)
    .child(
      Text::new(&label)
        .variant(theme::TypographyStyle::FieldLabel)
        .color(theme::PaletteColor::TextMuted),
    )
    .into()
}

fn role_label_key(role: Role) -> &'static str {
  match role {
    Role::Owner => "lobby.role.owner",
    Role::Admin => "lobby.role.admin",
    Role::Moderator => "lobby.role.moderator",
    Role::User => "lobby.role.member",
  }
}

fn role_meta_label_key(role: Role) -> &'static str {
  match role {
    Role::Owner => "lobby.role_meta.owner",
    Role::Admin => "lobby.role_meta.admin",
    Role::Moderator => "lobby.role_meta.moderator",
    Role::User => "lobby.role_meta.member",
  }
}

fn user_avatar(name: &str, active: bool) -> Element {
  user_avatar_sized(name, active, 22.0)
}

fn user_avatar_sized(name: &str, active: bool, size: f32) -> Element {
  let avatar = Row::new()
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
    .child(Text::styled(
      &initials_for(name),
      TextStyle {
        font_family: Arc::from("Inter"),
        font_size: (size * 0.5).round(),
        line_height: 1.0,
        weight: FontWeight::Bold,
        color: if active {
          theme::palette().text_primary
        } else {
          theme::palette().text_secondary
        },
        ..TextStyle::default()
      },
    ));

  avatar.into()
}

fn user_voice_icons(ctx: &mut Ctx, user: &LobbyUser, streaming: bool) -> Element {
  let mut icons = Row::new()
    .align_items(Alignment::Center)
    .justify(Justify::End)
    .spacing(6.0);

  if streaming {
    icons = icons.child(aligned_channel_icon_with_color(
      ctx,
      "monitor-up",
      14.0,
      theme::palette().accent,
    ));
  }

  if user.deafened {
    icons = icons
      .child(aligned_channel_icon_with_color(
        ctx,
        "headphone-off",
        14.0,
        theme::palette().danger,
      ))
      .child(aligned_channel_icon_with_color(
        ctx,
        "mic-off",
        14.0,
        theme::palette().danger,
      ));
  } else if user.muted {
    icons = icons.child(aligned_channel_icon_with_color(
      ctx,
      "mic-off",
      14.0,
      theme::palette().danger,
    ));
  }

  icons.into()
}

fn initials_for(name: &str) -> String {
  let initials = name
    .chars()
    .filter(|ch| ch.is_alphanumeric())
    .flat_map(|ch| ch.to_uppercase())
    .take(1)
    .collect::<String>();

  if initials.is_empty() { "?".to_owned() } else { initials }
}
