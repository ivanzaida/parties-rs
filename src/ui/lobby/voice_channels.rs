use std::collections::HashMap;

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
  network::protocol::{ChannelId, UserId},
  session::{LobbyChannel, LobbyUser},
  theme,
  ui::lobby::channel_section::{aligned_channel_icon, aligned_channel_icon_with_color, section_head},
};

pub(super) type JoinChannelAction = lurq::app::ctx::FutureAction<ChannelId, (), String>;

#[derive(Clone)]
pub(super) struct VoiceChannelsProps {
  pub channels: Vec<LobbyChannel>,
  pub users_by_channel: HashMap<ChannelId, Vec<LobbyUser>>,
  pub streaming_user_ids: Vec<UserId>,
  pub selected_channel_id: Option<ChannelId>,
  pub local_user_id: UserId,
  pub join_channel: Option<JoinChannelAction>,
}

impl PartialEq for VoiceChannelsProps {
  fn eq(&self, other: &Self) -> bool {
    self.channels == other.channels
      && self.users_by_channel == other.users_by_channel
      && self.streaming_user_ids == other.streaming_user_ids
      && self.selected_channel_id == other.selected_channel_id
      && self.local_user_id == other.local_user_id
      && self.join_channel.is_some() == other.join_channel.is_some()
  }
}

impl DevtoolsInspectable for VoiceChannelsProps {
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
    buffer.push(ComponentInfo::with_value(
      "users_by_channel",
      std::any::type_name::<usize>(),
      self.users_by_channel.values().map(Vec::len).sum::<usize>().to_string(),
    ));
    buffer.push(ComponentInfo::with_value(
      "streaming_user_ids",
      std::any::type_name::<usize>(),
      self.streaming_user_ids.len().to_string(),
    ));
  }
}

pub(super) struct VoiceChannels {
  expanded: Signal<bool>,
}

impl Component for VoiceChannels {
  type Props = VoiceChannelsProps;

  fn create(ctx: &mut Ctx) -> Self {
    Self {
      expanded: ctx.signal(true),
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let props = ctx.props::<Self::Props>().clone();
    let is_expanded = self.expanded.get();
    let mut body = Column::new()
      .width(Dimension::Pct(100.0))
      .spacing(theme::SpacingSize::Xs)
      .child(section_head(
        ctx,
        self.expanded.clone(),
        &ctx.t("lobby.voice_channels.title"),
        None,
      ));

    if !is_expanded {
      return body;
    }

    if props.channels.is_empty() {
      body = body.child(
        Row::new()
          .width(Dimension::Pct(100.0))
          .padding_vertical(6.0)
          .padding_horizontal(8.0)
          .child(
            Text::new(&ctx.t("lobby.voice_channels.empty"))
              .variant(theme::TypographyStyle::Link)
              .color(theme::PaletteColor::TextMuted),
          ),
      );
    } else {
      let users_by_channel = props.users_by_channel.clone();
      let selected_channel_id = props.selected_channel_id;
      let join_channel = props.join_channel.clone();
      let local_user_id = props.local_user_id;
      let streaming_user_ids = props.streaming_user_ids.clone();
      let channel_groups = ctx.for_each(
        props.channels,
        |channel| channel.id,
        move |ctx, channel| {
          let users = users_by_channel.get(&channel.id).cloned().unwrap_or_default();
          channel_group(
            ctx,
            &channel,
            selected_channel_id,
            join_channel.as_ref(),
            users,
            local_user_id,
            streaming_user_ids.clone(),
          )
        },
      );
      body = body.with_children(channel_groups);
    }

    body
  }
}

fn channel_group(
  ctx: &mut Ctx,
  channel: &LobbyChannel,
  selected_channel_id: Option<ChannelId>,
  join_channel: Option<&JoinChannelAction>,
  users: Vec<LobbyUser>,
  local_user_id: UserId,
  streaming_user_ids: Vec<UserId>,
) -> Element {
  let user_rows = ctx.for_each(
    users,
    |user| user.user_id,
    move |ctx, user| {
      let streaming = streaming_user_ids.contains(&user.user_id);
      channel_user_row(ctx, &user, local_user_id, streaming)
    },
  );

  Column::new()
    .width(Dimension::Pct(100.0))
    .spacing(2.0)
    .child(channel_row(ctx, channel, selected_channel_id, join_channel))
    .with_children(user_rows)
    .into()
}

fn channel_row(
  ctx: &mut Ctx,
  channel: &LobbyChannel,
  selected_channel_id: Option<ChannelId>,
  join_channel: Option<&JoinChannelAction>,
) -> Element {
  let selected = selected_channel_id == Some(channel.id);
  let channel_id = channel.id;
  let count = channel.user_count.to_string();
  let channel_color = if selected {
    theme::palette().accent
  } else {
    theme::palette().text_muted
  };
  let mut row = Row::new()
    .width(Dimension::Pct(100.0))
    .align_items(Alignment::Center)
    .justify(Justify::SpaceBetween)
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
    .child(
      Row::new()
        .width(Dimension::Pct(100.0))
        .flex(1.0)
        .align_items(Alignment::Center)
        .spacing(theme::SpacingSize::Sm)
        .child(aligned_channel_icon_with_color(ctx, "volume-2", 16.0, channel_color))
        .child(
          Text::new(&channel.name)
            .variant(theme::TypographyStyle::Description)
            .color(if selected {
              theme::PaletteColor::TextPrimary
            } else {
              theme::PaletteColor::TextSecondary
            })
            .width(Dimension::Pct(100.0)),
        ),
    )
    .child(
      Row::new()
        .align_items(Alignment::Center)
        .spacing(theme::SpacingSize::Xs)
        .child(aligned_channel_icon(ctx, "user", 12.0))
        .child(
          Text::new(&count)
            .variant(theme::TypographyStyle::Mono)
            .color(theme::PaletteColor::TextMuted),
        ),
    );

  if let Some(join_channel) = join_channel {
    let action = join_channel.clone();
    row = row.on_click(move |_| action.run(channel_id));
  }

  row.into()
}

fn channel_user_row(ctx: &mut Ctx, user: &LobbyUser, _local_user_id: UserId, streaming: bool) -> Element {
  let speaking = user.speaking && !user.muted && !user.deafened;

  Row::new()
    .width(Dimension::Pct(100.0))
    .align_items(Alignment::Center)
    .spacing(theme::SpacingSize::Sm)
    .padding_vertical(5.0)
    .padding_horizontal(16.0)
    .child(user_avatar(&user.username, speaking))
    .child(
      Text::new(&user.username)
        .flex(1.0)
        .variant(if speaking {
          theme::TypographyStyle::Button
        } else {
          theme::TypographyStyle::Description
        })
        .color(if speaking {
          theme::PaletteColor::TextPrimary
        } else {
          theme::PaletteColor::TextSecondary
        }),
    )
    .child(user_voice_icons(ctx, user, streaming))
    .into()
}

fn user_avatar(name: &str, active: bool) -> Element {
  let mut avatar = Row::new()
    .width(22.0)
    .height(22.0)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .rounded(11.0)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised))
    .child(
      Text::new(&initials_for(name))
        .variant(theme::TypographyStyle::FieldLabel)
        .color(if active {
          theme::PaletteColor::TextPrimary
        } else {
          theme::PaletteColor::TextSecondary
        }),
    );

  if active {
    avatar = avatar.border_inside(1.5, BackgroundColor::Palette(theme::PaletteColor::Success));
  }

  avatar.into()
}

fn user_voice_icons(ctx: &mut Ctx, user: &LobbyUser, streaming: bool) -> Element {
  let mut icons = Row::new()
    .align_items(Alignment::Center)
    .justify(Justify::End)
    .spacing(6.0);

  if streaming {
    icons = icons.child(aligned_channel_icon_with_color(
      ctx,
      "monitor-up",
      14.0,
      theme::palette().accent,
    ));
  }

  if user.deafened {
    icons = icons
      .child(aligned_channel_icon_with_color(
        ctx,
        "headphone-off",
        14.0,
        theme::palette().danger,
      ))
      .child(aligned_channel_icon_with_color(
        ctx,
        "mic-off",
        14.0,
        theme::palette().danger,
      ));
  } else if user.muted {
    icons = icons.child(aligned_channel_icon_with_color(
      ctx,
      "mic-off",
      14.0,
      theme::palette().danger,
    ));
  }

  icons.into()
}

fn initials_for(name: &str) -> String {
  let initials = name
    .chars()
    .filter(|ch| ch.is_alphanumeric())
    .flat_map(|ch| ch.to_uppercase())
    .take(1)
    .collect::<String>();

  if initials.is_empty() { "?".to_owned() } else { initials }
}
