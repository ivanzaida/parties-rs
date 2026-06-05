use lurq::{
  app::{component::Component, ctx::Ctx},
  components::{Column, Row, Spacer, Text},
  layout::Alignment,
  node::{BackgroundColor, CursorIcon, Element, Style, dimension::Dimension},
  router::Navigator,
};

use crate::{
  screens::shared::{
    self, CARD_WIDTH, INTRO_WIDTH, ROUTE_IMPORT_PRIVATE_KEY, ROUTE_RESTORE_IDENTITY, ROUTE_SEED_PHRASE,
  },
  storage::Storage,
  theme,
};

fn option_row(title: &str, desc: &str, active: bool, navigator: Option<Navigator>, target_route: &'static str) -> Row {
  let bg = if active {
    BackgroundColor::Palette(theme::PaletteColor::SuccessMuted)
  } else {
    BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised)
  };
  let stroke = if active {
    theme::PaletteColor::Accent
  } else {
    theme::PaletteColor::Border
  };
  let state_color = if active {
    theme::palette().accent
  } else {
    theme::palette().text_muted
  };

  let row = Row::new()
    .width(Dimension::Pct(100.0))
    .align_items(Alignment::Center)
    .spacing(12.0)
    .padding(12.0)
    .rounded(6.0)
    .background(bg)
    .border_inside(1.0, stroke)
    .child(shared::dot(state_color))
    .child(
      Column::new()
        .flex(1.0)
        .spacing(3.0)
        .child(Text::new(title).variant(theme::TypographyStyle::Button))
        .child(Text::new(desc).variant(theme::TypographyStyle::Link)),
    )
    .child(shared::icon("chevron-right", 14.0, state_color));

  if let Some(navigator) = navigator {
    row
      .cursor(CursorIcon::Pointer)
      .hovered_style(Style::new().background(theme::PaletteColor::SurfaceInput))
      .on_click(move |_| navigator.push(target_route))
  } else {
    row
  }
}

pub struct IdentitySetup;

impl Component for IdentitySetup {
  type Props = ();

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let navigator = ctx.navigator();
    let storage_available = ctx.use_context::<Storage>().is_some();
    let action_navigator = if storage_available { navigator.clone() } else { None };
    let storage_note = if storage_available {
      ctx.t("identity.setup.storage_note")
    } else {
      ctx.t("identity.setup.storage_unavailable")
    };
    let storage_note_bg = if storage_available {
      BackgroundColor::Palette(theme::PaletteColor::SuccessMuted)
    } else {
      BackgroundColor::Palette(theme::PaletteColor::DangerMuted)
    };
    let storage_note_border = if storage_available {
      theme::PaletteColor::BorderStrong
    } else {
      theme::PaletteColor::Danger
    };
    let storage_note_icon = if storage_available {
      "shield-check"
    } else {
      "alert-triangle"
    };
    let storage_note_icon_color = if storage_available {
      theme::palette().accent
    } else {
      theme::palette().danger
    };

    shared::identity_screen(
      Column::new()
        .width(INTRO_WIDTH)
        .spacing(18.0)
        .child(Text::new(&ctx.t("identity.setup.caption")).variant(theme::TypographyStyle::Caption))
        .child(Text::new(&ctx.t("identity.setup.title")).variant(theme::TypographyStyle::Title))
        .child(Text::new(&ctx.t("identity.setup.desc")).variant(theme::TypographyStyle::Description))
        .child(
          Column::new()
            .width(INTRO_WIDTH)
            .spacing(8.0)
            .padding(12.0)
            .rounded(6.0)
            .background(BackgroundColor::Palette(theme::PaletteColor::SurfacePanel))
            .border_inside(1.0, theme::PaletteColor::Border)
            .child(shared::dot(theme::palette().warning))
            .child(Text::new(&ctx.t("identity.setup.meta_title")).variant(theme::TypographyStyle::Button))
            .child(Text::new(&ctx.t("identity.setup.meta_desc")).variant(theme::TypographyStyle::Link)),
        ),
      Column::new()
        .width(CARD_WIDTH)
        .spacing(14.0)
        .padding(18.0)
        .rounded(8.0)
        .background(BackgroundColor::Palette(theme::PaletteColor::SurfacePanel))
        .border_inside(1.0, theme::PaletteColor::Border)
        .child(Text::new(&ctx.t("identity.setup.heading")).variant(theme::TypographyStyle::Heading))
        .child(option_row(
          &ctx.t("identity.setup.option.generate_title"),
          &ctx.t("identity.setup.option.generate_desc"),
          storage_available,
          action_navigator.clone(),
          ROUTE_SEED_PHRASE,
        ))
        .child(option_row(
          &ctx.t("identity.setup.option.restore_title"),
          &ctx.t("identity.setup.option.restore_desc"),
          false,
          action_navigator.clone(),
          ROUTE_RESTORE_IDENTITY,
        ))
        .child(option_row(
          &ctx.t("identity.setup.option.import_title"),
          &ctx.t("identity.setup.option.import_desc"),
          false,
          action_navigator,
          ROUTE_IMPORT_PRIVATE_KEY,
        ))
        .child(
          Row::new()
            .width(Dimension::Pct(100.0))
            .align_items(Alignment::Center)
            .spacing(8.0)
            .padding(10.0)
            .rounded(5.0)
            .background(storage_note_bg)
            .border_inside(1.0, storage_note_border)
            .child(shared::icon(storage_note_icon, 14.0, storage_note_icon_color))
            .child(Text::new(&storage_note).variant(theme::TypographyStyle::Link))
            .child(Spacer::new()),
        ),
    )
  }
}
