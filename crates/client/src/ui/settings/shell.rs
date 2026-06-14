use lurq::{
  app::{ctx::Ctx, theme::Breakpoint},
  components::{Column, Row, ScrollVertical, Text},
  core::Signal,
  layout::{
    Alignment,
    scrollbar::{ScrollBarPlacement, ScrollBarStyle},
  },
  node::{BackgroundColor, CursorIcon, Element, Style, border::Border, dimension::Dimension},
};

use crate::{
  routes::{
    ROUTE_CHOOSE_SERVER, ROUTE_LOBBY, ROUTE_SETTINGS, ROUTE_SETTINGS_AUDIO, ROUTE_SETTINGS_IDENTITY,
    ROUTE_SETTINGS_NOTIFICATIONS, ROUTE_SETTINGS_SERVERS, ROUTE_SETTINGS_STREAM,
  },
  session::ServerSession,
  theme,
  ui::{
    app_chrome::content_height,
    common::lucide_icon::{LucideIcon, LucideIconProps},
  },
};

const NAV_LABEL_Y_OFFSET: f32 = -1.0;

#[derive(Clone, Copy)]
pub(super) struct SettingsLayoutMetrics {
  pub nav_width: f32,
  pub nav_padding_x: f32,
  pub content_padding_x: f32,
  pub content_padding_y: f32,
  pub page_spacing: f32,
  pub section_spacing: f32,
  pub card_padding_x: f32,
  pub card_padding_y: f32,
}

pub(super) fn settings_layout_metrics(ctx: &Ctx) -> SettingsLayoutMetrics {
  match ctx.breakpoint() {
    Some(Breakpoint::Md) => SettingsLayoutMetrics {
      nav_width: 236.0,
      nav_padding_x: 10.0,
      content_padding_x: 24.0,
      content_padding_y: 28.0,
      page_spacing: 22.0,
      section_spacing: 20.0,
      card_padding_x: 16.0,
      card_padding_y: 16.0,
    },
    Some(Breakpoint::Lg) => SettingsLayoutMetrics {
      nav_width: 280.0,
      nav_padding_x: 12.0,
      content_padding_x: 32.0,
      content_padding_y: 34.0,
      page_spacing: 24.0,
      section_spacing: 22.0,
      card_padding_x: 18.0,
      card_padding_y: 18.0,
    },
    Some(Breakpoint::Xl) | Some(Breakpoint::Sm) | None => SettingsLayoutMetrics {
      nav_width: 320.0,
      nav_padding_x: 14.0,
      content_padding_x: 40.0,
      content_padding_y: 40.0,
      page_spacing: 26.0,
      section_spacing: 24.0,
      card_padding_x: 20.0,
      card_padding_y: 20.0,
    },
  }
}

pub(super) fn settings_content_padding(ctx: &Ctx) -> (f32, f32) {
  let metrics = settings_layout_metrics(ctx);
  (metrics.content_padding_x, metrics.content_padding_y)
}

pub(super) fn settings_section_spacing(ctx: &Ctx) -> f32 {
  settings_layout_metrics(ctx).section_spacing
}

fn settings_scroll_view(content: impl Into<Element>) -> ScrollVertical {
  ScrollVertical::new(content)
    .scrollbar(settings_scrollbar_style())
    .scrollbar_hovered(|mut style| {
      let palette = theme::palette();
      style.thumb_color = palette.accent_hover;
      style.track_color = palette.surface_input.with_opacity(0.75);
      style
    })
}

fn settings_scrollbar_style() -> ScrollBarStyle {
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

#[derive(Clone, Copy, PartialEq, Eq, lurq::DevtoolsInspectable)]
pub enum SettingsPage {
  Overview,
  Identity,
  Servers,
  Audio,
  Notifications,
  Stream,
}

#[derive(Clone)]
pub struct SettingsPopupHandle {
  open: Signal<bool>,
  page: Signal<SettingsPage>,
}

impl SettingsPopupHandle {
  pub fn new(open: Signal<bool>, page: Signal<SettingsPage>) -> Self {
    Self { open, page }
  }

  pub fn open(&self) {
    self.open_page(SettingsPage::Overview);
  }

  pub fn open_page(&self, page: SettingsPage) {
    self.page.set(page);
    self.open.set(true);
  }

  pub fn close(&self) {
    self.open.set(false);
  }

  pub fn is_open(&self) -> bool {
    self.open.get()
  }

  pub fn page(&self) -> SettingsPage {
    self.page.get()
  }
}

pub(super) fn screen(ctx: &mut Ctx, page: SettingsPage, content: impl Into<Element>) -> Element {
  let metrics = settings_layout_metrics(ctx);
  let content = Column::new()
    .width(Dimension::Pct(100.0))
    .padding_vertical(metrics.content_padding_y)
    .padding_horizontal(metrics.content_padding_x)
    .child(content);

  screen_base(
    ctx,
    page,
    settings_scroll_view(content)
      .width(Dimension::Pct(100.0))
      .height(Dimension::Pct(100.0)),
  )
}

pub(super) fn screen_full(ctx: &mut Ctx, page: SettingsPage, content: impl Into<Element>) -> Element {
  screen_base(ctx, page, content)
}

fn screen_base(ctx: &mut Ctx, page: SettingsPage, content: impl Into<Element>) -> Element {
  let window_height = content_height(ctx);

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

pub(super) fn page_stack(ctx: &Ctx) -> Column {
  Column::new()
    .width(Dimension::Pct(100.0))
    .spacing(settings_layout_metrics(ctx).page_spacing)
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

pub(super) fn card(ctx: &Ctx) -> Column {
  let metrics = settings_layout_metrics(ctx);
  Column::new()
    .width(Dimension::Pct(100.0))
    .spacing(theme::SpacingSize::Lg)
    .padding_vertical(metrics.card_padding_y)
    .padding_horizontal(metrics.card_padding_x)
    .rounded(theme::RadiusSize::Lg)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfacePanel))
    .border_inside(1.0, theme::PaletteColor::Border)
}

pub(super) fn muted_notice(ctx: &mut Ctx, title: &str, description: &str) -> Element {
  let metrics = settings_layout_metrics(ctx);
  Row::new()
    .width(Dimension::Pct(100.0))
    .align_items(Alignment::Start)
    .spacing(theme::SpacingSize::Lg)
    .padding_vertical(theme::SpacingSize::Lg)
    .padding_horizontal(metrics.card_padding_x)
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
  let metrics = settings_layout_metrics(ctx);

  Column::new()
    .width(metrics.nav_width)
    .height(Dimension::Pct(100.0))
    .spacing(theme::SpacingSize::Sm)
    .padding_vertical(theme::SpacingSize::Xl)
    .padding_horizontal(metrics.nav_padding_x)
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
      SettingsPage::Notifications,
      page,
      "volume-2",
      &ctx.t("settings.nav.notifications"),
      ROUTE_SETTINGS_NOTIFICATIONS,
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
  let settings_popup = ctx
    .use_context::<SettingsPopupHandle>()
    .filter(SettingsPopupHandle::is_open);
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
  let back_route = if current_server.is_some() {
    ROUTE_LOBBY
  } else {
    ROUTE_CHOOSE_SERVER
  };
  let mut row = Row::new()
    .width(Dimension::Pct(100.0))
    .align_items(Alignment::Center)
    .spacing(theme::SpacingSize::Md)
    .padding_vertical(theme::SpacingSize::Sm)
    .padding_horizontal(theme::SpacingSize::Sm)
    .rounded(theme::RadiusSize::Lg)
    .cursor(CursorIcon::Pointer)
    .hovered_style(Style::new().background(BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised)))
    .active_style(Style::new().background(BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised)))
    .child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon: "arrow-left",
      size: 16.0,
      color: theme::palette().text_secondary,
    }))
    .child(
      Text::new(&back_label)
        .variant(theme::TypographyStyle::Description)
        .offset(0.0, NAV_LABEL_Y_OFFSET)
        .color(theme::PaletteColor::TextSecondary),
    );

  if let Some(settings_popup) = settings_popup {
    row = row.on_click(move |_| {
      settings_popup.close();
    });
  } else if let Some(navigator) = navigator {
    row = row.on_click(move |_| {
      navigator.replace(back_route);
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
  let settings_popup = ctx
    .use_context::<SettingsPopupHandle>()
    .filter(SettingsPopupHandle::is_open);
  let navigator = ctx.navigator();
  let mut row = nav_item_base(ctx, icon, label, active).cursor(CursorIcon::Pointer);

  if let Some(settings_popup) = settings_popup {
    row = row.on_click(move |_| {
      settings_popup.open_page(item_page);
    });
  } else if let Some(navigator) = navigator {
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
        .offset(0.0, NAV_LABEL_Y_OFFSET)
        .color(if active {
          theme::PaletteColor::TextPrimary
        } else {
          theme::PaletteColor::TextSecondary
        })
        .width(Dimension::Pct(100.0)),
    )
}
