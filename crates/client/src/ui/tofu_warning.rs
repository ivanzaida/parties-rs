use lurq::{
  app::{component::Component, ctx::Ctx},
  components::{Column, Row, Text},
  core::{Signal, Store},
  layout::{Alignment, layout_kind::Justify},
  node::{BackgroundColor, CursorIcon, Element, Style, border::Border, color::Color, dimension::Dimension},
};

use crate::{
  routes::{ROUTE_CHOOSE_SERVER, ROUTE_LOBBY},
  session::{ServerSession, TofuWarning},
  storage::{Storage, StoredServer, upsert_stored_server},
  theme,
  ui::common::lucide_icon::{LucideIcon, LucideIconProps},
};

const CARD_WIDTH: f32 = 900.0;
const DANGER: &str = "#FF6B5F";
const DANGER_MUTED: &str = "#2A1A1C";
const WARNING_CARD: &str = "#14100F";
const FINGERPRINT_PANEL: &str = "#171A1E";

pub struct TofuWarningScreen {
  save_error: Signal<Option<String>>,
  navigated: Signal<bool>,
}

impl Component for TofuWarningScreen {
  type Props = ();

  fn create(ctx: &mut Ctx) -> Self {
    Self {
      save_error: ctx.signal(None),
      navigated: ctx.signal(false),
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let session = ctx.use_context::<ServerSession>();
    let storage = ctx.use_context::<Storage>();
    let servers_store = ctx.use_context::<Store<Vec<StoredServer>>>();
    let navigator = ctx.navigator();
    let Some(warning) = session.as_ref().and_then(ServerSession::tofu_warning) else {
      if let Some(navigator) = navigator.as_ref()
        && !self.navigated.get_untracked()
      {
        self.navigated.set(true);
        navigator.replace(ROUTE_CHOOSE_SERVER);
      }
      let empty: Element = empty_screen().into();
      return empty;
    };
    let trust = ctx.future_action(
      |(storage, servers_store, session, warning): (
        Storage,
        Option<Store<Vec<StoredServer>>>,
        ServerSession,
        TofuWarning,
      )| async move { trust_certificate(storage, servers_store, session, warning).await },
    );
    let trust_state = trust.state().get();
    let pending = trust_state.is_pending();

    if trust_state.is_fulfilled() && !self.navigated.get_untracked() {
      self.navigated.set(true);
      if let Some(navigator) = navigator.as_ref() {
        navigator.replace(ROUTE_LOBBY);
      }
    }
    if let Some(error) = trust_state.error {
      self.save_error.set(Some(error));
    }

    Column::new()
      .width(Dimension::Pct(100.0))
      .height(Dimension::Pct(100.0))
      .flex(1.0)
      .align_items(Alignment::Center)
      .justify(Justify::Center)
      .background(BackgroundColor::Color(Color::from_hex("#0B0C0E")))
      .clip()
      .child(
        Column::new()
          .width(CARD_WIDTH)
          .spacing(20.0)
          .padding(28.0)
          .rounded(10.0)
          .background(BackgroundColor::Color(Color::from_hex(WARNING_CARD)))
          .border_inside(1.0, BackgroundColor::Color(Color::from_hex(DANGER)))
          .child(header(ctx, &warning))
          .child(fingerprints(ctx, &warning))
          .child(media_blocked(ctx))
          .child(error_notice(ctx, self.save_error.get()))
          .child(actions(ctx, session, storage, servers_store, warning, trust, pending)),
      )
      .into()
  }
}

fn empty_screen() -> Column {
  Column::new()
    .width(Dimension::Pct(100.0))
    .height(Dimension::Pct(100.0))
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceBase))
}

fn header(ctx: &mut Ctx, warning: &TofuWarning) -> Row {
  let description = ctx.t_args("tofu_warning.desc", [("server", warning.address.clone())]);
  Row::new()
    .width(Dimension::Pct(100.0))
    .align_items(Alignment::Start)
    .spacing(18.0)
    .child(
      Row::new()
        .width(56.0)
        .height(56.0)
        .align_items(Alignment::Center)
        .justify(Justify::Center)
        .rounded(14.0)
        .background(BackgroundColor::Color(Color::from_hex(DANGER_MUTED)))
        .border_inside(1.0, BackgroundColor::Color(Color::from_hex("#FF6B5F4D")))
        .child(ctx.mount::<LucideIcon>(LucideIconProps {
          icon: "shield-alert",
          size: 26.0,
          color: Color::from_hex(DANGER),
        })),
    )
    .child(
      Column::new()
        .flex(1.0)
        .spacing(8.0)
        .child(Text::new(&ctx.t("tofu_warning.title")).variant(theme::TypographyStyle::Title))
        .child(
          Text::new(&description)
            .variant(theme::TypographyStyle::Description)
            .color(theme::PaletteColor::TextSecondary)
            .width(Dimension::Pct(100.0)),
        ),
    )
}

fn fingerprints(ctx: &mut Ctx, warning: &TofuWarning) -> Column {
  Column::new()
    .width(Dimension::Pct(100.0))
    .clip()
    .rounded(8.0)
    .border_inside(1.0, theme::PaletteColor::BorderStrong)
    .child(fingerprint_row(
      ctx,
      &ctx.t("tofu_warning.saved_label"),
      &warning.saved_fingerprint,
      false,
    ))
    .child(fingerprint_row(
      ctx,
      &ctx.t("tofu_warning.received_label"),
      &warning.received_fingerprint,
      true,
    ))
}

fn fingerprint_row(ctx: &mut Ctx, label: &str, fingerprint: &str, received: bool) -> Column {
  let mut row = Column::new()
    .width(Dimension::Pct(100.0))
    .spacing(8.0)
    .padding_vertical(14.0)
    .padding_horizontal(16.0)
    .background(BackgroundColor::Color(Color::from_hex(if received {
      DANGER_MUTED
    } else {
      FINGERPRINT_PANEL
    })))
    .child(
      Row::new()
        .align_items(Alignment::Center)
        .spacing(7.0)
        .child(ctx.mount::<LucideIcon>(LucideIconProps {
          icon: if received { "shield-alert" } else { "shield" },
          size: 13.0,
          color: if received {
            Color::from_hex(DANGER)
          } else {
            theme::palette().text_muted
          },
        }))
        .child(
          Text::new(label)
            .variant(theme::TypographyStyle::Caption)
            .color(if received {
              theme::PaletteColor::Danger
            } else {
              theme::PaletteColor::TextMuted
            }),
        ),
    )
    .child(
      Text::new(&format!("SHA256 {}", fingerprint.trim()))
        .variant(theme::TypographyStyle::Mono)
        .color(theme::PaletteColor::TextSecondary)
        .width(Dimension::Pct(100.0)),
    );

  if received {
    row = row.border_top(Border::inside(1.0, BackgroundColor::Color(Color::from_hex(DANGER))));
  }
  row
}

fn media_blocked(ctx: &mut Ctx) -> Row {
  Row::new()
    .width(Dimension::Pct(100.0))
    .align_items(Alignment::Start)
    .spacing(12.0)
    .padding_vertical(14.0)
    .padding_horizontal(16.0)
    .rounded(6.0)
    .background(BackgroundColor::Color(Color::from_hex(DANGER_MUTED)))
    .border_inside(1.0, BackgroundColor::Color(Color::from_hex("#FF6B5F4D")))
    .child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon: "lock",
      size: 16.0,
      color: Color::from_hex(DANGER),
    }))
    .child(
      Column::new()
        .flex(1.0)
        .spacing(4.0)
        .child(
          Text::new(&ctx.t("tofu_warning.media.title"))
            .variant(theme::TypographyStyle::Button)
            .color(theme::PaletteColor::Danger),
        )
        .child(
          Text::new(&ctx.t("tofu_warning.media.desc"))
            .variant(theme::TypographyStyle::Link)
            .color(theme::PaletteColor::TextSecondary)
            .width(Dimension::Pct(100.0)),
        ),
    )
}

fn error_notice(ctx: &mut Ctx, error: Option<String>) -> Element {
  let Some(error) = error else {
    return Column::new().into();
  };
  Row::new()
    .width(Dimension::Pct(100.0))
    .align_items(Alignment::Start)
    .spacing(10.0)
    .child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon: "triangle-alert",
      size: 15.0,
      color: theme::palette().danger,
    }))
    .child(
      Text::new(&error)
        .variant(theme::TypographyStyle::Description)
        .color(theme::PaletteColor::Danger)
        .width(Dimension::Pct(100.0)),
    )
    .into()
}

fn actions(
  ctx: &mut Ctx,
  session: Option<ServerSession>,
  storage: Option<Storage>,
  servers_store: Option<Store<Vec<StoredServer>>>,
  warning: TofuWarning,
  trust: lurq::app::ctx::FutureAction<
    (Storage, Option<Store<Vec<StoredServer>>>, ServerSession, TofuWarning),
    (),
    String,
  >,
  pending: bool,
) -> Row {
  let navigator = ctx.navigator();
  let disconnect_session = session.clone();
  let trust_storage = storage.clone();
  let trust_servers_store = servers_store.clone();
  let trust_session = session.clone();
  Row::new()
    .width(Dimension::Pct(100.0))
    .align_items(Alignment::Center)
    .spacing(12.0)
    .child(
      action_button(
        ctx,
        "power",
        &ctx.t("tofu_warning.action.disconnect"),
        ButtonTone::Disconnect,
        pending,
      )
      .on_click(move |_| {
        if pending {
          return;
        }
        if let Some(session) = disconnect_session.as_ref() {
          session.disconnect();
        }
        if let Some(navigator) = navigator.as_ref() {
          navigator.replace(ROUTE_CHOOSE_SERVER);
        }
      }),
    )
    .child(
      action_button(
        ctx,
        "shield-alert",
        &ctx.t("tofu_warning.action.trust"),
        ButtonTone::Trust,
        pending,
      )
      .on_click(move |_| {
        if pending {
          return;
        }
        let (Some(storage), Some(session)) = (trust_storage.as_ref(), trust_session.as_ref()) else {
          return;
        };
        trust.run((
          storage.clone(),
          trust_servers_store.clone(),
          session.clone(),
          warning.clone(),
        ));
      }),
    )
}

#[derive(Clone, Copy)]
enum ButtonTone {
  Disconnect,
  Trust,
}

fn action_button(ctx: &mut Ctx, icon: &'static str, label: &str, tone: ButtonTone, disabled: bool) -> Row {
  let (background, border, text_color, icon_color, hover_background, flex) = match tone {
    ButtonTone::Disconnect => (
      BackgroundColor::Palette(theme::PaletteColor::Accent),
      BackgroundColor::Palette(theme::PaletteColor::Accent),
      theme::PaletteColor::TextInverse,
      theme::palette().text_inverse,
      BackgroundColor::Palette(theme::PaletteColor::AccentHover),
      true,
    ),
    ButtonTone::Trust => (
      BackgroundColor::Color(Color::from_hex("#00000000")),
      BackgroundColor::Color(Color::from_hex("#FF6B5F80")),
      theme::PaletteColor::Danger,
      Color::from_hex(DANGER),
      BackgroundColor::Color(Color::from_hex("#2A1A1C")),
      false,
    ),
  };
  let mut button = Row::new()
    .height(42.0)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .spacing(8.0)
    .padding_horizontal(20.0)
    .rounded(5.0)
    .background(background)
    .border_inside(1.0, border)
    .cursor(if disabled {
      CursorIcon::Default
    } else {
      CursorIcon::Pointer
    })
    .child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon,
      size: 16.0,
      color: icon_color,
    }))
    .child(
      Text::new(label)
        .variant(theme::TypographyStyle::Button)
        .color(text_color),
    );

  if !disabled {
    button = button
      .hovered_style(Style::new().background(hover_background.clone()))
      .active_style(Style::new().background(hover_background));
  }
  if flex {
    button = button.flex(1.0);
  }
  button
}

async fn trust_certificate(
  storage: Storage,
  servers_store: Option<Store<Vec<StoredServer>>>,
  session: ServerSession,
  warning: TofuWarning,
) -> Result<(), String> {
  upsert_stored_server(
    servers_store.as_ref(),
    Some(&storage),
    StoredServer {
      address: warning.address,
      server_name: warning.server_name,
      user_id: warning.user_id,
      role: warning.role,
      certificate_fingerprint: warning.received_fingerprint,
      server_password: warning.server_password,
      display_name: warning.display_name,
    },
  )
  .map_err(|error| error.to_string())?;
  session.clear_tofu_warning();
  Ok(())
}
