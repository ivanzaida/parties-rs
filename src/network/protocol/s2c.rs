use super::{
    BinaryReader, ChannelId, ControlFrame, ControlMessageType, DecodeError, DecodeResult, Role,
    UserId, VideoCodecId,
    control::{
        AdminResult, AuthResponse, ChannelKey, ChannelList, ChannelUserList,
        ChatFileUploadResponse, ChatHistoryResponse, ChatMessage, ScreenShareMetadata,
        ScreenShareStarted, TextChannelInfo, UserJoinedChannel, UserLeftChannel, UserRoleChanged,
        UserVoiceState,
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
    ChannelKey(ChannelKey),
    ScreenShareStarted(ScreenShareStarted),
    ScreenShareStopped { sharer_user_id: UserId },
    ScreenShareDenied { reason: String },
    ServerError { message: String },
    AdminResult(AdminResult),
    ChatMessage(ChatMessage),
    ChatHistoryResp(ChatHistoryResponse),
    ChatMessageDeleted { message_id: u64, channel_id: ChannelId },
    ChatFileUploadResp(ChatFileUploadResponse),
    ChatFileReady { message_id: u64, file_index: u8, file_id: u64 },
    ChatSearchResp { channel_id: ChannelId, messages: Vec<ChatMessage> },
    ChatPinnedResp { channel_id: ChannelId, messages: Vec<ChatMessage> },
    ChatChannelList { channels: Vec<TextChannelInfo> },
}

impl S2C {
    pub fn decode(frame: &ControlFrame) -> DecodeResult<Self> {
        use ControlMessageType as M;

        let bytes = &frame.payload;
        match frame.ty {
            M::AuthResponse => Ok(Self::AuthResponse(AuthResponse::decode_payload(bytes)?)),
            M::ChannelList => Ok(Self::ChannelList(ChannelList::decode_payload(bytes)?)),
            M::ChannelUserList => {
                Ok(Self::ChannelUserList(ChannelUserList::decode_payload(bytes)?))
            }
            M::UserJoinedChannel => {
                Ok(Self::UserJoinedChannel(UserJoinedChannel::decode_payload(bytes)?))
            }
            M::UserLeftChannel => {
                Ok(Self::UserLeftChannel(UserLeftChannel::decode_payload(bytes)?))
            }
            M::UserVoiceState => {
                Ok(Self::UserVoiceState(UserVoiceState::decode_payload(bytes)?))
            }
            M::KeepalivePong => Ok(Self::KeepalivePong),
            M::UserRoleChanged => {
                Ok(Self::UserRoleChanged(UserRoleChanged::decode_payload(bytes)?))
            }
            M::ChannelKey => Ok(Self::ChannelKey(ChannelKey::decode_payload(bytes)?)),
            M::ScreenShareStarted => {
                Ok(Self::ScreenShareStarted(ScreenShareStarted::decode_payload(bytes)?))
            }
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
                let message = r.read_string()?;
                r.finish()?;
                Ok(Self::ServerError { message })
            }
            M::AdminResult => Ok(Self::AdminResult(AdminResult::decode_payload(bytes)?)),
            M::ChatMessage => {
                let mut r = BinaryReader::new(bytes);
                let msg = ChatMessage::decode_from(&mut r)?;
                r.finish()?;
                Ok(Self::ChatMessage(msg))
            }
            M::ChatHistoryResp => {
                Ok(Self::ChatHistoryResp(ChatHistoryResponse::decode_payload(bytes)?))
            }
            M::ChatMessageDeleted => {
                let mut r = BinaryReader::new(bytes);
                let message_id = r.read_u64()?;
                let channel_id = r.read_u32()?;
                r.finish()?;
                Ok(Self::ChatMessageDeleted { message_id, channel_id })
            }
            M::ChatFileUploadResp => {
                Ok(Self::ChatFileUploadResp(ChatFileUploadResponse::decode_payload(bytes)?))
            }
            M::ChatFileReady => {
                let mut r = BinaryReader::new(bytes);
                let message_id = r.read_u64()?;
                let file_index = r.read_u8()?;
                let file_id = r.read_u64()?;
                r.finish()?;
                Ok(Self::ChatFileReady { message_id, file_index, file_id })
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
mod tests {
    use super::*;

    #[test]
    fn keepalive_pong_decodes() {
        let frame = ControlFrame {
            ty: ControlMessageType::KeepalivePong,
            payload: Vec::new(),
        };
        assert_eq!(S2C::decode(&frame).unwrap(), S2C::KeepalivePong);
    }

    #[test]
    fn server_error_decodes() {
        let mut w = super::super::BinaryWriter::new();
        w.write_string("bad request").unwrap();
        let frame = ControlFrame {
            ty: ControlMessageType::ServerError,
            payload: w.into_bytes(),
        };
        match S2C::decode(&frame).unwrap() {
            S2C::ServerError { message } => assert_eq!(message, "bad request"),
            other => panic!("expected ServerError, got {other:?}"),
        }
    }

    #[test]
    fn c2s_type_returns_error() {
        let frame = ControlFrame {
            ty: ControlMessageType::AuthIdentity,
            payload: Vec::new(),
        };
        assert!(S2C::decode(&frame).is_err());
    }
}
