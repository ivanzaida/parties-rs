use lurq::{
  app::{
    component::{Component, ComponentInfo, DevtoolsFormatter, DevtoolsInspectable},
    ctx::Ctx,
  },
  components::{Column, Rect, Row, Text},
  core::Signal,
  layout::{Alignment, layout_kind::Justify},
  node::{BackgroundColor, CursorIcon, Element, Style, color::Color, dimension::Dimension},
};

use crate::{
  network::protocol::ChannelId,
  session::ServerSession,
  theme,
  ui::lobby::{
    channel_section::{aligned_channel_icon_with_color, section_head},
    model::TextChannelRowModel,
    session_identity::same_session,
  },
};

#[derive(Clone)]
pub(super) struct SelectTextChannelAction {
  session: ServerSession,
}

impl SelectTextChannelAction {
  pub(super) fn new(session: ServerSession) -> Self {
    Self { session }
  }

  fn run(&self, channel_id: ChannelId) {
    self.session.select_text_channel(channel_id);
  }
}

impl PartialEq for SelectTextChannelAction {
  fn eq(&self, other: &Self) -> bool {
    same_session(&self.session, &other.session)
  }
}

#[derive(Clone, PartialEq)]
pub(super) struct TextChannelsProps {
  pub channels: Vec<TextChannelRowModel>,
  pub select_channel: Option<SelectTextChannelAction>,
}

impl DevtoolsInspectable for TextChannelsProps {
  fn inspect(&self, formatter: &mut DevtoolsFormatter<'_>) {
    formatter.buffer_mut().push(ComponentInfo::with_value(
      "channels",
      std::any::type_name::<usize>(),
      self.channels.len().to_string(),
    ));
    formatter.buffer_mut().push(ComponentInfo::with_value(
      "selected_channels",
      std::any::type_name::<usize>(),
      self
        .channels
        .iter()
        .filter(|channel| channel.selected)
        .count()
        .to_string(),
    ));
  }
}

pub(super) struct TextChannels {
  expanded: Signal<bool>,
}

impl Component for TextChannels {
  type Props = TextChannelsProps;

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
        &ctx.t("lobby.text_channels.title"),
        None,
        None,
        false,
      ));

    if is_expanded {
      if props.channels.is_empty() {
        section = section.child(
          Row::new()
            .width(Dimension::Pct(100.0))
            .padding_vertical(6.0)
            .padding_horizontal(8.0)
            .child(
              Text::new(&ctx.t("lobby.text_channels.empty"))
                .variant(theme::TypographyStyle::Link)
                .color(theme::PaletteColor::TextMuted),
            ),
        );
      } else {
        section = section.with_children(ctx.for_each(
          props.channels,
          |row| row.channel.id,
          move |ctx, row| {
            ctx.mount::<TextChannelRow>(TextChannelRowProps {
              model: row,
              select_channel: props.select_channel.clone(),
            })
          },
        ));
      }
    }

    section
  }
}

#[derive(Clone, PartialEq)]
struct TextChannelRowProps {
  model: TextChannelRowModel,
  select_channel: Option<SelectTextChannelAction>,
}

impl DevtoolsInspectable for TextChannelRowProps {}

struct TextChannelRow;

impl Component for TextChannelRow {
  type Props = TextChannelRowProps;

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let props = ctx.props::<Self::Props>().clone();
    text_channel_row(ctx, &props.model, props.select_channel)
  }
}

fn text_channel_row(
  ctx: &mut Ctx,
  model: &TextChannelRowModel,
  select_channel: Option<SelectTextChannelAction>,
) -> Element {
  let selected = model.selected;
  let channel_id = model.channel.id;
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
    .child(aligned_channel_icon_with_color(ctx, "hash", 16.0, channel_color))
    .child(
      Text::new(&model.channel.name)
        .width(Dimension::Pct(100.0))
        .flex(1.0)
        .variant(theme::TypographyStyle::Description)
        .color(if selected {
          theme::PaletteColor::TextPrimary
        } else {
          theme::PaletteColor::TextSecondary
        }),
    );
  if model.unread {
    row = row.child(
      Rect::new(7.0, 7.0)
        .rounded(4.0)
        .background(BackgroundColor::Palette(theme::PaletteColor::Accent)),
    );
  }

  if let Some(select_channel) = select_channel {
    row = row.on_click(move |_| select_channel.run(channel_id));
  }

  row.into()
}
