use std::{
  collections::HashMap,
  sync::{
    Arc, Condvar, Mutex,
    atomic::{AtomicBool, Ordering},
  },
  thread::{self, JoinHandle},
  time::{Duration, Instant},
};

use serde::Deserialize;
use serde_json::Value;

use crate::{
  config::SoundCloudConfig,
  sources::{
    model::{ResolvedAudio, ResolvedAudioPayload, SourceKind, SourceRequest},
    resolver::SourceResolver,
  },
};

const SOUNDCLOUD_API_BASE_URL: &str = "https://api.soundcloud.com";
const SOUNDCLOUD_TOKEN_URL: &str = "https://secure.soundcloud.com/oauth/token";
const MAX_REMOTE_AUDIO_BYTES: u64 = 256 * 1024 * 1024;
const TOKEN_REFRESH_MARGIN: Duration = Duration::from_secs(5 * 60);
const TOKEN_REFRESH_RETRY_DELAY: Duration = Duration::from_secs(30);
const DEFAULT_TOKEN_LIFETIME: Duration = Duration::from_secs(60 * 60);
const MAX_HLS_PLAYLIST_DEPTH: usize = 4;
const HTTP_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const HTTP_REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Clone)]
pub(crate) struct SoundCloudTokenProvider {
  inner: Arc<SoundCloudTokenProviderInner>,
}

#[derive(Clone)]
pub(crate) struct SoundCloudResolver {
  token_provider: SoundCloudTokenProvider,
}

struct SoundCloudTokenProviderInner {
  config: SoundCloudConfig,
  client: reqwest::blocking::Client,
  state: Mutex<TokenState>,
  wakeup: Condvar,
  shutdown: AtomicBool,
  worker: Mutex<Option<JoinHandle<()>>>,
}

#[derive(Default)]
struct TokenState {
  auth_header: Option<String>,
  refresh_token: Option<String>,
  expires_at: Option<Instant>,
  last_error: Option<String>,
}

impl SoundCloudTokenProvider {
  pub(crate) fn new(config: SoundCloudConfig) -> Result<Self, String> {
    let client = reqwest::blocking::Client::builder()
      .user_agent("parties-music-bot/0.1")
      .connect_timeout(HTTP_CONNECT_TIMEOUT)
      .timeout(HTTP_REQUEST_TIMEOUT)
      .build()
      .map_err(|error| format!("failed to initialize SoundCloud HTTP client: {error}"))?;
    let provider = Self {
      inner: Arc::new(SoundCloudTokenProviderInner {
        config,
        client,
        state: Mutex::new(TokenState::default()),
        wakeup: Condvar::new(),
        shutdown: AtomicBool::new(false),
        worker: Mutex::new(None),
      }),
    };

    provider.refresh_now()?;
    provider.spawn_refresh_worker();
    Ok(provider)
  }

  #[cfg(test)]
  pub(crate) fn new_for_tests(auth_header: &str) -> Self {
    Self {
      inner: Arc::new(SoundCloudTokenProviderInner {
        config: SoundCloudConfig {
          client_id: "client-id".to_owned(),
          client_secret: "client-secret".to_owned(),
        },
        client: reqwest::blocking::Client::new(),
        state: Mutex::new(TokenState {
          auth_header: Some(auth_header.to_owned()),
          refresh_token: None,
          expires_at: Some(Instant::now() + DEFAULT_TOKEN_LIFETIME),
          last_error: None,
        }),
        wakeup: Condvar::new(),
        shutdown: AtomicBool::new(false),
        worker: Mutex::new(None),
      }),
    }
  }

  pub(crate) fn authorization_header(&self) -> Result<String, String> {
    if let Some(auth_header) = self.valid_cached_header() {
      return Ok(auth_header);
    }

    self.refresh_now()?;
    self
      .valid_cached_header()
      .ok_or_else(|| "SoundCloud token refresh did not produce a usable token.".to_owned())
  }

  pub(crate) fn shutdown(&self) {
    self.inner.shutdown.store(true, Ordering::SeqCst);
    self.inner.wakeup.notify_all();
    if let Some(worker) = self
      .inner
      .worker
      .lock()
      .expect("soundcloud worker mutex poisoned")
      .take()
    {
      worker.join().ok();
    }
  }

  fn valid_cached_header(&self) -> Option<String> {
    let state = self.inner.state.lock().expect("soundcloud token state mutex poisoned");
    let expires_at = state.expires_at?;
    (expires_at > Instant::now() + Duration::from_secs(30))
      .then(|| state.auth_header.clone())
      .flatten()
  }

  fn refresh_now(&self) -> Result<(), String> {
    let refresh_token = {
      self
        .inner
        .state
        .lock()
        .expect("soundcloud token state mutex poisoned")
        .refresh_token
        .clone()
    };
    let token = if let Some(refresh_token) = refresh_token {
      refresh_access_token(&self.inner.client, &self.inner.config, &refresh_token).or_else(|refresh_error| {
        request_access_token(&self.inner.client, &self.inner.config).map_err(|request_error| {
          format!(
            "failed to refresh SoundCloud access token: {refresh_error}; fallback client credentials request failed: {request_error}"
          )
        })
      })?
    } else {
      request_access_token(&self.inner.client, &self.inner.config)?
    };
    let mut state = self.inner.state.lock().expect("soundcloud token state mutex poisoned");
    state.auth_header = Some(token.auth_header);
    state.refresh_token = token.refresh_token;
    state.expires_at = Some(token.expires_at);
    state.last_error = None;
    self.inner.wakeup.notify_all();
    Ok(())
  }

  fn spawn_refresh_worker(&self) {
    let provider = self.clone();
    let worker = thread::spawn(move || provider.run_refresh_worker());
    *self.inner.worker.lock().expect("soundcloud worker mutex poisoned") = Some(worker);
  }

  fn run_refresh_worker(self) {
    loop {
      let wait_duration = {
        let state = self.inner.state.lock().expect("soundcloud token state mutex poisoned");
        if self.inner.shutdown.load(Ordering::SeqCst) {
          return;
        }
        state
          .expires_at
          .map(refresh_wait_duration)
          .unwrap_or(TOKEN_REFRESH_RETRY_DELAY)
      };

      let state = self.inner.state.lock().expect("soundcloud token state mutex poisoned");
      let (state, wait_result) = self
        .inner
        .wakeup
        .wait_timeout(state, wait_duration)
        .expect("soundcloud token state mutex poisoned");
      if self.inner.shutdown.load(Ordering::SeqCst) {
        return;
      }
      if !wait_result.timed_out() {
        drop(state);
        continue;
      }
      drop(state);

      if let Err(error) = self.refresh_now() {
        let mut state = self.inner.state.lock().expect("soundcloud token state mutex poisoned");
        state.last_error = Some(error);
        let (state, _) = self
          .inner
          .wakeup
          .wait_timeout(state, TOKEN_REFRESH_RETRY_DELAY)
          .expect("soundcloud token state mutex poisoned");
        drop(state);
      }
    }
  }
}

impl SoundCloudResolver {
  pub(crate) fn new(token_provider: SoundCloudTokenProvider) -> Self {
    Self { token_provider }
  }

  pub(crate) fn shutdown(&self) {
    self.token_provider.shutdown();
  }
}

impl SourceResolver for SoundCloudResolver {
  fn supports(&self, input: &str) -> bool {
    is_soundcloud_url(input)
  }

  fn request(&self, input: &str) -> Option<SourceRequest> {
    self.supports(input).then(|| SourceRequest {
      kind: SourceKind::SoundCloud,
      url: input.to_owned(),
      provider_id: None,
      duration_ms: None,
      loading_title: "SoundCloud URL".to_owned(),
    })
  }

  fn requests(&self, input: &str) -> Result<Vec<SourceRequest>, String> {
    if !self.supports(input) {
      return Err("SoundCloud URL could not be parsed.".to_owned());
    }

    resolve_requests(input, &self.token_provider)
  }

  fn resolve(&self, request: &SourceRequest) -> Result<ResolvedAudio, String> {
    if request.kind != SourceKind::SoundCloud {
      return Err("SoundCloud resolver received a non-SoundCloud source.".to_owned());
    }

    if let Some(track_id) = request.provider_id.as_deref().and_then(|id| id.parse::<u64>().ok()) {
      return resolve_audio_from_track_id(&request.url, track_id, &request.loading_title, &self.token_provider);
    }

    resolve_audio(&request.url, &self.token_provider)
  }
}

struct RequestedToken {
  auth_header: String,
  refresh_token: Option<String>,
  expires_at: Instant,
}

#[derive(Deserialize)]
struct SoundCloudTrack {
  id: u64,
  title: Option<String>,
  kind: Option<String>,
  streamable: Option<bool>,
  duration: Option<u64>,
  permalink_url: Option<String>,
}

#[derive(Deserialize)]
struct SoundCloudPlaylist {
  tracks: Vec<SoundCloudPlaylistTrack>,
}

#[derive(Deserialize)]
struct SoundCloudPlaylistTrack {
  id: Option<u64>,
  title: Option<String>,
  kind: Option<String>,
  streamable: Option<bool>,
  duration: Option<u64>,
  permalink_url: Option<String>,
}

#[derive(Deserialize)]
struct SoundCloudStreamRedirect {
  url: String,
}

struct SoundCloudStreamCandidate {
  key: String,
  url: String,
}

struct DownloadedAudio {
  bytes: Vec<u8>,
  container_hint: Option<String>,
}

#[derive(Deserialize)]
struct SoundCloudTokenResponse {
  access_token: String,
  refresh_token: Option<String>,
  token_type: Option<String>,
  expires_in: Option<u64>,
}

pub(crate) fn resolve_audio(url: &str, token_provider: &SoundCloudTokenProvider) -> Result<ResolvedAudio, String> {
  let client = soundcloud_client()?;
  let track = resolve_track(&client, token_provider, url)?;
  resolve_audio_from_track(&client, token_provider, url, track)
}

fn resolve_audio_from_track_id(
  source_url: &str,
  track_id: u64,
  title: &str,
  token_provider: &SoundCloudTokenProvider,
) -> Result<ResolvedAudio, String> {
  let client = soundcloud_client()?;
  let track = SoundCloudTrack {
    id: track_id,
    title: Some(title.to_owned()),
    kind: Some("track".to_owned()),
    streamable: None,
    duration: None,
    permalink_url: Some(source_url.to_owned()),
  };
  resolve_audio_from_track(&client, token_provider, source_url, track)
}

fn resolve_audio_from_track(
  client: &reqwest::blocking::Client,
  token_provider: &SoundCloudTokenProvider,
  source_url: &str,
  track: SoundCloudTrack,
) -> Result<ResolvedAudio, String> {
  if track.kind.as_deref().is_some_and(|kind| kind != "track") {
    return Err("Only SoundCloud track URLs are supported right now.".to_owned());
  }
  if track.streamable == Some(false) {
    return Err("This SoundCloud track is not streamable through the API.".to_owned());
  }

  let candidate = resolve_stream_candidate(client, token_provider, track.id)?;
  let is_hls_candidate = is_hls_url(&candidate.url) || candidate.key.contains("hls");
  let is_direct_api_audio_candidate = candidate.key.contains("mp3");
  let stream_url = if is_hls_candidate || is_direct_api_audio_candidate {
    candidate.url.clone()
  } else {
    resolve_stream_url(client, token_provider, &candidate.url)?
  };
  let audio = if is_hls_candidate || is_hls_url(&stream_url) {
    download_hls_audio(client, token_provider, &stream_url)?
  } else {
    DownloadedAudio {
      bytes: download_remote_audio(client, token_provider, &stream_url)?,
      container_hint: infer_audio_extension(&stream_url).or_else(|| infer_audio_extension(&candidate.key)),
    }
  };

  Ok(ResolvedAudio {
    title: track.title.unwrap_or_else(|| "SoundCloud track".to_owned()),
    source_kind: SourceKind::SoundCloud,
    source_url: source_url.to_owned(),
    payload: ResolvedAudioPayload::Buffered {
      bytes: audio.bytes,
      container_hint: audio.container_hint,
    },
  })
}

fn resolve_requests(url: &str, token_provider: &SoundCloudTokenProvider) -> Result<Vec<SourceRequest>, String> {
  let client = soundcloud_client()?;
  let value = resolve_url_value(&client, token_provider, url)?;
  requests_from_resolved_value(url, value)
}

fn requests_from_resolved_value(url: &str, value: Value) -> Result<Vec<SourceRequest>, String> {
  if resolved_value_is_playlist(&value) {
    let playlist: SoundCloudPlaylist = serde_json::from_value(value)
      .map_err(|error| format!("failed to parse SoundCloud playlist response: {error}"))?;
    let requests = playlist
      .tracks
      .into_iter()
      .filter_map(|track| request_from_playlist_track(url, track))
      .collect::<Vec<_>>();

    if requests.is_empty() {
      return Err("SoundCloud playlist did not contain queueable tracks.".to_owned());
    }

    return Ok(requests);
  }

  let track: SoundCloudTrack =
    serde_json::from_value(value).map_err(|error| format!("failed to parse SoundCloud track response: {error}"))?;
  Ok(vec![request_from_track(url, track)])
}

fn resolved_value_is_playlist(value: &Value) -> bool {
  value
    .get("kind")
    .and_then(Value::as_str)
    .is_some_and(|kind| kind == "playlist" || kind == "system-playlist")
    || value.get("tracks").is_some_and(Value::is_array)
}

fn request_from_track(fallback_url: &str, track: SoundCloudTrack) -> SourceRequest {
  let title = track.title.unwrap_or_else(|| "SoundCloud track".to_owned());
  SourceRequest {
    kind: SourceKind::SoundCloud,
    url: track.permalink_url.unwrap_or_else(|| fallback_url.to_owned()),
    provider_id: Some(track.id.to_string()),
    duration_ms: track.duration,
    loading_title: title,
  }
}

fn request_from_playlist_track(fallback_url: &str, track: SoundCloudPlaylistTrack) -> Option<SourceRequest> {
  if track.kind.as_deref().is_some_and(|kind| kind != "track") || track.streamable == Some(false) {
    return None;
  }

  let track_id = track.id?;
  let title = track.title.unwrap_or_else(|| "SoundCloud track".to_owned());
  Some(SourceRequest {
    kind: SourceKind::SoundCloud,
    url: track.permalink_url.unwrap_or_else(|| fallback_url.to_owned()),
    provider_id: Some(track_id.to_string()),
    duration_ms: track.duration,
    loading_title: title,
  })
}

fn soundcloud_client() -> Result<reqwest::blocking::Client, String> {
  reqwest::blocking::Client::builder()
    .user_agent("parties-music-bot/0.1")
    .connect_timeout(HTTP_CONNECT_TIMEOUT)
    .timeout(HTTP_REQUEST_TIMEOUT)
    .build()
    .map_err(|error| format!("failed to initialize SoundCloud HTTP client: {error}"))
}

fn request_access_token(
  client: &reqwest::blocking::Client,
  config: &SoundCloudConfig,
) -> Result<RequestedToken, String> {
  let params = [("grant_type", "client_credentials")];
  let response: SoundCloudTokenResponse = client
    .post(SOUNDCLOUD_TOKEN_URL)
    .header(reqwest::header::ACCEPT, "application/json; charset=utf-8")
    .basic_auth(&config.client_id, Some(&config.client_secret))
    .form(&params)
    .send()
    .map_err(|error| format!("failed to request SoundCloud access token: {error}"))?
    .error_for_status()
    .map_err(|error| format!("SoundCloud token request failed: {error}"))?
    .json()
    .map_err(|error| format!("failed to parse SoundCloud token response: {error}"))?;

  if response.access_token.trim().is_empty() {
    return Err("SoundCloud token response did not contain an access token.".to_owned());
  }

  let lifetime = response
    .expires_in
    .map(Duration::from_secs)
    .unwrap_or(DEFAULT_TOKEN_LIFETIME)
    .max(Duration::from_secs(60));
  Ok(RequestedToken {
    auth_header: soundcloud_auth_header(response.token_type.as_deref(), &response.access_token),
    refresh_token: response.refresh_token,
    expires_at: Instant::now() + lifetime,
  })
}

fn refresh_access_token(
  client: &reqwest::blocking::Client,
  config: &SoundCloudConfig,
  refresh_token: &str,
) -> Result<RequestedToken, String> {
  let params = [("grant_type", "refresh_token"), ("refresh_token", refresh_token)];
  let response: SoundCloudTokenResponse = client
    .post(SOUNDCLOUD_TOKEN_URL)
    .header(reqwest::header::ACCEPT, "application/json; charset=utf-8")
    .basic_auth(&config.client_id, Some(&config.client_secret))
    .form(&params)
    .send()
    .map_err(|error| format!("failed to refresh SoundCloud access token: {error}"))?
    .error_for_status()
    .map_err(|error| format!("SoundCloud token refresh failed: {error}"))?
    .json()
    .map_err(|error| format!("failed to parse SoundCloud token refresh response: {error}"))?;

  if response.access_token.trim().is_empty() {
    return Err("SoundCloud token refresh response did not contain an access token.".to_owned());
  }

  let lifetime = response
    .expires_in
    .map(Duration::from_secs)
    .unwrap_or(DEFAULT_TOKEN_LIFETIME)
    .max(Duration::from_secs(60));
  Ok(RequestedToken {
    auth_header: soundcloud_auth_header(response.token_type.as_deref(), &response.access_token),
    refresh_token: response.refresh_token,
    expires_at: Instant::now() + lifetime,
  })
}

fn refresh_wait_duration(expires_at: Instant) -> Duration {
  let refresh_at = expires_at.checked_sub(TOKEN_REFRESH_MARGIN).unwrap_or(expires_at);
  refresh_at.saturating_duration_since(Instant::now())
}

fn resolve_track(
  client: &reqwest::blocking::Client,
  token_provider: &SoundCloudTokenProvider,
  url: &str,
) -> Result<SoundCloudTrack, String> {
  let value = resolve_url_value(client, token_provider, url)?;
  serde_json::from_value(value).map_err(|error| format!("failed to parse SoundCloud track response: {error}"))
}

fn resolve_url_value(
  client: &reqwest::blocking::Client,
  token_provider: &SoundCloudTokenProvider,
  url: &str,
) -> Result<Value, String> {
  let endpoint = reqwest::Url::parse_with_params(&format!("{SOUNDCLOUD_API_BASE_URL}/resolve"), [("url", url)])
    .map_err(|error| format!("failed to build SoundCloud resolve URL: {error}"))?;
  soundcloud_get_json(client, token_provider, endpoint)
}

fn resolve_stream_candidate(
  client: &reqwest::blocking::Client,
  token_provider: &SoundCloudTokenProvider,
  track_id: u64,
) -> Result<SoundCloudStreamCandidate, String> {
  let endpoint = format!("{SOUNDCLOUD_API_BASE_URL}/tracks/{track_id}/streams");
  let streams: HashMap<String, Option<String>> = soundcloud_get_json(client, token_provider, endpoint)?;
  select_stream(streams)
}

fn select_stream(streams: HashMap<String, Option<String>>) -> Result<SoundCloudStreamCandidate, String> {
  let mut candidates = streams
    .into_iter()
    .filter_map(|(key, url)| url.map(|url| SoundCloudStreamCandidate { key, url }))
    .collect::<Vec<_>>();
  candidates.sort_by_key(|candidate| stream_rank(&candidate.key, &candidate.url));
  let available_streams = candidates
    .iter()
    .map(|candidate| candidate.key.as_str())
    .collect::<Vec<_>>()
    .join(", ");

  candidates.into_iter().find(is_supported_stream).ok_or_else(|| {
    if available_streams.is_empty() {
      "SoundCloud did not return any stream URLs for this track.".to_owned()
    } else {
      format!("SoundCloud did not return a supported AAC/M4A/HLS stream. Available streams: {available_streams}")
    }
  })
}

fn stream_rank(key: &str, url: &str) -> usize {
  let key = key.to_ascii_lowercase();
  let extension = infer_audio_extension(url);
  if !key.contains("hls") && (key.contains("aac") || extension.as_deref() == Some("m4a")) {
    0
  } else if !key.contains("hls") && (key.contains("mp3") || extension.as_deref() == Some("mp3")) {
    1
  } else if key.contains("hls") && (key.contains("aac") || key.contains("mp3") || is_hls_url(url)) {
    2
  } else {
    3
  }
}

fn is_supported_stream(candidate: &SoundCloudStreamCandidate) -> bool {
  let key = candidate.key.to_ascii_lowercase();
  let extension = infer_audio_extension(&candidate.url);
  if is_hls_url(&candidate.url) || key.contains("hls") {
    return key.contains("aac") || key.contains("mp3") || is_hls_url(&candidate.url);
  }

  key.contains("aac") || key.contains("mp3") || matches!(extension.as_deref(), Some("m4a") | Some("mp3"))
}

fn resolve_stream_url(
  client: &reqwest::blocking::Client,
  token_provider: &SoundCloudTokenProvider,
  url: &str,
) -> Result<String, String> {
  if !is_soundcloud_api_url(url) {
    return Ok(url.to_owned());
  }

  let redirect: SoundCloudStreamRedirect = soundcloud_get_json(client, token_provider, url)?;
  Ok(redirect.url)
}

fn soundcloud_get_json<T, U>(
  client: &reqwest::blocking::Client,
  token_provider: &SoundCloudTokenProvider,
  url: U,
) -> Result<T, String>
where
  T: for<'de> Deserialize<'de>,
  U: reqwest::IntoUrl,
{
  let url = url
    .into_url()
    .map_err(|error| format!("failed to build SoundCloud API URL: {error}"))?;
  let auth_header = token_provider.authorization_header()?;
  match send_soundcloud_get_json(client, &auth_header, url.clone()) {
    Ok(value) => Ok(value),
    Err(SoundCloudApiError::Unauthorized) => {
      token_provider.refresh_now()?;
      let auth_header = token_provider.authorization_header()?;
      send_soundcloud_get_json(client, &auth_header, url).map_err(SoundCloudApiError::into_message)
    }
    Err(error) => Err(error.into_message()),
  }
}

fn soundcloud_get_text<U>(
  client: &reqwest::blocking::Client,
  token_provider: &SoundCloudTokenProvider,
  url: U,
) -> Result<String, String>
where
  U: reqwest::IntoUrl,
{
  let url = url
    .into_url()
    .map_err(|error| format!("failed to build SoundCloud API URL: {error}"))?;
  let auth_header = token_provider.authorization_header()?;
  match send_soundcloud_get_text(client, &auth_header, url.clone()) {
    Ok(value) => Ok(value),
    Err(SoundCloudApiError::Unauthorized) => {
      token_provider.refresh_now()?;
      let auth_header = token_provider.authorization_header()?;
      send_soundcloud_get_text(client, &auth_header, url).map_err(SoundCloudApiError::into_message)
    }
    Err(error) => Err(error.into_message()),
  }
}

fn soundcloud_get_bytes<U>(
  client: &reqwest::blocking::Client,
  token_provider: &SoundCloudTokenProvider,
  url: U,
) -> Result<Vec<u8>, String>
where
  U: reqwest::IntoUrl,
{
  let url = url
    .into_url()
    .map_err(|error| format!("failed to build SoundCloud API URL: {error}"))?;
  let auth_header = token_provider.authorization_header()?;
  match send_soundcloud_get_bytes(client, &auth_header, url.clone()) {
    Ok(value) => Ok(value),
    Err(SoundCloudApiError::Unauthorized) => {
      token_provider.refresh_now()?;
      let auth_header = token_provider.authorization_header()?;
      send_soundcloud_get_bytes(client, &auth_header, url).map_err(SoundCloudApiError::into_message)
    }
    Err(error) => Err(error.into_message()),
  }
}

enum SoundCloudApiError {
  Unauthorized,
  Other(String),
}

impl SoundCloudApiError {
  fn into_message(self) -> String {
    match self {
      Self::Unauthorized => "SoundCloud API returned an unauthorized response after token refresh.".to_owned(),
      Self::Other(message) => message,
    }
  }
}

fn send_soundcloud_get_json<T>(
  client: &reqwest::blocking::Client,
  auth_header: &str,
  url: reqwest::Url,
) -> Result<T, SoundCloudApiError>
where
  T: for<'de> Deserialize<'de>,
{
  let response = client
    .get(url)
    .header(reqwest::header::ACCEPT, "application/json; charset=utf-8")
    .header(reqwest::header::AUTHORIZATION, auth_header)
    .send()
    .map_err(|error| SoundCloudApiError::Other(format!("failed to call SoundCloud API: {error}")))?;

  if response.status() == reqwest::StatusCode::UNAUTHORIZED {
    return Err(SoundCloudApiError::Unauthorized);
  }

  response
    .error_for_status()
    .map_err(|error| SoundCloudApiError::Other(format!("SoundCloud API returned an error: {error}")))?
    .text()
    .map_err(|error| SoundCloudApiError::Other(format!("failed to read SoundCloud API response: {error}")))
    .and_then(|body| {
      serde_json::from_str(&body).map_err(|error| {
        SoundCloudApiError::Other(format!(
          "failed to parse SoundCloud API response: {error}; body starts with: {}",
          response_snippet(&body)
        ))
      })
    })
}

fn send_soundcloud_get_text(
  client: &reqwest::blocking::Client,
  auth_header: &str,
  url: reqwest::Url,
) -> Result<String, SoundCloudApiError> {
  let response = client
    .get(url)
    .header(reqwest::header::AUTHORIZATION, auth_header)
    .send()
    .map_err(|error| SoundCloudApiError::Other(format!("failed to call SoundCloud API: {error}")))?;

  if response.status() == reqwest::StatusCode::UNAUTHORIZED {
    return Err(SoundCloudApiError::Unauthorized);
  }

  response
    .error_for_status()
    .map_err(|error| SoundCloudApiError::Other(format!("SoundCloud API returned an error: {error}")))?
    .text()
    .map_err(|error| SoundCloudApiError::Other(format!("failed to read SoundCloud API response: {error}")))
}

fn send_soundcloud_get_bytes(
  client: &reqwest::blocking::Client,
  auth_header: &str,
  url: reqwest::Url,
) -> Result<Vec<u8>, SoundCloudApiError> {
  let response = client
    .get(url)
    .header(reqwest::header::AUTHORIZATION, auth_header)
    .send()
    .map_err(|error| SoundCloudApiError::Other(format!("failed to call SoundCloud API: {error}")))?;

  if response.status() == reqwest::StatusCode::UNAUTHORIZED {
    return Err(SoundCloudApiError::Unauthorized);
  }

  let response = response
    .error_for_status()
    .map_err(|error| SoundCloudApiError::Other(format!("SoundCloud API returned an error: {error}")))?;

  if response
    .content_length()
    .is_some_and(|length| length > MAX_REMOTE_AUDIO_BYTES)
  {
    return Err(SoundCloudApiError::Other(
      "audio stream is too large for the in-memory decoder.".to_owned(),
    ));
  }

  let bytes = response
    .bytes()
    .map_err(|error| SoundCloudApiError::Other(format!("failed to read SoundCloud API response: {error}")))?;
  if bytes.len() > MAX_REMOTE_AUDIO_BYTES as usize {
    return Err(SoundCloudApiError::Other(
      "audio stream is too large for the in-memory decoder.".to_owned(),
    ));
  }

  Ok(bytes.to_vec())
}

fn response_snippet(body: &str) -> String {
  const MAX_SNIPPET_LEN: usize = 160;
  body
    .chars()
    .take(MAX_SNIPPET_LEN)
    .collect::<String>()
    .replace(['\r', '\n'], " ")
}

fn soundcloud_auth_header(token_type: Option<&str>, access_token: &str) -> String {
  let _token_type = token_type;
  format!("OAuth {access_token}")
}

fn download_remote_audio(
  client: &reqwest::blocking::Client,
  token_provider: &SoundCloudTokenProvider,
  url: &str,
) -> Result<Vec<u8>, String> {
  if is_soundcloud_api_url(url) {
    soundcloud_get_bytes(client, token_provider, url)
  } else {
    download_remote_bytes(client, url)
  }
}

fn download_hls_audio(
  client: &reqwest::blocking::Client,
  token_provider: &SoundCloudTokenProvider,
  playlist_url: &str,
) -> Result<DownloadedAudio, String> {
  download_hls_playlist(client, token_provider, playlist_url, 0)
}

fn download_hls_playlist(
  client: &reqwest::blocking::Client,
  token_provider: &SoundCloudTokenProvider,
  playlist_url: &str,
  depth: usize,
) -> Result<DownloadedAudio, String> {
  if depth > MAX_HLS_PLAYLIST_DEPTH {
    return Err("SoundCloud HLS playlist nested too deeply.".to_owned());
  }

  let base_url =
    reqwest::Url::parse(playlist_url).map_err(|error| format!("failed to parse HLS playlist URL: {error}"))?;
  let playlist = if is_soundcloud_api_url(base_url.as_str()) {
    soundcloud_get_text(client, token_provider, base_url.clone())?
  } else {
    client
      .get(base_url.clone())
      .send()
      .map_err(|error| format!("failed to open HLS playlist: {error}"))?
      .error_for_status()
      .map_err(|error| format!("failed to open HLS playlist: {error}"))?
      .text()
      .map_err(|error| format!("failed to read HLS playlist: {error}"))?
  };

  if playlist.lines().any(|line| line.trim().starts_with("#EXT-X-KEY")) {
    return Err("Encrypted SoundCloud HLS streams are not supported yet.".to_owned());
  }

  if let Some(variant_url) = select_hls_variant(&playlist, &base_url)? {
    return download_hls_playlist(client, token_provider, variant_url.as_str(), depth + 1);
  }

  let mut bytes = Vec::new();
  let mut container_hint = None;
  if let Some(init_url) = hls_init_segment_url(&playlist, &base_url)? {
    append_remote_bytes(client, init_url.as_str(), &mut bytes)?;
    container_hint = infer_audio_extension(init_url.as_str()).or(Some("m4a".to_owned()));
  }

  for segment_url in hls_segment_urls(&playlist, &base_url)? {
    append_remote_bytes(client, segment_url.as_str(), &mut bytes)?;
    if container_hint.is_none() {
      container_hint = infer_audio_extension(segment_url.as_str());
    }
  }

  if bytes.is_empty() {
    return Err("SoundCloud HLS playlist did not contain downloadable audio segments.".to_owned());
  }

  Ok(DownloadedAudio {
    bytes,
    container_hint: container_hint
      .or_else(|| infer_audio_extension(playlist_url))
      .or(Some("m4a".to_owned())),
  })
}

fn select_hls_variant(playlist: &str, base_url: &reqwest::Url) -> Result<Option<reqwest::Url>, String> {
  let mut next_line_is_variant = false;
  for line in playlist.lines().map(str::trim) {
    if line.is_empty() {
      continue;
    }
    if next_line_is_variant && !line.starts_with('#') {
      return base_url
        .join(line)
        .map(Some)
        .map_err(|error| format!("failed to resolve HLS variant URL: {error}"));
    }
    next_line_is_variant = line.starts_with("#EXT-X-STREAM-INF");
  }
  Ok(None)
}

fn hls_init_segment_url(playlist: &str, base_url: &reqwest::Url) -> Result<Option<reqwest::Url>, String> {
  for line in playlist.lines().map(str::trim) {
    let Some(attributes) = line.strip_prefix("#EXT-X-MAP:") else {
      continue;
    };
    let Some(uri) = hls_attribute(attributes, "URI") else {
      continue;
    };
    return base_url
      .join(&uri)
      .map(Some)
      .map_err(|error| format!("failed to resolve HLS init segment URL: {error}"));
  }
  Ok(None)
}

fn hls_segment_urls(playlist: &str, base_url: &reqwest::Url) -> Result<Vec<reqwest::Url>, String> {
  playlist
    .lines()
    .map(str::trim)
    .filter(|line| !line.is_empty() && !line.starts_with('#'))
    .map(|line| {
      base_url
        .join(line)
        .map_err(|error| format!("failed to resolve HLS segment URL: {error}"))
    })
    .collect()
}

fn hls_attribute(attributes: &str, name: &str) -> Option<String> {
  let prefix = format!("{name}=\"");
  let start = attributes.find(&prefix)? + prefix.len();
  let value = attributes[start..].split('"').next()?;
  Some(value.to_owned())
}

fn append_remote_bytes(client: &reqwest::blocking::Client, url: &str, output: &mut Vec<u8>) -> Result<(), String> {
  let bytes = download_remote_bytes(client, url)?;
  if output.len() + bytes.len() > MAX_REMOTE_AUDIO_BYTES as usize {
    return Err("audio stream is too large for the in-memory decoder.".to_owned());
  }
  output.extend(bytes);
  Ok(())
}

fn download_remote_bytes(client: &reqwest::blocking::Client, url: &str) -> Result<Vec<u8>, String> {
  let response = client
    .get(url)
    .send()
    .map_err(|error| format!("failed to open audio stream: {error}"))?
    .error_for_status()
    .map_err(|error| format!("failed to open audio stream: {error}"))?;

  if response
    .content_length()
    .is_some_and(|length| length > MAX_REMOTE_AUDIO_BYTES)
  {
    return Err("audio stream is too large for the in-memory decoder.".to_owned());
  }

  let bytes = response
    .bytes()
    .map_err(|error| format!("failed to read audio stream: {error}"))?;
  if bytes.len() > MAX_REMOTE_AUDIO_BYTES as usize {
    return Err("audio stream is too large for the in-memory decoder.".to_owned());
  }

  Ok(bytes.to_vec())
}

fn is_hls_url(url: &str) -> bool {
  url.to_ascii_lowercase().contains(".m3u8")
}

fn is_soundcloud_api_url(url: &str) -> bool {
  reqwest::Url::parse(url)
    .ok()
    .and_then(|url| url.host_str().map(str::to_ascii_lowercase))
    .is_some_and(|host| host == "api.soundcloud.com")
}

fn infer_audio_extension(value: &str) -> Option<String> {
  let value = value.to_ascii_lowercase();
  if value.contains("m4a") || value.contains("aac") || value.contains("mp4") {
    Some("m4a".to_owned())
  } else if value.contains("mp3") || value.contains("mpeg") {
    Some("mp3".to_owned())
  } else {
    None
  }
}

pub(crate) fn is_soundcloud_url(input: &str) -> bool {
  reqwest::Url::parse(input)
    .ok()
    .and_then(|url| url.host_str().map(str::to_ascii_lowercase))
    .is_some_and(|host| host == "soundcloud.com" || host.ends_with(".soundcloud.com"))
}
