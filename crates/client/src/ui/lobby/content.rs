use lurq::{
  app::{
    component::{Component, DevtoolsInspectable},
    ctx::Ctx,
  },
  components::{Column, Rect, Row, Text},
  core::{Signal, Store},
  layout::{
    Alignment,
    layout_kind::{Justify, ScrollState},
  },
  node::{BackgroundColor, CursorIcon, Element, Style, border::Border, dimension::Dimension},
};

use super::{
  StopWatchingAction, WatchStreamAction,
  chat::{ChatActions, ChatChannel, ChatCommandInvalidFeedback, text_channel_detail},
  layout::lobby_layout_metrics,
  model::{MainBodyModel, MainTopBarModel, main_body_model, main_top_bar_model},
  session_identity::same_session,
  shared::error_notice,
  stream_watching::{stream_channel_detail, stream_watching_top_bar},
  subscription::{LobbyModelSubscription, apply_current_model, apply_model},
};
use crate::{
  network::protocol::ChannelId,
  session::{ConnectedServerInfo, LobbyChannel, ServerSession},
  storage::Storage,
  theme,
  ui::common::lucide_icon::{LucideIcon, LucideIconProps},
};

pub(super) fn main(
  ctx: &mut Ctx,
  info: &ConnectedServerInfo,
  message_input: Signal<String>,
  chat_command_selected_index: Signal<usize>,
  chat_command_scroll_state: ScrollState,
  chat_command_invalid_feedback: ChatCommandInvalidFeedback,
  chat_scroll_state: ScrollState,
  chat_bottom_anchor: Signal<Option<(ChannelId, u64)>>,
  chat_bottom_settle_anchor: Signal<Option<(ChannelId, u64, f32)>>,
  chat_bottom_detached_anchor: Signal<Option<(ChannelId, u64)>>,
  chat_top_anchor: Signal<Option<(ChannelId, u64)>>,
  chat_prepend_settle_anchor: Signal<Option<(ChannelId, u64, f32)>>,
  debug_mode_enabled: bool,
  storage: Option<Storage>,
  session: ServerSession,
  chat_actions: ChatActions,
  start_stream_modal_open: Signal<bool>,
  watch_stream: &WatchStreamAction,
  stop_watching: &StopWatchingAction,
) -> Element {
  Column::new()
    .width(Dimension::Pct(100.0))
    .height(Dimension::Pct(100.0))
    .flex(1.0)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceBase))
    .child(main_top_bar(
      ctx,
      debug_mode_enabled,
      session.clone(),
      start_stream_modal_open.clone(),
      stop_watching,
    ))
    .child(main_body(
      ctx,
      info,
      message_input,
      chat_command_selected_index,
      chat_command_scroll_state,
      chat_command_invalid_feedback,
      chat_scroll_state,
      chat_bottom_anchor,
      chat_bottom_settle_anchor,
      chat_bottom_detached_anchor,
      chat_top_anchor,
      chat_prepend_settle_anchor,
      debug_mode_enabled,
      storage,
      session,
      chat_actions,
      watch_stream,
    ))
    .into()
}

fn main_top_bar(
  ctx: &mut Ctx,
  debug_mode_enabled: bool,
  session: ServerSession,
  start_stream_modal_open: Signal<bool>,
  stop_watching: &StopWatchingAction,
) -> Element {
  ctx.mount::<MainTopBar>(MainTopBarProps {
    debug_mode_enabled,
    session,
    start_stream_modal_open,
    stop_watching: stop_watching.clone(),
  })
}

#[derive(Clone)]
struct MainTopBarProps {
  debug_mode_enabled: bool,
  session: ServerSession,
  start_stream_modal_open: Signal<bool>,
  stop_watching: StopWatchingAction,
}

impl PartialEq for MainTopBarProps {
  fn eq(&self, other: &Self) -> bool {
    self.debug_mode_enabled == other.debug_mode_enabled && same_session(&self.session, &other.session)
  }
}

impl DevtoolsInspectable for MainTopBarProps {}

struct MainTopBar {
  model_store: Store<Option<MainTopBarModel>>,
}

impl Component for MainTopBar {
  type Props = MainTopBarProps;

  fn create(ctx: &mut Ctx) -> Self {
    Self {
      model_store: ctx.store(None),
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let props = ctx.props::<Self::Props>().clone();
    ctx.provide(self.model_store.clone());
    apply_current_model(&self.model_store, &props.session, |lobby| {
      main_top_bar_model(lobby, props.debug_mode_enabled)
    });
    let subscriber = ctx.mount::<MainTopBarModelSubscriber>(MainTopBarModelSubscriberProps {
      session: props.session.clone(),
      debug_mode_enabled: props.debug_mode_enabled,
    });
    let model = self.model_store.get().unwrap_or(MainTopBarModel::VoiceDefault);
    main_top_bar_view(
      ctx,
      subscriber,
      model,
      props.debug_mode_enabled,
      props.start_stream_modal_open,
      &props.stop_watching,
    )
  }
}

#[derive(Clone)]
struct MainTopBarModelSubscriberProps {
  session: ServerSession,
  debug_mode_enabled: bool,
}

impl PartialEq for MainTopBarModelSubscriberProps {
  fn eq(&self, other: &Self) -> bool {
    self.debug_mode_enabled == other.debug_mode_enabled && same_session(&self.session, &other.session)
  }
}

impl DevtoolsInspectable for MainTopBarModelSubscriberProps {}

struct MainTopBarModelSubscriber {
  subscription: LobbyModelSubscription,
}

impl Component for MainTopBarModelSubscriber {
  type Props = MainTopBarModelSubscriberProps;

  fn create(ctx: &mut Ctx) -> Self {
    Self {
      subscription: LobbyModelSubscription::new(ctx),
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let props = ctx.props::<Self::Props>().clone();
    let Some(model_store) = ctx.use_context::<Store<Option<MainTopBarModel>>>() else {
      return empty_subscriber_node();
    };

    apply_current_model(&model_store, &props.session, |lobby| {
      main_top_bar_model(lobby, props.debug_mode_enabled)
    });

    let debug_mode_enabled = props.debug_mode_enabled;
    if let Some((_snapshot_generation, model)) =
      self
        .subscription
        .next_model(ctx, props.session.clone(), move |snapshot| {
          main_top_bar_model(&snapshot.lobby, debug_mode_enabled)
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

fn main_top_bar_view(
  ctx: &mut Ctx,
  subscriber: Element,
  model: MainTopBarModel,
  debug_mode_enabled: bool,
  start_stream_modal_open: Signal<bool>,
  stop_watching: &StopWatchingAction,
) -> Element {
  let metrics = lobby_layout_metrics(ctx);
  match model {
    MainTopBarModel::DebugChat => {
      let channel = ChatChannel::debug(ctx);
      chat_channel_top_bar(ctx, subscriber, &channel, None)
    }
    MainTopBarModel::Text {
      channel,
      command_registry,
      member_count,
    } => {
      let channel = ChatChannel::server_text(ctx, &channel, command_registry);
      chat_channel_top_bar(ctx, subscriber, &channel, Some(member_count))
    }
    MainTopBarModel::StreamWatching { stream } => stream_watching_top_bar(
      ctx,
      subscriber,
      stream,
      debug_mode_enabled,
      start_stream_modal_open,
      stop_watching,
    ),
    MainTopBarModel::StreamBrowser { channel, user_count } => {
      voice_stream_top_bar(ctx, subscriber, &channel, user_count)
    }
    MainTopBarModel::VoiceDefault => Row::new()
      .width(Dimension::Pct(100.0))
      .height(56.0)
      .align_items(Alignment::Center)
      .spacing(theme::SpacingSize::Md)
      .padding_horizontal(metrics.top_bar_padding_x)
      .border_bottom(Border::inside(1.0, theme::PaletteColor::Border))
      .child(subscriber)
      .child(ctx.mount::<LucideIcon>(LucideIconProps {
        icon: "volume-2",
        size: 16.0,
        color: theme::palette().text_secondary,
      }))
      .child(Text::new(&ctx.t("lobby.title")).variant(theme::TypographyStyle::Heading))
      .into(),
  }
}

fn chat_channel_top_bar(
  ctx: &mut Ctx,
  subscriber: Element,
  channel: &ChatChannel,
  member_count: Option<usize>,
) -> Element {
  let metrics = lobby_layout_metrics(ctx);
  let mut row = Row::new()
    .width(Dimension::Pct(100.0))
    .height(56.0)
    .align_items(Alignment::Center)
    .spacing(12.0)
    .padding_horizontal(metrics.top_bar_padding_x)
    .border_bottom(Border::inside(1.0, theme::PaletteColor::Border))
    .child(subscriber)
    .child(top_bar_plain_icon(ctx, channel.icon(), 18.0))
    .child(top_bar_label(
      channel.name(),
      theme::TypographyStyle::Heading,
      theme::PaletteColor::TextPrimary,
    ))
    .child(
      Row::new()
        .height(Dimension::Pct(100.0))
        .align_items(Alignment::Center)
        .child(
          Row::new()
            .width(1.0)
            .height(20.0)
            .background(BackgroundColor::Palette(theme::PaletteColor::Border)),
        ),
    )
    .child(top_bar_label(
      channel.topic(),
      theme::TypographyStyle::Caption,
      theme::PaletteColor::TextMuted,
    ))
    .child(Row::new().flex(1.0));

  if channel.shows_text_tools() {
    row = row.child(top_bar_icon(ctx, "search")).child(top_bar_icon(ctx, "pin"));
    if let Some(member_count) = member_count {
      row = row.child(
        Row::new()
          .height(Dimension::Pct(100.0))
          .align_items(Alignment::Center)
          .spacing(6.0)
          .child(top_bar_icon(ctx, "users"))
          .child(top_bar_label(
            &member_count.to_string(),
            theme::TypographyStyle::Mono,
            theme::PaletteColor::TextMuted,
          )),
      );
    }
  }

  row.into()
}

fn voice_stream_top_bar(ctx: &mut Ctx, subscriber: Element, channel: &LobbyChannel, user_count: usize) -> Element {
  let metrics = lobby_layout_metrics(ctx);
  Row::new()
    .width(Dimension::Pct(100.0))
    .height(56.0)
    .align_items(Alignment::Center)
    .spacing(10.0)
    .padding_horizontal(metrics.top_bar_padding_x)
    .border_bottom(Border::inside(1.0, theme::PaletteColor::Border))
    .child(subscriber)
    .child(top_bar_plain_icon(ctx, "volume-2", 16.0))
    .child(top_bar_label(
      &channel.name,
      theme::TypographyStyle::Heading,
      theme::PaletteColor::TextPrimary,
    ))
    .child(user_count_chip(ctx, user_count))
    .child(Row::new().flex(1.0))
    .into()
}

fn user_count_chip(ctx: &mut Ctx, user_count: usize) -> Element {
  Row::new()
    .height(Dimension::Pct(100.0))
    .align_items(Alignment::Center)
    .child(
      Row::new()
        .height(22.0)
        .align_items(Alignment::Center)
        .justify(Justify::Center)
        .spacing(5.0)
        .padding_horizontal(4.0)
        .child(ctx.mount::<LucideIcon>(LucideIconProps {
          icon: "users",
          size: 14.0,
          color: theme::palette().text_muted,
        }))
        .child(
          Text::new(&user_count.to_string())
            .variant(theme::TypographyStyle::Caption)
            .color(theme::PaletteColor::TextMuted),
        ),
    )
    .into()
}

fn top_bar_label(text: &str, variant: theme::TypographyStyle, color: theme::PaletteColor) -> Element {
  Row::new()
    .height(Dimension::Pct(100.0))
    .align_items(Alignment::Center)
    .child(Text::new(text).variant(variant).color(color))
    .into()
}

fn top_bar_plain_icon(ctx: &mut Ctx, icon: &'static str, size: f32) -> Element {
  Row::new()
    .height(Dimension::Pct(100.0))
    .align_items(Alignment::Center)
    .child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon,
      size,
      color: theme::palette().text_muted,
    }))
    .into()
}

fn top_bar_icon(ctx: &mut Ctx, icon: &'static str) -> Element {
  Row::new()
    .width(28.0)
    .height(28.0)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .rounded(theme::RadiusSize::Md)
    .child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon,
      size: 17.0,
      color: theme::palette().text_muted,
    }))
    .into()
}

fn main_body(
  ctx: &mut Ctx,
  info: &ConnectedServerInfo,
  message_input: Signal<String>,
  chat_command_selected_index: Signal<usize>,
  chat_command_scroll_state: ScrollState,
  chat_command_invalid_feedback: ChatCommandInvalidFeedback,
  chat_scroll_state: ScrollState,
  chat_bottom_anchor: Signal<Option<(ChannelId, u64)>>,
  chat_bottom_settle_anchor: Signal<Option<(ChannelId, u64, f32)>>,
  chat_bottom_detached_anchor: Signal<Option<(ChannelId, u64)>>,
  chat_top_anchor: Signal<Option<(ChannelId, u64)>>,
  chat_prepend_settle_anchor: Signal<Option<(ChannelId, u64, f32)>>,
  debug_mode_enabled: bool,
  storage: Option<Storage>,
  session: ServerSession,
  chat_actions: ChatActions,
  watch_stream: &WatchStreamAction,
) -> Element {
  ctx.mount::<MainBody>(MainBodyProps {
    info: info.clone(),
    message_input,
    chat_command_selected_index,
    chat_command_scroll_state,
    chat_command_invalid_feedback,
    chat_scroll_state,
    chat_bottom_anchor,
    chat_bottom_settle_anchor,
    chat_bottom_detached_anchor,
    chat_top_anchor,
    chat_prepend_settle_anchor,
    debug_mode_enabled,
    storage,
    session,
    chat_actions,
    watch_stream: watch_stream.clone(),
  })
}

#[derive(Clone)]
struct MainBodyProps {
  info: ConnectedServerInfo,
  message_input: Signal<String>,
  chat_command_selected_index: Signal<usize>,
  chat_command_scroll_state: ScrollState,
  chat_command_invalid_feedback: ChatCommandInvalidFeedback,
  chat_scroll_state: ScrollState,
  chat_bottom_anchor: Signal<Option<(ChannelId, u64)>>,
  chat_bottom_settle_anchor: Signal<Option<(ChannelId, u64, f32)>>,
  chat_bottom_detached_anchor: Signal<Option<(ChannelId, u64)>>,
  chat_top_anchor: Signal<Option<(ChannelId, u64)>>,
  chat_prepend_settle_anchor: Signal<Option<(ChannelId, u64, f32)>>,
  debug_mode_enabled: bool,
  storage: Option<Storage>,
  session: ServerSession,
  chat_actions: ChatActions,
  watch_stream: WatchStreamAction,
}

impl PartialEq for MainBodyProps {
  fn eq(&self, other: &Self) -> bool {
    self.info == other.info
      && self.debug_mode_enabled == other.debug_mode_enabled
      && self.storage.is_some() == other.storage.is_some()
      && same_session(&self.session, &other.session)
  }
}

impl DevtoolsInspectable for MainBodyProps {}

struct MainBody {
  model_store: Store<Option<MainBodyModel>>,
}

impl Component for MainBody {
  type Props = MainBodyProps;

  fn create(ctx: &mut Ctx) -> Self {
    Self {
      model_store: ctx.store(None),
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let props = ctx.props::<Self::Props>().clone();
    ctx.provide(self.model_store.clone());
    apply_current_model(&self.model_store, &props.session, |lobby| {
      main_body_model(lobby, props.debug_mode_enabled)
    });
    let subscriber = ctx.mount::<MainBodyModelSubscriber>(MainBodyModelSubscriberProps {
      session: props.session.clone(),
      debug_mode_enabled: props.debug_mode_enabled,
    });
    let model = self
      .model_store
      .get()
      .unwrap_or(MainBodyModel::SelectChannel { error: None });
    main_body_view(ctx, subscriber, model, props)
  }
}

#[derive(Clone)]
struct MainBodyModelSubscriberProps {
  session: ServerSession,
  debug_mode_enabled: bool,
}

impl PartialEq for MainBodyModelSubscriberProps {
  fn eq(&self, other: &Self) -> bool {
    self.debug_mode_enabled == other.debug_mode_enabled && same_session(&self.session, &other.session)
  }
}

impl DevtoolsInspectable for MainBodyModelSubscriberProps {}

struct MainBodyModelSubscriber {
  subscription: LobbyModelSubscription,
}

impl Component for MainBodyModelSubscriber {
  type Props = MainBodyModelSubscriberProps;

  fn create(ctx: &mut Ctx) -> Self {
    Self {
      subscription: LobbyModelSubscription::new(ctx),
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let props = ctx.props::<Self::Props>().clone();
    let Some(model_store) = ctx.use_context::<Store<Option<MainBodyModel>>>() else {
      return empty_subscriber_node();
    };

    apply_current_model(&model_store, &props.session, |lobby| {
      main_body_model(lobby, props.debug_mode_enabled)
    });

    let debug_mode_enabled = props.debug_mode_enabled;
    if let Some((_snapshot_generation, model)) =
      self
        .subscription
        .next_model(ctx, props.session.clone(), move |snapshot| {
          main_body_model(&snapshot.lobby, debug_mode_enabled)
        })
    {
      apply_model(&model_store, model);
    }

    empty_subscriber_node()
  }
}

fn main_body_view(ctx: &mut Ctx, subscriber: Element, model: MainBodyModel, props: MainBodyProps) -> Element {
  let body = match model {
    MainBodyModel::DebugChat => {
      let channel = ChatChannel::debug(ctx);
      text_channel_detail(
        ctx,
        channel,
        props.info,
        props.message_input,
        props.chat_command_selected_index,
        props.chat_command_scroll_state,
        props.chat_command_invalid_feedback,
        props.chat_scroll_state,
        props.chat_bottom_anchor,
        props.chat_bottom_settle_anchor,
        props.chat_bottom_detached_anchor,
        props.chat_top_anchor,
        props.chat_prepend_settle_anchor,
        props.debug_mode_enabled,
        props.session,
        props.chat_actions.clone(),
      )
    }
    MainBodyModel::Text {
      channel,
      command_registry,
    } => {
      let channel = ChatChannel::server_text(ctx, &channel, command_registry);
      text_channel_detail(
        ctx,
        channel,
        props.info,
        props.message_input,
        props.chat_command_selected_index,
        props.chat_command_scroll_state,
        props.chat_command_invalid_feedback,
        props.chat_scroll_state,
        props.chat_bottom_anchor,
        props.chat_bottom_settle_anchor,
        props.chat_bottom_detached_anchor,
        props.chat_top_anchor,
        props.chat_prepend_settle_anchor,
        props.debug_mode_enabled,
        props.session,
        props.chat_actions.clone(),
      )
    }
    MainBodyModel::StreamChannel { channel } => stream_channel_detail(
      ctx,
      channel,
      props.info.user_id,
      props.debug_mode_enabled,
      props.storage,
      props.session,
      &props.watch_stream,
    ),
    MainBodyModel::EmptyVoice { error } => empty_voice_state(ctx, error.as_deref()),
    MainBodyModel::SelectChannel { error } => select_channel_state(ctx, error.as_deref()),
  };

  with_subscriber(subscriber, body)
}

fn with_subscriber(subscriber: Element, body: Element) -> Element {
  Column::new()
    .width(Dimension::Pct(100.0))
    .flex(1.0)
    .child(subscriber)
    .child(body)
    .into()
}

fn empty_voice_state(ctx: &mut Ctx, error: Option<&str>) -> Element {
  let metrics = lobby_layout_metrics(ctx);
  let mut body = Column::new()
    .width(Dimension::Pct(100.0))
    .flex(1.0)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .spacing(theme::SpacingSize::Xl)
    .child(
      Row::new()
        .width(64.0)
        .height(64.0)
        .align_items(Alignment::Center)
        .justify(Justify::Center)
        .rounded(16.0)
        .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised))
        .border_inside(1.0, theme::PaletteColor::Border)
        .child(ctx.mount::<LucideIcon>(LucideIconProps {
          icon: "volume-2",
          size: 28.0,
          color: theme::palette().text_secondary,
        })),
    )
    .child(
      Column::new()
        .width(Dimension::Pct(100.0))
        .max_width(metrics.copy_max_width)
        .align_items(Alignment::Center)
        .spacing(theme::SpacingSize::Md)
        .child(Text::new(&ctx.t("lobby.empty.title")).variant(theme::TypographyStyle::Title))
        .child(
          Text::new(&ctx.t("lobby.empty.description"))
            .variant(theme::TypographyStyle::Description)
            .text_align(Alignment::Center)
            .width(Dimension::Pct(100.0)),
        ),
    )
    .child(create_voice_button(ctx));

  if let Some(error) = error {
    body = body.child(error_notice(ctx, error));
  }

  body.into()
}

fn select_channel_state(ctx: &mut Ctx, error: Option<&str>) -> Element {
  let mut body = Column::new()
    .width(Dimension::Pct(100.0))
    .flex(1.0)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .spacing(theme::SpacingSize::Lg)
    .child(
      Text::new(&ctx.t("lobby.select.title"))
        .variant(theme::TypographyStyle::Title)
        .color(theme::PaletteColor::TextPrimary),
    )
    .child(
      Text::new(&ctx.t("lobby.select.description"))
        .variant(theme::TypographyStyle::Description)
        .color(theme::PaletteColor::TextSecondary),
    );

  if let Some(error) = error {
    body = body.child(error_notice(ctx, error));
  }

  body.into()
}

fn create_voice_button(ctx: &mut Ctx) -> Element {
  Row::new()
    .height(34.0)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .spacing(theme::SpacingSize::Sm)
    .padding_horizontal(theme::SpacingSize::Lg)
    .rounded(theme::RadiusSize::Md)
    .background(BackgroundColor::Palette(theme::PaletteColor::Accent))
    .cursor(CursorIcon::Pointer)
    .hovered_style(Style::new().background(BackgroundColor::Palette(theme::PaletteColor::AccentHover)))
    .child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon: "plus",
      size: 16.0,
      color: theme::palette().text_inverse,
    }))
    .child(
      Text::new(&ctx.t("lobby.empty.create"))
        .variant(theme::TypographyStyle::Button)
        .color(theme::PaletteColor::TextInverse),
    )
    .into()
}
