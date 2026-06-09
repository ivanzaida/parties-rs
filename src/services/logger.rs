use std::{
  collections::HashSet,
  env,
  fs::{self, OpenOptions},
  path::{Path, PathBuf},
  sync::{LazyLock, Once, RwLock},
};

use simplelog::{Config, ConfigBuilder, LevelFilter, SimpleLogger, WriteLogger};

const DEFAULT_LOG_FILE_PREFIX: &str = "parties_rs_";
const DEFAULT_LOG_FILE_SUFFIX: &str = ".log";

static SEEN_MSGS: LazyLock<RwLock<HashSet<String>>> = LazyLock::new(|| RwLock::new(HashSet::new()));
static LOGGER_INIT: Once = Once::new();

#[macro_export]
macro_rules! log_once {
  () => {
    $crate::services::logger::log_once("")
  };
  ($($arg:tt)*) => {{
    $crate::services::logger::log_once(&format!($($arg)*));
  }};
}

pub fn log_once(msg: &str) {
  {
    let read = SEEN_MSGS.read().expect("logger lock poisoned");
    if read.contains(msg) {
      return;
    }
  }

  let mut write = SEEN_MSGS.write().expect("logger lock poisoned");
  if write.insert(msg.to_owned()) {
    log(msg);
  }
}

pub fn init() {
  LOGGER_INIT.call_once(|| {
    let config = logger_config();
    let explicit_log_file = startup_log_file_arg(env::args_os().skip(1));
    let path = explicit_log_file.clone().or_else(default_log_file_path);
    if let Some(path) = path {
      if explicit_log_file.is_none() {
        cleanup_old_default_log_files(&path);
      }
      match open_log_file(&path) {
        Ok(file) => {
          if WriteLogger::init(LevelFilter::Info, config.clone(), file).is_ok() {
            return;
          }
        }
        Err(error) => {
          eprintln!("failed to open log file {}: {error}", path.display());
        }
      }
    }

    let _ = SimpleLogger::init(LevelFilter::Info, config);
  });
}

pub fn log(msg: &str) {
  log::info!("{msg}");
}

fn logger_config() -> Config {
  ConfigBuilder::new()
    .set_time_format_rfc3339()
    .set_thread_level(LevelFilter::Off)
    .set_target_level(LevelFilter::Off)
    .build()
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
