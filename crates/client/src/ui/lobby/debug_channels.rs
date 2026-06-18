use lurq::{
  app::{
    component::{Component, ComponentInfo, DevtoolsFormatter, DevtoolsInspectable},
    ctx::Ctx,
  },
  components::{Column, Row, Text},
  core::Signal,
  layout::{Alignment, layout_kind::Justify},
  node::{BackgroundColor, CursorIcon, Element, Style, color::Color, dimension::Dimension},
};

use crate::{
  session::ServerSession,
  theme,
  ui::lobby::channel_section::{aligned_channel_icon_with_color, section_head},
};

#[derive(Clone)]
pub(super) struct SelectDebugChatAction {
  session: ServerSession,
}

impl SelectDebugChatAction {
  pub(super) fn new(session: ServerSession) -> Self {
    Self { session }
  }

  fn run(&self) {
    self.session.select_debug_chat();
  }
}

#[derive(Clone, PartialEq)]
pub(super) struct DebugChannelsProps {
  pub selected: bool,
  pub select_debug_chat: Option<SelectDebugChatAction>,
}

impl PartialEq for SelectDebugChatAction {
  fn eq(&self, other: &Self) -> bool {
    self.session.info().map(|info| info.address) == other.session.info().map(|info| info.address)
  }
}

impl DevtoolsInspectable for DebugChannelsProps {
  fn inspect(&self, formatter: &mut DevtoolsFormatter<'_>) {
    formatter.buffer_mut().push(ComponentInfo::with_value(
      "selected",
      std::any::type_name::<bool>(),
      self.selected.to_string(),
    ));
  }
}

pub(super) struct DebugChannels {
  expanded: Signal<bool>,
}

impl Component for DebugChannels {
  type Props = DebugChannelsProps;

  fn create(ctx: &mut Ctx) -> Self {
    Self {
      expanded: ctx.signal(true),
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let props = ctx.props::<Self::Props>().clone();
    let is_expanded = self.expanded.get();
    let mut section = Column::new()
      .width(Dimension::Pct(100.0))
      .spacing(theme::SpacingSize::Xs)
      .child(section_head(
        ctx,
        self.expanded.clone(),
        &ctx.t("lobby.debug_channels.title"),
        None,
        None,
        false,
      ));

    if is_expanded {
      section = section.child(debug_chat_row(ctx, props.selected, props.select_debug_chat));
    }

    section
  }
}

fn debug_chat_row(ctx: &mut Ctx, selected: bool, select_debug_chat: Option<SelectDebugChatAction>) -> Element {
  let channel_color = if selected {
    theme::palette().accent
  } else {
    theme::palette().text_muted
  };
  let mut row = Row::new()
    .width(Dimension::Pct(100.0))
    .align_items(Alignment::Center)
    .justify(Justify::Start)
    .spacing(theme::SpacingSize::Sm)
    .padding_vertical(10.0)
    .padding_horizontal(12.0)
    .rounded(theme::RadiusSize::Md)
    .background(if selected {
      BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised)
    } else {
      BackgroundColor::Color(Color::from_hex("#00000000"))
    })
    .cursor(CursorIcon::Pointer)
    .hovered_style(Style::new().background(BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised)))
    .child(aligned_channel_icon_with_color(ctx, "terminal", 16.0, channel_color))
    .child(
      Text::new(&ctx.t("lobby.debug_channels.chat"))
        .width(Dimension::Pct(100.0))
        .flex(1.0)
        .variant(theme::TypographyStyle::Description)
        .color(if selected {
          theme::PaletteColor::TextPrimary
        } else {
          theme::PaletteColor::TextSecondary
        }),
    );

  if let Some(select_debug_chat) = select_debug_chat {
    row = row.on_click(move |_| select_debug_chat.run());
  }

  row.into()
}
