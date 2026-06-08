use std::{collections::BTreeMap, time::Duration};

use lurq::{
  app::{component::Component, ctx::Ctx, theme::Breakpoint},
  components::{Column, Row, ScrollVertical, Text},
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
    protocol::{PROTOCOL_VERSION, Role, UserId},
    server_query::{ServerQueryInfo, query_server},
  },
  routes::{ROUTE_CHOOSE_SERVER, ROUTE_LOBBY},
  services::hotkeys,
  session::{ConnectedServerInfo, LobbyChannel, LobbyState, LobbyTextChannel, LobbyUser, ServerSession},
  theme,
  ui::{
    common::lucide_icon::{LucideIcon, LucideIconProps},
    connect_server::{resolve_address, with_default_port},
    loader::loader,
  },
};

type ServerInfoQueryAction = lurq::app::ctx::FutureAction<String, Option<ServerQueryInfo>, String>;

const SERVER_SETTINGS_QUERY_TIMEOUT: Duration = Duration::from_millis(800);

struct ServerSettingsMetrics {
  nav_width: f32,
  nav_padding_x: f32,
  main_padding: f32,
  main_max_width: f32,
  card_padding: f32,
  stat_gap: f32,
}

fn server_settings_metrics(ctx: &Ctx) -> ServerSettingsMetrics {
  match ctx.breakpoint() {
    Some(Breakpoint::Md) => ServerSettingsMetrics {
      nav_width: 236.0,
      nav_padding_x: 12.0,
      main_padding: 28.0,
      main_max_width: 760.0,
      card_padding: 16.0,
      stat_gap: 10.0,
    },
    Some(Breakpoint::Lg) => ServerSettingsMetrics {
      nav_width: 280.0,
      nav_padding_x: 14.0,
      main_padding: 34.0,
      main_max_width: 820.0,
      card_padding: 18.0,
      stat_gap: 12.0,
    },
    Some(Breakpoint::Xl) | Some(Breakpoint::Sm) | None => ServerSettingsMetrics {
      nav_width: 320.0,
      nav_padding_x: 16.0,
      main_padding: 40.0,
      main_max_width: 860.0,
      card_padding: 18.0,
      stat_gap: 14.0,
    },
  }
}

pub struct ServerSettingsScreen;

impl Component for ServerSettingsScreen {
  type Props = ();

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
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
    let server_query = server_info_query_action(ctx);
    let query_state = server_query.state().get();
    let query_pending = query_state.is_pending();

    if query_state.data.is_none() && !query_pending {
      server_query.run(info.address.clone());
    }

    let server_query_info = query_state.data.flatten();
    let lobby = session.lobby();
    server_settings_screen(ctx, &info, &lobby, server_query_info.as_ref())
  }
}

fn server_info_query_action(ctx: &mut Ctx) -> ServerInfoQueryAction {
  ctx.future_action(|address: String| async move {
    let socket = resolve_address(with_default_port(&address)).await?;
    query_server(socket, SERVER_SETTINGS_QUERY_TIMEOUT)
      .await
      .map_err(|error| error.to_string())
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

fn server_settings_screen(
  ctx: &mut Ctx,
  info: &ConnectedServerInfo,
  lobby: &LobbyState,
  server_query: Option<&ServerQueryInfo>,
) -> Element {
  let navigator = ctx.navigator();

  Row::new()
    .width(Dimension::Pct(100.0))
    .height(Dimension::Pct(100.0))
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceBase))
    .clip()
    .child(server_settings_nav(ctx, info))
    .child(server_settings_main(ctx, info, lobby, server_query))
    .on_key_down(move |event| {
      if hotkeys::is_cancel_key(event)
        && let Some(navigator) = navigator.as_ref()
      {
        navigator.replace(ROUTE_LOBBY);
      }
    })
    .into()
}

fn server_settings_nav(ctx: &mut Ctx, info: &ConnectedServerInfo) -> Element {
  let metrics = server_settings_metrics(ctx);
  let nav_section_label = ctx.t("server_settings.nav.section").to_string();
  let server_label = ctx.t("server_settings.nav.server").to_string();
  let channels_label = ctx.t("server_settings.nav.channels").to_string();
  let members_label = ctx.t("server_settings.nav.members").to_string();
  let roles_label = ctx.t("server_settings.nav.roles").to_string();

  Column::new()
    .width(metrics.nav_width)
    .height(Dimension::Pct(100.0))
    .spacing(theme::SpacingSize::Sm)
    .padding_vertical(20.0)
    .padding_horizontal(metrics.nav_padding_x)
    .background(BackgroundColor::Color(Color::from_hex("#0E0F11")))
    .border_right(Border::inside(1.0, theme::PaletteColor::Border))
    .child(back_to_lobby(ctx, info))
    .child(nav_section(&nav_section_label))
    .child(nav_item(ctx, "sliders-horizontal", &server_label, true))
    .child(nav_item(ctx, "hash", &channels_label, false))
    .child(nav_item(ctx, "users", &members_label, false))
    .child(nav_item(ctx, "shield", &roles_label, false))
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

fn nav_item(ctx: &mut Ctx, icon: &'static str, label: &str, active: bool) -> Element {
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
    .background(background)
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
  info: &ConnectedServerInfo,
  lobby: &LobbyState,
  server_query: Option<&ServerQueryInfo>,
) -> Element {
  let metrics = server_settings_metrics(ctx);
  ScrollVertical::new(
    Column::new()
      .width(Dimension::Pct(100.0))
      .align_items(Alignment::Center)
      .padding(metrics.main_padding)
      .child(
        Column::new()
          .width(Dimension::Pct(100.0))
          .max_width(metrics.main_max_width)
          .spacing(24.0)
          .child(page_header(ctx))
          .child(server_info_card(ctx, info, metrics.card_padding))
          .child(glance_card(
            ctx,
            lobby,
            server_query,
            metrics.card_padding,
            metrics.stat_gap,
          ))
          .child(channels_card(ctx, lobby, metrics.card_padding))
          .child(members_card(ctx, lobby, metrics.card_padding))
          .child(roles_card(ctx, metrics.card_padding)),
      ),
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

fn page_header(ctx: &mut Ctx) -> Element {
  Column::new()
    .width(Dimension::Pct(100.0))
    .spacing(theme::SpacingSize::Xs)
    .child(Text::new(&ctx.t("server_settings.title")).variant(theme::TypographyStyle::Title))
    .child(
      Text::new(&ctx.t("server_settings.subtitle"))
        .variant(theme::TypographyStyle::Description)
        .color(theme::PaletteColor::TextSecondary)
        .width(Dimension::Pct(100.0)),
    )
    .into()
}

fn server_info_card(ctx: &mut Ctx, info: &ConnectedServerInfo, padding: f32) -> Element {
  let title = ctx.t("server_settings.info.title").to_string();
  let server_name_label = ctx.t("server_settings.info.server_name").to_string();
  let role_label = ctx.t("server_settings.info.role").to_string();
  let role_value = ctx.t(role_label_key(info.role)).to_string();
  let protocol_label = ctx.t("server_settings.info.protocol").to_string();
  let protocol_value = format!("v{PROTOCOL_VERSION}");

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

fn channels_card(ctx: &mut Ctx, lobby: &LobbyState, padding: f32) -> Element {
  let title = ctx.t("server_settings.sections.channels").to_string();
  let voice_title = ctx.t("server_settings.channels.voice_title").to_string();
  let text_title = ctx.t("server_settings.channels.text_title").to_string();
  let voice_rows = lobby
    .channels
    .iter()
    .map(|channel| voice_channel_settings_row(ctx, channel))
    .collect();
  let text_rows = lobby
    .text_channels
    .iter()
    .map(|channel| text_channel_settings_row(ctx, channel))
    .collect();

  settings_card(ctx, "hash", &title, padding)
    .child(divider())
    .child(channel_group(
      &voice_title,
      &ctx.t("server_settings.channels.empty_voice"),
      voice_rows,
    ))
    .child(channel_group(
      &text_title,
      &ctx.t("server_settings.channels.empty_text"),
      text_rows,
    ))
    .into()
}

fn channel_group(title: &str, empty: &str, rows: Vec<Element>) -> Element {
  let mut group = Column::new()
    .width(Dimension::Pct(100.0))
    .spacing(theme::SpacingSize::Sm)
    .child(section_label(title));

  if rows.is_empty() {
    group = group.child(empty_row(empty));
  } else {
    group = group.with_children(rows);
  }

  group.into()
}

fn voice_channel_settings_row(ctx: &mut Ctx, channel: &LobbyChannel) -> Element {
  let max_users = if channel.max_users == 0 {
    ctx.t("server_settings.channels.unlimited").to_string()
  } else {
    ctx
      .t_args(
        "server_settings.channels.max_users",
        [("count", channel.max_users.to_string())],
      )
      .to_string()
  };
  let connected = ctx
    .t_args(
      "server_settings.channels.connected",
      [("count", channel.user_count.to_string())],
    )
    .to_string();

  settings_data_row(ctx, "volume-2", &channel.name, &max_users, Some(&connected))
}

fn text_channel_settings_row(ctx: &mut Ctx, channel: &LobbyTextChannel) -> Element {
  let meta = ctx.t_args("lobby.channel_management.row.id", [("id", channel.id.to_string())]);
  settings_data_row(ctx, "hash", &channel.name, &meta, None)
}

fn members_card(ctx: &mut Ctx, lobby: &LobbyState, padding: f32) -> Element {
  let title = ctx.t("server_settings.sections.members").to_string();
  let mut card = settings_card(ctx, "users", &title, padding)
    .child(divider())
    .child(section_label(&ctx.t("server_settings.members.online_title")));
  let members = active_members(lobby);

  if members.is_empty() {
    card = card.child(empty_row(&ctx.t("server_settings.members.empty")));
  } else {
    for member in members {
      card = card.child(member_row(ctx, member));
    }
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

fn member_row(ctx: &mut Ctx, member: ActiveMember) -> Element {
  let role = ctx.t(role_label_key(member.user.role)).to_string();
  let channels = if member.channels.is_empty() {
    ctx.t("server_settings.members.no_voice").to_string()
  } else {
    member.channels.join(", ")
  };
  let meta = ctx
    .t_args(
      "server_settings.members.user_meta",
      [("role", role), ("channels", channels)],
    )
    .to_string();
  let id = ctx
    .t_args(
      "server_settings.members.user_id",
      [("id", member.user.user_id.to_string())],
    )
    .to_string();

  settings_data_row(ctx, "user", &member.user.username, &meta, Some(&id))
}

fn roles_card(ctx: &mut Ctx, padding: f32) -> Element {
  let title = ctx.t("server_settings.sections.roles").to_string();
  let mut card = settings_card(ctx, "shield", &title, padding).child(divider());

  for role in [Role::Owner, Role::Admin, Role::Moderator, Role::User] {
    card = card.child(role_row(ctx, role));
  }

  card.into()
}

fn role_row(ctx: &mut Ctx, role: Role) -> Element {
  let permissions = role.default_permissions();
  let role_label = ctx.t(role_label_key(role)).to_string();
  let permission_label = if role == Role::Owner {
    ctx.t("server_settings.roles.all_permissions").to_string()
  } else {
    ctx
      .t_args(
        "server_settings.roles.default_permissions",
        [("count", permissions.count_ones().to_string())],
      )
      .to_string()
  };
  let mask = format!("0x{permissions:08X}");

  settings_data_row(ctx, "shield-check", &role_label, &permission_label, Some(&mask))
}

fn section_label(label: &str) -> Element {
  Row::new()
    .width(Dimension::Pct(100.0))
    .padding_top(2.0)
    .padding_horizontal(2.0)
    .child(
      Text::new(label)
        .variant(theme::TypographyStyle::FieldLabel)
        .color(theme::PaletteColor::TextMuted),
    )
    .into()
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

fn settings_data_row(
  ctx: &mut Ctx,
  icon: &'static str,
  title: &str,
  subtitle: &str,
  trailing: Option<&str>,
) -> Element {
  let mut row = Row::new()
    .width(Dimension::Pct(100.0))
    .min_height(52.0)
    .align_items(Alignment::Center)
    .spacing(theme::SpacingSize::Md)
    .padding_vertical(10.0)
    .padding_horizontal(14.0)
    .rounded(theme::RadiusSize::Lg)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceInput))
    .border_inside(1.0, theme::PaletteColor::Border)
    .child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon,
      size: 16.0,
      color: theme::palette().text_muted,
    }))
    .child(
      Column::new()
        .width(Dimension::Pct(100.0))
        .flex(1.0)
        .spacing(2.0)
        .child(
          Text::new(title)
            .variant(theme::TypographyStyle::Description)
            .color(theme::PaletteColor::TextPrimary),
        )
        .child(
          Text::new(subtitle)
            .variant(theme::TypographyStyle::Caption)
            .color(theme::PaletteColor::TextMuted),
        ),
    );

  if let Some(trailing) = trailing {
    row = row.child(
      Text::new(trailing)
        .variant(theme::TypographyStyle::Mono)
        .color(theme::PaletteColor::TextMuted),
    );
  }

  row.into()
}

fn settings_card(ctx: &mut Ctx, icon: &'static str, title: &str, padding: f32) -> Column {
  Column::new()
    .width(Dimension::Pct(100.0))
    .spacing(theme::SpacingSize::Lg)
    .padding(padding)
    .rounded(8.0)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfacePanel))
    .border_inside(1.0, theme::PaletteColor::Border)
    .child(
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
