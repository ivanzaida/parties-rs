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
    .child(shared::dot(BackgroundColor::Palette(theme::PaletteColor::Success)))
    .child(Text::new(title).variant(theme::TypographyStyle::Button))
    .child(Text::new(description).variant(theme::TypographyStyle::Link))
}

pub struct RestoreIdentity {
  form: FormHandle,
}

impl Component for RestoreIdentity {
  type Props = ();

  fn create(ctx: &mut Ctx) -> Self {
    let invalid_seed_phrase = ctx.t("identity.restore.status.invalid");

    Self {
      form: ctx.form(FormOptions::new().field("seed_phrase", "").validate_string(
        "seed_phrase",
        move |seed_phrase, _| {
          if identity::restore_seed_phrase(seed_phrase).is_ok() {
            ValidationResult::valid()
          } else {
            ValidationResult::invalid(invalid_seed_phrase.clone())
          }
        },
      )),
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let navigator = ctx.navigator();
    let storage = ctx.use_context::<Storage>();
    let help = ctx.t("identity.restore.help");
    let seed_phrase_error = self.form.error("seed_phrase").get();

    shared::identity_screen(
      Column::new()
        .width(INTRO_WIDTH)
        .spacing(18.0)
        .child(Text::new(&ctx.t("identity.restore.caption")).variant(theme::TypographyStyle::Caption))
        .child(Text::new(&ctx.t("identity.restore.title")).variant(theme::TypographyStyle::Title))
        .child(Text::new(&ctx.t("identity.restore.desc")).variant(theme::TypographyStyle::Description))
        .child(meta_card(
          &ctx.t("identity.restore.meta_title"),
          &ctx.t("identity.restore.meta_desc"),
        )),
      ctx.form_view_with(
        FormProps::new({
          let navigator = navigator.clone();
          let storage = storage.clone();
          self.form.clone().on_submit(move |values| {
            let seed_phrase = values.get_string("seed_phrase").unwrap_or_default();
            if let Ok(identity) = identity::restore_seed_phrase(seed_phrase)
              && let Some(storage) = &storage
              && storage.save_identity(&identity).is_ok()
              && let Some(navigator) = &navigator
            {
              navigator.replace(ROUTE_CHOOSE_SERVER);
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
            .child(Text::new(&ctx.t("identity.restore.heading")).variant(theme::TypographyStyle::Heading))
            .child(
              ctx.mount_keyed::<shared::FormTextInput>(
                if seed_phrase_error.is_some() {
                  "seed_phrase-invalid"
                } else {
                  "seed_phrase-valid"
                },
                shared::FormTextInputProps::new(self.form.string_control("seed_phrase"))
                  .label(ctx.t("identity.restore.field_label"))
                  .placeholder(ctx.t("identity.restore.placeholder"))
                  .height(82.0)
                  .multiline(),
              ),
            )
            .child(shared::notice_row(
              &help,
              "info",
              theme::palette().warning,
              theme::PaletteColor::SurfaceRaised,
              theme::PaletteColor::Border,
            ))
            .child(
              ctx.mount::<shared::FormPrimaryButton>(shared::FormPrimaryButtonProps::new(
                ctx.t("identity.action.restore"),
              )),
            )
            .child(shared::back_button(navigator.clone(), &ctx.t("identity.action.back")))
        },
      ),
    )
  }
}
