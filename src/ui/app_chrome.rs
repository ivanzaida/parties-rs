use lurq::{
  app::{WindowHandle, WindowResizeDirection, component::Component, ctx::Ctx, events::MouseButton},
  components::{Row, Text},
  layout::{Alignment, layout_kind::Justify},
  node::{BackgroundColor, CursorIcon, Element, Style, border::Border, color::Color, dimension::Dimension},
};

use crate::{
  session::ServerSession,
  theme,
  ui::common::lucide_icon::{LucideIcon, LucideIconProps},
};

pub(crate) const CUSTOM_WINDOWS_CHROME: bool = cfg!(target_os = "windows");
pub(crate) const CUSTOM_MACOS_CHROME: bool = cfg!(target_os = "macos");
pub(crate) const CUSTOM_WINDOW_CHROME: bool = CUSTOM_WINDOWS_CHROME || CUSTOM_MACOS_CHROME;
pub(crate) const CHROME_HEIGHT: f32 = if CUSTOM_MACOS_CHROME {
  28.0
} else if CUSTOM_WINDOWS_CHROME {
  36.0
} else {
  0.0
};
pub(crate) const RESIZE_HANDLE_SIZE: f32 = if CUSTOM_WINDOW_CHROME { 6.0 } else { 0.0 };

pub struct AppChrome;

impl Component for AppChrome {
  type Props = ();

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    if CUSTOM_MACOS_CHROME {
      return Row::new()
        .width(Dimension::Pct(100.0))
        .height(CHROME_HEIGHT)
        .align_items(Alignment::Center)
        .background(BackgroundColor::Palette(theme::PaletteColor::SurfacePanel))
        .border_bottom(Border::inside(1.0, theme::PaletteColor::Border))
        .child(window_controls(ctx))
        .child(macos_window_drag_region(ctx));
    }

    Row::new()
      .width(Dimension::Pct(100.0))
      .height(CHROME_HEIGHT)
      .align_items(Alignment::Center)
      .background(BackgroundColor::Palette(theme::PaletteColor::SurfacePanel))
      .border_bottom(Border::inside(1.0, theme::PaletteColor::Border))
      .child(window_drag_region(ctx))
      .child(window_controls(ctx))
  }
}

pub(crate) fn content_height(ctx: &Ctx) -> f32 {
  (ctx.window().logical_height() - CHROME_HEIGHT).max(0.0)
}

pub(crate) fn modal_y(y: f32) -> f32 {
  (y - CHROME_HEIGHT).max(0.0)
}

pub(crate) fn modal_layer(ctx: &mut Ctx, content: impl Into<Element>) -> Row {
  let window = ctx.window();
  let width = window.logical_width();
  let height = (window.logical_height() - CHROME_HEIGHT).max(0.0);

  Row::new()
    .width(width)
    .height(height)
    .absolute(0.0, CHROME_HEIGHT, width, height)
    .clip()
    .child(content)
}

pub(crate) fn window_affordance_layers(ctx: &mut Ctx) -> Vec<Element> {
  if !CUSTOM_WINDOW_CHROME {
    return Vec::new();
  }

  let mut layers = window_border_strips(ctx);
  layers.extend(window_resize_handles(ctx));
  layers
}

fn window_border_strips(ctx: &Ctx) -> Vec<Element> {
  if CUSTOM_MACOS_CHROME {
    return Vec::new();
  }

  let window = ctx.window();
  let width = window.logical_width();
  let height = window.logical_height();
  let size = 1.0;
  let horizontal_width = width.max(0.0);
  let vertical_height = height.max(0.0);
  let right = (width - size).max(0.0);
  let bottom = (height - size).max(0.0);

  vec![
    border_strip(0.0, 0.0, horizontal_width, size),
    border_strip(0.0, bottom, horizontal_width, size),
    border_strip(0.0, 0.0, size, vertical_height),
    border_strip(right, 0.0, size, vertical_height),
  ]
}

fn border_strip(x: f32, y: f32, width: f32, height: f32) -> Element {
  Row::new()
    .absolute(x, y, width, height)
    .background(BackgroundColor::Palette(theme::PaletteColor::BorderStrong))
    .into()
}

fn window_resize_handles(ctx: &mut Ctx) -> Vec<Element> {
  let window = ctx.window();
  if window.is_maximized || window.is_full_screen {
    return Vec::new();
  }

  let width = window.logical_width();
  let height = window.logical_height();
  let edge = RESIZE_HANDLE_SIZE;
  let horizontal_width = (width - edge * 2.0).max(0.0);
  let vertical_height = (height - edge * 2.0).max(0.0);

  vec![
    resize_handle(
      window.clone(),
      WindowResizeDirection::North,
      edge,
      0.0,
      horizontal_width,
      edge,
      CursorIcon::NResize,
    ),
    resize_handle(
      window.clone(),
      WindowResizeDirection::South,
      edge,
      height - edge,
      horizontal_width,
      edge,
      CursorIcon::SResize,
    ),
    resize_handle(
      window.clone(),
      WindowResizeDirection::West,
      0.0,
      edge,
      edge,
      vertical_height,
      CursorIcon::WResize,
    ),
    resize_handle(
      window.clone(),
      WindowResizeDirection::East,
      width - edge,
      edge,
      edge,
      vertical_height,
      CursorIcon::EResize,
    ),
    resize_handle(
      window.clone(),
      WindowResizeDirection::NorthWest,
      0.0,
      0.0,
      edge,
      edge,
      CursorIcon::NwResize,
    ),
    resize_handle(
      window.clone(),
      WindowResizeDirection::NorthEast,
      width - edge,
      0.0,
      edge,
      edge,
      CursorIcon::NeResize,
    ),
    resize_handle(
      window.clone(),
      WindowResizeDirection::SouthWest,
      0.0,
      height - edge,
      edge,
      edge,
      CursorIcon::SwResize,
    ),
    resize_handle(
      window,
      WindowResizeDirection::SouthEast,
      width - edge,
      height - edge,
      edge,
      edge,
      CursorIcon::SeResize,
    ),
  ]
}

fn resize_handle(
  window: WindowHandle,
  direction: WindowResizeDirection,
  x: f32,
  y: f32,
  width: f32,
  height: f32,
  cursor: CursorIcon,
) -> Element {
  Row::new()
    .absolute(x, y, width, height)
    .background(BackgroundColor::Color(Color::from_hex("#00000000")))
    .cursor(cursor)
    .on_mouse_down(move |event| {
      if event.button == MouseButton::Left {
        window.start_resize(direction);
      }
    })
    .into()
}

fn window_drag_region(ctx: &mut Ctx) -> Element {
  let window = ctx.window();
  let drag_window = window.clone();
  let stop_drag_window = window.clone();
  let maximize_window = window.clone();
  let maximized = window.is_maximized;

  Row::new()
    .height(Dimension::Pct(100.0))
    .flex(1.0)
    .align_items(Alignment::Center)
    .spacing(theme::SpacingSize::Md)
    .padding_left(theme::SpacingSize::Lg)
    .on_drag_start(move |event| {
      if event.button == MouseButton::Left && !maximized {
        drag_window.start_drag();
      }
    })
    .on_drag_end(move |_| stop_drag_window.stop_drag())
    .on_dblclick(move |_| maximize_window.set_maximized(!maximized))
    .child(
      Row::new()
        .width(22.0)
        .height(22.0)
        .align_items(Alignment::Center)
        .justify(Justify::Center)
        .rounded(theme::RadiusSize::Md)
        .background(BackgroundColor::Palette(theme::PaletteColor::Accent))
        .child(ctx.mount::<LucideIcon>(LucideIconProps {
          icon: "volume-2",
          size: 13.0,
          color: theme::palette().text_inverse,
        })),
    )
    .child(
      Text::new(&ctx.t("common.app_name"))
        .variant(theme::TypographyStyle::Button)
        .color(theme::PaletteColor::TextSecondary),
    )
    .into()
}

fn macos_window_drag_region(ctx: &mut Ctx) -> Element {
  let window = ctx.window();
  let drag_window = window.clone();
  let stop_drag_window = window.clone();
  let maximize_window = window.clone();
  let maximized = window.is_maximized;

  Row::new()
    .height(Dimension::Pct(100.0))
    .flex(1.0)
    .on_drag_start(move |event| {
      if event.button == MouseButton::Left && !maximized {
        drag_window.start_drag();
      }
    })
    .on_drag_end(move |_| stop_drag_window.stop_drag())
    .on_dblclick(move |_| maximize_window.set_maximized(!maximized))
    .into()
}

fn window_controls(ctx: &mut Ctx) -> Element {
  if CUSTOM_MACOS_CHROME {
    return macos_window_controls(ctx);
  }

  windows_window_controls(ctx)
}

fn windows_window_controls(ctx: &mut Ctx) -> Element {
  let window = ctx.window();
  let minimize_window = window.clone();
  let maximize_window = window.clone();
  let close_window = window.clone();
  let session = ctx.use_context::<ServerSession>();
  let maximized = window.is_maximized;

  Row::new()
    .height(Dimension::Pct(100.0))
    .align_items(Alignment::Center)
    .child(
      window_control_button(ctx, "minus", ControlTone::Default).on_click(move |_| {
        minimize_window.set_minimized(true);
      }),
    )
    .child(
      window_control_button(
        ctx,
        if maximized { "minimize-2" } else { "maximize" },
        ControlTone::Default,
      )
      .on_click(move |_| {
        maximize_window.set_maximized(!maximized);
      }),
    )
    .child(window_control_button(ctx, "x", ControlTone::Danger).on_click(move |_| {
      if let Some(session) = session.as_ref() {
        session.disconnect_for_shutdown();
      }
      close_window.close();
    }))
    .into()
}

fn macos_window_controls(ctx: &mut Ctx) -> Element {
  let window = ctx.window();
  let close_window = window.clone();
  let minimize_window = window.clone();
  let maximize_window = window.clone();
  let session = ctx.use_context::<ServerSession>();
  let maximized = window.is_maximized;

  Row::new()
    .height(Dimension::Pct(100.0))
    .align_items(Alignment::Center)
    .spacing(0.0)
    .padding_left(8.0)
    .child(macos_window_control_button("#FF5F57", "#E2463F").on_click(move |_| {
      if let Some(session) = session.as_ref() {
        session.disconnect_for_shutdown();
      }
      close_window.close();
    }))
    .child(macos_window_control_button("#FFBD2E", "#E0A11B").on_click(move |_| {
      minimize_window.set_minimized(true);
    }))
    .child(macos_window_control_button("#28C840", "#1EAD34").on_click(move |_| {
      maximize_window.set_maximized(!maximized);
    }))
    .into()
}

#[derive(Clone, Copy)]
enum ControlTone {
  Default,
  Danger,
}

fn window_control_button(ctx: &mut Ctx, icon: &'static str, tone: ControlTone) -> Row {
  let (hover_background, active_background, icon_color) = match tone {
    ControlTone::Default => (
      BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised),
      BackgroundColor::Palette(theme::PaletteColor::SurfaceInput),
      theme::palette().text_muted,
    ),
    ControlTone::Danger => (
      BackgroundColor::Palette(theme::PaletteColor::Danger),
      BackgroundColor::Palette(theme::PaletteColor::DangerMuted),
      theme::palette().text_muted,
    ),
  };

  Row::new()
    .width(46.0)
    .height(CHROME_HEIGHT)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .background(BackgroundColor::Color(Color::from_hex("#00000000")))
    .cursor(CursorIcon::Pointer)
    .hovered_style(Style::new().background(hover_background))
    .active_style(Style::new().background(active_background))
    .child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon,
      size: 13.0,
      color: icon_color,
    }))
}

fn macos_window_control_button(color: &'static str, active_color: &'static str) -> Row {
  Row::new()
    .width(20.0)
    .height(CHROME_HEIGHT)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .cursor(CursorIcon::Pointer)
    .child(
      Row::new()
        .width(12.0)
        .height(12.0)
        .rounded(6.0)
        .background(BackgroundColor::Color(Color::from_hex(color)))
        .hovered_style(Style::new().background(BackgroundColor::Color(Color::from_hex(color))))
        .active_style(Style::new().background(BackgroundColor::Color(Color::from_hex(active_color)))),
    )
}
