use std::sync::{Arc, Mutex};

use lurq::{
  app::{
    component::{Component, ComponentInfo, DevtoolsFormatter, DevtoolsInspectable},
    ctx::Ctx,
  },
  components::{Column, Rect, Row, Stack, Text, TextOverflow},
  core::{Signal, Store},
  layout::{Alignment, StackAlignment, layout_kind::Justify},
  node::{BackgroundColor, CursorIcon, Element, Style, border::Border, color::Color, dimension::Dimension},
};

#[cfg(test)]
pub(super) use super::model::watched_stream_for_channel;
use super::{
  StopWatchingAction, WatchStreamAction,
  model::{ChannelScreenShare, StreamWatchingModel, stream_speaking, stream_watching_model},
  session_identity::{same_session, session_address},
  stream_browser::stream_browser,
  stream_shared::{live_badge, resolution_badge, stream_avatar, stream_footer_meta, stream_name},
  subscription::{LobbyModelSubscription, apply_current_optional_model, apply_optional_model},
};
use crate::{
  network::protocol::{ChannelId, UserId},
  session::{LobbyChannel, LobbyScreenShare, ServerSession},
  storage::Storage,
  theme,
  ui::common::{
    lucide_icon::{LucideIcon, LucideIconProps},
    percent_slider::{PercentSliderSaveAction, percent_slider_control},
  },
};

const STREAM_VOLUME_CONTROL_WIDTH: f32 = 168.0;
const STREAM_VOLUME_TRACK_WIDTH: f32 = 104.0;
const STREAM_VOLUME_VALUE_WIDTH: f32 = 36.0;
const STREAM_VOLUME_VALUE_SPACING: f32 = 8.0;

pub(super) fn stream_watching_top_bar(
  ctx: &mut Ctx,
  subscriber: Element,
  stream: ChannelScreenShare,
  debug_user_ids: bool,
  start_stream_modal_open: Signal<bool>,
  stop_watching: &StopWatchingAction,
) -> Element {
  let name = stream_name(ctx, &stream, debug_user_ids);
  let title = ctx.t_args("lobby.stream_browser.watching.screen_name", [("user", name)]);

  Row::new()
    .width(Dimension::Pct(100.0))
    .height(56.0)
    .align_items(Alignment::Center)
    .justify(Justify::SpaceBetween)
    .padding_horizontal(20.0)
    .border_bottom(Border::inside(1.0, theme::PaletteColor::Border))
    .child(subscriber)
    .child(
      Row::new()
        .align_items(Alignment::Center)
        .spacing(10.0)
        .child(back_button(ctx, stop_watching))
        .child(
          Text::new(&title)
            .variant(theme::TypographyStyle::Button)
            .color(theme::PaletteColor::TextPrimary),
        )
        .child(live_badge(ctx)),
    )
    .child(start_stream_button(ctx, start_stream_modal_open))
    .into()
}

pub(super) fn stream_channel_detail(
  ctx: &mut Ctx,
  channel: LobbyChannel,
  local_user_id: UserId,
  debug_user_ids: bool,
  storage: Option<Storage>,
  session: ServerSession,
  watch_stream: &WatchStreamAction,
) -> Element {
  let key = format!("stream-channel-detail-{}", channel.id);
  ctx.mount_keyed::<StreamWatchingPane>(
    &key,
    StreamWatchingPaneProps {
      channel,
      local_user_id,
      debug_user_ids,
      storage,
      session,
      watch_stream: watch_stream.clone(),
    },
  )
}

#[derive(Clone)]
struct StreamWatchingPaneProps {
  channel: LobbyChannel,
  local_user_id: UserId,
  debug_user_ids: bool,
  storage: Option<Storage>,
  session: ServerSession,
  watch_stream: WatchStreamAction,
}

impl PartialEq for StreamWatchingPaneProps {
  fn eq(&self, other: &Self) -> bool {
    self.channel == other.channel
      && self.local_user_id == other.local_user_id
      && self.debug_user_ids == other.debug_user_ids
      && self.storage.is_some() == other.storage.is_some()
      && same_session(&self.session, &other.session)
  }
}

impl DevtoolsInspectable for StreamWatchingPaneProps {}

struct StreamWatchingPane {
  model_store: Store<Option<StreamWatchingModel>>,
}

impl Component for StreamWatchingPane {
  type Props = StreamWatchingPaneProps;

  fn create(ctx: &mut Ctx) -> Self {
    Self {
      model_store: ctx.store(None),
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let props = ctx.props::<Self::Props>().clone();
    ctx.provide(self.model_store.clone());
    if self.model_store.with(Option::is_none) {
      let channel_id = props.channel.id;
      apply_current_optional_model(&self.model_store, &props.session, |lobby| {
        stream_watching_model(lobby, channel_id)
      });
    }
    let subscriber = ctx.mount::<StreamWatchingModelSubscriber>(StreamWatchingModelSubscriberProps {
      session: props.session.clone(),
      channel_id: props.channel.id,
    });
    let Some(model) = self.model_store.get() else {
      return Column::new()
        .width(Dimension::Pct(100.0))
        .height(Dimension::Pct(100.0))
        .flex(1.0)
        .child(subscriber)
        .child(stream_browser(
          ctx,
          props.channel,
          props.local_user_id,
          props.debug_user_ids,
          props.session.clone(),
          &props.watch_stream,
        ))
        .into();
    };

    stream_watching_view(
      ctx,
      subscriber,
      model,
      props.debug_user_ids,
      props.storage,
      props.session,
      &props.watch_stream,
    )
  }
}

fn stream_watching_view(
  ctx: &mut Ctx,
  subscriber: Element,
  model: StreamWatchingModel,
  debug_user_ids: bool,
  storage: Option<Storage>,
  session: ServerSession,
  watch_stream: &WatchStreamAction,
) -> Element {
  let StreamWatchingModel { stream, streams, error } = model;
  let watched_user_id = stream.share.sharer_user_id;
  let mut body = Column::new()
    .width(Dimension::Pct(100.0))
    .height(Dimension::Pct(100.0))
    .flex(1.0)
    .spacing(16.0)
    .padding(20.0)
    .child(subscriber)
    .child(stage(ctx, &stream, debug_user_ids, storage, &session))
    .child(stream_switcher(
      ctx,
      watched_user_id,
      streams,
      debug_user_ids,
      watch_stream,
    ));

  if let Some(error) = error.as_deref() {
    body = body.child(super::shared::error_notice(ctx, error));
  }

  body.into()
}

#[derive(Clone)]
struct StreamWatchingModelSubscriberProps {
  session: ServerSession,
  channel_id: ChannelId,
}

impl PartialEq for StreamWatchingModelSubscriberProps {
  fn eq(&self, other: &Self) -> bool {
    self.channel_id == other.channel_id && same_session(&self.session, &other.session)
  }
}

impl DevtoolsInspectable for StreamWatchingModelSubscriberProps {}

struct StreamWatchingModelSubscriber {
  subscription: LobbyModelSubscription,
}

impl Component for StreamWatchingModelSubscriber {
  type Props = StreamWatchingModelSubscriberProps;

  fn create(ctx: &mut Ctx) -> Self {
    Self {
      subscription: LobbyModelSubscription::new(ctx),
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let props = ctx.props::<Self::Props>().clone();
    let Some(model_store) = ctx.use_context::<Store<Option<StreamWatchingModel>>>() else {
      return empty_subscriber_node();
    };

    apply_current_optional_model(&model_store, &props.session, |lobby| {
      stream_watching_model(lobby, props.channel_id)
    });

    let channel_id = props.channel_id;
    if let Some((_snapshot_generation, model)) =
      self
        .subscription
        .next_model(ctx, props.session.clone(), move |snapshot| {
          stream_watching_model(&snapshot.lobby, channel_id)
        })
    {
      apply_optional_model(&model_store, model);
    }

    empty_subscriber_node()
  }
}

fn empty_subscriber_node() -> Element {
  Rect::new(0.0, 0.0).into()
}

fn stage(
  ctx: &mut Ctx,
  stream: &ChannelScreenShare,
  debug_user_ids: bool,
  storage: Option<Storage>,
  session: &ServerSession,
) -> Element {
  let key = format!("watched-stage-{}", stream.share.sharer_user_id);
  ctx.mount_keyed::<WatchedStreamStage>(
    &key,
    WatchedStreamStageProps {
      share: stream.share.clone(),
      user: stream.user.clone(),
      debug_user_ids,
      storage,
      session: session.clone(),
    },
  )
}

#[derive(Clone)]
struct WatchedStreamStageProps {
  share: LobbyScreenShare,
  user: Option<crate::session::LobbyUser>,
  debug_user_ids: bool,
  storage: Option<Storage>,
  session: ServerSession,
}

impl PartialEq for WatchedStreamStageProps {
  fn eq(&self, other: &Self) -> bool {
    self.share == other.share
      && self.user == other.user
      && self.debug_user_ids == other.debug_user_ids
      && self.storage.is_some() == other.storage.is_some()
      && same_session(&self.session, &other.session)
  }
}

impl DevtoolsInspectable for WatchedStreamStageProps {
  fn inspect(&self, formatter: &mut DevtoolsFormatter<'_>) {
    formatter.buffer_mut().push(ComponentInfo::with_value(
      "sharer_user_id",
      std::any::type_name::<UserId>(),
      self.share.sharer_user_id.to_string(),
    ));
  }
}

struct WatchedStreamStage {
  hovered: Signal<bool>,
}

impl Component for WatchedStreamStage {
  type Props = WatchedStreamStageProps;

  fn create(ctx: &mut Ctx) -> Self {
    Self {
      hovered: ctx.signal(false),
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let props = ctx.props::<Self::Props>().clone();
    let stream = ChannelScreenShare {
      share: props.share.clone(),
      user: props.user.clone(),
    };
    watched_stream_stage(
      ctx,
      &stream,
      props.debug_user_ids,
      props.storage,
      &props.session,
      self.hovered.clone(),
    )
  }
}

fn watched_stream_stage(
  ctx: &mut Ctx,
  stream: &ChannelScreenShare,
  debug_user_ids: bool,
  storage: Option<Storage>,
  session: &ServerSession,
  hovered: Signal<bool>,
) -> Element {
  let image = session.video_frame(stream.share.sharer_user_id);
  let video_error = session.video_error(stream.share.sharer_user_id);

  let mut stage = Stack::new()
    .stack_align(StackAlignment::Center)
    .width(Dimension::Pct(100.0))
    .flex(1.0)
    .rounded(10.0)
    .clip()
    .background(BackgroundColor::Color(Color::from_hex("#0F1013")))
    .border_inside(1.0, theme::PaletteColor::Border)
    .on_mouse_enter({
      let hovered = hovered.clone();
      move || hovered.set(true)
    })
    .on_mouse_leave({
      let hovered = hovered.clone();
      move || hovered.set(false)
    });

  if let Some(image) = image {
    stage = stage.background_image(image).background_contain();
  } else if let Some(error) = video_error {
    let (title, message) = stream_error_text(ctx, &error);
    stage = stage.child(stream_error_panel(ctx, &title, &message));
  } else {
    stage = stage.child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon: "monitor",
      size: 72.0,
      color: Color::from_hex("#2E333B"),
    }));
  }

  if hovered.get() {
    let name = stream_name(ctx, stream, debug_user_ids);
    let avatar_name = stream_name(ctx, stream, false);
    let title = ctx.t_args("lobby.stream_browser.watching.screen_name", [("user", name.clone())]);
    let meta = stream_footer_meta(ctx, &name, &stream.share);
    let speaking = stream_speaking(stream);

    stage = stage.child(
      Column::new()
        .width(Dimension::Pct(100.0))
        .height(Dimension::Pct(100.0))
        .justify(Justify::SpaceBetween)
        .padding(14.0)
        .child(
          Row::new()
            .width(Dimension::Pct(100.0))
            .align_items(Alignment::Center)
            .justify(Justify::SpaceBetween)
            .child(live_badge(ctx))
            .child(resolution_badge(ctx, &stream.share)),
        )
        .child(
          Row::new()
            .width(Dimension::Pct(100.0))
            .align_items(Alignment::End)
            .justify(Justify::SpaceBetween)
            .child(streamer_label(&avatar_name, &title, &meta, speaking))
            .child(stage_controls(ctx, session, storage, stream.share.sharer_user_id)),
        ),
    );
  }

  stage.into()
}

fn stream_error_text(ctx: &mut Ctx, error: &crate::session::VideoStreamError) -> (String, String) {
  match error.i18n_key {
    Some("lobby.stream_error.unsupported_av1") => (
      ctx.t("lobby.stream_error.unsupported_av1.title").to_string(),
      ctx.t("lobby.stream_error.unsupported_av1.message").to_string(),
    ),
    Some("lobby.stream_error.decoder_unavailable") => (
      ctx.t("lobby.stream_error.decoder_unavailable.title").to_string(),
      ctx
        .t_args(
          "lobby.stream_error.decoder_unavailable.message",
          [("reason", error.message.clone())],
        )
        .to_string(),
    ),
    _ => (error.title.clone(), error.message.clone()),
  }
}

fn stream_error_panel(ctx: &mut Ctx, title: &str, message: &str) -> Element {
  Column::new()
    .width(420.0)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .spacing(14.0)
    .padding(24.0)
    .rounded(14.0)
    .background(BackgroundColor::Color(Color::from_hex("#15171AE6")))
    .border_inside(1.0, theme::PaletteColor::BorderStrong)
    .child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon: "triangle-alert",
      size: 42.0,
      color: theme::palette().danger,
    }))
    .child(
      Text::new(title)
        .variant(theme::TypographyStyle::Title)
        .color(theme::PaletteColor::TextPrimary),
    )
    .child(
      Text::new(message)
        .variant(theme::TypographyStyle::Body)
        .color(theme::PaletteColor::TextSecondary),
    )
    .into()
}

fn stream_switcher(
  ctx: &mut Ctx,
  watched_user_id: UserId,
  streams: Vec<ChannelScreenShare>,
  debug_user_ids: bool,
  watch_stream: &WatchStreamAction,
) -> Element {
  let mut row = Row::new()
    .width(Dimension::Pct(100.0))
    .height(126.0)
    .align_items(Alignment::Center)
    .spacing(10.0);

  for stream in streams {
    row = row.child(switcher_card(
      ctx,
      stream,
      watched_user_id,
      debug_user_ids,
      watch_stream,
    ));
  }

  row.into()
}

fn switcher_card(
  ctx: &mut Ctx,
  stream: ChannelScreenShare,
  watched_user_id: UserId,
  debug_user_ids: bool,
  watch_stream: &WatchStreamAction,
) -> Element {
  let sharer_id = stream.share.sharer_user_id;
  let watching = sharer_id == watched_user_id;
  let name = stream_name(ctx, &stream, debug_user_ids);
  let avatar_name = stream_name(ctx, &stream, false);
  let title = ctx.t_args("lobby.stream_browser.watching.screen_name", [("user", name.clone())]);
  let speaking = stream_speaking(&stream);
  let action = watch_stream.clone();
  let mut card = Column::new()
    .width(168.0)
    .height(126.0)
    .rounded(8.0)
    .clip()
    .background(BackgroundColor::Color(Color::from_hex("#15171A")))
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
    .child(mini_thumb(ctx, &stream.share))
    .child(
      Row::new()
        .width(Dimension::Pct(100.0))
        .height(44.0)
        .align_items(Alignment::Center)
        .padding_vertical(8.0)
        .padding_horizontal(10.0)
        .spacing(8.0)
        .child(stream_avatar(&avatar_name, 22.0, speaking))
        .child(
          Text::new(&title)
            .width(Dimension::Pct(100.0))
            .flex(1.0)
            .variant(theme::TypographyStyle::Caption)
            .color(theme::PaletteColor::TextPrimary)
            .nowrap()
            .text_overflow(TextOverflow::Elipsis),
        ),
    );

  if !watching && !watch_stream.state().get().is_pending() {
    card = card.on_click(move |_| action.run(sharer_id));
  }

  card.into()
}

fn mini_thumb(ctx: &mut Ctx, stream: &LobbyScreenShare) -> Element {
  Stack::new()
    .stack_align(StackAlignment::Center)
    .width(Dimension::Pct(100.0))
    .height(82.0)
    .background(BackgroundColor::Color(Color::from_hex("#0F1013")))
    .child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon: "monitor",
      size: 28.0,
      color: Color::from_hex("#2E333B"),
    }))
    .child(
      Column::new()
        .width(Dimension::Pct(100.0))
        .height(Dimension::Pct(100.0))
        .justify(Justify::End)
        .padding(8.0)
        .child(
          Row::new()
            .width(Dimension::Pct(100.0))
            .justify(Justify::End)
            .child(resolution_badge(ctx, stream)),
        ),
    )
    .into()
}

fn streamer_label(name: &str, title: &str, meta: &str, active: bool) -> Element {
  Row::new()
    .align_items(Alignment::Center)
    .spacing(10.0)
    .child(stream_avatar(name, 32.0, active))
    .child(
      Column::new()
        .spacing(2.0)
        .child(
          Text::new(title)
            .variant(theme::TypographyStyle::Button)
            .color(theme::PaletteColor::TextPrimary),
        )
        .child(
          Text::new(meta)
            .variant(theme::TypographyStyle::Caption)
            .color(theme::PaletteColor::TextMuted),
        ),
    )
    .into()
}

fn stage_controls(ctx: &mut Ctx, session: &ServerSession, storage: Option<Storage>, user_id: UserId) -> Element {
  let key = format!("stream-volume-{user_id}");

  Row::new()
    .align_items(Alignment::Center)
    .spacing(8.0)
    .padding(6.0)
    .rounded(10.0)
    .background(BackgroundColor::Color(Color::from_hex("#000000A6")))
    .child(stage_control_icon(ctx, "volume-2"))
    .child(ctx.mount_keyed::<StreamVolumeControl>(
      &key,
      StreamVolumeControlProps {
        user_id,
        session: session.clone(),
        storage,
      },
    ))
    .into()
}

#[derive(Clone)]
struct StreamVolumeControlProps {
  user_id: UserId,
  session: ServerSession,
  storage: Option<Storage>,
}

impl PartialEq for StreamVolumeControlProps {
  fn eq(&self, other: &Self) -> bool {
    self.user_id == other.user_id
      && same_session(&self.session, &other.session)
      && self.storage.is_some() == other.storage.is_some()
  }
}

impl DevtoolsInspectable for StreamVolumeControlProps {
  fn inspect(&self, formatter: &mut DevtoolsFormatter<'_>) {
    formatter.buffer_mut().push(ComponentInfo::with_value(
      "user_id",
      std::any::type_name::<UserId>(),
      self.user_id.to_string(),
    ));
  }
}

struct StreamVolumeControl {
  user_id: Signal<UserId>,
  server_id: Signal<Option<String>>,
  value: Signal<i32>,
  apply_session: Arc<Mutex<ServerSession>>,
  last_applied_volume: Arc<Mutex<i32>>,
}

impl Component for StreamVolumeControl {
  type Props = StreamVolumeControlProps;

  fn create(ctx: &mut Ctx) -> Self {
    let props = ctx.props::<Self::Props>().clone();
    let server_id = session_address(&props.session);
    let initial = load_stream_volume(
      props.storage.as_ref(),
      &props.session,
      server_id.as_deref(),
      props.user_id,
    );
    props.session.set_stream_volume(props.user_id, initial);
    let user_id = ctx.signal(props.user_id);
    let server_id = ctx.signal(server_id);
    let value = ctx.signal(initial);
    let apply_session = Arc::new(Mutex::new(props.session));
    let last_applied_volume = Arc::new(Mutex::new(initial));
    {
      let apply_session = apply_session.clone();
      let user_id = user_id.clone();
      let last_applied_volume = last_applied_volume.clone();
      ctx.watch(&value, move |volume| {
        let volume = (*volume).clamp(0, 100);
        let mut last_applied_volume = last_applied_volume
          .lock()
          .expect("stream volume last-applied lock poisoned");
        if *last_applied_volume != volume {
          apply_session
            .lock()
            .expect("stream volume session lock poisoned")
            .set_stream_volume(user_id.get_untracked(), volume);
          *last_applied_volume = volume;
        }
      });
    }

    Self {
      user_id,
      server_id,
      value,
      apply_session,
      last_applied_volume,
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let props = ctx.props::<Self::Props>().clone();
    let server_id = session_address(&props.session);

    *self.apply_session.lock().expect("stream volume session lock poisoned") = props.session.clone();

    if self.user_id.get_untracked() != props.user_id || self.server_id.get_untracked() != server_id {
      let value = load_stream_volume(
        props.storage.as_ref(),
        &props.session,
        server_id.as_deref(),
        props.user_id,
      );
      self.user_id.set(props.user_id);
      self.server_id.set(server_id.clone());
      self.value.set(value);
      props.session.set_stream_volume(props.user_id, value);
      *self
        .last_applied_volume
        .lock()
        .expect("stream volume last-applied lock poisoned") = value;
    }

    let save_session = props.session.clone();
    let save_storage = props.storage.clone();
    let save_server_id = server_id.clone();
    let save_user_id = props.user_id;

    stream_volume_control(
      ctx,
      self.value.clone(),
      Arc::new(move |volume| {
        let volume = volume.clamp(0, 100);
        save_session.set_stream_volume(save_user_id, volume);
        if let (Some(storage), Some(server_id)) = (save_storage.as_ref(), save_server_id.as_deref()) {
          let _ = storage.save_stream_volume_override(server_id, save_user_id, volume);
        }
      }),
    )
  }
}

fn load_stream_volume(
  storage: Option<&Storage>,
  session: &ServerSession,
  server_id: Option<&str>,
  user_id: UserId,
) -> i32 {
  storage
    .zip(server_id)
    .and_then(|(storage, server_id)| storage.load_stream_volume_override(server_id, user_id).ok().flatten())
    .unwrap_or_else(|| session.stream_volume(user_id))
    .clamp(0, 100)
}

fn stream_volume_control(ctx: &mut Ctx, value: Signal<i32>, on_blur: PercentSliderSaveAction) -> Element {
  percent_slider_control(
    ctx,
    value,
    STREAM_VOLUME_CONTROL_WIDTH,
    STREAM_VOLUME_TRACK_WIDTH,
    STREAM_VOLUME_VALUE_WIDTH,
    STREAM_VOLUME_VALUE_SPACING,
    on_blur,
  )
}

fn stage_control_icon(ctx: &mut Ctx, icon: &'static str) -> Element {
  Row::new()
    .width(28.0)
    .height(28.0)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .rounded(6.0)
    .child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon,
      size: 16.0,
      color: theme::palette().text_secondary,
    }))
    .into()
}

fn back_button(ctx: &mut Ctx, stop_watching: &StopWatchingAction) -> Element {
  let pending = stop_watching.state().get().is_pending();
  let action = stop_watching.clone();
  let mut button = Row::new()
    .width(28.0)
    .height(28.0)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .rounded(6.0)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised))
    .border_inside(1.0, theme::PaletteColor::Border)
    .cursor(CursorIcon::Pointer)
    .hovered_style(Style::new().background(BackgroundColor::Palette(theme::PaletteColor::SurfaceInput)))
    .child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon: "arrow-left",
      size: 16.0,
      color: theme::palette().text_secondary,
    }));

  if !pending {
    button = button.on_click(move |_| action.run(()));
  }

  button.into()
}

fn start_stream_button(ctx: &mut Ctx, start_stream_modal_open: Signal<bool>) -> Element {
  let open = start_stream_modal_open.clone();
  let mut button = Row::new()
    .height(34.0)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .spacing(7.0)
    .padding_horizontal(14.0)
    .rounded(5.0)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised))
    .border_inside(1.0, theme::PaletteColor::BorderStrong)
    .cursor(CursorIcon::Pointer)
    .hovered_style(Style::new().background(BackgroundColor::Palette(theme::PaletteColor::SurfaceInput)))
    .child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon: "monitor-up",
      size: 16.0,
      color: theme::palette().text_secondary,
    }))
    .child(
      Text::new(&ctx.t("lobby.stream_browser.watching.share_screen"))
        .variant(theme::TypographyStyle::Button)
        .color(theme::PaletteColor::TextPrimary),
    );

  button = button.on_click(move |_| open.set(true));

  button.into()
}

#[cfg(test)]
#[path = "../../../tests/unit/ui/lobby/stream_watching.rs"]
mod tests;
