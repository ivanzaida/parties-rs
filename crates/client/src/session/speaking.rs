use std::{
  collections::{HashMap, HashSet},
  sync::Arc,
  thread,
  time::Duration,
};

use parking_lot::Mutex;

use crate::network::protocol::UserId;

pub(super) trait SpeakingSession: Clone + Send + 'static {
  fn set_user_speaking(&self, user_id: UserId, speaking: bool);
}

pub(super) struct SpeakingTracker {
  marks: Mutex<HashMap<UserId, u64>>,
  counter: Mutex<u64>,
  clear_scheduled: Mutex<HashSet<UserId>>,
  active_sources: Mutex<HashMap<UserId, HashSet<u64>>>,
}

impl SpeakingTracker {
  pub(super) fn new() -> Self {
    Self {
      marks: Mutex::new(HashMap::new()),
      counter: Mutex::new(0),
      clear_scheduled: Mutex::new(HashSet::new()),
      active_sources: Mutex::new(HashMap::new()),
    }
  }

  pub(super) fn clear_all(&self) {
    self.marks.lock().clear();
    self.clear_scheduled.lock().clear();
    self.active_sources.lock().clear();
  }

  pub(super) fn forget_user(&self, user_id: UserId) {
    self.marks.lock().remove(&user_id);
    self.clear_scheduled.lock().remove(&user_id);
    self.active_sources.lock().remove(&user_id);
  }

  pub(super) fn mark_user_speaking<S>(self: &Arc<Self>, session: S, user_id: UserId)
  where
    S: SpeakingSession,
  {
    let mark = {
      let mut counter = self.counter.lock();
      *counter = counter.wrapping_add(1);
      let mark = *counter;
      let mut marks = self.marks.lock();
      marks.insert(user_id, mark);
      mark
    };

    let should_schedule = {
      let mut clear_scheduled = self.clear_scheduled.lock();
      clear_scheduled.insert(user_id)
    };
    if should_schedule && !self.has_active_source(user_id) {
      session.set_user_speaking(user_id, true);
    }

    if should_schedule {
      let tracker = self.clone();
      thread::spawn(move || {
        tracker.clear_user_speaking_after_idle(session, user_id, mark);
      });
    }
  }

  pub(super) fn start_user_speaking_activity<S>(&self, session: S, user_id: UserId) -> u64
  where
    S: SpeakingSession,
  {
    let token = self.next_mark();
    let should_set_speaking = {
      let mut active_sources = self.active_sources.lock();
      let sources = active_sources.entry(user_id).or_default();
      let was_inactive = sources.is_empty();
      sources.insert(token);
      was_inactive && !self.has_packet_activity(user_id)
    };
    if should_set_speaking {
      session.set_user_speaking(user_id, true);
    }
    token
  }

  pub(super) fn stop_user_speaking_activity<S>(&self, session: S, user_id: UserId, token: u64)
  where
    S: SpeakingSession,
  {
    let should_clear_speaking = {
      let mut active_sources = self.active_sources.lock();
      let Some(sources) = active_sources.get_mut(&user_id) else {
        return;
      };
      if !sources.remove(&token) {
        return;
      }
      let stopped_last_source = sources.is_empty();
      if stopped_last_source {
        active_sources.remove(&user_id);
      }
      stopped_last_source && !self.has_packet_activity(user_id)
    };
    if should_clear_speaking {
      session.set_user_speaking(user_id, false);
    }
  }

  pub(super) fn clear_user_speaking<S>(&self, session: S, user_id: UserId)
  where
    S: SpeakingSession,
  {
    self.forget_user(user_id);
    session.set_user_speaking(user_id, false);
  }

  fn clear_user_speaking_after_idle<S>(&self, session: S, user_id: UserId, mut observed_mark: u64)
  where
    S: SpeakingSession,
  {
    loop {
      thread::sleep(Duration::from_millis(850));

      let mut marks = self.marks.lock();
      match marks.get(&user_id).copied() {
        Some(current_mark) if current_mark == observed_mark => {
          marks.remove(&user_id);
          self.clear_scheduled.lock().remove(&user_id);
          drop(marks);
          if !self.has_active_source(user_id) {
            session.set_user_speaking(user_id, false);
          }
          return;
        }
        Some(current_mark) => {
          observed_mark = current_mark;
        }
        None => {
          self.clear_scheduled.lock().remove(&user_id);
          return;
        }
      }
    }
  }

  fn next_mark(&self) -> u64 {
    let mut counter = self.counter.lock();
    *counter = counter.wrapping_add(1);
    *counter
  }

  fn has_packet_activity(&self, user_id: UserId) -> bool {
    self.clear_scheduled.lock().contains(&user_id)
  }

  fn has_active_source(&self, user_id: UserId) -> bool {
    self
      .active_sources
      .lock()
      .get(&user_id)
      .is_some_and(|sources| !sources.is_empty())
  }
}

impl Default for SpeakingTracker {
  fn default() -> Self {
    Self::new()
  }
}

#[cfg(test)]
mod tests {
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
}
