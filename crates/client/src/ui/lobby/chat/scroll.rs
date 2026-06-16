use lurq::{
  app::events::ScrollEvent,
  components::ScrollVertical,
  core::Signal,
  layout::{
    layout_kind::ScrollState,
    scrollbar::{ScrollBarPlacement, ScrollBarStyle},
  },
  node::{Element, dimension::Dimension},
};

use super::{
  super::{ChatHistoryAction, ChatHistoryRequest},
  scroll_policy::{BottomScrollMetrics, plan_bottom_scroll},
};
use crate::{network::protocol::ChannelId, session::ServerSession, theme};

pub(super) fn chat_messages_scroll(
  messages: ScrollVertical,
  scroll_state: ScrollState,
  bottom_settle_anchor: Signal<Option<(ChannelId, u64, f32)>>,
  bottom_anchor: Signal<Option<(ChannelId, u64)>>,
  bottom_detached_anchor: Signal<Option<(ChannelId, u64)>>,
  session: ServerSession,
  chat_history: &ChatHistoryAction,
  channel_id: ChannelId,
  before_id: u64,
  can_page: bool,
) -> Element {
  let history = chat_history.clone();
  let scroll_for_input = scroll_state.clone();
  let scroll_for_reach_top = scroll_state.clone();

  messages
    .width(Dimension::Pct(100.0))
    .height(0.0)
    .flex(1.0)
    .with_scroll_state(scroll_state)
    .scrollbar(chat_scrollbar_style())
    .scrollbar_hovered(|mut style| {
      let palette = theme::palette();
      style.thumb_color = palette.accent_hover;
      style.track_color = palette.surface_input.with_opacity(0.75);
      style
    })
    .on_scroll(move |event: ScrollEvent| {
      if event.delta_y.abs() > 0.0 || event.delta_x.abs() > 0.0 || scroll_for_input.is_dragging() {
        if bottom_settle_anchor.get_untracked().is_some() {
          bottom_settle_anchor.set(None);
        }
        if let Some(anchor) = bottom_anchor.get_untracked() {
          if bottom_detached_anchor.get_untracked() != Some(anchor) {
            bottom_detached_anchor.set(Some(anchor));
          }
        }
      }
    })
    .on_scroll_reach_top(move |event: ScrollEvent| {
      if can_page && session.begin_chat_history_request(channel_id, before_id) {
        tracing::info!(
          target: "chat::history",
          "[chat/history] pagination requested: trigger=reach_top channel={} before={} event_y={:.1} scroll_y={:.1} viewport_h={:.1} content_h={:.1} delta_y={:.1}",
          channel_id,
          before_id,
          event.y,
          scroll_for_reach_top.scroll_y(),
          scroll_for_reach_top.viewport_height(),
          scroll_for_reach_top.content_height(),
          event.delta_y,
        );
        history.run(vec![ChatHistoryRequest { channel_id, before_id }]);
      }
    })
    .into()
}

pub(super) fn schedule_chat_scroll_to_bottom(
  channel_id: ChannelId,
  newest_message_id: u64,
  force_bottom: bool,
  scroll_state: ScrollState,
  bottom_anchor: Signal<Option<(ChannelId, u64)>>,
  bottom_settle_anchor: Signal<Option<(ChannelId, u64, f32)>>,
  bottom_detached_anchor: Signal<Option<(ChannelId, u64)>>,
) {
  let current_bottom_anchor = bottom_anchor.get_untracked();
  let current_bottom_settle_anchor = bottom_settle_anchor.get_untracked();
  let current_bottom_detached_anchor = bottom_detached_anchor.get_untracked();
  let plan = plan_bottom_scroll(
    channel_id,
    newest_message_id,
    force_bottom,
    current_bottom_anchor,
    current_bottom_settle_anchor,
    current_bottom_detached_anchor,
    BottomScrollMetrics {
      scroll_y: scroll_state.scroll_y(),
      viewport_height: scroll_state.viewport_height(),
      content_height: scroll_state.content_height(),
    },
  );

  if current_bottom_anchor != plan.bottom_anchor {
    bottom_anchor.set(plan.bottom_anchor);
  }
  if current_bottom_settle_anchor != plan.bottom_settle_anchor {
    bottom_settle_anchor.set(plan.bottom_settle_anchor);
  }
  if current_bottom_detached_anchor != plan.bottom_detached_anchor {
    bottom_detached_anchor.set(plan.bottom_detached_anchor);
  }
  if plan.scroll_to_bottom_pending {
    scroll_state.scroll_to_bottom_pending();
  }
}

pub(super) fn preserve_chat_scroll_on_prepend(
  channel_id: ChannelId,
  oldest_message_id: u64,
  scroll_state: ScrollState,
  top_anchor: Signal<Option<(ChannelId, u64)>>,
  prepend_settle_anchor: Signal<Option<(ChannelId, u64, f32)>>,
) -> bool {
  if oldest_message_id == 0 {
    return false;
  }

  let mut prepended_history = false;
  if let Some((anchor_channel_id, previous_oldest_message_id)) = top_anchor.get_untracked()
    && anchor_channel_id == channel_id
    && oldest_message_id < previous_oldest_message_id
  {
    prepended_history = true;
    let previous_content_height = scroll_state.content_height();
    let previous_scroll_y = scroll_state.scroll_y();
    let is_dragging = scroll_state.is_dragging();
    scroll_state.preserve_prepend_anchor_pending();
    prepend_settle_anchor.set(Some((channel_id, oldest_message_id, previous_content_height)));
    tracing::info!(
      target: "chat::history",
      "[chat/history] pagination follow-up suppressed: reason=preserve_prepend channel={} previous_oldest={} current_oldest={} previous_scroll_y={:.1} dragging={} previous_content_h={:.1}",
      channel_id,
      previous_oldest_message_id,
      oldest_message_id,
      previous_scroll_y,
      is_dragging,
      previous_content_height,
    );
  }

  top_anchor.set(Some((channel_id, oldest_message_id)));
  prepended_history
}

pub(super) fn chat_scrollbar_style() -> ScrollBarStyle {
  let palette = theme::palette();
  ScrollBarStyle {
    width: 8.0,
    min_thumb_length: 32.0,
    track_color: palette.surface_input.with_opacity(0.55),
    thumb_color: palette.accent,
    thumb_radius: 4.0,
    track_radius: 4.0,
    padding: 2.0,
    placement: ScrollBarPlacement::Reserved,
    ..ScrollBarStyle::default()
  }
}
