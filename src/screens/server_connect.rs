use std::{
  net::SocketAddr,
  sync::Arc,
  time::{SystemTime, UNIX_EPOCH},
};

use lurq::{
  app::{component::Component, ctx::Ctx},
  components::{
    Column, FormErrors, FormHandle, FormOptions, FormProps, FormValues, Text, ValidationResult, validators,
  },
  layout::text_style::FontWeight,
  node::{BackgroundColor, Element, dimension::Dimension},
};

use crate::{
  identity,
  network::{
    protocol::{DEFAULT_PORT, S2C},
    server::Server,
  },
  screens::shared::{
    self, CARD_WIDTH, INTRO_WIDTH, ROUTE_CHOOSE_SERVER, ROUTE_LOBBY, ROUTE_TOFU_WARNING, action_button,
  },
  session::{ConnectedServer, ConnectedServerInfo, ServerSession, TofuWarning},
  storage::{Storage, StoredServer},
  theme,
};

fn meta_card(title: &str, body: &str) -> Column {
  Column::new()
    .width(INTRO_WIDTH)
    .spacing(8.0)
    .padding(12.0)
    .rounded(6.0)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfacePanel))
    .border_inside(1.0, theme::PaletteColor::Border)
    .child(shared::dot(BackgroundColor::Palette(theme::PaletteColor::Warning)))
    .child(Text::new(title).variant(theme::TypographyStyle::Button))
    .child(Text::new(body).variant(theme::TypographyStyle::Link))
}

fn trust_preview(label: &str, value: &str) -> Column {
  Column::new()
    .width(Dimension::Pct(100.0))
    .spacing(6.0)
    .padding(10.0)
    .rounded(5.0)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised))
    .border_inside(1.0, theme::PaletteColor::Border)
    .child(shared::styled_text(
      label,
      "JetBrains Mono",
      10.0,
      FontWeight::Bold,
      theme::palette().text_muted,
      1.2,
    ))
    .child(shared::styled_text(
      value,
      "JetBrains Mono",
      10.0,
      FontWeight::Medium,
      theme::palette().text_muted,
      1.2,
    ))
}

pub struct ServerConnect {
  form: FormHandle,
}

#[derive(Clone)]
pub(crate) struct ServerConnectMessages {
  storage_unavailable: String,
  load_identity_failed: String,
  no_identity: String,
  connect_failed: String,
  clock_failed: String,
  auth_payload_failed: String,
  auth_failed: String,
  auth_unexpected_response: String,
  auth_read_failed: String,
  certificate_missing: String,
  save_server_failed: String,
  invalid_address: String,
  address_unresolved: String,
}

impl ServerConnectMessages {
  pub(crate) fn from_ctx(ctx: &Ctx) -> Self {
    Self {
      storage_unavailable: ctx.t("server_connect.error.storage_unavailable"),
      load_identity_failed: ctx.t("server_connect.error.load_identity_failed"),
      no_identity: ctx.t("server_connect.error.no_identity"),
      connect_failed: ctx.t("server_connect.error.connect_failed"),
      clock_failed: ctx.t("server_connect.error.clock_failed"),
      auth_payload_failed: ctx.t("server_connect.error.auth_payload_failed"),
      auth_failed: ctx.t("server_connect.error.auth_failed"),
      auth_unexpected_response: ctx.t("server_connect.error.auth_unexpected_response"),
      auth_read_failed: ctx.t("server_connect.error.auth_read_failed"),
      certificate_missing: ctx.t("server_connect.error.certificate_missing"),
      save_server_failed: ctx.t("server_connect.error.save_server_failed"),
      invalid_address: ctx.t("server_connect.error.invalid_address"),
      address_unresolved: ctx.t("server_connect.error.address_unresolved"),
    }
  }
}

fn detail_message(template: &str, detail: impl ToString) -> String {
  template.replace("{{detail}}", &detail.to_string())
}

async fn connect_to_server(
  values: FormValues,
  storage: Option<Storage>,
  session: ServerSession,
  messages: ServerConnectMessages,
) -> Result<ConnectedServerInfo, FormErrors> {
  let address = values
    .get_string("server_address")
    .unwrap_or_default()
    .trim()
    .to_owned();
  let server_password = values
    .get_string("server_password")
    .unwrap_or_default()
    .trim()
    .to_owned();
  let display_name = values.get_string("display_name").unwrap_or_default().trim().to_owned();

  connect_to_server_address(address, server_password, display_name, storage, session, messages).await
}

pub(crate) async fn connect_to_server_address(
  address: String,
  server_password: String,
  display_name: String,
  storage: Option<Storage>,
  session: ServerSession,
  messages: ServerConnectMessages,
) -> Result<ConnectedServerInfo, FormErrors> {
  let Some(storage) = storage else {
    return Err(FormErrors::new().with("server_address", messages.storage_unavailable));
  };

  let identity_storage = storage.clone();
  let identity = tokio::task::spawn_blocking(move || identity_storage.load_identity())
    .await
    .map_err(|error| field_error("server_address", detail_message(&messages.load_identity_failed, error)))?
    .map_err(|error| field_error("server_address", detail_message(&messages.load_identity_failed, error)))?
    .ok_or_else(|| field_error("server_address", messages.no_identity.clone()))?;

  let addr = resolve_server_address(&address, &messages)
    .await
    .map_err(|message| field_error("server_address", message))?;
  let server = Server::connect(addr)
    .await
    .map_err(|error| field_error("server_address", detail_message(&messages.connect_failed, error)))?;
  let certificate_fingerprint = server
    .certificate_fingerprint()
    .ok_or_else(|| field_error("server_address", messages.certificate_missing.clone()))?;

  let timestamp = SystemTime::now()
    .duration_since(UNIX_EPOCH)
    .map_err(|error| field_error("server_address", detail_message(&messages.clock_failed, error)))?
    .as_secs();
  let auth = identity::auth_identity(&identity, &display_name, timestamp, server_password.clone())
    .map_err(|error| field_error("server_address", detail_message(&messages.auth_payload_failed, error)))?;
  server
    .authenticate(auth)
    .await
    .map_err(|error| field_error("server_address", detail_message(&messages.auth_failed, error)))?;

  let auth_response = match server.recv().await {
    Ok(S2C::AuthResponse(auth_response)) => auth_response,
    Ok(S2C::ServerError { message }) => return Err(field_error("server_address", message)),
    Ok(message) => {
      return Err(field_error(
        "server_address",
        detail_message(&messages.auth_unexpected_response, format!("{message:?}")),
      ));
    }
    Err(error) => {
      return Err(field_error(
        "server_address",
        detail_message(&messages.auth_read_failed, error),
      ));
    }
  };

  let info = ConnectedServerInfo {
    address: addr.to_string(),
    server_name: auth_response.server_name,
    user_id: auth_response.user_id,
    role: auth_response.role,
    certificate_fingerprint,
  };
  let stored_server = StoredServer {
    address: info.address.clone(),
    server_name: info.server_name.clone(),
    user_id: info.user_id,
    role: info.role,
    certificate_fingerprint: info.certificate_fingerprint.clone(),
    server_password,
    display_name: display_name.clone(),
  };
  let warning_server_password = stored_server.server_password.clone();
  let warning_display_name = stored_server.display_name.clone();
  let trust_warning = tokio::task::spawn_blocking(move || {
    let existing = storage.load_server(&stored_server.address)?;
    if let Some(existing) = existing
      && !existing.certificate_fingerprint.is_empty()
      && existing.certificate_fingerprint != stored_server.certificate_fingerprint
    {
      return Ok::<_, crate::storage::StorageError>(Some(existing.certificate_fingerprint));
    }

    storage.save_server(&stored_server)?;
    Ok(None)
  })
  .await
  .map_err(|error| field_error("server_address", detail_message(&messages.save_server_failed, error)))?
  .map_err(|error| field_error("server_address", detail_message(&messages.save_server_failed, error)))?;

  session.set_connected(ConnectedServer {
    info: info.clone(),
    server: Arc::new(server),
  });
  if let Some(saved_fingerprint) = trust_warning {
    session.set_tofu_warning(TofuWarning {
      address: info.address.clone(),
      server_name: info.server_name.clone(),
      user_id: info.user_id,
      role: info.role,
      saved_fingerprint,
      received_fingerprint: info.certificate_fingerprint.clone(),
      server_password: warning_server_password,
      display_name: warning_display_name,
    });
  } else {
    session.clear_tofu_warning();
  }
  Ok(info)
}

async fn resolve_server_address(input: &str, messages: &ServerConnectMessages) -> Result<SocketAddr, String> {
  if let Ok(addr) = input.parse::<SocketAddr>() {
    return Ok(addr);
  }

  let endpoint = if has_explicit_port(input) {
    input.to_owned()
  } else {
    format!("{input}:{DEFAULT_PORT}")
  };
  let mut addrs = tokio::net::lookup_host(&endpoint)
    .await
    .map_err(|error| detail_message(&messages.invalid_address, error))?;
  addrs.next().ok_or_else(|| messages.address_unresolved.clone())
}

fn has_explicit_port(input: &str) -> bool {
  input
    .rsplit_once(':')
    .is_some_and(|(_, port)| !port.is_empty() && port.chars().all(|ch| ch.is_ascii_digit()))
}

fn field_error(name: impl Into<Arc<str>>, message: impl Into<Arc<str>>) -> FormErrors {
  FormErrors::new().with(name, message)
}

impl Component for ServerConnect {
  type Props = ();

  fn create(ctx: &mut Ctx) -> Self {
    let address_required = ctx.t("server_connect.address.error_required");
    let address_invalid = ctx.t("server_connect.address.error_format");

    Self {
      form: ctx.form(
        FormOptions::new()
          .field("server_address", "")
          .field("server_password", "")
          .field("display_name", "")
          .validate_string("server_address", validators::required(address_required))
          .validate_string("server_address", move |address, _| {
            let address = address.trim();
            if address.is_empty() || address.parse::<SocketAddr>().is_ok() || !address.contains(char::is_whitespace) {
              ValidationResult::valid()
            } else {
              ValidationResult::invalid(address_invalid.clone())
            }
          }),
      ),
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let navigator = ctx.navigator();
    let storage = ctx.use_context::<Storage>();
    let session = ctx.use_context::<ServerSession>().unwrap_or_default();
    let messages = ServerConnectMessages::from_ctx(ctx);
    let connect_action = ctx.future_action({
      let storage = storage.clone();
      let session = session.clone();
      let messages = messages.clone();
      move |values| connect_to_server(values, storage.clone(), session.clone(), messages.clone())
    });
    let connect_state = connect_action.state().get();
    let address = self.form.string("server_address");
    let trust_text = if address.get().trim().is_empty() {
      ctx.t("server_connect.trust.empty")
    } else if connect_state.is_pending() {
      ctx.t("server_connect.trust.connecting")
    } else if let Some(info) = connect_state.data.as_ref() {
      ctx.t_args(
        "server_connect.trust.connected",
        [
          ("server", info.server_name.clone()),
          ("user_id", info.user_id.to_string()),
        ],
      )
    } else {
      ctx.t("server_connect.trust.pending")
    };
    let server_address_error = self.form.error("server_address").get();
    let server_password_error = self.form.error("server_password").get();
    if connect_state.data.is_some()
      && let Some(navigator) = navigator.as_ref()
    {
      if session.tofu_warning().is_some() {
        navigator.replace(ROUTE_TOFU_WARNING);
      } else {
        navigator.replace(ROUTE_LOBBY);
      }
    }

    shared::identity_screen(
      Column::new()
        .width(INTRO_WIDTH)
        .spacing(18.0)
        .child(Text::new(&ctx.t("server_connect.caption")).variant(theme::TypographyStyle::Caption))
        .child(Text::new(&ctx.t("server_connect.title")).variant(theme::TypographyStyle::Title))
        .child(Text::new(&ctx.t("server_connect.desc")).variant(theme::TypographyStyle::Description))
        .child(meta_card(
          &ctx.t("server_connect.meta_title"),
          &ctx.t("server_connect.meta_desc"),
        )),
      ctx.form_view_with(FormProps::new(self.form.clone()).submit_action(connect_action), |ctx| {
        Column::new()
          .width(CARD_WIDTH)
          .spacing(14.0)
          .padding(18.0)
          .rounded(8.0)
          .background(BackgroundColor::Palette(theme::PaletteColor::SurfacePanel))
          .border_inside(1.0, theme::PaletteColor::Border)
          .child(Text::new(&ctx.t("server_connect.heading")).variant(theme::TypographyStyle::Heading))
          .child(
            ctx.mount_keyed::<shared::FormTextInput>(
              if server_address_error.is_some() {
                "server_address-invalid"
              } else {
                "server_address-valid"
              },
              shared::FormTextInputProps::new(self.form.string_control("server_address"))
                .label(ctx.t("server_connect.address.label"))
                .placeholder(ctx.t("server_connect.address.placeholder"))
                .height(38.0),
            ),
          )
          .child(
            ctx.mount_keyed::<shared::FormTextInput>(
              if server_password_error.is_some() {
                "server_password-invalid"
              } else {
                "server_password-valid"
              },
              shared::FormTextInputProps::new(self.form.string_control("server_password"))
                .label(ctx.t("server_connect.password.label"))
                .placeholder(ctx.t("server_connect.password.placeholder"))
                .height(38.0),
            ),
          )
          .child(
            ctx.mount::<shared::FormTextInput>(
              shared::FormTextInputProps::new(self.form.string_control("display_name"))
                .label(ctx.t("server_connect.display_name.label"))
                .placeholder(ctx.t("server_connect.display_name.placeholder"))
                .height(38.0),
            ),
          )
          .child(trust_preview(&ctx.t("server_connect.trust.label"), &trust_text))
          .child(
            ctx.mount::<shared::FormPrimaryButton>(shared::FormPrimaryButtonProps::new(
              ctx.t("server_connect.action.connect"),
            )),
          )
          .child({
            let button = action_button(&ctx.t("identity.action.back"), false);
            if let Some(navigator) = navigator {
              button.on_click(move |_| navigator.push(ROUTE_CHOOSE_SERVER))
            } else {
              button
            }
          })
      }),
    )
  }
}
