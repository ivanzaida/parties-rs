use super::{
  ServerQueryEntry, ServerQueryState, initials_for, query_result_for, server_card::ServerCardLiveState,
  server_live_info, server_query_signature,
};
use crate::{
  network::{protocol::Role, server_query::ServerQueryInfo},
  storage::StoredServer,
};

fn stored_server(address: &str) -> StoredServer {
  StoredServer {
    address: address.to_owned(),
    server_name: String::new(),
    user_id: 1,
    role: Role::User,
    certificate_fingerprint: String::new(),
    server_password: String::new(),
    display_name: String::new(),
  }
}

fn online_info() -> ServerQueryInfo {
  ServerQueryInfo {
    protocol_version: 9,
    server_name: "Live server".to_owned(),
    current_users: 4,
    max_users: 12,
    total_users: Some(5),
    password_locked: true,
  }
}

#[test]
fn server_query_signature_tracks_order_and_duplicates() {
  let servers = vec![
    stored_server("alpha.example:7800"),
    stored_server("beta.example:7800"),
    stored_server("alpha.example:7800"),
  ];

  assert_eq!(
    server_query_signature(&servers),
    "alpha.example:7800\nbeta.example:7800\nalpha.example:7800"
  );
  assert_eq!(server_query_signature(&[]), "");
}

#[test]
fn query_result_for_returns_matching_state_by_address() {
  let results = vec![
    ServerQueryEntry {
      address: "alpha.example:7800".to_owned(),
      state: ServerQueryState::NoResponse,
    },
    ServerQueryEntry {
      address: "beta.example:7800".to_owned(),
      state: ServerQueryState::Online(online_info()),
    },
  ];

  assert!(matches!(
    query_result_for(&results, "alpha.example:7800"),
    Some(ServerQueryState::NoResponse)
  ));
  assert!(matches!(
    query_result_for(&results, "beta.example:7800"),
    Some(ServerQueryState::Online(_))
  ));
  assert!(query_result_for(&results, "missing.example:7800").is_none());
}

#[test]
fn server_live_info_maps_online_query_data() {
  let info = online_info();
  let live = server_live_info(Some(&ServerQueryState::Online(info.clone())), false);

  assert!(matches!(live.state, ServerCardLiveState::Online));
  assert_eq!(live.server_name.as_deref(), Some(info.server_name.as_str()));
  assert_eq!(live.current_users, Some(info.current_users));
  assert_eq!(live.max_users, Some(info.max_users));
  assert_eq!(live.protocol_version, Some(info.protocol_version));
  assert!(live.password_locked);
}

#[test]
fn server_live_info_distinguishes_checking_no_response_and_unknown() {
  let no_response = server_live_info(Some(&ServerQueryState::NoResponse), true);
  let checking = server_live_info(None, true);
  let unknown = server_live_info(None, false);

  assert!(matches!(no_response.state, ServerCardLiveState::NoResponse));
  assert!(no_response.server_name.is_none());
  assert!(no_response.current_users.is_none());
  assert!(no_response.max_users.is_none());
  assert!(no_response.protocol_version.is_none());
  assert!(!no_response.password_locked);
  assert!(matches!(checking.state, ServerCardLiveState::Checking));
  assert!(matches!(unknown.state, ServerCardLiveState::Unknown));
}

#[test]
fn initials_for_uses_first_two_alphanumeric_characters() {
  assert_eq!(initials_for(" lurq master ", "??"), "LU");
  assert_eq!(initials_for("9 lives", "??"), "9L");
  assert_eq!(initials_for("!!!", "??"), "??");
}
