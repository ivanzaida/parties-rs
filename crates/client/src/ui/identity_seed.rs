use lurq::{
  app::{component::Component, ctx::Ctx, theme::Breakpoint},
  clipboard,
  components::{Column, Row, Text},
  core::Signal,
  layout::{Alignment, layout_kind::Justify},
  node::{BackgroundColor, CursorIcon, Element, Style, color::Color, dimension::Dimension},
};

use crate::{
  identity::{LocalIdentity, generate_identity},
  routes::ROUTE_CHOOSE_SERVER,
  storage::Storage,
  theme,
  ui::{
    common::lucide_icon::{LucideIcon, LucideIconProps},
    onboarding_shell::{self, OnboardingIntroCopy},
  },
};

const COPY_ACTION_MIN_WIDTH: f32 = 148.0;

pub struct IdentitySeedScreen {
  identity: LocalIdentity,
  confirmed: Signal<bool>,
  copied: Signal<bool>,
  hidden: Signal<bool>,
}

impl Component for IdentitySeedScreen {
  type Props = ();

  fn create(ctx: &mut Ctx) -> Self {
    Self {
      identity: generate_identity().expect("failed to generate recovery phrase"),
      confirmed: ctx.signal(false),
      copied: ctx.signal(false),
      hidden: ctx.signal(false),
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let confirmed = self.confirmed.get();
    let copied = self.copied.get();
    let hidden = self.hidden.get();
    let phrase = self.seed_phrase();

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
        ctx.breakpoint(),
        Column::new()
          .width(Dimension::Pct(100.0))
          .spacing(theme::SpacingSize::Section)
          .child(panel_header(
            &ctx.t("identity_seed.overline"),
            &ctx.t("identity_seed.title"),
            &ctx.t("identity_seed.subtitle"),
          ))
          .child(seed_grid(ctx, phrase, hidden))
          .child(self.seed_bottom(ctx, phrase, confirmed, copied, hidden)),
      ),
    )
  }
}

impl IdentitySeedScreen {
  fn seed_phrase(&self) -> &str {
    self.identity.seed_phrase.as_deref().unwrap_or("")
  }

  fn seed_bottom(&self, ctx: &mut Ctx, phrase: &str, confirmed: bool, copied: bool, hidden: bool) -> Column {
    let phrase = phrase.to_owned();
    let identity = self.identity.clone();
    let navigator = ctx.navigator();
    let storage = ctx.use_context::<Storage>();
    let copied_signal = self.copied.clone();
    let hidden_signal = self.hidden.clone();

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
    let continue_phrase = phrase.clone();
    let continue_identity = identity.clone();
    let continue_button = action_button_with_icon_position(
      ctx,
      "arrow-right",
      &ctx.t("identity_seed.action.continue"),
      if confirmed {
        ButtonTone::Primary
      } else {
        ButtonTone::Disabled
      },
      IconPosition::Trailing,
    );
    let continue_button = if confirmed {
      continue_button.on_click(move |_| {
        let _ = clipboard::copy_to_clipboard(&continue_phrase);
        if let Some(storage) = storage.as_ref() {
          let _ = storage.save_identity(&continue_identity);
        }
        if let Some(navigator) = navigator.as_ref() {
          navigator.replace(ROUTE_CHOOSE_SERVER);
        }
      })
    } else {
      continue_button
    };
    let copy_button = action_button(ctx, copy_icon, &copy_label, copy_tone)
      .min_width(COPY_ACTION_MIN_WIDTH)
      .on_click({
        let phrase = phrase.clone();
        move |_| {
          if clipboard::copy_to_clipboard(&phrase) {
            copied_signal.set(true);
          }
        }
      });
    let hide_button = action_button(ctx, "eye-off", &hide_label, ButtonTone::Ghost).on_click(move |_| {
      hidden_signal.set(!hidden);
    });
    let confirm_control =
      confirm_row(ctx, &ctx.t("identity_seed.confirm"), confirmed, self.confirmed.clone()).flex(1.0);
    let actions = Column::new()
      .width(Dimension::Pct(100.0))
      .spacing(theme::SpacingSize::Xl)
      .child(
        Row::new()
          .width(Dimension::Pct(100.0))
          .align_items(Alignment::Center)
          .spacing(theme::SpacingSize::Md)
          .child(copy_button)
          .child(hide_button),
      )
      .child(
        Row::new()
          .width(Dimension::Pct(100.0))
          .align_items(Alignment::Center)
          .justify(Justify::SpaceBetween)
          .spacing(theme::SpacingSize::Lg)
          .child(confirm_control)
          .child(continue_button),
      );

    Column::new()
      .width(Dimension::Pct(100.0))
      .spacing(theme::SpacingSize::Xl)
      .child(actions)
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
        .width(Dimension::Pct(100.0))
        .max_width(430.0),
    )
}

fn seed_grid(ctx: &Ctx, phrase: &str, hidden: bool) -> Column {
  let words = phrase.split_whitespace().collect::<Vec<_>>();
  let columns = match ctx.breakpoint() {
    Some(Breakpoint::Md) => 2,
    Some(Breakpoint::Lg | Breakpoint::Xl | Breakpoint::Sm) | None => 3,
  };
  let rows = words.len().div_ceil(columns);
  let mut grid = Column::new()
    .width(Dimension::Pct(100.0))
    .spacing(theme::SpacingSize::Md);

  for row in 0..rows {
    let mut word_row = Row::new().width(Dimension::Pct(100.0)).spacing(theme::SpacingSize::Md);

    for col in 0..columns {
      let index = row * columns + col;
      word_row = word_row.child(seed_word(index + 1, words.get(index).copied().unwrap_or(""), hidden));
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
        .color(theme::PaletteColor::TextMuted)
        .nowrap(),
    )
    .child(Text::new(display_word).variant(theme::TypographyStyle::Mono).nowrap())
}

#[derive(Clone, Copy)]
enum ButtonTone {
  Primary,
  Secondary,
  Ghost,
  Success,
  Disabled,
}

#[derive(Clone, Copy)]
enum IconPosition {
  Leading,
  Trailing,
}

fn action_button(ctx: &mut Ctx, icon: &'static str, label: &str, tone: ButtonTone) -> Row {
  action_button_with_icon_position(ctx, icon, label, tone, IconPosition::Leading)
}

fn action_button_with_icon_position(
  ctx: &mut Ctx,
  icon_name: &'static str,
  label: &str,
  tone: ButtonTone,
  icon_position: IconPosition,
) -> Row {
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
    ButtonTone::Disabled => (
      BackgroundColor::Palette(theme::PaletteColor::SurfaceInput),
      BackgroundColor::Palette(theme::PaletteColor::Border),
      theme::PaletteColor::TextMuted,
      theme::palette().text_muted,
      BackgroundColor::Palette(theme::PaletteColor::SurfaceInput),
    ),
  };
  let enabled = !matches!(tone, ButtonTone::Disabled);

  let button = Row::new()
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
  let button = if enabled {
    button
      .hovered_style(Style::new().background(hover_background.clone()))
      .active_style(Style::new().background(hover_background))
  } else {
    button
  };

  let icon = ctx.mount::<LucideIcon>(LucideIconProps {
    icon: icon_name,
    size: 16.0,
    color: icon_color,
  });
  let label = Text::new(label)
    .variant(theme::TypographyStyle::Button)
    .color(text_color);

  match icon_position {
    IconPosition::Leading => button.child(icon).child(label),
    IconPosition::Trailing => button.child(label).child(icon),
  }
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
    .width(Dimension::Pct(100.0))
    .align_items(Alignment::Start)
    .spacing(theme::SpacingSize::Md)
    .cursor(CursorIcon::Pointer)
    .on_click(move |_| confirmed_signal.set(!confirmed))
    .child(Column::new().padding_top(3.0).child(checkbox))
    .child(
      Text::new(label)
        .variant(theme::TypographyStyle::Description)
        .color(label_color)
        .flex(1.0),
    )
}
