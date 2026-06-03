use super::{
  BinaryWriter, ChannelId, ControlFrame, ControlMessageType, DecodeResult, Role, UserId,
  control::{AuthIdentity, ChatSendAttachment, ScreenShareMetadata, VoiceState},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum C2S {
  Auth(AuthIdentity),
  ChannelJoin {
    channel_id: ChannelId,
  },
  ChannelLeave,
  KeepalivePing,
  VoiceStateUpdate(VoiceState),
  ScreenShareStart(ScreenShareMetadata),
  ScreenShareStop,
  ScreenShareView {
    target_user_id: UserId,
  },
  ScreenShareUpdate(ScreenShareMetadata),
  AdminCreateChannel {
    name: String,
    max_users: u32,
  },
  AdminDeleteChannel {
    channel_id: ChannelId,
  },
  AdminSetRole {
    target_user_id: UserId,
    role: Role,
  },
  AdminKickUser {
    target_user_id: UserId,
  },
  AdminRenameChannel {
    channel_id: ChannelId,
    new_name: String,
  },
  ChatSend {
    channel_id: ChannelId,
    text: String,
    attachments: Vec<ChatSendAttachment>,
  },
  ChatHistoryReq {
    channel_id: ChannelId,
    before_id: u64,
    limit: u16,
  },
  ChatPin {
    message_id: u64,
  },
  ChatUnpin {
    message_id: u64,
  },
  ChatDelete {
    message_id: u64,
  },
  ChatFileUploadReq {
    message_id: u64,
    file_index: u8,
    file_size: u64,
  },
  ChatFileDownloadReq {
    file_id: u64,
  },
  ChatSearch {
    channel_id: ChannelId,
    query: String,
    before_id: u64,
    limit: u16,
  },
  ChatPinnedReq {
    channel_id: ChannelId,
  },
  AdminCreateTextChannel {
    name: String,
  },
  AdminDeleteTextChannel {
    channel_id: ChannelId,
  },
}

impl C2S {
  pub fn encode(&self) -> DecodeResult<ControlFrame> {
    use ControlMessageType as M;

    let (ty, payload) = match self {
      Self::Auth(auth) => (M::AuthIdentity, auth.encode_payload()?),

      Self::ChannelJoin { channel_id } => {
        let mut w = BinaryWriter::new();
        w.write_u32(*channel_id);
        (M::ChannelJoin, w.into_bytes())
      }
      Self::ChannelLeave => (M::ChannelLeave, Vec::new()),
      Self::KeepalivePing => (M::KeepalivePing, Vec::new()),

      Self::VoiceStateUpdate(state) => (M::VoiceStateUpdate, state.encode_payload()),

      Self::ScreenShareStart(meta) => (M::ScreenShareStart, meta.encode_payload()),
      Self::ScreenShareStop => (M::ScreenShareStop, Vec::new()),
      Self::ScreenShareView { target_user_id } => {
        let mut w = BinaryWriter::new();
        w.write_u32(*target_user_id);
        (M::ScreenShareView, w.into_bytes())
      }
      Self::ScreenShareUpdate(meta) => (M::ScreenShareUpdate, meta.encode_payload()),

      Self::AdminCreateChannel { name, max_users } => {
        let mut w = BinaryWriter::new();
        w.write_string(name)?;
        w.write_u32(*max_users);
        (M::AdminCreateChannel, w.into_bytes())
      }
      Self::AdminDeleteChannel { channel_id } => {
        let mut w = BinaryWriter::new();
        w.write_u32(*channel_id);
        (M::AdminDeleteChannel, w.into_bytes())
      }
      Self::AdminSetRole { target_user_id, role } => {
        let mut w = BinaryWriter::new();
        w.write_u32(*target_user_id);
        w.write_u8(*role as u8);
        (M::AdminSetRole, w.into_bytes())
      }
      Self::AdminKickUser { target_user_id } => {
        let mut w = BinaryWriter::new();
        w.write_u32(*target_user_id);
        (M::AdminKickUser, w.into_bytes())
      }
      Self::AdminRenameChannel { channel_id, new_name } => {
        let mut w = BinaryWriter::new();
        w.write_u32(*channel_id);
        w.write_string(new_name)?;
        (M::AdminRenameChannel, w.into_bytes())
      }

      Self::ChatSend {
        channel_id,
        text,
        attachments,
      } => {
        let mut w = BinaryWriter::new();
        w.write_u32(*channel_id);
        w.write_string(text)?;
        w.write_u8(attachments.len() as u8);
        for att in attachments {
          w.write_string(&att.file_name)?;
          w.write_u64(att.file_size);
          w.write_string(&att.mime_type)?;
        }
        (M::ChatSend, w.into_bytes())
      }
      Self::ChatHistoryReq {
        channel_id,
        before_id,
        limit,
      } => {
        let mut w = BinaryWriter::new();
        w.write_u32(*channel_id);
        w.write_u64(*before_id);
        w.write_u16(*limit);
        (M::ChatHistoryReq, w.into_bytes())
      }
      Self::ChatPin { message_id } => {
        let mut w = BinaryWriter::new();
        w.write_u64(*message_id);
        (M::ChatPin, w.into_bytes())
      }
      Self::ChatUnpin { message_id } => {
        let mut w = BinaryWriter::new();
        w.write_u64(*message_id);
        (M::ChatUnpin, w.into_bytes())
      }
      Self::ChatDelete { message_id } => {
        let mut w = BinaryWriter::new();
        w.write_u64(*message_id);
        (M::ChatDelete, w.into_bytes())
      }
      Self::ChatFileUploadReq {
        message_id,
        file_index,
        file_size,
      } => {
        let mut w = BinaryWriter::new();
        w.write_u64(*message_id);
        w.write_u8(*file_index);
        w.write_u64(*file_size);
        (M::ChatFileUploadReq, w.into_bytes())
      }
      Self::ChatFileDownloadReq { file_id } => {
        let mut w = BinaryWriter::new();
        w.write_u64(*file_id);
        (M::ChatFileDownloadReq, w.into_bytes())
      }
      Self::ChatSearch {
        channel_id,
        query,
        before_id,
        limit,
      } => {
        let mut w = BinaryWriter::new();
        w.write_u32(*channel_id);
        w.write_string(query)?;
        w.write_u64(*before_id);
        w.write_u16(*limit);
        (M::ChatSearch, w.into_bytes())
      }
      Self::ChatPinnedReq { channel_id } => {
        let mut w = BinaryWriter::new();
        w.write_u32(*channel_id);
        (M::ChatPinnedReq, w.into_bytes())
      }
      Self::AdminCreateTextChannel { name } => {
        let mut w = BinaryWriter::new();
        w.write_string(name)?;
        (M::AdminCreateTextChannel, w.into_bytes())
      }
      Self::AdminDeleteTextChannel { channel_id } => {
        let mut w = BinaryWriter::new();
        w.write_u32(*channel_id);
        (M::AdminDeleteTextChannel, w.into_bytes())
      }
    };

    Ok(ControlFrame { ty, payload })
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn keepalive_ping_encodes_empty() {
    let frame = C2S::KeepalivePing.encode().unwrap();
    assert_eq!(frame.ty, ControlMessageType::KeepalivePing);
    assert!(frame.payload.is_empty());
  }

  #[test]
  fn channel_join_encodes_id() {
    let frame = C2S::ChannelJoin { channel_id: 42 }.encode().unwrap();
    assert_eq!(frame.ty, ControlMessageType::ChannelJoin);
    assert_eq!(frame.payload, 42_u32.to_le_bytes());
  }
}
