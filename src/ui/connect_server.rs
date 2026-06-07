use std::{
  net::SocketAddr,
  sync::Arc,
  time::{SystemTime, UNIX_EPOCH},
};

use lurq::{
  app::{component::Component, ctx::Ctx},
  components::{Button, Column, Form, FormProps, Row, Text, TextInput},
  core::Signal,
  layout::{Alignment, layout_kind::Justify},
  node::{BackgroundColor, CursorIcon, Element, Style, color::Color, dimension::Dimension},
};

use crate::{
  identity::auth_identity,
  network::{protocol::S2C, server::Server},
  routes::{ROUTE_CHOOSE_SERVER, ROUTE_SETTINGS_SERVERS},
  session::{ConnectedServer, ConnectedServerInfo, ServerSession},
  storage::{AppSettings, Storage, StoredServer},
  theme,
  ui::{
    common::lucide_icon::{LucideIcon, LucideIconProps},
    loader::loader,
  },
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ConnectOrigin {
  ServerList,
  Settings,
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
    let settings = ctx
      .use_context::<Storage>()
      .and_then(|storage| storage.load_settings().ok())
      .unwrap_or_else(AppSettings::default);

    Self {
      address: ctx.signal(String::new()),
      seed: ctx.signal(String::new()),
      display_name: ctx.signal(settings.display_name),
      navigated: ctx.signal(false),
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let storage = ctx.use_context::<Storage>();
    let session = ctx.use_context::<ServerSession>();

    let from_settings = ctx
      .route_state::<ConnectOrigin>()
      .is_some_and(|origin| *origin == ConnectOrigin::Settings);
    let home_route = if from_settings {
      ROUTE_SETTINGS_SERVERS
    } else {
      ROUTE_CHOOSE_SERVER
    };
    let back_label = if from_settings {
      ctx.t("connect_server.back_to_settings")
    } else {
      ctx.t("connect_server.back_to_list")
    };

    let connect = ctx.future_action(move |(address, seed, display_name): (String, String, String)| {
      let storage = storage.clone();
      let session = session.clone();
      async move { connect_and_store(address, seed, display_name, storage, session).await }
    });

    let state = connect.state().get();
    let connecting = state.is_pending();
    let error = state.error.clone();

    if state.data.is_some() && !self.navigated.get_untracked() {
      self.navigated.set(true);
      if let Some(navigator) = ctx.navigator() {
        navigator.replace(home_route);
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

    card = card.child(self.actions(ctx, &connect, connecting, can_connect, home_route));

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
          .child(back_row(ctx, &back_label, home_route))
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
    home_route: &'static str,
  ) -> impl Into<Element> {
    let navigator = ctx.navigator();
    let cancel_action = connect.clone();
    let cancel_label = ctx.t("connect_server.action.cancel");
    let connect_label = ctx.t("connect_server.action.connect");

    let cancel = ghost_button(&cancel_label).on_click(move |_| {
      cancel_action.cancel();
      if let Some(navigator) = navigator.as_ref() {
        if !navigator.back() {
          navigator.push(home_route);
        }
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

const DEFAULT_SERVER_PORT: u16 = 7800;

async fn connect_and_store(
  address: String,
  seed: String,
  display_name: String,
  storage: Option<Storage>,
  session: Option<ServerSession>,
) -> Result<ConnectedServerInfo, String> {
  let address = with_default_port(&address);
  let display_name = display_name.trim().to_owned();

  let identity = storage
    .as_ref()
    .ok_or_else(|| "Local storage is unavailable.".to_owned())?
    .load_identity()
    .map_err(|error| error.to_string())?
    .ok_or_else(|| "No local identity found.".to_owned())?;

  let socket = resolve_address(address.clone()).await?;
  let server = Server::connect(socket).await.map_err(|error| error.to_string())?;
  let fingerprint = server.certificate_fingerprint().unwrap_or_default();

  let timestamp = SystemTime::now()
    .duration_since(UNIX_EPOCH)
    .map_err(|error| error.to_string())?
    .as_secs();
  let auth = auth_identity(&identity, &display_name, timestamp, seed.clone()).map_err(|error| error.to_string())?;
  server.authenticate(auth).await.map_err(|error| error.to_string())?;

  let response = match server.recv().await.map_err(|error| error.to_string())? {
    S2C::AuthResponse(response) => response,
    S2C::ServerError { message } => return Err(message),
    _ => return Err("Unexpected response from server.".to_owned()),
  };

  let info = ConnectedServerInfo {
    address: address.clone(),
    server_name: response.server_name.clone(),
    user_id: response.user_id,
    role: response.role,
    certificate_fingerprint: fingerprint.clone(),
  };

  if let Some(storage) = storage.as_ref() {
    storage
      .save_server(&StoredServer {
        address,
        server_name: response.server_name,
        user_id: response.user_id,
        role: response.role,
        certificate_fingerprint: fingerprint,
        server_password: seed,
        display_name,
      })
      .map_err(|error| error.to_string())?;
  }

  if let Some(session) = session {
    session.set_connected(ConnectedServer {
      info: info.clone(),
      server: Arc::new(server),
    });
  }

  Ok(info)
}

fn with_default_port(address: &str) -> String {
  let address = address.trim();

  if let Some(rest) = address.strip_prefix('[') {
    return match rest.find(']') {
      Some(end) if rest[end + 1..].starts_with(':') => address.to_owned(),
      _ => format!("{address}:{DEFAULT_SERVER_PORT}"),
    };
  }

  if address.contains(':') {
    address.to_owned()
  } else {
    format!("{address}:{DEFAULT_SERVER_PORT}")
  }
}

async fn resolve_address(address: String) -> Result<SocketAddr, String> {
  tokio::task::spawn_blocking(move || {
    use std::net::ToSocketAddrs;
    address
      .to_socket_addrs()
      .map_err(|error| error.to_string())?
      .next()
      .ok_or_else(|| "Could not resolve server address.".to_owned())
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

fn back_row(ctx: &mut Ctx, label: &str, home_route: &'static str) -> impl Into<Element> {
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
      if !navigator.back() {
        navigator.push(home_route);
      }
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
    .focused_style(
      Style::new()
        .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised))
        .border_inside(1.0, theme::PaletteColor::BorderFocus),
    )
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
      .active_style(Style::new().background(BackgroundColor::Palette(theme::PaletteColor::AccentHover)))
      .focused_style(
        Style::new()
          .background(BackgroundColor::Palette(theme::PaletteColor::AccentHover))
          .border_inside(1.0, theme::PaletteColor::BorderFocus),
      );
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
