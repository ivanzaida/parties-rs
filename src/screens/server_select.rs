use lurq::{
  app::{component::Component, ctx::Ctx},
  components::{Column, Row, Text},
  core::Signal,
  layout::{Alignment, layout_kind::Justify, text_style::FontWeight},
  node::{BackgroundColor, CursorIcon, Element, Style, color::Color, dimension::Dimension},
  router::Navigator,
};

use crate::{
  screens::shared::{self, BORDER, CARD_WIDTH, INTRO_WIDTH, ROUTE_CONNECT_SERVER, ROUTE_IDENTITY_SETUP, styled_text},
  storage::Storage,
  theme,
};

struct SavedServer {
  name: &'static str,
  address: &'static str,
  fingerprint: &'static str,
  selected: bool,
  trusted: bool,
}

const SAVED_SERVERS: &[SavedServer] = &[
  SavedServer {
    name: "My Server",
    address: "192.168.1.50:7800",
    fingerprint: "a3:f1:7b",
    selected: true,
    trusted: true,
  },
  SavedServer {
    name: "Dev Team",
    address: "dev.parties.io:7800",
    fingerprint: "7b:02:91",
    selected: false,
    trusted: true,
  },
  SavedServer {
    name: "Gaming Night",
    address: "10.0.0.5:7800",
    fingerprint: "91:cc:d4",
    selected: false,
    trusted: true,
  },
  SavedServer {
    name: "localhost",
    address: "127.0.0.1:7800",
    fingerprint: "untrusted",
    selected: false,
    trusted: false,
  },
];

fn add_server_button(label: &str, navigator: Option<Navigator>) -> Row {
  let row = Row::new()
    .width(58.0)
    .height(34.0)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .rounded(5.0)
    .background(BackgroundColor::Palette(theme::BG_ELEVATED))
    .border_inside(1.0, Color::from_hex(BORDER))
    .cursor(CursorIcon::Pointer)
    .hovered_style(Style::new().background(BackgroundColor::Palette(theme::BG_INPUT)))
    .child(Text::new(label).variant(theme::TYP_BUTTON));

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
    .background(BackgroundColor::Palette(theme::RED_MUTED))
    .border_inside(1.0, Color::from_hex("#4A2A27"))
    .cursor(CursorIcon::Pointer)
    .hovered_style(Style::new().background("#351D1F"))
    .child(shared::icon("trash-2", 14.0, "#FF6B5F"))
    .child(styled_text(label, "Inter", 13.0, FontWeight::Bold, "#FF6B5F", 1.2))
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
    .background(BackgroundColor::Palette(theme::RED_MUTED))
    .border_inside(1.0, Color::from_hex("#4A2A27"))
    .child(shared::icon("alert-triangle", 14.0, "#FF6B5F"))
    .child(Text::new(message).variant(theme::TYP_LINK).flex(1.0))
}

fn meta_card(title: &str, body: &str) -> Column {
  Column::new()
    .width(INTRO_WIDTH)
    .spacing(8.0)
    .padding(12.0)
    .rounded(6.0)
    .background(BackgroundColor::Palette(theme::BG_TERTIARY))
    .border_inside(1.0, Color::from_hex(BORDER))
    .child(shared::dot(BackgroundColor::Palette(theme::GREEN)))
    .child(Text::new(title).variant(theme::TYP_BUTTON))
    .child(Text::new(body).variant(theme::TYP_LINK))
}

fn server_row(server: &SavedServer) -> Row {
  let background = if server.selected {
    theme::GREEN_MUTED_COLOR
  } else {
    theme::BG_ELEVATED_COLOR
  };
  let border = if server.selected {
    theme::ACCENT_COLOR
  } else {
    theme::BORDER_COLOR
  };
  let dot = if server.selected {
    theme::ACCENT_COLOR
  } else {
    theme::TEXT_MUTED_COLOR
  };
  let fingerprint = if server.selected && server.trusted {
    theme::ACCENT_COLOR
  } else {
    theme::TEXT_MUTED_COLOR
  };

  Row::new()
    .width(Dimension::Pct(100.0))
    .align_items(Alignment::Center)
    .spacing(10.0)
    .padding_vertical(10.0)
    .padding_horizontal(12.0)
    .rounded(6.0)
    .background(background)
    .border_inside(1.0, border)
    .cursor(CursorIcon::Pointer)
    .hovered_style(Style::new().background(BackgroundColor::Palette(theme::BG_INPUT)))
    .child(shared::dot(dot))
    .child(
      Column::new()
        .flex(1.0)
        .spacing(2.0)
        .child(styled_text(
          server.name,
          "Inter",
          12.0,
          FontWeight::Bold,
          theme::TEXT_PRIMARY_COLOR,
          1.2,
        ))
        .child(styled_text(
          server.address,
          "JetBrains Mono",
          10.0,
          FontWeight::Medium,
          theme::TEXT_MUTED_COLOR,
          1.2,
        )),
    )
    .child(styled_text(
      server.fingerprint,
      "JetBrains Mono",
      10.0,
      FontWeight::Bold,
      fingerprint,
      1.2,
    ))
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

    shared::identity_screen(
      Column::new()
        .width(INTRO_WIDTH)
        .spacing(18.0)
        .child(Text::new(&ctx.t("server_select.caption")).variant(theme::TYP_CAPTION))
        .child(Text::new(&ctx.t("server_select.title")).variant(theme::TYP_TITLE))
        .child(Text::new(&ctx.t("server_select.desc")).variant(theme::TYP_DESC))
        .child(meta_card(
          &ctx.t("server_select.meta_title"),
          &ctx.t("server_select.meta_desc"),
        )),
      Column::new()
        .width(CARD_WIDTH)
        .spacing(14.0)
        .padding(18.0)
        .rounded(8.0)
        .background(BackgroundColor::Palette(theme::BG_TERTIARY))
        .border_inside(1.0, Color::from_hex(BORDER))
        .child(
          Row::new()
            .width(Dimension::Pct(100.0))
            .align_items(Alignment::Center)
            .spacing(12.0)
            .child(Text::new(&ctx.t("server_select.heading")).variant(theme::TYP_HEADING))
            .child(Row::new().height(1.0).flex(1.0))
            .child(add_server_button(&ctx.t("server_select.action.add"), navigator.clone())),
        )
        .child(server_row(&SAVED_SERVERS[0]))
        .child(server_row(&SAVED_SERVERS[1]))
        .child(server_row(&SAVED_SERVERS[2]))
        .child(server_row(&SAVED_SERVERS[3]))
        .child(drop_identity_error(
          &ctx.t("server_select.drop_identity_failed"),
          self.drop_failed.get(),
        ))
        .child(drop_identity_button(
          &ctx.t("server_select.action.drop_identity"),
          storage,
          navigator,
          self.drop_failed.clone(),
        )),
    )
  }
}
