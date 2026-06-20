use lurq::{
  app::{component::Component, ctx::Ctx, theme::Breakpoint},
  components::{Column, Row, Text},
  core::Signal,
  layout::{Alignment, layout_kind::Justify},
  node::{BackgroundColor, CursorIcon, Element, Style, border::Border, color::Color, dimension::Dimension},
};

use crate::{
  network::protocol::{Role, protocol_version_label},
  storage::StoredServer,
  theme,
  ui::{
    common::lucide_icon::{LucideIcon, LucideIconProps},
    loader::loader,
  },
};

#[derive(Clone, Copy)]
struct ServerCardLayoutMetrics {
  avatar_size: f32,
  row_spacing: f32,
  row_padding_x: f32,
  row_padding_y: f32,
  status_spacing: f32,
}

fn server_card_layout_metrics(ctx: &Ctx) -> ServerCardLayoutMetrics {
  match ctx.breakpoint() {
    Some(Breakpoint::Md) => ServerCardLayoutMetrics {
      avatar_size: 36.0,
      row_spacing: 12.0,
      row_padding_x: 16.0,
      row_padding_y: 14.0,
      status_spacing: 8.0,
    },
    Some(Breakpoint::Lg) => ServerCardLayoutMetrics {
      avatar_size: 40.0,
      row_spacing: 14.0,
      row_padding_x: 18.0,
      row_padding_y: 14.0,
      status_spacing: 10.0,
    },
    Some(Breakpoint::Xl) | Some(Breakpoint::Sm) | None => ServerCardLayoutMetrics {
      avatar_size: 40.0,
      row_spacing: 14.0,
      row_padding_x: 20.0,
      row_padding_y: 14.0,
      status_spacing: 10.0,
    },
  }
}

#[derive(Clone, Copy, PartialEq, Eq, lurq::DevtoolsInspectable)]
pub enum ServerCardState {
  Idle,
  Connecting,
  Error,
}

#[derive(Clone, Copy, PartialEq, Eq, lurq::DevtoolsInspectable)]
pub enum ServerCardLiveState {
  Unknown,
  Checking,
  Online,
  NoResponse,
}

#[derive(Clone, PartialEq, Eq, lurq::DevtoolsInspectable)]
pub struct ServerCardLiveInfo {
  pub state: ServerCardLiveState,
  pub server_name: Option<String>,
  pub current_users: Option<u16>,
  pub max_users: Option<u16>,
  pub protocol_version: Option<u16>,
  pub password_locked: bool,
}

impl Default for ServerCardLiveInfo {
  fn default() -> Self {
    Self {
      state: ServerCardLiveState::Unknown,
      server_name: None,
      current_users: None,
      max_users: None,
      protocol_version: None,
      password_locked: false,
    }
  }
}

#[derive(Clone, lurq::DevtoolsInspectable)]
pub struct ServerCardProps {
  pub server: StoredServer,
  pub state: ServerCardState,
  pub live: ServerCardLiveInfo,
  pub error_message: Option<String>,
  pub connecting: Signal<Option<String>>,
  pub failed: Signal<Option<String>>,
}

impl PartialEq for ServerCardProps {
  fn eq(&self, other: &Self) -> bool {
    self.server == other.server
      && self.state == other.state
      && self.live == other.live
      && self.error_message == other.error_message
      && self.connecting.id() == other.connecting.id()
      && self.failed.id() == other.failed.id()
  }
}

pub struct ServerCard;

impl Component for ServerCard {
  type Props = ServerCardProps;

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let props = ctx.props::<Self::Props>().clone();
    let mut card = Column::new()
      .width(Dimension::Pct(100.0))
      .clip()
      .rounded(theme::RadiusSize::Lg)
      .background(BackgroundColor::Palette(theme::PaletteColor::SurfacePanel))
      .child(card_body(ctx, &props));

    if props.state == ServerCardState::Error {
      card = card.child(error_bar(ctx, props.error_message.as_deref()));
    }

    card
  }
}

fn card_body(ctx: &mut Ctx, props: &ServerCardProps) -> impl Into<Element> {
  let metrics = server_card_layout_metrics(ctx);
  let name = display_server_name(props);
  let letter = server_letter(&name);
  let address = props.server.address.clone();
  let role = role_label(ctx, props.server.role);
  let connecting = props.connecting.clone();
  let failed = props.failed.clone();
  let click_address = props.server.address.clone();
  let border = Border::inside(1.0, state_border(props.state));

  let mut row = Row::new()
    .width(Dimension::Pct(100.0))
    .align_items(Alignment::Center)
    .spacing(metrics.row_spacing)
    .padding_vertical(metrics.row_padding_y)
    .padding_horizontal(metrics.row_padding_x)
    .border_top(border.clone())
    .border_right(border.clone())
    .border_left(border.clone())
    .cursor(CursorIcon::Pointer)
    .hovered_style(Style::new().background(BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised)))
    .on_click(move |_| {
      if connecting.get_untracked().as_deref() == Some(click_address.as_str()) {
        return;
      }
      failed.set(None);
      connecting.set(Some(click_address.clone()));
    });

  if props.state != ServerCardState::Error {
    row = row.border_bottom(border).rounded(theme::RadiusSize::Lg);
  } else {
    row = row
      .corner_radius_top_left(theme::RadiusSize::Lg)
      .corner_radius_top_right(theme::RadiusSize::Lg);
  }

  row
    .child(avatar(&letter, props.server.role, metrics.avatar_size))
    .child(
      Column::new()
        .flex(1.0)
        .spacing(theme::SpacingSize::Xs)
        .child(
          Row::new()
            .align_items(Alignment::Center)
            .spacing(theme::SpacingSize::Sm)
            .child(
              Text::new(&name)
                .variant(theme::TypographyStyle::Heading)
                .color(theme::PaletteColor::TextPrimary),
            )
            .child(role_chip(&role, props.server.role)),
        )
        .child(
          Text::new(&address)
            .variant(theme::TypographyStyle::Mono)
            .color(theme::PaletteColor::TextMuted)
            .width(Dimension::Pct(100.0)),
        )
        .child(live_meta_row(ctx, props)),
    )
    .child(card_status(ctx, props, metrics))
}

fn state_border(state: ServerCardState) -> theme::PaletteColor {
  match state {
    ServerCardState::Connecting => theme::PaletteColor::Accent,
    ServerCardState::Error => theme::PaletteColor::Danger,
    ServerCardState::Idle => theme::PaletteColor::Border,
  }
}

fn card_status(ctx: &mut Ctx, props: &ServerCardProps, metrics: ServerCardLayoutMetrics) -> Element {
  match props.state {
    ServerCardState::Connecting => Row::new()
      .align_items(Alignment::Center)
      .spacing(theme::SpacingSize::Sm)
      .child(
        Text::new(&ctx.t("servers.row.connecting"))
          .variant(theme::TypographyStyle::Description)
          .color(theme::PaletteColor::Accent),
      )
      .child(loader(16.0))
      .into(),
    ServerCardState::Error => retry_button(ctx, props).into(),
    ServerCardState::Idle => Row::new()
      .align_items(Alignment::Center)
      .spacing(metrics.status_spacing)
      .child(live_state_chip(ctx, &props.live))
      .child(trusted_chip(ctx, !props.server.certificate_fingerprint.is_empty()))
      .child(ctx.mount::<LucideIcon>(LucideIconProps {
        icon: "chevron-right",
        size: 18.0,
        color: theme::palette().text_muted,
      }))
      .into(),
  }
}

fn live_meta_row(ctx: &mut Ctx, props: &ServerCardProps) -> impl Into<Element> {
  let mut row = Row::new()
    .width(Dimension::Pct(100.0))
    .align_items(Alignment::Center)
    .spacing(theme::SpacingSize::Sm);

  for item in live_meta_items(ctx, &props.live) {
    row = row.child(meta_chip(ctx, item.icon, &item.label));
  }

  row
}

struct LiveMetaItem {
  icon: &'static str,
  label: String,
}

fn live_meta_items(ctx: &Ctx, live: &ServerCardLiveInfo) -> Vec<LiveMetaItem> {
  match live.state {
    ServerCardLiveState::Online => {
      let mut items = Vec::new();
      if let (Some(current), Some(max)) = (live.current_users, live.max_users) {
        let label = if max == 0 {
          ctx
            .t_args("servers.row.meta.users_online", [("count", current.to_string())])
            .to_string()
        } else {
          ctx
            .t_args(
              "servers.row.meta.users_online_max",
              [("count", current.to_string()), ("max", max.to_string())],
            )
            .to_string()
        };
        items.push(LiveMetaItem { icon: "users", label });
      }
      if let Some(protocol_version) = live.protocol_version {
        items.push(LiveMetaItem {
          icon: "radio",
          label: ctx
            .t_args(
              "servers.row.meta.protocol",
              [("version", protocol_version_label(protocol_version))],
            )
            .to_string(),
        });
      }
      items.push(LiveMetaItem {
        icon: if live.password_locked { "lock" } else { "unlock" },
        label: if live.password_locked {
          ctx.t("servers.row.meta.password_required").to_string()
        } else {
          ctx.t("servers.row.meta.open_server").to_string()
        },
      });
      items
    }
    ServerCardLiveState::Checking => vec![LiveMetaItem {
      icon: "radar",
      label: ctx.t("servers.row.meta.checking_info").to_string(),
    }],
    ServerCardLiveState::NoResponse => vec![LiveMetaItem {
      icon: "wifi-off",
      label: ctx.t("servers.row.meta.no_response").to_string(),
    }],
    ServerCardLiveState::Unknown => Vec::new(),
  }
}

fn meta_chip(ctx: &mut Ctx, icon: &'static str, label: &str) -> impl Into<Element> {
  Row::new()
    .height(22.0)
    .align_items(Alignment::Center)
    .spacing(theme::SpacingSize::Xs)
    .padding_horizontal(theme::SpacingSize::Sm)
    .rounded(theme::RadiusSize::Sm)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised))
    .border_inside(1.0, theme::PaletteColor::Border)
    .child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon,
      size: 12.0,
      color: theme::palette().text_muted,
    }))
    .child(
      Text::new(label)
        .variant(theme::TypographyStyle::Caption)
        .color(theme::PaletteColor::TextSecondary),
    )
}

fn live_state_chip(ctx: &mut Ctx, live: &ServerCardLiveInfo) -> impl Into<Element> {
  let (icon, label, color) = match live.state {
    ServerCardLiveState::Online => (
      "activity",
      ctx.t("servers.row.live_state.online"),
      theme::PaletteColor::Accent,
    ),
    ServerCardLiveState::Checking => (
      "radar",
      ctx.t("servers.row.live_state.checking"),
      theme::PaletteColor::TextSecondary,
    ),
    ServerCardLiveState::NoResponse => (
      "wifi-off",
      ctx.t("servers.row.live_state.no_query"),
      theme::PaletteColor::TextMuted,
    ),
    ServerCardLiveState::Unknown => (
      "circle",
      ctx.t("servers.row.live_state.unknown"),
      theme::PaletteColor::TextMuted,
    ),
  };

  Row::new()
    .height(22.0)
    .align_items(Alignment::Center)
    .spacing(theme::SpacingSize::Xs)
    .padding_horizontal(theme::SpacingSize::Sm)
    .rounded(theme::RadiusSize::Md)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised))
    .border_inside(1.0, theme::PaletteColor::Border)
    .child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon,
      size: 12.0,
      color: theme::palette().text_muted,
    }))
    .child(Text::new(&label).variant(theme::TypographyStyle::Caption).color(color))
}

fn retry_button(ctx: &mut Ctx, props: &ServerCardProps) -> impl Into<Element> {
  let connecting = props.connecting.clone();
  let failed = props.failed.clone();
  let address = props.server.address.clone();

  secondary_button(ctx, "rotate-cw", &ctx.t("servers.action.retry")).on_click(move |_| {
    if connecting.get_untracked().as_deref() == Some(address.as_str()) {
      return;
    }
    failed.set(None);
    connecting.set(Some(address.clone()));
  })
}

fn error_bar(ctx: &mut Ctx, message: Option<&str>) -> impl Into<Element> {
  let fallback = ctx.t("servers.row.error_fallback").to_string();
  let message = message
    .map(str::trim)
    .filter(|message| !message.is_empty())
    .unwrap_or(&fallback)
    .to_owned();

  Row::new()
    .width(Dimension::Pct(100.0))
    .align_items(Alignment::Center)
    .spacing(theme::SpacingSize::Sm)
    .padding_vertical(theme::SpacingSize::Md)
    .padding_horizontal(theme::SpacingSize::Xl)
    .background(BackgroundColor::Palette(theme::PaletteColor::DangerMuted))
    .border_top(Border::inside(1.0, theme::PaletteColor::Danger))
    .border_right(Border::inside(1.0, theme::PaletteColor::Danger))
    .border_bottom(Border::inside(1.0, theme::PaletteColor::Danger))
    .border_left(Border::inside(1.0, theme::PaletteColor::Danger))
    .corner_radius_bottom_right(theme::RadiusSize::Lg)
    .corner_radius_bottom_left(theme::RadiusSize::Lg)
    .child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon: "triangle-alert",
      size: 14.0,
      color: theme::palette().danger,
    }))
    .child(
      Text::new(&ctx.t_args("servers.row.error", [("error", message)]))
        .variant(theme::TypographyStyle::Description)
        .color(theme::PaletteColor::Danger),
    )
}

fn avatar(letter: &str, role: Role, size: f32) -> impl Into<Element> {
  let (background, text_color) = if matches!(role, Role::Admin | Role::Owner) {
    (
      BackgroundColor::Palette(theme::PaletteColor::Accent),
      theme::PaletteColor::TextInverse,
    )
  } else {
    (
      BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised),
      theme::PaletteColor::TextSecondary,
    )
  };

  Row::new()
    .width(size)
    .height(size)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .rounded(theme::RadiusSize::Lg)
    .background(background)
    .child(
      Text::new(letter)
        .variant(theme::TypographyStyle::Heading)
        .color(text_color),
    )
}

fn role_chip(label: &str, role: Role) -> impl Into<Element> {
  let admin = matches!(role, Role::Admin | Role::Owner);
  let border = if admin {
    BackgroundColor::Color(Color::from_hex("#6EA8D866"))
  } else {
    BackgroundColor::Palette(theme::PaletteColor::BorderStrong)
  };

  Row::new()
    .height(18.0)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .padding_horizontal(theme::SpacingSize::Sm)
    .rounded(theme::RadiusSize::Sm)
    .background(if admin {
      BackgroundColor::Palette(theme::PaletteColor::AccentMuted)
    } else {
      BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised)
    })
    .border_inside(1.0, border)
    .child(
      Text::new(label)
        .variant(theme::TypographyStyle::Caption)
        .color(if admin {
          theme::PaletteColor::Accent
        } else {
          theme::PaletteColor::TextSecondary
        }),
    )
}

fn trusted_chip(ctx: &mut Ctx, trusted: bool) -> impl Into<Element> {
  let label = ctx.t(if trusted {
    "servers.row.trust.trusted"
  } else {
    "servers.row.trust.new"
  });

  Row::new()
    .height(22.0)
    .align_items(Alignment::Center)
    .spacing(theme::SpacingSize::Xs)
    .padding_horizontal(theme::SpacingSize::Sm)
    .rounded(theme::RadiusSize::Md)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised))
    .border_inside(1.0, theme::PaletteColor::Border)
    .child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon: "shield",
      size: 12.0,
      color: theme::palette().text_muted,
    }))
    .child(
      Text::new(&label)
        .variant(theme::TypographyStyle::Caption)
        .color(theme::PaletteColor::TextSecondary),
    )
}

fn secondary_button(ctx: &mut Ctx, icon: &'static str, label: &str) -> Row {
  let hover_background = BackgroundColor::Palette(theme::PaletteColor::SurfaceInput);

  Row::new()
    .height(34.0)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .spacing(theme::SpacingSize::Sm)
    .padding_horizontal(theme::SpacingSize::Lg)
    .rounded(theme::RadiusSize::Md)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised))
    .border_inside(1.0, theme::PaletteColor::BorderStrong)
    .cursor(CursorIcon::Pointer)
    .hovered_style(Style::new().background(hover_background.clone()))
    .active_style(Style::new().background(hover_background))
    .child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon,
      size: 16.0,
      color: theme::palette().text_secondary,
    }))
    .child(
      Text::new(label)
        .variant(theme::TypographyStyle::Button)
        .color(theme::PaletteColor::TextPrimary),
    )
}

fn display_server_name(props: &ServerCardProps) -> String {
  if let Some(name) = props
    .live
    .server_name
    .as_ref()
    .map(|name| name.trim())
    .filter(|name| !name.is_empty())
  {
    name.to_owned()
  } else if props.server.server_name.trim().is_empty() {
    props.server.address.clone()
  } else {
    props.server.server_name.clone()
  }
}

fn server_letter(name: &str) -> String {
  name
    .chars()
    .find(|ch| ch.is_alphanumeric())
    .map(|ch| ch.to_uppercase().to_string())
    .unwrap_or_else(|| "?".to_owned())
}

fn role_label(ctx: &Ctx, role: Role) -> std::sync::Arc<str> {
  match role {
    Role::Owner | Role::Admin => ctx.t("servers.row.role.admin"),
    Role::Moderator => ctx.t("servers.row.role.moderator"),
    Role::User => ctx.t("servers.row.role.member"),
  }
}

#[cfg(test)]
#[path = "../../../tests/unit/ui/servers/server_card.rs"]
mod tests;
