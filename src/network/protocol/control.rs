use super::{
  BinaryReader, BinaryWriter, ChannelId, ChannelKeyBytes, DecodeError, DecodeResult, PublicKey, Role, SessionToken,
  Signature, UserId, VideoCodecId,
};

pub const MAX_CONTROL_MESSAGE_LEN: usize = 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum ControlMessageType {
  AuthIdentity = 0x0001,
  ChannelJoin = 0x0002,
  ChannelLeave = 0x0003,
  KeepalivePing = 0x0004,
  VoiceStateUpdate = 0x0005,
  ScreenShareStart = 0x0007,
  ScreenShareStop = 0x0008,
  ScreenShareView = 0x0009,
  ScreenShareUpdate = 0x000A,
  AuthResponse = 0x0101,
  ChannelList = 0x0102,
  ChannelUserList = 0x0103,
  UserJoinedChannel = 0x0104,
  UserLeftChannel = 0x0105,
  UserVoiceState = 0x0106,
  KeepalivePong = 0x0107,
  UserRoleChanged = 0x0108,
  ChannelKey = 0x0109,
  ScreenShareStarted = 0x010A,
  ScreenShareStopped = 0x010B,
  ScreenShareDenied = 0x010C,
  ServerError = 0x01FF,
  AdminCreateChannel = 0x0201,
  AdminDeleteChannel = 0x0202,
  AdminSetRole = 0x0203,
  AdminKickUser = 0x0204,
  AdminRenameChannel = 0x0205,
  AdminResult = 0x0301,
  ChatSend = 0x0401,
  ChatHistoryReq = 0x0402,
  ChatPin = 0x0403,
  ChatUnpin = 0x0404,
  ChatDelete = 0x0405,
  ChatFileUploadReq = 0x0406,
  ChatFileDownloadReq = 0x0407,
  ChatSearch = 0x0408,
  ChatPinnedReq = 0x0409,
  AdminCreateTextChannel = 0x040A,
  AdminDeleteTextChannel = 0x040B,
  ChatMessage = 0x0501,
  ChatHistoryResp = 0x0502,
  ChatMessageDeleted = 0x0503,
  ChatFileUploadResp = 0x0504,
  ChatFileReady = 0x0505,
  ChatSearchResp = 0x0506,
  ChatPinnedResp = 0x0507,
  ChatChannelList = 0x0508,
}

impl ControlMessageType {
  pub fn from_u16(value: u16) -> Option<Self> {
    use ControlMessageType::*;
    Some(match value {
      0x0001 => AuthIdentity,
      0x0002 => ChannelJoin,
      0x0003 => ChannelLeave,
      0x0004 => KeepalivePing,
      0x0005 => VoiceStateUpdate,
      0x0007 => ScreenShareStart,
      0x0008 => ScreenShareStop,
      0x0009 => ScreenShareView,
      0x000A => ScreenShareUpdate,
      0x0101 => AuthResponse,
      0x0102 => ChannelList,
      0x0103 => ChannelUserList,
      0x0104 => UserJoinedChannel,
      0x0105 => UserLeftChannel,
      0x0106 => UserVoiceState,
      0x0107 => KeepalivePong,
      0x0108 => UserRoleChanged,
      0x0109 => ChannelKey,
      0x010A => ScreenShareStarted,
      0x010B => ScreenShareStopped,
      0x010C => ScreenShareDenied,
      0x01FF => ServerError,
      0x0201 => AdminCreateChannel,
      0x0202 => AdminDeleteChannel,
      0x0203 => AdminSetRole,
      0x0204 => AdminKickUser,
      0x0205 => AdminRenameChannel,
      0x0301 => AdminResult,
      0x0401 => ChatSend,
      0x0402 => ChatHistoryReq,
      0x0403 => ChatPin,
      0x0404 => ChatUnpin,
      0x0405 => ChatDelete,
      0x0406 => ChatFileUploadReq,
      0x0407 => ChatFileDownloadReq,
      0x0408 => ChatSearch,
      0x0409 => ChatPinnedReq,
      0x040A => AdminCreateTextChannel,
      0x040B => AdminDeleteTextChannel,
      0x0501 => ChatMessage,
      0x0502 => ChatHistoryResp,
      0x0503 => ChatMessageDeleted,
      0x0504 => ChatFileUploadResp,
      0x0505 => ChatFileReady,
      0x0506 => ChatSearchResp,
      0x0507 => ChatPinnedResp,
      0x0508 => ChatChannelList,
      _ => return None,
    })
  }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlFrame {
  pub ty: ControlMessageType,
  pub payload: Vec<u8>,
}

impl ControlFrame {
  pub fn encode(&self) -> DecodeResult<Vec<u8>> {
    let msg_len = 2 + self.payload.len();
    if msg_len > MAX_CONTROL_MESSAGE_LEN {
      return Err(DecodeError::InvalidLength {
        len: msg_len,
        max: MAX_CONTROL_MESSAGE_LEN,
      });
    }

    let mut w = BinaryWriter::new();
    w.write_u32(msg_len as u32);
    w.write_u16(self.ty as u16);
    w.write_bytes(&self.payload);
    Ok(w.into_bytes())
  }

  pub fn decode(bytes: &[u8]) -> DecodeResult<Self> {
    let mut r = BinaryReader::new(bytes);
    let msg_len = r.read_u32()? as usize;
    if !(2..=MAX_CONTROL_MESSAGE_LEN).contains(&msg_len) {
      return Err(DecodeError::InvalidLength {
        len: msg_len,
        max: MAX_CONTROL_MESSAGE_LEN,
      });
    }
    let raw_ty = r.read_u16()?;
    let ty = ControlMessageType::from_u16(raw_ty).ok_or(DecodeError::InvalidMessageType(raw_ty))?;
    let payload = r.read_bytes(msg_len - 2)?.to_vec();
    r.finish()?;
    Ok(Self { ty, payload })
  }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthIdentity {
  pub protocol_version: u16,
  pub public_key: PublicKey,
  pub display_name: String,
  pub timestamp: u64,
  pub signature: Signature,
  pub password: String,
}

impl AuthIdentity {
  pub fn encode_payload(&self) -> DecodeResult<Vec<u8>> {
    let mut w = BinaryWriter::new();
    w.write_u16(self.protocol_version);
    w.write_bytes(&self.public_key);
    w.write_string(&self.display_name)?;
    w.write_u64(self.timestamp);
    w.write_bytes(&self.signature);
    w.write_string(&self.password)?;
    Ok(w.into_bytes())
  }

  pub fn encode_legacy_payload(&self) -> DecodeResult<Vec<u8>> {
    let mut w = BinaryWriter::new();
    w.write_bytes(&self.public_key);
    w.write_string(&self.display_name)?;
    w.write_u64(self.timestamp);
    w.write_bytes(&self.signature);
    w.write_string(&self.password)?;
    Ok(w.into_bytes())
  }

  pub fn signed_payload(public_key: &PublicKey, display_name: &str, timestamp: u64) -> DecodeResult<Vec<u8>> {
    let mut w = BinaryWriter::new();
    w.write_bytes(public_key);
    w.write_string(display_name)?;
    w.write_u64(timestamp);
    Ok(w.into_bytes())
  }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthResponse {
  pub user_id: UserId,
  pub session_token: SessionToken,
  pub role: Role,
  pub server_name: String,
}

impl AuthResponse {
  pub fn decode_payload(bytes: &[u8]) -> DecodeResult<Self> {
    let mut r = BinaryReader::new(bytes);
    let user_id = r.read_u32()?;
    let session_token = r.read_array()?;
    let raw_role = r.read_u8()?;
    let role = Role::from_u8(raw_role).ok_or(DecodeError::InvalidEnumValue {
      field: "role",
      value: raw_role,
    })?;
    let server_name = r.read_string()?;
    r.finish()?;
    Ok(Self {
      user_id,
      session_token,
      role,
      server_name,
    })
  }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelInfo {
  pub id: ChannelId,
  pub name: String,
  pub max_users: u32,
  pub sort_order: u32,
  pub user_count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelList {
  pub channels: Vec<ChannelInfo>,
}

impl ChannelList {
  pub fn decode_payload(bytes: &[u8]) -> DecodeResult<Self> {
    let mut r = BinaryReader::new(bytes);
    let count = r.read_u32()? as usize;
    let mut channels = Vec::with_capacity(count);
    for _ in 0..count {
      channels.push(ChannelInfo {
        id: r.read_u32()?,
        name: r.read_string()?,
        max_users: r.read_u32()?,
        sort_order: r.read_u32()?,
        user_count: r.read_u32()?,
      });
    }
    r.finish()?;
    Ok(Self { channels })
  }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelUser {
  pub user_id: UserId,
  pub username: String,
  pub role: Role,
  pub muted: bool,
  pub deafened: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelUserList {
  pub channel_id: ChannelId,
  pub users: Vec<ChannelUser>,
}

impl ChannelUserList {
  pub fn decode_payload(bytes: &[u8]) -> DecodeResult<Self> {
    let mut r = BinaryReader::new(bytes);
    let channel_id = r.read_u32()?;
    let count = r.read_u32()? as usize;
    let mut users = Vec::with_capacity(count);
    for _ in 0..count {
      let raw_role = r.read_u8_after_user_string()?;
      let (user_id, username, role) = raw_role;
      users.push(ChannelUser {
        user_id,
        username,
        role,
        muted: r.read_u8()? != 0,
        deafened: r.read_u8()? != 0,
      });
    }
    r.finish()?;
    Ok(Self { channel_id, users })
  }
}

trait ChannelUserReadExt {
  fn read_u8_after_user_string(&mut self) -> DecodeResult<(UserId, String, Role)>;
}

impl ChannelUserReadExt for BinaryReader<'_> {
  fn read_u8_after_user_string(&mut self) -> DecodeResult<(UserId, String, Role)> {
    let user_id = self.read_u32()?;
    let username = self.read_string()?;
    let raw_role = self.read_u8()?;
    let role = Role::from_u8(raw_role).ok_or(DecodeError::InvalidEnumValue {
      field: "role",
      value: raw_role,
    })?;
    Ok((user_id, username, role))
  }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserJoinedChannel {
  pub user_id: UserId,
  pub username: String,
  pub channel_id: ChannelId,
  pub role: Role,
}

impl UserJoinedChannel {
  pub fn decode_payload(bytes: &[u8]) -> DecodeResult<Self> {
    let mut r = BinaryReader::new(bytes);
    let user_id = r.read_u32()?;
    let username = r.read_string()?;
    let channel_id = r.read_u32()?;
    let role = if r.has_remaining(1) {
      let raw_role = r.read_u8()?;
      Role::from_u8(raw_role).ok_or(DecodeError::InvalidEnumValue {
        field: "role",
        value: raw_role,
      })?
    } else {
      Role::User
    };
    r.finish()?;
    Ok(Self {
      user_id,
      username,
      channel_id,
      role,
    })
  }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserLeftChannel {
  pub user_id: UserId,
  pub channel_id: ChannelId,
}

impl UserLeftChannel {
  pub fn decode_payload(bytes: &[u8]) -> DecodeResult<Self> {
    let mut r = BinaryReader::new(bytes);
    let user_id = r.read_u32()?;
    let channel_id = r.read_u32()?;
    r.finish()?;
    Ok(Self { user_id, channel_id })
  }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoiceState {
  pub muted: bool,
  pub deafened: bool,
}

impl VoiceState {
  pub fn encode_payload(&self) -> Vec<u8> {
    vec![self.muted as u8, self.deafened as u8]
  }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserVoiceState {
  pub user_id: UserId,
  pub muted: bool,
  pub deafened: bool,
}

impl UserVoiceState {
  pub fn decode_payload(bytes: &[u8]) -> DecodeResult<Self> {
    let mut r = BinaryReader::new(bytes);
    let user_id = r.read_u32()?;
    let muted = r.read_u8()? != 0;
    let deafened = r.read_u8()? != 0;
    r.finish()?;
    Ok(Self {
      user_id,
      muted,
      deafened,
    })
  }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserRoleChanged {
  pub user_id: UserId,
  pub role: Role,
}

impl UserRoleChanged {
  pub fn decode_payload(bytes: &[u8]) -> DecodeResult<Self> {
    let mut r = BinaryReader::new(bytes);
    let user_id = r.read_u32()?;
    let raw_role = r.read_u8()?;
    let role = Role::from_u8(raw_role).ok_or(DecodeError::InvalidEnumValue {
      field: "role",
      value: raw_role,
    })?;
    r.finish()?;
    Ok(Self { user_id, role })
  }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelKey {
  pub channel_id: ChannelId,
  pub key: ChannelKeyBytes,
}

impl ChannelKey {
  pub fn decode_payload(bytes: &[u8]) -> DecodeResult<Self> {
    let mut r = BinaryReader::new(bytes);
    let channel_id = r.read_u32()?;
    let key = r.read_array()?;
    r.finish()?;
    Ok(Self { channel_id, key })
  }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreenShareMetadata {
  pub codec: VideoCodecId,
  pub width: u16,
  pub height: u16,
}

impl ScreenShareMetadata {
  pub fn encode_payload(&self) -> Vec<u8> {
    let mut w = BinaryWriter::new();
    w.write_u8(self.codec as u8);
    w.write_u16(self.width);
    w.write_u16(self.height);
    w.into_bytes()
  }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreenShareStarted {
  pub sharer_user_id: UserId,
  pub metadata: ScreenShareMetadata,
}

impl ScreenShareStarted {
  pub fn decode_payload(bytes: &[u8]) -> DecodeResult<Self> {
    let mut r = BinaryReader::new(bytes);
    let sharer_user_id = r.read_u32()?;
    let raw_codec = r.read_u8()?;
    let codec = VideoCodecId::from_u8(raw_codec).ok_or(DecodeError::InvalidEnumValue {
      field: "video codec",
      value: raw_codec,
    })?;
    let width = r.read_u16()?;
    let height = r.read_u16()?;
    r.finish()?;
    Ok(Self {
      sharer_user_id,
      metadata: ScreenShareMetadata { codec, width, height },
    })
  }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdminResult {
  pub success: bool,
  pub message: String,
}

impl AdminResult {
  pub fn decode_payload(bytes: &[u8]) -> DecodeResult<Self> {
    let mut r = BinaryReader::new(bytes);
    let success = r.read_u8()? != 0;
    let message = r.read_string()?;
    r.finish()?;
    Ok(Self { success, message })
  }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextChannelInfo {
  pub id: ChannelId,
  pub name: String,
  pub sort_order: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatFileUploadResponse {
  pub message_id: u64,
  pub file_index: u8,
  pub accepted: bool,
  pub reason: String,
}

impl ChatFileUploadResponse {
  pub fn decode_payload(bytes: &[u8]) -> DecodeResult<Self> {
    let mut r = BinaryReader::new(bytes);
    let message_id = r.read_u64()?;
    let file_index = r.read_u8()?;
    let accepted = r.read_u8()? != 0;
    let reason = r.read_string()?;
    r.finish()?;
    Ok(Self {
      message_id,
      file_index,
      accepted,
      reason,
    })
  }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatSendAttachment {
  pub file_name: String,
  pub file_size: u64,
  pub mime_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatAttachment {
  pub id: u64,
  pub file_name: String,
  pub file_size: u64,
  pub mime_type: String,
  pub uploaded: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatMessage {
  pub id: u64,
  pub channel_id: ChannelId,
  pub sender_id: UserId,
  pub sender_name: String,
  pub timestamp: u64,
  pub text: String,
  pub pinned: bool,
  pub attachments: Vec<ChatAttachment>,
}

impl ChatMessage {
  pub fn decode_from(reader: &mut BinaryReader<'_>) -> DecodeResult<Self> {
    let id = reader.read_u64()?;
    let channel_id = reader.read_u32()?;
    let sender_id = reader.read_u32()?;
    let sender_name = reader.read_string()?;
    let timestamp = reader.read_u64()?;
    let text = reader.read_string()?;
    let pinned = reader.read_u8()? != 0;
    let attachment_count = reader.read_u8()? as usize;
    let mut attachments = Vec::with_capacity(attachment_count);
    for _ in 0..attachment_count {
      attachments.push(ChatAttachment {
        id: reader.read_u64()?,
        file_name: reader.read_string()?,
        file_size: reader.read_u64()?,
        mime_type: reader.read_string()?,
        uploaded: reader.read_u8()? != 0,
      });
    }
    Ok(Self {
      id,
      channel_id,
      sender_id,
      sender_name,
      timestamp,
      text,
      pinned,
      attachments,
    })
  }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatHistoryResponse {
  pub channel_id: ChannelId,
  pub has_more: bool,
  pub messages: Vec<ChatMessage>,
}

impl ChatHistoryResponse {
  pub fn decode_payload(bytes: &[u8]) -> DecodeResult<Self> {
    let mut r = BinaryReader::new(bytes);
    let channel_id = r.read_u32()?;
    let has_more = r.read_u8()? != 0;
    let count = r.read_u16()? as usize;
    let mut messages = Vec::with_capacity(count);
    for _ in 0..count {
      messages.push(ChatMessage::decode_from(&mut r)?);
    }
    r.finish()?;
    Ok(Self {
      channel_id,
      has_more,
      messages,
    })
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn control_frame_round_trips() {
    let frame = ControlFrame {
      ty: ControlMessageType::KeepalivePing,
      payload: Vec::new(),
    };

    let encoded = frame.encode().unwrap();
    assert_eq!(encoded, vec![2, 0, 0, 0, 4, 0]);
    assert_eq!(ControlFrame::decode(&encoded).unwrap(), frame);
  }

  #[test]
  fn auth_signed_payload_matches_upstream_shape() {
    let public_key = [7; 32];
    let payload = AuthIdentity::signed_payload(&public_key, "alice", 42).unwrap();
    assert_eq!(&payload[..32], &public_key);
    assert_eq!(&payload[32..39], &[5, 0, b'a', b'l', b'i', b'c', b'e']);
    assert_eq!(&payload[39..], &42_u64.to_le_bytes());
  }

  #[test]
  fn auth_payload_supports_versioned_and_legacy_shapes() {
    let auth = AuthIdentity {
      protocol_version: 1,
      public_key: [7; 32],
      display_name: "alice".to_owned(),
      timestamp: 42,
      signature: [9; 64],
      password: "secret".to_owned(),
    };

    let versioned = auth.encode_payload().unwrap();
    assert_eq!(&versioned[..2], &1_u16.to_le_bytes());
    assert_eq!(&versioned[2..34], &[7; 32]);

    let legacy = auth.encode_legacy_payload().unwrap();
    assert_eq!(&legacy[..32], &[7; 32]);
    assert_eq!(&legacy[32..39], &[5, 0, b'a', b'l', b'i', b'c', b'e']);
    assert_eq!(legacy.len(), versioned.len() - 2);
  }
}
