use std::time::{SystemTime, UNIX_EPOCH};

use lurq::{
  app::{component::Component, ctx::Ctx},
  components::{Column, Rect, Row, Text},
  layout::{Alignment, layout_kind::Justify, text_style::FontWeight},
  node::{BackgroundColor, Element, dimension::Dimension},
};

use crate::{
  screens::shared::{self, CARD_WIDTH, INTRO_WIDTH, styled_text},
  theme,
};

fn pulse_opacity(index: u64) -> f32 {
  let tick = SystemTime::now()
    .duration_since(UNIX_EPOCH)
    .map(|duration| duration.as_millis() as u64 / 180)
    .unwrap_or(0);

  if tick % 3 == index { 1.0 } else { 0.32 }
}

fn loader_dots() -> Row {
  Row::new()
    .spacing(8.0)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .child(
      Rect::new(10.0, 10.0)
        .rounded(5.0)
        .background(BackgroundColor::Palette(theme::PaletteColor::Accent))
        .opacity(pulse_opacity(0)),
    )
    .child(
      Rect::new(10.0, 10.0)
        .rounded(5.0)
        .background(BackgroundColor::Palette(theme::PaletteColor::Accent))
        .opacity(pulse_opacity(1)),
    )
    .child(
      Rect::new(10.0, 10.0)
        .rounded(5.0)
        .background(BackgroundColor::Palette(theme::PaletteColor::Accent))
        .opacity(pulse_opacity(2)),
    )
}

fn meta_card(title: &str, body: &str) -> Column {
  Column::new()
    .width(INTRO_WIDTH)
    .spacing(8.0)
    .padding(12.0)
    .rounded(6.0)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfacePanel))
    .border_inside(1.0, theme::PaletteColor::Border)
    .child(shared::dot(BackgroundColor::Palette(theme::PaletteColor::Accent)))
    .child(Text::new(title).variant(theme::TypographyStyle::Button))
    .child(Text::new(body).variant(theme::TypographyStyle::Link))
}

fn status_row(label: &str, detail: &str, active: bool) -> Row {
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

  Row::new()
    .width(Dimension::Pct(100.0))
    .align_items(Alignment::Center)
    .spacing(10.0)
    .padding(10.0)
    .rounded(6.0)
    .background(background)
    .border_inside(1.0, border)
    .child(shared::dot(dot))
    .child(
      Column::new()
        .flex(1.0)
        .spacing(3.0)
        .child(styled_text(
          label,
          "Inter",
          12.0,
          FontWeight::Bold,
          theme::palette().text_primary,
          1.2,
        ))
        .child(styled_text(
          detail,
          "Inter",
          10.0,
          FontWeight::Normal,
          theme::palette().text_secondary,
          1.2,
        )),
    )
}

pub struct LoadingScreen;

impl Component for LoadingScreen {
  type Props = ();

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    shared::identity_screen(
      Column::new()
        .width(INTRO_WIDTH)
        .spacing(18.0)
        .child(Text::new(&ctx.t("loading.caption")).variant(theme::TypographyStyle::Caption))
        .child(Text::new(&ctx.t("loading.title")).variant(theme::TypographyStyle::Title))
        .child(Text::new(&ctx.t("loading.desc")).variant(theme::TypographyStyle::Description))
        .child(meta_card(&ctx.t("loading.meta_title"), &ctx.t("loading.meta_desc"))),
      Column::new()
        .width(CARD_WIDTH)
        .spacing(14.0)
        .padding(18.0)
        .rounded(8.0)
        .background(BackgroundColor::Palette(theme::PaletteColor::SurfacePanel))
        .border_inside(1.0, theme::PaletteColor::Border)
        .child(Text::new(&ctx.t("loading.card_title")).variant(theme::TypographyStyle::Heading))
        .child(
          Column::new()
            .width(Dimension::Pct(100.0))
            .align_items(Alignment::Center)
            .spacing(16.0)
            .padding_vertical(18.0)
            .child(loader_dots())
            .child(
              Column::new()
                .width(Dimension::Pct(100.0))
                .align_items(Alignment::Center)
                .spacing(6.0)
                .child(styled_text(
                  &ctx.t("loading.status.title"),
                  "Inter",
                  14.0,
                  FontWeight::Bold,
                  theme::palette().text_primary,
                  1.2,
                ))
                .child(styled_text(
                  &ctx.t("loading.status.desc"),
                  "Inter",
                  11.0,
                  FontWeight::Normal,
                  theme::palette().text_secondary,
                  1.2,
                )),
            )
            .child(
              Column::new()
                .width(Dimension::Pct(100.0))
                .spacing(8.0)
                .child(status_row(
                  &ctx.t("loading.identity.title"),
                  &ctx.t("loading.identity.desc"),
                  true,
                ))
                .child(status_row(
                  &ctx.t("loading.servers.title"),
                  &ctx.t("loading.servers.desc"),
                  false,
                )),
            ),
        ),
    )
  }
}
