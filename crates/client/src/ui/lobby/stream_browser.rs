use lurq::{
  app::{
    component::{Component, DevtoolsInspectable},
    ctx::Ctx,
  },
  components::{Column, Rect, Row, ScrollVertical, Stack, Text, TextOverflow},
  core::Store,
  layout::{
    Alignment, StackAlignment,
    layout_kind::Justify,
    scrollbar::{ScrollBarPlacement, ScrollBarStyle},
  },
  node::{BackgroundColor, CursorIcon, Element, Style, color::Color, dimension::Dimension},
};

use super::{
  WatchStreamAction,
  layout::lobby_layout_metrics,
  model::{ChannelScreenShare, StreamBrowserModel, stream_browser_model, stream_speaking},
  shared::error_notice,
  stream_shared::{initials_for_user, live_badge, resolution_badge, stream_avatar, stream_footer_meta, stream_name},
  subscription::{LobbyModelSubscription, apply_model},
};
use crate::{
  network::protocol::UserId,
  session::{LobbyChannel, LobbyScreenShare, LobbyUser, ServerSession},
  theme,
  ui::common::lucide_icon::{LucideIcon, LucideIconProps},
};

const LOBBY_GRID_PADDING: f32 = 20.0;
const LOBBY_GRID_GAP: f32 = 16.0;
const LOBBY_GRID_MIN_CARD_WIDTH: f32 = 300.0;
const LOBBY_GRID_MAX_CARD_WIDTH: f32 = 380.0;
const LOBBY_STREAM_CARD_HEIGHT: f32 = 208.0;
const LOBBY_STREAM_FOOTER_HEIGHT: f32 = 58.0;

pub(super) fn stream_browser(
  ctx: &mut Ctx,
  channel: LobbyChannel,
  local_user_id: UserId,
  debug_user_ids: bool,
  session: ServerSession,
  watch_stream: &WatchStreamAction,
) -> Element {
  let key = format!("stream-browser-{}", channel.id);
  ctx.mount_keyed::<StreamBrowserPane>(
    &key,
    StreamBrowserPaneProps {
      channel,
      local_user_id,
      debug_user_ids,
      session,
      watch_stream: watch_stream.clone(),
    },
  )
}

#[derive(Clone)]
struct StreamBrowserPaneProps {
  channel: LobbyChannel,
  local_user_id: UserId,
  debug_user_ids: bool,
  session: ServerSession,
  watch_stream: WatchStreamAction,
}

impl PartialEq for StreamBrowserPaneProps {
  fn eq(&self, other: &Self) -> bool {
    self.channel == other.channel
      && self.local_user_id == other.local_user_id
      && self.debug_user_ids == other.debug_user_ids
      && self.session.info().map(|info| info.address) == other.session.info().map(|info| info.address)
  }
}

impl DevtoolsInspectable for StreamBrowserPaneProps {}

struct StreamBrowserPane {
  model_store: Store<Option<StreamBrowserModel>>,
}

impl Component for StreamBrowserPane {
  type Props = StreamBrowserPaneProps;

  fn create(ctx: &mut Ctx) -> Self {
    Self {
      model_store: ctx.store(None),
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let props = ctx.props::<Self::Props>().clone();
    ctx.provide(self.model_store.clone());
    if self.model_store.with(Option::is_none) {
      apply_model(
        &self.model_store,
        stream_browser_model(&props.session.lobby(), &props.channel),
      );
    }
    let subscriber = ctx.mount::<StreamBrowserModelSubscriber>(StreamBrowserModelSubscriberProps {
      session: props.session.clone(),
      channel: props.channel.clone(),
    });
    let Some(model) = self.model_store.get() else {
      return Column::new()
        .width(Dimension::Pct(100.0))
        .flex(1.0)
        .child(subscriber)
        .into();
    };

    stream_browser_view(
      ctx,
      subscriber,
      model,
      props.local_user_id,
      props.debug_user_ids,
      &props.watch_stream,
    )
  }
}

fn stream_browser_view(
  ctx: &mut Ctx,
  subscriber: Element,
  model: StreamBrowserModel,
  local_user_id: UserId,
  debug_user_ids: bool,
  watch_stream: &WatchStreamAction,
) -> Element {
  let mut content = Column::new()
    .width(Dimension::Pct(100.0))
    .spacing(12.0)
    .padding(LOBBY_GRID_PADDING)
    .child(subscriber)
    .child(merged_lobby_grid(
      ctx,
      &model.channel,
      &model.users,
      model.streams,
      local_user_id,
      model.watching_user_id,
      debug_user_ids,
      watch_stream,
    ));

  if let Some(error) = model.error.as_deref() {
    content = content.child(error_notice(ctx, error));
  }

  ScrollVertical::new(content)
    .width(Dimension::Pct(100.0))
    .height(Dimension::Pct(100.0))
    .flex(1.0)
    .scrollbar(lobby_stream_scrollbar_style())
    .scrollbar_hovered(|mut style| {
      let palette = theme::palette();
      style.thumb_color = palette.accent_hover;
      style.track_color = palette.surface_input.with_opacity(0.75);
      style
    })
    .into()
}

#[derive(Clone)]
struct StreamBrowserModelSubscriberProps {
  session: ServerSession,
  channel: LobbyChannel,
}

impl PartialEq for StreamBrowserModelSubscriberProps {
  fn eq(&self, other: &Self) -> bool {
    self.channel == other.channel
      && self.session.info().map(|info| info.address) == other.session.info().map(|info| info.address)
  }
}

impl DevtoolsInspectable for StreamBrowserModelSubscriberProps {}

struct StreamBrowserModelSubscriber {
  subscription: LobbyModelSubscription,
}

impl Component for StreamBrowserModelSubscriber {
  type Props = StreamBrowserModelSubscriberProps;

  fn create(ctx: &mut Ctx) -> Self {
    Self {
      subscription: LobbyModelSubscription::new(ctx),
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let props = ctx.props::<Self::Props>().clone();
    let Some(model_store) = ctx.use_context::<Store<Option<StreamBrowserModel>>>() else {
      return empty_subscriber_node();
    };

    apply_model(
      &model_store,
      stream_browser_model(&props.session.lobby(), &props.channel),
    );

    let channel = props.channel.clone();
    if let Some((_snapshot_generation, model)) =
      self
        .subscription
        .next_model(ctx, props.session.clone(), move |snapshot| {
          stream_browser_model(&snapshot.lobby, &channel)
        })
    {
      apply_model(&model_store, model);
    }

    empty_subscriber_node()
  }
}

fn empty_subscriber_node() -> Element {
  Rect::new(0.0, 0.0).into()
}

fn lobby_stream_scrollbar_style() -> ScrollBarStyle {
  let palette = theme::palette();
  ScrollBarStyle {
    width: 8.0,
    min_thumb_length: 32.0,
    track_color: palette.surface_input.with_opacity(0.55),
    thumb_color: palette.accent,
    thumb_radius: 4.0,
    track_radius: 4.0,
    padding: 0.0,
    placement: ScrollBarPlacement::Reserved,
    ..ScrollBarStyle::default()
  }
}

fn merged_lobby_grid(
  ctx: &mut Ctx,
  channel: &LobbyChannel,
  users: &[LobbyUser],
  streams: Vec<ChannelScreenShare>,
  local_user_id: UserId,
  watching_user_id: Option<UserId>,
  debug_user_ids: bool,
  watch_stream: &WatchStreamAction,
) -> Element {
  let columns = lobby_grid_columns(ctx);
  let card_width = lobby_card_width(ctx, columns);
  let mut stream_by_user = streams
    .into_iter()
    .map(|stream| (stream.share.sharer_user_id, stream))
    .collect::<std::collections::HashMap<_, _>>();
  let mut cards = Vec::new();

  for user in users {
    if let Some(stream) = stream_by_user.remove(&user.user_id) {
      cards.push(merged_stream_card(
        ctx,
        channel,
        stream,
        watching_user_id,
        debug_user_ids,
        watch_stream,
        card_width,
      ));
    } else {
      cards.push(merged_user_card(
        ctx,
        user,
        user.user_id == local_user_id,
        debug_user_ids,
        card_width,
      ));
    }
  }

  for stream in stream_by_user.into_values() {
    cards.push(merged_stream_card(
      ctx,
      channel,
      stream,
      watching_user_id,
      debug_user_ids,
      watch_stream,
      card_width,
    ));
  }

  if cards.is_empty() {
    cards.push(merged_empty_card(ctx, card_width));
  }

  let mut grid = Column::new().width(Dimension::Pct(100.0)).spacing(LOBBY_GRID_GAP);
  let mut cards = cards.into_iter();

  loop {
    let mut row = Row::new().width(Dimension::Pct(100.0)).spacing(LOBBY_GRID_GAP);
    let mut has_card = false;

    for _ in 0..columns {
      if let Some(card) = cards.next() {
        has_card = true;
        row = row.child(card);
      } else if columns > 1 {
        row = row.child(Row::new().width(card_width));
      }
    }

    if !has_card {
      break;
    }

    grid = grid.child(row);
  }

  grid.into()
}

fn lobby_grid_columns(ctx: &Ctx) -> usize {
  let content_width = lobby_grid_content_width(ctx);
  let columns = ((content_width + LOBBY_GRID_GAP) / (LOBBY_GRID_MIN_CARD_WIDTH + LOBBY_GRID_GAP)).floor() as usize;

  columns.max(1)
}

fn lobby_card_width(ctx: &Ctx, columns: usize) -> f32 {
  let gaps = (columns.saturating_sub(1) as f32) * LOBBY_GRID_GAP;

  ((lobby_grid_content_width(ctx) - gaps) / columns.max(1) as f32).clamp(0.0, LOBBY_GRID_MAX_CARD_WIDTH)
}

fn lobby_grid_content_width(ctx: &Ctx) -> f32 {
  let metrics = lobby_layout_metrics(ctx);

  (ctx.window().logical_width() - metrics.rail_width - LOBBY_GRID_PADDING * 2.0).max(0.0)
}

fn merged_stream_card(
  ctx: &mut Ctx,
  _channel: &LobbyChannel,
  stream: ChannelScreenShare,
  watching_user_id: Option<UserId>,
  debug_user_ids: bool,
  watch_stream: &WatchStreamAction,
  card_width: f32,
) -> Element {
  let sharer_id = stream.share.sharer_user_id;
  let name = stream_name(ctx, &stream, debug_user_ids);
  let avatar_name = stream_name(ctx, &stream, false);
  let watching = watching_user_id == Some(sharer_id);
  let speaking = stream_speaking(&stream);
  let title = ctx.t_args("lobby.stream_browser.watching.screen_name", [("user", name.clone())]);
  let footer_meta = stream_footer_meta(ctx, &name, &stream.share);
  let action = watch_stream.clone();

  let mut card = Column::new()
    .width(card_width)
    .height(LOBBY_STREAM_CARD_HEIGHT)
    .rounded(8.0)
    .clip()
    .background(BackgroundColor::Color(if watching {
      Color::from_hex("#121A23")
    } else {
      Color::from_hex("#15171A")
    }))
    .border_inside(
      1.0,
      if speaking {
        theme::PaletteColor::Success
      } else if watching {
        theme::PaletteColor::Accent
      } else {
        theme::PaletteColor::Border
      },
    )
    .cursor(CursorIcon::Pointer)
    .hovered_style(Style::new().background(BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised)))
    .child(stream_thumbnail(ctx, &stream.share))
    .child(
      Row::new()
        .width(Dimension::Pct(100.0))
        .height(LOBBY_STREAM_FOOTER_HEIGHT)
        .align_items(Alignment::Center)
        .padding_vertical(10.0)
        .padding_horizontal(14.0)
        .spacing(10.0)
        .child(stream_avatar(&avatar_name, 28.0, speaking))
        .child(
          Column::new()
            .width(Dimension::Pct(100.0))
            .flex(1.0)
            .spacing(2.0)
            .child(
              Text::new(&title)
                .variant(theme::TypographyStyle::Button)
                .color(theme::PaletteColor::TextPrimary),
            )
            .child(
              Text::new(&footer_meta)
                .variant(theme::TypographyStyle::Caption)
                .color(theme::PaletteColor::TextMuted),
            ),
        ),
    );

  if !watching && !watch_stream.state().get().is_pending() {
    card = card.on_click(move |_| action.run(sharer_id));
  }

  card.into()
}

fn stream_thumbnail(ctx: &mut Ctx, stream: &LobbyScreenShare) -> Element {
  Stack::new()
    .stack_align(StackAlignment::Center)
    .width(Dimension::Pct(100.0))
    .flex(1.0)
    .background(BackgroundColor::Color(Color::from_hex("#0F1013")))
    .child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon: "monitor",
      size: 40.0,
      color: Color::from_hex("#2E333B"),
    }))
    .child(
      Column::new()
        .width(Dimension::Pct(100.0))
        .height(Dimension::Pct(100.0))
        .justify(Justify::SpaceBetween)
        .padding(12.0)
        .child(
          Row::new()
            .width(Dimension::Pct(100.0))
            .height(22.0)
            .align_items(Alignment::Center)
            .child(live_badge(ctx)),
        )
        .child(
          Row::new()
            .width(Dimension::Pct(100.0))
            .height(20.0)
            .align_items(Alignment::Center)
            .justify(Justify::End)
            .child(resolution_badge(ctx, stream)),
        ),
    )
    .into()
}

fn merged_user_card(ctx: &mut Ctx, user: &LobbyUser, _local: bool, debug_user_ids: bool, card_width: f32) -> Element {
  let active = user.speaking && !user.muted && !user.deafened;
  let name_max_width = (card_width - 74.0).max(60.0);
  let username = super::shared::user_display_name(user.user_id, &user.username, debug_user_ids);

  Column::new()
    .width(card_width)
    .height(LOBBY_STREAM_CARD_HEIGHT)
    .padding(12.0)
    .rounded(8.0)
    .background(BackgroundColor::Color(Color::from_hex("#15171A")))
    .border_inside(
      1.0,
      if active {
        theme::PaletteColor::Success
      } else {
        theme::PaletteColor::Border
      },
    )
    .child(Row::new().width(Dimension::Pct(100.0)).height(22.0))
    .child(
      Row::new()
        .width(Dimension::Pct(100.0))
        .flex(1.0)
        .align_items(Alignment::Center)
        .justify(Justify::Center)
        .child(stream_user_avatar(&user.username, active, 56.0)),
    )
    .child(
      Column::new()
        .width(Dimension::Pct(100.0))
        .height(42.0)
        .align_items(Alignment::Center)
        .justify(Justify::Center)
        .child(
          Row::new()
            .width(Dimension::Pct(100.0))
            .height(22.0)
            .align_items(Alignment::Center)
            .justify(Justify::Center)
            .spacing(6.0)
            .child(
              Text::new(&username)
                .max_width(name_max_width)
                .variant(theme::TypographyStyle::Button)
                .color(theme::PaletteColor::TextPrimary)
                .nowrap()
                .text_overflow(TextOverflow::Elipsis),
            )
            .child(merged_voice_icons(ctx, user)),
        ),
    )
    .into()
}

fn merged_empty_card(ctx: &mut Ctx, card_width: f32) -> Element {
  Column::new()
    .width(card_width)
    .height(LOBBY_STREAM_CARD_HEIGHT)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .spacing(10.0)
    .rounded(8.0)
    .background(BackgroundColor::Color(Color::from_hex("#15171A")))
    .border_inside(1.0, theme::PaletteColor::Border)
    .child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon: "users",
      size: 28.0,
      color: theme::palette().text_muted,
    }))
    .child(
      Text::new(&ctx.t("lobby.users.empty"))
        .variant(theme::TypographyStyle::Caption)
        .color(theme::PaletteColor::TextMuted),
    )
    .into()
}

fn merged_voice_icons(ctx: &mut Ctx, user: &LobbyUser) -> Element {
  let mut icons = Row::new()
    .height(18.0)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .spacing(6.0);

  if user.deafened {
    icons = icons
      .child(ctx.mount::<LucideIcon>(LucideIconProps {
        icon: "headphone-off",
        size: 14.0,
        color: theme::palette().danger,
      }))
      .child(ctx.mount::<LucideIcon>(LucideIconProps {
        icon: "mic-off",
        size: 14.0,
        color: theme::palette().danger,
      }));
  } else if user.muted {
    icons = icons.child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon: "mic-off",
      size: 14.0,
      color: theme::palette().danger,
    }));
  } else if user.speaking {
    icons = icons.child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon: "mic",
      size: 14.0,
      color: theme::palette().success,
    }));
  }

  icons.into()
}

fn stream_user_avatar(name: &str, active: bool, size: f32) -> Element {
  Row::new()
    .width(size)
    .height(size)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .rounded(size / 2.0)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised))
    .border_inside(
      1.5,
      BackgroundColor::Palette(if active {
        theme::PaletteColor::Success
      } else {
        theme::PaletteColor::Border
      }),
    )
    .child(
      Text::new(&initials_for_user(name))
        .variant(theme::TypographyStyle::Heading)
        .color(if active {
          theme::PaletteColor::TextPrimary
        } else {
          theme::PaletteColor::TextSecondary
        }),
    )
    .into()
}
