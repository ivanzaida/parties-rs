use crate::session::ServerSession;

pub(super) fn session_address(session: &ServerSession) -> Option<String> {
  session.info().map(|info| info.address)
}

pub(super) fn optional_session_address(session: Option<&ServerSession>) -> Option<String> {
  session.and_then(session_address)
}

pub(super) fn same_session(left: &ServerSession, right: &ServerSession) -> bool {
  session_address(left) == session_address(right)
}

pub(super) fn same_optional_session(left: Option<&ServerSession>, right: Option<&ServerSession>) -> bool {
  match (left, right) {
    (Some(left), Some(right)) => same_session(left, right),
    (None, None) => true,
    _ => false,
  }
}
