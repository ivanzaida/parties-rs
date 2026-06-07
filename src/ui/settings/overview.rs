use lurq::{
  app::{component::Component, ctx::Ctx},
  components::{Column, Row, Text},
  core::Signal,
  layout::{Alignment, layout_kind::Justify},
  node::{BackgroundColor, CursorIcon, Element, Style, dimension::Dimension},
};

use crate::{
  routes::{ROUTE_SETTINGS_IDENTITY, ROUTE_SETTINGS_SERVERS},
  storage::Storage,
  theme,
  ui::{
    common::lucide_icon::{LucideIcon, LucideIconProps},
    settings::{
      shell::{SettingsPage, card, header, page_stack, screen, setting_row},
      toggle::{SettingsToggle, SettingsToggleProps},
    },
  },
};

pub struct SettingsOverviewScreen {
  start_muted_when_joining: Signal<bool>,
  launch_parties_at_login: Signal<bool>,
}

impl Component for SettingsOverviewScreen {
  type Props = ();

  fn create(ctx: &mut Ctx) -> Self {
    let storage = ctx.use_context::<Storage>();
    let settings = storage
      .as_ref()
      .and_then(|storage| storage.load_settings().ok())
      .unwrap_or_default();
    let start_muted_when_joining = ctx.signal(settings.start_muted_when_joining);
    let launch_parties_at_login = ctx.signal(settings.launch_parties_at_login);

    if let Some(storage) = storage {
      let start_muted_signal = start_muted_when_joining.clone();
      let launch_login_signal = launch_parties_at_login.clone();
      ctx.on_effect(move || {
        let mut settings = storage.load_settings().unwrap_or_default();
        settings.start_muted_when_joining = start_muted_signal.get();
        settings.launch_parties_at_login = launch_login_signal.get();
        let _ = storage.save_settings(&settings);
      });
    }

    Self {
      start_muted_when_joining,
      launch_parties_at_login,
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let storage = ctx.use_context::<Storage>();
    let identity = storage
      .as_ref()
      .and_then(|storage| storage.load_identity().ok())
      .flatten();
    let settings = storage.as_ref().and_then(|storage| storage.load_settings().ok());
    let servers = storage
      .as_ref()
      .and_then(|storage| storage.load_servers().ok())
      .unwrap_or_default();
    let server_count = servers.len();

    let public_id = identity
      .as_ref()
      .map(|identity| format_public_id(&identity.public_key))
      .unwrap_or_else(|| ctx.t("settings.identity.missing").to_string());
    let identity_name = settings
      .map(|settings| settings.display_name.trim().to_owned())
      .filter(|name| !name.is_empty())
      .unwrap_or_else(|| ctx.t("servers.user.name").to_string());
    let identity_initials = initials_for(&identity_name, &ctx.t("servers.user.initials"));
    let identity_subtitle = ctx.t("settings.overview.identity.subtitle");
    let identity_action = ctx.t("settings.overview.identity.manage");
    let identity_field = ctx.t("settings.overview.identity.public_id");
    let identity_card = identity_card(
      ctx,
      &identity_initials,
      &identity_name,
      &identity_subtitle,
      &identity_field,
      &public_id,
      &identity_action,
    );

    let server_title = if server_count == 1 {
      ctx.t("settings.overview.servers.count_one")
    } else {
      ctx.t_args(
        "settings.overview.servers.count_many",
        [("count", server_count.to_string())],
      )
    };
    let trusted = servers.iter().all(|server| !server.certificate_fingerprint.is_empty());
    let server_subtitle = if server_count == 0 {
      ctx.t("settings.overview.servers.empty_subtitle")
    } else if trusted {
      ctx.t("settings.overview.servers.trusted")
    } else {
      ctx.t("settings.overview.servers.untrusted")
    };
    let last_server = servers
      .first()
      .map(|server| {
        if server.server_name.trim().is_empty() {
          server.address.clone()
        } else {
          server.server_name.clone()
        }
      })
      .unwrap_or_else(|| ctx.t("settings.overview.servers.none").to_string());
    let servers_action = ctx.t("settings.overview.servers.manage");
    let servers_field = ctx.t("settings.overview.servers.last_connected");
    let servers_card = servers_card(
      ctx,
      &server_title,
      &server_subtitle,
      &servers_field,
      &last_server,
      &servers_action,
    );
    let muted_row = setting_row(
      &ctx.t("settings.overview.toggle.muted.title"),
      &ctx.t("settings.overview.toggle.muted.description"),
      ctx.mount::<SettingsToggle>(SettingsToggleProps {
        enabled: self.start_muted_when_joining.clone(),
      }),
      false,
    );
    let login_row = setting_row(
      &ctx.t("settings.overview.toggle.login.title"),
      &ctx.t("settings.overview.toggle.login.description"),
      ctx.mount::<SettingsToggle>(SettingsToggleProps {
        enabled: self.launch_parties_at_login.clone(),
      }),
      false,
    );
    let content = page_stack()
      .child(header(
        &ctx.t("settings.overview.title"),
        &ctx.t("settings.overview.description"),
      ))
      .child(
        Row::new()
          .width(Dimension::Pct(100.0))
          .spacing(theme::SpacingSize::Section)
          .child(identity_card.flex(1.0))
          .child(servers_card.flex(1.0)),
      )
      .child(
        Column::new()
          .width(Dimension::Pct(100.0))
          .spacing(theme::SpacingSize::Sm)
          .child(
            Text::new(&ctx.t("settings.overview.toggles"))
              .variant(theme::TypographyStyle::Caption)
              .color(theme::PaletteColor::TextMuted),
          )
          .child(
            Column::new()
              .width(Dimension::Pct(100.0))
              .child(muted_row)
              .child(login_row),
          ),
      );

    screen(ctx, SettingsPage::Overview, content)
  }
}

fn identity_card(
  ctx: &mut Ctx,
  initials: &str,
  name: &str,
  subtitle: &str,
  field_label: &str,
  field_value: &str,
  action: &str,
) -> Column {
  overview_card(
    ctx,
    Row::new()
      .width(44.0)
      .height(44.0)
      .align_items(Alignment::Center)
      .justify(Justify::Center)
      .rounded(22.0)
      .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised))
      .child(Text::new(initials).variant(theme::TypographyStyle::Mono)),
    name,
    subtitle,
    field_label,
    field_value,
    action,
    ROUTE_SETTINGS_IDENTITY,
  )
}

fn servers_card(
  ctx: &mut Ctx,
  title: &str,
  subtitle: &str,
  field_label: &str,
  field_value: &str,
  action: &str,
) -> Column {
  let leading = Row::new()
    .width(44.0)
    .height(44.0)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .rounded(theme::RadiusSize::Lg)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised))
    .child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon: "server",
      size: 20.0,
      color: theme::palette().text_secondary,
    }));

  overview_card(
    ctx,
    leading,
    title,
    subtitle,
    field_label,
    field_value,
    action,
    ROUTE_SETTINGS_SERVERS,
  )
}

fn overview_card(
  ctx: &mut Ctx,
  leading: impl Into<Element>,
  title: &str,
  subtitle: &str,
  field_label: &str,
  field_value: &str,
  action: &str,
  route: &'static str,
) -> Column {
  card()
    .child(
      Row::new()
        .width(Dimension::Pct(100.0))
        .align_items(Alignment::Center)
        .spacing(theme::SpacingSize::Lg)
        .child(leading)
        .child(
          Column::new()
            .spacing(theme::SpacingSize::Xs)
            .child(Text::new(title).variant(theme::TypographyStyle::Heading))
            .child(
              Text::new(subtitle)
                .variant(theme::TypographyStyle::Link)
                .color(theme::PaletteColor::TextMuted),
            ),
        ),
    )
    .child(
      Column::new()
        .width(Dimension::Pct(100.0))
        .spacing(theme::SpacingSize::Xs)
        .child(
          Text::new(field_label)
            .variant(theme::TypographyStyle::Link)
            .color(theme::PaletteColor::TextMuted),
        )
        .child(
          Text::new(field_value)
            .variant(theme::TypographyStyle::Mono)
            .color(theme::PaletteColor::TextSecondary),
        ),
    )
    .child(manage_button(ctx, action, route))
}

fn manage_button(ctx: &mut Ctx, label: &str, route: &'static str) -> Row {
  let navigator = ctx.navigator();
  let mut row = Row::new()
    .width(Dimension::Pct(100.0))
    .height(34.0)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .spacing(theme::SpacingSize::Sm)
    .padding_horizontal(theme::SpacingSize::Lg)
    .rounded(theme::RadiusSize::Md)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised))
    .border_inside(1.0, theme::PaletteColor::Border)
    .cursor(CursorIcon::Pointer)
    .hovered_style(Style::new().background(BackgroundColor::Palette(theme::PaletteColor::SurfaceInput)))
    .active_style(Style::new().background(BackgroundColor::Palette(theme::PaletteColor::SurfaceInput)))
    .child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon: "check",
      size: 16.0,
      color: theme::palette().text_secondary,
    }))
    .child(Text::new(label).variant(theme::TypographyStyle::Button));

  if let Some(navigator) = navigator {
    row = row.on_click(move |_| navigator.push(route));
  }

  row
}

fn format_public_id(bytes: &[u8]) -> String {
  let mut out = String::from("pk_");
  for byte in bytes.iter().take(4) {
    out.push(hex_char(byte >> 4));
    out.push(hex_char(byte & 0x0f));
  }
  out.push_str("...");
  for byte in bytes.iter().rev().take(4).collect::<Vec<_>>().into_iter().rev() {
    out.push(hex_char(byte >> 4));
    out.push(hex_char(byte & 0x0f));
  }
  out
}

fn initials_for(name: &str, fallback: &str) -> String {
  let initials = name
    .chars()
    .filter(|ch| ch.is_alphanumeric())
    .flat_map(|ch| ch.to_uppercase())
    .take(2)
    .collect::<String>();

  if initials.is_empty() {
    fallback.to_owned()
  } else {
    initials
  }
}

fn hex_char(value: u8) -> char {
  match value {
    0..=9 => (b'0' + value) as char,
    10..=15 => (b'a' + value - 10) as char,
    _ => '?',
  }
}
