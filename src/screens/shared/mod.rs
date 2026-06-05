use std::sync::Arc;

use lurq::{
  app::{component::Component, ctx::Ctx},
  components::{Button, Column, Control, Rect, Row, Text, TextInput},
  layout::{
    Alignment,
    layout_kind::Justify,
    text_style::{FontWeight, TextStyle},
  },
  node::{BackgroundColor, CursorIcon, Element, Style, color::Color, dimension::Dimension},
};

use crate::theme;

pub const BORDER: &str = "#30343A";
pub const ROUTE_LOADING: &str = "/loading";
pub const ROUTE_IDENTITY_SETUP: &str = "/identity/setup";
pub const ROUTE_SEED_PHRASE: &str = "/identity/seed";
pub const ROUTE_IMPORT_PRIVATE_KEY: &str = "/identity/import";
pub const ROUTE_RESTORE_IDENTITY: &str = "/identity/restore";
pub const ROUTE_CHOOSE_SERVER: &str = "/servers";
pub const ROUTE_CONNECT_SERVER: &str = "/servers/connect";
pub const CONTENT_HEIGHT: f32 = 520.0;
pub const INTRO_WIDTH: f32 = 280.0;
pub const CARD_WIDTH: f32 = 440.0;

pub fn identity_screen(intro: impl Into<Element>, card: impl Into<Element>) -> Column {
  Column::new()
    .width(Dimension::Pct(100.0))
    .height(Dimension::Pct(100.0))
    .justify(Justify::Center)
    .background(BackgroundColor::Palette(theme::BG_PRIMARY))
    .clip()
    .child(
      Column::new()
        .width(Dimension::Pct(100.0))
        .background(BackgroundColor::Palette(theme::BG_PRIMARY))
        .child(
          Row::new()
            .width(Dimension::Pct(100.0))
            .height(CONTENT_HEIGHT)
            .align_items(Alignment::Start)
            .justify(Justify::Center)
            .spacing(40.0)
            .padding_horizontal(72.0)
            .background(BackgroundColor::Palette(theme::BG_PRIMARY))
            .child(intro)
            .child(card),
        ),
    )
}

pub(crate) fn icon(name: &str, size: f32, color: impl Into<Color>) -> Text {
  let ch = match name {
    "arrow-left" => '\u{e048}',
    "alert-circle" | "circle-alert" => '\u{e077}',
    "check" => '\u{e06c}',
    "chevron-right" => '\u{e06f}',
    "copy" => '\u{e09e}',
    "info" => '\u{e0f9}',
    "shield-check" => '\u{e1ff}',
    "trash-2" => '\u{e18e}',
    "alert-triangle" | "triangle-alert" => '\u{e193}',
    _ => '\u{e06f}',
  };
  let glyph = String::from(ch);

  Text::styled(
    &glyph,
    TextStyle {
      font_family: "lucide".into(),
      font_size: size,
      color: color.into(),
      ..TextStyle::default()
    },
  )
}

pub fn styled_text(
  content: &str,
  family: &str,
  size: f32,
  weight: FontWeight,
  color: impl Into<Color>,
  line_height: f32,
) -> Text {
  Text::styled(
    content,
    TextStyle {
      font_family: Arc::from(family),
      font_size: size,
      line_height,
      weight,
      color: color.into(),
      ..TextStyle::default()
    },
  )
}

pub fn text_style(family: &str, size: f32, weight: FontWeight, color: &str, line_height: f32) -> TextStyle {
  TextStyle {
    font_family: Arc::from(family),
    font_size: size,
    line_height,
    weight,
    color: Color::from_hex(color),
    ..TextStyle::default()
  }
}

pub fn dot(color: impl Into<BackgroundColor>) -> Rect {
  Rect::new(8.0, 8.0).rounded(4.0).background(color)
}

#[derive(Clone, Debug, PartialEq, lurq::DevtoolsInspectable)]
pub struct FormTextInputProps {
  pub control: Control<String>,
  pub label: Arc<str>,
  pub placeholder: Arc<str>,
  pub height: f32,
  pub multiline: bool,
}

impl FormTextInputProps {
  pub fn new(
    control: Control<String>,
    label: impl Into<Arc<str>>,
    placeholder: impl Into<Arc<str>>,
    height: f32,
  ) -> Self {
    Self {
      control,
      label: label.into(),
      placeholder: placeholder.into(),
      height,
      multiline: false,
    }
  }

  pub fn multiline(mut self) -> Self {
    self.multiline = true;
    self
  }
}

pub struct FormTextInput;

impl Component for FormTextInput {
  type Props = FormTextInputProps;

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let props = ctx.props::<Self::Props>().clone();
    let control = ctx.form_control(&props.control);
    let raw_error = control.error().get();
    let visible_error = control.visible_error();
    let touched = control.is_touched();
    let submitted = control.has_submit_attempted();
    let invalid = control.is_invalid();
    let submitting = control.is_submitting();
    eprintln!(
      "form_text_input render name={} label={:?} touched={} submitted={} invalid={} submitting={} raw_error={:?} visible_error={:?}",
      control.name(),
      props.label,
      touched,
      submitted,
      invalid,
      submitting,
      raw_error,
      visible_error,
    );
    let blur_control = control.clone();
    let blur_name: Arc<str> = Arc::from(control.name());
    form_text_input_view(
      control.value(),
      control.name(),
      &props.label,
      &props.placeholder,
      props.height,
      props.multiline,
      raw_error.as_deref(),
      Some(move || {
        eprintln!(
          "form_text_input blur start name={} touched={} error={:?}",
          blur_name,
          blur_control.is_touched(),
          blur_control.error().get()
        );
        blur_control.mark_touched();
        let valid = blur_control.validate();
        eprintln!(
          "form_text_input blur end name={} valid={} touched={} error={:?}",
          blur_name,
          valid,
          blur_control.is_touched(),
          blur_control.error().get()
        );
      }),
    )
  }
}

fn form_text_input_view(
  value: lurq::core::Signal<String>,
  name: &str,
  label: &str,
  placeholder: &str,
  height: f32,
  multiline: bool,
  error: Option<&str>,
  on_blur: Option<impl Fn() + Send + Sync + 'static>,
) -> Column {
  let has_error = error.is_some_and(|message| !message.is_empty());
  let label_color = if has_error {
    theme::RED_COLOR
  } else {
    theme::TEXT_MUTED_COLOR
  };
  let input_background = if has_error {
    BackgroundColor::Palette(theme::RED_MUTED)
  } else {
    BackgroundColor::Palette(theme::BG_SECONDARY)
  };
  let input_border = if has_error {
    theme::RED_COLOR
  } else {
    Color::from_hex(BORDER)
  };
  eprintln!(
    "form_text_input view name={} label={:?} placeholder={:?} height={} multiline={} error={:?} has_error={} label_color={} border={} background={}",
    name,
    label,
    placeholder,
    height,
    multiline,
    error,
    has_error,
    label_color.to_hex(),
    input_border.to_hex(),
    if has_error { "RED_MUTED" } else { "BG_SECONDARY" }
  );
  let value_style = text_style("JetBrains Mono", 12.0, FontWeight::Medium, "#F4F4F2", 1.2);
  let placeholder_style = TextStyle {
    color: theme::TEXT_SECONDARY_COLOR,
    ..value_style.clone()
  };
  let input = TextInput::styled(value, value_style)
    .name(name)
    .width(Dimension::Pct(100.0))
    .height(height)
    .padding_horizontal(10.0)
    .rounded(5.0)
    .background(input_background)
    .border_inside(1.0, input_border)
    .caret_color(theme::ACCENT_COLOR)
    .placeholder(placeholder)
    .placeholder_style(placeholder_style);
  let input = if multiline {
    input.padding_vertical(10.0).multiline()
  } else {
    input.single_line()
  };
  let input = if let Some(on_blur) = on_blur {
    input.on_blur(on_blur)
  } else {
    input
  };

  let field = Column::new()
    .width(Dimension::Pct(100.0))
    .spacing(7.0)
    .child(styled_text(
      label,
      "JetBrains Mono",
      10.0,
      FontWeight::Bold,
      label_color,
      1.2,
    ))
    .child(input);

  if let Some(message) = error.filter(|message| !message.is_empty()) {
    eprintln!(
      "form_text_input error_row name={} message={:?} border={}",
      name,
      message,
      input_border.to_hex()
    );
    return field.child(
      Row::new()
        .width(Dimension::Pct(100.0))
        .height(14.0)
        .align_items(Alignment::Center)
        .spacing(6.0)
        .child(icon("circle-alert", 12.0, theme::RED_COLOR))
        .child(styled_text(message, "Inter", 11.0, FontWeight::Bold, theme::RED_COLOR, 1.2).flex(1.0)),
    );
  }

  field
}

pub fn notice_row(
  message: &str,
  icon_name: &str,
  icon_color: impl Into<Color>,
  background: impl Into<BackgroundColor>,
  border: impl Into<Color>,
) -> Row {
  Row::new()
    .width(Dimension::Pct(100.0))
    .align_items(Alignment::Center)
    .spacing(8.0)
    .padding(10.0)
    .rounded(5.0)
    .background(background)
    .border_inside(1.0, border.into())
    .child(icon(icon_name, 14.0, icon_color))
    .child(
      styled_text(
        message,
        "Inter",
        11.0,
        FontWeight::Medium,
        theme::TEXT_SECONDARY_COLOR,
        1.2,
      )
      .flex(1.0),
    )
}

pub fn action_button(label: &str, primary: bool) -> Row {
  let background = if primary {
    BackgroundColor::Palette(theme::ACCENT)
  } else {
    BackgroundColor::Palette(theme::BG_ELEVATED)
  };
  let border = if primary { "#42D28B" } else { BORDER };
  let hover_bg = if primary {
    BackgroundColor::Palette(theme::ACCENT_HOVER)
  } else {
    BackgroundColor::Palette(theme::BG_INPUT)
  };
  let label = if primary {
    styled_text(label, "Inter", 13.0, FontWeight::Bold, theme::TEXT_INVERSE_COLOR, 1.2)
  } else {
    Text::new(label).variant(theme::TYP_BUTTON)
  };

  Row::new()
    .width(Dimension::Pct(100.0))
    .height(34.0)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .rounded(5.0)
    .background(background)
    .border_inside(1.0, Color::from_hex(border))
    .cursor(CursorIcon::Pointer)
    .hovered_style(Style::new().background(hover_bg))
    .child(label)
}

pub fn submit_action_button(label: &str, primary: bool) -> Button {
  let background = if primary {
    BackgroundColor::Palette(theme::ACCENT)
  } else {
    BackgroundColor::Palette(theme::BG_ELEVATED)
  };
  let border = if primary { "#42D28B" } else { BORDER };
  let hover_bg = if primary {
    BackgroundColor::Palette(theme::ACCENT_HOVER)
  } else {
    BackgroundColor::Palette(theme::BG_INPUT)
  };
  let label = if primary {
    styled_text(label, "Inter", 13.0, FontWeight::Bold, theme::TEXT_INVERSE_COLOR, 1.2)
  } else {
    Text::new(label).variant(theme::TYP_BUTTON)
  };

  Button::empty()
    .submit()
    .width(Dimension::Pct(100.0))
    .height(34.0)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .rounded(5.0)
    .background(background)
    .border_inside(1.0, Color::from_hex(border))
    .cursor(CursorIcon::Pointer)
    .hovered_style(Style::new().background(hover_bg))
    .child(label)
}

pub fn back_button(navigator: Option<lurq::router::Navigator>, label: &str) -> Row {
  let row = action_button(label, false);

  if let Some(navigator) = navigator {
    row.on_click(move |_| navigator.push(ROUTE_IDENTITY_SETUP))
  } else {
    row
  }
}
