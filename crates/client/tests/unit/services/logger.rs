use std::time::{SystemTime, UNIX_EPOCH};

use super::*;

#[test]
fn startup_log_file_arg_supports_equals_form() {
  assert_eq!(
    startup_log_file_arg([std::ffi::OsString::from("-log_file=parties.log")]),
    Some(PathBuf::from("parties.log"))
  );
}

#[test]
fn startup_log_file_arg_supports_separate_value_form() {
  assert_eq!(
    startup_log_file_arg([
      std::ffi::OsString::from("--log_file"),
      std::ffi::OsString::from("parties.log")
    ]),
    Some(PathBuf::from("parties.log"))
  );
}

#[test]
fn startup_log_file_arg_ignores_empty_equals_form() {
  assert_eq!(startup_log_file_arg([std::ffi::OsString::from("--log_file=")]), None);
}

#[test]
fn startup_log_level_arg_supports_equals_form() {
  assert_eq!(
    startup_log_level_arg([std::ffi::OsString::from("--log-level=debug")]),
    Some("debug".to_owned())
  );
}

#[test]
fn startup_log_level_arg_supports_separate_value_form() {
  assert_eq!(
    startup_log_level_arg([
      std::ffi::OsString::from("--log_level"),
      std::ffi::OsString::from("warn")
    ]),
    Some("warn".to_owned())
  );
}

#[test]
fn startup_log_level_arg_ignores_invalid_value() {
  assert_eq!(
    startup_log_level_arg([std::ffi::OsString::from("--log-level=nope")]),
    None
  );
}

#[test]
fn startup_log_filter_arg_supports_equals_form() {
  assert_eq!(
    startup_log_filter_arg([std::ffi::OsString::from("--log-filter=video=debug,network=warn")]),
    Some("video=debug,network=warn".to_owned())
  );
}

#[test]
fn startup_log_filter_arg_supports_separate_value_form() {
  assert_eq!(
    startup_log_filter_arg([
      std::ffi::OsString::from("--log_filter"),
      std::ffi::OsString::from("audio=off,voice=trace")
    ]),
    Some("audio=off,voice=trace".to_owned())
  );
}

#[test]
fn startup_log_domain_arg_supports_equals_form() {
  assert_eq!(
    startup_log_domain_arg([std::ffi::OsString::from("--log-domain=video")]),
    log_domain_filter("video")
  );
}

#[test]
fn startup_log_domain_arg_supports_separate_value_form() {
  assert_eq!(
    startup_log_domain_arg([
      std::ffi::OsString::from("--log_domain"),
      std::ffi::OsString::from("network")
    ]),
    log_domain_filter("network")
  );
}

#[test]
fn startup_log_domain_arg_ignores_unknown_domain() {
  assert_eq!(
    startup_log_domain_arg([std::ffi::OsString::from("--log-domain=nope")]),
    None
  );
}

#[test]
fn normalize_log_filter_aliases_supports_media_categories() {
  assert_eq!(
    normalize_log_filter_aliases("video:decode=debug,audio:encode=trace"),
    "video::decode=debug,audio::encode=trace"
  );
  assert_eq!(
    normalize_log_filter_aliases("video-decode[frame]=debug,audio-decode=info"),
    "video::decode[frame]=debug,audio::decode=info"
  );
}

#[test]
fn suppress_noisy_log_targets_adds_default_wgpu_suppression() {
  assert_eq!(
    suppress_noisy_log_targets("debug"),
    "debug,wgpu=warn,wgpu_core=warn,wgpu_hal=warn,naga=warn,profile=warn"
  );
}

#[test]
fn suppress_noisy_log_targets_keeps_explicit_wgpu_directive() {
  assert_eq!(
    suppress_noisy_log_targets("debug,wgpu=debug,wgpu_core::device=trace,profile=info"),
    "debug,wgpu=debug,wgpu_core::device=trace,profile=info,wgpu_hal=warn,naga=warn"
  );
}

#[test]
fn sentry_dsn_arg_supports_equals_and_separate_value_forms() {
  assert_eq!(
    sentry_dsn_arg([std::ffi::OsString::from("--sentry-dsn=https://public@example.com/1")]),
    Some("https://public@example.com/1".to_owned())
  );
  assert_eq!(
    sentry_dsn_arg([
      std::ffi::OsString::from("--sentry_dsn"),
      std::ffi::OsString::from("https://public@example.com/2")
    ]),
    Some("https://public@example.com/2".to_owned())
  );
}

#[test]
fn sentry_disabled_arg_supports_no_sentry_and_false_values() {
  assert!(sentry_disabled_arg([std::ffi::OsString::from("--no-sentry")]));
  assert!(sentry_disabled_arg([std::ffi::OsString::from("--sentry=false")]));
  assert!(!sentry_disabled_arg([std::ffi::OsString::from("--sentry=true")]));
}

#[test]
fn scrub_sensitive_text_redacts_inline_key_values() {
  assert_eq!(
    scrub_sensitive_text("connect address=127.0.0.1 certificate_fingerprint=abc123 frame=7"),
    "connect address=[redacted] certificate_fingerprint=[redacted] frame=7"
  );
}

#[test]
fn sentry_filter_drops_noisy_dx12_allocator_reset_render_errors() {
  let event = sentry::protocol::Event {
      message: Some(
        r#"failed to render native dx12 frame: Error { code: HRESULT(0x80004005), message: "reset dx12 command allocator: Unspecified error" }"#.to_owned(),
      ),
      ..Default::default()
    };

  assert!(is_noisy_dx12_render_event(&event));
}

#[test]
fn sentry_filter_drops_noisy_dx12_success_hresult_render_errors() {
  let event = sentry::protocol::Event {
      message: Some(
        r#"failed to render native dx12 frame: Error { code: HRESULT(0x00000000), message: "The operation completed successfully." }"#.to_owned(),
      ),
      ..Default::default()
    };

  assert!(is_noisy_dx12_render_event(&event));
}

#[test]
fn sentry_filter_drops_noisy_dx12_image_draw_fallbacks() {
  let event = sentry::protocol::Event {
    logentry: Some(sentry::protocol::LogEntry {
      message: "render error".to_owned(),
      params: vec![serde_json::json!(
        "skipping dx12 image draws after ERROR_MOD_NOT_FOUND; continuing frame so non-image UI can render"
      )],
    }),
    ..Default::default()
  };

  assert!(is_noisy_dx12_render_event(&event));
}

#[test]
fn scrub_value_redacts_sensitive_object_fields() {
  let mut value = serde_json::json!({
    "address": "127.0.0.1",
    "frame": 7,
    "nested": {
      "token": "secret-token"
    }
  });
  scrub_value(&mut value);
  assert_eq!(value["address"], "[redacted]");
  assert_eq!(value["frame"], 7);
  assert_eq!(value["nested"]["token"], "[redacted]");
}

#[test]
fn default_log_file_name_filter_matches_only_app_logs() {
  assert!(is_default_log_file_name(std::ffi::OsStr::new("parties_rs_123.log")));
  assert!(!is_default_log_file_name(std::ffi::OsStr::new("parties_rs_.txt")));
  assert!(!is_default_log_file_name(std::ffi::OsStr::new(
    "other_parties_rs_123.log"
  )));
  assert!(!is_default_log_file_name(std::ffi::OsStr::new("parties.log")));
}

#[test]
fn cleanup_old_default_log_files_removes_only_sibling_app_logs() {
  let dir = unique_test_dir("logger_cleanup");
  fs::create_dir_all(&dir).expect("create test log dir");

  let current = dir.join("parties_rs_current.log");
  let old = dir.join("parties_rs_old.log");
  let unrelated = dir.join("parties.log");
  fs::write(&current, "current").expect("write current log");
  fs::write(&old, "old").expect("write old log");
  fs::write(&unrelated, "keep").expect("write unrelated log");

  cleanup_old_default_log_files(&current);

  assert!(current.exists());
  assert!(!old.exists());
  assert!(unrelated.exists());

  fs::remove_dir_all(dir).expect("remove test log dir");
}

fn unique_test_dir(name: &str) -> PathBuf {
  let nanos = SystemTime::now()
    .duration_since(UNIX_EPOCH)
    .expect("system time before unix epoch")
    .as_nanos();
  env::temp_dir().join(format!("{name}_{}_{}", std::process::id(), nanos))
}
