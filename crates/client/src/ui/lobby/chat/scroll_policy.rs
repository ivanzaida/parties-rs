pub type BottomAnchor = (u32, u64);
pub type BottomSettleAnchor = (u32, u64, f32);

const BOTTOM_EPSILON: f32 = 2.0;
const STICKY_BOTTOM_THRESHOLD: f32 = 64.0;

#[derive(Clone, Copy, Debug)]
pub struct BottomScrollMetrics {
  pub scroll_y: f32,
  pub viewport_height: f32,
  pub content_height: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BottomScrollPlan {
  pub bottom_anchor: Option<BottomAnchor>,
  pub bottom_settle_anchor: Option<BottomSettleAnchor>,
  pub bottom_detached_anchor: Option<BottomAnchor>,
  pub scroll_to_bottom_pending: bool,
}

pub fn plan_bottom_scroll(
  channel_id: u32,
  newest_message_id: u64,
  force_bottom: bool,
  bottom_anchor: Option<BottomAnchor>,
  bottom_settle_anchor: Option<BottomSettleAnchor>,
  bottom_detached_anchor: Option<BottomAnchor>,
  metrics: BottomScrollMetrics,
) -> BottomScrollPlan {
  if newest_message_id == 0 {
    return BottomScrollPlan {
      bottom_anchor: None,
      bottom_settle_anchor: None,
      bottom_detached_anchor: None,
      scroll_to_bottom_pending: false,
    };
  }

  if let Some((settle_channel_id, settle_message_id, settle_content_height)) = bottom_settle_anchor
    && settle_channel_id == channel_id
    && settle_message_id == newest_message_id
  {
    if chat_scroll_is_at_bottom(metrics) && (metrics.content_height - settle_content_height).abs() <= 0.5 {
      return BottomScrollPlan {
        bottom_anchor,
        bottom_settle_anchor: None,
        bottom_detached_anchor,
        scroll_to_bottom_pending: false,
      };
    }

    return BottomScrollPlan {
      bottom_anchor,
      bottom_settle_anchor: Some((channel_id, newest_message_id, metrics.content_height)),
      bottom_detached_anchor,
      scroll_to_bottom_pending: true,
    };
  }

  let anchor = (channel_id, newest_message_id);
  if bottom_detached_anchor == Some(anchor) && !force_bottom {
    if chat_scroll_is_at_bottom(metrics) {
      return BottomScrollPlan {
        bottom_anchor,
        bottom_settle_anchor: Some((anchor.0, anchor.1, metrics.content_height)),
        bottom_detached_anchor: None,
        scroll_to_bottom_pending: true,
      };
    }

    return BottomScrollPlan {
      bottom_anchor,
      bottom_settle_anchor: None,
      bottom_detached_anchor: Some(anchor),
      scroll_to_bottom_pending: false,
    };
  }

  if bottom_anchor == Some(anchor) {
    let should_stick = chat_scroll_is_near_bottom(metrics, STICKY_BOTTOM_THRESHOLD);
    return BottomScrollPlan {
      bottom_anchor,
      bottom_settle_anchor: should_stick.then_some((anchor.0, anchor.1, metrics.content_height)),
      bottom_detached_anchor,
      scroll_to_bottom_pending: should_stick,
    };
  }

  let should_scroll_to_bottom =
    bottom_anchor.is_none() || bottom_anchor.is_some_and(|(anchor_channel_id, _)| anchor_channel_id != channel_id);

  if should_scroll_to_bottom || force_bottom {
    BottomScrollPlan {
      bottom_anchor: Some(anchor),
      bottom_settle_anchor: Some((anchor.0, anchor.1, metrics.content_height)),
      bottom_detached_anchor: None,
      scroll_to_bottom_pending: true,
    }
  } else {
    let next_detached_anchor =
      bottom_detached_anchor.and_then(|(detached_channel_id, _)| (detached_channel_id == channel_id).then_some(anchor));
    BottomScrollPlan {
      bottom_anchor: Some(anchor),
      bottom_settle_anchor: None,
      bottom_detached_anchor: next_detached_anchor,
      scroll_to_bottom_pending: next_detached_anchor.is_none()
        && chat_scroll_is_near_bottom(metrics, STICKY_BOTTOM_THRESHOLD),
    }
  }
}

fn chat_scroll_is_at_bottom(metrics: BottomScrollMetrics) -> bool {
  if metrics.viewport_height <= 0.0 || metrics.content_height <= metrics.viewport_height {
    return false;
  }

  let max_scroll_y = metrics.content_height - metrics.viewport_height;
  max_scroll_y - metrics.scroll_y <= BOTTOM_EPSILON
}

fn chat_scroll_is_near_bottom(metrics: BottomScrollMetrics, threshold: f32) -> bool {
  if metrics.viewport_height <= 0.0 || metrics.content_height <= metrics.viewport_height {
    return false;
  }

  let max_scroll_y = metrics.content_height - metrics.viewport_height;
  max_scroll_y - metrics.scroll_y <= threshold.max(0.0)
}
