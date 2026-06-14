mod action_card;
mod notice;

use lurq::{
  app::{component::Component, ctx::Ctx},
  components::{Column, Text},
  node::{Element, dimension::Dimension},
};

use crate::{
  routes::{ROUTE_IMPORT_PRIVATE_KEY, ROUTE_RESTORE_IDENTITY, ROUTE_SEED_PHRASE},
  theme,
  ui::{
    identity_setup::{
      action_card::{IdentityActionCard, IdentityActionCardProps},
      notice::notice,
    },
    onboarding_shell::{self, OnboardingIntroCopy},
  },
};

pub struct IdentitySetupScreen;

impl Component for IdentitySetupScreen {
  type Props = ();

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    onboarding_shell::screen(
      onboarding_shell::intro(
        ctx,
        OnboardingIntroCopy {
          app_name: &ctx.t("common.app_name"),
          headline: &ctx.t("identity_setup.intro.headline"),
          description: &ctx.t("identity_setup.intro.description"),
          footer_note: &ctx.t("identity_setup.intro.footer"),
        },
      ),
      onboarding_shell::panel(
        ctx.breakpoint(),
        Column::new()
          .width(Dimension::Pct(100.0))
          .spacing(26.0)
          .child(header(
            &ctx.t("identity_setup.overline"),
            &ctx.t("identity_setup.title"),
            &ctx.t("identity_setup.subtitle"),
          ))
          .child(
            Column::new()
              .width(Dimension::Pct(100.0))
              .spacing(12.0)
              .child(ctx.mount_keyed::<IdentityActionCard>(
                "generate",
                IdentityActionCardProps {
                  icon: "sprout",
                  title: ctx.t("identity_setup.option.generate.title"),
                  description: ctx.t("identity_setup.option.generate.description"),
                  target_route: Some(ROUTE_SEED_PHRASE),
                },
              ))
              .child(ctx.mount_keyed::<IdentityActionCard>(
                "restore",
                IdentityActionCardProps {
                  icon: "refresh-cw",
                  title: ctx.t("identity_setup.option.restore.title"),
                  description: ctx.t("identity_setup.option.restore.description"),
                  target_route: Some(ROUTE_RESTORE_IDENTITY),
                },
              ))
              .child(ctx.mount_keyed::<IdentityActionCard>(
                "import",
                IdentityActionCardProps {
                  icon: "key",
                  title: ctx.t("identity_setup.option.import.title"),
                  description: ctx.t("identity_setup.option.import.description"),
                  target_route: Some(ROUTE_IMPORT_PRIVATE_KEY),
                },
              )),
          )
          .child(notice(ctx, &ctx.t("identity_setup.notice.no_identity"))),
      ),
    )
  }
}

fn header(overline: &str, title: &str, subtitle: &str) -> Column {
  Column::new()
    .width(Dimension::Pct(100.0))
    .spacing(10.0)
    .child(
      Text::new(overline)
        .variant(theme::TypographyStyle::Caption)
        .color(theme::PaletteColor::Accent),
    )
    .child(Text::new(title).variant(theme::TypographyStyle::Title))
    .child(
      Text::new(subtitle)
        .variant(theme::TypographyStyle::Description)
        .width(Dimension::Pct(100.0))
        .max_width(430.0),
    )
}
