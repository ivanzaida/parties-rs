use lurq::{
  app::{
    component::{ComponentInfo, DevtoolsFormatter, DevtoolsInspectable},
    ctx::Ctx,
    events::MouseEvent,
  },
  components::{
    ChromeBorderPolicy, ChromeTitleBar, ResizeHandlePolicy, Row, Text, WindowChrome, WindowChromeMode,
    WindowChromeProps,
  },
  core::Signal,
  layout::{Alignment, layout_kind::Justify},
  node::{BackgroundColor, CursorIcon, Element, Style, border::Border, color::Color, dimension::Dimension},
};

use crate::{
  session::ServerSession,
  theme,
  ui::{
    brand_logo::logo_mark,
    common::lucide_icon::{LucideIcon, LucideIconProps},
  },
};

pub const CUSTOM_MACOS_CHROME: bool = cfg!(target_os = "macos");
pub const CUSTOM_WINDOW_CHROME: bool = cfg!(target_os = "windows") || CUSTOM_MACOS_CHROME;
pub(crate) const CHROME_HEIGHT: f32 = if CUSTOM_MACOS_CHROME {
  28.0
} else if cfg!(target_os = "windows") {
  36.0
} else {
  0.0
};
pub(crate) const RESIZE_HANDLE_SIZE: f32 = if CUSTOM_WINDOW_CHROME { 3.0 } else { 0.0 };

#[derive(Clone, Debug)]
pub struct FrameRateSignal(pub Signal<u32>);

impl PartialEq for FrameRateSignal {
  fn eq(&self, other: &Self) -> bool {
    self.0.id() == other.0.id()
  }
}

impl DevtoolsInspectable for FrameRateSignal {
  fn inspect(&self, formatter: &mut DevtoolsFormatter<'_>) {
    formatter.buffer_mut().push(ComponentInfo::with_value(
      "signal",
      std::any::type_name::<Signal<u32>>(),
      self.0.id().to_string(),
    ));
  }
}

pub(crate) fn wrap_window_chrome(
  ctx: &mut Ctx,
  content: impl Into<Element>,
  frame_rate: FrameRateSignal,
  session: ServerSession,
) -> Element {
  let content = content.into();
  if !CUSTOM_WINDOW_CHROME {
    return content;
  }

  WindowChrome::new()
    .props(window_chrome_props())
    .title_bar(
      ChromeTitleBar::new()
        .height(CHROME_HEIGHT)
        .background(BackgroundColor::Palette(theme::PaletteColor::SurfacePanel))
        .border_bottom(Border::inside(1.0, theme::PaletteColor::Border))
        .leading(titlebar_identity(ctx, frame_rate))
        .trailing(window_controls(ctx, session))
        .without_controls(),
    )
    .content(content)
    .mount(ctx)
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
  let height = content_height(ctx);

  Row::new()
    .width(width)
    .height(height)
    .absolute(0.0, 0.0, width, height)
    .clip()
    .child(content)
}

fn window_chrome_props() -> WindowChromeProps {
  WindowChromeProps::new()
    .mode(WindowChromeMode::CustomDesktop)
    .windows_height(36.0)
    .macos_height(28.0)
    .resize_handles(ResizeHandlePolicy::Enabled {
      size: RESIZE_HANDLE_SIZE,
    })
    .border(ChromeBorderPolicy::Visible {
      size: 1.0,
      color: BackgroundColor::Palette(theme::PaletteColor::BorderStrong),
    })
}

fn titlebar_identity(ctx: &mut Ctx, frame_rate: FrameRateSignal) -> Element {
  let fps_label = format!("{} FPS", frame_rate.0.get());

  Row::new()
    .height(Dimension::Pct(100.0))
    .align_items(Alignment::Center)
    .spacing(theme::SpacingSize::Md)
    .padding_left(theme::SpacingSize::Lg)
    .child(
      Row::new()
        .width(22.0)
        .height(22.0)
        .align_items(Alignment::Center)
        .justify(Justify::Center)
        .child(logo_mark(22.0, 6.0)),
    )
    .child(
      Text::new(&ctx.t("common.app_name"))
        .variant(theme::TypographyStyle::Button)
        .color(theme::PaletteColor::TextSecondary),
    )
    .child(
      Text::new(concat!("v", env!("CARGO_PKG_VERSION")))
        .variant(theme::TypographyStyle::Caption)
        .color(theme::PaletteColor::TextMuted),
    )
    .child(
      Text::new(&fps_label)
        .variant(theme::TypographyStyle::Caption)
        .color(theme::PaletteColor::TextMuted),
    )
    .into()
}

fn window_controls(ctx: &mut Ctx, session: ServerSession) -> Element {
  if CUSTOM_MACOS_CHROME {
    return macos_window_controls(ctx, session);
  }

  windows_window_controls(ctx, session)
}

fn windows_window_controls(ctx: &mut Ctx, session: ServerSession) -> Element {
  let window = ctx.window();
  let minimize_window = window.clone();
  let maximize_window = window.clone();
  let close_window = window.clone();
  let maximized = window.is_maximized;

  Row::new()
    .height(Dimension::Pct(100.0))
    .align_items(Alignment::Center)
    .child(
      window_control_button(ctx, "minus", ControlTone::Default).on_click(move |event: MouseEvent| {
        minimize_window.set_minimized(true);
        event.prevent_default();
        event.stop_immediate_propagation();
      }),
    )
    .child(
      window_control_button(
        ctx,
        if maximized { "minimize-2" } else { "maximize" },
        ControlTone::Default,
      )
      .on_click(move |event: MouseEvent| {
        maximize_window.set_maximized(!maximized);
        event.prevent_default();
        event.stop_immediate_propagation();
      }),
    )
    .child(
      window_control_button(ctx, "x", ControlTone::Danger).on_click(move |event: MouseEvent| {
        session.disconnect_for_shutdown();
        close_window.close();
        event.prevent_default();
        event.stop_immediate_propagation();
      }),
    )
    .into()
}

fn macos_window_controls(ctx: &mut Ctx, session: ServerSession) -> Element {
  let window = ctx.window();
  let close_window = window.clone();
  let minimize_window = window.clone();
  let maximize_window = window.clone();
  let maximized = window.is_maximized;

  Row::new()
    .height(Dimension::Pct(100.0))
    .align_items(Alignment::Center)
    .spacing(0.0)
    .padding_left(8.0)
    .child(
      macos_window_control_button("#FF5F57", "#E2463F").on_click(move |event: MouseEvent| {
        session.disconnect_for_shutdown();
        close_window.close();
        event.prevent_default();
        event.stop_immediate_propagation();
      }),
    )
    .child(
      macos_window_control_button("#FFBD2E", "#E0A11B").on_click(move |event: MouseEvent| {
        minimize_window.set_minimized(true);
        event.prevent_default();
        event.stop_immediate_propagation();
      }),
    )
    .child(
      macos_window_control_button("#28C840", "#1EAD34").on_click(move |event: MouseEvent| {
        maximize_window.set_maximized(!maximized);
        event.prevent_default();
        event.stop_immediate_propagation();
      }),
    )
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
    .on_mouse_down(|event: MouseEvent| {
      event.prevent_default();
      event.stop_immediate_propagation();
    })
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
    .on_mouse_down(|event: MouseEvent| {
      event.prevent_default();
      event.stop_immediate_propagation();
    })
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
