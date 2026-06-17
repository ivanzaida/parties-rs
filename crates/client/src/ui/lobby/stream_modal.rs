use std::sync::Arc;

use lurq::{
  animation::Transition,
  app::{ctx::Ctx, events::KeyboardEvent},
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
  routes::ROUTE_SETTINGS_STREAM,
  services::{
    hotkeys,
    screen_share_sources::{
      ScreenSharePreview, ScreenSharePreviewKey, ScreenShareSource, ScreenShareSourceKind, list_screen_sources,
      list_webcam_sources, list_webcam_sources_with_labels, list_window_sources, load_source_preview,
    },
  },
  theme,
  ui::{
    app_chrome::content_height,
    common::lucide_icon::{LucideIcon, LucideIconProps},
    loader::loader,
    settings::{SettingsPage, SettingsPopupHandle},
  },
};

const STREAM_TOGGLE_TRANSITION_MS: u64 = 240;
const STREAM_MODAL_HEADER_HEIGHT: f32 = 44.0;
const STREAM_MODAL_AUDIO_HEIGHT: f32 = 74.0;
const STREAM_MODAL_ACTIONS_HEIGHT: f32 = 34.0;
const STREAM_MODAL_ERROR_HEIGHT: f32 = 76.0;

#[derive(Clone, Copy)]
struct StreamModalMetrics {
  dialog_width: f32,
  dialog_height: f32,
  padding: f32,
  spacing: f32,
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
    source_columns,
    source_card_height,
    source_preview_height,
    source_grid_height: source_grid_height.min(content_scroll_height.max(148.0)),
  }
}

pub(super) fn start_stream_modal(
  ctx: &mut Ctx,
  open: Signal<bool>,
  source_kind: Signal<ScreenShareSourceKind>,
  source_index: Signal<usize>,
  audio_enabled: Signal<bool>,
  stream_codec_label: String,
  start_stream: StartStreamAction,
  start_submitted: Signal<bool>,
) -> Element {
  let window = ctx.window();
  let window_width = window.logical_width();
  let modal_height = content_height(ctx);
  let metrics = stream_modal_metrics(ctx);
  let close_on_escape = open.clone();
  let settings_popup = ctx.use_context::<SettingsPopupHandle>();
  let settings_open = settings_popup.as_ref().is_some_and(SettingsPopupHandle::is_open);
  let dialog_ref = ctx.element_ref();
  let close_on_outside = open.clone();
  ctx.on_click_outside(dialog_ref.clone(), move |_| {
    if settings_open {
      return;
    }
    close_on_outside.set(false);
  });
  let start_state = start_stream.state().get();
  let submitted = start_submitted.get();
  if submitted && start_state.is_fulfilled() {
    open.set(false);
    start_submitted.set(false);
  }
  let start_error = if submitted { start_state.error.clone() } else { None };
  let source_scroll_height = stream_modal_source_scroll_height(metrics, start_error.is_some());
  let mut dialog = Column::new()
    .width(metrics.dialog_width)
    .height(metrics.dialog_height)
    .max_height(metrics.dialog_height)
    .spacing(metrics.spacing)
    .padding(metrics.padding)
    .rounded(10.0)
    .ref_element(dialog_ref)
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
            source_kind.clone(),
            source_index.clone(),
            &stream_codec_label,
            metrics,
          )),
      )
      .width(Dimension::Pct(100.0))
      .height(source_scroll_height)
      .scrollbar(source_grid_scrollbar_style())
      .scrollbar_hovered(|mut style| {
        let palette = theme::palette();
        style.thumb_color = palette.accent_hover;
        style.track_color = palette.surface_input.with_opacity(0.75);
        style
      }),
    )
    .child(stream_modal_audio_toggle(ctx, audio_enabled.clone()));

  if let Some(error) = start_error.as_deref() {
    dialog = dialog.child(stream_modal_error(ctx, error));
  }

  dialog = dialog.child(stream_modal_actions(
    ctx,
    open,
    source_kind,
    source_index,
    audio_enabled,
    start_stream,
    start_submitted,
  ));

  Column::new()
    .width(window_width)
    .height(modal_height)
    .absolute(0.0, 0.0, window_width, modal_height)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .background(BackgroundColor::Color(Color::from_hex("#00000099")))
    .child(dialog)
    .on_key_down(move |event: KeyboardEvent| {
      if hotkeys::is_cancel_key(&event) && !settings_open {
        close_on_escape.set(false);
      }
    })
    .into()
}

fn stream_modal_source_scroll_height(metrics: StreamModalMetrics, has_error: bool) -> f32 {
  let fixed_height = STREAM_MODAL_HEADER_HEIGHT
    + STREAM_MODAL_AUDIO_HEIGHT
    + STREAM_MODAL_ACTIONS_HEIGHT
    + metrics.spacing * 3.0
    + if has_error {
      STREAM_MODAL_ERROR_HEIGHT + metrics.spacing
    } else {
      0.0
    };
  (metrics.dialog_height - metrics.padding * 2.0 - fixed_height).max(132.0)
}

fn stream_modal_error(ctx: &mut Ctx, message: &str) -> Element {
  Row::new()
    .width(Dimension::Pct(100.0))
    .align_items(Alignment::Center)
    .spacing(theme::SpacingSize::Md)
    .padding(theme::SpacingSize::Md)
    .rounded(theme::RadiusSize::Md)
    .background(BackgroundColor::Palette(theme::PaletteColor::DangerMuted))
    .border_inside(1.0, theme::PaletteColor::Danger)
    .child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon: "triangle-alert",
      size: 16.0,
      color: theme::palette().danger,
    }))
    .child(
      Text::new(message)
        .variant(theme::TypographyStyle::Description)
        .color(theme::PaletteColor::Danger)
        .width(Dimension::Pct(100.0))
        .flex(1.0),
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
  source_kind: Signal<ScreenShareSourceKind>,
  source_index: Signal<usize>,
  stream_codec_label: &str,
  metrics: StreamModalMetrics,
) -> Element {
  Column::new()
    .width(Dimension::Pct(100.0))
    .spacing(12.0)
    .child(stream_modal_tabs(ctx, source_kind.clone(), source_index.clone()))
    .child(stream_source_grid(
      ctx,
      source_kind,
      source_index,
      stream_codec_label,
      metrics,
    ))
    .into()
}

fn stream_modal_tabs(
  ctx: &mut Ctx,
  source_kind: Signal<ScreenShareSourceKind>,
  source_index: Signal<usize>,
) -> Element {
  let active_kind = source_kind.get();
  Row::new()
    .width(Dimension::Pct(100.0))
    .spacing(3.0)
    .padding(3.0)
    .rounded(theme::RadiusSize::Lg)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceInput))
    .child(stream_modal_tab(
      ctx,
      "lobby.stream_modal.tab.screen",
      active_kind == ScreenShareSourceKind::Screen,
      source_kind.clone(),
      source_index.clone(),
      ScreenShareSourceKind::Screen,
    ))
    .child(stream_modal_tab(
      ctx,
      "lobby.stream_modal.tab.window",
      active_kind == ScreenShareSourceKind::Window,
      source_kind.clone(),
      source_index.clone(),
      ScreenShareSourceKind::Window,
    ))
    .child(stream_modal_tab(
      ctx,
      "lobby.stream_modal.tab.webcam",
      active_kind == ScreenShareSourceKind::Webcam,
      source_kind,
      source_index,
      ScreenShareSourceKind::Webcam,
    ))
    .into()
}

fn stream_modal_tab(
  ctx: &mut Ctx,
  label_key: &'static str,
  active: bool,
  source_kind: Signal<ScreenShareSourceKind>,
  source_index: Signal<usize>,
  value: ScreenShareSourceKind,
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
    .on_click(move |_| {
      source_kind.set(value);
      source_index.set(0);
    })
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
  source_kind: Signal<ScreenShareSourceKind>,
  source_index: Signal<usize>,
  stream_codec_label: &str,
  metrics: StreamModalMetrics,
) -> Element {
  let selected_kind = source_kind.get();
  let sources = list_sources(ctx, selected_kind);
  let selected_index = source_index.get().min(sources.len().saturating_sub(1));

  if sources.is_empty() {
    return stream_source_empty_state(ctx, selected_kind, metrics);
  }

  let mut grid = Column::new().width(Dimension::Pct(100.0)).spacing(12.0);

  for (row_index, row_sources) in sources.chunks(metrics.source_columns).enumerate() {
    grid = grid.child(stream_source_row(
      ctx,
      row_sources,
      row_index * metrics.source_columns,
      selected_index,
      source_index.clone(),
      stream_codec_label,
      metrics,
    ));
  }

  grid.into()
}

fn stream_source_empty_state(
  ctx: &mut Ctx,
  source_kind: ScreenShareSourceKind,
  metrics: StreamModalMetrics,
) -> Element {
  Row::new()
    .width(Dimension::Pct(100.0))
    .height(metrics.source_grid_height)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .rounded(8.0)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceInput))
    .border_inside(1.0, theme::PaletteColor::Border)
    .child(
      Text::new(&ctx.t(match source_kind {
        ScreenShareSourceKind::Screen => "lobby.stream_modal.source.empty_screens",
        ScreenShareSourceKind::Window => "lobby.stream_modal.source.empty_windows",
        ScreenShareSourceKind::Webcam => "lobby.stream_modal.source.empty_webcams",
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
  stream_codec_label: &str,
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
      stream_codec_label,
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
  stream_codec_label: &str,
  metrics: StreamModalMetrics,
) -> Element {
  let selected = selected_index == index;
  let select = source_index.clone();
  let preview_key = ScreenSharePreviewKey {
    kind: source.kind,
    id: source.id,
    width: source.width,
    height: source.height,
  };
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
      preview_key,
      source.resolution.as_deref(),
      stream_codec_label,
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
    ScreenShareSourceKind::Webcam => "camera",
  }
}

fn stream_source_preview(
  ctx: &mut Ctx,
  icon: &'static str,
  selected: bool,
  preview_key: ScreenSharePreviewKey,
  resolution: Option<&str>,
  stream_codec_label: &str,
  metrics: StreamModalMetrics,
) -> Element {
  let preview_state = ctx
    .future(preview_key, |key| async move {
      Ok::<ScreenSharePreview, String>(load_source_preview(key).await)
    })
    .state()
    .get();
  let loading = preview_state.is_idle() || preview_state.is_pending();
  let preview_image = preview_state.data.as_ref().and_then(|preview| preview.image.clone());

  let mut preview = Stack::new()
    .width(Dimension::Pct(100.0))
    .height(metrics.source_preview_height)
    .rounded(theme::RadiusSize::Lg)
    .clip()
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised));

  if let Some(image) = preview_image {
    preview = preview.background_image(image).background_contain();
  } else if loading {
    preview = preview.child(
      Column::new()
        .width(Dimension::Pct(100.0))
        .height(metrics.source_preview_height)
        .align_items(Alignment::Center)
        .justify(Justify::Center)
        .child(loader(22.0)),
    );
  } else {
    preview = preview.child(
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
  }

  preview = preview.child(
    Column::new()
      .width(Dimension::Pct(100.0))
      .height(metrics.source_preview_height)
      .padding_top(8.0)
      .padding_horizontal(8.0)
      .child(
        Row::new()
          .width(Dimension::Pct(100.0))
          .align_items(Alignment::Start)
          .justify(Justify::SpaceBetween)
          .spacing(8.0)
          .child(stream_source_metadata_badge(stream_codec_label))
          .child(
            resolution
              .filter(|value| !value.trim().is_empty())
              .map(stream_source_metadata_badge)
              .unwrap_or_else(|| Row::new().into()),
          ),
      ),
  );

  preview.into()
}

fn stream_source_metadata_badge(label: &str) -> Element {
  Row::new()
    .height(20.0)
    .align_items(Alignment::Center)
    .padding_horizontal(6.0)
    .rounded(theme::RadiusSize::Sm)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceInput))
    .border_inside(1.0, theme::PaletteColor::Border)
    .child(
      Text::new(label)
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
  source_kind: Signal<ScreenShareSourceKind>,
  source_index: Signal<usize>,
  audio_enabled: Signal<bool>,
  start_stream: StartStreamAction,
  start_submitted: Signal<bool>,
) -> Element {
  let close = open.clone();
  let settings_submitted = start_submitted.clone();
  let settings_popup = ctx.use_context::<SettingsPopupHandle>();
  let navigator = ctx.navigator();
  let cancel_submitted = start_submitted.clone();
  let run_submitted = start_submitted.clone();
  let run_close = open.clone();
  let pending = start_stream.state().get().is_pending();
  let start_source_kind = source_kind.clone();
  let start_source_index = source_index.clone();
  let start_audio_enabled = audio_enabled.clone();
  Row::new()
    .width(Dimension::Pct(100.0))
    .align_items(Alignment::Center)
    .justify(Justify::SpaceBetween)
    .spacing(10.0)
    .child(
      stream_modal_button(ctx, Some("settings"), "lobby.stream_modal.action.settings", false).on_click(move |_| {
        settings_submitted.set(false);
        if let Some(settings_popup) = settings_popup.as_ref() {
          settings_popup.open_page(SettingsPage::Stream);
        } else if let Some(navigator) = navigator.as_ref() {
          navigator.push(ROUTE_SETTINGS_STREAM);
        }
      }),
    )
    .child(
      Row::new()
        .align_items(Alignment::Center)
        .justify(Justify::End)
        .spacing(10.0)
        .child(
          stream_modal_button(ctx, None, "common.action.cancel", false).on_click(move |_| {
            cancel_submitted.set(false);
            close.set(false);
          }),
        )
        .child({
          let mut button = stream_modal_button(ctx, Some("monitor-up"), "lobby.stream_modal.action.start", true);
          if !pending {
            button = button.on_click(move |_| {
              if let Some(input) = selected_stream_input(
                start_source_kind.get_untracked(),
                start_source_index.get_untracked(),
                start_audio_enabled.get_untracked(),
              ) {
                start_stream.run(input);
                run_submitted.set(false);
                run_close.set(false);
              }
            });
          }
          button
        }),
    )
    .into()
}

fn selected_stream_input(
  source_kind: ScreenShareSourceKind,
  selected_index: usize,
  audio_enabled: bool,
) -> Option<StartStreamInput> {
  let sources = list_sources_for_input(source_kind);
  let source = sources.get(selected_index.min(sources.len().saturating_sub(1)))?;
  Some(StartStreamInput {
    source_kind: source.kind,
    source_id: source.id,
    width: source.width.min(u16::MAX as u32) as u16,
    height: source.height.min(u16::MAX as u32) as u16,
    audio_enabled,
  })
}

fn list_sources(ctx: &mut Ctx, source_kind: ScreenShareSourceKind) -> Vec<ScreenShareSource> {
  match source_kind {
    ScreenShareSourceKind::Screen => list_screen_sources(),
    ScreenShareSourceKind::Window => list_window_sources(),
    ScreenShareSourceKind::Webcam => {
      let camera = ctx.t("lobby.stream_modal.source.camera").to_string();
      let camera_indexed = ctx.t("lobby.stream_modal.source.camera_indexed").to_string();
      list_webcam_sources_with_labels(&camera, &camera, &|index| {
        camera_indexed.replace("{{index}}", &(index + 1).to_string())
      })
    }
  }
}

fn list_sources_for_input(source_kind: ScreenShareSourceKind) -> Vec<ScreenShareSource> {
  match source_kind {
    ScreenShareSourceKind::Screen => list_screen_sources(),
    ScreenShareSourceKind::Window => list_window_sources(),
    ScreenShareSourceKind::Webcam => list_webcam_sources(),
  }
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
