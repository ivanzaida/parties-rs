use std::{
  sync::{Arc, Mutex},
  time::Duration,
};

use lurq::{
  app::{
    component::{Component, ComponentInfo, DevtoolsInspectable},
    ctx::{Ctx, Interval},
  },
  components::{Column, Row, Stack, Text, TextOverflow},
  core::Signal,
  layout::{Alignment, StackAlignment, layout_kind::Justify},
  node::{BackgroundColor, CursorIcon, Element, Style, border::Border, color::Color, dimension::Dimension},
};

use super::{
  StopWatchingAction, WatchStreamAction,
  stream_shared::{
    ChannelScreenShare, live_badge, resolution_badge, screen_shares_for_channel, stream_avatar, stream_footer_meta,
    stream_name, stream_speaking,
  },
};
use crate::{
  network::protocol::{ChannelId, UserId},
  session::{LobbyScreenShare, LobbyState, ServerSession},
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

pub(super) fn watched_stream_for_channel(lobby: &LobbyState, channel_id: ChannelId) -> Option<ChannelScreenShare<'_>> {
  let watching_user_id = lobby.watching_user_id?;
  screen_shares_for_channel(lobby, channel_id)
    .into_iter()
    .find(|stream| stream.share.sharer_user_id == watching_user_id)
}

pub(super) fn stream_watching_top_bar(
  ctx: &mut Ctx,
  stream: ChannelScreenShare<'_>,
  start_stream_modal_open: Signal<bool>,
  stop_watching: &StopWatchingAction,
) -> Element {
  let name = stream_name(ctx, &stream);
  let title = ctx.t_args("lobby.stream_browser.watching.screen_name", [("user", name)]);

  Row::new()
    .width(Dimension::Pct(100.0))
    .height(56.0)
    .align_items(Alignment::Center)
    .justify(Justify::SpaceBetween)
    .padding_horizontal(20.0)
    .border_bottom(Border::inside(1.0, theme::PaletteColor::Border))
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

pub(super) fn stream_watching(
  ctx: &mut Ctx,
  stream: ChannelScreenShare<'_>,
  streams: Vec<ChannelScreenShare<'_>>,
  error: Option<&str>,
  storage: Option<Storage>,
  session: ServerSession,
  watch_stream: &WatchStreamAction,
) -> Element {
  let watched_user_id = stream.share.sharer_user_id;
  let mut body = Column::new()
    .width(Dimension::Pct(100.0))
    .height(Dimension::Pct(100.0))
    .flex(1.0)
    .spacing(16.0)
    .padding(20.0)
    .child(stage(ctx, &stream, storage, &session))
    .child(stream_switcher(ctx, watched_user_id, streams, watch_stream));

  if let Some(error) = error {
    body = body.child(super::shared::error_notice(ctx, error));
  }

  body.into()
}

fn stage(ctx: &mut Ctx, stream: &ChannelScreenShare<'_>, storage: Option<Storage>, session: &ServerSession) -> Element {
  let name = stream_name(ctx, stream);
  let title = ctx.t_args("lobby.stream_browser.watching.screen_name", [("user", name.clone())]);
  let meta = stream_footer_meta(ctx, &name, stream.share);
  let speaking = stream_speaking(stream);
  let image = session.video_frame(stream.share.sharer_user_id);
  let video_error = session.video_error(stream.share.sharer_user_id);

  let mut stage = Stack::new()
    .stack_align(StackAlignment::Center)
    .width(Dimension::Pct(100.0))
    .flex(1.0)
    .rounded(10.0)
    .clip()
    .background(BackgroundColor::Color(Color::from_hex("#0F1013")))
    .border_inside(1.0, theme::PaletteColor::Border);

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

  stage
    .child(
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
            .child(resolution_badge(ctx, stream.share)),
        )
        .child(
          Row::new()
            .width(Dimension::Pct(100.0))
            .align_items(Alignment::End)
            .justify(Justify::SpaceBetween)
            .child(streamer_label(&name, &title, &meta, speaking))
            .child(stage_controls(ctx, session, storage, stream.share.sharer_user_id)),
        ),
    )
    .into()
}

fn stream_error_text(ctx: &mut Ctx, error: &crate::session::VideoStreamError) -> (String, String) {
  match error.i18n_key {
    Some("lobby.stream_error.unsupported_av1") => (
      ctx.t("lobby.stream_error.unsupported_av1.title").to_string(),
      ctx.t("lobby.stream_error.unsupported_av1.message").to_string(),
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
  streams: Vec<ChannelScreenShare<'_>>,
  watch_stream: &WatchStreamAction,
) -> Element {
  let mut row = Row::new()
    .width(Dimension::Pct(100.0))
    .height(126.0)
    .align_items(Alignment::Center)
    .spacing(10.0);

  for stream in streams {
    row = row.child(switcher_card(ctx, stream, watched_user_id, watch_stream));
  }

  row.into()
}

fn switcher_card(
  ctx: &mut Ctx,
  stream: ChannelScreenShare<'_>,
  watched_user_id: UserId,
  watch_stream: &WatchStreamAction,
) -> Element {
  let sharer_id = stream.share.sharer_user_id;
  let watching = sharer_id == watched_user_id;
  let name = stream_name(ctx, &stream);
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
    .child(mini_thumb(ctx, stream.share))
    .child(
      Row::new()
        .width(Dimension::Pct(100.0))
        .height(44.0)
        .align_items(Alignment::Center)
        .padding_vertical(8.0)
        .padding_horizontal(10.0)
        .spacing(8.0)
        .child(stream_avatar(&name, 22.0, speaking))
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
      && self.session.info().map(|info| info.address) == other.session.info().map(|info| info.address)
      && self.storage.is_some() == other.storage.is_some()
  }
}

impl DevtoolsInspectable for StreamVolumeControlProps {
  fn write_info(&self, buffer: &mut Vec<ComponentInfo>) {
    buffer.push(ComponentInfo::with_value(
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
  apply_interval: Interval,
}

impl Component for StreamVolumeControl {
  type Props = StreamVolumeControlProps;

  fn create(ctx: &mut Ctx) -> Self {
    let props = ctx.props::<Self::Props>().clone();
    let server_id = props.session.info().map(|info| info.address);
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
    let apply_interval = {
      let apply_session = apply_session.clone();
      let user_id = user_id.clone();
      let value = value.clone();
      let last_applied_volume = last_applied_volume.clone();
      let interval = ctx.create_interval(Duration::from_millis(16), move || {
        let volume = value.get_untracked().clamp(0, 100);
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
      interval.start();
      interval
    };

    Self {
      user_id,
      server_id,
      value,
      apply_session,
      last_applied_volume,
      apply_interval,
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let props = ctx.props::<Self::Props>().clone();
    let server_id = props.session.info().map(|info| info.address);

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

  fn on_unmounted(&self) {
    self.apply_interval.stop();
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
