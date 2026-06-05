use lurq::{
  app::{component::Component, ctx::Ctx},
  components::{Column, FormHandle, FormOptions, FormProps, Text, ValidationResult},
  node::{BackgroundColor, Element},
};

use crate::{
  identity,
  screens::shared::{self, CARD_WIDTH, INTRO_WIDTH, ROUTE_CHOOSE_SERVER},
  storage::Storage,
  theme,
};

fn meta_card(title: &str, description: &str) -> Column {
  Column::new()
    .width(INTRO_WIDTH)
    .spacing(8.0)
    .padding(12.0)
    .rounded(6.0)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfacePanel))
    .border_inside(1.0, theme::PaletteColor::Border)
    .child(shared::dot(theme::palette().danger))
    .child(Text::new(title).variant(theme::TypographyStyle::Button))
    .child(Text::new(description).variant(theme::TypographyStyle::Link))
}

pub struct ImportPrivateKey {
  form: FormHandle,
}

impl Component for ImportPrivateKey {
  type Props = ();

  fn create(ctx: &mut Ctx) -> Self {
    let invalid_private_key = ctx.t("identity.import.status.invalid");

    Self {
      form: ctx.form(FormOptions::new().field("private_key", "").validate_string(
        "private_key",
        move |private_key, _| {
          if identity::import_private_key_hex(private_key).is_ok() {
            ValidationResult::valid()
          } else {
            ValidationResult::invalid(invalid_private_key.clone())
          }
        },
      )),
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let navigator = ctx.navigator();
    let storage = ctx.use_context::<Storage>();
    let warning = ctx.t("identity.import.warning");
    let private_key_error = self.form.error("private_key").get();

    shared::identity_screen(
      Column::new()
        .width(INTRO_WIDTH)
        .spacing(18.0)
        .child(Text::new(&ctx.t("identity.import.caption")).variant(theme::TypographyStyle::Caption))
        .child(Text::new(&ctx.t("identity.import.title")).variant(theme::TypographyStyle::Title))
        .child(Text::new(&ctx.t("identity.import.desc")).variant(theme::TypographyStyle::Description))
        .child(meta_card(
          &ctx.t("identity.import.meta_title"),
          &ctx.t("identity.import.meta_desc"),
        )),
      ctx.form_view_with(
        FormProps::new({
          let navigator = navigator.clone();
          let storage = storage.clone();
          self.form.clone().on_submit(move |values| {
            let private_key = values.get_string("private_key").unwrap_or_default();
            if let Ok(identity) = identity::import_private_key_hex(private_key) {
              if let Some(storage) = &storage
                && storage.save_identity(&identity).is_ok()
                && let Some(navigator) = &navigator
              {
                navigator.replace(ROUTE_CHOOSE_SERVER);
              }
            }
          })
        }),
        |ctx| {
          Column::new()
            .width(CARD_WIDTH)
            .spacing(14.0)
            .padding(18.0)
            .rounded(8.0)
            .background(BackgroundColor::Palette(theme::PaletteColor::SurfacePanel))
            .border_inside(1.0, theme::PaletteColor::Border)
            .child(Text::new(&ctx.t("identity.import.heading")).variant(theme::TypographyStyle::Heading))
            .child(
              ctx.mount_keyed::<shared::FormTextInput>(
                if private_key_error.is_some() {
                  "private_key-invalid"
                } else {
                  "private_key-valid"
                },
                shared::FormTextInputProps::new(self.form.string_control("private_key"))
                  .label(ctx.t("identity.import.field_label"))
                  .placeholder(ctx.t("identity.import.placeholder"))
                  .height(40.0),
              ),
            )
            .child(shared::notice_row(
              &warning,
              "alert-triangle",
              theme::palette().danger,
              theme::PaletteColor::DangerMuted,
              theme::PaletteColor::Danger,
            ))
            .child(
              ctx.mount::<shared::FormPrimaryButton>(shared::FormPrimaryButtonProps::new(
                ctx.t("identity.action.import_key"),
              )),
            )
            .child(shared::back_button(navigator.clone(), &ctx.t("identity.action.back")))
        },
      ),
    )
  }
}
