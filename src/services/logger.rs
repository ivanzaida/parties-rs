use std::{
  borrow::Cow,
  collections::HashSet,
  env,
  fs::{self, OpenOptions},
  io,
  path::{Path, PathBuf},
  sync::{Arc, LazyLock, Once, OnceLock, RwLock},
  time::Duration,
};

use sentry::protocol::Value;
use tracing_subscriber::{EnvFilter, fmt::MakeWriter, prelude::*};

const DEFAULT_LOG_FILE_PREFIX: &str = "parties_rs_";
const DEFAULT_LOG_FILE_SUFFIX: &str = ".log";
const DEFAULT_SENTRY_DSN: &str = "https://26d5cb6b996d927b87a439d87ba63ec9@sentry.lurq.dev/2";

static SEEN_MSGS: LazyLock<RwLock<HashSet<String>>> = LazyLock::new(|| RwLock::new(HashSet::new()));
static LOGGER_INIT: Once = Once::new();
static LOG_GUARD: OnceLock<tracing_appender::non_blocking::WorkerGuard> = OnceLock::new();
static SENTRY_GUARD: OnceLock<sentry::ClientInitGuard> = OnceLock::new();

macro_rules! emit_once {
  ($level:expr, $target:literal, $msg:expr) => {
    match $level {
      tracing::Level::ERROR => tracing::error!(target: $target, "{}", $msg),
      tracing::Level::WARN => tracing::warn!(target: $target, "{}", $msg),
      tracing::Level::INFO => tracing::info!(target: $target, "{}", $msg),
      tracing::Level::DEBUG => tracing::debug!(target: $target, "{}", $msg),
      tracing::Level::TRACE => tracing::trace!(target: $target, "{}", $msg),
    }
  };
}

#[macro_export]
macro_rules! log_once {
  () => {
    $crate::services::logger::log_once(tracing::Level::INFO, "app", "")
  };
  (error, target: $target:expr, $($arg:tt)*) => {{
    $crate::services::logger::log_once(tracing::Level::ERROR, $target, &format!($($arg)*));
  }};
  (error, $($arg:tt)*) => {{
    $crate::services::logger::log_once(tracing::Level::ERROR, "app", &format!($($arg)*));
  }};
  (warn, target: $target:expr, $($arg:tt)*) => {{
    $crate::services::logger::log_once(tracing::Level::WARN, $target, &format!($($arg)*));
  }};
  (warn, $($arg:tt)*) => {{
    $crate::services::logger::log_once(tracing::Level::WARN, "app", &format!($($arg)*));
  }};
  (info, target: $target:expr, $($arg:tt)*) => {{
    $crate::services::logger::log_once(tracing::Level::INFO, $target, &format!($($arg)*));
  }};
  (info, $($arg:tt)*) => {{
    $crate::services::logger::log_once(tracing::Level::INFO, "app", &format!($($arg)*));
  }};
  (debug, target: $target:expr, $($arg:tt)*) => {{
    $crate::services::logger::log_once(tracing::Level::DEBUG, $target, &format!($($arg)*));
  }};
  (debug, $($arg:tt)*) => {{
    $crate::services::logger::log_once(tracing::Level::DEBUG, "app", &format!($($arg)*));
  }};
  (trace, target: $target:expr, $($arg:tt)*) => {{
    $crate::services::logger::log_once(tracing::Level::TRACE, $target, &format!($($arg)*));
  }};
  (trace, $($arg:tt)*) => {{
    $crate::services::logger::log_once(tracing::Level::TRACE, "app", &format!($($arg)*));
  }};
  ($($arg:tt)*) => {{
    $crate::services::logger::log_once(tracing::Level::INFO, "app", &format!($($arg)*));
  }};
}

pub fn log_once(level: tracing::Level, target: &'static str, msg: &str) {
  {
    let read = SEEN_MSGS.read().expect("logger lock poisoned");
    if read.contains(msg) {
      return;
    }
  }

  let mut write = SEEN_MSGS.write().expect("logger lock poisoned");
  if write.insert(msg.to_owned()) {
    match target {
      "stream::sources" => emit_once!(level, "stream::sources", msg),
      "stream::thumbnails" => emit_once!(level, "stream::thumbnails", msg),
      "ui::icons" => emit_once!(level, "ui::icons", msg),
      _ => emit_once!(level, "app", msg),
    }
  }
}

pub fn init() {
  LOGGER_INIT.call_once(|| {
    let args = env::args_os().skip(1).collect::<Vec<_>>();
    let log_filter_directive = startup_log_filter_directive(args.clone());
    let filter = log_filter_from_directive(&log_filter_directive);
    let sentry_enabled = init_sentry(&args, &log_filter_directive);
    let _ = tracing_log::LogTracer::init();
    let explicit_log_file = startup_log_file_arg(args.clone());
    let path = explicit_log_file.clone().or_else(default_log_file_path);
    if let Some(path) = path {
      if explicit_log_file.is_none() {
        cleanup_old_default_log_files(&path);
      }
      match open_log_file(&path) {
        Ok(file) => {
          let (writer, guard) = tracing_appender::non_blocking(file);
          let _ = LOG_GUARD.set(guard);
          if init_subscriber(filter.clone(), writer, sentry_enabled).is_ok() {
            return;
          }
        }
        Err(error) => {
          eprintln!("failed to open log file {}: {error}", path.display());
        }
      }
    }

    let _ = init_subscriber(filter, io::stderr, sentry_enabled);
  });
}

fn init_subscriber<W>(
  filter: EnvFilter,
  writer: W,
  sentry_enabled: bool,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
where
  W: for<'writer> MakeWriter<'writer> + Send + Sync + 'static,
{
  let fmt_layer = tracing_subscriber::fmt::layer()
    .with_writer(writer)
    .with_ansi(false)
    .with_target(false)
    .with_thread_ids(false)
    .with_thread_names(false)
    .with_timer(tracing_subscriber::fmt::time::UtcTime::rfc_3339());
  let sentry_layer = sentry_enabled.then(|| {
    sentry_tracing::layer()
      .event_filter(sentry_event_filter)
      .span_filter(sentry_span_filter)
  });

  Ok(
    tracing_subscriber::registry()
      .with(filter)
      .with(fmt_layer)
      .with(sentry_layer)
      .try_init()?,
  )
}

fn sentry_event_filter(metadata: &tracing::Metadata<'_>) -> sentry_tracing::EventFilter {
  if metadata.target() == "native::windows::seh" {
    return sentry_tracing::EventFilter::Breadcrumb;
  }

  match *metadata.level() {
    tracing::Level::ERROR => sentry_tracing::EventFilter::Event,
    tracing::Level::WARN | tracing::Level::INFO => sentry_tracing::EventFilter::Breadcrumb,
    tracing::Level::DEBUG | tracing::Level::TRACE => sentry_tracing::EventFilter::Ignore,
  }
}

pub fn flush_sentry(timeout: Duration) -> bool {
  SENTRY_GUARD.get().is_some_and(|guard| guard.flush(Some(timeout)))
}

fn sentry_span_filter(metadata: &tracing::Metadata<'_>) -> bool {
  matches!(
    *metadata.level(),
    tracing::Level::ERROR | tracing::Level::WARN | tracing::Level::INFO
  )
}

fn init_sentry(args: &[std::ffi::OsString], log_filter_directive: &str) -> bool {
  if sentry_disabled(args.iter().cloned()) {
    return false;
  }

  let dsn_text = sentry_dsn_arg(args.iter().cloned())
    .or_else(sentry_dsn_env)
    .unwrap_or_else(|| DEFAULT_SENTRY_DSN.to_owned());
  let dsn = match dsn_text.parse() {
    Ok(dsn) => dsn,
    Err(error) => {
      eprintln!("ignoring invalid Sentry DSN: {error}");
      return false;
    }
  };

  let guard = sentry::init(sentry::ClientOptions {
    dsn: Some(dsn),
    release: sentry_release().map(Cow::Owned),
    environment: sentry_environment().map(Cow::Owned),
    debug: sentry_debug_enabled(),
    send_default_pii: false,
    attach_stacktrace: true,
    traces_sample_rate: 0.0,
    before_send: Some(Arc::new(scrub_sentry_event)),
    ..Default::default()
  });
  let enabled = guard.is_enabled();
  if enabled {
    sentry::configure_scope(|scope| {
      scope.set_tag("app", env!("CARGO_PKG_NAME"));
      scope.set_tag("log_filter", log_filter_directive);
    });
  }
  let _ = SENTRY_GUARD.set(guard);
  enabled
}

fn sentry_disabled(args: impl IntoIterator<Item = std::ffi::OsString>) -> bool {
  sentry_disabled_arg(args)
    || env_flag("PARTIES_SENTRY_DISABLED")
    || env_flag("SENTRY_DISABLED")
    || env_false("PARTIES_SENTRY")
}

fn sentry_disabled_arg(args: impl IntoIterator<Item = std::ffi::OsString>) -> bool {
  let mut args = args.into_iter();

  while let Some(arg) = args.next() {
    let arg_text = arg.to_string_lossy();
    if arg_text == "--no-sentry" || arg_text == "-no-sentry" {
      return true;
    }
    if let Some(value) = arg_text
      .strip_prefix("--sentry=")
      .or_else(|| arg_text.strip_prefix("-sentry="))
    {
      return !truthy(value);
    }
    if arg_text == "--sentry" || arg_text == "-sentry" {
      if let Some(value) = args.next() {
        return !truthy(&value.to_string_lossy());
      }
    }
  }

  false
}

fn sentry_dsn_env() -> Option<String> {
  ["PARTIES_SENTRY_DSN", "SENTRY_DSN"]
    .into_iter()
    .find_map(|key| env::var(key).ok().filter(|value| !value.trim().is_empty()))
}

fn sentry_dsn_arg(args: impl IntoIterator<Item = std::ffi::OsString>) -> Option<String> {
  let mut args = args.into_iter();

  while let Some(arg) = args.next() {
    let arg_text = arg.to_string_lossy();
    for prefix in ["--sentry-dsn=", "-sentry-dsn=", "--sentry_dsn=", "-sentry_dsn="] {
      if let Some(dsn) = arg_text.strip_prefix(prefix).filter(|dsn| !dsn.trim().is_empty()) {
        return Some(dsn.to_owned());
      }
    }
    if arg_text == "--sentry-dsn"
      || arg_text == "-sentry-dsn"
      || arg_text == "--sentry_dsn"
      || arg_text == "-sentry_dsn"
    {
      return args.next().and_then(|dsn| {
        let dsn = dsn.to_string_lossy().trim().to_owned();
        (!dsn.is_empty()).then_some(dsn)
      });
    }
  }

  None
}

fn sentry_release() -> Option<String> {
  ["PARTIES_SENTRY_RELEASE", "SENTRY_RELEASE"]
    .into_iter()
    .find_map(|key| env::var(key).ok().filter(|value| !value.trim().is_empty()))
    .or_else(|| Some(format!("{}@{}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"))))
}

fn sentry_environment() -> Option<String> {
  [
    "PARTIES_SENTRY_ENVIRONMENT",
    "PARTIES_ENVIRONMENT",
    "SENTRY_ENVIRONMENT",
  ]
  .into_iter()
  .find_map(|key| env::var(key).ok().filter(|value| !value.trim().is_empty()))
  .or_else(|| {
    Some(if cfg!(debug_assertions) {
      "development".to_owned()
    } else {
      "production".to_owned()
    })
  })
}

fn sentry_debug_enabled() -> bool {
  env_flag("PARTIES_SENTRY_DEBUG") || env_flag("SENTRY_DEBUG") || cfg!(debug_assertions)
}

fn env_flag(key: &str) -> bool {
  env::var(key).is_ok_and(|value| truthy(&value))
}

fn env_false(key: &str) -> bool {
  env::var(key).is_ok_and(|value| falsy(&value))
}

fn truthy(value: &str) -> bool {
  matches!(
    value.trim().to_ascii_lowercase().as_str(),
    "1" | "true" | "yes" | "on" | "enabled" | "enable"
  )
}

fn falsy(value: &str) -> bool {
  matches!(
    value.trim().to_ascii_lowercase().as_str(),
    "0" | "false" | "no" | "off" | "disabled" | "disable"
  )
}

fn scrub_sentry_event(mut event: sentry::protocol::Event<'static>) -> Option<sentry::protocol::Event<'static>> {
  event.server_name = None;
  event.user = None;
  event.request = None;
  scrub_optional_text(&mut event.message);
  if let Some(logentry) = event.logentry.as_mut() {
    logentry.message = scrub_sensitive_text(&logentry.message);
    for param in &mut logentry.params {
      scrub_value(param);
    }
  }
  for breadcrumb in &mut event.breadcrumbs {
    scrub_optional_text(&mut breadcrumb.message);
    for value in breadcrumb.data.values_mut() {
      scrub_value(value);
    }
  }
  for value in event.extra.values_mut() {
    scrub_value(value);
  }
  Some(event)
}

fn scrub_optional_text(text: &mut Option<String>) {
  if let Some(text) = text {
    *text = scrub_sensitive_text(text);
  }
}

fn scrub_value(value: &mut Value) {
  match value {
    Value::String(text) => *text = scrub_sensitive_text(text),
    Value::Array(values) => {
      for value in values {
        scrub_value(value);
      }
    }
    Value::Object(values) => {
      for (key, value) in values {
        if sensitive_key(key) {
          *value = Value::String("[redacted]".to_owned());
        } else {
          scrub_value(value);
        }
      }
    }
    _ => {}
  }
}

fn scrub_sensitive_text(text: &str) -> String {
  text
    .split_whitespace()
    .map(redact_sensitive_token)
    .collect::<Vec<_>>()
    .join(" ")
}

fn redact_sensitive_token(token: &str) -> String {
  let Some((key, _)) = token.split_once('=') else {
    return token.to_owned();
  };
  let normalized_key = key.trim_matches(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_' && ch != '-');
  if sensitive_key(normalized_key) {
    format!("{key}=[redacted]")
  } else {
    token.to_owned()
  }
}

fn sensitive_key(key: &str) -> bool {
  let key = key.to_ascii_lowercase();
  key.contains("certificate")
    || key.contains("fingerprint")
    || key.contains("token")
    || key.contains("secret")
    || key.contains("password")
    || key.contains("dsn")
    || key == "address"
    || key == "server_address"
    || key == "username"
    || key == "display_name"
}

fn open_log_file(path: &Path) -> std::io::Result<std::fs::File> {
  if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
    fs::create_dir_all(parent)?;
  }

  OpenOptions::new().create(true).append(true).open(path)
}

#[cfg(not(debug_assertions))]
fn default_log_file_path() -> Option<PathBuf> {
  Some(PathBuf::from(format!(
    "{DEFAULT_LOG_FILE_PREFIX}{}{DEFAULT_LOG_FILE_SUFFIX}",
    std::process::id()
  )))
}

#[cfg(debug_assertions)]
fn default_log_file_path() -> Option<PathBuf> {
  None
}

fn cleanup_old_default_log_files(current_path: &Path) {
  let Some(current_file_name) = current_path.file_name() else {
    return;
  };
  let parent = current_path
    .parent()
    .filter(|parent| !parent.as_os_str().is_empty())
    .unwrap_or_else(|| Path::new("."));

  let Ok(entries) = fs::read_dir(parent) else {
    return;
  };

  for entry in entries.flatten() {
    let file_name = entry.file_name();
    if file_name == current_file_name || !is_default_log_file_name(&file_name) {
      continue;
    }

    let path = entry.path();
    if path.is_file() {
      let _ = fs::remove_file(path);
    }
  }
}

fn is_default_log_file_name(file_name: &std::ffi::OsStr) -> bool {
  let Some(file_name) = file_name.to_str() else {
    return false;
  };

  file_name.starts_with(DEFAULT_LOG_FILE_PREFIX) && file_name.ends_with(DEFAULT_LOG_FILE_SUFFIX)
}

fn startup_log_file_arg(args: impl IntoIterator<Item = std::ffi::OsString>) -> Option<PathBuf> {
  let mut args = args.into_iter();

  while let Some(arg) = args.next() {
    let arg_text = arg.to_string_lossy();

    for prefix in ["-log_file=", "--log_file="] {
      if let Some(path) = arg_text.strip_prefix(prefix).filter(|path| !path.is_empty()) {
        return Some(PathBuf::from(path));
      }
    }

    if arg_text == "-log_file" || arg_text == "--log_file" {
      return args.next().map(PathBuf::from);
    }
  }

  None
}

fn startup_log_filter_directive(args: impl IntoIterator<Item = std::ffi::OsString>) -> String {
  let args: Vec<_> = args.into_iter().collect();
  let directive = startup_log_filter_arg(args.clone())
    .or_else(startup_log_filter_env)
    .or_else(|| startup_log_domain_arg(args.clone()))
    .or_else(startup_log_domain_env)
    .or_else(|| startup_log_level_arg(args))
    .or_else(startup_log_level_env)
    .unwrap_or_else(|| "info".to_owned());

  normalize_log_filter_aliases(&directive)
}

fn log_filter_from_directive(directive: &str) -> EnvFilter {
  EnvFilter::try_new(directive).unwrap_or_else(|_| EnvFilter::new("info"))
}

fn startup_log_filter_env() -> Option<String> {
  ["PARTIES_LOG_FILTER", "PARTIES_LOG", "RUST_LOG"]
    .into_iter()
    .find_map(|key| env::var(key).ok().filter(|filter| !filter.trim().is_empty()))
}

fn startup_log_filter_arg(args: impl IntoIterator<Item = std::ffi::OsString>) -> Option<String> {
  let mut args = args.into_iter();

  while let Some(arg) = args.next() {
    let arg_text = arg.to_string_lossy();

    for prefix in ["-log_filter=", "--log_filter=", "-log-filter=", "--log-filter="] {
      if let Some(filter) = arg_text.strip_prefix(prefix).filter(|filter| !filter.trim().is_empty()) {
        return Some(filter.to_owned());
      }
    }

    if arg_text == "-log_filter"
      || arg_text == "--log_filter"
      || arg_text == "-log-filter"
      || arg_text == "--log-filter"
    {
      return args.next().and_then(|filter| {
        let filter = filter.to_string_lossy().trim().to_owned();
        (!filter.is_empty()).then_some(filter)
      });
    }
  }

  None
}

fn startup_log_domain_env() -> Option<String> {
  ["PARTIES_LOG_DOMAIN", "PARTIES_LOG_DOMAINS"]
    .into_iter()
    .find_map(|key| env::var(key).ok().and_then(|domain| log_domain_filter(&domain)))
}

fn startup_log_domain_arg(args: impl IntoIterator<Item = std::ffi::OsString>) -> Option<String> {
  let mut args = args.into_iter();

  while let Some(arg) = args.next() {
    let arg_text = arg.to_string_lossy();

    for prefix in ["-log_domain=", "--log_domain=", "-log-domain=", "--log-domain="] {
      if let Some(domain) = arg_text.strip_prefix(prefix) {
        return log_domain_filter(domain);
      }
    }

    if arg_text == "-log_domain"
      || arg_text == "--log_domain"
      || arg_text == "-log-domain"
      || arg_text == "--log-domain"
    {
      return args
        .next()
        .and_then(|domain| log_domain_filter(&domain.to_string_lossy()));
    }
  }

  None
}

fn log_domain_filter(domain: &str) -> Option<String> {
  match domain.trim().to_ascii_lowercase().as_str() {
    "video" | "stream" | "decode" => Some(
      [
        "warn",
        "video=info",
        "video::decode=debug",
        "video::decode::windows=debug",
        "profile=info",
        "native::windows=info",
        "native::windows::seh=warn",
      ]
      .join(","),
    ),
    "audio" | "voice" => Some("warn,audio::decode=debug,audio::encode=debug,voice=info,profile=info".to_owned()),
    "network" => Some("warn,network=info,network::connect=info".to_owned()),
    _ => None,
  }
}

fn startup_log_level_env() -> Option<String> {
  env::var("PARTIES_LOG_LEVEL")
    .ok()
    .and_then(|level| parse_level_filter(&level))
}

fn startup_log_level_arg(args: impl IntoIterator<Item = std::ffi::OsString>) -> Option<String> {
  let mut args = args.into_iter();

  while let Some(arg) = args.next() {
    let arg_text = arg.to_string_lossy();

    for prefix in ["-log_level=", "--log_level=", "-log-level=", "--log-level="] {
      if let Some(level) = arg_text.strip_prefix(prefix).filter(|level| !level.is_empty()) {
        return parse_level_filter(level);
      }
    }

    if arg_text == "-log_level" || arg_text == "--log_level" || arg_text == "-log-level" || arg_text == "--log-level" {
      return args
        .next()
        .and_then(|level| parse_level_filter(&level.to_string_lossy()));
    }
  }

  None
}

fn parse_level_filter(level: &str) -> Option<String> {
  match level.trim().to_ascii_lowercase().as_str() {
    "off" => Some("off".to_owned()),
    "error" => Some("error".to_owned()),
    "warn" | "warning" => Some("warn".to_owned()),
    "info" => Some("info".to_owned()),
    "debug" => Some("debug".to_owned()),
    "trace" => Some("trace".to_owned()),
    _ => None,
  }
}

fn normalize_log_filter_aliases(filter: &str) -> String {
  filter
    .split(',')
    .map(normalize_log_filter_directive_alias)
    .collect::<Vec<_>>()
    .join(",")
}

fn normalize_log_filter_directive_alias(directive: &str) -> String {
  let directive = directive.trim();
  if directive.is_empty() {
    return String::new();
  }

  let target_end = directive
    .char_indices()
    .find_map(|(index, ch)| matches!(ch, '=' | '[').then_some(index))
    .unwrap_or(directive.len());
  let target = &directive[..target_end];
  let Some(normalized_target) = log_filter_alias_target(target) else {
    return directive.to_owned();
  };

  format!("{normalized_target}{}", &directive[target_end..])
}

fn log_filter_alias_target(target: &str) -> Option<&'static str> {
  match target {
    "video:decode" | "video-decode" => Some("video::decode"),
    "video:encode" | "video-encode" => Some("video::encode"),
    "audio:decode" | "audio-decode" => Some("audio::decode"),
    "audio:encode" | "audio-encode" => Some("audio::encode"),
    _ => None,
  }
}

#[cfg(test)]
mod tests {
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
}
