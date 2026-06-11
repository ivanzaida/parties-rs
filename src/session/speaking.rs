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
}

impl SpeakingTracker {
  pub(super) fn new() -> Self {
    Self {
      marks: Mutex::new(HashMap::new()),
      counter: Mutex::new(0),
      clear_scheduled: Mutex::new(HashSet::new()),
    }
  }

  pub(super) fn clear_all(&self) {
    self.marks.lock().clear();
    self.clear_scheduled.lock().clear();
  }

  pub(super) fn forget_user(&self, user_id: UserId) {
    self.marks.lock().remove(&user_id);
    self.clear_scheduled.lock().remove(&user_id);
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

    session.set_user_speaking(user_id, true);

    let should_schedule = self.clear_scheduled.lock().insert(user_id);
    if should_schedule {
      let tracker = self.clone();
      thread::spawn(move || {
        tracker.clear_user_speaking_after_idle(session, user_id, mark);
      });
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
          session.set_user_speaking(user_id, false);
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
}

impl Default for SpeakingTracker {
  fn default() -> Self {
    Self::new()
  }
}
