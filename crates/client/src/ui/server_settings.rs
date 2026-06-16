use std::{collections::BTreeMap, sync::Arc, time::Duration};

use lurq::{
  app::{
    component::{Component, ComponentInfo, DevtoolsFormatter, DevtoolsInspectable},
    ctx::{Ctx, Modal, Root},
    events::{KeyboardEvent, MouseButton, MouseEvent},
    theme::Breakpoint,
  },
  components::{Column, Row, ScrollVertical, Stack, Text, TextInput},
  core::Signal,
  layout::{
    Alignment,
    layout_kind::Justify,
    scrollbar::{ScrollBarPlacement, ScrollBarStyle},
    text_style::{FontWeight, TextStyle},
  },
  node::{BackgroundColor, CursorIcon, Element, Style, border::Border, color::Color, dimension::Dimension},
};

use crate::{
  network::{
    protocol::{
      ChannelId, PERMISSION_MATRIX_PERMISSIONS, PERMISSION_MATRIX_ROLES, PROTOCOL_VERSION, Permission, Role, UserId,
      protocol_version_label,
    },
    server_query::{ServerQueryInfo, query_server},
  },
  routes::{
    ROUTE_CHOOSE_SERVER, ROUTE_LOBBY, ROUTE_SERVER_SETTINGS, ROUTE_SERVER_SETTINGS_CHANNELS,
    ROUTE_SERVER_SETTINGS_MEMBERS, ROUTE_SERVER_SETTINGS_ROLES,
  },
  services::hotkeys,
  session::{ConnectedServerInfo, LobbyChannel, LobbyState, LobbyTextChannel, LobbyUser, ServerSession},
  theme,
  ui::{
    app_chrome::{CHROME_HEIGHT, content_height, modal_y},
    common::{
      confirm_modal::{ConfirmAction, ConfirmModal, ConfirmModalProps},
      lucide_icon::{LucideIcon, LucideIconProps},
    },
    connect_server::{resolve_address, with_default_port},
    loader::loader,
  },
};

type ServerInfoQueryAction = lurq::app::ctx::FutureAction<String, Option<ServerQueryInfo>, String>;
type ServerAdminAction = lurq::app::ctx::FutureAction<ServerAdminRequest, (), String>;
type ChannelInputBlurAction = Arc<dyn Fn() + Send + Sync>;

const SERVER_SETTINGS_QUERY_TIMEOUT: Duration = Duration::from_millis(800);
const MEMBER_ROLES: [Role; 4] = [Role::Owner, Role::Admin, Role::Moderator, Role::User];

struct ServerSettingsMetrics {
  nav_width: f32,
  nav_padding_x: f32,
  main_padding: f32,
  card_padding: f32,
  stat_gap: f32,
}

fn server_settings_metrics(ctx: &Ctx) -> ServerSettingsMetrics {
  match ctx.breakpoint() {
    Some(Breakpoint::Md) => ServerSettingsMetrics {
      nav_width: 236.0,
      nav_padding_x: 12.0,
      main_padding: 28.0,
      card_padding: 16.0,
      stat_gap: 10.0,
    },
    Some(Breakpoint::Lg) => ServerSettingsMetrics {
      nav_width: 280.0,
      nav_padding_x: 14.0,
      main_padding: 34.0,
      card_padding: 18.0,
      stat_gap: 12.0,
    },
    Some(Breakpoint::Xl) | Some(Breakpoint::Sm) | None => ServerSettingsMetrics {
      nav_width: 320.0,
      nav_padding_x: 16.0,
      main_padding: 40.0,
      card_padding: 18.0,
      stat_gap: 14.0,
    },
  }
}

pub struct ServerSettingsScreen {
  voice_name: Signal<String>,
  voice_max_users: Signal<String>,
  text_name: Signal<String>,
  delete_open: Signal<bool>,
  pending_delete: Signal<Option<ChannelDeleteTarget>>,
  role_picker_user_id: Signal<Option<UserId>>,
  role_picker_open: Signal<bool>,
  role_picker_anchor: Signal<Option<(f32, f32)>>,
}

#[derive(Clone, Copy, PartialEq, Eq, lurq::DevtoolsInspectable)]
pub enum ServerSettingsPage {
  Server,
  Channels,
  Members,
  Roles,
}

impl Component for ServerSettingsScreen {
  type Props = ServerSettingsPage;

  fn create(ctx: &mut Ctx) -> Self {
    Self {
      voice_name: ctx.signal("New voice".to_owned()),
      voice_max_users: ctx.signal("8".to_owned()),
      text_name: ctx.signal("new-channel".to_owned()),
      delete_open: ctx.signal(false),
      pending_delete: ctx.signal(None),
      role_picker_user_id: ctx.signal(None),
      role_picker_open: ctx.signal(false),
      role_picker_anchor: ctx.signal(None),
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let page = *ctx.props::<Self::Props>();
    let Some(session) = ctx.use_context::<ServerSession>() else {
      return unavailable_screen(ctx);
    };
    let _revision = session.revision().get();
    let Some(info) = session.info() else {
      if let Some(navigator) = ctx.navigator() {
        navigator.replace(ROUTE_CHOOSE_SERVER);
      }
      return unavailable_screen(ctx);
    };
    if !info.role.can_edit_server_settings() {
      if let Some(navigator) = ctx.navigator() {
        navigator.replace(ROUTE_LOBBY);
      }
      return redirecting_screen();
    }
    if page == ServerSettingsPage::Channels && !info.role.can_manage_channels() {
      if let Some(navigator) = ctx.navigator() {
        navigator.replace(ROUTE_SERVER_SETTINGS);
      }
      return redirecting_screen();
    }
    let server_query = server_info_query_action(ctx);
    let admin_action = server_admin_action(ctx, session.clone());
    let query_state = server_query.state().get();
    let query_pending = query_state.is_pending();
    let admin_state = admin_action.state().get();

    if query_state.data.is_none() && !query_pending {
      server_query.run(info.address.clone());
    }

    let server_query_info = query_state.data.flatten();
    let lobby = session.lobby();
    server_settings_screen(
      ctx,
      page,
      &info,
      &lobby,
      server_query_info.as_ref(),
      &admin_action,
      admin_state.error.as_deref(),
      admin_state.is_pending(),
      &ChannelSettingsState {
        voice_name: self.voice_name.clone(),
        voice_max_users: self.voice_max_users.clone(),
        text_name: self.text_name.clone(),
        delete_open: self.delete_open.clone(),
        pending_delete: self.pending_delete.clone(),
        role_picker_user_id: self.role_picker_user_id.clone(),
        role_picker_open: self.role_picker_open.clone(),
        role_picker_anchor: self.role_picker_anchor.clone(),
      },
    )
  }
}

#[derive(Clone, PartialEq, Eq, lurq::DevtoolsInspectable)]
enum ServerAdminRequest {
  CreateVoice {
    name: String,
    max_users: u32,
  },
  RenameVoice {
    channel_id: ChannelId,
    name: String,
  },
  DeleteVoice {
    channel_id: ChannelId,
  },
  CreateText {
    name: String,
  },
  DeleteText {
    channel_id: ChannelId,
  },
  SetRole {
    user_id: UserId,
    role: Role,
  },
  SetUserVoiceState {
    user_id: UserId,
    muted: bool,
    deafened: bool,
  },
  DisconnectUser {
    user_id: UserId,
  },
  KickUser {
    user_id: UserId,
  },
}

#[derive(Clone, PartialEq, Eq, lurq::DevtoolsInspectable)]
enum ChannelDeleteTarget {
  Voice { id: ChannelId, name: String },
  Text { id: ChannelId, name: String },
}

#[derive(Clone)]
struct ChannelSettingsState {
  voice_name: Signal<String>,
  voice_max_users: Signal<String>,
  text_name: Signal<String>,
  delete_open: Signal<bool>,
  pending_delete: Signal<Option<ChannelDeleteTarget>>,
  role_picker_user_id: Signal<Option<UserId>>,
  role_picker_open: Signal<bool>,
  role_picker_anchor: Signal<Option<(f32, f32)>>,
}

fn server_admin_action(ctx: &mut Ctx, session: ServerSession) -> ServerAdminAction {
  let no_connected_server = ctx.t("server_settings.error.no_connected_server").to_string();
  let channel_name_empty = ctx.t("server_settings.error.channel_name_empty").to_string();
  ctx.future_action(move |request: ServerAdminRequest| {
    let session = session.clone();
    let no_connected_server = no_connected_server.clone();
    let channel_name_empty = channel_name_empty.clone();
    async move {
      let server = session.server().ok_or(no_connected_server)?;
      match request {
        ServerAdminRequest::CreateVoice { name, max_users } => {
          let name = validated_channel_name(name, &channel_name_empty)?;
          server
            .create_channel(name, max_users)
            .await
            .map_err(|error| error.to_string())?;
        }
        ServerAdminRequest::RenameVoice { channel_id, name } => {
          let name = validated_channel_name(name, &channel_name_empty)?;
          server
            .rename_channel(channel_id, name)
            .await
            .map_err(|error| error.to_string())?;
        }
        ServerAdminRequest::DeleteVoice { channel_id } => {
          server
            .delete_channel(channel_id)
            .await
            .map_err(|error| error.to_string())?;
        }
        ServerAdminRequest::CreateText { name } => {
          let name = validated_channel_name(name, &channel_name_empty)?;
          server
            .create_text_channel(name)
            .await
            .map_err(|error| error.to_string())?;
        }
        ServerAdminRequest::DeleteText { channel_id } => {
          server
            .delete_text_channel(channel_id)
            .await
            .map_err(|error| error.to_string())?;
        }
        ServerAdminRequest::SetRole { user_id, role } => {
          server
            .set_role(user_id, role)
            .await
            .map_err(|error| error.to_string())?;
        }
        ServerAdminRequest::SetUserVoiceState {
          user_id,
          muted,
          deafened,
        } => {
          server
            .set_user_voice_state(user_id, muted, deafened)
            .await
            .map_err(|error| error.to_string())?;
        }
        ServerAdminRequest::DisconnectUser { user_id } => {
          server
            .disconnect_user_from_voice(user_id)
            .await
            .map_err(|error| error.to_string())?;
        }
        ServerAdminRequest::KickUser { user_id } => {
          server.kick_user(user_id).await.map_err(|error| error.to_string())?;
        }
      }
      Ok(())
    }
  })
}

fn validated_channel_name(name: String, empty_error: &str) -> Result<String, String> {
  let name = name.trim().to_owned();
  if name.is_empty() {
    Err(empty_error.to_owned())
  } else {
    Ok(name)
  }
}

fn server_info_query_action(ctx: &mut Ctx) -> ServerInfoQueryAction {
  let resolve_failed = ctx.t("connect_server.error.resolve_failed").to_string();
  ctx.future_action(move |address: String| {
    let resolve_failed = resolve_failed.clone();
    async move {
      let socket = resolve_address(with_default_port(&address), resolve_failed).await?;
      query_server(socket, SERVER_SETTINGS_QUERY_TIMEOUT)
        .await
        .map_err(|error| error.to_string())
    }
  })
}

fn unavailable_screen(ctx: &mut Ctx) -> Element {
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

fn redirecting_screen() -> Element {
  Column::new()
    .width(Dimension::Pct(100.0))
    .height(Dimension::Pct(100.0))
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceBase))
    .into()
}

fn server_settings_screen(
  ctx: &mut Ctx,
  page: ServerSettingsPage,
  info: &ConnectedServerInfo,
  lobby: &LobbyState,
  server_query: Option<&ServerQueryInfo>,
  admin_action: &ServerAdminAction,
  admin_error: Option<&str>,
  admin_pending: bool,
  channel_state: &ChannelSettingsState,
) -> Element {
  let navigator = ctx.navigator();

  Row::new()
    .width(Dimension::Pct(100.0))
    .height(Dimension::Pct(100.0))
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceBase))
    .clip()
    .child(server_settings_nav(ctx, page, info))
    .child(server_settings_main(
      ctx,
      page,
      info,
      lobby,
      server_query,
      admin_action,
      admin_error,
      admin_pending,
      channel_state,
    ))
    .on_key_down(move |event: KeyboardEvent| {
      if hotkeys::is_cancel_key(&event)
        && let Some(navigator) = navigator.as_ref()
      {
        navigator.replace(ROUTE_LOBBY);
      }
    })
    .into()
}

fn server_settings_nav(ctx: &mut Ctx, page: ServerSettingsPage, info: &ConnectedServerInfo) -> Element {
  let metrics = server_settings_metrics(ctx);
  let nav_section_label = ctx.t("server_settings.nav.section").to_string();
  let server_label = ctx.t("server_settings.nav.server").to_string();
  let channels_label = ctx.t("server_settings.nav.channels").to_string();
  let members_label = ctx.t("server_settings.nav.members").to_string();
  let roles_label = ctx.t("server_settings.nav.roles").to_string();

  let mut nav = Column::new()
    .width(metrics.nav_width)
    .height(Dimension::Pct(100.0))
    .spacing(theme::SpacingSize::Sm)
    .padding_vertical(20.0)
    .padding_horizontal(metrics.nav_padding_x)
    .background(BackgroundColor::Color(Color::from_hex("#0E0F11")))
    .border_right(Border::inside(1.0, theme::PaletteColor::Border))
    .child(back_to_lobby(ctx, info))
    .child(nav_section(&nav_section_label))
    .child(nav_item(
      ctx,
      "sliders-horizontal",
      &server_label,
      page == ServerSettingsPage::Server,
      ROUTE_SERVER_SETTINGS,
    ));

  if info.role.can_manage_channels() {
    nav = nav.child(nav_item(
      ctx,
      "hash",
      &channels_label,
      page == ServerSettingsPage::Channels,
      ROUTE_SERVER_SETTINGS_CHANNELS,
    ));
  }

  nav
    .child(nav_item(
      ctx,
      "users",
      &members_label,
      page == ServerSettingsPage::Members,
      ROUTE_SERVER_SETTINGS_MEMBERS,
    ))
    .child(nav_item(
      ctx,
      "shield",
      &roles_label,
      page == ServerSettingsPage::Roles,
      ROUTE_SERVER_SETTINGS_ROLES,
    ))
    .child(Column::new().width(Dimension::Pct(100.0)).flex(1.0))
    .child(protocol_footer(ctx))
    .into()
}

fn back_to_lobby(ctx: &mut Ctx, info: &ConnectedServerInfo) -> Element {
  let navigator = ctx.navigator();
  let label = ctx.t_args(
    "server_settings.nav.back",
    [("server", display_server_name(info).to_owned())],
  );

  Row::new()
    .width(Dimension::Pct(100.0))
    .align_items(Alignment::Center)
    .spacing(theme::SpacingSize::Md)
    .padding(8.0)
    .rounded(theme::RadiusSize::Lg)
    .cursor(CursorIcon::Pointer)
    .hovered_style(Style::new().background(BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised)))
    .child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon: "arrow-left",
      size: 16.0,
      color: theme::palette().text_secondary,
    }))
    .child(
      Text::new(&label)
        .variant(theme::TypographyStyle::Button)
        .color(theme::PaletteColor::TextSecondary)
        .width(Dimension::Pct(100.0))
        .flex(1.0),
    )
    .on_click(move |_| {
      if let Some(navigator) = navigator.as_ref() {
        navigator.replace(ROUTE_LOBBY);
      }
    })
    .into()
}

fn nav_section(label: &str) -> Element {
  Row::new()
    .width(Dimension::Pct(100.0))
    .padding_top(10.0)
    .padding_bottom(2.0)
    .padding_horizontal(10.0)
    .child(
      Text::new(label)
        .variant(theme::TypographyStyle::Caption)
        .color(theme::PaletteColor::TextMuted),
    )
    .into()
}

fn nav_item(ctx: &mut Ctx, icon: &'static str, label: &str, active: bool, route: &'static str) -> Element {
  let navigator = ctx.navigator();
  let background = if active {
    BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised)
  } else {
    BackgroundColor::Color(Color::from_hex("#00000000"))
  };
  let color = if active {
    theme::palette().text_primary
  } else {
    theme::palette().text_secondary
  };

  Row::new()
    .width(Dimension::Pct(100.0))
    .align_items(Alignment::Center)
    .spacing(theme::SpacingSize::Md)
    .padding_vertical(9.0)
    .padding_horizontal(10.0)
    .rounded(theme::RadiusSize::Lg)
    .cursor(CursorIcon::Pointer)
    .background(background)
    .hovered_style(Style::new().background(BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised)))
    .child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon,
      size: 16.0,
      color,
    }))
    .child(
      Text::new(label)
        .variant(theme::TypographyStyle::Description)
        .color(if active {
          theme::PaletteColor::TextPrimary
        } else {
          theme::PaletteColor::TextSecondary
        })
        .width(Dimension::Pct(100.0))
        .flex(1.0),
    )
    .on_click(move |_| {
      if let Some(navigator) = navigator.as_ref() {
        navigator.push(route);
      }
    })
    .into()
}

fn protocol_footer(ctx: &mut Ctx) -> Element {
  Row::new()
    .width(Dimension::Pct(100.0))
    .align_items(Alignment::Center)
    .spacing(theme::SpacingSize::Sm)
    .padding_vertical(9.0)
    .padding_horizontal(10.0)
    .rounded(theme::RadiusSize::Lg)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceInput))
    .border_inside(1.0, theme::PaletteColor::Border)
    .child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon: "shield-check",
      size: 14.0,
      color: theme::palette().success,
    }))
    .child(
      Text::new(&ctx.t("server_settings.protocol_supported"))
        .variant(theme::TypographyStyle::Caption)
        .color(theme::PaletteColor::TextSecondary),
    )
    .into()
}

fn server_settings_main(
  ctx: &mut Ctx,
  page: ServerSettingsPage,
  info: &ConnectedServerInfo,
  lobby: &LobbyState,
  server_query: Option<&ServerQueryInfo>,
  admin_action: &ServerAdminAction,
  admin_error: Option<&str>,
  admin_pending: bool,
  channel_state: &ChannelSettingsState,
) -> Element {
  let metrics = server_settings_metrics(ctx);
  let content = match page {
    ServerSettingsPage::Server => server_page(ctx, info, lobby, server_query, &metrics),
    ServerSettingsPage::Channels => channels_page(
      ctx,
      lobby,
      &metrics,
      admin_action,
      admin_error,
      admin_pending,
      channel_state,
    ),
    ServerSettingsPage::Members => members_page(
      ctx,
      info,
      lobby,
      &metrics,
      admin_action,
      admin_error,
      admin_pending,
      &channel_state.role_picker_user_id,
      &channel_state.role_picker_open,
      &channel_state.role_picker_anchor,
    ),
    ServerSettingsPage::Roles => roles_page(ctx, &metrics),
  };

  ScrollVertical::new(
    Column::new()
      .width(Dimension::Pct(100.0))
      .align_items(Alignment::Center)
      .padding(metrics.main_padding)
      .child(content),
  )
  .width(Dimension::Pct(100.0))
  .height(Dimension::Pct(100.0))
  .flex(1.0)
  .scrollbar(server_settings_scrollbar_style())
  .scrollbar_hovered(|mut style| {
    let palette = theme::palette();
    style.thumb_color = palette.accent_hover;
    style.track_color = palette.surface_input.with_opacity(0.75);
    style
  })
  .into()
}

fn server_page(
  ctx: &mut Ctx,
  info: &ConnectedServerInfo,
  lobby: &LobbyState,
  server_query: Option<&ServerQueryInfo>,
  metrics: &ServerSettingsMetrics,
) -> Element {
  page_stack()
    .child(channel_page_header(
      ctx,
      &ctx.t("server_settings.title"),
      &ctx.t("server_settings.subtitle"),
    ))
    .child(server_info_card(ctx, info, metrics.card_padding))
    .child(glance_card(
      ctx,
      lobby,
      server_query,
      metrics.card_padding,
      metrics.stat_gap,
    ))
    .into()
}

fn channels_page(
  ctx: &mut Ctx,
  lobby: &LobbyState,
  metrics: &ServerSettingsMetrics,
  admin_action: &ServerAdminAction,
  admin_error: Option<&str>,
  admin_pending: bool,
  channel_state: &ChannelSettingsState,
) -> Element {
  let mut page = page_stack().child(channel_page_header(
    ctx,
    &ctx.t("server_settings.sections.channels"),
    &ctx.t("server_settings.channels.description"),
  ));

  if let Some(error) = admin_error {
    page = page.child(admin_error_banner(ctx, error));
  }
  if let Some(modal) = delete_channel_modal(ctx, channel_state, admin_action) {
    page = page.child(modal);
  }

  page
    .child(voice_channels_card(
      ctx,
      lobby,
      metrics.card_padding,
      admin_action,
      admin_pending,
      channel_state,
    ))
    .child(text_channels_card(
      ctx,
      lobby,
      metrics.card_padding,
      admin_action,
      admin_pending,
      channel_state,
    ))
    .into()
}

fn members_page(
  ctx: &mut Ctx,
  info: &ConnectedServerInfo,
  lobby: &LobbyState,
  metrics: &ServerSettingsMetrics,
  admin_action: &ServerAdminAction,
  admin_error: Option<&str>,
  admin_pending: bool,
  role_picker_user_id: &Signal<Option<UserId>>,
  role_picker_open: &Signal<bool>,
  role_picker_anchor: &Signal<Option<(f32, f32)>>,
) -> Element {
  let mut page = page_stack().child(channel_page_header(
    ctx,
    &ctx.t("server_settings.sections.members"),
    &ctx.t("server_settings.members.description"),
  ));

  if let Some(error) = admin_error {
    page = page.child(admin_error_banner(ctx, error));
  }

  page
    .child(members_card(
      ctx,
      info,
      lobby,
      metrics.card_padding,
      admin_action,
      admin_pending,
      role_picker_user_id,
      role_picker_open,
      role_picker_anchor,
    ))
    .into()
}

fn roles_page(ctx: &mut Ctx, metrics: &ServerSettingsMetrics) -> Element {
  page_stack()
    .child(channel_page_header(
      ctx,
      &ctx.t("server_settings.sections.roles"),
      &ctx.t("server_settings.roles.description"),
    ))
    .child(permissions_matrix_card(ctx, metrics.card_padding))
    .into()
}

fn page_stack() -> Column {
  Column::new().width(Dimension::Pct(100.0)).spacing(24.0)
}

fn channel_page_header(_ctx: &mut Ctx, title: &str, description: &str) -> Element {
  Column::new()
    .width(Dimension::Pct(100.0))
    .spacing(4.0)
    .child(Text::styled(title, channel_page_title_style()))
    .child(Text::styled(description, channel_page_description_style()).width(Dimension::Pct(100.0)))
    .into()
}

fn server_info_card(ctx: &mut Ctx, info: &ConnectedServerInfo, padding: f32) -> Element {
  let title = ctx.t("server_settings.info.title").to_string();
  let server_name_label = ctx.t("server_settings.info.server_name").to_string();
  let role_label = ctx.t("server_settings.info.role").to_string();
  let role_value = ctx.t(role_label_key(info.role)).to_string();
  let protocol_label = ctx.t("server_settings.info.protocol").to_string();
  let protocol_value = format!("v{}", protocol_version_label(PROTOCOL_VERSION));

  settings_card(ctx, "server", &title, padding)
    .child(divider())
    .child(readonly_row(ctx, &server_name_label, display_server_name(info)))
    .child(readonly_row(ctx, &role_label, &role_value))
    .child(readonly_row(ctx, &protocol_label, &protocol_value))
    .into()
}

fn glance_card(
  ctx: &mut Ctx,
  lobby: &LobbyState,
  server_query: Option<&ServerQueryInfo>,
  padding: f32,
  gap: f32,
) -> Element {
  let title = ctx.t("server_settings.glance.title").to_string();
  let voice_label = ctx.t("server_settings.glance.voice_channels").to_string();
  let text_label = ctx.t("server_settings.glance.text_channels").to_string();
  let users_label = ctx.t("server_settings.glance.users").to_string();
  let voice_count = lobby.channels.len().to_string();
  let text_count = lobby.text_channels.len().to_string();
  let total_users = total_users_value(server_query);

  settings_card(ctx, "activity", &title, padding)
    .child(divider())
    .child(
      Row::new()
        .width(Dimension::Pct(100.0))
        .spacing(gap)
        .child(stat_card(ctx, "volume-2", &voice_label, &voice_count))
        .child(stat_card(ctx, "hash", &text_label, &text_count))
        .child(stat_card(ctx, "users", &users_label, &total_users)),
    )
    .into()
}

fn total_users_value(server_query: Option<&ServerQueryInfo>) -> String {
  if let Some(total_users) = server_query.and_then(|query| query.total_users) {
    total_users.to_string()
  } else {
    "...".to_owned()
  }
}

fn voice_channels_card(
  ctx: &mut Ctx,
  lobby: &LobbyState,
  padding: f32,
  admin_action: &ServerAdminAction,
  admin_pending: bool,
  channel_state: &ChannelSettingsState,
) -> Element {
  let voice_title = ctx.t("server_settings.channels.voice_title").to_string();
  let create_name = channel_state.voice_name.clone();
  let create_max_users = channel_state.voice_max_users.clone();
  let can_create =
    !create_name.get().trim().is_empty() && parse_max_users(&create_max_users.get()).is_ok() && !admin_pending;
  let create_row = voice_create_row(
    ctx,
    channel_state.voice_name.clone(),
    channel_state.voice_max_users.clone(),
    admin_action,
    can_create,
  );
  let delete_open = channel_state.delete_open.clone();
  let pending_delete = channel_state.pending_delete.clone();
  let rows = if lobby.channels.is_empty() {
    vec![empty_row(&ctx.t("server_settings.channels.empty_voice"))]
  } else {
    let action = admin_action.clone();
    let pending = admin_pending;
    ctx.for_each(
      lobby.channels.clone(),
      |channel| channel.id,
      move |ctx, channel| {
        ctx.mount::<VoiceChannelSettingsRow>(VoiceChannelSettingsRowProps {
          channel,
          action: action.clone(),
          disabled: pending,
          delete_open: delete_open.clone(),
          pending_delete: pending_delete.clone(),
        })
      },
    )
  };

  channel_management_card(
    ctx,
    "volume-2",
    &voice_title,
    &ctx.t("server_settings.channels.voice_protocol"),
    lobby.channels.len(),
    padding,
  )
  .child(create_row)
  .child(divider())
  .with_children(rows)
  .into()
}

fn text_channels_card(
  ctx: &mut Ctx,
  lobby: &LobbyState,
  padding: f32,
  admin_action: &ServerAdminAction,
  admin_pending: bool,
  channel_state: &ChannelSettingsState,
) -> Element {
  let text_title = ctx.t("server_settings.channels.text_title").to_string();
  let can_create = !channel_state.text_name.get().trim().is_empty() && !admin_pending;
  let create_row = text_create_row(ctx, channel_state.text_name.clone(), admin_action, can_create);
  let rows = if lobby.text_channels.is_empty() {
    vec![empty_row(&ctx.t("server_settings.channels.empty_text"))]
  } else {
    lobby
      .text_channels
      .iter()
      .map(|channel| {
        text_channel_settings_row(
          ctx,
          channel,
          admin_pending,
          channel_state.delete_open.clone(),
          channel_state.pending_delete.clone(),
        )
      })
      .collect()
  };

  channel_management_card(
    ctx,
    "hash",
    &text_title,
    &ctx.t("server_settings.channels.text_protocol"),
    lobby.text_channels.len(),
    padding,
  )
  .child(create_row)
  .child(divider())
  .with_children(rows)
  .into()
}

fn channel_management_card(
  ctx: &mut Ctx,
  icon: &'static str,
  title: &str,
  caption: &str,
  count: usize,
  padding: f32,
) -> Column {
  settings_card_shell(padding).child(
    Row::new()
      .width(Dimension::Pct(100.0))
      .align_items(Alignment::Center)
      .spacing(theme::SpacingSize::Md)
      .child(icon_box(ctx, icon))
      .child(
        Column::new()
          .width(Dimension::Pct(100.0))
          .flex(1.0)
          .spacing(2.0)
          .child(Text::styled(title, channel_card_title_style()))
          .child(Text::styled(caption, channel_card_caption_style()).width(Dimension::Pct(100.0))),
      )
      .child(count_badge(&count.to_string())),
  )
}

fn voice_create_row(
  ctx: &mut Ctx,
  name: Signal<String>,
  max_users: Signal<String>,
  admin_action: &ServerAdminAction,
  enabled: bool,
) -> Element {
  let click_name = name.clone();
  let click_max_users = max_users.clone();
  let click_action = admin_action.clone();
  let key_name = name.clone();
  let key_max_users = max_users.clone();
  let key_action = admin_action.clone();

  create_block(ctx, &ctx.t("server_settings.channels.new_voice"))
    .child(
      Row::new()
        .width(Dimension::Pct(100.0))
        .align_items(Alignment::Center)
        .spacing(8.0)
        .child(
          channel_input(
            ctx,
            name,
            &ctx.t("server_settings.channels.voice_placeholder"),
            "server-settings-new-voice",
            1,
            None,
          )
          .flex(1.0),
        )
        .child(max_users_input(
          ctx,
          max_users,
          &ctx.t("server_settings.channels.max_users_placeholder"),
          "server-settings-new-voice-max-users",
          2,
        ))
        .child(
          channel_button(
            ctx,
            "plus",
            &ctx.t("server_settings.channels.create"),
            if enabled {
              ChannelButtonTone::Primary
            } else {
              ChannelButtonTone::Disabled
            },
          )
          .on_click(move |_| {
            if enabled {
              create_voice_channel(&click_name, &click_max_users, &click_action);
            }
          }),
        ),
    )
    .on_key_down(move |event: KeyboardEvent| {
      if enabled && event.key == "Enter" {
        create_voice_channel(&key_name, &key_max_users, &key_action);
      }
    })
    .into()
}

fn text_create_row(ctx: &mut Ctx, name: Signal<String>, admin_action: &ServerAdminAction, enabled: bool) -> Element {
  let click_name = name.clone();
  let click_action = admin_action.clone();
  let key_name = name.clone();
  let key_action = admin_action.clone();

  create_block(ctx, &ctx.t("server_settings.channels.new_text"))
    .child(
      Row::new()
        .width(Dimension::Pct(100.0))
        .align_items(Alignment::Center)
        .spacing(8.0)
        .child(
          channel_input(
            ctx,
            name,
            &ctx.t("server_settings.channels.text_placeholder"),
            "server-settings-new-text",
            3,
            None,
          )
          .flex(1.0),
        )
        .child(
          channel_button(
            ctx,
            "plus",
            &ctx.t("server_settings.channels.create"),
            if enabled {
              ChannelButtonTone::Primary
            } else {
              ChannelButtonTone::Disabled
            },
          )
          .on_click(move |_| {
            if enabled {
              create_text_channel(&click_name, &click_action);
            }
          }),
        ),
    )
    .on_key_down(move |event: KeyboardEvent| {
      if enabled && event.key == "Enter" {
        create_text_channel(&key_name, &key_action);
      }
    })
    .into()
}

fn create_block(_ctx: &mut Ctx, label: &str) -> Column {
  Column::new()
    .width(Dimension::Pct(100.0))
    .spacing(7.0)
    .child(Text::styled(label, channel_create_label_style()))
}

fn create_voice_channel(name: &Signal<String>, max_users: &Signal<String>, admin_action: &ServerAdminAction) {
  let Ok(max_users) = parse_max_users(&max_users.get_untracked()) else {
    return;
  };
  admin_action.run(ServerAdminRequest::CreateVoice {
    name: name.get_untracked(),
    max_users,
  });
}

fn create_text_channel(name: &Signal<String>, admin_action: &ServerAdminAction) {
  admin_action.run(ServerAdminRequest::CreateText {
    name: name.get_untracked(),
  });
}

fn parse_max_users(value: &str) -> Result<u32, String> {
  let value = value.trim();
  if value.is_empty() {
    return Ok(0);
  }
  value
    .parse::<u32>()
    .map_err(|_| "Max users must be a whole number.".to_owned())
}

fn text_channel_settings_row(
  ctx: &mut Ctx,
  channel: &LobbyTextChannel,
  disabled: bool,
  delete_open: Signal<bool>,
  pending_delete: Signal<Option<ChannelDeleteTarget>>,
) -> Element {
  let target = ChannelDeleteTarget::Text {
    id: channel.id,
    name: channel.name.clone(),
  };
  let delete_button = channel_button(
    ctx,
    "trash-2",
    &ctx.t("server_settings.channels.delete"),
    if disabled {
      ChannelButtonTone::Disabled
    } else {
      ChannelButtonTone::Danger
    },
  )
  .height(32.0)
  .padding_horizontal(10.0)
  .on_click(move |_| {
    if !disabled {
      pending_delete.set(Some(target.clone()));
      delete_open.set(true);
    }
  });

  Row::new()
    .width(Dimension::Pct(100.0))
    .height(48.0)
    .align_items(Alignment::Center)
    .spacing(theme::SpacingSize::Md)
    .padding_horizontal(12.0)
    .rounded(theme::RadiusSize::Lg)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceInput))
    .border_inside(1.0, theme::PaletteColor::Border)
    .child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon: text_channel_icon(&channel.name),
      size: 16.0,
      color: theme::palette().text_muted,
    }))
    .child(
      Column::new()
        .width(Dimension::Pct(100.0))
        .flex(1.0)
        .spacing(2.0)
        .child(
          Text::new(&channel.name)
            .variant(theme::TypographyStyle::Button)
            .color(theme::PaletteColor::TextPrimary),
        )
        .child(
          Text::new(&ctx.t_args("server_settings.channels.id_meta", [("id", channel.id.to_string())]))
            .variant(theme::TypographyStyle::Caption)
            .color(theme::PaletteColor::TextMuted),
        ),
    )
    .child(delete_button)
    .into()
}

fn text_channel_icon(name: &str) -> &'static str {
  if name.contains("announcement") || name.contains("announce") {
    "megaphone"
  } else {
    "hash"
  }
}

#[derive(Clone)]
struct VoiceChannelSettingsRowProps {
  channel: LobbyChannel,
  action: ServerAdminAction,
  disabled: bool,
  delete_open: Signal<bool>,
  pending_delete: Signal<Option<ChannelDeleteTarget>>,
}

impl PartialEq for VoiceChannelSettingsRowProps {
  fn eq(&self, other: &Self) -> bool {
    self.channel == other.channel && self.disabled == other.disabled
  }
}

impl DevtoolsInspectable for VoiceChannelSettingsRowProps {
  fn inspect(&self, formatter: &mut DevtoolsFormatter<'_>) {
    formatter.buffer_mut().push(ComponentInfo::with_value(
      "channel_id",
      std::any::type_name::<ChannelId>(),
      self.channel.id.to_string(),
    ));
    formatter.buffer_mut().push(ComponentInfo::with_value(
      "name",
      std::any::type_name::<String>(),
      self.channel.name.clone(),
    ));
  }
}

struct VoiceChannelSettingsRow {
  name: Signal<String>,
}

impl Component for VoiceChannelSettingsRow {
  type Props = VoiceChannelSettingsRowProps;

  fn create(ctx: &mut Ctx) -> Self {
    Self {
      name: ctx.signal(ctx.props::<Self::Props>().channel.name.clone()),
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let props = ctx.props::<Self::Props>().clone();
    let channel = props.channel;
    let key_name = self.name.clone();
    let key_action = props.action.clone();
    let channel_id = channel.id;
    let original_name = channel.name.clone();
    let disabled = props.disabled;
    let blur_name = self.name.clone();
    let blur_action = props.action.clone();
    let blur_original_name = original_name.clone();
    let save_on_blur: ChannelInputBlurAction = Arc::new(move || {
      rename_voice_channel_if_changed(channel_id, &blur_original_name, &blur_name, disabled, &blur_action);
    });
    let target = ChannelDeleteTarget::Voice {
      id: channel.id,
      name: channel.name.clone(),
    };
    let delete_open = props.delete_open.clone();
    let pending_delete = props.pending_delete.clone();

    let row = Row::new()
      .width(Dimension::Pct(100.0))
      .height(48.0)
      .align_items(Alignment::Center)
      .spacing(theme::SpacingSize::Md)
      .padding_horizontal(12.0)
      .rounded(theme::RadiusSize::Lg)
      .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceInput))
      .border_inside(1.0, theme::PaletteColor::Border)
      .child(ctx.mount::<LucideIcon>(LucideIconProps {
        icon: voice_channel_icon(&channel.name),
        size: 16.0,
        color: theme::palette().text_muted,
      }))
      .child(
        channel_input(
          ctx,
          self.name.clone(),
          &ctx.t("server_settings.channels.voice_placeholder"),
          "server-settings-voice-name",
          4,
          Some(save_on_blur),
        )
        .flex(1.0),
      )
      .child(channel_meta_badge(&voice_channel_meta(ctx, &channel)));

    let element: Element = row
      .child(
        channel_button(
          ctx,
          "trash-2",
          &ctx.t("server_settings.channels.delete"),
          if props.disabled {
            ChannelButtonTone::Disabled
          } else {
            ChannelButtonTone::Danger
          },
        )
        .height(32.0)
        .padding_horizontal(10.0)
        .on_click(move |_| {
          if !props.disabled {
            pending_delete.set(Some(target.clone()));
            delete_open.set(true);
          }
        }),
      )
      .on_key_down(move |event: KeyboardEvent| {
        if event.key == "Enter" {
          rename_voice_channel_if_changed(channel_id, &original_name, &key_name, disabled, &key_action);
        }
      })
      .into();
    element
  }
}

fn rename_voice_channel(channel_id: ChannelId, name: &Signal<String>, admin_action: &ServerAdminAction) {
  admin_action.run(ServerAdminRequest::RenameVoice {
    channel_id,
    name: name.get_untracked(),
  });
}

fn rename_voice_channel_if_changed(
  channel_id: ChannelId,
  original_name: &str,
  name: &Signal<String>,
  disabled: bool,
  admin_action: &ServerAdminAction,
) {
  let next_name = name.get_untracked();
  if disabled || next_name.trim().is_empty() || next_name.trim() == original_name.trim() {
    return;
  }
  rename_voice_channel(channel_id, name, admin_action);
}

fn voice_channel_icon(name: &str) -> &'static str {
  if name.to_ascii_lowercase().contains("stage") {
    "radio"
  } else {
    "volume-2"
  }
}

fn voice_channel_meta(ctx: &mut Ctx, channel: &LobbyChannel) -> String {
  if channel.max_users == 0 {
    ctx
      .t_args(
        "server_settings.channels.voice_meta_unlimited",
        [("id", channel.id.to_string())],
      )
      .to_string()
  } else {
    ctx
      .t_args(
        "server_settings.channels.voice_meta_max",
        [("id", channel.id.to_string()), ("count", channel.max_users.to_string())],
      )
      .to_string()
  }
}

fn delete_channel_modal(
  ctx: &mut Ctx,
  channel_state: &ChannelSettingsState,
  admin_action: &ServerAdminAction,
) -> Option<Element> {
  let Some(target) = channel_state.pending_delete.get() else {
    return None;
  };
  let (name, title_key, body_key, request) = match target {
    ChannelDeleteTarget::Voice { id, name } => (
      name,
      "server_settings.channels.confirm_delete_voice.title",
      "server_settings.channels.confirm_delete_voice.body",
      ServerAdminRequest::DeleteVoice { channel_id: id },
    ),
    ChannelDeleteTarget::Text { id, name } => (
      name,
      "server_settings.channels.confirm_delete_text.title",
      "server_settings.channels.confirm_delete_text.body",
      ServerAdminRequest::DeleteText { channel_id: id },
    ),
  };
  let action = admin_action.clone();
  let on_confirm: ConfirmAction = Arc::new(move || {
    action.run(request.clone());
  });
  let props = ConfirmModalProps {
    open: channel_state.delete_open.clone(),
    icon: "trash-2",
    title: ctx.t_args(title_key, [("channel", name.clone())]),
    body: ctx.t_args(body_key, [("channel", name)]),
    warning: Some(ctx.t("server_settings.channels.confirm_delete_warning")),
    cancel_label: ctx.t("common.action.cancel"),
    confirm_label: ctx.t("server_settings.channels.delete"),
    on_confirm,
  };
  Some(
    Modal::new(ctx.mount::<ConfirmModal>(props))
      .open(channel_state.delete_open.clone())
      .target(Root)
      .into(),
  )
}

fn channel_input(
  ctx: &mut Ctx,
  value: Signal<String>,
  placeholder: &str,
  name: &'static str,
  tab_index: i32,
  on_blur: Option<ChannelInputBlurAction>,
) -> Row {
  let mut text_style = ctx.theme().typography().description.clone();
  text_style.weight = FontWeight::Bold;
  text_style.font_size = 13.0;
  text_style.line_height = 1.2;
  text_style.color = theme::palette().text_primary;
  let mut placeholder_style = text_style.clone();
  placeholder_style.color = theme::palette().text_muted.with_opacity(0.62);

  let mut input = TextInput::styled(value, text_style)
    .placeholder(placeholder)
    .placeholder_style(placeholder_style)
    .single_line()
    .name(name)
    .tab_index(tab_index)
    .height(Dimension::Pct(100.0))
    .flex(1.0)
    .padding_left(11.0)
    .padding_right(11.0)
    .background(BackgroundColor::Color(Color::from_hex("#00000000")))
    .caret_color(theme::PaletteColor::Accent);

  if let Some(on_blur) = on_blur {
    input = input.on_blur(move || on_blur());
  }

  Row::new()
    .width(Dimension::Pct(100.0))
    .height(36.0)
    .align_items(Alignment::Center)
    .rounded(theme::RadiusSize::Md)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceBase))
    .border_inside(1.0, theme::PaletteColor::BorderStrong)
    .child(input)
}

fn max_users_input(ctx: &mut Ctx, value: Signal<String>, placeholder: &str, name: &'static str, tab_index: i32) -> Row {
  channel_input(ctx, value, placeholder, name, tab_index, None).width(80.0)
}

fn icon_box(ctx: &mut Ctx, icon: &'static str) -> Element {
  Row::new()
    .width(32.0)
    .height(32.0)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .rounded(8.0)
    .background(BackgroundColor::Palette(theme::PaletteColor::AccentMuted))
    .border_inside(1.0, theme::PaletteColor::Accent)
    .child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon,
      size: 17.0,
      color: theme::palette().accent,
    }))
    .into()
}

fn count_badge(label: &str) -> Element {
  count_badge_row(label).into()
}

fn channel_meta_badge(label: &str) -> Element {
  count_badge_row(label).width(154.0).into()
}

fn count_badge_row(label: &str) -> Row {
  Row::new()
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .padding_vertical(6.0)
    .padding_horizontal(8.0)
    .rounded(theme::RadiusSize::Md)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfacePanel))
    .border_inside(1.0, theme::PaletteColor::Border)
    .child(Text::styled(
      label,
      TextStyle {
        color: theme::palette().text_muted,
        font_size: 11.0,
        weight: FontWeight::Bold,
        ..TextStyle::default()
      },
    ))
}

#[derive(Clone, Copy)]
enum ChannelButtonTone {
  Primary,
  Danger,
  Disabled,
}

fn channel_button(ctx: &mut Ctx, icon: &'static str, label: &str, tone: ChannelButtonTone) -> Row {
  let palette = theme::palette();
  let (background, border, text_color, icon_color, hover, enabled) = match tone {
    ChannelButtonTone::Primary => (
      BackgroundColor::Palette(theme::PaletteColor::Accent),
      BackgroundColor::Palette(theme::PaletteColor::Accent),
      palette.text_inverse,
      palette.text_inverse,
      BackgroundColor::Palette(theme::PaletteColor::AccentHover),
      true,
    ),
    ChannelButtonTone::Danger => (
      BackgroundColor::Palette(theme::PaletteColor::DangerMuted),
      BackgroundColor::Palette(theme::PaletteColor::Danger),
      palette.danger,
      palette.danger,
      BackgroundColor::Color(palette.danger_muted.with_opacity(0.82)),
      true,
    ),
    ChannelButtonTone::Disabled => (
      BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised),
      BackgroundColor::Palette(theme::PaletteColor::Border),
      palette.text_muted,
      palette.text_muted,
      BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised),
      false,
    ),
  };
  let mut button = Row::new()
    .height(36.0)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .spacing(7.0)
    .padding_horizontal(14.0)
    .rounded(theme::RadiusSize::Md)
    .background(background)
    .border_inside(1.0, border)
    .child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon,
      size: 15.0,
      color: icon_color,
    }))
    .child(Text::styled(
      label,
      TextStyle {
        font_family: Arc::from("Inter"),
        font_size: 12.0,
        line_height: 1.2,
        weight: FontWeight::Bold,
        color: text_color,
        ..TextStyle::default()
      },
    ));

  if enabled {
    button = button
      .cursor(CursorIcon::Pointer)
      .hovered_style(Style::new().background(hover.clone()))
      .active_style(Style::new().background(hover));
  }

  button
}

fn admin_error_banner(ctx: &mut Ctx, error: &str) -> Element {
  Row::new()
    .width(Dimension::Pct(100.0))
    .align_items(Alignment::Center)
    .spacing(theme::SpacingSize::Md)
    .padding_vertical(12.0)
    .padding_horizontal(14.0)
    .rounded(theme::RadiusSize::Lg)
    .background(BackgroundColor::Palette(theme::PaletteColor::DangerMuted))
    .border_inside(1.0, theme::PaletteColor::Danger)
    .child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon: "triangle-alert",
      size: 16.0,
      color: theme::palette().danger,
    }))
    .child(
      Text::new(&ctx.t_args("server_settings.channels.error", [("message", error.to_owned())]))
        .variant(theme::TypographyStyle::Description)
        .color(theme::PaletteColor::Danger)
        .width(Dimension::Pct(100.0))
        .flex(1.0),
    )
    .into()
}

fn members_card(
  ctx: &mut Ctx,
  info: &ConnectedServerInfo,
  lobby: &LobbyState,
  padding: f32,
  admin_action: &ServerAdminAction,
  admin_pending: bool,
  role_picker_user_id: &Signal<Option<UserId>>,
  role_picker_open: &Signal<bool>,
  role_picker_anchor: &Signal<Option<(f32, f32)>>,
) -> Element {
  let title = ctx.t("server_settings.sections.members").to_string();
  let caption = ctx.t("server_settings.members.protocol_caption").to_string();
  let mut card = settings_card_shell(padding)
    .child(
      Row::new()
        .width(Dimension::Pct(100.0))
        .align_items(Alignment::Center)
        .spacing(10.0)
        .child(
          Column::new()
            .width(Dimension::Pct(100.0))
            .flex(1.0)
            .spacing(2.0)
            .child(Text::new(&title).variant(theme::TypographyStyle::Heading))
            .child(
              Text::new(&caption)
                .variant(theme::TypographyStyle::FieldLabel)
                .color(theme::PaletteColor::TextMuted)
                .width(Dimension::Pct(100.0)),
            ),
        ),
    )
    .child(divider());
  let members = active_members(lobby);
  let selected_member = role_picker_user_id
    .get()
    .and_then(|user_id| members.iter().find(|member| member.user.user_id == user_id).cloned());

  if role_picker_open.get_untracked() && selected_member.is_none() {
    close_member_role_picker(
      role_picker_user_id.clone(),
      role_picker_open.clone(),
      role_picker_anchor.clone(),
    );
  }

  if let Some(member) = selected_member {
    let modal_role_picker_user_id = role_picker_user_id.clone();
    let modal_role_picker_open = role_picker_open.clone();
    let modal_role_picker_anchor = role_picker_anchor.clone();
    let modal_action = admin_action.clone();
    card = card.child(
      Modal::new(member_role_picker_overlay(
        ctx,
        member.user.user_id,
        member.user.role,
        info.role,
        &modal_action,
        modal_role_picker_user_id.clone(),
        modal_role_picker_open.clone(),
        modal_role_picker_anchor.clone(),
        admin_pending,
      ))
      .open(role_picker_open.clone())
      .target(Root),
    );
  }

  if members.is_empty() {
    card = card.child(empty_row(&ctx.t("server_settings.members.empty")));
  } else {
    let mut list = Column::new().width(Dimension::Pct(100.0)).spacing(8.0);
    for member in members {
      list = list.child(member_row(
        ctx,
        member,
        info.user_id,
        info.role,
        admin_action,
        admin_pending,
        role_picker_user_id,
        role_picker_open,
        role_picker_anchor,
      ));
    }
    card = card.child(list);
  }

  card.into()
}

#[derive(Clone)]
struct ActiveMember {
  user: LobbyUser,
  channels: Vec<String>,
}

fn active_members(lobby: &LobbyState) -> Vec<ActiveMember> {
  let mut members = BTreeMap::<UserId, ActiveMember>::new();

  for (channel_id, users) in &lobby.users_by_channel {
    let channel_name = lobby
      .channels
      .iter()
      .find(|channel| channel.id == *channel_id)
      .map(|channel| channel.name.clone())
      .unwrap_or_else(|| format!("#{channel_id}"));

    for user in users {
      members
        .entry(user.user_id)
        .and_modify(|member| member.channels.push(channel_name.clone()))
        .or_insert_with(|| ActiveMember {
          user: user.clone(),
          channels: vec![channel_name.clone()],
        });
    }
  }

  for user in &lobby.users {
    members.entry(user.user_id).or_insert_with(|| ActiveMember {
      user: user.clone(),
      channels: Vec::new(),
    });
  }

  members.into_values().collect()
}

fn member_row(
  ctx: &mut Ctx,
  member: ActiveMember,
  local_user_id: UserId,
  local_role: Role,
  admin_action: &ServerAdminAction,
  admin_pending: bool,
  role_picker_user_id: &Signal<Option<UserId>>,
  role_picker_open: &Signal<bool>,
  role_picker_anchor: &Signal<Option<(f32, f32)>>,
) -> Element {
  let role = ctx.t(role_label_key(member.user.role)).to_string();
  let channels = if member.channels.is_empty() {
    ctx.t("server_settings.members.no_voice").to_string()
  } else {
    member.channels.join(", ")
  };
  let handle = member_handle(&member.user.username);
  let subtitle = if member.channels.is_empty() {
    format!("@{handle} · {channels}")
  } else {
    format!("@{handle} · in {channels}")
  };
  let target_user_id = member.user.user_id;
  let can_moderate = target_user_id != local_user_id && local_role.can_moderate(member.user.role);
  let can_set_role = can_moderate && local_role.has_permission(Permission::ManageRoles) && !admin_pending;
  let can_mute = can_moderate && local_role.has_permission(Permission::MuteOthers) && !admin_pending;
  let can_deafen = can_moderate && local_role.has_permission(Permission::DeafenOthers) && !admin_pending;
  let can_disconnect = can_moderate && local_role.has_permission(Permission::KickFromChannel) && !admin_pending;
  let can_kick = can_moderate && local_role.has_permission(Permission::KickFromServer) && !admin_pending;
  let role_picker_active = role_picker_user_id.get() == Some(target_user_id) && role_picker_open.get();
  let scale = ctx.window().scale_factor.max(f32::EPSILON);
  let muted = member.user.muted;
  let deafened = member.user.deafened;
  let role_picker_user = role_picker_user_id.clone();
  let role_picker_visible = role_picker_open.clone();
  let role_picker_anchor = role_picker_anchor.clone();
  let mute_action = admin_action.clone();
  let deafen_action = admin_action.clone();
  let disconnect_action = admin_action.clone();
  let kick_action = admin_action.clone();

  let row = Row::new()
    .width(Dimension::Pct(100.0))
    .height(62.0)
    .align_items(Alignment::Center)
    .spacing(12.0)
    .padding_vertical(0.0)
    .padding_horizontal(14.0)
    .rounded(6.0)
    .background(BackgroundColor::Color(Color::from_hex("#171A1E")))
    .border_inside(1.0, BackgroundColor::Color(Color::from_hex("#30343A")))
    .child(member_avatar(&member.user.username, member.user.role))
    .child(
      Column::new()
        .width(Dimension::Pct(100.0))
        .flex(1.0)
        .spacing(2.0)
        .child(Text::styled(
          &member.user.username,
          TextStyle {
            font_family: Arc::from("Inter"),
            font_size: 13.0,
            line_height: 1.2,
            weight: FontWeight::Bold,
            color: theme::palette().text_primary,
            ..TextStyle::default()
          },
        ))
        .child(Text::styled(
          &subtitle,
          TextStyle {
            font_family: Arc::from("Inter"),
            font_size: 11.0,
            line_height: 1.2,
            weight: FontWeight::Medium,
            color: theme::palette().text_muted,
            ..TextStyle::default()
          },
        )),
    )
    .child(member_role_pill(
      ctx,
      &role,
      member.user.role,
      can_set_role,
      role_picker_active,
      role_picker_user,
      role_picker_visible,
      role_picker_anchor,
      target_user_id,
      scale,
    ))
    .child(
      member_action_button(ctx, "mic-off", member_control_color(muted), can_mute).on_click(move |_| {
        if can_mute {
          mute_action.run(ServerAdminRequest::SetUserVoiceState {
            user_id: target_user_id,
            muted: !muted,
            deafened,
          });
        }
      }),
    )
    .child(
      member_action_button(ctx, "volume-x", member_control_color(deafened), can_deafen).on_click(move |_| {
        if can_deafen {
          deafen_action.run(ServerAdminRequest::SetUserVoiceState {
            user_id: target_user_id,
            muted,
            deafened: !deafened,
          });
        }
      }),
    )
    .child(
      member_action_button(ctx, "phone-off", Color::from_hex("#D6B25E"), can_disconnect).on_click(move |_| {
        if can_disconnect {
          disconnect_action.run(ServerAdminRequest::DisconnectUser {
            user_id: target_user_id,
          });
        }
      }),
    )
    .child(
      member_action_button(ctx, "log-out", Color::from_hex("#FF6B5F"), can_kick).on_click(move |_| {
        if can_kick {
          kick_action.run(ServerAdminRequest::KickUser {
            user_id: target_user_id,
          });
        }
      }),
    );

  row.into()
}

fn member_avatar(username: &str, role: Role) -> Element {
  Row::new()
    .width(38.0)
    .height(38.0)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .rounded(38.0)
    .background(BackgroundColor::Color(member_avatar_color(role)))
    .child(Text::styled(
      &member_initials(username),
      TextStyle {
        font_family: Arc::from("Inter"),
        font_size: 13.0,
        line_height: 1.0,
        weight: FontWeight::Bold,
        color: theme::palette().text_primary,
        ..TextStyle::default()
      },
    ))
    .into()
}

fn member_role_pill(
  ctx: &mut Ctx,
  label: &str,
  role: Role,
  enabled: bool,
  open: bool,
  role_picker_user_id: Signal<Option<UserId>>,
  role_picker_open: Signal<bool>,
  role_picker_anchor: Signal<Option<(f32, f32)>>,
  user_id: UserId,
  scale: f32,
) -> Row {
  let mut pill = Row::new()
    .height(26.0)
    .align_items(Alignment::Center)
    .spacing(7.0)
    .padding_vertical(6.0)
    .padding_horizontal(9.0)
    .rounded(5.0)
    .background(BackgroundColor::Color(if open {
      Color::from_hex("#232830")
    } else {
      Color::from_hex("#15171A")
    }))
    .border_inside(1.0, BackgroundColor::Color(Color::from_hex("#30343A")))
    .child(
      Row::new()
        .width(7.0)
        .height(7.0)
        .rounded(7.0)
        .background(BackgroundColor::Color(member_role_color(role))),
    )
    .child(Text::styled(
      label,
      TextStyle {
        font_family: Arc::from("Inter"),
        font_size: 12.0,
        line_height: 1.2,
        weight: FontWeight::Bold,
        color: theme::palette().text_secondary,
        ..TextStyle::default()
      },
    ))
    .child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon: "chevron-down",
      size: 13.0,
      color: theme::palette().text_muted,
    }));

  if enabled {
    pill = pill
      .cursor(CursorIcon::Pointer)
      .hovered_style(Style::new().background(BackgroundColor::Color(Color::from_hex("#232830"))))
      .on_mouse_click(MouseButton::Left, move |event: MouseEvent| {
        if role_picker_user_id.get_untracked() == Some(user_id) {
          close_member_role_picker(
            role_picker_user_id.clone(),
            role_picker_open.clone(),
            role_picker_anchor.clone(),
          );
        } else {
          role_picker_anchor.set(Some((event.x / scale, event.y / scale)));
          role_picker_user_id.set(Some(user_id));
          role_picker_open.set(true);
        }
      });
  }

  pill
}

fn member_action_button(ctx: &mut Ctx, icon: &'static str, color: Color, enabled: bool) -> Row {
  let icon_color = if enabled {
    color
  } else {
    theme::palette().text_muted.with_opacity(0.5)
  };
  let mut button = Row::new()
    .width(32.0)
    .height(32.0)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .rounded(5.0)
    .background(BackgroundColor::Color(Color::from_hex("#15171A")))
    .border_inside(1.0, BackgroundColor::Color(Color::from_hex("#30343A")))
    .child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon,
      size: 15.0,
      color: icon_color,
    }));

  if enabled {
    button = button
      .cursor(CursorIcon::Pointer)
      .hovered_style(Style::new().background(BackgroundColor::Color(Color::from_hex("#232830"))));
  }

  button
}

fn close_member_role_picker(
  role_picker_user_id: Signal<Option<UserId>>,
  role_picker_open: Signal<bool>,
  role_picker_anchor: Signal<Option<(f32, f32)>>,
) {
  role_picker_user_id.set(None);
  role_picker_open.set(false);
  role_picker_anchor.set(None);
}

fn member_role_picker_overlay(
  ctx: &mut Ctx,
  target_user_id: UserId,
  current_role: Role,
  local_role: Role,
  admin_action: &ServerAdminAction,
  role_picker_user_id: Signal<Option<UserId>>,
  role_picker_open: Signal<bool>,
  role_picker_anchor: Signal<Option<(f32, f32)>>,
  admin_pending: bool,
) -> Element {
  let window = ctx.window();
  let window_width = window.logical_width();
  let modal_height = content_height(ctx);
  let picker_width = 288.0;
  let (anchor_x, anchor_y) = role_picker_anchor
    .get_untracked()
    .unwrap_or((window_width - picker_width - 32.0, CHROME_HEIGHT + 160.0));
  let menu_left = (anchor_x - picker_width + 96.0).clamp(8.0, (window_width - picker_width - 8.0).max(8.0));
  let menu_top = (modal_y(anchor_y) + 8.0).clamp(8.0, (modal_height - 8.0).max(8.0));
  let close_left_user = role_picker_user_id.clone();
  let close_left_open = role_picker_open.clone();
  let close_left_anchor = role_picker_anchor.clone();
  let close_right_user = role_picker_user_id.clone();
  let close_right_open = role_picker_open.clone();
  let close_right_anchor = role_picker_anchor.clone();
  let close_middle_user = role_picker_user_id.clone();
  let close_middle_open = role_picker_open.clone();
  let close_middle_anchor = role_picker_anchor.clone();

  Stack::new()
    .width(window_width)
    .height(modal_height)
    .absolute(0.0, CHROME_HEIGHT, window_width, modal_height)
    .child(
      Row::new()
        .width(window_width)
        .height(modal_height)
        .on_click(move |_| {
          close_member_role_picker(
            close_left_user.clone(),
            close_left_open.clone(),
            close_left_anchor.clone(),
          );
        })
        .on_mouse_click(MouseButton::Right, move |_| {
          close_member_role_picker(
            close_right_user.clone(),
            close_right_open.clone(),
            close_right_anchor.clone(),
          );
        })
        .on_mouse_click(MouseButton::Middle, move |_| {
          close_member_role_picker(
            close_middle_user.clone(),
            close_middle_open.clone(),
            close_middle_anchor.clone(),
          );
        }),
    )
    .child(
      member_role_picker_menu(
        ctx,
        target_user_id,
        current_role,
        local_role,
        admin_action,
        role_picker_user_id,
        role_picker_open,
        role_picker_anchor,
        admin_pending,
        picker_width,
      )
      .absolute_position(menu_left, menu_top),
    )
    .into()
}

fn member_role_picker_menu(
  ctx: &mut Ctx,
  target_user_id: UserId,
  current_role: Role,
  local_role: Role,
  admin_action: &ServerAdminAction,
  role_picker_user_id: Signal<Option<UserId>>,
  role_picker_open: Signal<bool>,
  role_picker_anchor: Signal<Option<(f32, f32)>>,
  admin_pending: bool,
  width: f32,
) -> Column {
  let mut picker = Column::new()
    .width(width)
    .spacing(4.0)
    .padding_vertical(8.0)
    .padding_horizontal(10.0)
    .rounded(6.0)
    .background(BackgroundColor::Color(Color::from_hex("#0F1216")))
    .border_inside(1.0, BackgroundColor::Color(Color::from_hex("#30343A")));

  for role in MEMBER_ROLES {
    picker = picker.child(member_role_option(
      ctx,
      target_user_id,
      current_role,
      local_role,
      role,
      admin_action,
      role_picker_user_id.clone(),
      role_picker_open.clone(),
      role_picker_anchor.clone(),
      admin_pending,
    ));
  }

  picker
}

fn member_role_option(
  ctx: &mut Ctx,
  target_user_id: UserId,
  current_role: Role,
  local_role: Role,
  role: Role,
  admin_action: &ServerAdminAction,
  role_picker_user_id: Signal<Option<UserId>>,
  role_picker_open: Signal<bool>,
  role_picker_anchor: Signal<Option<(f32, f32)>>,
  admin_pending: bool,
) -> Row {
  let current = role == current_role;
  let assignable =
    !admin_pending && role != Role::Owner && role != current_role && can_assign_member_role(local_role, role);
  let action = admin_action.clone();
  let label = ctx.t(role_label_key(role)).to_string();
  let meta = if current {
    "current"
  } else if assignable {
    "assign"
  } else if role == Role::Owner {
    "config only"
  } else {
    "locked"
  };
  let mut row = Row::new()
    .width(Dimension::Pct(100.0))
    .height(34.0)
    .align_items(Alignment::Center)
    .spacing(8.0)
    .padding_vertical(0.0)
    .padding_horizontal(8.0)
    .rounded(5.0)
    .background(BackgroundColor::Color(if current {
      Color::from_hex("#121A23")
    } else {
      Color::from_hex("#00000000")
    }))
    .child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon: role_option_icon(role),
      size: 14.0,
      color: if assignable || current {
        theme::palette().text_secondary
      } else {
        theme::palette().text_muted
      },
    }))
    .child(
      Column::new()
        .width(Dimension::Pct(100.0))
        .flex(1.0)
        .spacing(1.0)
        .child(Text::styled(
          &label,
          TextStyle {
            font_family: Arc::from("Inter"),
            font_size: 12.0,
            line_height: 1.2,
            weight: FontWeight::Bold,
            color: if assignable || current {
              theme::palette().text_primary
            } else {
              theme::palette().text_muted
            },
            ..TextStyle::default()
          },
        ))
        .child(Text::styled(
          meta,
          TextStyle {
            font_family: Arc::from("Inter"),
            font_size: 10.0,
            line_height: 1.2,
            weight: FontWeight::Medium,
            color: theme::palette().text_muted,
            ..TextStyle::default()
          },
        )),
    )
    .child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon: if current { "check" } else { "lock" },
      size: 14.0,
      color: if current {
        theme::palette().accent
      } else {
        theme::palette().text_muted
      },
    }));

  if assignable {
    row = row
      .cursor(CursorIcon::Pointer)
      .hovered_style(Style::new().background(BackgroundColor::Color(Color::from_hex("#232830"))))
      .on_click(move |_| {
        action.run(ServerAdminRequest::SetRole {
          user_id: target_user_id,
          role,
        });
        close_member_role_picker(
          role_picker_user_id.clone(),
          role_picker_open.clone(),
          role_picker_anchor.clone(),
        );
      });
  }

  row
}

fn can_assign_member_role(actor_role: Role, target_role: Role) -> bool {
  actor_role == Role::Owner || (target_role as u8) > actor_role as u8
}

fn role_option_icon(role: Role) -> &'static str {
  match role {
    Role::Owner | Role::Admin => "shield",
    Role::Moderator => "shield-check",
    Role::User => "user",
  }
}

fn member_control_color(active: bool) -> Color {
  if active {
    Color::from_hex("#FF6B5F")
  } else {
    theme::palette().text_secondary
  }
}

fn member_role_color(role: Role) -> Color {
  match role {
    Role::Owner => Color::from_hex("#FF6B5F"),
    Role::Admin => Color::from_hex("#6EA8D8"),
    Role::Moderator => Color::from_hex("#D6B25E"),
    Role::User => Color::from_hex("#42D28B"),
  }
}

fn member_avatar_color(role: Role) -> Color {
  match role {
    Role::Owner => Color::from_hex("#3A2A2C"),
    Role::Admin => Color::from_hex("#1E2A36"),
    Role::Moderator => Color::from_hex("#2A2418"),
    Role::User => Color::from_hex("#16301F"),
  }
}

fn member_handle(username: &str) -> String {
  let handle = username
    .chars()
    .filter(|ch| ch.is_ascii_alphanumeric() || *ch == '_' || *ch == '-')
    .flat_map(|ch| ch.to_lowercase())
    .collect::<String>();

  if handle.is_empty() { "user".to_owned() } else { handle }
}

fn member_initials(username: &str) -> String {
  let mut initials = username
    .split_whitespace()
    .filter_map(|part| part.chars().find(|ch| ch.is_alphanumeric()))
    .flat_map(|ch| ch.to_uppercase())
    .take(2)
    .collect::<String>();

  if initials.is_empty() {
    initials = username
      .chars()
      .filter(|ch| ch.is_alphanumeric())
      .flat_map(|ch| ch.to_uppercase())
      .take(2)
      .collect();
  }

  if initials.is_empty() { "?".to_owned() } else { initials }
}

fn permissions_matrix_card(ctx: &mut Ctx, _padding: f32) -> Element {
  let mut card = Column::new()
    .width(Dimension::Pct(100.0))
    .clip()
    .rounded(8.0)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfacePanel))
    .border_inside(1.0, theme::PaletteColor::Border)
    .child(permission_matrix_header_row(ctx));

  for (index, permission) in PERMISSION_MATRIX_PERMISSIONS.into_iter().enumerate() {
    card = card.child(permission_matrix_row(ctx, permission, index % 2 == 1));
  }

  card.into()
}

fn permission_matrix_header_row(ctx: &mut Ctx) -> Element {
  let mut row = Row::new()
    .width(Dimension::Pct(100.0))
    .height(42.0)
    .align_items(Alignment::Center)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised))
    .child(
      Row::new()
        .width(Dimension::Pct(100.0))
        .height(Dimension::Pct(100.0))
        .flex(1.0)
        .align_items(Alignment::Center)
        .padding_horizontal(16.0)
        .child(Text::styled(
          &ctx.t("server_settings.roles.permission_header"),
          permission_header_style(),
        )),
    );

  for role in PERMISSION_MATRIX_ROLES {
    row = row.child(permission_role_header_cell(ctx, role));
  }

  row.into()
}

fn permission_role_header_cell(ctx: &mut Ctx, role: Role) -> Element {
  Row::new()
    .width(104.0)
    .height(Dimension::Pct(100.0))
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .spacing(6.0)
    .child(role_dot(role))
    .child(Text::styled(
      &ctx.t(role_label_key(role)),
      permission_role_header_style(),
    ))
    .into()
}

fn permission_matrix_row(ctx: &mut Ctx, permission: Permission, raised: bool) -> Element {
  let background = if raised {
    BackgroundColor::Palette(theme::PaletteColor::SurfaceInput)
  } else {
    BackgroundColor::Color(Color::from_hex("#00000000"))
  };

  let mut row = Row::new()
    .width(Dimension::Pct(100.0))
    .height(34.0)
    .align_items(Alignment::Center)
    .background(background)
    .border_top(Border::inside(1.0, theme::PaletteColor::Border))
    .child(
      Row::new()
        .width(Dimension::Pct(100.0))
        .height(Dimension::Pct(100.0))
        .flex(1.0)
        .align_items(Alignment::Center)
        .padding_horizontal(16.0)
        .child(Text::styled(
          &ctx.t(permission_label_key(permission)),
          permission_name_style(),
        )),
    );

  for role in PERMISSION_MATRIX_ROLES {
    row = row.child(permission_matrix_cell(ctx, role, permission));
  }

  row.into()
}

fn permission_matrix_cell(ctx: &mut Ctx, role: Role, permission: Permission) -> Element {
  let allowed = role_has_default_permission(role, permission);
  Row::new()
    .width(104.0)
    .height(Dimension::Pct(100.0))
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon: if allowed { "check" } else { "minus" },
      size: if allowed { 15.0 } else { 14.0 },
      color: if allowed {
        theme::palette().success
      } else {
        theme::palette().text_muted
      },
    }))
    .into()
}

fn role_has_default_permission(role: Role, permission: Permission) -> bool {
  if role == Role::Owner {
    true
  } else {
    role.default_permissions() & permission as u32 != 0
  }
}

fn role_dot(role: Role) -> Element {
  Row::new()
    .width(7.0)
    .height(7.0)
    .rounded(4.0)
    .background(BackgroundColor::Color(role_matrix_color(role)))
    .into()
}

fn role_matrix_color(role: Role) -> Color {
  match role {
    Role::Owner => theme::palette().danger,
    Role::Admin => theme::palette().accent,
    Role::Moderator => theme::palette().success,
    Role::User => theme::palette().info,
  }
}

fn permission_header_style() -> TextStyle {
  TextStyle {
    font_family: Arc::from("Inter"),
    font_size: 11.0,
    line_height: 1.2,
    weight: FontWeight::Bold,
    color: theme::palette().text_muted,
    ..TextStyle::default()
  }
}

fn permission_role_header_style() -> TextStyle {
  TextStyle {
    font_family: Arc::from("Inter"),
    font_size: 12.0,
    line_height: 1.2,
    weight: FontWeight::Bold,
    color: theme::palette().text_secondary,
    ..TextStyle::default()
  }
}

fn permission_name_style() -> TextStyle {
  TextStyle {
    font_family: Arc::from("Inter"),
    font_size: 13.0,
    line_height: 1.2,
    weight: FontWeight::Medium,
    color: theme::palette().text_primary,
    ..TextStyle::default()
  }
}

fn empty_row(label: &str) -> Element {
  Row::new()
    .width(Dimension::Pct(100.0))
    .min_height(44.0)
    .align_items(Alignment::Center)
    .padding_vertical(10.0)
    .padding_horizontal(14.0)
    .rounded(theme::RadiusSize::Lg)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceInput))
    .border_inside(1.0, theme::PaletteColor::Border)
    .child(
      Text::new(label)
        .variant(theme::TypographyStyle::Description)
        .color(theme::PaletteColor::TextMuted),
    )
    .into()
}

fn settings_card(ctx: &mut Ctx, icon: &'static str, title: &str, padding: f32) -> Column {
  settings_card_shell(padding).child(
    Row::new()
      .width(Dimension::Pct(100.0))
      .align_items(Alignment::Center)
      .spacing(theme::SpacingSize::Md)
      .child(ctx.mount::<LucideIcon>(LucideIconProps {
        icon,
        size: 16.0,
        color: theme::palette().accent,
      }))
      .child(Text::new(title).variant(theme::TypographyStyle::Heading)),
  )
}

fn settings_card_shell(padding: f32) -> Column {
  Column::new()
    .width(Dimension::Pct(100.0))
    .spacing(theme::SpacingSize::Lg)
    .padding(padding)
    .rounded(8.0)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfacePanel))
    .border_inside(1.0, theme::PaletteColor::Border)
}

fn readonly_row(ctx: &mut Ctx, label: &str, value: &str) -> Element {
  Row::new()
    .width(Dimension::Pct(100.0))
    .min_height(48.0)
    .align_items(Alignment::Center)
    .spacing(theme::SpacingSize::Md)
    .padding_vertical(12.0)
    .padding_horizontal(14.0)
    .rounded(theme::RadiusSize::Lg)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceInput))
    .border_inside(1.0, theme::PaletteColor::Border)
    .child(
      Text::new(label)
        .variant(theme::TypographyStyle::Description)
        .color(theme::PaletteColor::TextSecondary)
        .width(Dimension::Pct(100.0))
        .flex(1.0),
    )
    .child(
      Text::new(value)
        .variant(theme::TypographyStyle::Description)
        .color(theme::PaletteColor::TextPrimary),
    )
    .child(readonly_badge(ctx))
    .into()
}

fn readonly_badge(ctx: &mut Ctx) -> Element {
  Row::new()
    .align_items(Alignment::Center)
    .spacing(theme::SpacingSize::Xs)
    .padding_vertical(4.0)
    .padding_horizontal(7.0)
    .rounded(theme::RadiusSize::Md)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceBase))
    .border_inside(1.0, theme::PaletteColor::Border)
    .child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon: "lock",
      size: 13.0,
      color: theme::palette().text_muted,
    }))
    .child(
      Text::new(&ctx.t("server_settings.read_only"))
        .variant(theme::TypographyStyle::Caption)
        .color(theme::PaletteColor::TextMuted),
    )
    .into()
}

fn stat_card(ctx: &mut Ctx, icon: &'static str, label: &str, value: &str) -> Element {
  Column::new()
    .width(Dimension::Pct(100.0))
    .flex(1.0)
    .spacing(theme::SpacingSize::Sm)
    .padding(16.0)
    .rounded(8.0)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceInput))
    .border_inside(1.0, theme::PaletteColor::Border)
    .child(
      Row::new()
        .width(34.0)
        .height(34.0)
        .align_items(Alignment::Center)
        .justify(Justify::Center)
        .rounded(8.0)
        .background(BackgroundColor::Palette(theme::PaletteColor::SurfacePanel))
        .border_inside(1.0, theme::PaletteColor::Border)
        .child(ctx.mount::<LucideIcon>(LucideIconProps {
          icon,
          size: 17.0,
          color: theme::palette().accent,
        })),
    )
    .child(Text::styled(value, stat_value_style()))
    .child(Text::styled(label, stat_label_style()).width(Dimension::Pct(100.0)))
    .into()
}

fn divider() -> Element {
  Row::new()
    .width(Dimension::Pct(100.0))
    .height(1.0)
    .background(BackgroundColor::Palette(theme::PaletteColor::Border))
    .into()
}

fn display_server_name(info: &ConnectedServerInfo) -> &str {
  if info.server_name.trim().is_empty() {
    "Server"
  } else {
    &info.server_name
  }
}

fn role_label_key(role: Role) -> &'static str {
  match role {
    Role::Owner => "lobby.role.owner",
    Role::Admin => "lobby.role.admin",
    Role::Moderator => "lobby.role.moderator",
    Role::User => "lobby.role.member",
  }
}

fn permission_label_key(permission: Permission) -> &'static str {
  match permission {
    Permission::None => "server_settings.roles.permission.none",
    Permission::JoinChannel => "server_settings.roles.permission.join_channel",
    Permission::Speak => "server_settings.roles.permission.speak",
    Permission::MuteOthers => "server_settings.roles.permission.mute_others",
    Permission::DeafenOthers => "server_settings.roles.permission.deafen_others",
    Permission::KickFromChannel => "server_settings.roles.permission.kick_from_channel",
    Permission::KickFromServer => "server_settings.roles.permission.kick_from_server",
    Permission::CreateChannel => "server_settings.roles.permission.create_channel",
    Permission::DeleteChannel => "server_settings.roles.permission.delete_channel",
    Permission::ManagePermissions => "server_settings.roles.permission.manage_permissions",
    Permission::ManageRoles => "server_settings.roles.permission.manage_roles",
    Permission::ManageServer => "server_settings.roles.permission.manage_server",
    Permission::SendText => "server_settings.roles.permission.send_text",
    Permission::UploadFiles => "server_settings.roles.permission.upload_files",
    Permission::ShareScreen => "server_settings.roles.permission.share_screen",
    Permission::ShareWebcam => "server_settings.roles.permission.share_webcam",
  }
}

fn stat_value_style() -> TextStyle {
  TextStyle {
    color: theme::palette().text_primary,
    font_size: 24.0,
    weight: FontWeight::Bold,
    ..TextStyle::default()
  }
}

fn stat_label_style() -> TextStyle {
  TextStyle {
    color: theme::palette().text_muted,
    font_size: 12.0,
    weight: FontWeight::Medium,
    ..TextStyle::default()
  }
}

fn channel_page_title_style() -> TextStyle {
  TextStyle {
    font_family: Arc::from("Inter"),
    font_size: 22.0,
    line_height: 1.2,
    weight: FontWeight::Bold,
    color: theme::palette().text_primary,
    ..TextStyle::default()
  }
}

fn channel_page_description_style() -> TextStyle {
  TextStyle {
    font_family: Arc::from("Inter"),
    font_size: 13.0,
    line_height: 1.4,
    weight: FontWeight::Medium,
    color: theme::palette().text_secondary,
    ..TextStyle::default()
  }
}

fn channel_card_title_style() -> TextStyle {
  TextStyle {
    font_family: Arc::from("Inter"),
    font_size: 15.0,
    line_height: 1.2,
    weight: FontWeight::Bold,
    color: theme::palette().text_primary,
    ..TextStyle::default()
  }
}

fn channel_card_caption_style() -> TextStyle {
  TextStyle {
    font_family: Arc::from("Inter"),
    font_size: 11.0,
    line_height: 1.3,
    weight: FontWeight::Medium,
    color: theme::palette().text_muted,
    ..TextStyle::default()
  }
}

fn channel_create_label_style() -> TextStyle {
  TextStyle {
    font_family: Arc::from("Inter"),
    font_size: 10.0,
    line_height: 1.2,
    weight: FontWeight::Bold,
    color: theme::palette().text_muted,
    ..TextStyle::default()
  }
}

fn server_settings_scrollbar_style() -> ScrollBarStyle {
  let palette = theme::palette();
  ScrollBarStyle {
    width: 8.0,
    min_thumb_length: 32.0,
    track_color: palette.surface_input.with_opacity(0.55),
    thumb_color: palette.accent,
    thumb_radius: 4.0,
    track_radius: 4.0,
    padding: 0.0,
    placement: ScrollBarPlacement::Reserved,
    ..ScrollBarStyle::default()
  }
}
