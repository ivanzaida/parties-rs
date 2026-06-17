use std::{
  io,
  net::{SocketAddr, UdpSocket},
  time::{Duration, SystemTime, UNIX_EPOCH},
};

use super::protocol::BinaryReader;

const SERVER_QUERY_MAGIC: [u8; 8] = [0xC0, b'P', b'A', b'R', b'T', b'Y', b'Q', b'1'];
const SERVER_QUERY_REPLY_MARKER: [u8; 4] = [b'P', b'Q', b'R', b'1'];
const SERVER_QUERY_TOKEN_OFFSET: usize = 8;
const SERVER_QUERY_REQUEST_SIZE: usize = 256;
const SERVER_QUERY_FLAG_PASSWORD_LOCKED: u8 = 0x01;

#[derive(Debug, Clone, PartialEq, Eq, lurq::DevtoolsInspectable)]
pub struct ServerQueryInfo {
  pub protocol_version: u16,
  pub server_name: String,
  pub current_users: u16,
  pub max_users: u16,
  pub total_users: Option<u16>,
  pub password_locked: bool,
}

pub async fn query_server(addr: SocketAddr, timeout: Duration) -> io::Result<Option<ServerQueryInfo>> {
  tokio::task::spawn_blocking(move || query_server_blocking(addr, timeout))
    .await
    .map_err(|error| io::Error::new(io::ErrorKind::Other, error.to_string()))?
}

fn query_server_blocking(addr: SocketAddr, timeout: Duration) -> io::Result<Option<ServerQueryInfo>> {
  let token = query_token()?;
  let request = build_server_query_request(token);
  let bind_addr = if addr.is_ipv6() {
    SocketAddr::from(([0, 0, 0, 0, 0, 0, 0, 0], 0))
  } else {
    SocketAddr::from(([0, 0, 0, 0], 0))
  };

  let socket = UdpSocket::bind(bind_addr)?;
  socket.set_read_timeout(Some(timeout))?;
  socket.set_write_timeout(Some(timeout))?;

  let sent = socket.send_to(&request, addr)?;
  if sent != request.len() {
    return Ok(None);
  }

  let mut buf = [0u8; 1500];
  match socket.recv_from(&mut buf) {
    Ok((len, _)) => Ok(parse_server_query_reply(&buf[..len], token)),
    Err(error) if matches!(error.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut) => Ok(None),
    Err(error) => Err(error),
  }
}

fn query_token() -> io::Result<u32> {
  let mut raw = [0u8; 4];
  if getrandom::fill(&mut raw).is_ok() {
    return Ok(u32::from_le_bytes(raw));
  }

  let nanos = SystemTime::now()
    .duration_since(UNIX_EPOCH)
    .map_err(|error| io::Error::new(io::ErrorKind::Other, error.to_string()))?
    .as_nanos();
  Ok((nanos as u32) ^ ((nanos >> 32) as u32))
}

fn build_server_query_request(token: u32) -> Vec<u8> {
  let mut request = vec![0; SERVER_QUERY_REQUEST_SIZE];
  request[..SERVER_QUERY_MAGIC.len()].copy_from_slice(&SERVER_QUERY_MAGIC);
  request[SERVER_QUERY_TOKEN_OFFSET..SERVER_QUERY_TOKEN_OFFSET + 4].copy_from_slice(&token.to_le_bytes());
  request
}

fn parse_server_query_reply(bytes: &[u8], expected_token: u32) -> Option<ServerQueryInfo> {
  if bytes.len() < SERVER_QUERY_REPLY_MARKER.len() || bytes[..4] != SERVER_QUERY_REPLY_MARKER {
    return None;
  }

  let mut r = BinaryReader::new(bytes);
  let _marker = r.read_bytes(SERVER_QUERY_REPLY_MARKER.len()).ok()?;
  let token = r.read_u32().ok()?;
  if token != expected_token {
    return None;
  }

  let protocol_version = r.read_u16().ok()?;
  let current_users = r.read_u16().ok()?;
  let max_users = r.read_u16().ok()?;
  let flags = r.read_u8().ok()?;
  let server_name = r.read_string().ok()?;
  let total_users = if r.remaining() >= 2 {
    Some(r.read_u16().ok()?)
  } else {
    None
  };
  r.finish().ok()?;

  Some(ServerQueryInfo {
    protocol_version,
    server_name,
    current_users,
    max_users,
    total_users,
    password_locked: flags & SERVER_QUERY_FLAG_PASSWORD_LOCKED != 0,
  })
}

#[cfg(test)]
#[path = "../../tests/unit/network/server_query.rs"]
mod tests;
