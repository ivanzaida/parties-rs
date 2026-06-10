use std::{
  collections::HashMap,
  env,
  sync::{LazyLock, Mutex},
  time::{Duration, Instant},
};

const DEFAULT_LOG_INTERVAL: Duration = Duration::from_secs(2);

static PROFILER: LazyLock<Profiler> = LazyLock::new(Profiler::from_startup);

pub struct Span {
  name: &'static str,
  started_at: Option<Instant>,
}

struct Profiler {
  enabled: bool,
  log_interval: Duration,
  state: Mutex<ProfilerState>,
}

#[derive(Default)]
struct ProfilerState {
  stats: HashMap<&'static str, SpanStats>,
  last_log: Option<Instant>,
}

#[derive(Clone, Copy, Default)]
struct SpanStats {
  count: u64,
  total: Duration,
  max: Duration,
}

pub fn span(name: &'static str) -> Span {
  if !PROFILER.enabled {
    return Span { name, started_at: None };
  }

  Span {
    name,
    started_at: Some(Instant::now()),
  }
}

impl Drop for Span {
  fn drop(&mut self) {
    let Some(started_at) = self.started_at else {
      return;
    };
    PROFILER.record(self.name, started_at.elapsed());
  }
}

impl Profiler {
  fn from_startup() -> Self {
    Self {
      enabled: profiling_enabled(env::args().skip(1)),
      log_interval: profile_log_interval(),
      state: Mutex::new(ProfilerState::default()),
    }
  }

  fn record(&self, name: &'static str, elapsed: Duration) {
    let mut summaries = Vec::new();
    {
      let mut state = self.state.lock().expect("profiler lock poisoned");
      let stat = state.stats.entry(name).or_default();
      stat.count += 1;
      stat.total += elapsed;
      stat.max = stat.max.max(elapsed);

      let now = Instant::now();
      let last_log = state.last_log.get_or_insert(now);
      if now.duration_since(*last_log) < self.log_interval {
        return;
      }

      *last_log = now;
      summaries.extend(state.stats.iter().map(|(name, stat)| (*name, *stat)));
      state.stats.clear();
    }

    summaries.sort_by_key(|(name, _)| *name);
    for (name, stat) in summaries {
      let avg = stat.total.as_secs_f64() * 1000.0 / stat.count.max(1) as f64;
      let max = stat.max.as_secs_f64() * 1000.0;
      tracing::info!(target: "profile", "[profile] {name}: count={} avg={avg:.3}ms max={max:.3}ms", stat.count);
    }
  }
}

fn profiling_enabled(args: impl IntoIterator<Item = String>) -> bool {
  if env_flag("PARTIES_PROFILE") {
    return true;
  }

  profiling_arg_enabled(args)
}

fn profiling_arg_enabled(args: impl IntoIterator<Item = String>) -> bool {
  args.into_iter().any(|arg| {
    matches!(arg.as_str(), "-profile" | "--profile")
      || arg
        .strip_prefix("-profile=")
        .or_else(|| arg.strip_prefix("--profile="))
        .is_some_and(truthy)
  })
}

fn profile_log_interval() -> Duration {
  env::var("PARTIES_PROFILE_INTERVAL_MS")
    .ok()
    .and_then(|value| value.parse::<u64>().ok())
    .filter(|millis| *millis > 0)
    .map(Duration::from_millis)
    .unwrap_or(DEFAULT_LOG_INTERVAL)
}

fn env_flag(name: &str) -> bool {
  env::var(name).is_ok_and(|value| truthy(&value))
}

fn truthy(value: &str) -> bool {
  matches!(value.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on")
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn profiling_enabled_accepts_profile_arg() {
    assert!(profiling_arg_enabled(["--profile".to_owned()]));
    assert!(profiling_arg_enabled(["-profile=true".to_owned()]));
    assert!(!profiling_arg_enabled(["--profile=false".to_owned()]));
  }
}
