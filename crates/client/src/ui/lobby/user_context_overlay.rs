use std::sync::{Arc, Mutex};

use lurq::{
  app::{
    component::{Component, ComponentInfo, DevtoolsFormatter, DevtoolsInspectable},
    ctx::Ctx,
    events::MouseButton,
  },
  components::{Column, Row, Stack, Text},
  core::Signal,
  layout::{Alignment, layout_kind::Justify},
  node::{BackgroundColor, CursorIcon, Element, Style, color::Color, dimension::Dimension},
};

use crate::{
  network::protocol::{Permission, Role, UserId},
  session::{LobbyUser, ServerSession},
  storage::Storage,
  theme,
  ui::{
    app_chrome::{CHROME_HEIGHT, content_height, modal_y},
    common::percent_slider::{PercentSliderSaveAction, percent_slider_control},
    lobby::{
      channel_section::aligned_channel_icon_with_color, shared::user_display_name, voice_channels::user_avatar_sized,
    },
    settings::settings_toggle_track,
  },
};

pub(super) type SetRoleAction = lurq::app::ctx::FutureAction<(UserId, Role), (), String>;
pub(super) type SetUserVoiceStateAction = lurq::app::ctx::FutureAction<(UserId, bool, bool), (), String>;
pub(super) type DisconnectUserAction = lurq::app::ctx::FutureAction<UserId, (), String>;
pub(super) type KickUserAction = lurq::app::ctx::FutureAction<UserId, (), String>;

#[derive(Clone, Default)]
pub(super) struct UserMenuActions {
  pub set_role: Option<SetRoleAction>,
  pub set_user_voice_state: Option<SetUserVoiceStateAction>,
  pub disconnect_user: Option<DisconnectUserAction>,
  pub kick_user: Option<KickUserAction>,
}

impl PartialEq for UserMenuActions {
  fn eq(&self, other: &Self) -> bool {
    self.set_role.is_some() == other.set_role.is_some()
      && self.set_user_voice_state.is_some() == other.set_user_voice_state.is_some()
      && self.disconnect_user.is_some() == other.disconnect_user.is_some()
      && self.kick_user.is_some() == other.kick_user.is_some()
  }
}

const USER_CONTEXT_MENU_WIDTH: f32 = 286.0;
const USER_CONTEXT_MENU_HORIZONTAL_PADDING: f32 = 10.0;
const USER_VOLUME_SLIDER_WIDTH: f32 = USER_CONTEXT_MENU_WIDTH - USER_CONTEXT_MENU_HORIZONTAL_PADDING * 2.0;
const USER_VOLUME_VALUE_WIDTH: f32 = 42.0;
const USER_VOLUME_VALUE_SPACING: f32 = 10.0;
const USER_VOLUME_TRACK_WIDTH: f32 = USER_VOLUME_SLIDER_WIDTH - USER_VOLUME_VALUE_WIDTH - USER_VOLUME_VALUE_SPACING;
const DEFAULT_USER_VOLUME: i32 = 100;
const ASSIGNABLE_ROLES: [Role; 3] = [Role::Admin, Role::Moderator, Role::User];

#[derive(Clone)]
pub(super) struct UserContextOverlayProps {
  pub user: LobbyUser,
  pub channel_name: String,
  pub local_user_id: UserId,
  pub local_role: Role,
  pub context_user_id: Signal<Option<UserId>>,
  pub context_menu_open: Signal<bool>,
  pub context_menu_anchor: Signal<Option<(f32, f32)>>,
  pub role_menu_user_id: Signal<Option<UserId>>,
  pub session: Option<ServerSession>,
  pub storage: Option<Storage>,
  pub actions: UserMenuActions,
  pub debug_user_ids: bool,
}

impl PartialEq for UserContextOverlayProps {
  fn eq(&self, other: &Self) -> bool {
    self.user == other.user
      && self.channel_name == other.channel_name
      && self.local_user_id == other.local_user_id
      && self.local_role == other.local_role
      && same_optional_session(self.session.as_ref(), other.session.as_ref())
      && self.storage.is_some() == other.storage.is_some()
      && self.actions == other.actions
      && self.debug_user_ids == other.debug_user_ids
  }
}

impl DevtoolsInspectable for UserContextOverlayProps {
  fn inspect(&self, formatter: &mut DevtoolsFormatter<'_>) {
    formatter.buffer_mut().push(ComponentInfo::with_value(
      "user_id",
      std::any::type_name::<UserId>(),
      self.user.user_id.to_string(),
    ));
    formatter.buffer_mut().push(ComponentInfo::with_value(
      "channel_name",
      std::any::type_name::<String>(),
      self.channel_name.clone(),
    ));
  }
}

pub(super) struct UserContextOverlay;

impl Component for UserContextOverlay {
  type Props = UserContextOverlayProps;

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let props = ctx.props::<Self::Props>().clone();
    user_context_overlay(ctx, props)
  }
}

pub(super) fn close_user_context_menu(
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

fn user_context_overlay(ctx: &mut Ctx, props: UserContextOverlayProps) -> Element {
  let window = ctx.window();
  let window_width = window.logical_width();
  let modal_height = content_height(ctx);
  let (anchor_x, anchor_y) = props.context_menu_anchor.get().unwrap_or((250.0, CHROME_HEIGHT + 96.0));
  let menu_left = (anchor_x + 8.0).clamp(8.0, (window_width - USER_CONTEXT_MENU_WIDTH - 8.0).max(8.0));
  let menu_top = modal_y(anchor_y).clamp(8.0, (modal_height - 8.0).max(8.0));
  let close_left_user_id = props.context_user_id.clone();
  let close_left_menu = props.context_menu_open.clone();
  let close_left_anchor = props.context_menu_anchor.clone();
  let close_left_role_menu = props.role_menu_user_id.clone();
  let close_right_user_id = props.context_user_id.clone();
  let close_right_menu = props.context_menu_open.clone();
  let close_right_anchor = props.context_menu_anchor.clone();
  let close_right_role_menu = props.role_menu_user_id.clone();
  let close_middle_user_id = props.context_user_id.clone();
  let close_middle_menu = props.context_menu_open.clone();
  let close_middle_anchor = props.context_menu_anchor.clone();
  let close_middle_role_menu = props.role_menu_user_id.clone();

  Stack::new()
    .width(window_width)
    .height(modal_height)
    .absolute(0.0, 0.0, window_width, modal_height)
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
    .child(user_context_menu(ctx, props).absolute_position(menu_left, menu_top))
    .into()
}

fn user_context_menu(ctx: &mut Ctx, props: UserContextOverlayProps) -> Column {
  let target_user_id = props.user.user_id;
  let can_moderate = target_user_id != props.local_user_id && props.local_role.can_moderate(props.user.role);
  let can_set_role =
    can_moderate && props.local_role.has_permission(Permission::ManageRoles) && props.actions.set_role.is_some();
  let can_mute = can_moderate
    && props.local_role.has_permission(Permission::MuteOthers)
    && props.actions.set_user_voice_state.is_some();
  let can_deafen = can_moderate
    && props.local_role.has_permission(Permission::DeafenOthers)
    && props.actions.set_user_voice_state.is_some();
  let can_disconnect = can_moderate
    && props.local_role.has_permission(Permission::KickFromChannel)
    && props.actions.disconnect_user.is_some();
  let can_kick =
    can_moderate && props.local_role.has_permission(Permission::KickFromServer) && props.actions.kick_user.is_some();
  let role_menu_open = props.role_menu_user_id.get() == Some(target_user_id);
  let volume_control_key = format!("user-volume-{target_user_id}");
  let normalization_control_key = format!("user-normalization-{target_user_id}");
  let session_for_volume = props.session.clone();
  let storage_for_volume = props.storage.clone();
  let mut menu = Column::new()
    .width(USER_CONTEXT_MENU_WIDTH)
    .spacing(0.0)
    .padding_vertical(8.0)
    .rounded(6.0)
    .background(BackgroundColor::Color(Color::from_hex("#15171A")))
    .border_inside(1.0, BackgroundColor::Color(Color::from_hex("#3A4047")))
    .child(user_context_header(
      ctx,
      &props.user,
      &props.channel_name,
      props.debug_user_ids,
    ))
    .child(menu_separator())
    .child(ctx.mount_keyed::<UserVolumeControl>(
      &volume_control_key,
      UserVolumeControlProps {
        user_id: target_user_id,
        session: session_for_volume,
        storage: storage_for_volume,
      },
    ))
    .child(ctx.mount_keyed::<UserNormalizationToggle>(
      &normalization_control_key,
      UserNormalizationToggleProps {
        user_id: target_user_id,
        session: props.session.clone(),
        storage: props.storage.clone(),
      },
    ));

  if can_set_role || can_mute || can_deafen || can_disconnect || can_kick {
    menu = menu.child(menu_separator()).child(admin_section_label(ctx));
  }

  if let Some(set_role) = props.actions.set_role.clone().filter(|_| can_set_role) {
    let open_role_menu = props.role_menu_user_id.clone();
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
        props.user.role,
        props.local_role,
        set_role,
        props.context_user_id.clone(),
        props.context_menu_open.clone(),
        props.context_menu_anchor.clone(),
        props.role_menu_user_id.clone(),
      ));
    }
  }

  if let Some(set_user_voice_state) = props.actions.set_user_voice_state.clone().filter(|_| can_mute) {
    let close_context = props.context_user_id.clone();
    let close_menu = props.context_menu_open.clone();
    let close_anchor = props.context_menu_anchor.clone();
    let close_role_menu = props.role_menu_user_id.clone();
    let muted = !props.user.muted;
    let deafened = props.user.deafened;
    let label = if props.user.muted {
      ctx.t("lobby.voice_menu.unmute")
    } else {
      ctx.t("lobby.voice_menu.mute")
    };
    let icon = if props.user.muted { "mic" } else { "mic-off" };
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

  if let Some(set_user_voice_state) = props.actions.set_user_voice_state.filter(|_| can_deafen) {
    let close_context = props.context_user_id.clone();
    let close_menu = props.context_menu_open.clone();
    let close_anchor = props.context_menu_anchor.clone();
    let close_role_menu = props.role_menu_user_id.clone();
    let muted = props.user.muted;
    let deafened = !props.user.deafened;
    let label = if props.user.deafened {
      ctx.t("lobby.voice_menu.undeafen")
    } else {
      ctx.t("lobby.voice_menu.deafen")
    };
    let icon = if props.user.deafened {
      "headphones"
    } else {
      "headphone-off"
    };
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

  if let Some(disconnect_user) = props.actions.disconnect_user.filter(|_| can_disconnect) {
    let close_context = props.context_user_id.clone();
    let close_menu = props.context_menu_open.clone();
    let close_anchor = props.context_menu_anchor.clone();
    let close_role_menu = props.role_menu_user_id.clone();
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

  if let Some(kick_user) = props.actions.kick_user.filter(|_| can_kick) {
    let close_context = props.context_user_id.clone();
    let close_menu = props.context_menu_open.clone();
    let close_anchor = props.context_menu_anchor.clone();
    let close_role_menu = props.role_menu_user_id.clone();
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
  storage: Option<Storage>,
}

impl PartialEq for UserVolumeControlProps {
  fn eq(&self, other: &Self) -> bool {
    self.user_id == other.user_id
      && same_optional_session(self.session.as_ref(), other.session.as_ref())
      && self.storage.is_some() == other.storage.is_some()
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
}

#[derive(Clone)]
struct UserNormalizationToggleProps {
  user_id: UserId,
  session: Option<ServerSession>,
  storage: Option<Storage>,
}

impl PartialEq for UserNormalizationToggleProps {
  fn eq(&self, other: &Self) -> bool {
    self.user_id == other.user_id
      && same_optional_session(self.session.as_ref(), other.session.as_ref())
      && self.storage.is_some() == other.storage.is_some()
  }
}

fn same_session(left: &ServerSession, right: &ServerSession) -> bool {
  left.info().map(|info| info.address) == right.info().map(|info| info.address)
}

fn same_optional_session(left: Option<&ServerSession>, right: Option<&ServerSession>) -> bool {
  match (left, right) {
    (Some(left), Some(right)) => same_session(left, right),
    (None, None) => true,
    _ => false,
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
    let server_id = props
      .session
      .as_ref()
      .and_then(|session| session.info().map(|info| info.address));
    let initial = load_user_normalization(
      props.storage.as_ref(),
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
    let server_id = props
      .session
      .as_ref()
      .and_then(|session| session.info().map(|info| info.address));

    if self.user_id.get_untracked() != props.user_id || self.server_id.get_untracked() != server_id {
      let enabled = load_user_normalization(
        props.storage.as_ref(),
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
      props.storage,
      server_id,
      props.user_id,
    )
  }
}

impl Component for UserVolumeControl {
  type Props = UserVolumeControlProps;

  fn create(ctx: &mut Ctx) -> Self {
    let props = ctx.props::<Self::Props>().clone();
    let initial_server_id = props
      .session
      .as_ref()
      .and_then(|session| session.info().map(|info| info.address));
    let initial = load_user_volume(
      props.storage.as_ref(),
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
    {
      let apply_session = apply_session.clone();
      let user_id = user_id.clone();
      let last_applied_volume = last_applied_volume.clone();
      ctx.watch(&value, move |volume| {
        let volume = (*volume).clamp(0, 100);
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
    }

    Self {
      user_id,
      server_id,
      value,
      apply_session,
      last_applied_volume,
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let props = ctx.props::<Self::Props>().clone();
    let server_id = props
      .session
      .as_ref()
      .and_then(|session| session.info().map(|info| info.address));

    *self.apply_session.lock().expect("user volume session lock poisoned") = props.session.clone();

    if self.user_id.get_untracked() != props.user_id || self.server_id.get_untracked() != server_id {
      let value = load_user_volume(
        props.storage.as_ref(),
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

    let save_storage = props.storage.clone();
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
