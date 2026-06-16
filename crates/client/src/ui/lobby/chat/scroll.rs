use lurq::{
  components::{Column, ScrollVertical},
  core::Signal,
  layout::{
    layout_kind::ScrollState,
    scrollbar::{ScrollBarPlacement, ScrollBarStyle},
  },
  node::{Element, dimension::Dimension},
};

use super::super::{ChatHistoryAction, ChatHistoryRequest};
use crate::{network::protocol::ChannelId, session::ServerSession, theme};

pub(super) fn chat_messages_scroll(
  messages: Column,
  scroll_state: ScrollState,
  scroll_revision: Signal<u64>,
  session: ServerSession,
  chat_history: &ChatHistoryAction,
  channel_id: ChannelId,
  before_id: u64,
  can_page: bool,
) -> Element {
  let history = chat_history.clone();
  let scroll = scroll_state.clone();
  let revision = scroll_revision.clone();

  ScrollVertical::new(messages)
    .width(Dimension::Pct(100.0))
    .height(Dimension::Pct(100.0))
    .flex(1.0)
    .with_scroll_state(scroll_state)
    .scrollbar(chat_scrollbar_style())
    .scrollbar_hovered(|mut style| {
      let palette = theme::palette();
      style.thumb_color = palette.accent_hover;
      style.track_color = palette.surface_input.with_opacity(0.75);
      style
    })
    .on_scroll(move |event| {
      revision.set(revision.get_untracked().wrapping_add(1));
      if can_page
        && (event.y <= 48.0 || scroll.scroll_y() <= 48.0)
        && session.begin_chat_history_request(channel_id, before_id)
      {
        history.run(vec![ChatHistoryRequest { channel_id, before_id }]);
      }
    })
    .into()
}

pub(super) fn request_chat_history_if_at_top(
  scroll_state: ScrollState,
  bottom_settle_anchor: Signal<Option<(ChannelId, u64)>>,
  session: ServerSession,
  chat_history: &ChatHistoryAction,
  channel_id: ChannelId,
  before_id: u64,
  can_page: bool,
) {
  if !can_page || bottom_settle_anchor.get_untracked().is_some() || scroll_state.viewport_height() <= 0.0 {
    return;
  }

  let near_top = scroll_state.scroll_y() <= 48.0;
  let scrollable = scroll_state.content_height() > scroll_state.viewport_height();
  if near_top && scrollable && session.begin_chat_history_request(channel_id, before_id) {
    chat_history.run(vec![ChatHistoryRequest { channel_id, before_id }]);
  }
}

pub(super) fn schedule_chat_scroll_to_bottom(
  channel_id: ChannelId,
  newest_message_id: u64,
  force_bottom: bool,
  scroll_state: ScrollState,
  bottom_anchor: Signal<Option<(ChannelId, u64)>>,
  bottom_settle_anchor: Signal<Option<(ChannelId, u64)>>,
) {
  if newest_message_id == 0 {
    bottom_anchor.set(None);
    bottom_settle_anchor.set(None);
    return;
  }

  if bottom_settle_anchor.get_untracked() == Some((channel_id, newest_message_id)) {
    if chat_scroll_is_at_bottom(&scroll_state) {
      bottom_settle_anchor.set(None);
    } else {
      scroll_state.scroll_to_bottom_pending();
      return;
    }
  }

  let anchor = (channel_id, newest_message_id);
  if bottom_anchor.get_untracked() == Some(anchor) {
    return;
  }

  let previous_anchor = bottom_anchor.get_untracked();
  let should_scroll_to_bottom =
    previous_anchor.is_none() || previous_anchor.is_some_and(|(anchor_channel_id, _)| anchor_channel_id != channel_id);
  bottom_anchor.set(Some(anchor));

  if should_scroll_to_bottom || force_bottom {
    bottom_settle_anchor.set(Some(anchor));
    scroll_state.scroll_to_bottom_pending();
  } else {
    scroll_state.stick_to_bottom_if_near_end(64.0);
  }
}

fn chat_scroll_is_at_bottom(scroll_state: &ScrollState) -> bool {
  let viewport_height = scroll_state.viewport_height();
  let content_height = scroll_state.content_height();
  if viewport_height <= 0.0 || content_height <= viewport_height {
    return false;
  }

  let max_scroll_y = content_height - viewport_height;
  max_scroll_y - scroll_state.scroll_y() <= 2.0
}

pub(super) fn preserve_chat_scroll_on_prepend(
  channel_id: ChannelId,
  oldest_message_id: u64,
  scroll_state: ScrollState,
  top_anchor: Signal<Option<(ChannelId, u64)>>,
) {
  if oldest_message_id == 0 {
    return;
  }

  if let Some((anchor_channel_id, previous_oldest_message_id)) = top_anchor.get_untracked()
    && anchor_channel_id == channel_id
    && oldest_message_id < previous_oldest_message_id
  {
    scroll_state.preserve_prepend_anchor_pending();
  }

  top_anchor.set(Some((channel_id, oldest_message_id)));
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
