use lurq::{
  app::{component::Component, ctx::Ctx},
  components::{Column, Row, Text, TextInput},
  core::Signal,
  layout::{
    Alignment,
    text_style::{FontWeight, TextStyle},
  },
  node::{BackgroundColor, Element, color::Color, dimension::Dimension},
};

use crate::{
  identity,
  screens::shared::{self, BORDER, CARD_WIDTH, INTRO_WIDTH, STEP_CHOOSE_SERVER, action_button, text_style},
  storage::Storage,
  theme,
};

fn meta_card(title: &str, description: &str) -> Column {
  Column::new()
    .width(INTRO_WIDTH)
    .spacing(8.0)
    .padding(12.0)
    .rounded(6.0)
    .background(BackgroundColor::Palette(theme::BG_TERTIARY))
    .border_inside(1.0, Color::from_hex(BORDER))
    .child(shared::dot("#FF6B5F"))
    .child(Text::new(title).variant(theme::TYP_BUTTON))
    .child(Text::new(description).variant(theme::TYP_LINK))
}

fn private_key_field(value: Signal<String>, label: &str, placeholder: &str) -> Column {
  let value_style = text_style("JetBrains Mono", 12.0, FontWeight::Medium, "#F4F4F2", 1.2);
  let placeholder_style = TextStyle {
    color: Color::from_hex("#B7B2AA"),
    ..value_style.clone()
  };

  Column::new()
    .width(Dimension::Pct(100.0))
    .spacing(7.0)
    .child(shared::styled_text(
      label,
      "JetBrains Mono",
      10.0,
      FontWeight::Bold,
      "#7D766C",
      1.2,
    ))
    .child(
      TextInput::styled(value, value_style)
        .width(Dimension::Pct(100.0))
        .height(40.0)
        .padding_horizontal(10.0)
        .rounded(5.0)
        .background("#101215")
        .border_inside(1.0, Color::from_hex(BORDER))
        .caret_color(theme::ACCENT_COLOR)
        .placeholder(placeholder)
        .placeholder_style(placeholder_style)
        .single_line(),
    )
}

fn private_key_warning(message: &str, accepted: bool) -> Row {
  let (bg, border, icon_color) = if accepted {
    ("#111A14", "#2D4634", "#42D28B")
  } else {
    ("#2B1715", "#4A2A27", "#FF6B5F")
  };

  Row::new()
    .width(Dimension::Pct(100.0))
    .align_items(Alignment::Center)
    .spacing(8.0)
    .padding(10.0)
    .rounded(5.0)
    .background(bg)
    .border_inside(1.0, Color::from_hex(border))
    .child(shared::icon("alert-triangle", 14.0, icon_color))
    .child(Text::new(message).variant(theme::TYP_LINK).flex(1.0))
}

pub struct ImportPrivateKey {
  private_key: Signal<String>,
  status: Signal<String>,
  accepted: Signal<bool>,
}

impl Component for ImportPrivateKey {
  type Props = ();

  fn create(ctx: &mut Ctx) -> Self {
    Self {
      private_key: ctx.signal(String::new()),
      status: ctx.signal(String::new()),
      accepted: ctx.signal(false),
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let step = ctx.use_context::<Signal<u8>>();
    let storage = ctx.use_context::<Storage>();
    let status_key = self.status.get();
    let accepted = self.accepted.get();
    let warning = if status_key.is_empty() {
      ctx.t("identity.import.status.empty")
    } else {
      ctx.t(status_key.as_str())
    };

    shared::identity_screen(
      Column::new()
        .width(INTRO_WIDTH)
        .spacing(18.0)
        .child(Text::new(&ctx.t("identity.import.caption")).variant(theme::TYP_CAPTION))
        .child(Text::new(&ctx.t("identity.import.title")).variant(theme::TYP_TITLE))
        .child(Text::new(&ctx.t("identity.import.desc")).variant(theme::TYP_DESC))
        .child(meta_card(
          &ctx.t("identity.import.meta_title"),
          &ctx.t("identity.import.meta_desc"),
        )),
      Column::new()
        .width(CARD_WIDTH)
        .spacing(14.0)
        .padding(18.0)
        .rounded(8.0)
        .background(BackgroundColor::Palette(theme::BG_TERTIARY))
        .border_inside(1.0, Color::from_hex(BORDER))
        .child(Text::new(&ctx.t("identity.import.heading")).variant(theme::TYP_HEADING))
        .child(private_key_field(
          self.private_key.clone(),
          &ctx.t("identity.import.field_label"),
          &ctx.t("identity.import.placeholder"),
        ))
        .child(private_key_warning(&warning, accepted))
        .child({
          let private_key = self.private_key.clone();
          let status = self.status.clone();
          let accepted = self.accepted.clone();
          let next_step = step.clone();
          let storage = storage.clone();
          action_button(&ctx.t("identity.action.import_key"), true).on_click(move |_| {
            match identity::import_private_key_hex(&private_key.get()) {
              Ok(identity) => match &storage {
                Some(storage) if storage.save_identity(&identity).is_ok() => {
                  accepted.set(true);
                  status.set("identity.import.status.accepted".to_owned());
                  if let Some(next_step) = &next_step {
                    next_step.set(STEP_CHOOSE_SERVER);
                  }
                }
                _ => {
                  accepted.set(false);
                  status.set("identity.import.status.save_failed".to_owned());
                }
              },
              Err(_) => {
                accepted.set(false);
                status.set("identity.import.status.invalid".to_owned());
              }
            }
          })
        })
        .child(shared::back_button(step.clone(), &ctx.t("identity.action.back"))),
    )
  }
}
