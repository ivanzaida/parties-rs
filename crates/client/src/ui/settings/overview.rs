use lurq::{
  app::{component::Component, ctx::Ctx},
  components::{Column, Row, Text},
  core::Signal,
  layout::{Alignment, layout_kind::Justify},
  node::{BackgroundColor, CursorIcon, Element, Style, dimension::Dimension},
};

use crate::{
  routes::{ROUTE_SETTINGS_IDENTITY, ROUTE_SETTINGS_SERVERS},
  services::logger,
  session::ServerSession,
  storage::Storage,
  theme,
  ui::{
    common::{
      dropdown_menu::{DropdownOption, dropdown_menu},
      lucide_icon::{LucideIcon, LucideIconProps},
    },
    settings::{
      audio::{audio_row, audio_section_label},
      shell::{SettingsPage, SettingsPopupHandle, card, header, page_stack, screen, settings_section_spacing},
      toggle::settings_toggle,
    },
  },
};

const LANGUAGE_DROPDOWN_WIDTH: f32 = 220.0;

pub struct SettingsOverviewScreen {
  start_muted_when_joining: bool,
  sentry_reports_enabled: bool,
  debug_mode_enabled: bool,
  locale: String,
}

impl Component for SettingsOverviewScreen {
  type Props = ();

  fn create(ctx: &mut Ctx) -> Self {
    let storage = ctx.use_context::<Storage>();
    let settings = storage
      .as_ref()
      .and_then(|storage| storage.load_settings().ok())
      .unwrap_or_default();
    let start_muted_when_joining = settings.start_muted_when_joining;
    let sentry_reports_enabled = settings.sentry_reports_enabled.unwrap_or(false);
    let debug_mode_enabled = settings.debug_mode_enabled;
    let locale = settings.locale;

    Self {
      start_muted_when_joining,
      sentry_reports_enabled,
      debug_mode_enabled,
      locale,
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
    let section_spacing = settings_section_spacing(ctx);
    let content = page_stack(ctx)
      .child(header(
        &ctx.t("settings.overview.title"),
        &ctx.t("settings.overview.description"),
      ))
      .child(
        Row::new()
          .width(Dimension::Pct(100.0))
          .spacing(section_spacing)
          .child(identity_card.flex(1.0))
          .child(servers_card.flex(1.0)),
      )
      .child(
        Column::new()
          .width(Dimension::Pct(100.0))
          .spacing(12.0)
          .child(audio_section_label(&ctx.t("settings.overview.language.section")))
          .child(
            Column::new()
              .width(Dimension::Pct(100.0))
              .child(ctx.mount::<OverviewLanguageSetting>(OverviewLanguageSettingProps {
                initial_locale: self.locale.clone(),
              })),
          ),
      )
      .child(
        Column::new()
          .width(Dimension::Pct(100.0))
          .spacing(12.0)
          .child(audio_section_label(&ctx.t("settings.overview.toggles")))
          .child(
            Column::new()
              .width(Dimension::Pct(100.0))
              .child(ctx.mount::<OverviewToggleSetting>(OverviewToggleSettingProps {
                title_key: "settings.overview.toggle.muted.title",
                description_key: "settings.overview.toggle.muted.description",
                initial_enabled: self.start_muted_when_joining,
                setting: OverviewBoolSetting::StartMutedWhenJoining,
              }))
              .child(ctx.mount::<OverviewToggleSetting>(OverviewToggleSettingProps {
                title_key: "settings.overview.toggle.sentry_reports.title",
                description_key: "settings.overview.toggle.sentry_reports.description",
                initial_enabled: self.sentry_reports_enabled,
                setting: OverviewBoolSetting::SentryReportsEnabled,
              }))
              .child(ctx.mount::<OverviewToggleSetting>(OverviewToggleSettingProps {
                title_key: "settings.overview.toggle.debug_mode.title",
                description_key: "settings.overview.toggle.debug_mode.description",
                initial_enabled: self.debug_mode_enabled,
                setting: OverviewBoolSetting::DebugModeEnabled,
              })),
          ),
      );

    screen(ctx, SettingsPage::Overview, content)
  }
}

#[derive(Clone, Copy, PartialEq, Eq, lurq::DevtoolsInspectable)]
enum OverviewBoolSetting {
  StartMutedWhenJoining,
  SentryReportsEnabled,
  DebugModeEnabled,
}

#[derive(Clone, lurq::DevtoolsInspectable)]
struct OverviewToggleSettingProps {
  title_key: &'static str,
  description_key: &'static str,
  initial_enabled: bool,
  setting: OverviewBoolSetting,
}

impl PartialEq for OverviewToggleSettingProps {
  fn eq(&self, other: &Self) -> bool {
    self.title_key == other.title_key
      && self.description_key == other.description_key
      && self.initial_enabled == other.initial_enabled
      && self.setting == other.setting
  }
}

struct OverviewToggleSetting {
  enabled: Signal<bool>,
}

impl Component for OverviewToggleSetting {
  type Props = OverviewToggleSettingProps;

  fn create(ctx: &mut Ctx) -> Self {
    let props = ctx.props::<Self::Props>().clone();
    let enabled = ctx.signal(props.initial_enabled);
    let session = ctx.use_context::<ServerSession>();
    if let Some(storage) = ctx.use_context::<Storage>() {
      ctx.watch(&enabled, move |enabled| {
        let mut settings = storage.load_settings().unwrap_or_default();
        match props.setting {
          OverviewBoolSetting::StartMutedWhenJoining => settings.start_muted_when_joining = *enabled,
          OverviewBoolSetting::SentryReportsEnabled => settings.sentry_reports_enabled = Some(*enabled),
          OverviewBoolSetting::DebugModeEnabled => settings.debug_mode_enabled = *enabled,
        }
        let _ = storage.save_settings(&settings);
        if props.setting == OverviewBoolSetting::SentryReportsEnabled {
          logger::apply_sentry_reports_enabled(Some(*enabled));
        }
        if props.setting == OverviewBoolSetting::DebugModeEnabled
          && let Some(session) = session.as_ref()
        {
          session.refresh_lobby();
        }
      });
    }
    Self { enabled }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let props = ctx.props::<Self::Props>().clone();
    let enabled = self.enabled.get();
    let enabled_signal = self.enabled.clone();

    audio_row(
      &ctx.t(props.title_key),
      &ctx.t(props.description_key),
      settings_toggle(enabled, move || {
        let current = enabled_signal.get_untracked();
        enabled_signal.set(!current);
      }),
      true,
    )
  }
}

#[derive(Clone, lurq::DevtoolsInspectable)]
struct OverviewLanguageSettingProps {
  initial_locale: String,
}

impl PartialEq for OverviewLanguageSettingProps {
  fn eq(&self, other: &Self) -> bool {
    self.initial_locale == other.initial_locale
  }
}

struct OverviewLanguageSetting {
  locale: Signal<String>,
}

impl Component for OverviewLanguageSetting {
  type Props = OverviewLanguageSettingProps;

  fn create(ctx: &mut Ctx) -> Self {
    let props = ctx.props::<Self::Props>().clone();
    let locale = ctx.signal(props.initial_locale);
    let storage = ctx.use_context::<Storage>();
    let i18n = ctx.i18n().clone();
    ctx.watch(&locale, move |locale| {
      i18n.set_locale(locale.clone());
      if let Some(storage) = storage.as_ref() {
        let mut settings = storage.load_settings().unwrap_or_default();
        settings.locale = locale.clone();
        let _ = storage.save_settings(&settings);
      }
    });
    Self { locale }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    audio_row(
      &ctx.t("settings.overview.language.title"),
      &ctx.t("settings.overview.language.description"),
      dropdown_menu(self.locale.clone(), language_options(ctx), "", LANGUAGE_DROPDOWN_WIDTH),
      true,
    )
  }
}

fn language_options(ctx: &Ctx) -> Vec<DropdownOption> {
  vec![
    DropdownOption {
      value: "en".to_owned(),
      label: ctx.t("settings.overview.language.option.en").to_string(),
    },
    DropdownOption {
      value: "uk".to_owned(),
      label: ctx.t("settings.overview.language.option.uk").to_string(),
    },
    DropdownOption {
      value: "be".to_owned(),
      label: ctx.t("settings.overview.language.option.be").to_string(),
    },
  ]
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
    SettingsPage::Identity,
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
    SettingsPage::Servers,
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
  page: SettingsPage,
  route: &'static str,
) -> Column {
  card(ctx)
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
    .child(manage_button(ctx, action, page, route))
}

fn manage_button(ctx: &mut Ctx, label: &str, page: SettingsPage, route: &'static str) -> Row {
  let settings_popup = ctx
    .use_context::<SettingsPopupHandle>()
    .filter(SettingsPopupHandle::is_open);
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

  if let Some(settings_popup) = settings_popup {
    row = row.on_click(move |_| settings_popup.open_page(page));
  } else if let Some(navigator) = navigator {
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
