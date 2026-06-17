use std::sync::{
  Arc,
  atomic::{AtomicBool, AtomicUsize, Ordering},
};

use super::*;

#[derive(Clone, Default)]
struct TestStreamWatchSession {
  target_available: Arc<AtomicBool>,
  set_watching_calls: Arc<Mutex<Vec<Option<UserId>>>>,
  ensure_audio_calls: Arc<AtomicUsize>,
}

impl StreamWatchSession for TestStreamWatchSession {
  fn server(&self) -> Option<Arc<Server>> {
    None
  }

  fn reconnect_watch_target_available(&self, _user_id: UserId) -> bool {
    self.target_available.load(Ordering::Relaxed)
  }

  fn set_watching_user(&self, user_id: Option<UserId>) {
    self.set_watching_calls.lock().push(user_id);
  }

  fn ensure_stream_audio_playback(&self, _settings: AppSettings) -> Result<(), String> {
    self.ensure_audio_calls.fetch_add(1, Ordering::Relaxed);
    Ok(())
  }
}

fn block_on_restore(runtime: &StreamRuntime, session: TestStreamWatchSession, timeout: Duration) {
  tokio::runtime::Builder::new_current_thread()
    .enable_time()
    .build()
    .unwrap()
    .block_on(runtime.restore_pending_reconnect_watch(session, AppSettings::default(), timeout));
}

#[test]
fn pending_reconnect_watch_only_takes_matching_user() {
  let runtime = StreamRuntime::new();

  runtime.set_pending_reconnect_watch(Some(7));

  assert!(runtime.has_pending_reconnect_watch());
  assert!(runtime.pending_reconnect_watch_matches(7));
  assert!(!runtime.pending_reconnect_watch_matches(8));
  assert!(!runtime.take_pending_reconnect_watch_if_matches(8));
  assert_eq!(runtime.pending_reconnect_watch_user_id(), Some(7));
  assert!(runtime.take_pending_reconnect_watch_if_matches(7));
  assert!(!runtime.has_pending_reconnect_watch());
}

#[test]
fn pending_reconnect_watch_timeout_clears_stale_target() {
  let runtime = StreamRuntime::new();
  let session = TestStreamWatchSession::default();

  runtime.set_pending_reconnect_watch(Some(7));
  block_on_restore(&runtime, session.clone(), Duration::ZERO);

  assert!(!runtime.has_pending_reconnect_watch());
  assert!(session.set_watching_calls.lock().is_empty());
  assert_eq!(session.ensure_audio_calls.load(Ordering::Relaxed), 0);
}

#[test]
fn pending_reconnect_watch_without_server_clears_pending_without_selecting_stream() {
  let runtime = StreamRuntime::new();
  let session = TestStreamWatchSession::default();
  session.target_available.store(true, Ordering::Relaxed);

  runtime.set_pending_reconnect_watch(Some(7));
  block_on_restore(&runtime, session.clone(), Duration::from_secs(1));

  assert!(!runtime.has_pending_reconnect_watch());
  assert!(session.set_watching_calls.lock().is_empty());
  assert_eq!(session.ensure_audio_calls.load(Ordering::Relaxed), 0);
}
