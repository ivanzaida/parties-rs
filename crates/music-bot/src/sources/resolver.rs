use crate::sources::model::{ResolvedAudio, SourceRequest};

pub(crate) trait SourceResolver {
  fn supports(&self, input: &str) -> bool;
  fn request(&self, input: &str) -> Option<SourceRequest>;
  fn requests(&self, input: &str) -> Result<Vec<SourceRequest>, String> {
    self
      .request(input)
      .map(|request| vec![request])
      .ok_or_else(|| "source URL could not be parsed.".to_owned())
  }
  fn resolve(&self, request: &SourceRequest) -> Result<ResolvedAudio, String>;
}
