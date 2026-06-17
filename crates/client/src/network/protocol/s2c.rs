use super::{
  BinaryReader, ChannelId, ControlFrame, ControlMessageType, DecodeError, DecodeResult, Role, ServerErrorCode, UserId,
  VideoCodecId,
  control::{
    AdminResult, AuthResponse, ChannelList, ChannelUserList, ChatCommandList, ChatFileUploadResponse,
    ChatHistoryResponse, ChatMessage, ScreenShareMetadata, ScreenShareStarted, TextChannelInfo, UserJoinedChannel,
    UserLeftChannel, UserRoleChanged, UserVoiceState,
  },
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum S2C {
  AuthResponse(AuthResponse),
  ChannelList(ChannelList),
  ChannelUserList(ChannelUserList),
  UserJoinedChannel(UserJoinedChannel),
  UserLeftChannel(UserLeftChannel),
  UserVoiceState(UserVoiceState),
  KeepalivePong,
  UserRoleChanged(UserRoleChanged),
  ScreenShareStarted(ScreenShareStarted),
  ScreenShareStopped {
    sharer_user_id: UserId,
  },
  ScreenShareDenied {
    reason: String,
  },
  ServerError {
    code: ServerErrorCode,
    message: String,
  },
  AdminResult(AdminResult),
  ChatMessage(ChatMessage),
  ChatHistoryResp(ChatHistoryResponse),
  ChatMessageDeleted {
    message_id: u64,
    channel_id: ChannelId,
  },
  ChatFileUploadResp(ChatFileUploadResponse),
  ChatFileReady {
    message_id: u64,
    attachment_id: u64,
  },
  ChatSearchResp {
    channel_id: ChannelId,
    messages: Vec<ChatMessage>,
  },
  ChatPinnedResp {
    channel_id: ChannelId,
    messages: Vec<ChatMessage>,
  },
  ChatChannelList {
    channels: Vec<TextChannelInfo>,
  },
  ChatCommandList(ChatCommandList),
}

impl S2C {
  pub fn decode(frame: &ControlFrame) -> DecodeResult<Self> {
    use ControlMessageType as M;

    let bytes = &frame.payload;
    match frame.ty {
      M::AuthResponse => Ok(Self::AuthResponse(AuthResponse::decode_payload(bytes)?)),
      M::ChannelList => Ok(Self::ChannelList(ChannelList::decode_payload(bytes)?)),
      M::ChannelUserList => Ok(Self::ChannelUserList(ChannelUserList::decode_payload(bytes)?)),
      M::UserJoinedChannel => Ok(Self::UserJoinedChannel(UserJoinedChannel::decode_payload(bytes)?)),
      M::UserLeftChannel => Ok(Self::UserLeftChannel(UserLeftChannel::decode_payload(bytes)?)),
      M::UserVoiceState => Ok(Self::UserVoiceState(UserVoiceState::decode_payload(bytes)?)),
      M::KeepalivePong => Ok(Self::KeepalivePong),
      M::UserRoleChanged => Ok(Self::UserRoleChanged(UserRoleChanged::decode_payload(bytes)?)),
      M::ScreenShareStarted => Ok(Self::ScreenShareStarted(ScreenShareStarted::decode_payload(bytes)?)),
      M::ScreenShareStopped => {
        let mut r = BinaryReader::new(bytes);
        let sharer_user_id = r.read_u32()?;
        r.finish()?;
        Ok(Self::ScreenShareStopped { sharer_user_id })
      }
      M::ScreenShareDenied => {
        let mut r = BinaryReader::new(bytes);
        let reason = r.read_string()?;
        r.finish()?;
        Ok(Self::ScreenShareDenied { reason })
      }
      M::ServerError => {
        let mut r = BinaryReader::new(bytes);
        let code = ServerErrorCode::from_u16(r.read_u16()?);
        let message = r.read_string()?;
        r.finish()?;
        Ok(Self::ServerError { code, message })
      }
      M::AdminResult => Ok(Self::AdminResult(AdminResult::decode_payload(bytes)?)),
      M::ChatMessage => {
        let mut r = BinaryReader::new(bytes);
        let msg = ChatMessage::decode_from(&mut r)?;
        r.finish()?;
        Ok(Self::ChatMessage(msg))
      }
      M::ChatHistoryResp => Ok(Self::ChatHistoryResp(ChatHistoryResponse::decode_payload(bytes)?)),
      M::ChatMessageDeleted => {
        let mut r = BinaryReader::new(bytes);
        let message_id = r.read_u64()?;
        let channel_id = r.read_u32()?;
        r.finish()?;
        Ok(Self::ChatMessageDeleted { message_id, channel_id })
      }
      M::ChatFileUploadResp => Ok(Self::ChatFileUploadResp(ChatFileUploadResponse::decode_payload(bytes)?)),
      M::ChatFileReady => {
        let mut r = BinaryReader::new(bytes);
        let message_id = r.read_u64()?;
        let attachment_id = r.read_u64()?;
        r.finish()?;
        Ok(Self::ChatFileReady {
          message_id,
          attachment_id,
        })
      }
      M::ChatSearchResp => {
        let mut r = BinaryReader::new(bytes);
        let channel_id = r.read_u32()?;
        let count = r.read_u16()? as usize;
        let mut messages = Vec::with_capacity(count);
        for _ in 0..count {
          messages.push(ChatMessage::decode_from(&mut r)?);
        }
        r.finish()?;
        Ok(Self::ChatSearchResp { channel_id, messages })
      }
      M::ChatPinnedResp => {
        let mut r = BinaryReader::new(bytes);
        let channel_id = r.read_u32()?;
        let count = r.read_u16()? as usize;
        let mut messages = Vec::with_capacity(count);
        for _ in 0..count {
          messages.push(ChatMessage::decode_from(&mut r)?);
        }
        r.finish()?;
        Ok(Self::ChatPinnedResp { channel_id, messages })
      }
      M::ChatChannelList => {
        let mut r = BinaryReader::new(bytes);
        let count = r.read_u32()? as usize;
        let mut channels = Vec::with_capacity(count);
        for _ in 0..count {
          channels.push(TextChannelInfo {
            id: r.read_u32()?,
            name: r.read_string()?,
            sort_order: r.read_u32()?,
          });
        }
        r.finish()?;
        Ok(Self::ChatChannelList { channels })
      }
      M::ChatCommandList => Ok(Self::ChatCommandList(ChatCommandList::decode_payload(bytes)?)),

      M::AuthIdentity
      | M::ChannelJoin
      | M::ChannelLeave
      | M::KeepalivePing
      | M::VoiceStateUpdate
      | M::ScreenShareStart
      | M::ScreenShareStop
      | M::ScreenShareView
      | M::ScreenShareUpdate
      | M::AdminCreateChannel
      | M::AdminDeleteChannel
      | M::AdminSetRole
      | M::AdminKickUser
      | M::AdminRenameChannel
      | M::AdminSetUserVoiceState
      | M::AdminDisconnectUser
      | M::ChatSend
      | M::ChatHistoryReq
      | M::ChatPin
      | M::ChatUnpin
      | M::ChatDelete
      | M::ChatFileUploadReq
      | M::ChatFileDownloadReq
      | M::ChatSearch
      | M::ChatPinnedReq
      | M::AdminCreateTextChannel
      | M::AdminDeleteTextChannel => Err(DecodeError::InvalidMessageType(frame.ty as u16)),
    }
  }
}

#[cfg(test)]
#[path = "../../../tests/unit/network/protocol/s2c.rs"]
mod tests;
