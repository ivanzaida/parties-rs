use lurq::{
  app::{component::Component, ctx::Ctx},
  components::{Column, FormHandle, FormOptions, FormProps, Text, ValidationResult},
  node::{BackgroundColor, Element, color::Color},
};

use crate::{
  identity,
  screens::shared::{self, BORDER, CARD_WIDTH, INTRO_WIDTH, ROUTE_CHOOSE_SERVER},
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
    .child(shared::dot(BackgroundColor::Palette(theme::GREEN)))
    .child(Text::new(title).variant(theme::TYP_BUTTON))
    .child(Text::new(description).variant(theme::TYP_LINK))
}

pub struct RestoreIdentity {
  form: FormHandle,
}

impl Component for RestoreIdentity {
  type Props = ();

  fn create(ctx: &mut Ctx) -> Self {
    Self {
      form: ctx.form(
        FormOptions::new()
          .field("seed_phrase", "")
          .validate_string("seed_phrase", |seed_phrase, _| {
            if identity::restore_seed_phrase(seed_phrase).is_ok() {
              ValidationResult::valid()
            } else {
              ValidationResult::invalid("Invalid seed phrase. Enter 12 known words.")
            }
          }),
      ),
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let navigator = ctx.navigator();
    let storage = ctx.use_context::<Storage>();
    let help = ctx.t("identity.restore.help");
    let seed_phrase_error = self.form.error("seed_phrase").get();
    eprintln!("restore_identity render errors={:?}", self.form.errors());

    shared::identity_screen(
      Column::new()
        .width(INTRO_WIDTH)
        .spacing(18.0)
        .child(Text::new(&ctx.t("identity.restore.caption")).variant(theme::TYP_CAPTION))
        .child(Text::new(&ctx.t("identity.restore.title")).variant(theme::TYP_TITLE))
        .child(Text::new(&ctx.t("identity.restore.desc")).variant(theme::TYP_DESC))
        .child(meta_card(
          &ctx.t("identity.restore.meta_title"),
          &ctx.t("identity.restore.meta_desc"),
        )),
      ctx.form_view_with(
        FormProps::new({
          let navigator = navigator.clone();
          let storage = storage.clone();
          self
            .form
            .clone()
            .on_submit(move |values| {
              let seed_phrase = values.get_string("seed_phrase").unwrap_or_default();
              eprintln!("restore_identity submit seed_phrase_len={}", seed_phrase.len());
              match identity::restore_seed_phrase(seed_phrase) {
                Ok(identity) => match &storage {
                  Some(storage) if storage.save_identity(&identity).is_ok() => {
                    eprintln!("restore_identity submit saved identity");
                    if let Some(navigator) = &navigator {
                      navigator.replace(ROUTE_CHOOSE_SERVER);
                    }
                  }
                  _ => eprintln!("restore_identity submit failed to save identity"),
                },
                Err(_) => eprintln!("restore_identity submit failed to restore seed phrase"),
              }
            })
            .on_invalid(|errors| {
              eprintln!("restore_identity invalid errors={errors:?}");
            })
        }),
        |ctx| {
          Column::new()
            .width(CARD_WIDTH)
            .spacing(14.0)
            .padding(18.0)
            .rounded(8.0)
            .background(BackgroundColor::Palette(theme::BG_TERTIARY))
            .border_inside(1.0, Color::from_hex(BORDER))
            .child(Text::new(&ctx.t("identity.restore.heading")).variant(theme::TYP_HEADING))
            .child(
              ctx.mount_keyed::<shared::FormTextInput>(
                if seed_phrase_error.is_some() {
                  "seed_phrase-invalid"
                } else {
                  "seed_phrase-valid"
                },
                shared::FormTextInputProps::new(
                  self.form.string_control("seed_phrase"),
                  ctx.t("identity.restore.field_label"),
                  ctx.t("identity.restore.placeholder"),
                  82.0,
                )
                .multiline(),
              ),
            )
            .child(shared::notice_row(&help, "info", "#F2B84B", "#1B1E23", BORDER))
            .child(shared::submit_action_button(&ctx.t("identity.action.restore"), true))
            .child(shared::back_button(navigator.clone(), &ctx.t("identity.action.back")))
        },
      ),
    )
  }
}
