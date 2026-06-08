use std::{
  collections::HashSet,
  sync::{LazyLock, RwLock},
};

static SEEN_MSGS: LazyLock<RwLock<HashSet<String>>> = LazyLock::new(|| RwLock::new(HashSet::new()));

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
    println!("{msg}");
  }
}
