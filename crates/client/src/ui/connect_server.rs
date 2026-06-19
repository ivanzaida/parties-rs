use std::{
  net::SocketAddr,
  sync::Arc,
  time::{Duration, SystemTime, UNIX_EPOCH},
};

use lurq::{
  app::{component::Component, ctx::Ctx},
  components::{Button, Column, Form, FormProps, Row, Text, TextInput},
  core::{Signal, Store},
  layout::{Alignment, layout_kind::Justify},
  node::{BackgroundColor, CursorIcon, Element, Style, color::Color, dimension::Dimension},
};

use crate::{
  identity::{LocalIdentity, auth_identity},
  network::{
    protocol::{DEFAULT_PORT, S2C},
    server::Server,
    server_query::query_server,
  },
  routes::{ROUTE_CHOOSE_SERVER, ROUTE_LOBBY, ROUTE_SETTINGS_SERVERS, ROUTE_TOFU_WARNING},
  session::{ConnectedServer, ConnectedServerInfo, ServerSession, TofuWarning},
  storage::{
    AppDisplayName, Storage, StoredServer, UserAudioPreferences, server_user_audio_preferences,
    stored_server_by_address, upsert_stored_server,
  },
  theme,
  ui::{
    common::lucide_icon::{LucideIcon, LucideIconProps},
    loader::loader,
    settings::{SettingsPage, SettingsPopupHandle},
  },
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ConnectOrigin {
  ServerList,
  Settings,
  SettingsPopup,
}

#[derive(Clone, PartialEq, Eq)]
pub struct ConnectServerRouteState {
  pub origin: ConnectOrigin,
}

impl ConnectServerRouteState {
  pub fn new(origin: ConnectOrigin) -> Self {
    Self { origin }
  }
}

#[derive(Clone)]
pub struct ConnectErrorCopy {
  storage_unavailable: String,
  identity_missing: String,
  timeout: String,
  unexpected_response: String,
  resolve_failed: String,
}

impl ConnectErrorCopy {
  pub fn from_ctx(ctx: &mut Ctx) -> Self {
    Self {
      storage_unavailable: ctx.t("connect_server.error.storage_unavailable").to_string(),
      identity_missing: ctx.t("connect_server.error.identity_missing").to_string(),
      timeout: ctx
        .t_args(
          "connect_server.error.timeout",
          [("seconds", CONNECT_TIMEOUT.as_secs().to_string())],
        )
        .to_string(),
      unexpected_response: ctx.t("connect_server.error.unexpected_response").to_string(),
      resolve_failed: ctx.t("connect_server.error.resolve_failed").to_string(),
    }
  }
}

pub struct ConnectServerScreen {
  address: Signal<String>,
  seed: Signal<String>,
  display_name: Signal<String>,
  navigated: Signal<bool>,
}

impl Component for ConnectServerScreen {
  type Props = ();

  fn create(ctx: &mut Ctx) -> Self {
    let display_name = ctx
      .use_context::<Store<AppDisplayName>>()
      .map(|display_name| display_name.with(|display_name| display_name.value.clone()))
      .unwrap_or_default();

    Self {
      address: ctx.signal(String::new()),
      seed: ctx.signal(String::new()),
      display_name: ctx.signal(display_name),
      navigated: ctx.signal(false),
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let storage = ctx.use_context::<Storage>();
    let session = ctx.use_context::<ServerSession>();

    let route_state = ctx.route_state::<ConnectServerRouteState>().as_deref().cloned();
    let origin = route_state
      .as_ref()
      .map(|state| state.origin)
      .or_else(|| ctx.route_state::<ConnectOrigin>().as_deref().copied())
      .unwrap_or(ConnectOrigin::ServerList);
    let from_settings = matches!(origin, ConnectOrigin::Settings | ConnectOrigin::SettingsPopup);
    let settings_popup = matches!(origin, ConnectOrigin::SettingsPopup)
      .then(|| ctx.use_context::<SettingsPopupHandle>())
      .flatten();
    let back_route = match origin {
      ConnectOrigin::Settings => ROUTE_SETTINGS_SERVERS,
      ConnectOrigin::SettingsPopup => {
        if session.as_ref().and_then(ServerSession::info).is_some() {
          ROUTE_LOBBY
        } else {
          ROUTE_CHOOSE_SERVER
        }
      }
      ConnectOrigin::ServerList => ROUTE_CHOOSE_SERVER,
    };
    let back_label = if from_settings {
      ctx.t("connect_server.back_to_settings")
    } else {
      ctx.t("connect_server.back_to_list")
    };

    let errors = ConnectErrorCopy::from_ctx(ctx);
    let identity_store = ctx.use_context::<Store<Option<LocalIdentity>>>();
    let user_audio_preferences = ctx.use_context::<Store<UserAudioPreferences>>();
    let servers_store = ctx.use_context::<Store<Vec<StoredServer>>>();
    let route_session = session.clone();
    let connect = ctx.future_action(move |(address, seed, display_name): (String, String, String)| {
      let storage = storage.clone();
      let identity_store = identity_store.clone();
      let user_audio_preferences = user_audio_preferences.clone();
      let servers_store = servers_store.clone();
      let session = session.clone();
      let errors = errors.clone();
      async move {
        connect_and_store(
          address,
          seed,
          display_name,
          storage,
          identity_store,
          user_audio_preferences,
          servers_store,
          session,
          errors,
        )
        .await
      }
    });

    let state = connect.state().get();
    let connecting = state.is_pending();
    let error = state.error.clone();

    if state.data.is_some() && !self.navigated.get_untracked() {
      self.navigated.set(true);
      if let Some(navigator) = ctx.navigator() {
        if route_session.as_ref().and_then(ServerSession::tofu_warning).is_some() {
          navigator.replace(ROUTE_TOFU_WARNING);
        } else {
          navigator.replace(ROUTE_LOBBY);
        }
      }
    }

    let address_value = self.address.get();
    let display_name_value = self.display_name.get();
    let can_connect = !address_value.trim().is_empty() && !display_name_value.trim().is_empty() && !connecting;

    let mut card = Column::new()
      .width(Dimension::Pct(100.0))
      .spacing(theme::SpacingSize::Xl)
      .padding(28.0)
      .rounded(10.0)
      .background(BackgroundColor::Palette(theme::PaletteColor::SurfacePanel))
      .border_inside(1.0, theme::PaletteColor::Border)
      .child(header(ctx))
      .child(self.address_field(ctx))
      .child(self.display_name_field(ctx))
      .child(self.seed_field(ctx));

    if connecting {
      card = card.child(authenticating(ctx, &address_value));
    } else if let Some(error) = error.as_ref() {
      card = card.child(error_banner(ctx, error));
    }

    card = card.child(self.actions(
      ctx,
      &connect,
      connecting,
      can_connect,
      back_route,
      settings_popup.clone(),
    ));

    let submit_action = connect.clone();
    let card = Form::element(
      FormProps::default().on_submit_data(move |data| {
        if can_connect {
          submit_action.run((
            data.get("address").unwrap_or_default().to_owned(),
            data.get("seed").unwrap_or_default().to_owned(),
            data.get("display_name").unwrap_or_default().to_owned(),
          ));
        }
      }),
      card,
    );

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
          .width(600.0)
          .spacing(16.0)
          .child(back_row(ctx, &back_label, back_route, settings_popup))
          .child(card),
      )
  }
}

type ConnectAction = lurq::app::ctx::FutureAction<(String, String, String), ConnectedServerInfo, String>;

impl ConnectServerScreen {
  fn address_field(&self, ctx: &mut Ctx) -> impl Into<Element> {
    Column::new()
      .width(Dimension::Pct(100.0))
      .spacing(8.0)
      .child(
        Text::new(&ctx.t("connect_server.address.label"))
          .variant(theme::TypographyStyle::FieldLabel)
          .color(theme::PaletteColor::TextMuted),
      )
      .child(input_box(
        ctx,
        self.address.clone(),
        &ctx.t("connect_server.address.placeholder"),
        "globe",
        "address",
        1,
      ))
      .child(
        Text::new(&ctx.t("connect_server.address.hint"))
          .variant(theme::TypographyStyle::Link)
          .color(theme::PaletteColor::TextMuted),
      )
  }

  fn seed_field(&self, ctx: &mut Ctx) -> impl Into<Element> {
    Column::new()
      .width(Dimension::Pct(100.0))
      .spacing(8.0)
      .child(
        Row::new()
          .align_items(Alignment::Center)
          .spacing(theme::SpacingSize::Sm)
          .child(
            Text::new(&ctx.t("connect_server.seed.label"))
              .variant(theme::TypographyStyle::FieldLabel)
              .color(theme::PaletteColor::TextMuted),
          )
          .child(
            Text::new(&ctx.t("connect_server.seed.optional"))
              .variant(theme::TypographyStyle::FieldLabel)
              .color(theme::PaletteColor::TextMuted),
          ),
      )
      .child(input_box(
        ctx,
        self.seed.clone(),
        &ctx.t("connect_server.seed.placeholder"),
        "eye",
        "seed",
        3,
      ))
      .child(
        Text::new(&ctx.t("connect_server.seed.hint"))
          .variant(theme::TypographyStyle::Link)
          .color(theme::PaletteColor::TextMuted),
      )
  }

  fn display_name_field(&self, ctx: &mut Ctx) -> impl Into<Element> {
    Column::new()
      .width(Dimension::Pct(100.0))
      .spacing(8.0)
      .child(
        Text::new(&ctx.t("connect_server.display_name.label"))
          .variant(theme::TypographyStyle::FieldLabel)
          .color(theme::PaletteColor::TextMuted),
      )
      .child(input_box(
        ctx,
        self.display_name.clone(),
        &ctx.t("connect_server.display_name.placeholder"),
        "user",
        "display_name",
        2,
      ))
      .child(
        Text::new(&ctx.t("connect_server.display_name.hint"))
          .variant(theme::TypographyStyle::Link)
          .color(theme::PaletteColor::TextMuted),
      )
  }

  fn actions(
    &self,
    ctx: &mut Ctx,
    connect: &ConnectAction,
    connecting: bool,
    can_connect: bool,
    back_route: &'static str,
    settings_popup: Option<SettingsPopupHandle>,
  ) -> impl Into<Element> {
    let navigator = ctx.navigator();
    let cancel_action = connect.clone();
    let cancel_label = ctx.t("connect_server.action.cancel");
    let connect_label = ctx.t("connect_server.action.connect");

    let cancel = ghost_button(&cancel_label).on_click(move |_| {
      cancel_action.cancel();
      if let Some(settings_popup) = settings_popup.as_ref() {
        settings_popup.open_page(SettingsPage::Servers);
      }
      if let Some(navigator) = navigator.as_ref() {
        navigator.replace(back_route);
      }
    });

    let connect_child: Element = if connecting {
      connecting_button(ctx).into()
    } else {
      connect_button(&connect_label, can_connect).into()
    };

    Row::new()
      .width(Dimension::Pct(100.0))
      .align_items(Alignment::Center)
      .justify(Justify::SpaceBetween)
      .child(cancel)
      .child(connect_child)
  }
}

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const SERVER_QUERY_TIMEOUT: Duration = Duration::from_millis(800);

pub async fn connect_and_store(
  address: String,
  seed: String,
  display_name: String,
  storage: Option<Storage>,
  identity_store: Option<Store<Option<LocalIdentity>>>,
  user_audio_preferences: Option<Store<UserAudioPreferences>>,
  servers_store: Option<Store<Vec<StoredServer>>>,
  session: Option<ServerSession>,
  errors: ConnectErrorCopy,
) -> Result<ConnectedServerInfo, String> {
  let address = with_default_port(&address);
  let display_name = display_name.trim().to_owned();
  tracing::debug!(target: "network::connect",
    "[network/connect] connecting to server: address={} display='{}'",
    address,
    display_name
  );

  let identity = identity_from_store(identity_store.as_ref(), &errors)?;

  let connect_result = tokio::time::timeout(CONNECT_TIMEOUT, async {
    let socket = resolve_address(address.clone(), errors.resolve_failed.clone()).await?;
    tracing::debug!(target: "network::connect", "[network/connect] resolved server address: address={address} socket={socket}");
    let query = query_server(socket, SERVER_QUERY_TIMEOUT).await.unwrap_or(None);
    tracing::debug!(target: "network::connect",
      "[network/connect] query result: address={} responded={}",
      address,
      query.is_some()
    );
    let server = Server::connect(socket).await.map_err(|error| error.to_string())?;
    let fingerprint = server.certificate_fingerprint().unwrap_or_default();
    tracing::debug!(target: "network::connect",
      "[network/connect] transport connected: address={} certificate_fingerprint={}",
      address,
      fingerprint
    );

    let timestamp = SystemTime::now()
      .duration_since(UNIX_EPOCH)
      .map_err(|error| error.to_string())?
      .as_secs();
    let auth = auth_identity(&identity, &display_name, timestamp, seed.clone()).map_err(|error| error.to_string())?;
    let response = authenticate_with_query(&server, auth, &errors).await?;
    tracing::debug!(target: "network::connect",
      "[network/connect] authenticated: server='{}' user={} role={:?}",
      response.server_name,
      response.user_id,
      response.role
    );

    Ok::<_, String>((server, fingerprint, response))
  })
  .await;
  let (server, fingerprint, response) = match connect_result {
    Ok(Ok(result)) => result,
    Ok(Err(error)) => {
      tracing::debug!(target: "network::connect", "[network/connect] connection attempt failed: address={address} error={error}");
      return Err(error);
    }
    Err(_) => {
      tracing::debug!(
        target: "network::connect",
        "[network/connect] connection attempt timed out: address={} timeout_seconds={}",
        address,
        CONNECT_TIMEOUT.as_secs()
      );
      return Err(errors.timeout.clone());
    }
  };

  let info = ConnectedServerInfo {
    address: address.clone(),
    server_name: response.server_name.clone(),
    display_name: display_name.clone(),
    user_id: response.user_id,
    role: response.role,
    certificate_fingerprint: fingerprint.clone(),
  };
  let audio_preferences =
    server_user_audio_preferences(user_audio_preferences.as_ref(), storage.as_ref(), &info.address);
  let saved_server = servers_store
    .as_ref()
    .and_then(|servers| stored_server_by_address(&servers.get(), &info.address));
  let saved_fingerprint = saved_server
    .as_ref()
    .map(|server| server.certificate_fingerprint.clone())
    .unwrap_or_default();
  let tofu_warning = certificate_fingerprint_changed(&saved_fingerprint, &fingerprint).then(|| TofuWarning {
    address: info.address.clone(),
    server_name: info.server_name.clone(),
    user_id: info.user_id,
    role: info.role,
    saved_fingerprint: saved_fingerprint.clone(),
    received_fingerprint: fingerprint.clone(),
    server_password: seed.clone(),
    display_name: info.display_name.clone(),
  });

  if let Some(storage) = storage.as_ref()
    && tofu_warning.is_none()
  {
    upsert_stored_server(
      servers_store.as_ref(),
      Some(storage),
      StoredServer {
        address,
        server_name: response.server_name,
        user_id: response.user_id,
        role: response.role,
        certificate_fingerprint: fingerprint,
        server_password: seed,
        display_name,
      },
    )
    .map_err(|error| error.to_string())?;
    tracing::debug!(target: "network::connect",
      "[network/connect] saved server credentials metadata: address={} server='{}' user={}",
      info.address,
      info.server_name,
      info.user_id
    );
  } else if tofu_warning.is_some() {
    tracing::warn!(
      target: "network::connect",
      "[network/connect] server certificate fingerprint changed: address={} saved={} received={}",
      info.address,
      saved_fingerprint,
      fingerprint
    );
  }

  if let Some(session) = session {
    session.set_connected(ConnectedServer {
      info: info.clone(),
      server: Arc::new(server),
    });
    for (user_id, volume) in audio_preferences.voice_volumes {
      session.set_user_volume(user_id, volume);
    }
    for (user_id, volume) in audio_preferences.stream_volumes {
      session.set_stream_volume(user_id, volume);
    }
    for user_id in audio_preferences.normalized_users {
      session.set_user_normalization(user_id, true);
    }
    if let Some(warning) = tofu_warning {
      session.set_tofu_warning(warning);
    } else {
      session.clear_tofu_warning();
    }
  }

  tracing::debug!(target: "network::connect",
    "[network/connect] server ready: address={} server='{}' local_user={}",
    info.address,
    info.server_name,
    info.user_id
  );
  Ok(info)
}

fn certificate_fingerprint_changed(saved: &str, received: &str) -> bool {
  let saved = saved.trim();
  let received = received.trim();
  !saved.is_empty() && !received.is_empty() && !saved.eq_ignore_ascii_case(received)
}

#[allow(dead_code)]
pub async fn test_connection(
  address: String,
  seed: String,
  display_name: String,
  identity_store: Option<Store<Option<LocalIdentity>>>,
  errors: ConnectErrorCopy,
) -> Result<ConnectedServerInfo, String> {
  let address = with_default_port(&address);
  let display_name = display_name.trim().to_owned();

  let identity = identity_from_store(identity_store.as_ref(), &errors)?;

  let (server, fingerprint, response) = tokio::time::timeout(CONNECT_TIMEOUT, async {
    let socket = resolve_address(address.clone(), errors.resolve_failed.clone()).await?;
    let server = Server::connect(socket).await.map_err(|error| error.to_string())?;
    let fingerprint = server.certificate_fingerprint().unwrap_or_default();

    let timestamp = SystemTime::now()
      .duration_since(UNIX_EPOCH)
      .map_err(|error| error.to_string())?
      .as_secs();
    let auth = auth_identity(&identity, &display_name, timestamp, seed.clone()).map_err(|error| error.to_string())?;
    let response = authenticate_with_query(&server, auth, &errors).await?;

    Ok::<_, String>((server, fingerprint, response))
  })
  .await
  .map_err(|_| errors.timeout.clone())??;

  server.disconnect();

  Ok(ConnectedServerInfo {
    address,
    server_name: response.server_name,
    display_name,
    user_id: response.user_id,
    role: response.role,
    certificate_fingerprint: fingerprint,
  })
}

fn identity_from_store(
  identity_store: Option<&Store<Option<LocalIdentity>>>,
  errors: &ConnectErrorCopy,
) -> Result<LocalIdentity, String> {
  identity_store
    .ok_or_else(|| errors.storage_unavailable.clone())?
    .get()
    .ok_or_else(|| errors.identity_missing.clone())
}

enum AuthAttempt {
  Authenticated(crate::network::protocol::AuthResponse),
}

async fn authenticate_with_query(
  server: &Server,
  auth: crate::network::protocol::AuthIdentity,
  errors: &ConnectErrorCopy,
) -> Result<crate::network::protocol::AuthResponse, String> {
  server
    .authenticate(auth.clone())
    .await
    .map_err(|error| error.to_string())?;

  match recv_auth_response(server, errors).await? {
    AuthAttempt::Authenticated(response) => Ok(response),
  }
}

async fn recv_auth_response(server: &Server, errors: &ConnectErrorCopy) -> Result<AuthAttempt, String> {
  match server.recv().await.map_err(|error| error.to_string())? {
    S2C::AuthResponse(response) => Ok(AuthAttempt::Authenticated(response)),
    S2C::ServerError { message, .. } => Err(message),
    _ => Err(errors.unexpected_response.clone()),
  }
}

pub(crate) fn with_default_port(address: &str) -> String {
  let address = address.trim();

  if let Some(rest) = address.strip_prefix('[') {
    return match rest.find(']') {
      Some(end) if rest[end + 1..].starts_with(':') => address.to_owned(),
      _ => format!("{address}:{DEFAULT_PORT}"),
    };
  }

  if address.contains(':') {
    address.to_owned()
  } else {
    format!("{address}:{DEFAULT_PORT}")
  }
}

pub(crate) async fn resolve_address(address: String, resolve_failed: String) -> Result<SocketAddr, String> {
  tokio::task::spawn_blocking(move || {
    use std::net::ToSocketAddrs;
    address
      .to_socket_addrs()
      .map_err(|error| error.to_string())?
      .next()
      .ok_or(resolve_failed)
  })
  .await
  .map_err(|error| error.to_string())?
}

fn header(ctx: &mut Ctx) -> impl Into<Element> {
  Column::new()
    .width(Dimension::Pct(100.0))
    .spacing(theme::SpacingSize::Sm)
    .child(
      Text::new(&ctx.t("connect_server.overline"))
        .variant(theme::TypographyStyle::Caption)
        .color(theme::PaletteColor::TextMuted),
    )
    .child(Text::new(&ctx.t("connect_server.title")).variant(theme::TypographyStyle::Title))
}

fn back_row(
  ctx: &mut Ctx,
  label: &str,
  home_route: &'static str,
  settings_popup: Option<SettingsPopupHandle>,
) -> impl Into<Element> {
  let navigator = ctx.navigator();
  let mut row = Row::new()
    .align_items(Alignment::Center)
    .spacing(theme::SpacingSize::Sm)
    .cursor(CursorIcon::Pointer)
    .child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon: "arrow-left",
      size: 16.0,
      color: theme::palette().text_muted,
    }))
    .child(
      Text::new(label)
        .variant(theme::TypographyStyle::Description)
        .color(theme::PaletteColor::TextMuted),
    );

  if let Some(navigator) = navigator {
    row = row.on_click(move |_| {
      if let Some(settings_popup) = settings_popup.as_ref() {
        settings_popup.open_page(SettingsPage::Servers);
      }
      navigator.replace(home_route);
    });
  }

  row
}

fn authenticating(ctx: &mut Ctx, address: &str) -> impl Into<Element> {
  let host = address.split(':').next().unwrap_or(address).to_owned();

  Row::new()
    .width(Dimension::Pct(100.0))
    .align_items(Alignment::Center)
    .spacing(theme::SpacingSize::Md)
    .padding_vertical(theme::SpacingSize::Lg)
    .padding_horizontal(16.0)
    .rounded(theme::RadiusSize::Lg)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceInput))
    .border_inside(1.0, theme::PaletteColor::Border)
    .child(loader(16.0))
    .child(
      Column::new().flex(1.0).child(
        Text::new(&ctx.t_args("connect_server.authenticating", [("host", host)]))
          .variant(theme::TypographyStyle::Description)
          .color(theme::PaletteColor::TextSecondary)
          .width(Dimension::Pct(100.0)),
      ),
    )
}

fn error_banner(ctx: &mut Ctx, message: &str) -> impl Into<Element> {
  Row::new()
    .width(Dimension::Pct(100.0))
    .align_items(Alignment::Center)
    .spacing(theme::SpacingSize::Md)
    .padding_vertical(theme::SpacingSize::Lg)
    .padding_horizontal(16.0)
    .rounded(theme::RadiusSize::Lg)
    .background(BackgroundColor::Palette(theme::PaletteColor::DangerMuted))
    .border_inside(1.0, theme::PaletteColor::Danger)
    .child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon: "triangle-alert",
      size: 16.0,
      color: theme::palette().danger,
    }))
    .child(
      Column::new().flex(1.0).child(
        Text::new(message)
          .variant(theme::TypographyStyle::Description)
          .color(theme::PaletteColor::Danger)
          .width(Dimension::Pct(100.0)),
      ),
    )
}

fn input_box(
  ctx: &mut Ctx,
  value: Signal<String>,
  placeholder: &str,
  icon: &'static str,
  name: &'static str,
  tab_index: i32,
) -> Row {
  let text_style = ctx.theme().typography().mono.clone();
  let mut placeholder_style = text_style.clone();
  placeholder_style.color = theme::palette().text_muted.with_opacity(0.55);

  Row::new()
    .width(Dimension::Pct(100.0))
    .height(46.0)
    .align_items(Alignment::Center)
    .spacing(0.0)
    .rounded(theme::RadiusSize::Md)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceInput))
    .border_inside(1.0, theme::PaletteColor::Border)
    .child(
      TextInput::styled(value, text_style)
        .placeholder(placeholder)
        .placeholder_style(placeholder_style)
        .single_line()
        .name(name)
        .flex(1.0)
        .height(Dimension::Pct(100.0))
        .padding_left(theme::SpacingSize::Lg)
        .padding_right(theme::SpacingSize::Sm)
        .tab_index(tab_index)
        .background(BackgroundColor::Color(Color::from_hex("#00000000")))
        .caret_color(theme::PaletteColor::Accent),
    )
    .child(
      Row::new()
        .height(Dimension::Pct(100.0))
        .align_items(Alignment::Center)
        .justify(Justify::Center)
        .padding_left(theme::SpacingSize::Sm)
        .padding_right(theme::SpacingSize::Lg)
        .child(ctx.mount::<LucideIcon>(LucideIconProps {
          icon,
          size: 16.0,
          color: theme::palette().text_muted,
        })),
    )
}

fn ghost_button(label: &str) -> Button {
  Button::empty()
    .button()
    .height(34.0)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .padding_horizontal(theme::SpacingSize::Lg)
    .rounded(theme::RadiusSize::Md)
    .cursor(CursorIcon::Pointer)
    .tab_index(4)
    .hovered_style(Style::new().background(BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised)))
    .active_style(Style::new().background(BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised)))
    .child(
      Text::new(label)
        .variant(theme::TypographyStyle::Button)
        .color(theme::PaletteColor::TextSecondary),
    )
}

fn connect_button(label: &str, enabled: bool) -> Button {
  let (background, text_color) = if enabled {
    (
      BackgroundColor::Palette(theme::PaletteColor::Accent),
      theme::PaletteColor::TextInverse,
    )
  } else {
    (
      BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised),
      theme::PaletteColor::TextMuted,
    )
  };

  let mut button = Button::empty()
    .height(34.0)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .padding_horizontal(16.0)
    .rounded(theme::RadiusSize::Md)
    .tab_index(if enabled { 5 } else { -1 })
    .background(background)
    .child(
      Text::new(label)
        .variant(theme::TypographyStyle::Button)
        .color(text_color),
    );

  if enabled {
    button = button
      .submit()
      .cursor(CursorIcon::Pointer)
      .hovered_style(Style::new().background(BackgroundColor::Palette(theme::PaletteColor::AccentHover)))
      .active_style(Style::new().background(BackgroundColor::Palette(theme::PaletteColor::AccentHover)));
  } else {
    button = button.button();
  }

  button
}

fn connecting_button(ctx: &mut Ctx) -> Row {
  Row::new()
    .height(34.0)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .spacing(theme::SpacingSize::Sm)
    .padding_horizontal(16.0)
    .rounded(theme::RadiusSize::Md)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised))
    .child(loader(16.0))
    .child(
      Text::new(&ctx.t("connect_server.action.connecting"))
        .variant(theme::TypographyStyle::Button)
        .color(theme::PaletteColor::TextSecondary),
    )
}

#[cfg(test)]
#[path = "../../tests/unit/ui/connect_server.rs"]
mod tests;
