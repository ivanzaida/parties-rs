use lurq::{
  app::{component::Component, ctx::Ctx},
  components::{Column, Row, Text, TextInput},
  core::Signal,
  layout::{layout_kind::Justify, Alignment},
  node::{color::Color, dimension::Dimension, BackgroundColor, CursorIcon, Element, Style},
};

use crate::{
  identity::import_private_key_hex,
  routes::{ROUTE_CHOOSE_SERVER, ROUTE_IDENTITY_SETUP},
  storage::Storage,
  theme,
  ui::{
    common::lucide_icon::{LucideIcon, LucideIconProps},
    onboarding_shell::{self, OnboardingIntroCopy},
  },
};

const PRIVATE_KEY_HEX_LENGTH: usize = 64;

pub struct ImportIdentityScreen {
  private_key: Signal<String>,
}

impl Component for ImportIdentityScreen {
  type Props = ();

  fn create(ctx: &mut Ctx) -> Self {
    Self {
      private_key: ctx.signal(String::new()),
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    onboarding_shell::screen(
      onboarding_shell::intro(
        ctx,
        OnboardingIntroCopy {
          app_name: &ctx.t("common.app_name"),
          headline: &ctx.t("identity_import.intro.headline"),
          description: &ctx.t("identity_import.intro.description"),
          footer_note: &ctx.t("identity_import.intro.footer"),
        },
      ),
      onboarding_shell::panel(
        Column::new()
          .width(Dimension::Pct(100.0))
          .spacing(theme::SpacingSize::Xl)
          .child(panel_header(
            &ctx.t("identity_import.overline"),
            &ctx.t("identity_import.title"),
            &ctx.t("identity_import.subtitle"),
          ))
          .child(self.field(ctx))
          .child(self.actions(ctx)),
      ),
    )
  }
}

impl ImportIdentityScreen {
  fn field(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let private_key = self.private_key.get();
    let state = private_key_state(&private_key);
    let has_error = state.error_key.is_some();
    let mut placeholder_style = ctx.theme().typography().mono.clone();
    placeholder_style.color = theme::palette().text_muted.with_opacity(0.55);
    let input_background = if has_error {
      BackgroundColor::Palette(theme::PaletteColor::DangerMuted)
    } else {
      BackgroundColor::Palette(theme::PaletteColor::SurfaceInput)
    };
    let input_border = if has_error {
      theme::PaletteColor::Danger
    } else {
      theme::PaletteColor::Border
    };

    let mut field = Column::new()
      .width(Dimension::full())
      .spacing(theme::SpacingSize::Sm)
      .child(
        Text::new(&ctx.t("identity_import.field_label"))
          .variant(theme::TypographyStyle::FieldLabel)
          .color(theme::PaletteColor::TextMuted),
      )
      .child(
        TextInput::styled(self.private_key.clone(), ctx.theme().typography().mono.clone())
          .placeholder(&ctx.t("identity_import.placeholder"))
          .placeholder_style(placeholder_style)
          .width(Dimension::full())
          .height(96.0)
          .padding_vertical(theme::SpacingSize::Lg)
          .padding_horizontal(theme::SpacingSize::Xl)
          .rounded(theme::RadiusSize::Lg)
          .background(input_background)
          .border_inside(1.0, input_border.clone())
          .focused_style(Style::new().border_inside(1.0, input_border))
          .caret_color(theme::PaletteColor::Accent)
          .multiline(),
      );

    if let Some(error_key) = state.error_key {
      field = field.child(error_row(
        ctx,
        &ctx.t_args(error_key, [("count", state.count.to_string())]),
      ));
    } else {
      let hint_key = if state.valid {
        "identity_import.hint.valid"
      } else {
        "identity_import.hint.count"
      };
      field = field.child(
        Text::new(&ctx.t_args(hint_key, [("count", state.count.to_string())]))
          .variant(theme::TypographyStyle::Mono)
          .color(theme::PaletteColor::TextMuted),
      );
    }

    field
  }

  fn actions(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let navigator = ctx.navigator();
    let back_navigator = navigator.clone();
    let import_navigator = navigator;
    let storage = ctx.use_context::<Storage>();
    let private_key = self.private_key.clone();
    let can_import = private_key_state(&private_key.get()).valid;
    let import_button = action_button(
      ctx,
      "arrow-right",
      &ctx.t("identity_import.action.import"),
      if can_import {
        ButtonTone::Primary
      } else {
        ButtonTone::Disabled
      },
    );
    let import_button = if can_import {
      import_button.on_click(move |_| {
        let Ok(identity) = import_private_key_hex(&private_key.get_untracked()) else {
          return;
        };
        if let Some(storage) = storage.as_ref() {
          let _ = storage.save_identity(&identity);
        }
        if let Some(navigator) = import_navigator.as_ref() {
          navigator.replace(ROUTE_CHOOSE_SERVER);
        }
      })
    } else {
      import_button
    };

    Row::new()
      .width(Dimension::Pct(100.0))
      .align_items(Alignment::Center)
      .justify(Justify::SpaceBetween)
      .child(
        action_button(
          ctx,
          "arrow-left",
          &ctx.t("identity_import.action.back"),
          ButtonTone::Ghost,
        )
        .on_click(move |_| {
          if let Some(navigator) = back_navigator.as_ref() {
            navigator.replace(ROUTE_IDENTITY_SETUP);
          }
        }),
      )
      .child(import_button)
  }
}

struct PrivateKeyState {
  count: usize,
  valid: bool,
  error_key: Option<&'static str>,
}

fn private_key_state(input: &str) -> PrivateKeyState {
  let value = input.trim();
  let count = value.chars().count();
  let has_invalid_hex = value.chars().any(|ch| !ch.is_ascii_hexdigit());
  let valid = count == PRIVATE_KEY_HEX_LENGTH && !has_invalid_hex;
  let error_key = if value.is_empty() {
    None
  } else if has_invalid_hex {
    Some("identity_import.error.hex")
  } else if count > PRIVATE_KEY_HEX_LENGTH {
    Some("identity_import.error.length")
  } else {
    None
  };

  PrivateKeyState {
    count,
    valid,
    error_key,
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
        .width(430.0),
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
