use std::{sync::Arc, time::Duration};

use crate::storage::Storage;

const STARTUP_STEP_DELAY: Duration = Duration::from_millis(120);

#[derive(Clone, PartialEq, lurq::DevtoolsInspectable)]
pub struct StartupProgress {
  pub ratio: f32,
  pub label: Arc<str>,
}

impl StartupProgress {
  pub fn new(ratio: f32, label: Arc<str>) -> Self {
    Self {
      ratio: ratio.clamp(0.0, 1.0),
      label,
    }
  }
}

impl Default for StartupProgress {
  fn default() -> Self {
    Self::new(0.08, "".into())
  }
}

#[derive(Clone)]
pub struct StartupProgressLabels {
  pub starting: Arc<str>,
  pub opening_storage: Arc<str>,
  pub checking_identity: Arc<str>,
  pub loading_servers: Arc<str>,
  pub preparing_workspace: Arc<str>,
  pub opening_workspace: Arc<str>,
}

#[derive(Clone, PartialEq, lurq::DevtoolsInspectable)]
pub struct StartupData {
  pub storage: Option<Storage>,
  pub has_identity: bool,
  pub saved_server_count: usize,
}

fn update_progress(progress: &lurq::core::Signal<StartupProgress>, ratio: f32, label: Arc<str>) {
  progress.set(StartupProgress::new(ratio, label));
  std::thread::sleep(STARTUP_STEP_DELAY);
}

fn load_startup_data_sync(
  progress: lurq::core::Signal<StartupProgress>,
  labels: StartupProgressLabels,
  initial_storage: Option<Storage>,
) -> Result<StartupData, String> {
  update_progress(&progress, 0.24, labels.opening_storage);

  match initial_storage.map_or_else(Storage::open_default, Ok) {
    Ok(storage) => {
      update_progress(&progress, 0.52, labels.checking_identity);
      let has_identity = storage.has_identity().map_err(|error| error.to_string())?;
      let saved_server_count = if has_identity {
        update_progress(&progress, 0.78, labels.loading_servers);
        storage.load_servers().map_err(|error| error.to_string())?.len()
      } else {
        update_progress(&progress, 0.82, labels.preparing_workspace);
        0
      };

      update_progress(&progress, 1.0, labels.opening_workspace);

      Ok(StartupData {
        storage: Some(storage),
        has_identity,
        saved_server_count,
      })
    }
    Err(error) => Err(error.to_string()),
  }
}

pub async fn load_startup_data(
  progress: lurq::core::Signal<StartupProgress>,
  labels: StartupProgressLabels,
  initial_storage: Option<Storage>,
) -> Result<StartupData, String> {
  progress.set(StartupProgress::new(0.08, labels.starting.clone()));
  tokio::task::spawn_blocking(move || load_startup_data_sync(progress, labels, initial_storage))
    .await
    .map_err(|error| error.to_string())?
}
