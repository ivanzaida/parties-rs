use super::*;

#[test]
fn receiver_lifecycle_allows_only_one_running_receiver() {
  let runtime = ConnectionRuntime::new();
  let stop = Arc::new(AtomicBool::new(false));

  assert!(runtime.try_begin_receiver());
  assert!(!runtime.try_begin_receiver());

  runtime.set_receiver_stop(Some(stop.clone()));
  runtime.stop_receivers();
  assert!(stop.load(Ordering::Relaxed));

  runtime.finish_receiver();
  assert!(runtime.try_begin_receiver());
}

#[test]
fn request_shutdown_stops_receivers_and_clear_resets_shutdown_and_tofu() {
  let runtime = ConnectionRuntime::new();
  let stop = Arc::new(AtomicBool::new(false));

  runtime.set_receiver_stop(Some(stop.clone()));
  runtime.set_tofu_warning(TofuWarning {
    address: "example.test:7800".to_owned(),
    server_name: "Example".to_owned(),
    user_id: 7,
    role: Role::User,
    saved_fingerprint: "old".to_owned(),
    received_fingerprint: "new".to_owned(),
    server_password: "secret".to_owned(),
    display_name: "local".to_owned(),
  });

  runtime.request_shutdown();
  assert!(runtime.shutdown_requested());
  assert!(stop.load(Ordering::Relaxed));
  assert!(runtime.tofu_warning().is_some());

  runtime.clear();
  assert!(!runtime.shutdown_requested());
  assert!(runtime.tofu_warning().is_none());
  assert!(runtime.info().is_none());
}

#[test]
fn pending_keepalive_timeout_requires_existing_expired_ping() {
  let runtime = ConnectionRuntime::new();
  let started_at = Instant::now();
  let timeout = Duration::from_secs(8);

  assert!(!runtime.pending_keepalive_timed_out(started_at, timeout));
  assert_eq!(runtime.pending_ping_age_ms(started_at), Some(0));
  assert!(!runtime.pending_keepalive_timed_out(started_at + Duration::from_secs(7), timeout));
  assert!(runtime.pending_keepalive_timed_out(started_at + timeout, timeout));

  assert!(runtime.take_pending_keepalive_ping().is_some());
  assert!(!runtime.pending_keepalive_timed_out(started_at + Duration::from_secs(9), timeout));
}
