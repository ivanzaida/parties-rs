use std::{
  collections::HashSet,
  sync::{LazyLock, Once, RwLock},
};

use simplelog::{ConfigBuilder, LevelFilter, SimpleLogger};

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
    let config = ConfigBuilder::new()
      .set_time_format_rfc3339()
      .set_thread_level(LevelFilter::Off)
      .set_target_level(LevelFilter::Off)
      .build();
    let _ = SimpleLogger::init(LevelFilter::Info, config);
  });
}

pub fn log(msg: &str) {
  log::info!("{msg}");
}
