use crate::sources::model::{ResolvedAudio, SourceRequest};

pub(crate) trait SourceResolver {
  fn supports(&self, input: &str) -> bool;
  fn request(&self, input: &str) -> Option<SourceRequest>;
  fn resolve(&self, request: &SourceRequest) -> Result<ResolvedAudio, String>;
}
