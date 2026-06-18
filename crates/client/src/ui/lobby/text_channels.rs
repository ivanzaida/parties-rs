use std::{collections::HashSet, sync::Mutex};

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

#[derive(Clone)]
pub(super) struct TextChannelsProps {
  pub channels: Vec<TextChannelRowModel>,
  pub select_channel: Option<SelectTextChannelAction>,
}

impl PartialEq for TextChannelsProps {
  fn eq(&self, other: &Self) -> bool {
    self.channels == other.channels && self.select_channel.is_some() == other.select_channel.is_some()
  }
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
  mounted_channel_ids: Mutex<HashSet<ChannelId>>,
}

impl Component for TextChannels {
  type Props = TextChannelsProps;

  fn create(ctx: &mut Ctx) -> Self {
    Self {
      expanded: ctx.signal(true),
      mounted_channel_ids: Mutex::new(HashSet::new()),
    }
  }

  fn on_mounted(&self) {
    tracing::info!(target: "lobby::text_channels", "[lobby:text_channels] component mounted");
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let props = ctx.props::<Self::Props>().clone();
    let is_expanded = self.expanded.get();
    tracing::debug!(
      target: "lobby::text_channels",
      "[lobby:text_channels] render channels={} selected={} unread={}",
      props.channels.len(),
      props.channels.iter().filter(|channel| channel.selected).count(),
      props.channels.iter().filter(|channel| channel.unread).count()
    );
    if let Ok(mut mounted_channel_ids) = self.mounted_channel_ids.lock() {
      for row in &props.channels {
        if mounted_channel_ids.insert(row.channel.id) {
          tracing::info!(
            target: "lobby::text_channels",
            "[lobby:text_channels] channel mounted id={} name='{}' selected={} total_channels={}",
            row.channel.id,
            row.channel.name,
            row.selected,
            props.channels.len()
          );
        }
      }
      mounted_channel_ids.retain(|id| props.channels.iter().any(|row| row.channel.id == *id));
    }
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
          move |ctx, row| text_channel_row(ctx, &row, props.select_channel.clone()),
        ));
      }
    }

    section
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
