use std::sync::Arc;

use lurq::{
  app::{component::Component, ctx::Ctx},
  components::{Column, Rect, Row, Text},
  layout::{
    Alignment,
    layout_kind::Justify,
    text_style::{FontWeight, TextStyle},
  },
  node::{BackgroundColor, CursorIcon, Element, Style, color::Color},
};

use crate::theme;

const BORDER: &str = "#30343A";
const FRAME_BG: &str = "#0D0F15";
const GRID_BG: &str = "#101215";
const WARNING: &str = "#FF6B5F";
const SCREEN_WIDTH: f32 = 860.0;
const CONTENT_HEIGHT: f32 = 640.0;
const SIDE_PADDING: f32 = 40.0;
const INTRO_WIDTH: f32 = 300.0;
const CARD_WIDTH: f32 = 360.0;
const CARD_CONTENT_WIDTH: f32 = 324.0;
const GRID_CONTENT_WIDTH: f32 = 300.0;
const SEED_WORDS: [&str; 12] = [
  "abandon", "ability", "able", "about", "above", "absent", "absorb", "abstract", "absurd", "abuse", "access",
  "accident",
];

fn text(content: &str, family: &str, size: f32, weight: FontWeight, color: &str, line_height: f32) -> Text {
  Text::styled(
    content,
    TextStyle {
      font_family: Arc::from(family),
      font_size: size,
      line_height,
      weight,
      color: Color::from_hex(color),
      ..TextStyle::default()
    },
  )
}

fn word_cell(index: usize, word: &str) -> Row {
  Row::new()
    .flex(1.0)
    .align_items(Alignment::Center)
    .spacing(5.0)
    .child(text(
      &format!("{:02}", index + 1),
      "JetBrains Mono",
      9.0,
      FontWeight::Bold,
      "#7D766C",
      1.2,
    ))
    .child(text(word, "JetBrains Mono", 10.0, FontWeight::Medium, "#F4F4F2", 1.2))
}

fn seed_row(start: usize) -> Row {
  Row::new()
    .width(GRID_CONTENT_WIDTH)
    .spacing(6.0)
    .child(word_cell(start, SEED_WORDS[start]))
    .child(word_cell(start + 1, SEED_WORDS[start + 1]))
    .child(word_cell(start + 2, SEED_WORDS[start + 2]))
}

fn button(label: &str, primary: bool) -> Row {
  let background = if primary {
    BackgroundColor::Palette(theme::ACCENT)
  } else {
    BackgroundColor::Palette(theme::BG_ELEVATED)
  };
  let border = if primary { "#42D28B" } else { BORDER };
  let label_color = if primary { "#07110B" } else { "#F4F4F2" };
  let hover_bg = if primary { "#57E09C" } else { "#22262B" };

  Row::new()
    .width(CARD_CONTENT_WIDTH)
    .height(34.0)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .rounded(5.0)
    .background(background)
    .border_inside(1.0, Color::from_hex(border))
    .cursor(CursorIcon::Pointer)
    .hovered_style(Style::new().background(hover_bg))
    .child(text(label, "Inter", 12.0, FontWeight::Bold, label_color, 1.2))
}

fn warning_dot() -> Rect {
  Rect::new(8.0, 8.0).rounded(4.0).background(WARNING)
}

pub struct SeedPhraseDisplay;

impl Component for SeedPhraseDisplay {
  type Props = ();

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, _ctx: &mut Ctx) -> impl Into<Element> {
    Column::new()
      .width(SCREEN_WIDTH)
      .height(690.0)
      .background(FRAME_BG)
      .clip()
      .child(
        Row::new()
          .width(SCREEN_WIDTH)
          .height(CONTENT_HEIGHT)
          .align_items(Alignment::Center)
          .justify(Justify::Center)
          .spacing(32.0)
          .padding_horizontal(SIDE_PADDING)
          .background(BackgroundColor::Palette(theme::BG_PRIMARY))
          .child(
            Column::new()
              .width(INTRO_WIDTH)
              .spacing(18.0)
              .child(text("BACKUP", "JetBrains Mono", 10.0, FontWeight::Bold, "#7D766C", 1.2))
              .child(text(
                "Save recovery phrase",
                "Inter",
                26.0,
                FontWeight::Black,
                "#F4F4F2",
                1.2,
              ))
              .child(text(
                "This phrase is the only recovery path for your identity. Store it before joining servers.",
                "Inter",
                13.0,
                FontWeight::Normal,
                "#B7B2AA",
                1.35,
              ))
              .child(
                Column::new()
                  .width(INTRO_WIDTH)
                  .spacing(8.0)
                  .padding(12.0)
                  .rounded(6.0)
                  .background(BackgroundColor::Palette(theme::BG_TERTIARY))
                  .border_inside(1.0, Color::from_hex(BORDER))
                  .child(warning_dot())
                  .child(text(
                    "Backup required",
                    "Inter",
                    12.0,
                    FontWeight::Black,
                    "#F4F4F2",
                    1.2,
                  ))
                  .child(text(
                    "The app cannot recover a lost seed phrase.",
                    "Inter",
                    11.0,
                    FontWeight::Normal,
                    "#B7B2AA",
                    1.25,
                  )),
              ),
          )
          .child(
            Column::new()
              .width(CARD_WIDTH)
              .spacing(14.0)
              .padding(18.0)
              .rounded(8.0)
              .background(BackgroundColor::Palette(theme::BG_TERTIARY))
              .border_inside(1.0, Color::from_hex(BORDER))
              .child(text(
                "Recovery phrase",
                "Inter",
                16.0,
                FontWeight::Black,
                "#F4F4F2",
                1.2,
              ))
              .child(
                Column::new()
                  .width(CARD_CONTENT_WIDTH)
                  .spacing(6.0)
                  .padding(12.0)
                  .rounded(6.0)
                  .background(GRID_BG)
                  .border_inside(1.0, Color::from_hex(BORDER))
                  .child(seed_row(0))
                  .child(seed_row(3))
                  .child(seed_row(6))
                  .child(seed_row(9)),
              )
              .child(button("Copy phrase", false))
              .child(button("I saved it - continue", true)),
          ),
      )
  }
}
