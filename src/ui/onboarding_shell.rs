use lurq::{
  app::{ctx::Ctx, theme::Breakpoint},
  components::{Column, Row, ScrollVertical, Stack, Text},
  layout::{
    Alignment,
    layout_kind::Justify,
    scrollbar::{ScrollBarPlacement, ScrollBarStyle},
  },
  node::{BackgroundColor, Element, Gradient, GradientStop, color::Color, dimension::Dimension},
};

use crate::{
  theme,
  ui::{
    brand_logo::logo_mark,
    common::lucide_icon::{LucideIcon, LucideIconProps},
  },
};

pub struct OnboardingIntroCopy<'a> {
  pub app_name: &'a str,
  pub headline: &'a str,
  pub description: &'a str,
  pub footer_note: &'a str,
}

#[derive(Clone, Copy)]
struct OnboardingLayoutMetrics {
  intro_width_pct: f32,
  panel_width_pct: f32,
  intro_padding: f32,
  intro_stack_spacing: f32,
  intro_copy_spacing: f32,
  intro_text_max_width: f32,
  panel_padding_x: f32,
  panel_padding_y: f32,
  panel_content_max_width: f32,
}

fn onboarding_layout_metrics(ctx: &Ctx) -> OnboardingLayoutMetrics {
  onboarding_layout_metrics_for(ctx.breakpoint())
}

fn onboarding_layout_metrics_for(breakpoint: Option<Breakpoint>) -> OnboardingLayoutMetrics {
  match breakpoint {
    Some(Breakpoint::Md) => OnboardingLayoutMetrics {
      intro_width_pct: 48.0,
      panel_width_pct: 52.0,
      intro_padding: 32.0,
      intro_stack_spacing: 28.0,
      intro_copy_spacing: 18.0,
      intro_text_max_width: 320.0,
      panel_padding_x: 32.0,
      panel_padding_y: 36.0,
      panel_content_max_width: 360.0,
    },
    Some(Breakpoint::Lg) => OnboardingLayoutMetrics {
      intro_width_pct: 50.0,
      panel_width_pct: 50.0,
      intro_padding: 44.0,
      intro_stack_spacing: 32.0,
      intro_copy_spacing: 20.0,
      intro_text_max_width: 420.0,
      panel_padding_x: 48.0,
      panel_padding_y: 48.0,
      panel_content_max_width: 430.0,
    },
    Some(Breakpoint::Xl) | Some(Breakpoint::Sm) | None => OnboardingLayoutMetrics {
      intro_width_pct: 50.0,
      panel_width_pct: 50.0,
      intro_padding: 56.0,
      intro_stack_spacing: 36.0,
      intro_copy_spacing: 22.0,
      intro_text_max_width: 470.0,
      panel_padding_x: 64.0,
      panel_padding_y: 56.0,
      panel_content_max_width: 430.0,
    },
  }
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
  let metrics = onboarding_layout_metrics(ctx);

  Stack::new()
    .width(Dimension::Pct(metrics.intro_width_pct))
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
        .padding(metrics.intro_padding)
        .clip()
        .child(
          Column::new()
            .width(Dimension::Pct(100.0))
            .spacing(metrics.intro_stack_spacing)
            .justify(Justify::Center)
            .flex(1.0)
            .child(brand(ctx, copy.app_name))
            .child(
              Column::new()
                .width(Dimension::Pct(100.0))
                .spacing(metrics.intro_copy_spacing)
                .child(Text::new(copy.headline).variant(theme::TypographyStyle::Title))
                .child(
                  Text::new(copy.description)
                    .variant(theme::TypographyStyle::Description)
                    .width(Dimension::Pct(100.0))
                    .max_width(metrics.intro_text_max_width),
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

pub fn panel(breakpoint: Option<Breakpoint>, content: impl Into<Element>) -> Column {
  let metrics = onboarding_layout_metrics_for(breakpoint);

  Column::new()
    .width(Dimension::Pct(metrics.panel_width_pct))
    .height(Dimension::Pct(100.0))
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceBase))
    .clip()
    .child(
      ScrollVertical::new(
        Column::new()
          .width(Dimension::Pct(100.0))
          .min_height(Dimension::Pct(100.0))
          .align_items(Alignment::Center)
          .justify(Justify::Center)
          .padding_vertical(metrics.panel_padding_y)
          .padding_horizontal(metrics.panel_padding_x)
          .child(
            Column::new()
              .width(Dimension::Pct(100.0))
              .max_width(metrics.panel_content_max_width)
              .child(content),
          ),
      )
      .width(Dimension::Pct(100.0))
      .height(Dimension::Pct(100.0))
      .scrollbar(onboarding_scrollbar_style())
      .scrollbar_hovered(|mut style| {
        let palette = theme::palette();
        style.thumb_color = palette.accent_hover;
        style.track_color = palette.surface_input.with_opacity(0.75);
        style
      }),
    )
}

fn onboarding_scrollbar_style() -> ScrollBarStyle {
  let palette = theme::palette();
  ScrollBarStyle {
    width: 8.0,
    min_thumb_length: 32.0,
    track_color: palette.surface_input.with_opacity(0.55),
    thumb_color: palette.accent,
    thumb_radius: 4.0,
    track_radius: 4.0,
    padding: 0.0,
    placement: ScrollBarPlacement::Reserved,
    ..ScrollBarStyle::default()
  }
}

fn brand(_ctx: &mut Ctx, app_name: &str) -> Row {
  Row::new()
    .align_items(Alignment::Center)
    .spacing(12.0)
    .child(
      Row::new()
        .width(32.0)
        .height(32.0)
        .align_items(Alignment::Center)
        .justify(Justify::Center)
        .child(logo_mark(32.0, 8.0)),
    )
    .child(Text::new(app_name).variant(theme::TypographyStyle::Heading))
}
