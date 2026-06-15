use crate::{
  audio,
  config::SoundCloudConfig,
  sources::{
    model::ResolvedAudioPayload,
    registry::SourceRegistry,
    soundcloud::{self, SoundCloudTokenProvider},
  },
};

pub struct SoundCloudProbe {
  pub title: String,
  pub source_url: String,
  pub container_hint: Option<String>,
  pub byte_len: usize,
  pub decoded_samples: usize,
}

pub struct SoundCloudQueueProbe {
  pub title: String,
  pub url: String,
}

pub fn probe_soundcloud_url(url: &str, client_id: &str, client_secret: &str) -> Result<SoundCloudProbe, String> {
  let token_provider = SoundCloudTokenProvider::new(SoundCloudConfig {
    client_id: client_id.to_owned(),
    client_secret: client_secret.to_owned(),
  })?;
  let resolved = soundcloud::resolve_audio(url, &token_provider);
  token_provider.shutdown();

  let resolved = resolved?;
  let ResolvedAudioPayload::Buffered { bytes, container_hint } = resolved.payload;
  let byte_len = bytes.len();
  let decoded_samples = audio::probe_decoded_sample_count(bytes, container_hint.as_deref())?;
  Ok(SoundCloudProbe {
    title: resolved.title,
    source_url: resolved.source_url,
    container_hint,
    byte_len,
    decoded_samples,
  })
}

pub fn probe_soundcloud_queue(
  url: &str,
  client_id: &str,
  client_secret: &str,
) -> Result<Vec<SoundCloudQueueProbe>, String> {
  let token_provider = SoundCloudTokenProvider::new(SoundCloudConfig {
    client_id: client_id.to_owned(),
    client_secret: client_secret.to_owned(),
  })?;
  let sources = SourceRegistry::new(token_provider);
  let requests = sources.parse_many(url);
  sources.shutdown();

  requests.map(|requests| {
    requests
      .into_iter()
      .map(|request| SoundCloudQueueProbe {
        title: request.loading_title,
        url: request.url,
      })
      .collect()
  })
}
