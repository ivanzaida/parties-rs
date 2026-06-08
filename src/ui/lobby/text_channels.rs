use std::sync::Arc;

use lurq::{
  app::{
    component::{Component, ComponentInfo, DevtoolsInspectable},
    ctx::Ctx,
  },
  components::{Column, Row, Text},
  core::Signal,
  layout::{Alignment, layout_kind::Justify},
  node::{BackgroundColor, CursorIcon, Element, Style, color::Color, dimension::Dimension},
};

use crate::{
  network::protocol::ChannelId,
  session::{LobbyTextChannel, ServerSession},
  theme,
  ui::lobby::{ChannelManagerKind, channel_section::{aligned_channel_icon_with_color, section_head}},
};

#[derive(Clone)]
pub(super) struct TextChannelsProps {
  pub channels: Vec<LobbyTextChannel>,
  pub selected_channel_id: Option<ChannelId>,
  pub channel_manager: Signal<Option<ChannelManagerKind>>,
}

impl PartialEq for TextChannelsProps {
  fn eq(&self, other: &Self) -> bool {
    self.channels == other.channels && self.selected_channel_id == other.selected_channel_id
  }
}

impl DevtoolsInspectable for TextChannelsProps {
  fn write_info(&self, buffer: &mut Vec<ComponentInfo>) {
    buffer.push(ComponentInfo::with_value(
      "channels",
      std::any::type_name::<usize>(),
      self.channels.len().to_string(),
    ));
    buffer.push(ComponentInfo::with_value(
      "selected_channel_id",
      std::any::type_name::<Option<ChannelId>>(),
      format!("{:?}", self.selected_channel_id),
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
    let manager = props.channel_manager.clone();
    let open_manager: Arc<dyn Fn() + Send + Sync + 'static> = Arc::new(move || {
      manager.set(Some(ChannelManagerKind::Text));
    });
    let mut section = Column::new()
      .width(Dimension::Pct(100.0))
      .spacing(theme::SpacingSize::Xs)
      .child(section_head(
        ctx,
        self.expanded.clone(),
        &ctx.t("lobby.text_channels.title"),
        Some("settings"),
        Some(open_manager),
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
        let selected_channel_id = props.selected_channel_id;
        let session = ctx.use_context::<ServerSession>();
        section = section.with_children(ctx.for_each(
          props.channels,
          |channel| channel.id,
          move |ctx, channel| text_channel_row(ctx, &channel, selected_channel_id, session.clone()),
        ));
      }
    }

    section
  }
}

fn text_channel_row(
  ctx: &mut Ctx,
  channel: &LobbyTextChannel,
  selected_channel_id: Option<ChannelId>,
  session: Option<ServerSession>,
) -> Element {
  let selected = selected_channel_id == Some(channel.id);
  let channel_id = channel.id;
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
      Text::new(&channel.name)
        .width(Dimension::Pct(100.0))
        .flex(1.0)
        .variant(theme::TypographyStyle::Description)
        .color(if selected {
          theme::PaletteColor::TextPrimary
        } else {
          theme::PaletteColor::TextSecondary
        }),
    );

  if let Some(session) = session {
    row = row.on_click(move |_| session.select_text_channel(channel_id));
  }

  row.into()
}
