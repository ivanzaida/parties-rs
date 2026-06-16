#[path = "../../src/ui/lobby/chat/scroll_policy.rs"]
mod scroll_policy;

use scroll_policy::{BottomScrollMetrics, plan_bottom_scroll};

#[test]
fn anchored_bottom_stays_pending_when_user_reaches_bottom_before_height_settles() {
  let plan = plan_bottom_scroll(
    1,
    490,
    false,
    Some((1, 490)),
    None,
    None,
    BottomScrollMetrics {
      scroll_y: 900.0,
      viewport_height: 100.0,
      content_height: 1000.0,
    },
  );

  assert!(
    plan.scroll_to_bottom_pending,
    "same-message anchor should keep sticky bottom while virtual-list measurements can still grow content"
  );
  assert_eq!(plan.bottom_settle_anchor, Some((1, 490, 1000.0)));
}

#[test]
fn bottom_settle_waits_for_stable_content_height() {
  let plan = plan_bottom_scroll(
    1,
    490,
    false,
    Some((1, 490)),
    Some((1, 490, 800.0)),
    None,
    BottomScrollMetrics {
      scroll_y: 900.0,
      viewport_height: 100.0,
      content_height: 1000.0,
    },
  );

  assert!(plan.scroll_to_bottom_pending);
  assert_eq!(plan.bottom_settle_anchor, Some((1, 490, 1000.0)));
}

#[test]
fn user_detached_anchor_does_not_stick_back_to_bottom() {
  let plan = plan_bottom_scroll(
    1,
    490,
    false,
    Some((1, 490)),
    None,
    Some((1, 490)),
    BottomScrollMetrics {
      scroll_y: 850.0,
      viewport_height: 100.0,
      content_height: 1000.0,
    },
  );

  assert!(
    !plan.scroll_to_bottom_pending,
    "user scroll away should not be pulled back to bottom for the same newest message"
  );
  assert_eq!(plan.bottom_detached_anchor, Some((1, 490)));
}

#[test]
fn user_detached_anchor_reattaches_after_reaching_bottom() {
  let plan = plan_bottom_scroll(
    1,
    490,
    false,
    Some((1, 490)),
    None,
    Some((1, 490)),
    BottomScrollMetrics {
      scroll_y: 900.0,
      viewport_height: 100.0,
      content_height: 1000.0,
    },
  );

  assert!(plan.scroll_to_bottom_pending);
  assert_eq!(plan.bottom_detached_anchor, None);
  assert_eq!(plan.bottom_settle_anchor, Some((1, 490, 1000.0)));
}
