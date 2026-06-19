mod server_card;

use std::time::Duration;

use lurq::{
  app::{component::Component, ctx::Ctx, theme::Breakpoint},
  components::{Column, Row, ScrollVertical, Text},
  core::{Signal, Store},
  layout::{
    Alignment,
    layout_kind::Justify,
    scrollbar::{ScrollBarPlacement, ScrollBarStyle},
  },
  node::{BackgroundColor, CursorIcon, Element, Style, border::Border, color::Color, dimension::Dimension},
};
use server_card::{ServerCard, ServerCardProps, ServerCardState};

use crate::{
  network::server_query::{ServerQueryInfo, query_server},
  routes::{ROUTE_CONNECT_SERVER, ROUTE_LOBBY, ROUTE_TOFU_WARNING},
  session::{ConnectedServerInfo, ServerSession},
  storage::{AppSettings, Storage, StoredServer},
  theme,
  ui::{
    common::lucide_icon::{LucideIcon, LucideIconProps},
    connect_server::{ConnectErrorCopy, ConnectOrigin, connect_and_store, resolve_address, with_default_port},
    settings::SettingsPopupHandle,
  },
};

const SERVER_LIST_GAP: f32 = 14.0;
const SERVER_LIST_QUERY_TIMEOUT: Duration = Duration::from_millis(800);

#[derive(Clone, Copy)]
struct ServersLayoutMetrics {
  content_max_width: f32,
  page_padding_y: f32,
  top_bar_padding_x: f32,
  empty_copy_width: f32,
}

fn servers_layout_metrics(ctx: &Ctx) -> ServersLayoutMetrics {
  match ctx.breakpoint() {
    Some(Breakpoint::Md) => ServersLayoutMetrics {
      content_max_width: 704.0,
      page_padding_y: 22.0,
      top_bar_padding_x: 20.0,
      empty_copy_width: 440.0,
    },
    Some(Breakpoint::Lg) => ServersLayoutMetrics {
      content_max_width: 820.0,
      page_padding_y: 26.0,
      top_bar_padding_x: 20.0,
      empty_copy_width: 480.0,
    },
    Some(Breakpoint::Xl) | Some(Breakpoint::Sm) | None => ServersLayoutMetrics {
      content_max_width: 860.0,
      page_padding_y: 28.0,
      top_bar_padding_x: 20.0,
      empty_copy_width: 480.0,
    },
  }
}

pub struct SavedServersScreen {
  connecting: Signal<Option<String>>,
  running: Signal<Option<String>>,
  failed: Signal<Option<String>>,
  failure_message: Signal<Option<String>>,
  query_signature: Signal<String>,
  query_results: Store<Vec<ServerQueryEntry>>,
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
      query_signature: ctx.signal(String::new()),
      query_results: ctx.store(Vec::new()),
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let storage = ctx.use_context::<Storage>();
    let session = ctx.use_context::<ServerSession>();
    let settings_store = ctx.use_context::<Store<AppSettings>>();
    let servers = ctx
      .use_context::<Storage>()
      .and_then(|storage| storage.load_servers().ok())
      .unwrap_or_default();
    let connect_errors = ConnectErrorCopy::from_ctx(ctx);
    let connect = ctx.future_action(move |server: StoredServer| {
      let storage = storage.clone();
      let session = session.clone();
      let connect_errors = connect_errors.clone();
      let settings_store = settings_store.clone();
      async move {
        let display_name = if server.display_name.trim().is_empty() {
          settings_store
            .as_ref()
            .map(|settings| settings.with(|settings| settings.display_name.clone()))
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
          connect_errors,
        )
        .await
      }
    });
    let query_resolve_failed = ctx.t("connect_server.error.resolve_failed").to_string();
    let query_servers = ctx.future_action(move |servers: Vec<StoredServer>| {
      let query_resolve_failed = query_resolve_failed.clone();
      async move { query_saved_servers(servers, query_resolve_failed).await }
    });
    self.sync_query_state(&servers, &query_servers);
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
type QueryAction = lurq::app::ctx::FutureAction<Vec<StoredServer>, Vec<ServerQueryEntry>, String>;

#[derive(Debug, Clone, PartialEq, Eq, lurq::DevtoolsInspectable)]
pub struct ServerQueryEntry {
  address: String,
  state: ServerQueryState,
}

#[derive(Debug, Clone, PartialEq, Eq, lurq::DevtoolsInspectable)]
pub enum ServerQueryState {
  Online(ServerQueryInfo),
  NoResponse,
}

impl SavedServersScreen {
  fn sync_query_state(&self, servers: &[StoredServer], query_servers: &QueryAction) {
    let signature = server_query_signature(servers);
    let state = query_servers.state().get();

    if self.query_signature.get_untracked() != signature && !state.is_pending() {
      self.query_signature.set(signature);
      self.query_results.set(Vec::new());
      query_servers.run(servers.to_vec());
      return;
    }

    if let Some(data) = state.data
      && self.query_results.with(|current| current != &data)
    {
      self.query_results.set(data);
    }
  }

  fn sync_connection_state(&self, ctx: &mut Ctx, servers: &[StoredServer], connect: &ConnectAction) {
    let state = connect.state().get();

    if let Some(next_address) = self.connecting.get_untracked()
      && let Some(running_address) = self.running.get_untracked()
      && running_address != next_address
    {
      connect.cancel();
      self.running.set(None);
      self.failed.set(None);
      self.failure_message.set(None);
    }

    if let Some(address) = self.running.get_untracked() {
      if state.is_fulfilled() {
        self.running.set(None);
        self.connecting.set(None);
        self.failed.set(None);
        self.failure_message.set(None);
        if let Some(navigator) = ctx.navigator() {
          if ctx
            .use_context::<ServerSession>()
            .as_ref()
            .and_then(ServerSession::tofu_warning)
            .is_some()
          {
            navigator.replace(ROUTE_TOFU_WARNING);
          } else {
            navigator.replace(ROUTE_LOBBY);
          }
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
    let metrics = servers_layout_metrics(ctx);

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
          .width(Dimension::Pct(100.0))
          .max_width(metrics.empty_copy_width)
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
    let metrics = servers_layout_metrics(ctx);
    let count = servers.len();
    let connecting = self.connecting.get();
    let failed = self.failed.get();
    let failure_message = self.failure_message.get();
    let connecting_signal = self.connecting.clone();
    let failed_signal = self.failed.clone();
    let query_results = self.query_results.get();
    let querying = self
      .query_signature
      .get()
      .split('\n')
      .filter(|address| !address.is_empty())
      .count()
      > query_results.len();

    let mut list = Column::new().width(Dimension::Pct(100.0)).spacing(SERVER_LIST_GAP);

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
          live: server_live_info(query_result_for(&query_results, &address), querying),
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

    ScrollVertical::new(
      Column::new()
        .width(Dimension::Pct(100.0))
        .align_items(Alignment::Center)
        .padding_vertical(metrics.page_padding_y)
        .child(
          Column::new()
            .width(Dimension::Pct(100.0))
            .max_width(metrics.content_max_width)
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
        ),
    )
    .width(Dimension::Pct(100.0))
    .flex(1.0)
    .scrollbar(server_list_scrollbar_style())
    .scrollbar_hovered(|mut style| {
      let palette = theme::palette();
      style.thumb_color = palette.accent_hover;
      style.track_color = palette.surface_input.with_opacity(0.75);
      style
    })
  }
}

async fn query_saved_servers(
  servers: Vec<StoredServer>,
  resolve_failed: String,
) -> Result<Vec<ServerQueryEntry>, String> {
  let mut tasks = Vec::with_capacity(servers.len());
  for server in servers {
    tasks.push(tokio::spawn(query_saved_server(server, resolve_failed.clone())));
  }

  let mut results = Vec::new();
  for task in tasks {
    results.push(task.await.map_err(|error| error.to_string())?);
  }
  Ok(results)
}

async fn query_saved_server(server: StoredServer, resolve_failed: String) -> ServerQueryEntry {
  let address = server.address.clone();
  let socket = match resolve_address(with_default_port(&server.address), resolve_failed).await {
    Ok(socket) => socket,
    Err(_) => {
      return ServerQueryEntry {
        address,
        state: ServerQueryState::NoResponse,
      };
    }
  };
  let info = query_server(socket, SERVER_LIST_QUERY_TIMEOUT).await.ok().flatten();
  let state = match info {
    Some(info) => ServerQueryState::Online(info),
    None => ServerQueryState::NoResponse,
  };
  ServerQueryEntry { address, state }
}

fn server_query_signature(servers: &[StoredServer]) -> String {
  servers
    .iter()
    .map(|server| server.address.as_str())
    .collect::<Vec<_>>()
    .join("\n")
}

fn query_result_for<'a>(results: &'a [ServerQueryEntry], address: &str) -> Option<&'a ServerQueryState> {
  results
    .iter()
    .find(|entry| entry.address == address)
    .map(|entry| &entry.state)
}

fn server_live_info(state: Option<&ServerQueryState>, querying: bool) -> server_card::ServerCardLiveInfo {
  match state {
    Some(ServerQueryState::Online(info)) => server_card::ServerCardLiveInfo {
      state: server_card::ServerCardLiveState::Online,
      server_name: Some(info.server_name.clone()),
      current_users: Some(info.current_users),
      max_users: Some(info.max_users),
      protocol_version: Some(info.protocol_version),
      password_locked: info.password_locked,
    },
    Some(ServerQueryState::NoResponse) => server_card::ServerCardLiveInfo {
      state: server_card::ServerCardLiveState::NoResponse,
      ..server_card::ServerCardLiveInfo::default()
    },
    None if querying => server_card::ServerCardLiveInfo {
      state: server_card::ServerCardLiveState::Checking,
      ..server_card::ServerCardLiveInfo::default()
    },
    None => server_card::ServerCardLiveInfo::default(),
  }
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
  let metrics = servers_layout_metrics(ctx);
  let settings_popup = ctx.use_context::<SettingsPopupHandle>();
  let identity_name = ctx
    .use_context::<Store<AppSettings>>()
    .map(|settings| settings.with(|settings| settings.display_name.trim().to_owned()))
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

  if let Some(settings_popup) = settings_popup {
    settings_button = settings_button.on_click(move |_| settings_popup.open());
  }

  Row::new()
    .width(Dimension::Pct(100.0))
    .height(56.0)
    .align_items(Alignment::Center)
    .justify(Justify::SpaceBetween)
    .padding_horizontal(metrics.top_bar_padding_x)
    .background(BackgroundColor::Color(Color::from_hex("#0D0E10")))
    .border_bottom(Border::inside(1.0, theme::PaletteColor::Border))
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

#[cfg(test)]
#[path = "../../tests/unit/ui/servers.rs"]
mod tests;
