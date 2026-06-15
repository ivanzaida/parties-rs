#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SourceKind {
  SoundCloud,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SourceRequest {
  pub(crate) kind: SourceKind,
  pub(crate) url: String,
  pub(crate) loading_title: String,
}

pub(crate) struct ResolvedAudio {
  pub(crate) title: String,
  pub(crate) source_kind: SourceKind,
  pub(crate) source_url: String,
  pub(crate) payload: ResolvedAudioPayload,
}

pub(crate) enum ResolvedAudioPayload {
  Buffered {
    bytes: Vec<u8>,
    container_hint: Option<String>,
  },
}
