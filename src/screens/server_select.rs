use lurq::{
  app::{
    component::Component,
    ctx::{Ctx, FutureAction},
  },
  components::{Column, FormErrors, Row, Text},
  core::Signal,
  layout::{Alignment, layout_kind::Justify, text_style::FontWeight},
  node::{BackgroundColor, CursorIcon, Element, Style, dimension::Dimension},
  router::Navigator,
};

use crate::{
  screens::{
    server_connect::{ServerConnectMessages, connect_to_server_address},
    shared::{
      self, CARD_WIDTH, INTRO_WIDTH, ROUTE_CONNECT_SERVER, ROUTE_IDENTITY_SETUP, ROUTE_LOBBY, ROUTE_TOFU_WARNING,
      styled_text,
    },
  },
  session::{ConnectedServerInfo, ServerSession},
  storage::{Storage, StoredServer},
  theme,
};

type SavedServerConnectArgs = (String, String, String);

async fn load_saved_servers(storage: Option<Storage>) -> Result<Vec<StoredServer>, String> {
  let Some(storage) = storage else {
    return Ok(Vec::new());
  };

  tokio::task::spawn_blocking(move || storage.load_servers())
    .await
    .map_err(|error| error.to_string())?
    .map_err(|error| error.to_string())
}

fn add_server_button(label: &str, navigator: Option<Navigator>) -> Row {
  let row = Row::new()
    .width(58.0)
    .height(34.0)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .rounded(5.0)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised))
    .border_inside(1.0, theme::PaletteColor::Border)
    .cursor(CursorIcon::Pointer)
    .hovered_style(Style::new().background(BackgroundColor::Palette(theme::PaletteColor::SurfaceInput)))
    .child(Text::new(label).variant(theme::TypographyStyle::Button));

  if let Some(navigator) = navigator {
    row.on_click(move |_| navigator.push(ROUTE_CONNECT_SERVER))
  } else {
    row
  }
}

fn drop_identity_button(
  label: &str,
  storage: Option<Storage>,
  navigator: Option<Navigator>,
  failed: Signal<bool>,
) -> Row {
  Row::new()
    .width(Dimension::Pct(100.0))
    .height(34.0)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .spacing(8.0)
    .rounded(5.0)
    .background(BackgroundColor::Palette(theme::PaletteColor::DangerMuted))
    .border_inside(1.0, theme::PaletteColor::Danger)
    .cursor(CursorIcon::Pointer)
    .hovered_style(Style::new().background(theme::PaletteColor::DangerMuted))
    .child(shared::icon("trash-2", 14.0, theme::palette().danger))
    .child(styled_text(
      label,
      "Inter",
      13.0,
      FontWeight::Bold,
      theme::palette().danger,
      1.2,
    ))
    .on_click(move |_| {
      let dropped = storage
        .as_ref()
        .map(|storage| storage.delete_identity().is_ok())
        .unwrap_or(false);

      failed.set(!dropped);
      if dropped && let Some(navigator) = &navigator {
        navigator.replace(ROUTE_IDENTITY_SETUP);
      }
    })
}

fn drop_identity_error(message: &str, visible: bool) -> Row {
  if !visible {
    return Row::new().height(0.0);
  }

  Row::new()
    .width(Dimension::Pct(100.0))
    .align_items(Alignment::Center)
    .spacing(8.0)
    .padding(10.0)
    .rounded(5.0)
    .background(BackgroundColor::Palette(theme::PaletteColor::DangerMuted))
    .border_inside(1.0, theme::PaletteColor::Danger)
    .child(shared::icon("alert-triangle", 14.0, theme::palette().danger))
    .child(Text::new(message).variant(theme::TypographyStyle::Link).flex(1.0))
}

fn meta_card(title: &str, body: &str) -> Column {
  Column::new()
    .width(INTRO_WIDTH)
    .spacing(8.0)
    .padding(12.0)
    .rounded(6.0)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfacePanel))
    .border_inside(1.0, theme::PaletteColor::Border)
    .child(shared::dot(BackgroundColor::Palette(theme::PaletteColor::Success)))
    .child(Text::new(title).variant(theme::TypographyStyle::Button))
    .child(Text::new(body).variant(theme::TypographyStyle::Link))
}

fn server_row(
  server: &StoredServer,
  active: bool,
  connect_action: FutureAction<SavedServerConnectArgs, ConnectedServerInfo, FormErrors>,
) -> Row {
  let background = if active {
    theme::palette().success_muted
  } else {
    theme::palette().surface_raised
  };
  let border = if active {
    theme::palette().accent
  } else {
    theme::palette().border
  };
  let dot = if active {
    theme::palette().accent
  } else {
    theme::palette().text_muted
  };
  let role = if active {
    theme::palette().accent
  } else {
    theme::palette().text_muted
  };
  let role_text = format!("{:?} #{}", server.role, server.user_id);

  let row = Row::new()
    .width(Dimension::Pct(100.0))
    .align_items(Alignment::Center)
    .spacing(10.0)
    .padding_vertical(10.0)
    .padding_horizontal(12.0)
    .rounded(6.0)
    .background(background)
    .border_inside(1.0, border)
    .cursor(CursorIcon::Pointer)
    .hovered_style(Style::new().background(BackgroundColor::Palette(theme::PaletteColor::SurfaceInput)))
    .child(shared::dot(dot))
    .child(
      Column::new()
        .flex(1.0)
        .spacing(2.0)
        .child(styled_text(
          &server.server_name,
          "Inter",
          12.0,
          FontWeight::Bold,
          theme::palette().text_primary,
          1.2,
        ))
        .child(styled_text(
          &server.address,
          "JetBrains Mono",
          10.0,
          FontWeight::Medium,
          theme::palette().text_muted,
          1.2,
        )),
    )
    .child(styled_text(
      &role_text,
      "JetBrains Mono",
      10.0,
      FontWeight::Bold,
      role,
      1.2,
    ));

  let address = server.address.clone();
  let server_password = server.server_password.clone();
  let display_name = server.display_name.clone();
  row.on_click(move |_| connect_action.run((address.clone(), server_password.clone(), display_name.clone())))
}

fn empty_servers_row(message: &str) -> Row {
  Row::new()
    .width(Dimension::Pct(100.0))
    .align_items(Alignment::Center)
    .spacing(8.0)
    .padding(10.0)
    .rounded(5.0)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised))
    .border_inside(1.0, theme::PaletteColor::Border)
    .child(Text::new(message).variant(theme::TypographyStyle::Link).flex(1.0))
}

pub struct ServerSelect {
  drop_failed: Signal<bool>,
}

impl Component for ServerSelect {
  type Props = ();

  fn create(ctx: &mut Ctx) -> Self {
    Self {
      drop_failed: ctx.signal(false),
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let navigator = ctx.navigator();
    let storage = ctx.use_context::<Storage>();
    let session = ctx.use_context::<ServerSession>().unwrap_or_default();
    let active_server = session.info();
    let connect_action = ctx.future_action({
      let storage = storage.clone();
      let session = session.clone();
      let messages = ServerConnectMessages::from_ctx(ctx);
      move |(address, server_password, display_name)| {
        connect_to_server_address(
          address,
          server_password,
          display_name,
          storage.clone(),
          session.clone(),
          messages.clone(),
        )
      }
    });
    let connect_state = connect_action.state().get();
    if connect_state.data.is_some()
      && let Some(navigator) = navigator.as_ref()
    {
      if session.tofu_warning().is_some() {
        navigator.replace(ROUTE_TOFU_WARNING);
      } else {
        navigator.replace(ROUTE_LOBBY);
      }
    }
    let servers_state = ctx.future(storage.clone(), load_saved_servers).state().get();
    let servers = servers_state.data.as_deref().unwrap_or(&[]);
    let server_count = servers.len();
    let meta_title = match server_count {
      0 => ctx.t("server_select.meta_title_empty"),
      1 => ctx.t("server_select.meta_title_one"),
      _ => {
        let count = server_count.to_string();
        ctx.t_args("server_select.meta_title_many", [("count", count)])
      }
    };
    let meta_desc = if server_count == 0 {
      ctx.t("server_select.meta_desc_empty")
    } else {
      ctx.t("server_select.meta_desc_saved")
    };
    let mut server_card = Column::new()
      .width(CARD_WIDTH)
      .spacing(14.0)
      .padding(18.0)
      .rounded(8.0)
      .background(BackgroundColor::Palette(theme::PaletteColor::SurfacePanel))
      .border_inside(1.0, theme::PaletteColor::Border)
      .child(
        Row::new()
          .width(Dimension::Pct(100.0))
          .align_items(Alignment::Center)
          .spacing(12.0)
          .child(Text::new(&ctx.t("server_select.heading")).variant(theme::TypographyStyle::Heading))
          .child(Row::new().height(1.0).flex(1.0))
          .child(add_server_button(&ctx.t("server_select.action.add"), navigator.clone())),
      );

    if servers_state.is_pending() {
      server_card = server_card.child(empty_servers_row(&ctx.t("server_select.status.loading")));
    } else if let Some(error) = servers_state.error.as_ref() {
      server_card = server_card.child(empty_servers_row(error));
    } else if servers.is_empty() {
      server_card = server_card.child(empty_servers_row(&ctx.t("server_select.status.empty")));
    } else {
      for server in servers {
        server_card = server_card.child(server_row(
          server,
          active_server
            .as_ref()
            .is_some_and(|active| active.address == server.address),
          connect_action.clone(),
        ));
      }
    }
    if let Some(error) = connect_state.error.as_ref() {
      let message = error
        .first("server_address")
        .map(str::to_owned)
        .unwrap_or_else(|| ctx.t("server_select.connect_failed"));
      server_card = server_card.child(empty_servers_row(&message));
    }

    server_card = server_card
      .child(drop_identity_error(
        &ctx.t("server_select.drop_identity_failed"),
        self.drop_failed.get(),
      ))
      .child(drop_identity_button(
        &ctx.t("server_select.action.drop_identity"),
        storage,
        navigator,
        self.drop_failed.clone(),
      ));

    shared::identity_screen(
      Column::new()
        .width(INTRO_WIDTH)
        .spacing(18.0)
        .child(Text::new(&ctx.t("server_select.caption")).variant(theme::TypographyStyle::Caption))
        .child(Text::new(&ctx.t("server_select.title")).variant(theme::TypographyStyle::Title))
        .child(Text::new(&ctx.t("server_select.desc")).variant(theme::TypographyStyle::Description))
        .child(meta_card(&meta_title, &meta_desc)),
      server_card,
    )
  }
}
