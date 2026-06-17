use std::sync::{
  Arc,
  atomic::{AtomicUsize, Ordering},
};

use super::*;

#[derive(Clone)]
struct TestSession {
  set_true_calls: Arc<AtomicUsize>,
  set_false_calls: Arc<AtomicUsize>,
}

impl SpeakingSession for TestSession {
  fn set_user_speaking(&self, _user_id: UserId, speaking: bool) {
    if speaking {
      self.set_true_calls.fetch_add(1, Ordering::Relaxed);
    } else {
      self.set_false_calls.fetch_add(1, Ordering::Relaxed);
    }
  }
}

#[test]
fn repeated_marks_do_not_emit_repeated_speaking_updates() {
  let tracker = Arc::new(SpeakingTracker::new());
  let set_true_calls = Arc::new(AtomicUsize::new(0));
  let set_false_calls = Arc::new(AtomicUsize::new(0));
  let session = TestSession {
    set_true_calls: set_true_calls.clone(),
    set_false_calls,
  };

  tracker.mark_user_speaking(session.clone(), 7);
  tracker.mark_user_speaking(session, 7);

  assert_eq!(set_true_calls.load(Ordering::Relaxed), 1);
}

#[test]
fn explicit_activity_combines_with_packet_activity() {
  let tracker = Arc::new(SpeakingTracker::new());
  let set_true_calls = Arc::new(AtomicUsize::new(0));
  let set_false_calls = Arc::new(AtomicUsize::new(0));
  let session = TestSession {
    set_true_calls: set_true_calls.clone(),
    set_false_calls: set_false_calls.clone(),
  };

  let token = tracker.start_user_speaking_activity(session.clone(), 7);
  tracker.mark_user_speaking(session.clone(), 7);
  tracker.stop_user_speaking_activity(session, 7, token);

  assert_eq!(set_true_calls.load(Ordering::Relaxed), 1);
  assert_eq!(set_false_calls.load(Ordering::Relaxed), 0);
}

#[test]
fn explicit_activity_clears_when_no_packet_activity_exists() {
  let tracker = Arc::new(SpeakingTracker::new());
  let set_true_calls = Arc::new(AtomicUsize::new(0));
  let set_false_calls = Arc::new(AtomicUsize::new(0));
  let session = TestSession {
    set_true_calls: set_true_calls.clone(),
    set_false_calls: set_false_calls.clone(),
  };

  let token = tracker.start_user_speaking_activity(session.clone(), 7);
  tracker.stop_user_speaking_activity(session, 7, token);

  assert_eq!(set_true_calls.load(Ordering::Relaxed), 1);
  assert_eq!(set_false_calls.load(Ordering::Relaxed), 1);
}

#[test]
fn stale_activity_stop_does_not_clear_new_activity() {
  let tracker = Arc::new(SpeakingTracker::new());
  let set_true_calls = Arc::new(AtomicUsize::new(0));
  let set_false_calls = Arc::new(AtomicUsize::new(0));
  let session = TestSession {
    set_true_calls: set_true_calls.clone(),
    set_false_calls: set_false_calls.clone(),
  };

  let stale_token = tracker.start_user_speaking_activity(session.clone(), 7);
  tracker.forget_user(7);
  let active_token = tracker.start_user_speaking_activity(session.clone(), 7);
  tracker.stop_user_speaking_activity(session, 7, stale_token);

  assert_eq!(set_true_calls.load(Ordering::Relaxed), 2);
  assert_eq!(set_false_calls.load(Ordering::Relaxed), 0);

  let session = TestSession {
    set_true_calls,
    set_false_calls: set_false_calls.clone(),
  };
  tracker.stop_user_speaking_activity(session, 7, active_token);
  assert_eq!(set_false_calls.load(Ordering::Relaxed), 1);
}
