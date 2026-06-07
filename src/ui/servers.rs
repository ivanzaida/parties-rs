use std::time::Duration;

mod server_card;

use lurq::{
  app::{
    component::Component,
    ctx::{Ctx, Timeout},
  },
  components::{Column, Row, Text},
  core::Signal,
  layout::{Alignment, layout_kind::Justify},
  node::{BackgroundColor, CursorIcon, Element, Style, color::Color, dimension::Dimension},
};
use server_card::{ServerCard, ServerCardProps, ServerCardState};

use crate::{
  routes::{ROUTE_CONNECT_SERVER, ROUTE_SETTINGS},
  storage::{Storage, StoredServer},
  theme,
  ui::{
    common::lucide_icon::{LucideIcon, LucideIconProps},
    connect_server::ConnectOrigin,
  },
};

const CONNECT_PREVIEW_DELAY: Duration = Duration::from_millis(1300);

pub struct SavedServersScreen {
  connecting: Signal<Option<String>>,
  failed: Signal<Option<String>>,
  connect_timeout: Timeout,
}

impl Component for SavedServersScreen {
  type Props = ();

  fn create(ctx: &mut Ctx) -> Self {
    let connecting = ctx.signal(None::<String>);
    let failed = ctx.signal(None::<String>);
    let timeout_connecting = connecting.clone();
    let timeout_failed = failed.clone();
    let connect_timeout = ctx.create_timeout(CONNECT_PREVIEW_DELAY, move || {
      if let Some(address) = timeout_connecting.get_untracked() {
        timeout_connecting.set(None);
        timeout_failed.set(Some(address));
      }
    });

    Self {
      connecting,
      failed,
      connect_timeout,
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let servers = ctx
      .use_context::<Storage>()
      .and_then(|storage| storage.load_servers().ok())
      .unwrap_or_default();

    Column::new()
      .width(Dimension::Pct(100.0))
      .height(Dimension::Pct(100.0))
      .flex(1.0)
      .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceBase))
      .clip()
      .child(top_bar(ctx))
      .child(if servers.is_empty() {
        self.empty_state(ctx).into()
      } else {
        self.servers_state(ctx, servers).into()
      })
  }
}

impl SavedServersScreen {
  fn empty_state(&self, ctx: &mut Ctx) -> impl Into<Element> {
    Column::new()
      .width(Dimension::Pct(100.0))
      .flex(1.0)
      .align_items(Alignment::Center)
      .justify(Justify::Center)
      .spacing(theme::SpacingSize::Xl)
      .child(
        Row::new()
          .width(64.0)
          .height(64.0)
          .align_items(Alignment::Center)
          .justify(Justify::Center)
          .rounded(theme::RadiusSize::Lg)
          .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised))
          .border_inside(1.0, theme::PaletteColor::Border)
          .child(ctx.mount::<LucideIcon>(LucideIconProps {
            icon: "server",
            size: 28.0,
            color: theme::palette().text_secondary,
          })),
      )
      .child(
        Column::new()
          .width(480.0)
          .align_items(Alignment::Center)
          .spacing(theme::SpacingSize::Md)
          .child(Text::new(&ctx.t("servers.empty.title")).variant(theme::TypographyStyle::Title))
          .child(
            Text::new(&ctx.t("servers.empty.description"))
              .variant(theme::TypographyStyle::Description)
              .text_align(Alignment::Center)
              .width(Dimension::Pct(100.0)),
          ),
      )
      .child(add_server_button(
        ctx,
        &ctx.t("servers.action.add_one"),
        ButtonTone::Primary,
      ))
  }

  fn servers_state(&self, ctx: &mut Ctx, servers: Vec<StoredServer>) -> impl Into<Element> {
    let count = servers.len();
    let connecting = self.connecting.get();
    let failed = self.failed.get();
    let connecting_signal = self.connecting.clone();
    let failed_signal = self.failed.clone();

    if connecting.is_some() && !self.connect_timeout.is_active() {
      self.connect_timeout.start();
    }

    let mut list = Column::new()
      .width(Dimension::Pct(100.0))
      .spacing(theme::SpacingSize::Lg);

    for server in servers {
      let address = server.address.clone();
      let state = if connecting.as_deref() == Some(address.as_str()) {
        ServerCardState::Connecting
      } else if failed.as_deref() == Some(address.as_str()) {
        ServerCardState::Error
      } else {
        ServerCardState::Idle
      };
      list = list.child(ctx.mount_keyed::<ServerCard>(
        &address,
        ServerCardProps {
          server,
          state,
          connecting: connecting_signal.clone(),
          failed: failed_signal.clone(),
        },
      ));
    }

    Column::new()
      .width(Dimension::Pct(100.0))
      .flex(1.0)
      .align_items(Alignment::Center)
      .padding_vertical(theme::SpacingSize::Section)
      .child(
        Column::new()
          .width(860.0)
          .spacing(theme::SpacingSize::Xl)
          .child(header(ctx))
          .child(list)
          .child(
            Row::new()
              .align_items(Alignment::Center)
              .spacing(theme::SpacingSize::Sm)
              .child(ctx.mount::<LucideIcon>(LucideIconProps {
                icon: "lock",
                size: 14.0,
                color: theme::palette().text_muted,
              }))
              .child(
                Text::new(&ctx.t_args("servers.footer.saved_count", [("count", count.to_string())]))
                  .variant(theme::TypographyStyle::Link)
                  .color(theme::PaletteColor::TextMuted),
              ),
          ),
      )
  }
}

fn top_bar(ctx: &mut Ctx) -> impl Into<Element> {
  let navigator = ctx.navigator();
  let mut settings_button = Row::new()
    .width(32.0)
    .height(32.0)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .rounded(theme::RadiusSize::Lg)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised))
    .border_inside(1.0, theme::PaletteColor::Border)
    .cursor(CursorIcon::Pointer)
    .hovered_style(Style::new().background(BackgroundColor::Palette(theme::PaletteColor::SurfaceInput)))
    .child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon: "settings",
      size: 16.0,
      color: theme::palette().text_secondary,
    }));

  if let Some(navigator) = navigator {
    settings_button = settings_button.on_click(move |_| navigator.push(ROUTE_SETTINGS));
  }

  Row::new()
    .width(Dimension::Pct(100.0))
    .height(56.0)
    .align_items(Alignment::Center)
    .justify(Justify::SpaceBetween)
    .padding_horizontal(theme::SpacingSize::Xl)
    .background(BackgroundColor::Color(Color::from_hex("#0D0E10")))
    .border_inside(1.0, theme::PaletteColor::Border)
    .child(
      Row::new()
        .align_items(Alignment::Center)
        .spacing(theme::SpacingSize::Md)
        .child(
          Row::new()
            .width(28.0)
            .height(28.0)
            .align_items(Alignment::Center)
            .justify(Justify::Center)
            .rounded(theme::RadiusSize::Lg)
            .background(BackgroundColor::Palette(theme::PaletteColor::Accent))
            .child(ctx.mount::<LucideIcon>(LucideIconProps {
              icon: "volume-2",
              size: 15.0,
              color: theme::palette().text_inverse,
            })),
        )
        .child(Text::new(&ctx.t("common.app_name")).variant(theme::TypographyStyle::Heading)),
    )
    .child(
      Row::new()
        .align_items(Alignment::Center)
        .spacing(theme::SpacingSize::Lg)
        .child(
          Row::new()
            .width(28.0)
            .height(28.0)
            .align_items(Alignment::Center)
            .justify(Justify::Center)
            .rounded(theme::RadiusSize::Lg)
            .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised))
            .child(
              Text::new(&ctx.t("servers.user.initials"))
                .variant(theme::TypographyStyle::Mono)
                .color(theme::PaletteColor::TextSecondary),
            ),
        )
        .child(
          Text::new(&ctx.t("servers.user.name"))
            .variant(theme::TypographyStyle::Description)
            .color(theme::PaletteColor::TextSecondary),
        )
        .child(settings_button),
    )
}

fn header(ctx: &mut Ctx) -> impl Into<Element> {
  Row::new()
    .width(Dimension::Pct(100.0))
    .align_items(Alignment::End)
    .justify(Justify::SpaceBetween)
    .child(
      Column::new()
        .spacing(theme::SpacingSize::Sm)
        .child(
          Text::new(&ctx.t("servers.overline"))
            .variant(theme::TypographyStyle::Caption)
            .color(theme::PaletteColor::TextMuted),
        )
        .child(Text::new(&ctx.t("servers.title")).variant(theme::TypographyStyle::Title)),
    )
    .child(add_server_button(
      ctx,
      &ctx.t("servers.action.add"),
      ButtonTone::Secondary,
    ))
}

fn add_server_button(ctx: &mut Ctx, label: &str, tone: ButtonTone) -> Row {
  let navigator = ctx.navigator();
  let button = action_button(ctx, "plus", label, tone);
  if let Some(navigator) = navigator {
    button.on_click(move |_| navigator.push_with_state(ROUTE_CONNECT_SERVER, ConnectOrigin::ServerList))
  } else {
    button
  }
}

#[derive(Clone, Copy)]
enum ButtonTone {
  Primary,
  Secondary,
}

fn action_button(ctx: &mut Ctx, icon: &'static str, label: &str, tone: ButtonTone) -> Row {
  let (background, border, text_color, icon_color, hover_background) = match tone {
    ButtonTone::Primary => (
      BackgroundColor::Palette(theme::PaletteColor::Accent),
      BackgroundColor::Palette(theme::PaletteColor::Accent),
      theme::PaletteColor::TextInverse,
      theme::palette().text_inverse,
      BackgroundColor::Palette(theme::PaletteColor::AccentHover),
    ),
    ButtonTone::Secondary => (
      BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised),
      BackgroundColor::Palette(theme::PaletteColor::BorderStrong),
      theme::PaletteColor::TextPrimary,
      theme::palette().text_secondary,
      BackgroundColor::Palette(theme::PaletteColor::SurfaceInput),
    ),
  };

  Row::new()
    .height(34.0)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .spacing(theme::SpacingSize::Sm)
    .padding_horizontal(theme::SpacingSize::Lg)
    .rounded(theme::RadiusSize::Md)
    .background(background)
    .border_inside(1.0, border)
    .cursor(CursorIcon::Pointer)
    .hovered_style(Style::new().background(hover_background.clone()))
    .active_style(Style::new().background(hover_background))
    .child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon,
      size: 16.0,
      color: icon_color,
    }))
    .child(
      Text::new(label)
        .variant(theme::TypographyStyle::Button)
        .color(text_color),
    )
}
