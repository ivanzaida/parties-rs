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
    .child(shared::dot("#FF6B5F"))
    .child(Text::new(title).variant(theme::TYP_BUTTON))
    .child(Text::new(description).variant(theme::TYP_LINK))
}

pub struct ImportPrivateKey {
  form: FormHandle,
}

impl Component for ImportPrivateKey {
  type Props = ();

  fn create(ctx: &mut Ctx) -> Self {
    Self {
      form: ctx.form(
        FormOptions::new()
          .field("private_key", "")
          .validate_string("private_key", |private_key, _| {
            if identity::import_private_key_hex(private_key).is_ok() {
              ValidationResult::valid()
            } else {
              ValidationResult::invalid("Invalid private key. Must be 64 hex characters.")
            }
          }),
      ),
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let navigator = ctx.navigator();
    let storage = ctx.use_context::<Storage>();
    let warning = ctx.t("identity.import.warning");
    let private_key_error = self.form.error("private_key").get();
    eprintln!("import_private_key render errors={:?}", self.form.errors());
    eprintln!("import_private_key private_key_error={private_key_error:?}");

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
      ctx.form_view_with(
        FormProps::new({
          let navigator = navigator.clone();
          let storage = storage.clone();
          self
            .form
            .clone()
            .on_submit(move |values| {
              let private_key = values.get_string("private_key").unwrap_or_default();
              eprintln!("import_private_key submit private_key_len={}", private_key.len());
              if let Ok(identity) = identity::import_private_key_hex(private_key) {
                match &storage {
                  Some(storage) if storage.save_identity(&identity).is_ok() => {
                    eprintln!("import_private_key submit saved identity");
                    if let Some(navigator) = &navigator {
                      navigator.replace(ROUTE_CHOOSE_SERVER);
                    }
                  }
                  _ => eprintln!("import_private_key submit failed to save identity"),
                }
              }
            })
            .on_invalid(|errors| {
              eprintln!("import_private_key invalid errors={errors:?}");
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
            .child(Text::new(&ctx.t("identity.import.heading")).variant(theme::TYP_HEADING))
            .child(ctx.mount_keyed::<shared::FormTextInput>(
              if private_key_error.is_some() {
                "private_key-invalid"
              } else {
                "private_key-valid"
              },
              shared::FormTextInputProps::new(
                self.form.string_control("private_key"),
                ctx.t("identity.import.field_label"),
                ctx.t("identity.import.placeholder"),
                40.0,
              ),
            ))
            .child(shared::notice_row(
              &warning,
              "alert-triangle",
              "#FF6B5F",
              "#2B1715",
              "#4A2A27",
            ))
            .child(shared::submit_action_button(&ctx.t("identity.action.import_key"), true))
            .child(shared::back_button(navigator.clone(), &ctx.t("identity.action.back")))
        },
      ),
    )
  }
}
