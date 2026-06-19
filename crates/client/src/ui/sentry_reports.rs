use lurq::{
  app::{component::Component, ctx::Ctx},
  components::{Column, Row, Text},
  core::Store,
  layout::{Alignment, layout_kind::Justify},
  node::{BackgroundColor, CursorIcon, Element, Style, dimension::Dimension},
};

use crate::{
  identity::LocalIdentity,
  routes::{ROUTE_CHOOSE_SERVER, ROUTE_IDENTITY_SETUP},
  services::logger,
  storage::AppSettingsUpdater,
  theme,
  ui::common::lucide_icon::{LucideIcon, LucideIconProps},
};

const CONSENT_WIDTH: f32 = 560.0;

pub struct SentryReportsScreen;

impl Component for SentryReportsScreen {
  type Props = ();

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    Column::new()
      .width(Dimension::Pct(100.0))
      .height(Dimension::Pct(100.0))
      .flex(1.0)
      .align_items(Alignment::Center)
      .justify(Justify::Center)
      .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceBase))
      .clip()
      .child(
        Column::new()
          .width(CONSENT_WIDTH)
          .align_items(Alignment::Center)
          .spacing(26.0)
          .child(header(ctx))
          .child(notice(ctx))
          .child(actions(ctx)),
      )
  }
}

fn header(ctx: &mut Ctx) -> Column {
  Column::new()
    .align_items(Alignment::Center)
    .spacing(20.0)
    .child(
      Row::new()
        .width(68.0)
        .height(68.0)
        .align_items(Alignment::Center)
        .justify(Justify::Center)
        .rounded(16.0)
        .background(BackgroundColor::Palette(theme::PaletteColor::InfoMuted))
        .border_inside(1.0, theme::PaletteColor::Accent)
        .child(ctx.mount::<LucideIcon>(LucideIconProps {
          icon: "shield-check",
          size: 28.0,
          color: theme::palette().info,
        })),
    )
    .child(
      Column::new()
        .align_items(Alignment::Center)
        .spacing(10.0)
        .child(Text::new(&ctx.t("sentry_reports.title")).variant(theme::TypographyStyle::Title))
        .child(
          Text::new(&ctx.t("sentry_reports.description"))
            .variant(theme::TypographyStyle::Description)
            .width(500.0)
            .text_align(Alignment::Center),
        ),
    )
}

fn notice(ctx: &mut Ctx) -> Row {
  Row::new()
    .width(CONSENT_WIDTH)
    .spacing(12.0)
    .padding_vertical(16.0)
    .padding_horizontal(18.0)
    .rounded(theme::RadiusSize::Lg)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfacePanel))
    .border_inside(1.0, theme::PaletteColor::Border)
    .child(
      Column::new()
        .padding_top(2.0)
        .child(ctx.mount::<LucideIcon>(LucideIconProps {
          icon: "bug",
          size: 16.0,
          color: theme::palette().info,
        })),
    )
    .child(
      Column::new()
        .width(Dimension::Pct(100.0))
        .spacing(6.0)
        .child(
          Text::new(&ctx.t("sentry_reports.notice.label"))
            .variant(theme::TypographyStyle::Button)
            .color(theme::PaletteColor::Info),
        )
        .child(
          Text::new(&ctx.t("sentry_reports.notice.detail"))
            .variant(theme::TypographyStyle::Link)
            .color(theme::PaletteColor::TextSecondary)
            .width(Dimension::Pct(100.0)),
        ),
    )
}

fn actions(ctx: &mut Ctx) -> Row {
  Row::new()
    .width(CONSENT_WIDTH)
    .spacing(14.0)
    .child(consent_button(
      ctx,
      None,
      &ctx.t("sentry_reports.action.decline"),
      ConsentButtonTone::Secondary,
    ))
    .child(consent_button(
      ctx,
      Some("send"),
      &ctx.t("sentry_reports.action.accept"),
      ConsentButtonTone::Primary,
    ))
}

#[derive(Clone, Copy)]
enum ConsentButtonTone {
  Primary,
  Secondary,
}

fn consent_button(ctx: &mut Ctx, icon: Option<&'static str>, label: &str, tone: ConsentButtonTone) -> Row {
  let (background, border, text_color, icon_color, hover_background, enabled) = match tone {
    ConsentButtonTone::Primary => (
      BackgroundColor::Palette(theme::PaletteColor::Accent),
      theme::PaletteColor::Accent,
      theme::PaletteColor::TextInverse,
      theme::palette().text_inverse,
      BackgroundColor::Palette(theme::PaletteColor::AccentHover),
      true,
    ),
    ConsentButtonTone::Secondary => (
      BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised),
      theme::PaletteColor::BorderStrong,
      theme::PaletteColor::TextPrimary,
      theme::palette().text_primary,
      BackgroundColor::Palette(theme::PaletteColor::SurfaceInput),
      false,
    ),
  };
  let settings_updater = ctx.use_context::<AppSettingsUpdater>();
  let identity_store = ctx.use_context::<Store<Option<LocalIdentity>>>();
  let navigator = ctx.navigator();

  let mut button = Row::new()
    .height(34.0)
    .flex(1.0)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .spacing(7.0)
    .padding_horizontal(14.0)
    .rounded(theme::RadiusSize::Md)
    .background(background)
    .border_inside(1.0, border)
    .cursor(CursorIcon::Pointer)
    .hovered_style(Style::new().background(hover_background.clone()))
    .active_style(Style::new().background(hover_background))
    .on_click(move |_| {
      save_sentry_reports_choice(settings_updater.as_ref(), enabled);
      if let Some(navigator) = navigator.as_ref() {
        navigator.replace(route_after_consent(identity_store.as_ref()));
      }
    });

  if let Some(icon) = icon {
    button = button.child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon,
      size: 16.0,
      color: icon_color,
    }));
  }

  button.child(
    Text::new(label)
      .variant(theme::TypographyStyle::Button)
      .color(text_color)
      .nowrap(),
  )
}

fn save_sentry_reports_choice(settings_updater: Option<&AppSettingsUpdater>, enabled: bool) {
  if let Some(settings_updater) = settings_updater {
    settings_updater.update(|settings| {
      settings.sentry_reports_enabled = Some(enabled);
    });
  }

  if enabled {
    logger::enable_sentry_reports();
  }
}

fn route_after_consent(identity_store: Option<&Store<Option<LocalIdentity>>>) -> &'static str {
  if identity_store.is_some_and(|identity| identity.with(Option::is_some)) {
    ROUTE_CHOOSE_SERVER
  } else {
    ROUTE_IDENTITY_SETUP
  }
}
