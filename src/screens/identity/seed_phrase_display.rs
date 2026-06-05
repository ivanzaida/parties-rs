use lurq::{
  animation::{Easing, Transition},
  app::{component::Component, ctx::Ctx},
  components::{Column, FormHandle, FormOptions, FormProps, Row, Text, validators},
  core::Signal,
  layout::{Alignment, layout_kind::Justify, text_style::FontWeight},
  node::{BackgroundColor, CursorIcon, Element, Style, dimension::Dimension},
};

use crate::{
  identity,
  identity::LocalIdentity,
  screens::shared::{self, CARD_WIDTH, INTRO_WIDTH, ROUTE_CHOOSE_SERVER, styled_text},
  storage::Storage,
  theme,
};

const GRID_PADDING: f32 = 12.0;

fn word_cell(index: usize, word: &str) -> Row {
  Row::new()
    .flex(1.0)
    .align_items(Alignment::Center)
    .spacing(5.0)
    .child(styled_text(
      &format!("{:02}", index + 1),
      "JetBrains Mono",
      10.0,
      FontWeight::Bold,
      theme::palette().text_muted,
      1.2,
    ))
    .child(styled_text(
      word,
      "JetBrains Mono",
      11.0,
      FontWeight::Medium,
      theme::palette().text_primary,
      1.2,
    ))
}

fn seed_word(words: &[String], index: usize) -> &str {
  words.get(index).map(String::as_str).unwrap_or("")
}

fn seed_row(start: usize, words: &[String]) -> Row {
  Row::new()
    .width(Dimension::Pct(100.0))
    .spacing(6.0)
    .padding_horizontal(GRID_PADDING)
    .child(word_cell(start, seed_word(words, start)))
    .child(word_cell(start + 1, seed_word(words, start + 1)))
    .child(word_cell(start + 2, seed_word(words, start + 2)))
}

fn copy_phrase_icon_button(seed_phrase: Option<&str>, copied: Signal<bool>) -> Row {
  let copied_now = copied.get();
  let icon_name = if copied_now { "check" } else { "copy" };
  let icon_color = if copied_now {
    theme::palette().accent
  } else {
    theme::palette().text_secondary
  };
  let background = if copied_now {
    theme::palette().success_muted
  } else {
    theme::palette().surface_raised
  };
  let border = if copied_now {
    theme::palette().border_strong
  } else {
    theme::palette().border
  };
  let hover_background = if copied_now {
    theme::palette().accent_muted
  } else {
    theme::palette().surface_input
  };

  let row = Row::new()
    .width(30.0)
    .height(30.0)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .rounded(5.0)
    .background(background)
    .border_inside(1.0, border)
    .cursor(CursorIcon::Pointer)
    .transition(Transition::background_color().duration_ms(180).easing(Easing::EASE_OUT))
    .transition(Transition::border_color().duration_ms(180).easing(Easing::EASE_OUT))
    .hovered_style(Style::new().background(hover_background))
    .child(shared::icon(icon_name, 14.0, icon_color));

  if let Some(seed_phrase) = seed_phrase {
    let seed_phrase = seed_phrase.to_owned();
    let copied_on_click = copied.clone();
    row.on_click(move |_| {
      copied_on_click.set(lurq::clipboard::copy_to_clipboard(&seed_phrase));
    })
  } else {
    row
  }
}

pub struct SeedPhraseDisplay {
  form: FormHandle,
  identity: Result<LocalIdentity, String>,
  copied: Signal<bool>,
  copy_failed: Signal<bool>,
  recovery_failed: Signal<bool>,
  save_failed: Signal<bool>,
}

impl Component for SeedPhraseDisplay {
  type Props = ();

  fn create(ctx: &mut Ctx) -> Self {
    let recovery_unavailable = ctx.t("identity.seed.status.unavailable");
    let identity = identity::generate_identity().map_err(|error| error.to_string());
    let seed_phrase = identity
      .as_ref()
      .ok()
      .and_then(|identity| identity.seed_phrase.as_ref())
      .cloned()
      .unwrap_or_default();

    Self {
      form: ctx.form(
        FormOptions::new()
          .field("seed_phrase", seed_phrase)
          .validate_string("seed_phrase", validators::required(recovery_unavailable)),
      ),
      identity,
      copied: ctx.signal(false),
      copy_failed: ctx.signal(false),
      recovery_failed: ctx.signal(false),
      save_failed: ctx.signal(false),
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let navigator = ctx.navigator();
    let next_navigator = navigator.clone();
    let storage = ctx.use_context::<Storage>();
    let recovery_failed = self.recovery_failed.get();
    let copy_failed = self.copy_failed.get();
    let save_failed = self.save_failed.get();
    let meta_title_key = if recovery_failed {
      "identity.seed.recovery_failed_title"
    } else if copy_failed {
      "identity.seed.copy_failed_title"
    } else if save_failed {
      "identity.seed.save_failed_title"
    } else {
      "identity.seed.meta_title"
    };
    let meta_desc_key = if recovery_failed {
      "identity.seed.recovery_failed_desc"
    } else if copy_failed {
      "identity.seed.copy_failed_desc"
    } else if save_failed {
      "identity.seed.save_failed_desc"
    } else {
      "identity.seed.meta_desc"
    };
    let seed_phrase = self
      .identity
      .as_ref()
      .ok()
      .and_then(|identity| identity.seed_phrase.as_ref())
      .cloned();
    let seed_words: Vec<String> = seed_phrase
      .as_deref()
      .map(|phrase| phrase.split_whitespace().map(str::to_owned).collect())
      .unwrap_or_default();

    shared::identity_screen(
      Column::new()
        .width(INTRO_WIDTH)
        .spacing(18.0)
        .child(Text::new(&ctx.t("identity.seed.caption")).variant(theme::TypographyStyle::Caption))
        .child(Text::new(&ctx.t("identity.seed.title")).variant(theme::TypographyStyle::Title))
        .child(Text::new(&ctx.t("identity.seed.desc")).variant(theme::TypographyStyle::Description))
        .child(
          Column::new()
            .width(INTRO_WIDTH)
            .spacing(8.0)
            .padding(12.0)
            .rounded(6.0)
            .background(BackgroundColor::Palette(theme::PaletteColor::SurfacePanel))
            .border_inside(1.0, theme::PaletteColor::Border)
            .child(shared::dot(BackgroundColor::Palette(theme::PaletteColor::Danger)))
            .child(Text::new(&ctx.t(meta_title_key)).variant(theme::TypographyStyle::Button))
            .child(Text::new(&ctx.t(meta_desc_key)).variant(theme::TypographyStyle::Link)),
        ),
      ctx.form_view_with(
        FormProps::new({
          let identity = self.identity.as_ref().ok().cloned();
          let seed_phrase = seed_phrase.clone();
          let copied = self.copied.clone();
          let copy_failed = self.copy_failed.clone();
          let recovery_failed = self.recovery_failed.clone();
          let save_failed = self.save_failed.clone();
          self
            .form
            .clone()
            .on_submit(move |_| {
              recovery_failed.set(false);
              let copied_to_clipboard = seed_phrase.as_deref().is_some_and(lurq::clipboard::copy_to_clipboard);
              copied.set(copied_to_clipboard);
              copy_failed.set(!copied_to_clipboard);
              if !copied_to_clipboard {
                save_failed.set(false);
                return;
              }

              let saved = match (&storage, &identity) {
                (Some(storage), Some(identity)) => storage.save_identity(identity).is_ok(),
                _ => false,
              };
              save_failed.set(!saved);
              copy_failed.set(false);
              if saved && let Some(navigator) = &next_navigator {
                navigator.replace(ROUTE_CHOOSE_SERVER);
              }
            })
            .on_invalid({
              let copied = self.copied.clone();
              let copy_failed = self.copy_failed.clone();
              let recovery_failed = self.recovery_failed.clone();
              let save_failed = self.save_failed.clone();
              move |_| {
                copied.set(false);
                copy_failed.set(false);
                recovery_failed.set(true);
                save_failed.set(false);
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
            .child(
              Row::new()
                .width(Dimension::Pct(100.0))
                .align_items(Alignment::Center)
                .child(
                  Text::new(&ctx.t("identity.seed.heading"))
                    .variant(theme::TypographyStyle::Heading)
                    .flex(1.0),
                )
                .child(copy_phrase_icon_button(seed_phrase.as_deref(), self.copied.clone())),
            )
            .child(
              Column::new()
                .width(Dimension::Pct(100.0))
                .spacing(6.0)
                .padding_vertical(GRID_PADDING)
                .rounded(6.0)
                .background(BackgroundColor::Palette(theme::PaletteColor::SurfacePanel))
                .border_inside(1.0, theme::PaletteColor::Border)
                .child(seed_row(0, &seed_words))
                .child(seed_row(3, &seed_words))
                .child(seed_row(6, &seed_words))
                .child(seed_row(9, &seed_words)),
            )
            .child(
              ctx.mount::<shared::FormPrimaryButton>(shared::FormPrimaryButtonProps::new(
                ctx.t("identity.action.continue_saved"),
              )),
            )
            .child(shared::back_button(navigator, &ctx.t("identity.action.back")))
        },
      ),
    )
  }
}
