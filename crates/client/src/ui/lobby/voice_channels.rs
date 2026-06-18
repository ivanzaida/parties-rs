use std::sync::Arc;

use lurq::{
  app::{
    component::{Component, ComponentInfo, DevtoolsFormatter, DevtoolsInspectable},
    ctx::{Ctx, Modal, Root},
    events::{MouseButton, MouseEvent},
  },
  components::{Column, Row, Text},
  core::Signal,
  layout::{
    Alignment,
    layout_kind::Justify,
    text_style::{FontWeight, TextStyle},
  },
  node::{BackgroundColor, CursorIcon, Element, Style, color::Color, dimension::Dimension},
};

use super::{
  WatchStreamAction,
  user_context_overlay::{
    DisconnectUserAction, KickUserAction, SetRoleAction, SetUserVoiceStateAction, UserContextOverlay,
    UserContextOverlayProps, UserMenuActions, close_user_context_menu,
  },
};
use crate::{
  network::protocol::{ChannelId, Role, UserId},
  session::{LobbyUser, ServerSession},
  storage::Storage,
  theme,
  ui::lobby::{
    channel_section::{aligned_channel_icon, aligned_channel_icon_with_color, section_head},
    model::{VoiceChannelRowModel, VoiceUserRowModel},
    session_identity::{same_optional_session, same_session},
    shared::user_display_name,
  },
};

pub(super) type JoinChannelTask = lurq::app::ctx::FutureAction<JoinChannelRequest, (), String>;

#[derive(Clone, Copy)]
pub(super) struct JoinChannelRequest {
  pub channel_id: ChannelId,
  pub previous_channel_id: Option<ChannelId>,
}

#[derive(Clone)]
pub(super) struct JoinChannelAction {
  session: ServerSession,
  task: JoinChannelTask,
}

impl JoinChannelAction {
  pub(super) fn new(session: ServerSession, task: JoinChannelTask) -> Self {
    Self { session, task }
  }

  pub(super) fn run(&self, channel_id: ChannelId) {
    let previous_channel_id = self.session.selected_channel_id();
    self.session.select_channel(channel_id);
    self.session.open_stream_browser(channel_id);
    self.task.run(JoinChannelRequest {
      channel_id,
      previous_channel_id,
    });
  }
}

impl PartialEq for JoinChannelAction {
  fn eq(&self, other: &Self) -> bool {
    same_session(&self.session, &other.session)
  }
}

#[derive(Clone, Default)]
pub(super) struct VoiceChannelActions {
  pub session: Option<ServerSession>,
  pub join_channel: Option<JoinChannelAction>,
  pub watch_stream: Option<WatchStreamAction>,
}

impl PartialEq for VoiceChannelActions {
  fn eq(&self, other: &Self) -> bool {
    same_optional_session(self.session.as_ref(), other.session.as_ref())
      && self.join_channel == other.join_channel
      && self.watch_stream.is_some() == other.watch_stream.is_some()
  }
}

#[derive(Clone)]
pub(super) struct VoiceChannelsProps {
  pub channels: Vec<VoiceChannelRowModel>,
  pub disconnected: bool,
  pub local_user_id: UserId,
  pub local_role: Role,
  pub debug_user_ids: bool,
  pub storage: Option<Storage>,
  pub actions: VoiceChannelActions,
}

impl PartialEq for VoiceChannelsProps {
  fn eq(&self, other: &Self) -> bool {
    self.channels == other.channels
      && self.disconnected == other.disconnected
      && self.local_user_id == other.local_user_id
      && self.local_role == other.local_role
      && self.debug_user_ids == other.debug_user_ids
      && self.storage.is_some() == other.storage.is_some()
      && self.actions == other.actions
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
      format!(
        "{:?}",
        self
          .channels
          .iter()
          .find(|channel| channel.selected)
          .map(|channel| channel.channel.id)
      ),
    ));
    formatter.buffer_mut().push(ComponentInfo::with_value(
      "users_by_channel",
      std::any::type_name::<usize>(),
      self
        .channels
        .iter()
        .map(|channel| channel.users.len())
        .sum::<usize>()
        .to_string(),
    ));
    formatter.buffer_mut().push(ComponentInfo::with_value(
      "streaming_user_ids",
      std::any::type_name::<usize>(),
      self
        .channels
        .iter()
        .flat_map(|channel| channel.users.iter())
        .filter(|user| user.streaming)
        .count()
        .to_string(),
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
              .channels
              .iter()
              .flat_map(|channel| channel.users.iter())
              .any(|user| user.user.user_id == user_id)
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
    let session = props.actions.session.clone();
    let set_role = session.clone().map(|session| set_role_action(ctx, session));
    let set_user_voice_state = session.clone().map(|session| set_user_voice_state_action(ctx, session));
    let disconnect_user = session.clone().map(|session| disconnect_user_action(ctx, session));
    let kick_user = session.clone().map(|session| kick_user_action(ctx, session));
    let channels_for_menu = props.channels.clone();
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
      let join_channel = props.actions.join_channel.clone();
      let watch_stream = props.actions.watch_stream.clone();
      let debug_user_ids = props.debug_user_ids;
      let context_user_id = self.context_user_id.clone();
      let context_menu_open = self.context_menu_open.clone();
      let context_menu_anchor = self.context_menu_anchor.clone();
      let role_menu_user_id = self.role_menu_user_id.clone();
      let session_for_channels = session.clone();
      let channel_groups = ctx.for_each(
        props.channels,
        |channel| channel.channel.id,
        move |ctx, channel| {
          channel_group(
            ctx,
            &channel,
            join_channel.as_ref(),
            watch_stream.as_ref(),
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

    if let Some((modal_user, modal_channel_name)) =
      context_menu_target(&channels_for_menu, self.context_user_id.get_untracked())
    {
      let modal_context_user_id = self.context_user_id.clone();
      let modal_context_menu_open = self.context_menu_open.clone();
      let modal_anchor = self.context_menu_anchor.clone();
      let modal_role_menu_user_id = self.role_menu_user_id.clone();
      let modal_session = session.clone();
      let modal_actions = UserMenuActions {
        set_role: set_role.clone(),
        set_user_voice_state: set_user_voice_state.clone(),
        disconnect_user: disconnect_user.clone(),
        kick_user: kick_user.clone(),
      };
      let local_user_id = props.local_user_id;
      let local_role = props.local_role;
      body = body.child(
        Modal::new(ctx.mount::<UserContextOverlay>(UserContextOverlayProps {
          user: modal_user,
          channel_name: modal_channel_name,
          local_user_id,
          local_role,
          context_user_id: modal_context_user_id,
          context_menu_open: modal_context_menu_open,
          context_menu_anchor: modal_anchor,
          role_menu_user_id: modal_role_menu_user_id,
          session: modal_session,
          storage: props.storage.clone(),
          actions: modal_actions,
          debug_user_ids: props.debug_user_ids,
        }))
        .open(self.context_menu_open.clone())
        .target(Root),
      );
    }

    body
  }
}

fn context_menu_target(
  channels: &[VoiceChannelRowModel],
  target_user_id: Option<UserId>,
) -> Option<(LobbyUser, String)> {
  let target_user_id = target_user_id?;
  channels.iter().find_map(|channel| {
    channel
      .users
      .iter()
      .find(|row| row.user.user_id == target_user_id)
      .map(|row| (row.user.clone(), channel.channel.name.clone()))
  })
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
  channel: &VoiceChannelRowModel,
  join_channel: Option<&JoinChannelAction>,
  watch_stream: Option<&WatchStreamAction>,
  session: Option<ServerSession>,
  context_user_id: Signal<Option<UserId>>,
  context_menu_open: Signal<bool>,
  context_menu_anchor: Signal<Option<(f32, f32)>>,
  role_menu_user_id: Signal<Option<UserId>>,
  debug_user_ids: bool,
) -> Element {
  let mut users = channel.users.clone();
  users.sort_by(|left, right| {
    left
      .user
      .username
      .to_lowercase()
      .cmp(&right.user.username.to_lowercase())
      .then_with(|| left.user.username.cmp(&right.user.username))
      .then_with(|| left.user.user_id.cmp(&right.user.user_id))
  });

  let user_rows = ctx.for_each(
    users,
    |row| row.user.user_id,
    move |ctx, row| {
      ctx.mount::<ChannelUserRow>(ChannelUserRowProps {
        channel_id: channel.channel.id,
        channel_user_count: channel.users.len(),
        model: row,
        watch_stream: watch_stream.cloned(),
        context_user_id: context_user_id.clone(),
        context_menu_open: context_menu_open.clone(),
        context_menu_anchor: context_menu_anchor.clone(),
        role_menu_user_id: role_menu_user_id.clone(),
        debug_user_ids,
      })
    },
  );

  Column::new()
    .width(Dimension::Pct(100.0))
    .spacing(2.0)
    .child(channel_row(ctx, channel, join_channel, session))
    .with_children(user_rows)
    .into()
}

fn channel_row(
  ctx: &mut Ctx,
  model: &VoiceChannelRowModel,
  join_channel: Option<&JoinChannelAction>,
  session: Option<ServerSession>,
) -> Element {
  let selected = model.selected;
  let channel = &model.channel;
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

#[derive(Clone)]
struct ChannelUserRowProps {
  channel_id: ChannelId,
  channel_user_count: usize,
  model: VoiceUserRowModel,
  watch_stream: Option<WatchStreamAction>,
  context_user_id: Signal<Option<UserId>>,
  context_menu_open: Signal<bool>,
  context_menu_anchor: Signal<Option<(f32, f32)>>,
  role_menu_user_id: Signal<Option<UserId>>,
  debug_user_ids: bool,
}

impl PartialEq for ChannelUserRowProps {
  fn eq(&self, other: &Self) -> bool {
    self.channel_id == other.channel_id
      && self.channel_user_count == other.channel_user_count
      && self.model == other.model
      && self.watch_stream.is_some() == other.watch_stream.is_some()
      && self.debug_user_ids == other.debug_user_ids
  }
}

impl DevtoolsInspectable for ChannelUserRowProps {}

struct ChannelUserRow;

impl Component for ChannelUserRow {
  type Props = ChannelUserRowProps;

  fn create(ctx: &mut Ctx) -> Self {
    let props = ctx.props::<Self::Props>();
    let user = &props.model.user;
    tracing::info!(target: "lobby::voice",
      "[lobby:voice] channel user row mounted: channel={} user={} local={} channel_users={} muted={} deafened={} speaking={} streaming={}",
      props.channel_id,
      user.user_id,
      props.model.local,
      props.channel_user_count,
      user.muted,
      user.deafened,
      user.speaking,
      props.model.streaming
    );
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let props = ctx.props::<Self::Props>().clone();
    channel_user_row_view(
      ctx,
      props.channel_id,
      &props.model,
      props.watch_stream.as_ref(),
      props.context_user_id,
      props.context_menu_open,
      props.context_menu_anchor,
      props.role_menu_user_id,
      props.debug_user_ids,
    )
  }
}

fn channel_user_row_view(
  ctx: &mut Ctx,
  channel_id: ChannelId,
  model: &VoiceUserRowModel,
  watch_stream: Option<&WatchStreamAction>,
  context_user_id: Signal<Option<UserId>>,
  context_menu_open: Signal<bool>,
  context_menu_anchor: Signal<Option<(f32, f32)>>,
  role_menu_user_id: Signal<Option<UserId>>,
  debug_user_ids: bool,
) -> Element {
  let user = &model.user;
  let local = model.local;
  let streaming = model.streaming;
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
  tracing::info!(target: "lobby::voice",
    "[lobby:voice] channel user row rendered: channel={channel_id} user={user_id} local={local} muted={} deafened={} speaking={} streaming={streaming}",
    user.muted,
    user.deafened,
    user.speaking
  );

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
        .variant(theme::TypographyStyle::Description)
        .color(theme::PaletteColor::TextSecondary),
    )
    .child(user_voice_icons(ctx, user, streaming));

  Column::new().width(Dimension::Pct(100.0)).child(row).into()
}

fn user_avatar(name: &str, active: bool) -> Element {
  user_avatar_sized(name, active, 22.0)
}

pub(super) fn user_avatar_sized(name: &str, active: bool, size: f32) -> Element {
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
