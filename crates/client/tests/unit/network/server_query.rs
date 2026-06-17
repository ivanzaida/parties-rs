use super::*;
use crate::network::protocol::{BinaryWriter, PROTOCOL_VERSION};

#[test]
fn query_request_matches_cpp_shape() {
  let request = build_server_query_request(0x11223344);
  assert_eq!(request.len(), SERVER_QUERY_REQUEST_SIZE);
  assert_eq!(&request[..8], &SERVER_QUERY_MAGIC);
  assert_eq!(&request[8..12], &[0x44, 0x33, 0x22, 0x11]);
  assert!(request[12..].iter().all(|byte| *byte == 0));
}

#[test]
fn query_reply_parses_cpp_shape() {
  let token = 0x01020304;
  let mut w = BinaryWriter::new();
  w.write_bytes(&SERVER_QUERY_REPLY_MARKER);
  w.write_u32(token);
  w.write_u16(PROTOCOL_VERSION);
  w.write_u16(2);
  w.write_u16(16);
  w.write_u8(SERVER_QUERY_FLAG_PASSWORD_LOCKED);
  w.write_string("My Server").unwrap();
  w.write_u16(42);

  assert_eq!(
    parse_server_query_reply(&w.into_bytes(), token),
    Some(ServerQueryInfo {
      protocol_version: PROTOCOL_VERSION,
      server_name: "My Server".to_owned(),
      current_users: 2,
      max_users: 16,
      total_users: Some(42),
      password_locked: true,
    })
  );
}

#[test]
fn query_reply_parses_legacy_shape_without_total_users() {
  let token = 0x01020304;
  let mut w = BinaryWriter::new();
  w.write_bytes(&SERVER_QUERY_REPLY_MARKER);
  w.write_u32(token);
  w.write_u16(PROTOCOL_VERSION);
  w.write_u16(2);
  w.write_u16(16);
  w.write_u8(SERVER_QUERY_FLAG_PASSWORD_LOCKED);
  w.write_string("My Server").unwrap();

  assert_eq!(
    parse_server_query_reply(&w.into_bytes(), token),
    Some(ServerQueryInfo {
      protocol_version: PROTOCOL_VERSION,
      server_name: "My Server".to_owned(),
      current_users: 2,
      max_users: 16,
      total_users: None,
      password_locked: true,
    })
  );
}

#[test]
fn query_reply_rejects_wrong_token() {
  let mut w = BinaryWriter::new();
  w.write_bytes(&SERVER_QUERY_REPLY_MARKER);
  w.write_u32(1);
  w.write_u16(1);
  w.write_u16(0);
  w.write_u16(0);
  w.write_u8(0);
  w.write_string("My Server").unwrap();

  assert_eq!(parse_server_query_reply(&w.into_bytes(), 2), None);
}
