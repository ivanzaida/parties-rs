use lurq::{
  app::{component::Component, ctx::Ctx},
  components::{Column, Row, Text},
  core::Signal,
  layout::{Alignment, layout_kind::Justify, text_style::FontWeight},
  node::{BackgroundColor, CursorIcon, Element, Style, dimension::Dimension},
};

use crate::{
  screens::shared::{self, CARD_WIDTH, INTRO_WIDTH, ROUTE_CHOOSE_SERVER, ROUTE_LOBBY, styled_text},
  session::ServerSession,
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
    .child(shared::dot(BackgroundColor::Palette(theme::PaletteColor::Danger)))
    .child(Text::new(title).variant(theme::TypographyStyle::Button))
    .child(Text::new(body).variant(theme::TypographyStyle::Link))
}

fn fingerprint_box(label: &str, value: &str, value_color: impl Into<lurq::node::color::Color>) -> Column {
  Column::new()
    .width(Dimension::Pct(100.0))
    .spacing(6.0)
    .padding(10.0)
    .rounded(5.0)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceInput))
    .border_inside(1.0, theme::PaletteColor::Border)
    .child(styled_text(
      label,
      "JetBrains Mono",
      10.0,
      FontWeight::Bold,
      theme::palette().text_muted,
      1.2,
    ))
    .child(styled_text(
      value,
      "JetBrains Mono",
      12.0,
      FontWeight::Bold,
      value_color,
      1.2,
    ))
}

fn secondary_action(label: &str) -> Row {
  Row::new()
    .width(Dimension::Pct(100.0))
    .height(34.0)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .rounded(5.0)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised))
    .border_inside(1.0, theme::PaletteColor::Border)
    .cursor(CursorIcon::Pointer)
    .hovered_style(Style::new().background(BackgroundColor::Palette(theme::PaletteColor::SurfaceInput)))
    .child(Text::new(label).variant(theme::TypographyStyle::Button))
}

fn danger_action(label: &str) -> Row {
  Row::new()
    .width(Dimension::Pct(100.0))
    .height(34.0)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .rounded(5.0)
    .background(BackgroundColor::Palette(theme::PaletteColor::DangerMuted))
    .border_inside(1.0, theme::PaletteColor::Danger)
    .cursor(CursorIcon::Pointer)
    .hovered_style(Style::new().background(BackgroundColor::Palette(theme::PaletteColor::DangerMuted)))
    .child(styled_text(
      label,
      "Inter",
      12.0,
      FontWeight::Bold,
      theme::palette().danger,
      1.2,
    ))
}

pub struct TofuWarningScreen {
  trust_failed: Signal<bool>,
}

impl Component for TofuWarningScreen {
  type Props = ();

  fn create(ctx: &mut Ctx) -> Self {
    Self {
      trust_failed: ctx.signal(false),
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let navigator = ctx.navigator();
    let storage = ctx.use_context::<Storage>();
    let session = ctx.use_context::<ServerSession>().unwrap_or_default();
    let warning = session.tofu_warning();
    if warning.is_none()
      && let Some(navigator) = navigator.as_ref()
    {
      navigator.replace(ROUTE_CHOOSE_SERVER);
    }

    let saved = warning
      .as_ref()
      .map(|warning| warning.saved_fingerprint.as_str())
      .unwrap_or("-");
    let received = warning
      .as_ref()
      .map(|warning| warning.received_fingerprint.as_str())
      .unwrap_or("-");
    let meta_title = if self.trust_failed.get() {
      ctx.t("tofu_warning.save_failed_title")
    } else {
      ctx.t("tofu_warning.meta_title")
    };
    let meta_body = if self.trust_failed.get() {
      ctx.t("tofu_warning.save_failed_desc")
    } else {
      ctx.t("tofu_warning.meta_desc")
    };

    shared::identity_screen(
      Column::new()
        .width(INTRO_WIDTH)
        .spacing(18.0)
        .child(Text::new(&ctx.t("tofu_warning.caption")).variant(theme::TypographyStyle::Caption))
        .child(Text::new(&ctx.t("tofu_warning.title")).variant(theme::TypographyStyle::Title))
        .child(Text::new(&ctx.t("tofu_warning.desc")).variant(theme::TypographyStyle::Description))
        .child(meta_card(&meta_title, &meta_body)),
      Column::new()
        .width(CARD_WIDTH)
        .spacing(14.0)
        .padding(18.0)
        .rounded(8.0)
        .background(BackgroundColor::Palette(theme::PaletteColor::SurfacePanel))
        .border_inside(1.0, theme::PaletteColor::Border)
        .child(Text::new(&ctx.t("tofu_warning.heading")).variant(theme::TypographyStyle::Heading))
        .child(fingerprint_box(
          &ctx.t("tofu_warning.saved_label"),
          saved,
          theme::palette().accent,
        ))
        .child(fingerprint_box(
          &ctx.t("tofu_warning.received_label"),
          received,
          theme::palette().danger,
        ))
        .child(
          Row::new()
            .width(Dimension::Pct(100.0))
            .spacing(10.0)
            .child({
              let button = secondary_action(&ctx.t("tofu_warning.action.disconnect"));
              let session = session.clone();
              let navigator = navigator.clone();
              button.on_click(move |_| {
                session.clear();
                if let Some(navigator) = &navigator {
                  navigator.replace(ROUTE_CHOOSE_SERVER);
                }
              })
            })
            .child({
              let button = danger_action(&ctx.t("tofu_warning.action.trust"));
              let storage = storage.clone();
              let session = session.clone();
              let navigator = navigator.clone();
              let failed = self.trust_failed.clone();
              button.on_click(move |_| {
                let Some(warning) = session.tofu_warning() else {
                  failed.set(true);
                  return;
                };
                let Some(storage) = storage.as_ref() else {
                  failed.set(true);
                  return;
                };
                let stored = StoredServer {
                  address: warning.address,
                  server_name: warning.server_name,
                  user_id: warning.user_id,
                  role: warning.role,
                  certificate_fingerprint: warning.received_fingerprint,
                };
                if storage.save_server(&stored).is_ok() {
                  failed.set(false);
                  session.clear_tofu_warning();
                  if let Some(navigator) = &navigator {
                    navigator.replace(ROUTE_LOBBY);
                  }
                } else {
                  failed.set(true);
                }
              })
            }),
        ),
    )
  }
}
