use std::{
  collections::HashSet,
  env,
  fs::{self, OpenOptions},
  path::{Path, PathBuf},
  sync::{LazyLock, Once, RwLock},
};

use simplelog::{Config, ConfigBuilder, LevelFilter, SimpleLogger, WriteLogger};

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
    if let Some(path) = startup_log_file_arg(env::args_os().skip(1)) {
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
}
