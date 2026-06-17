#[cfg(test)]
use std::sync::Arc;

use crate::sources::{
  model::{ResolvedAudio, SourceKind, SourceRequest},
  resolver::SourceResolver,
  soundcloud::{SoundCloudResolver, SoundCloudTokenProvider},
};

#[derive(Clone)]
pub(crate) struct SourceRegistry {
  soundcloud: SoundCloudResolver,
  #[cfg(test)]
  test_backend: Option<Arc<dyn TestSourceBackend>>,
}

impl SourceRegistry {
  pub(crate) fn new(soundcloud_tokens: SoundCloudTokenProvider) -> Self {
    Self {
      soundcloud: SoundCloudResolver::new(soundcloud_tokens),
      #[cfg(test)]
      test_backend: None,
    }
  }

  #[cfg(test)]
  pub(crate) fn new_for_tests(test_backend: Arc<dyn TestSourceBackend>) -> Self {
    Self {
      soundcloud: SoundCloudResolver::new(SoundCloudTokenProvider::new_for_tests("Bearer test")),
      test_backend: Some(test_backend),
    }
  }

  pub(crate) fn supports(&self, input: &str) -> bool {
    self.soundcloud.supports(input)
  }

  #[cfg(test)]
  pub(crate) fn parse(&self, input: &str) -> Result<SourceRequest, String> {
    let input = input.trim();
    #[cfg(test)]
    if let Some(test_backend) = self.test_backend.as_ref() {
      return test_backend.parse(input);
    }

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
    #[cfg(test)]
    if let Some(test_backend) = self.test_backend.as_ref() {
      return test_backend.parse_many(input);
    }

    if self.soundcloud.supports(input) {
      return self.soundcloud.requests(input);
    }

    Err("Only SoundCloud URLs are supported right now.".to_owned())
  }

  pub(crate) fn resolve(&self, request: &SourceRequest) -> Result<ResolvedAudio, String> {
    #[cfg(test)]
    if let Some(test_backend) = self.test_backend.as_ref() {
      return test_backend.resolve(request);
    }

    match request.kind {
      SourceKind::SoundCloud => self.soundcloud.resolve(request),
    }
  }

  pub(crate) fn shutdown(&self) {
    self.soundcloud.shutdown();
  }
}

#[cfg(test)]
pub(crate) trait TestSourceBackend: Send + Sync {
  fn parse(&self, input: &str) -> Result<SourceRequest, String>;
  fn parse_many(&self, input: &str) -> Result<Vec<SourceRequest>, String>;
  fn resolve(&self, request: &SourceRequest) -> Result<ResolvedAudio, String>;
}
