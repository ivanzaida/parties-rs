use std::{
  env,
  error::Error,
  fmt, fs,
  io::Write,
  path::{Path, PathBuf},
  process::Command,
  time::{Duration, SystemTime, UNIX_EPOCH},
};

use reqwest::Client;
use semver::Version;
use serde_json::Value;

use crate::{
  session::ServerSession,
  storage::{Storage, StoredUpdateResumeState, StoredUpdateState},
};

const GITHUB_RELEASES_API: &str = "https://api.github.com/repos/ivanzaida/parties-rs/releases/latest";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
const SIMULATE_UPDATE_ENV: &str = "PARTIES_UPDATER_SIMULATE";
const SIMULATE_UPDATE_VERSION_ENV: &str = "PARTIES_UPDATER_SIMULATE_VERSION";

#[cfg(target_os = "macos")]
unsafe extern "C" {
  fn parties_macos_sparkle_start();
  #[allow(dead_code)]
  fn parties_macos_sparkle_check_for_updates();
}

#[derive(Clone, Debug, PartialEq, lurq::DevtoolsInspectable)]
pub enum StartupUpdateStatus {
  Idle,
  Checking,
  UpToDate,
  Downloading {
    version: String,
    downloaded: u64,
    total: Option<u64>,
  },
  Staging {
    version: String,
  },
  Ready {
    version: String,
    staged_executable: String,
  },
  Skipped(String),
  Failed(String),
}

impl Default for StartupUpdateStatus {
  fn default() -> Self {
    Self::Idle
  }
}

#[derive(Clone, Debug, PartialEq, Eq, lurq::DevtoolsInspectable)]
pub struct StartupUpdateOutcome {
  pub prepared: bool,
}

#[derive(Clone, Debug)]
struct ReleaseAsset {
  name: String,
  url: String,
  size: Option<u64>,
}

#[derive(Clone, Debug)]
struct LatestRelease {
  version: Version,
  asset: ReleaseAsset,
}

#[derive(Debug)]
enum UpdateError {
  MissingReleaseField(&'static str),
  MissingAsset(String),
  InvalidVersion(String),
  Http(reqwest::Error),
  Io(std::io::Error),
  Json(serde_json::Error),
  Archive(String),
}

impl fmt::Display for UpdateError {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      Self::MissingReleaseField(field) => write!(f, "release response missing {field}"),
      Self::MissingAsset(asset) => write!(f, "release asset not found: {asset}"),
      Self::InvalidVersion(version) => write!(f, "invalid release version: {version}"),
      Self::Http(error) => write!(f, "http: {error}"),
      Self::Io(error) => write!(f, "io: {error}"),
      Self::Json(error) => write!(f, "json: {error}"),
      Self::Archive(error) => write!(f, "archive: {error}"),
    }
  }
}

impl Error for UpdateError {}

impl From<reqwest::Error> for UpdateError {
  fn from(value: reqwest::Error) -> Self {
    Self::Http(value)
  }
}

impl From<std::io::Error> for UpdateError {
  fn from(value: std::io::Error) -> Self {
    Self::Io(value)
  }
}

impl From<serde_json::Error> for UpdateError {
  fn from(value: serde_json::Error) -> Self {
    Self::Json(value)
  }
}

pub async fn run_startup_update_check(
  status: lurq::core::Signal<StartupUpdateStatus>,
) -> Result<StartupUpdateOutcome, String> {
  match run_startup_update_check_inner(status.clone()).await {
    Ok(outcome) => Ok(outcome),
    Err(error) => {
      let error = error.to_string();
      status.set(StartupUpdateStatus::Failed(error.clone()));
      Ok(StartupUpdateOutcome { prepared: false })
    }
  }
}

pub fn start_platform_updater() {
  #[cfg(target_os = "macos")]
  unsafe {
    parties_macos_sparkle_start();
  }
}

#[allow(dead_code)]
pub fn check_for_platform_updates() {
  #[cfg(target_os = "macos")]
  unsafe {
    parties_macos_sparkle_check_for_updates();
  }
}

fn target_suffix() -> Option<&'static str> {
  #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
  {
    return Some("windows-x64");
  }
  #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
  {
    return Some("macos-arm64");
  }
  #[allow(unreachable_code)]
  None
}

async fn run_startup_update_check_inner(
  status: lurq::core::Signal<StartupUpdateStatus>,
) -> Result<StartupUpdateOutcome, UpdateError> {
  if platform_updater_handles_startup_checks() {
    status.set(StartupUpdateStatus::Skipped("Sparkle handles macOS updates".to_owned()));
    return Ok(StartupUpdateOutcome { prepared: false });
  }

  if simulated_update_enabled() {
    return run_simulated_startup_update(status).await;
  }

  if cfg!(debug_assertions) {
    status.set(StartupUpdateStatus::Skipped("debug build".to_owned()));
    return Ok(StartupUpdateOutcome { prepared: false });
  }

  let Some(target_suffix) = target_suffix() else {
    status.set(StartupUpdateStatus::Skipped("unsupported target".to_owned()));
    return Ok(StartupUpdateOutcome { prepared: false });
  };

  status.set(StartupUpdateStatus::Checking);
  let client = Client::builder()
    .user_agent(concat!("parties-rs/", env!("CARGO_PKG_VERSION")))
    .timeout(REQUEST_TIMEOUT)
    .build()?;
  let latest = fetch_latest_release(&client, target_suffix).await?;
  save_update_poll(&latest.version.to_string());
  let current = Version::parse(CURRENT_VERSION).map_err(|_| UpdateError::InvalidVersion(CURRENT_VERSION.to_owned()))?;

  if latest.version <= current {
    status.set(StartupUpdateStatus::UpToDate);
    return Ok(StartupUpdateOutcome { prepared: false });
  }

  let archive_path = download_asset(&client, &latest, status.clone()).await?;
  status.set(StartupUpdateStatus::Staging {
    version: latest.version.to_string(),
  });
  let version = latest.version.to_string();
  let staged_executable = tokio::task::spawn_blocking(move || stage_archive(&archive_path, &version))
    .await
    .map_err(|error| UpdateError::Archive(error.to_string()))??;
  status.set(StartupUpdateStatus::Ready {
    version: latest.version.to_string(),
    staged_executable: staged_executable.display().to_string(),
  });

  Ok(StartupUpdateOutcome { prepared: true })
}

async fn run_simulated_startup_update(
  status: lurq::core::Signal<StartupUpdateStatus>,
) -> Result<StartupUpdateOutcome, UpdateError> {
  let version = simulated_update_version();
  status.set(StartupUpdateStatus::Checking);
  tokio::time::sleep(Duration::from_millis(450)).await;
  save_update_poll(&version);

  let total = 10_u64 * 1024 * 1024;
  for step in 0..=10_u64 {
    status.set(StartupUpdateStatus::Downloading {
      version: version.clone(),
      downloaded: step * total / 10,
      total: Some(total),
    });
    tokio::time::sleep(Duration::from_millis(120)).await;
  }

  status.set(StartupUpdateStatus::Staging {
    version: version.clone(),
  });
  let staged_executable = tokio::task::spawn_blocking({
    let version = version.clone();
    move || stage_current_executable_copy(&version)
  })
  .await
  .map_err(|error| UpdateError::Archive(error.to_string()))??;

  status.set(StartupUpdateStatus::Ready {
    version,
    staged_executable: staged_executable.display().to_string(),
  });

  Ok(StartupUpdateOutcome { prepared: true })
}

fn simulated_update_enabled() -> bool {
  env::var(SIMULATE_UPDATE_ENV)
    .map(|value| matches!(value.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on"))
    .unwrap_or(false)
}

fn simulated_update_version() -> String {
  env::var(SIMULATE_UPDATE_VERSION_ENV)
    .ok()
    .map(|value| value.trim().to_owned())
    .filter(|value| !value.is_empty())
    .unwrap_or_else(|| "99.99.99".to_owned())
}

fn save_update_poll(version: &str) {
  let Ok(storage) = Storage::open_default() else {
    return;
  };
  let previous = storage.load_update_state().unwrap_or_default();
  let state = StoredUpdateState {
    last_checked_at: unix_timestamp(),
    last_seen_version: if version.is_empty() {
      previous.last_seen_version
    } else {
      version.to_owned()
    },
  };
  let _ = storage.save_update_state(&state);
}

fn unix_timestamp() -> i64 {
  SystemTime::now()
    .duration_since(UNIX_EPOCH)
    .map(|duration| duration.as_secs() as i64)
    .unwrap_or_default()
}

async fn fetch_latest_release(client: &Client, target_suffix: &str) -> Result<LatestRelease, UpdateError> {
  let value: Value = client
    .get(GITHUB_RELEASES_API)
    .send()
    .await?
    .error_for_status()?
    .json()
    .await?;
  let tag = value
    .get("tag_name")
    .and_then(Value::as_str)
    .ok_or(UpdateError::MissingReleaseField("tag_name"))?;
  let version_text = tag.trim_start_matches('v');
  let version = Version::parse(version_text).map_err(|_| UpdateError::InvalidVersion(version_text.to_owned()))?;
  let expected_asset = release_asset_name(&version, target_suffix);
  let assets = value
    .get("assets")
    .and_then(Value::as_array)
    .ok_or(UpdateError::MissingReleaseField("assets"))?;
  let asset = assets
    .iter()
    .filter_map(parse_release_asset)
    .find(|asset| asset.name == expected_asset)
    .ok_or(UpdateError::MissingAsset(expected_asset))?;

  Ok(LatestRelease { version, asset })
}

fn platform_updater_handles_startup_checks() -> bool {
  cfg!(target_os = "macos")
}

fn parse_release_asset(value: &Value) -> Option<ReleaseAsset> {
  Some(ReleaseAsset {
    name: value.get("name")?.as_str()?.to_owned(),
    url: value.get("browser_download_url")?.as_str()?.to_owned(),
    size: value.get("size").and_then(Value::as_u64),
  })
}

fn release_asset_name(version: &Version, target_suffix: &str) -> String {
  let extension = if target_suffix.starts_with("windows") {
    "zip"
  } else {
    "tar.gz"
  };
  format!("parties-rs-{version}-{target_suffix}.{extension}")
}

async fn download_asset(
  client: &Client,
  release: &LatestRelease,
  status: lurq::core::Signal<StartupUpdateStatus>,
) -> Result<PathBuf, UpdateError> {
  let update_dir = Storage::default_data_dir()
    .join("updates")
    .join(release.version.to_string());
  fs::create_dir_all(&update_dir)?;
  let archive_path = update_dir.join(&release.asset.name);
  let mut file = fs::File::create(&archive_path)?;
  let mut response = client.get(&release.asset.url).send().await?.error_for_status()?;
  let total = release.asset.size.or_else(|| response.content_length());
  let mut downloaded = 0_u64;

  status.set(StartupUpdateStatus::Downloading {
    version: release.version.to_string(),
    downloaded,
    total,
  });

  while let Some(chunk) = response.chunk().await? {
    file.write_all(&chunk)?;
    downloaded = downloaded.saturating_add(chunk.len() as u64);
    status.set(StartupUpdateStatus::Downloading {
      version: release.version.to_string(),
      downloaded,
      total,
    });
  }

  file.flush()?;
  Ok(archive_path)
}

fn stage_archive(archive_path: &Path, version: &str) -> Result<PathBuf, UpdateError> {
  let stage_dir = archive_path.parent().unwrap_or_else(|| Path::new(".")).join("staged");
  if stage_dir.exists() {
    fs::remove_dir_all(&stage_dir)?;
  }
  fs::create_dir_all(&stage_dir)?;

  let current_exe = std::env::current_exe()?;
  let executable_name =
    current_exe
      .file_name()
      .and_then(|name| name.to_str())
      .unwrap_or(if cfg!(target_os = "windows") {
        "parties-rs.exe"
      } else {
        "parties-rs"
      });
  let staged_executable = stage_dir.join(executable_name);

  #[cfg(target_os = "windows")]
  {
    extract_zip_executable(archive_path, &staged_executable, executable_name)?;
  }
  #[cfg(target_os = "macos")]
  {
    extract_tar_executable(archive_path, &staged_executable, executable_name)?;
  }
  #[cfg(not(any(target_os = "windows", target_os = "macos")))]
  {
    let _ = (archive_path, version, staged_executable.as_path(), executable_name);
    return Err(UpdateError::Archive("unsupported update target".to_owned()));
  }

  if !staged_executable.exists() {
    return Err(UpdateError::Archive(format!(
      "staged executable missing for {version}: {}",
      staged_executable.display()
    )));
  }

  Ok(staged_executable)
}

fn stage_current_executable_copy(version: &str) -> Result<PathBuf, UpdateError> {
  let current_exe = std::env::current_exe()?;
  let executable_name = current_exe
    .file_name()
    .ok_or_else(|| UpdateError::Archive("current executable has no file name".to_owned()))?;
  let stage_dir = Storage::default_data_dir()
    .join("updates")
    .join(format!("simulated-{version}"))
    .join("staged");

  if stage_dir.exists() {
    fs::remove_dir_all(&stage_dir)?;
  }
  fs::create_dir_all(&stage_dir)?;
  let staged_executable = stage_dir.join(executable_name);
  fs::copy(&current_exe, &staged_executable)?;

  Ok(staged_executable)
}

#[cfg(target_os = "windows")]
fn extract_zip_executable(
  archive_path: &Path,
  staged_executable: &Path,
  executable_name: &str,
) -> Result<(), UpdateError> {
  let file = fs::File::open(archive_path)?;
  let mut archive = zip::ZipArchive::new(file).map_err(|error| UpdateError::Archive(error.to_string()))?;

  for index in 0..archive.len() {
    let mut entry = archive
      .by_index(index)
      .map_err(|error| UpdateError::Archive(error.to_string()))?;
    let Some(name) = Path::new(entry.name()).file_name().and_then(|name| name.to_str()) else {
      continue;
    };
    if name != executable_name {
      continue;
    }

    let mut out = fs::File::create(staged_executable)?;
    std::io::copy(&mut entry, &mut out)?;
    return Ok(());
  }

  Err(UpdateError::Archive(format!("{executable_name} not found in zip")))
}

#[cfg(target_os = "macos")]
fn extract_tar_executable(
  archive_path: &Path,
  staged_executable: &Path,
  executable_name: &str,
) -> Result<(), UpdateError> {
  use std::os::unix::fs::PermissionsExt;

  let file = fs::File::open(archive_path)?;
  let decoder = flate2::read::GzDecoder::new(file);
  let mut archive = tar::Archive::new(decoder);

  for entry in archive.entries()? {
    let mut entry = entry?;
    let path = entry.path()?;
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
      continue;
    };
    if name != executable_name {
      continue;
    }

    let mut out = fs::File::create(staged_executable)?;
    std::io::copy(&mut entry, &mut out)?;
    fs::set_permissions(staged_executable, fs::Permissions::from_mode(0o755))?;
    return Ok(());
  }

  Err(UpdateError::Archive(format!("{executable_name} not found in tar.gz")))
}

pub fn restart_into_update(
  staged_executable: &str,
  storage: Option<&Storage>,
  session: Option<&ServerSession>,
) -> Result<(), String> {
  let staged_executable = PathBuf::from(staged_executable);
  let current_exe = std::env::current_exe().map_err(|error| error.to_string())?;
  schedule_replacement(&staged_executable, &current_exe)?;
  save_update_resume_state(storage, session);
  std::process::exit(0);
}

fn save_update_resume_state(storage: Option<&Storage>, session: Option<&ServerSession>) {
  let Some(storage) = storage else {
    return;
  };
  let Some(session) = session else {
    let _ = storage.clear_update_resume_state();
    return;
  };
  let Some(info) = session.info() else {
    let _ = storage.clear_update_resume_state();
    return;
  };

  let lobby = session.lobby();
  let voice_channel_id = lobby.selected_channel_id;
  let (muted, deafened) = session.local_voice_state().unwrap_or((false, false));
  let state = StoredUpdateResumeState {
    server_address: info.address,
    voice_channel_id,
    muted,
    deafened,
  };

  match storage.save_update_resume_state(&state) {
    Ok(()) => {
      tracing::debug!(
        target: "updater",
        "[updater] saved restart resume target: server={} voice_channel={:?} muted={} deafened={}",
        state.server_address,
        state.voice_channel_id,
        state.muted,
        state.deafened
      );
    }
    Err(error) => {
      tracing::debug!(target: "updater", "[updater] failed to save restart resume target: {error}");
    }
  }
}

#[cfg(target_os = "windows")]
fn schedule_replacement(staged_executable: &Path, current_exe: &Path) -> Result<(), String> {
  use std::os::windows::process::CommandExt;

  const CREATE_NO_WINDOW: u32 = 0x08000000;

  let script_path = Storage::default_data_dir().join("updates").join("apply-update.cmd");
  if let Some(parent) = script_path.parent() {
    fs::create_dir_all(parent).map_err(|error| error.to_string())?;
  }
  let script = format!(
    r#"@echo off
set "SRC={}"
set "DST={}"
:wait
>nul 2>nul copy /Y "%SRC%" "%DST%"
if errorlevel 1 (
  timeout /t 1 /nobreak >nul
  goto wait
)
start "" "%DST%"
del "%~f0"
"#,
    staged_executable.display(),
    current_exe.display()
  );
  fs::write(&script_path, script).map_err(|error| error.to_string())?;
  Command::new("cmd")
    .args(["/C", script_path.to_string_lossy().as_ref()])
    .creation_flags(CREATE_NO_WINDOW)
    .spawn()
    .map_err(|error| error.to_string())?;
  Ok(())
}

#[cfg(not(target_os = "windows"))]
fn schedule_replacement(staged_executable: &Path, current_exe: &Path) -> Result<(), String> {
  let command = format!(
    "sleep 1; cp '{}' '{}'; chmod +x '{}'; exec '{}' >/dev/null 2>&1 &",
    shell_quote(staged_executable),
    shell_quote(current_exe),
    shell_quote(current_exe),
    shell_quote(current_exe)
  );
  Command::new("sh")
    .args(["-c", &command])
    .spawn()
    .map_err(|error| error.to_string())?;
  Ok(())
}

#[cfg(not(target_os = "windows"))]
fn shell_quote(path: &Path) -> String {
  path.to_string_lossy().replace('\'', "'\\''")
}
