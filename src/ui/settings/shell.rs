use lurq::{
  app::ctx::Ctx,
  components::{Column, Row, Text},
  layout::Alignment,
  node::{BackgroundColor, CursorIcon, Element, Style, border::Border, dimension::Dimension},
};

use crate::{
  routes::{
    ROUTE_CHOOSE_SERVER, ROUTE_SETTINGS, ROUTE_SETTINGS_AUDIO, ROUTE_SETTINGS_IDENTITY, ROUTE_SETTINGS_SERVERS,
    ROUTE_SETTINGS_STREAM,
  },
  session::ServerSession,
  theme,
  ui::common::lucide_icon::{LucideIcon, LucideIconProps},
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum SettingsPage {
  Overview,
  Identity,
  Servers,
  Audio,
  Stream,
}

pub(super) fn screen(ctx: &mut Ctx, page: SettingsPage, content: impl Into<Element>) -> Element {
  screen_base(
    ctx,
    page,
    Column::new()
      .width(Dimension::Pct(100.0))
      .height(Dimension::Pct(100.0))
      .padding_vertical(40.0)
      .padding_horizontal(40.0)
      .child(content),
  )
}

pub(super) fn screen_full(ctx: &mut Ctx, page: SettingsPage, content: impl Into<Element>) -> Element {
  screen_base(ctx, page, content)
}

fn screen_base(ctx: &mut Ctx, page: SettingsPage, content: impl Into<Element>) -> Element {
  let window_height = ctx.window().logical_height();

  Row::new()
    .width(Dimension::Pct(100.0))
    .height(window_height)
    .clip()
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceBase))
    .child(nav(ctx, page))
    .child(
      Column::new()
        .flex(1.0)
        .height(Dimension::Pct(100.0))
        .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceBase))
        .child(content),
    )
    .into()
}

pub(super) fn page_stack() -> Column {
  Column::new().width(Dimension::Pct(100.0)).spacing(26.0)
}

pub(super) fn header(title: &str, description: &str) -> Element {
  Column::new()
    .width(Dimension::Pct(100.0))
    .spacing(theme::SpacingSize::Sm)
    .child(Text::new(title).variant(theme::TypographyStyle::Title))
    .child(
      Text::new(description)
        .variant(theme::TypographyStyle::Description)
        .width(Dimension::Pct(100.0)),
    )
    .into()
}

pub(super) fn card() -> Column {
  Column::new()
    .width(Dimension::Pct(100.0))
    .spacing(theme::SpacingSize::Lg)
    .padding_vertical(theme::SpacingSize::Xl)
    .padding_horizontal(theme::SpacingSize::Xl)
    .rounded(theme::RadiusSize::Lg)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfacePanel))
    .border_inside(1.0, theme::PaletteColor::Border)
}

pub(super) fn setting_row(label: &str, description: &str, trailing: Element, danger: bool) -> Element {
  Row::new()
    .width(Dimension::Pct(100.0))
    .align_items(Alignment::Center)
    .spacing(theme::SpacingSize::Lg)
    .padding_vertical(theme::SpacingSize::Lg)
    .padding_horizontal(theme::SpacingSize::Xl)
    .border_bottom(Border::inside(
      1.0,
      if danger {
        theme::PaletteColor::Danger
      } else {
        theme::PaletteColor::Border
      },
    ))
    .child(
      Column::new()
        .flex(1.0)
        .spacing(theme::SpacingSize::Xs)
        .child(
          Text::new(label)
            .variant(theme::TypographyStyle::Heading)
            .color(if danger {
              theme::PaletteColor::Danger
            } else {
              theme::PaletteColor::TextPrimary
            }),
        )
        .child(Text::new(description).variant(theme::TypographyStyle::Link)),
    )
    .child(trailing)
    .into()
}

pub(super) fn muted_notice(ctx: &mut Ctx, title: &str, description: &str) -> Element {
  Row::new()
    .width(Dimension::Pct(100.0))
    .align_items(Alignment::Start)
    .spacing(theme::SpacingSize::Lg)
    .padding_vertical(theme::SpacingSize::Lg)
    .padding_horizontal(theme::SpacingSize::Xl)
    .rounded(theme::RadiusSize::Lg)
    .border_inside(1.0, theme::PaletteColor::BorderStrong)
    .child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon: "info",
      size: 16.0,
      color: theme::palette().warning,
    }))
    .child(
      Column::new()
        .flex(1.0)
        .spacing(theme::SpacingSize::Xs)
        .child(
          Text::new(title)
            .variant(theme::TypographyStyle::Heading)
            .color(theme::PaletteColor::Warning),
        )
        .child(Text::new(description).variant(theme::TypographyStyle::Link)),
    )
    .into()
}

pub(super) fn value_text(value: &str) -> Element {
  Text::new(value)
    .variant(theme::TypographyStyle::Mono)
    .color(theme::PaletteColor::TextMuted)
    .into()
}

fn nav(ctx: &mut Ctx, page: SettingsPage) -> Element {
  Column::new()
    .width(320.0)
    .height(Dimension::Pct(100.0))
    .spacing(theme::SpacingSize::Sm)
    .padding_vertical(theme::SpacingSize::Xl)
    .padding_horizontal(theme::SpacingSize::Lg)
    .background(BackgroundColor::Color(theme::palette().surface_base.with_opacity(0.55)))
    .border_right(Border::inside(1.0, theme::PaletteColor::Border))
    .child(back_row(ctx))
    .child(nav_section(&ctx.t("settings.nav.section.account")))
    .child(nav_item(
      ctx,
      SettingsPage::Overview,
      page,
      "user",
      &ctx.t("settings.nav.overview"),
      ROUTE_SETTINGS,
    ))
    .child(nav_item(
      ctx,
      SettingsPage::Identity,
      page,
      "key-round",
      &ctx.t("settings.nav.identity"),
      ROUTE_SETTINGS_IDENTITY,
    ))
    .child(nav_item(
      ctx,
      SettingsPage::Servers,
      page,
      "server",
      &ctx.t("settings.nav.servers"),
      ROUTE_SETTINGS_SERVERS,
    ))
    .child(nav_section(&ctx.t("settings.nav.section.device")))
    .child(nav_item(
      ctx,
      SettingsPage::Audio,
      page,
      "sliders-horizontal",
      &ctx.t("settings.nav.audio"),
      ROUTE_SETTINGS_AUDIO,
    ))
    .child(nav_item(
      ctx,
      SettingsPage::Stream,
      page,
      "video",
      &ctx.t("settings.nav.video"),
      ROUTE_SETTINGS_STREAM,
    ))
    .into()
}

fn back_row(ctx: &mut Ctx) -> Element {
  let navigator = ctx.navigator();
  let current_server = ctx.use_context::<ServerSession>().and_then(|session| session.info());
  let back_label = current_server
    .as_ref()
    .map(|server| {
      let name = if server.server_name.trim().is_empty() {
        server.address.as_str()
      } else {
        server.server_name.as_str()
      };
      ctx.t_args("settings.nav.back_to_server", [("server", name.to_owned())])
    })
    .unwrap_or_else(|| ctx.t("settings.nav.back_to_server_selection"));
  let back_to_history = current_server.is_some();
  let mut row = Row::new()
    .width(Dimension::Pct(100.0))
    .align_items(Alignment::Center)
    .spacing(theme::SpacingSize::Md)
    .padding_vertical(theme::SpacingSize::Sm)
    .padding_horizontal(theme::SpacingSize::Sm)
    .cursor(CursorIcon::Pointer)
    .hovered_style(Style::new().background(BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised)))
    .child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon: "arrow-left",
      size: 16.0,
      color: theme::palette().text_secondary,
    }))
    .child(
      Text::new(&back_label)
        .variant(theme::TypographyStyle::Description)
        .color(theme::PaletteColor::TextSecondary),
    );

  if let Some(navigator) = navigator {
    row = row.on_click(move |_| {
      if !back_to_history || !navigator.back() {
        navigator.push(ROUTE_CHOOSE_SERVER);
      }
    });
  }

  row.into()
}

fn nav_section(label: &str) -> Element {
  Row::new()
    .width(Dimension::Pct(100.0))
    .padding_vertical(theme::SpacingSize::Xs)
    .padding_horizontal(theme::SpacingSize::Md)
    .child(
      Text::new(label)
        .variant(theme::TypographyStyle::Caption)
        .color(theme::PaletteColor::TextMuted),
    )
    .into()
}

fn nav_item(
  ctx: &mut Ctx,
  item_page: SettingsPage,
  current_page: SettingsPage,
  icon: &'static str,
  label: &str,
  route: &'static str,
) -> Element {
  let active = item_page == current_page;
  let navigator = ctx.navigator();
  let mut row = nav_item_base(ctx, icon, label, active).cursor(CursorIcon::Pointer);

  if let Some(navigator) = navigator {
    row = row.on_click(move |_| navigator.push(route));
  }

  row.into()
}

fn nav_item_base(ctx: &mut Ctx, icon: &'static str, label: &str, active: bool) -> Row {
  Row::new()
    .width(Dimension::Pct(100.0))
    .align_items(Alignment::Center)
    .spacing(theme::SpacingSize::Md)
    .padding_vertical(theme::SpacingSize::Sm)
    .padding_horizontal(theme::SpacingSize::Md)
    .rounded(theme::RadiusSize::Lg)
    .background(if active {
      BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised)
    } else {
      BackgroundColor::Color(theme::palette().surface_base.with_opacity(0.0))
    })
    .hovered_style(Style::new().background(BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised)))
    .child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon,
      size: 16.0,
      color: if active {
        theme::palette().text_primary
      } else {
        theme::palette().text_secondary
      },
    }))
    .child(
      Text::new(label)
        .variant(theme::TypographyStyle::Description)
        .color(if active {
          theme::PaletteColor::TextPrimary
        } else {
          theme::PaletteColor::TextSecondary
        })
        .width(Dimension::Pct(100.0)),
    )
}
