mod server_card;

use lurq::{
  app::{component::Component, ctx::Ctx},
  components::{Column, Row, ScrollVertical, Text},
  core::Signal,
  layout::{
    Alignment,
    layout_kind::Justify,
    scrollbar::{ScrollBarPlacement, ScrollBarStyle},
  },
  node::{BackgroundColor, CursorIcon, Element, Style, color::Color, dimension::Dimension},
};
use server_card::{ServerCard, ServerCardProps, ServerCardState};

use crate::{
  routes::{ROUTE_CONNECT_SERVER, ROUTE_LOBBY, ROUTE_SETTINGS},
  session::{ConnectedServerInfo, ServerSession},
  storage::{Storage, StoredServer},
  theme,
  ui::{
    common::lucide_icon::{LucideIcon, LucideIconProps},
    connect_server::{ConnectOrigin, connect_and_store},
  },
};

const VISIBLE_SERVER_COUNT: usize = 5;
const SERVER_LIST_MAX_HEIGHT: f32 = 396.0;

pub struct SavedServersScreen {
  connecting: Signal<Option<String>>,
  running: Signal<Option<String>>,
  failed: Signal<Option<String>>,
  failure_message: Signal<Option<String>>,
}

impl Component for SavedServersScreen {
  type Props = ();

  fn create(ctx: &mut Ctx) -> Self {
    let connecting = ctx.signal(None::<String>);
    let failed = ctx.signal(None::<String>);

    Self {
      connecting,
      running: ctx.signal(None::<String>),
      failed,
      failure_message: ctx.signal(None::<String>),
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let storage = ctx.use_context::<Storage>();
    let session = ctx.use_context::<ServerSession>();
    let servers = ctx
      .use_context::<Storage>()
      .and_then(|storage| storage.load_servers().ok())
      .unwrap_or_default();
    let connect = ctx.future_action(move |server: StoredServer| {
      let storage = storage.clone();
      let session = session.clone();
      async move {
        let display_name = if server.display_name.trim().is_empty() {
          storage
            .as_ref()
            .and_then(|storage| storage.load_settings().ok())
            .map(|settings| settings.display_name)
            .unwrap_or_default()
        } else {
          server.display_name.clone()
        };
        connect_and_store(
          server.address.clone(),
          server.server_password.clone(),
          display_name,
          storage,
          session,
        )
        .await
      }
    });
    self.sync_connection_state(ctx, &servers, &connect);

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

type ConnectAction = lurq::app::ctx::FutureAction<StoredServer, ConnectedServerInfo, String>;

impl SavedServersScreen {
  fn sync_connection_state(&self, ctx: &mut Ctx, servers: &[StoredServer], connect: &ConnectAction) {
    let state = connect.state().get();

    if let Some(address) = self.running.get_untracked() {
      if state.is_fulfilled() {
        self.running.set(None);
        self.connecting.set(None);
        self.failed.set(None);
        self.failure_message.set(None);
        if let Some(navigator) = ctx.navigator() {
          navigator.replace(ROUTE_LOBBY);
        }
      } else if state.is_rejected() {
        self.running.set(None);
        self.connecting.set(None);
        self.failed.set(Some(address));
        self.failure_message.set(state.error.clone());
      }
    }

    let Some(address) = self.connecting.get() else {
      return;
    };
    if self.running.get_untracked().is_some() {
      return;
    }
    let Some(server) = servers.iter().find(|server| server.address == address).cloned() else {
      self.connecting.set(None);
      return;
    };

    self.failed.set(None);
    self.failure_message.set(None);
    self.running.set(Some(address));
    connect.run(server);
  }

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
    let failure_message = self.failure_message.get();
    let connecting_signal = self.connecting.clone();
    let failed_signal = self.failed.clone();

    let mut list = Column::new()
      .width(Dimension::Pct(100.0))
      .spacing(theme::SpacingSize::Lg)
      .padding_right(16.0);

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
          error_message: if failed.as_deref() == Some(address.as_str()) {
            failure_message.clone()
          } else {
            None
          },
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
          .child(server_list_view(list, count))
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

fn server_list_view(list: Column, count: usize) -> Element {
  if count <= VISIBLE_SERVER_COUNT {
    return list.into();
  }

  ScrollVertical::new(list)
    .width(Dimension::Pct(100.0))
    .height(SERVER_LIST_MAX_HEIGHT)
    .scrollbar(server_list_scrollbar_style())
    .scrollbar_hovered(|mut style| {
      let palette = theme::palette();
      style.thumb_color = palette.accent_hover;
      style.track_color = palette.surface_input.with_opacity(0.75);
      style
    })
    .into()
}

fn server_list_scrollbar_style() -> ScrollBarStyle {
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

fn top_bar(ctx: &mut Ctx) -> impl Into<Element> {
  let navigator = ctx.navigator();
  let identity_name = ctx
    .use_context::<Storage>()
    .and_then(|storage| storage.load_settings().ok())
    .map(|settings| settings.display_name.trim().to_owned())
    .filter(|name| !name.is_empty())
    .unwrap_or_else(|| ctx.t("servers.user.name").to_string());
  let identity_initials = initials_for(&identity_name, &ctx.t("servers.user.initials"));
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
              Text::new(&identity_initials)
                .variant(theme::TypographyStyle::Mono)
                .color(theme::PaletteColor::TextSecondary),
            ),
        )
        .child(
          Text::new(&identity_name)
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
      BackgroundColor::Palette(theme::PaletteColor::Accent),
      theme::PaletteColor::TextPrimary,
      theme::palette().accent,
      BackgroundColor::Palette(theme::PaletteColor::AccentMuted),
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
