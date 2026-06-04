use lurq::{
  app::{component::Component, ctx::Ctx},
  components::{Column, Row, Spacer, Text},
  core::Signal,
  layout::Alignment,
  node::{BackgroundColor, CursorIcon, Element, Style, color::Color, dimension::Dimension},
};

use crate::{
  screens::shared::{
    self, BORDER, CARD_WIDTH, INTRO_WIDTH, STEP_IMPORT_PRIVATE_KEY, STEP_RESTORE_IDENTITY, STEP_SEED_PHRASE,
  },
  storage::Storage,
  theme,
};

fn option_row(title: &str, desc: &str, active: bool, step: Option<Signal<u8>>, target_step: u8) -> Row {
  let bg = if active {
    BackgroundColor::Palette(theme::GREEN_MUTED)
  } else {
    BackgroundColor::Palette(theme::BG_ELEVATED)
  };
  let stroke = if active { "#42D28B" } else { BORDER };
  let state_color = if active { "#42D28B" } else { "#7D766C" };

  let row = Row::new()
    .width(Dimension::Pct(100.0))
    .align_items(Alignment::Center)
    .spacing(12.0)
    .padding(12.0)
    .rounded(6.0)
    .background(bg)
    .border_inside(1.0, Color::from_hex(stroke))
    .child(shared::dot(state_color))
    .child(
      Column::new()
        .flex(1.0)
        .spacing(3.0)
        .child(Text::new(title).variant(theme::TYP_BUTTON))
        .child(Text::new(desc).variant(theme::TYP_LINK)),
    )
    .child(shared::icon("chevron-right", 14.0, state_color));

  if let Some(step) = step {
    row
      .cursor(CursorIcon::Pointer)
      .hovered_style(Style::new().background("#132D20"))
      .on_click(move |_| step.set(target_step))
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
    let step = ctx.use_context::<Signal<u8>>();
    let storage_available = ctx.use_context::<Storage>().is_some();
    let action_step = if storage_available { step.clone() } else { None };
    let storage_note = if storage_available {
      ctx.t("identity.setup.storage_note")
    } else {
      ctx.t("identity.setup.storage_unavailable")
    };
    let storage_note_bg = if storage_available {
      BackgroundColor::Palette(theme::GREEN_MUTED)
    } else {
      BackgroundColor::Palette(theme::RED_MUTED)
    };
    let storage_note_border = if storage_available {
      theme::BORDER_LIGHT_COLOR
    } else {
      Color::from_hex("#4A2A27")
    };
    let storage_note_icon = if storage_available {
      "shield-check"
    } else {
      "alert-triangle"
    };
    let storage_note_icon_color = if storage_available { "#42D28B" } else { "#FF6B5F" };

    shared::identity_screen(
      Column::new()
        .width(INTRO_WIDTH)
        .spacing(18.0)
        .child(Text::new(&ctx.t("identity.setup.caption")).variant(theme::TYP_CAPTION))
        .child(Text::new(&ctx.t("identity.setup.title")).variant(theme::TYP_TITLE))
        .child(Text::new(&ctx.t("identity.setup.desc")).variant(theme::TYP_DESC))
        .child(
          Column::new()
            .width(INTRO_WIDTH)
            .spacing(8.0)
            .padding(12.0)
            .rounded(6.0)
            .background(BackgroundColor::Palette(theme::BG_TERTIARY))
            .border_inside(1.0, Color::from_hex(BORDER))
            .child(shared::dot("#F2B84B"))
            .child(Text::new(&ctx.t("identity.setup.meta_title")).variant(theme::TYP_BUTTON))
            .child(Text::new(&ctx.t("identity.setup.meta_desc")).variant(theme::TYP_LINK)),
        ),
      Column::new()
        .width(CARD_WIDTH)
        .spacing(14.0)
        .padding(18.0)
        .rounded(8.0)
        .background(BackgroundColor::Palette(theme::BG_TERTIARY))
        .border_inside(1.0, Color::from_hex(BORDER))
        .child(Text::new(&ctx.t("identity.setup.heading")).variant(theme::TYP_HEADING))
        .child(option_row(
          &ctx.t("identity.setup.option.generate_title"),
          &ctx.t("identity.setup.option.generate_desc"),
          storage_available,
          action_step.clone(),
          STEP_SEED_PHRASE,
        ))
        .child(option_row(
          &ctx.t("identity.setup.option.restore_title"),
          &ctx.t("identity.setup.option.restore_desc"),
          false,
          action_step.clone(),
          STEP_RESTORE_IDENTITY,
        ))
        .child(option_row(
          &ctx.t("identity.setup.option.import_title"),
          &ctx.t("identity.setup.option.import_desc"),
          false,
          action_step,
          STEP_IMPORT_PRIVATE_KEY,
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
            .child(Text::new(&storage_note).variant(theme::TYP_LINK))
            .child(Spacer::new()),
        ),
    )
  }
}
