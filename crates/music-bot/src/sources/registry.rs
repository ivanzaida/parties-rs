use crate::sources::{
  model::{ResolvedAudio, SourceKind, SourceRequest},
  resolver::SourceResolver,
  soundcloud::{SoundCloudResolver, SoundCloudTokenProvider},
};

#[derive(Clone)]
pub(crate) struct SourceRegistry {
  soundcloud: SoundCloudResolver,
}

impl SourceRegistry {
  pub(crate) fn new(soundcloud_tokens: SoundCloudTokenProvider) -> Self {
    Self {
      soundcloud: SoundCloudResolver::new(soundcloud_tokens),
    }
  }

  #[cfg(test)]
  pub(crate) fn parse(&self, input: &str) -> Result<SourceRequest, String> {
    let input = input.trim();
    if self.soundcloud.supports(input) {
      return self
        .soundcloud
        .request(input)
        .ok_or_else(|| "SoundCloud URL could not be parsed.".to_owned());
    }

    Err("Only SoundCloud URLs are supported right now.".to_owned())
  }

  pub(crate) fn parse_many(&self, input: &str) -> Result<Vec<SourceRequest>, String> {
    let input = input.trim();
    if self.soundcloud.supports(input) {
      return self.soundcloud.requests(input);
    }

    Err("Only SoundCloud URLs are supported right now.".to_owned())
  }

  pub(crate) fn resolve(&self, request: &SourceRequest) -> Result<ResolvedAudio, String> {
    match request.kind {
      SourceKind::SoundCloud => self.soundcloud.resolve(request),
    }
  }

  pub(crate) fn shutdown(&self) {
    self.soundcloud.shutdown();
  }
}
