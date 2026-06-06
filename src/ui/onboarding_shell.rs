use lurq::{
  app::ctx::Ctx,
  components::{Column, Row, Stack, Text},
  layout::{Alignment, layout_kind::Justify},
  node::{BackgroundColor, Element, Gradient, GradientStop, color::Color, dimension::Dimension},
};

use crate::{
  theme,
  ui::common::lucide_icon::{LucideIcon, LucideIconProps},
};

pub struct OnboardingIntroCopy<'a> {
  pub app_name: &'a str,
  pub headline: &'a str,
  pub description: &'a str,
  pub footer_note: &'a str,
}

pub fn screen(intro: impl Into<Element>, panel: impl Into<Element>) -> Row {
  Row::new()
    .width(Dimension::Pct(100.0))
    .height(Dimension::Pct(100.0))
    .flex(1.0)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceBase))
    .clip()
    .child(intro)
    .child(panel)
}

pub fn intro(ctx: &mut Ctx, copy: OnboardingIntroCopy<'_>) -> Stack {
  Stack::new()
    .width(Dimension::Pct(50.0))
    .height(Dimension::Pct(100.0))
    .background(Color::from_hex("#0E1013"))
    .background_gradient(
      Gradient::radial([
        GradientStop::at(Color::from_hex("#1C2128"), 0.0),
        GradientStop::at(Color::from_hex("#0E1013"), 1.0),
      ])
      .center(0.22, 0.12),
    )
    .clip()
    .child(
      Column::new()
        .width(Dimension::Pct(100.0))
        .height(Dimension::Pct(100.0))
        .justify(Justify::SpaceBetween)
        .padding(56.0)
        .clip()
        .child(
          Column::new()
            .width(Dimension::Pct(100.0))
            .spacing(36.0)
            .justify(Justify::Center)
            .flex(1.0)
            .child(brand(ctx, copy.app_name))
            .child(
              Column::new()
                .width(Dimension::Pct(100.0))
                .spacing(22.0)
                .child(Text::new(copy.headline).variant(theme::TypographyStyle::Title))
                .child(
                  Text::new(copy.description)
                    .variant(theme::TypographyStyle::Description)
                    .width(470.0),
                ),
            ),
        )
        .child(
          Row::new()
            .align_items(Alignment::Center)
            .spacing(8.0)
            .child(ctx.mount::<LucideIcon>(LucideIconProps {
              icon: "lock",
              size: 14.0,
              color: theme::palette().text_muted,
            }))
            .child(
              Text::new(copy.footer_note)
                .variant(theme::TypographyStyle::Link)
                .color(theme::PaletteColor::TextMuted),
            ),
        ),
    )
}

pub fn panel(content: impl Into<Element>) -> Column {
  Column::new()
    .width(Dimension::Pct(50.0))
    .height(Dimension::Pct(100.0))
    .justify(Justify::Center)
    .padding_vertical(56.0)
    .padding_horizontal(64.0)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceBase))
    .clip()
    .child(content)
}

fn brand(ctx: &mut Ctx, app_name: &str) -> Row {
  Row::new()
    .align_items(Alignment::Center)
    .spacing(12.0)
    .child(
      Row::new()
        .width(32.0)
        .height(32.0)
        .align_items(Alignment::Center)
        .justify(Justify::Center)
        .rounded(8.0)
        .background(BackgroundColor::Palette(theme::PaletteColor::Accent))
        .child(ctx.mount::<LucideIcon>(LucideIconProps {
          icon: "volume-2",
          size: 16.0,
          color: theme::palette().text_inverse,
        })),
    )
    .child(Text::new(app_name).variant(theme::TypographyStyle::Heading))
}
