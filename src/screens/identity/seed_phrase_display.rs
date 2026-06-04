use lurq::{
  animation::{Easing, Transition},
  app::{component::Component, ctx::Ctx},
  components::{Column, Row, Text},
  core::Signal,
  layout::{Alignment, layout_kind::Justify, text_style::FontWeight},
  node::{BackgroundColor, CursorIcon, Element, Style, color::Color, dimension::Dimension},
};

use crate::{
  identity,
  identity::LocalIdentity,
  screens::shared::{self, BORDER, CARD_WIDTH, INTRO_WIDTH, STEP_CHOOSE_SERVER, action_button, styled_text},
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
      "#7D766C",
      1.2,
    ))
    .child(styled_text(
      word,
      "JetBrains Mono",
      11.0,
      FontWeight::Medium,
      "#F4F4F2",
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
    theme::ACCENT_COLOR
  } else {
    theme::TEXT_SECONDARY_COLOR
  };
  let background = if copied_now {
    theme::GREEN_MUTED_COLOR
  } else {
    theme::BG_ELEVATED_COLOR
  };
  let border = if copied_now {
    theme::BORDER_LIGHT_COLOR
  } else {
    theme::BORDER_COLOR
  };
  let hover_background = if copied_now {
    theme::ACCENT_MUTED_COLOR
  } else {
    theme::BG_INPUT_COLOR
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
  identity: Result<LocalIdentity, String>,
  copied: Signal<bool>,
  save_failed: Signal<bool>,
}

impl Component for SeedPhraseDisplay {
  type Props = ();

  fn create(ctx: &mut Ctx) -> Self {
    Self {
      identity: identity::generate_identity().map_err(|error| error.to_string()),
      copied: ctx.signal(false),
      save_failed: ctx.signal(false),
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let step = ctx.use_context::<Signal<u8>>();
    let next_step = step.clone();
    let storage = ctx.use_context::<Storage>();
    let save_failed = self.save_failed.get();
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
        .child(Text::new(&ctx.t("identity.seed.caption")).variant(theme::TYP_CAPTION))
        .child(Text::new(&ctx.t("identity.seed.title")).variant(theme::TYP_TITLE))
        .child(Text::new(&ctx.t("identity.seed.desc")).variant(theme::TYP_DESC))
        .child(
          Column::new()
            .width(INTRO_WIDTH)
            .spacing(8.0)
            .padding(12.0)
            .rounded(6.0)
            .background(BackgroundColor::Palette(theme::BG_TERTIARY))
            .border_inside(1.0, Color::from_hex(BORDER))
            .child(shared::dot(BackgroundColor::Palette(theme::RED)))
            .child(
              Text::new(&ctx.t(if save_failed {
                "identity.seed.save_failed_title"
              } else {
                "identity.seed.meta_title"
              }))
              .variant(theme::TYP_BUTTON),
            )
            .child(
              Text::new(&ctx.t(if save_failed {
                "identity.seed.save_failed_desc"
              } else {
                "identity.seed.meta_desc"
              }))
              .variant(theme::TYP_LINK),
            ),
        ),
      Column::new()
        .width(CARD_WIDTH)
        .spacing(14.0)
        .padding(18.0)
        .rounded(8.0)
        .background(BackgroundColor::Palette(theme::BG_TERTIARY))
        .border_inside(1.0, Color::from_hex(BORDER))
        .child(
          Row::new()
            .width(Dimension::Pct(100.0))
            .align_items(Alignment::Center)
            .child(
              Text::new(&ctx.t("identity.seed.heading"))
                .variant(theme::TYP_HEADING)
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
            .background(BackgroundColor::Palette(theme::BG_SECONDARY))
            .border_inside(1.0, Color::from_hex(BORDER))
            .child(seed_row(0, &seed_words))
            .child(seed_row(3, &seed_words))
            .child(seed_row(6, &seed_words))
            .child(seed_row(9, &seed_words)),
        )
        .child(shared::back_button(step, &ctx.t("identity.action.back")))
        .child({
          let button = action_button(&ctx.t("identity.action.continue_saved"), true);
          let identity = self.identity.as_ref().ok().cloned();
          let save_failed = self.save_failed.clone();
          if let Some(next_step) = next_step {
            button.on_click(move |_| {
              let saved = match (&storage, &identity) {
                (Some(storage), Some(identity)) => storage.save_identity(identity).is_ok(),
                _ => false,
              };
              save_failed.set(!saved);
              if saved {
                next_step.set(STEP_CHOOSE_SERVER);
              }
            })
          } else {
            button
          }
        }),
    )
  }
}
