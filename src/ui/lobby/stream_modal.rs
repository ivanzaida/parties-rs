use std::sync::Arc;

use lurq::{
  animation::Transition,
  app::ctx::Ctx,
  components::{Column, Row, ScrollVertical, Stack, Text, TextOverflow},
  core::Signal,
  layout::{
    Alignment,
    layout_kind::Justify,
    scrollbar::{ScrollBarPlacement, ScrollBarStyle},
    text_style::{FontWeight, TextStyle},
  },
  node::{BackgroundColor, CursorIcon, Element, Style, color::Color, dimension::Dimension, transform::Transform2D},
};

use super::{StartStreamAction, StartStreamInput};
use crate::{
  services::screen_share_sources::{
    ScreenShareSource, ScreenShareSourceKind, list_screen_sources, list_window_sources,
  },
  theme,
  ui::{
    app_chrome::{CHROME_HEIGHT, RESIZE_HANDLE_SIZE, content_height},
    common::lucide_icon::{LucideIcon, LucideIconProps},
  },
};

const STREAM_TOGGLE_TRANSITION_MS: u64 = 240;

#[derive(Clone, Copy)]
struct StreamModalMetrics {
  dialog_width: f32,
  dialog_height: f32,
  padding: f32,
  spacing: f32,
  content_scroll_height: f32,
  source_columns: usize,
  source_card_height: f32,
  source_preview_height: f32,
  source_grid_height: f32,
}

fn stream_modal_metrics(ctx: &Ctx) -> StreamModalMetrics {
  let window = ctx.window();
  let window_width = window.logical_width();
  let available_height = (content_height(ctx) - 24.0).max(320.0);
  let compact_height = available_height < 592.0;
  let dialog_width = (window_width - 24.0).min(560.0).max(300.0);
  let dialog_height = available_height.min(592.0);
  let compact_width = dialog_width < 520.0;
  let padding = if compact_height || compact_width { 18.0 } else { 28.0 };
  let spacing = if compact_height || compact_width { 14.0 } else { 20.0 };
  let source_columns = if dialog_width - padding * 2.0 < 420.0 { 1 } else { 2 };
  let source_card_height = if compact_height { 124.0 } else { 140.0 };
  let source_preview_height = if compact_height { 78.0 } else { 96.0 };
  let visible_rows = if compact_height { 1.0 } else { 2.0 };
  let source_grid_height = source_card_height * visible_rows + 12.0 * (visible_rows - 1.0);
  let content_scroll_height = (dialog_height - padding * 2.0 - 44.0 - 34.0 - spacing * 2.0).max(148.0);

  StreamModalMetrics {
    dialog_width,
    dialog_height,
    padding,
    spacing,
    content_scroll_height,
    source_columns,
    source_card_height,
    source_preview_height,
    source_grid_height: source_grid_height.min(content_scroll_height.max(148.0)),
  }
}

pub(super) fn start_stream_modal(
  ctx: &mut Ctx,
  open: Signal<bool>,
  screen_tab: Signal<bool>,
  source_index: Signal<usize>,
  audio_enabled: Signal<bool>,
  start_stream: StartStreamAction,
) -> Element {
  let window = ctx.window();
  let window_width = window.logical_width();
  let modal_height = content_height(ctx);
  let resize_gutter = if window.is_maximized || window.is_full_screen {
    0.0
  } else {
    RESIZE_HANDLE_SIZE
  };
  let layer_width = (window_width - resize_gutter * 2.0).max(0.0);
  let layer_height = (modal_height - resize_gutter).max(0.0);
  let metrics = stream_modal_metrics(ctx);

  Column::new()
    .width(layer_width)
    .height(layer_height)
    .absolute(resize_gutter, CHROME_HEIGHT, layer_width, layer_height)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .background(BackgroundColor::Color(Color::from_hex("#00000099")))
    .child(
      Column::new()
        .width(metrics.dialog_width)
        .height(metrics.dialog_height)
        .max_height(metrics.dialog_height)
        .spacing(metrics.spacing)
        .padding(metrics.padding)
        .rounded(10.0)
        .background(BackgroundColor::Color(Color::from_hex("#15171A")))
        .border_inside(1.0, BackgroundColor::Color(Color::from_hex("#30343A")))
        .child(stream_modal_header(ctx, open.clone()))
        .child(
          ScrollVertical::new(
            Column::new()
              .width(Dimension::Pct(100.0))
              .spacing(metrics.spacing)
              .child(stream_modal_sources(
                ctx,
                screen_tab.clone(),
                source_index.clone(),
                metrics,
              ))
              .child(stream_modal_audio_toggle(ctx, audio_enabled)),
          )
          .width(Dimension::Pct(100.0))
          .height(metrics.content_scroll_height)
          .scrollbar(source_grid_scrollbar_style())
          .scrollbar_hovered(|mut style| {
            let palette = theme::palette();
            style.thumb_color = palette.accent_hover;
            style.track_color = palette.surface_input.with_opacity(0.75);
            style
          }),
        )
        .child(stream_modal_actions(ctx, open, screen_tab, source_index, start_stream)),
    )
    .into()
}

fn stream_modal_header(ctx: &mut Ctx, open: Signal<bool>) -> Element {
  let close = open.clone();
  Row::new()
    .width(Dimension::Pct(100.0))
    .align_items(Alignment::Center)
    .spacing(theme::SpacingSize::Lg)
    .child(
      Row::new()
        .width(44.0)
        .height(44.0)
        .align_items(Alignment::Center)
        .justify(Justify::Center)
        .rounded(12.0)
        .background(BackgroundColor::Palette(theme::PaletteColor::AccentMuted))
        .border_inside(1.0, theme::PaletteColor::Accent)
        .child(ctx.mount::<LucideIcon>(LucideIconProps {
          icon: "monitor-up",
          size: 22.0,
          color: theme::palette().accent,
        })),
    )
    .child(
      Column::new()
        .width(Dimension::Pct(100.0))
        .flex(1.0)
        .spacing(3.0)
        .child(Text::styled(
          &ctx.t("lobby.stream_modal.title"),
          stream_modal_text_style(18.0, FontWeight::Bold, theme::palette().text_primary),
        ))
        .child(
          Text::styled(
            &ctx.t("lobby.stream_modal.subtitle"),
            stream_modal_text_style(13.0, FontWeight::Normal, theme::palette().text_secondary),
          )
          .width(Dimension::Pct(100.0)),
        ),
    )
    .child(
      Row::new()
        .width(30.0)
        .height(30.0)
        .align_items(Alignment::Center)
        .justify(Justify::Center)
        .rounded(theme::RadiusSize::Lg)
        .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceInput))
        .cursor(CursorIcon::Pointer)
        .hovered_style(Style::new().background(BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised)))
        .on_click(move |_| close.set(false))
        .child(ctx.mount::<LucideIcon>(LucideIconProps {
          icon: "x",
          size: 16.0,
          color: theme::palette().text_muted,
        })),
    )
    .into()
}

fn stream_modal_sources(
  ctx: &mut Ctx,
  screen_tab: Signal<bool>,
  source_index: Signal<usize>,
  metrics: StreamModalMetrics,
) -> Element {
  Column::new()
    .width(Dimension::Pct(100.0))
    .spacing(12.0)
    .child(stream_modal_tabs(ctx, screen_tab.clone()))
    .child(stream_source_grid(ctx, screen_tab, source_index, metrics))
    .into()
}

fn stream_modal_tabs(ctx: &mut Ctx, screen_tab: Signal<bool>) -> Element {
  let screen_active = screen_tab.get();
  Row::new()
    .width(Dimension::Pct(100.0))
    .spacing(3.0)
    .padding(3.0)
    .rounded(theme::RadiusSize::Lg)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceInput))
    .child(stream_modal_tab(
      ctx,
      "lobby.stream_modal.tab.screen",
      screen_active,
      screen_tab.clone(),
      true,
    ))
    .child(stream_modal_tab(
      ctx,
      "lobby.stream_modal.tab.window",
      !screen_active,
      screen_tab,
      false,
    ))
    .into()
}

fn stream_modal_tab(
  ctx: &mut Ctx,
  label_key: &'static str,
  active: bool,
  screen_tab: Signal<bool>,
  value: bool,
) -> Element {
  Row::new()
    .height(32.0)
    .width(Dimension::Pct(100.0))
    .flex(1.0)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .rounded(theme::RadiusSize::Md)
    .background(if active {
      BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised)
    } else {
      BackgroundColor::Color(Color::from_hex("#00000000"))
    })
    .cursor(CursorIcon::Pointer)
    .on_click(move |_| screen_tab.set(value))
    .child(Text::styled(
      &ctx.t(label_key),
      stream_modal_text_style(
        13.0,
        FontWeight::Bold,
        if active {
          theme::palette().text_primary
        } else {
          theme::palette().text_secondary
        },
      ),
    ))
    .into()
}

fn stream_source_grid(
  ctx: &mut Ctx,
  screen_tab: Signal<bool>,
  source_index: Signal<usize>,
  metrics: StreamModalMetrics,
) -> Element {
  let sources = if screen_tab.get() {
    list_screen_sources()
  } else {
    list_window_sources()
  };
  let selected_index = source_index.get().min(sources.len().saturating_sub(1));

  if sources.is_empty() {
    return stream_source_empty_state(ctx, screen_tab.get(), metrics);
  }

  let mut grid = Column::new().width(Dimension::Pct(100.0)).spacing(12.0);

  for (row_index, row_sources) in sources.chunks(metrics.source_columns).enumerate() {
    grid = grid.child(stream_source_row(
      ctx,
      row_sources,
      row_index * metrics.source_columns,
      selected_index,
      source_index.clone(),
      metrics,
    ));
  }

  ScrollVertical::new(grid)
    .width(Dimension::Pct(100.0))
    .height(metrics.source_grid_height)
    .scrollbar(source_grid_scrollbar_style())
    .scrollbar_hovered(|mut style| {
      let palette = theme::palette();
      style.thumb_color = palette.accent_hover;
      style.track_color = palette.surface_input.with_opacity(0.75);
      style
    })
    .into()
}

fn stream_source_empty_state(ctx: &mut Ctx, screen_tab: bool, metrics: StreamModalMetrics) -> Element {
  Row::new()
    .width(Dimension::Pct(100.0))
    .height(metrics.source_grid_height)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .rounded(8.0)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceInput))
    .border_inside(1.0, theme::PaletteColor::Border)
    .child(
      Text::new(&ctx.t(if screen_tab {
        "lobby.stream_modal.source.empty_screens"
      } else {
        "lobby.stream_modal.source.empty_windows"
      }))
      .variant(theme::TypographyStyle::Caption)
      .color(theme::PaletteColor::TextMuted),
    )
    .into()
}

fn source_grid_scrollbar_style() -> ScrollBarStyle {
  let palette = theme::palette();
  ScrollBarStyle {
    width: 6.0,
    min_thumb_length: 24.0,
    track_color: palette.surface_input.with_opacity(0.55),
    thumb_color: palette.accent,
    thumb_radius: 3.0,
    track_radius: 3.0,
    padding: 2.0,
    placement: ScrollBarPlacement::Reserved,
    ..ScrollBarStyle::default()
  }
}

fn stream_source_row(
  ctx: &mut Ctx,
  sources: &[ScreenShareSource],
  offset: usize,
  selected_index: usize,
  source_index: Signal<usize>,
  metrics: StreamModalMetrics,
) -> Element {
  let mut row = Row::new().width(Dimension::Pct(100.0)).spacing(12.0);

  for (column_index, source) in sources.iter().enumerate() {
    row = row.child(stream_source_card(
      ctx,
      source,
      offset + column_index,
      selected_index,
      source_index.clone(),
      metrics,
    ));
  }

  if sources.len() < metrics.source_columns {
    row = row.child(Row::new().width(Dimension::Pct(100.0)).flex(1.0));
  }

  row.into()
}

fn stream_source_card(
  ctx: &mut Ctx,
  source: &ScreenShareSource,
  index: usize,
  selected_index: usize,
  source_index: Signal<usize>,
  metrics: StreamModalMetrics,
) -> Element {
  let selected = selected_index == index;
  let select = source_index.clone();
  Column::new()
    .width(Dimension::Pct(100.0))
    .flex(1.0)
    .height(metrics.source_card_height)
    .spacing(8.0)
    .padding(10.0)
    .rounded(8.0)
    .clip()
    .background(BackgroundColor::Color(if selected {
      Color::from_hex("#121A23")
    } else {
      Color::from_hex("#171A1E")
    }))
    .border_inside(
      1.0,
      if selected {
        theme::PaletteColor::Accent
      } else {
        theme::PaletteColor::Border
      },
    )
    .cursor(CursorIcon::Pointer)
    .on_click(move |_| select.set(index))
    .child(stream_source_preview(
      ctx,
      stream_source_icon(source),
      selected,
      source.resolution.as_deref(),
      metrics,
    ))
    .child(
      Row::new()
        .width(Dimension::Pct(100.0))
        .height(16.0)
        .align_items(Alignment::Center)
        .spacing(8.0)
        .child(ctx.mount::<LucideIcon>(LucideIconProps {
          icon: stream_source_icon(source),
          size: 14.0,
          color: if selected {
            theme::palette().accent
          } else {
            theme::palette().text_muted
          },
        }))
        .child(
          Text::styled(
            &source.name,
            stream_modal_text_style(13.0, FontWeight::Bold, theme::palette().text_primary),
          )
          .nowrap()
          .text_overflow(TextOverflow::Elipsis)
          .width(Dimension::Pct(100.0))
          .min_width(0.0)
          .flex(1.0),
        )
        .child(if selected {
          ctx.mount::<LucideIcon>(LucideIconProps {
            icon: "check",
            size: 14.0,
            color: theme::palette().accent,
          })
        } else {
          Row::new().into()
        }),
    )
    .into()
}

fn stream_source_icon(source: &ScreenShareSource) -> &'static str {
  match &source.kind {
    ScreenShareSourceKind::Screen => "monitor",
    ScreenShareSourceKind::Window => "app-window",
  }
}

fn stream_source_preview(
  ctx: &mut Ctx,
  icon: &'static str,
  selected: bool,
  resolution: Option<&str>,
  metrics: StreamModalMetrics,
) -> Element {
  let mut preview = Stack::new()
    .width(Dimension::Pct(100.0))
    .height(metrics.source_preview_height)
    .rounded(theme::RadiusSize::Lg)
    .clip()
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised))
    .child(
      Column::new()
        .width(Dimension::Pct(100.0))
        .height(metrics.source_preview_height)
        .align_items(Alignment::Center)
        .justify(Justify::Center)
        .child(ctx.mount::<LucideIcon>(LucideIconProps {
          icon,
          size: 28.0,
          color: if selected {
            theme::palette().accent
          } else {
            theme::palette().text_muted
          },
        })),
    );

  if let Some(resolution) = resolution.filter(|value| !value.trim().is_empty()) {
    preview = preview.child(
      Column::new()
        .width(Dimension::Pct(100.0))
        .height(metrics.source_preview_height)
        .align_items(Alignment::End)
        .padding_top(8.0)
        .padding_right(8.0)
        .child(stream_source_resolution_badge(resolution)),
    );
  }

  preview.into()
}

fn stream_source_resolution_badge(resolution: &str) -> Element {
  Row::new()
    .height(20.0)
    .align_items(Alignment::Center)
    .padding_horizontal(6.0)
    .rounded(theme::RadiusSize::Sm)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceInput))
    .border_inside(1.0, theme::PaletteColor::Border)
    .child(
      Text::new(resolution)
        .variant(theme::TypographyStyle::FieldLabel)
        .color(theme::PaletteColor::TextMuted)
        .nowrap(),
    )
    .into()
}

fn stream_modal_audio_toggle(ctx: &mut Ctx, audio_enabled: Signal<bool>) -> Element {
  let enabled = audio_enabled.get();
  let palette = theme::palette();
  let knob_translate = if enabled { 16.0 } else { 0.0 };
  let toggle = audio_enabled.clone();
  Row::new()
    .width(Dimension::Pct(100.0))
    .align_items(Alignment::Center)
    .spacing(12.0)
    .padding_vertical(12.0)
    .padding_horizontal(14.0)
    .rounded(8.0)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceInput))
    .border_inside(1.0, theme::PaletteColor::Border)
    .child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon: "volume-2",
      size: 18.0,
      color: theme::palette().text_secondary,
    }))
    .child(
      Column::new()
        .width(Dimension::Pct(100.0))
        .flex(1.0)
        .spacing(2.0)
        .child(Text::styled(
          &ctx.t("lobby.stream_modal.audio.title"),
          stream_modal_text_style(13.0, FontWeight::Bold, theme::palette().text_primary),
        ))
        .child(
          Text::styled(
            &ctx.t("lobby.stream_modal.audio.description"),
            stream_modal_text_style(12.0, FontWeight::Normal, theme::palette().text_muted),
          )
          .width(Dimension::Pct(100.0)),
        ),
    )
    .child(
      Row::new()
        .width(38.0)
        .height(22.0)
        .align_items(Alignment::Center)
        .padding_left(2.0)
        .rounded(11.0)
        .background(BackgroundColor::Color(if enabled {
          palette.accent
        } else {
          palette.surface_raised
        }))
        .transition(Transition::background_color().duration_ms(STREAM_TOGGLE_TRANSITION_MS))
        .cursor(CursorIcon::Pointer)
        .on_click(move |_| toggle.set(!enabled))
        .child(
          Row::new()
            .width(18.0)
            .height(18.0)
            .rounded(9.0)
            .background(BackgroundColor::Color(if enabled {
              palette.surface_base
            } else {
              palette.text_muted
            }))
            .transform(Transform2D::translate(knob_translate, 0.0))
            .transition(Transition::background_color().duration_ms(STREAM_TOGGLE_TRANSITION_MS))
            .transition(Transition::transform().duration_ms(STREAM_TOGGLE_TRANSITION_MS)),
        ),
    )
    .into()
}

fn stream_modal_actions(
  ctx: &mut Ctx,
  open: Signal<bool>,
  screen_tab: Signal<bool>,
  source_index: Signal<usize>,
  start_stream: StartStreamAction,
) -> Element {
  let close = open.clone();
  let confirm_open = open.clone();
  let pending = start_stream.state().get().is_pending();
  let start_screen_tab = screen_tab.clone();
  let start_source_index = source_index.clone();
  Row::new()
    .width(Dimension::Pct(100.0))
    .align_items(Alignment::Center)
    .justify(Justify::End)
    .spacing(10.0)
    .child(
      stream_modal_button(ctx, None, "common.action.cancel", false).on_click(move |_| {
        close.set(false);
      }),
    )
    .child({
      let mut button = stream_modal_button(ctx, Some("monitor-up"), "lobby.stream_modal.action.start", true);
      if !pending {
        button = button.on_click(move |_| {
          if let Some(input) =
            selected_stream_input(start_screen_tab.get_untracked(), start_source_index.get_untracked())
          {
            confirm_open.set(false);
            start_stream.run(input);
          }
        });
      }
      button
    })
    .into()
}

fn selected_stream_input(screen_tab: bool, selected_index: usize) -> Option<StartStreamInput> {
  let sources = if screen_tab {
    list_screen_sources()
  } else {
    list_window_sources()
  };
  let source = sources.get(selected_index.min(sources.len().saturating_sub(1)))?;
  Some(StartStreamInput {
    source_kind: source.kind.clone(),
    source_id: source.id,
    width: source.width.min(u16::MAX as u32) as u16,
    height: source.height.min(u16::MAX as u32) as u16,
  })
}

fn stream_modal_button(ctx: &mut Ctx, icon: Option<&'static str>, label_key: &'static str, primary: bool) -> Row {
  let mut button = Row::new()
    .height(34.0)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .spacing(7.0)
    .padding_horizontal(if primary { 14.0 } else { 16.0 })
    .rounded(theme::RadiusSize::Md)
    .background(if primary {
      BackgroundColor::Palette(theme::PaletteColor::Accent)
    } else {
      BackgroundColor::Color(Color::from_hex("#00000000"))
    })
    .border_inside(
      1.0,
      if primary {
        theme::PaletteColor::Accent
      } else {
        theme::PaletteColor::Border
      },
    )
    .cursor(CursorIcon::Pointer)
    .hovered_style(Style::new().background(BackgroundColor::Palette(if primary {
      theme::PaletteColor::AccentHover
    } else {
      theme::PaletteColor::SurfaceRaised
    })));

  if let Some(icon) = icon {
    button = button.child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon,
      size: 16.0,
      color: if primary {
        theme::palette().text_inverse
      } else {
        theme::palette().text_secondary
      },
    }));
  }

  button.child(Text::styled(
    &ctx.t(label_key),
    stream_modal_text_style(
      13.0,
      FontWeight::Bold,
      if primary {
        theme::palette().text_inverse
      } else {
        theme::palette().text_secondary
      },
    ),
  ))
}

fn stream_modal_text_style(font_size: f32, weight: FontWeight, color: Color) -> TextStyle {
  TextStyle {
    font_family: Arc::from("Inter"),
    font_size,
    line_height: 1.2,
    weight,
    color,
    ..TextStyle::default()
  }
}
