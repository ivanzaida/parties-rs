use lurq::{
  app::{component::Component, ctx::Ctx},
  components::{Column, Row, Text, TextInput},
  core::Signal,
  layout::{Alignment, layout_kind::Justify},
  node::{BackgroundColor, CursorIcon, Element, Style, color::Color, dimension::Dimension},
};

use crate::{
  identity::{restore_seed_phrase, validate_seed_phrase},
  routes::{ROUTE_CHOOSE_SERVER, ROUTE_IDENTITY_SETUP},
  storage::Storage,
  theme,
  ui::{
    common::lucide_icon::{LucideIcon, LucideIconProps},
    onboarding_shell::{self, OnboardingIntroCopy},
  },
};

const PREVIEW_PHRASE: &str = "harbor velvet cinder oblige walnut tidal render summon pivot acorn freight murmur";

pub struct RestoreIdentityScreen {
  phrase: Signal<String>,
}

impl Component for RestoreIdentityScreen {
  type Props = ();

  fn create(ctx: &mut Ctx) -> Self {
    Self {
      phrase: ctx.signal(PREVIEW_PHRASE.to_owned()),
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    onboarding_shell::screen(
      onboarding_shell::intro(
        ctx,
        OnboardingIntroCopy {
          app_name: &ctx.t("common.app_name"),
          headline: &ctx.t("identity_restore.intro.headline"),
          description: &ctx.t("identity_restore.intro.description"),
          footer_note: &ctx.t("identity_restore.intro.footer"),
        },
      ),
      onboarding_shell::panel(
        Column::new()
          .width(Dimension::Pct(100.0))
          .spacing(theme::SpacingSize::Xl)
          .child(panel_header(
            &ctx.t("identity_restore.overline"),
            &ctx.t("identity_restore.title"),
            &ctx.t("identity_restore.subtitle"),
          ))
          .child(self.field(ctx))
          .child(self.actions(ctx)),
      ),
    )
  }
}

impl RestoreIdentityScreen {
  fn field(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let phrase = self.phrase.get();
    let word_count = phrase.split_whitespace().count();
    let hint = if validate_seed_phrase(&phrase).is_ok() {
      ctx.t("identity_restore.hint.valid")
    } else {
      ctx.t_args("identity_restore.hint.count", [("count", word_count.to_string())])
    };

    Column::new()
      .width(Dimension::Pct(100.0))
      .spacing(theme::SpacingSize::Sm)
      .child(
        Text::new(&ctx.t("identity_restore.field_label"))
          .variant(theme::TypographyStyle::FieldLabel)
          .color(theme::PaletteColor::TextMuted),
      )
      .child(
        TextInput::styled(self.phrase.clone(), ctx.theme().typography().mono.clone())
          .width(Dimension::Pct(100.0))
          .height(108.0)
          .padding_vertical(theme::SpacingSize::Lg)
          .padding_horizontal(theme::SpacingSize::Xl)
          .rounded(theme::RadiusSize::Lg)
          .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceInput))
          .border_inside(1.0, theme::PaletteColor::Border)
          .focused_style(Style::new().border_inside(1.0, theme::PaletteColor::Accent))
          .caret_color(theme::PaletteColor::Accent)
          .multiline(),
      )
      .child(
        Text::new(&hint)
          .variant(theme::TypographyStyle::Mono)
          .color(theme::PaletteColor::TextMuted),
      )
  }

  fn actions(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let navigator = ctx.navigator();
    let back_navigator = navigator.clone();
    let restore_navigator = navigator;
    let storage = ctx.use_context::<Storage>();
    let phrase = self.phrase.clone();

    Row::new()
      .width(Dimension::Pct(100.0))
      .align_items(Alignment::Center)
      .justify(Justify::SpaceBetween)
      .child(
        action_button(
          ctx,
          "arrow-left",
          &ctx.t("identity_restore.action.back"),
          ButtonTone::Ghost,
        )
        .on_click(move |_| {
          if let Some(navigator) = back_navigator.as_ref() {
            navigator.replace(ROUTE_IDENTITY_SETUP);
          }
        }),
      )
      .child(
        action_button(
          ctx,
          "arrow-right",
          &ctx.t("identity_restore.action.restore"),
          ButtonTone::Primary,
        )
        .on_click(move |_| {
          let Ok(identity) = restore_seed_phrase(&phrase.get_untracked()) else {
            return;
          };
          if let Some(storage) = storage.as_ref() {
            let _ = storage.save_identity(&identity);
          }
          if let Some(navigator) = restore_navigator.as_ref() {
            navigator.replace(ROUTE_CHOOSE_SERVER);
          }
        }),
      )
  }
}

fn panel_header(overline: &str, title: &str, subtitle: &str) -> Column {
  Column::new()
    .width(Dimension::Pct(100.0))
    .spacing(theme::SpacingSize::Md)
    .child(
      Text::new(overline)
        .variant(theme::TypographyStyle::Caption)
        .color(theme::PaletteColor::Accent),
    )
    .child(Text::new(title).variant(theme::TypographyStyle::Title))
    .child(
      Text::new(subtitle)
        .variant(theme::TypographyStyle::Description)
        .width(430.0),
    )
}

#[derive(Clone, Copy)]
enum ButtonTone {
  Primary,
  Ghost,
}

fn action_button(ctx: &mut Ctx, icon: &'static str, label: &str, tone: ButtonTone) -> Row {
  let (background, border, text_color, icon_color, hover_background) = match tone {
    ButtonTone::Primary => (
      BackgroundColor::Palette(theme::PaletteColor::Accent),
      BackgroundColor::Palette(theme::PaletteColor::Accent),
      theme::PaletteColor::TextInverse,
      theme::palette().text_inverse,
      BackgroundColor::Palette(theme::PaletteColor::AccentHover),
    ),
    ButtonTone::Ghost => (
      BackgroundColor::Color(Color::from_hex("#00000000")),
      BackgroundColor::Color(Color::from_hex("#00000000")),
      theme::PaletteColor::TextSecondary,
      theme::palette().text_secondary,
      BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised),
    ),
  };

  let mut button = Row::new()
    .height(34.0)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .spacing(theme::SpacingSize::Sm)
    .padding_horizontal(theme::SpacingSize::Lg)
    .rounded(theme::RadiusSize::Md)
    .background(background)
    .border_inside(1.0, border)
    .cursor(CursorIcon::Pointer)
    .hovered_style(Style::new().background(hover_background.clone()))
    .active_style(Style::new().background(hover_background));

  if matches!(tone, ButtonTone::Ghost) {
    button = button
      .child(ctx.mount::<LucideIcon>(LucideIconProps {
        icon,
        size: 16.0,
        color: icon_color,
      }))
      .child(
        Text::new(label)
          .variant(theme::TypographyStyle::Button)
          .color(text_color),
      );
  } else {
    button = button
      .child(
        Text::new(label)
          .variant(theme::TypographyStyle::Button)
          .color(text_color),
      )
      .child(ctx.mount::<LucideIcon>(LucideIconProps {
        icon,
        size: 16.0,
        color: icon_color,
      }));
  }

  button
}
