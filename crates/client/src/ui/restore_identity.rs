use lurq::{
  app::{component::Component, ctx::Ctx},
  components::{Column, Row, Text, TextInput},
  core::{Signal, Store},
  layout::{Alignment, layout_kind::Justify},
  node::{BackgroundColor, CursorIcon, Element, Style, color::Color, dimension::Dimension},
};

use crate::{
  identity::{LocalIdentity, first_invalid_seed_word, restore_seed_phrase, validate_seed_phrase},
  routes::{ROUTE_CHOOSE_SERVER, ROUTE_IDENTITY_SETUP},
  storage::{Storage, save_local_identity},
  theme,
  ui::{
    common::lucide_icon::{LucideIcon, LucideIconProps},
    onboarding_shell::{self, OnboardingIntroCopy},
  },
};

pub struct RestoreIdentityScreen {
  phrase: Signal<String>,
}

impl Component for RestoreIdentityScreen {
  type Props = ();

  fn create(ctx: &mut Ctx) -> Self {
    Self {
      phrase: ctx.signal(String::new()),
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
        ctx.breakpoint(),
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
    let invalid_word = (word_count == 12).then(|| first_invalid_seed_word(&phrase)).flatten();
    let mut placeholder_style = ctx.theme().typography().mono.clone();
    placeholder_style.color = theme::palette().text_muted.with_opacity(0.55);
    let hint = if invalid_word.is_some() {
      None
    } else if validate_seed_phrase(&phrase).is_ok() {
      Some(ctx.t("identity_restore.hint.valid"))
    } else {
      Some(ctx.t_args("identity_restore.hint.count", [("count", word_count.to_string())]))
    };
    let input_background = if invalid_word.is_some() {
      BackgroundColor::Palette(theme::PaletteColor::DangerMuted)
    } else {
      BackgroundColor::Palette(theme::PaletteColor::SurfaceInput)
    };
    let input_border = if invalid_word.is_some() {
      theme::PaletteColor::Danger
    } else {
      theme::PaletteColor::Border
    };

    let mut field = Column::new()
      .width(Dimension::Pct(100.0))
      .spacing(theme::SpacingSize::Sm)
      .child(
        Text::new(&ctx.t("identity_restore.field_label"))
          .variant(theme::TypographyStyle::FieldLabel)
          .color(theme::PaletteColor::TextMuted),
      )
      .child(
        TextInput::styled(self.phrase.clone(), ctx.theme().typography().mono.clone())
          .placeholder(&ctx.t("identity_restore.placeholder"))
          .placeholder_style(placeholder_style)
          .width(Dimension::Pct(100.0))
          .height(108.0)
          .padding_vertical(theme::SpacingSize::Lg)
          .padding_horizontal(theme::SpacingSize::Xl)
          .rounded(theme::RadiusSize::Lg)
          .background(input_background)
          .border_inside(1.0, input_border.clone())
          .caret_color(theme::PaletteColor::Accent)
          .multiline(),
      );

    if let Some((index, word)) = invalid_word {
      field = field.child(error_row(
        ctx,
        &ctx.t_args(
          "identity_restore.error.invalid_word",
          [("index", index.to_string()), ("word", word)],
        ),
      ));
    } else if let Some(hint) = hint {
      field = field.child(
        Text::new(&hint)
          .variant(theme::TypographyStyle::Mono)
          .color(theme::PaletteColor::TextMuted),
      );
    }

    field
  }

  fn actions(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let navigator = ctx.navigator();
    let back_navigator = navigator.clone();
    let restore_navigator = navigator;
    let storage = ctx.use_context::<Storage>();
    let identity_store = ctx.use_context::<Store<Option<LocalIdentity>>>();
    let phrase = self.phrase.clone();
    let can_restore = validate_seed_phrase(&phrase.get()).is_ok();
    let restore_button = action_button(
      ctx,
      "arrow-right",
      &ctx.t("identity_restore.action.restore"),
      if can_restore {
        ButtonTone::Primary
      } else {
        ButtonTone::Disabled
      },
    );
    let restore_button = if can_restore {
      restore_button.on_click(move |_| {
        let Ok(identity) = restore_seed_phrase(&phrase.get_untracked()) else {
          return;
        };
        let _ = save_local_identity(identity_store.as_ref(), storage.as_ref(), identity);
        if let Some(navigator) = restore_navigator.as_ref() {
          navigator.replace(ROUTE_CHOOSE_SERVER);
        }
      })
    } else {
      restore_button
    };

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
      .child(restore_button)
  }
}

fn error_row(ctx: &mut Ctx, message: &str) -> impl Into<Element> {
  Row::new()
    .align_items(Alignment::Center)
    .spacing(theme::SpacingSize::Sm)
    .child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon: "triangle-alert",
      size: 14.0,
      color: theme::palette().danger,
    }))
    .child(
      Text::new(message)
        .variant(theme::TypographyStyle::Description)
        .color(theme::PaletteColor::Danger),
    )
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
        .width(Dimension::Pct(100.0))
        .max_width(430.0),
    )
}

#[derive(Clone, Copy)]
enum ButtonTone {
  Primary,
  Ghost,
  Disabled,
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
    ButtonTone::Disabled => (
      BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised),
      BackgroundColor::Color(Color::from_hex("#00000000")),
      theme::PaletteColor::TextMuted,
      theme::palette().text_muted,
      BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised),
    ),
  };
  let enabled = !matches!(tone, ButtonTone::Disabled);

  let mut button = Row::new()
    .height(34.0)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .spacing(theme::SpacingSize::Sm)
    .padding_horizontal(theme::SpacingSize::Lg)
    .rounded(theme::RadiusSize::Md)
    .background(background)
    .border_inside(1.0, border)
    .cursor(if enabled {
      CursorIcon::Pointer
    } else {
      CursorIcon::NotAllowed
    });
  if enabled {
    button = button
      .hovered_style(Style::new().background(hover_background.clone()))
      .active_style(Style::new().background(hover_background));
  }

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
