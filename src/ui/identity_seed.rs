use lurq::{
  app::{component::Component, ctx::Ctx},
  clipboard,
  components::{Column, Row, Text},
  core::Signal,
  layout::{Alignment, layout_kind::Justify},
  node::{BackgroundColor, CursorIcon, Element, Style, color::Color, dimension::Dimension},
};

use crate::{
  identity::restore_seed_phrase,
  routes::ROUTE_CHOOSE_SERVER,
  storage::Storage,
  theme,
  ui::{
    common::lucide_icon::{LucideIcon, LucideIconProps},
    onboarding_shell::{self, OnboardingIntroCopy},
  },
};

const SEED_WORDS: [&str; 12] = [
  "harbor", "velvet", "cinder", "oblige", "walnut", "tidal", "render", "summon", "pivot", "acorn", "freight", "murmur",
];

pub struct IdentitySeedScreen {
  confirmed: Signal<bool>,
  copied: Signal<bool>,
  hidden: Signal<bool>,
}

impl Component for IdentitySeedScreen {
  type Props = ();

  fn create(ctx: &mut Ctx) -> Self {
    Self {
      confirmed: ctx.signal(false),
      copied: ctx.signal(false),
      hidden: ctx.signal(false),
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let confirmed = self.confirmed.get();
    let copied = self.copied.get();
    let hidden = self.hidden.get();

    onboarding_shell::screen(
      onboarding_shell::intro(
        ctx,
        OnboardingIntroCopy {
          app_name: &ctx.t("common.app_name"),
          headline: &ctx.t("identity_seed.intro.headline"),
          description: &ctx.t("identity_seed.intro.description"),
          footer_note: &ctx.t("identity_seed.intro.footer"),
        },
      ),
      onboarding_shell::panel(
        Column::new()
          .width(Dimension::Pct(100.0))
          .spacing(theme::SpacingSize::Section)
          .child(panel_header(
            &ctx.t("identity_seed.overline"),
            &ctx.t("identity_seed.title"),
            &ctx.t("identity_seed.subtitle"),
          ))
          .child(seed_grid(hidden))
          .child(self.seed_bottom(ctx, confirmed, copied, hidden)),
      ),
    )
  }
}

impl IdentitySeedScreen {
  fn seed_bottom(&self, ctx: &mut Ctx, confirmed: bool, copied: bool, hidden: bool) -> Column {
    let phrase = seed_phrase();
    let navigator = ctx.navigator();
    let storage = ctx.use_context::<Storage>();
    let copied_signal = self.copied.clone();
    let hidden_signal = self.hidden.clone();
    let confirmed_signal = self.confirmed.clone();

    let (copy_icon, copy_label, copy_tone) = if copied {
      ("check", ctx.t("identity_seed.action.copied"), ButtonTone::Success)
    } else {
      ("copy", ctx.t("identity_seed.action.copy"), ButtonTone::Secondary)
    };
    let hide_label = if hidden {
      ctx.t("identity_seed.action.show")
    } else {
      ctx.t("identity_seed.action.hide")
    };

    Column::new()
      .width(Dimension::Pct(100.0))
      .spacing(theme::SpacingSize::Xl)
      .child(
        Row::new()
          .width(Dimension::Pct(100.0))
          .align_items(Alignment::Center)
          .justify(Justify::SpaceBetween)
          .child(
            Row::new()
              .align_items(Alignment::Center)
              .spacing(theme::SpacingSize::Md)
              .child(action_button(ctx, copy_icon, &copy_label, copy_tone).on_click({
                let phrase = phrase.clone();
                move |_| {
                  if clipboard::copy_to_clipboard(&phrase) {
                    copied_signal.set(true);
                  }
                }
              }))
              .child(
                action_button(ctx, "eye-off", &hide_label, ButtonTone::Ghost).on_click(move |_| {
                  hidden_signal.set(!hidden);
                }),
              ),
          )
          .child(
            action_button(
              ctx,
              "arrow-right",
              &ctx.t("identity_seed.action.continue"),
              ButtonTone::Primary,
            )
            .on_click(move |_| {
              if !confirmed_signal.get_untracked() {
                confirmed_signal.set(true);
                return;
              }

              let _ = clipboard::copy_to_clipboard(&phrase);
              if let Ok(identity) = restore_seed_phrase(&phrase) {
                if let Some(storage) = storage.as_ref() {
                  let _ = storage.save_identity(&identity);
                }
              }
              if let Some(navigator) = navigator.as_ref() {
                navigator.replace(ROUTE_CHOOSE_SERVER);
              }
            }),
          ),
      )
      .child(confirm_row(
        ctx,
        &ctx.t("identity_seed.confirm"),
        confirmed,
        self.confirmed.clone(),
      ))
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

fn seed_grid(hidden: bool) -> Column {
  let mut grid = Column::new()
    .width(Dimension::Pct(100.0))
    .spacing(theme::SpacingSize::Md);

  for row in 0..4 {
    let mut word_row = Row::new().width(Dimension::Pct(100.0)).spacing(theme::SpacingSize::Md);

    for col in 0..3 {
      let index = row * 3 + col;
      word_row = word_row.child(seed_word(index + 1, SEED_WORDS[index], hidden));
    }

    grid = grid.child(word_row);
  }

  grid
}

fn seed_word(index: usize, word: &str, hidden: bool) -> impl Into<Element> {
  let display_word = if hidden { "******" } else { word };

  Row::new()
    .flex(1.0)
    .align_items(Alignment::Center)
    .spacing(theme::SpacingSize::Md)
    .padding_vertical(theme::SpacingSize::Md)
    .padding_horizontal(theme::SpacingSize::Lg)
    .rounded(theme::RadiusSize::Lg)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfacePanel))
    .border_inside(1.0, theme::PaletteColor::Border)
    .child(
      Text::new(&index.to_string())
        .variant(theme::TypographyStyle::Mono)
        .color(theme::PaletteColor::TextMuted),
    )
    .child(Text::new(display_word).variant(theme::TypographyStyle::Mono))
}

#[derive(Clone, Copy)]
enum ButtonTone {
  Primary,
  Secondary,
  Ghost,
  Success,
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
    ButtonTone::Secondary => (
      BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised),
      BackgroundColor::Palette(theme::PaletteColor::BorderStrong),
      theme::PaletteColor::TextPrimary,
      theme::palette().text_secondary,
      BackgroundColor::Palette(theme::PaletteColor::SurfaceInput),
    ),
    ButtonTone::Ghost => (
      BackgroundColor::Color(Color::from_hex("#00000000")),
      BackgroundColor::Color(Color::from_hex("#00000000")),
      theme::PaletteColor::TextSecondary,
      theme::palette().text_secondary,
      BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised),
    ),
    ButtonTone::Success => (
      BackgroundColor::Color(Color::from_hex("#00000000")),
      BackgroundColor::Color(Color::from_hex("#42D28B66")),
      theme::PaletteColor::Success,
      theme::palette().success,
      BackgroundColor::Palette(theme::PaletteColor::SuccessMuted),
    ),
  };

  Row::new()
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
    .active_style(Style::new().background(hover_background))
    .child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon,
      size: 16.0,
      color: icon_color,
    }))
    .child(
      Text::new(label)
        .variant(theme::TypographyStyle::Button)
        .color(text_color),
    )
}

fn confirm_row(ctx: &mut Ctx, label: &str, confirmed: bool, confirmed_signal: Signal<bool>) -> Row {
  let checkbox_background = if confirmed {
    BackgroundColor::Palette(theme::PaletteColor::Accent)
  } else {
    BackgroundColor::Palette(theme::PaletteColor::SurfaceInput)
  };
  let checkbox_border = if confirmed {
    theme::PaletteColor::Accent
  } else {
    theme::PaletteColor::BorderStrong
  };
  let label_color = if confirmed {
    theme::PaletteColor::TextPrimary
  } else {
    theme::PaletteColor::TextSecondary
  };

  let mut checkbox = Row::new()
    .width(18.0)
    .height(18.0)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .rounded(theme::RadiusSize::Md)
    .background(checkbox_background)
    .border_inside(1.0, checkbox_border);

  if confirmed {
    checkbox = checkbox.child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon: "check",
      size: 12.0,
      color: theme::palette().text_inverse,
    }));
  }

  Row::new()
    .align_items(Alignment::Center)
    .spacing(theme::SpacingSize::Md)
    .cursor(CursorIcon::Pointer)
    .on_click(move |_| confirmed_signal.set(!confirmed))
    .child(checkbox)
    .child(
      Text::new(label)
        .variant(theme::TypographyStyle::Description)
        .color(label_color),
    )
}

fn seed_phrase() -> String {
  SEED_WORDS.join(" ")
}
