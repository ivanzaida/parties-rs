#![allow(dead_code, unused_imports)]

pub mod c2s;
pub mod codec;
pub mod control;
pub mod data;
pub mod permissions;
pub mod s2c;

pub use c2s::C2S;
pub use codec::{BinaryReader, BinaryWriter, DecodeError, DecodeResult};
pub use control::*;
pub use data::*;
pub use permissions::*;
pub use s2c::S2C;

pub const DEFAULT_PORT: u16 = 7800;
pub const PROTOCOL_VERSION: u16 = 1;
pub const ALPN: &[u8] = b"parties";

pub const PUBLIC_KEY_LEN: usize = 32;
pub const SECRET_KEY_LEN: usize = 32;
pub const SIGNATURE_LEN: usize = 64;
pub const SESSION_TOKEN_LEN: usize = 32;
pub const CHANNEL_KEY_LEN: usize = 32;

pub type UserId = u32;
pub type ChannelId = u32;
pub type PublicKey = [u8; PUBLIC_KEY_LEN];
pub type SecretKey = [u8; SECRET_KEY_LEN];
pub type Signature = [u8; SIGNATURE_LEN];
pub type SessionToken = [u8; SESSION_TOKEN_LEN];
pub type ChannelKeyBytes = [u8; CHANNEL_KEY_LEN];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Role {
  Owner = 0,
  Admin = 1,
  Moderator = 2,
  User = 3,
}

impl Role {
  pub fn from_u8(value: u8) -> Option<Self> {
    match value {
      0 => Some(Self::Owner),
      1 => Some(Self::Admin),
      2 => Some(Self::Moderator),
      3 => Some(Self::User),
      _ => None,
    }
  }
}

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
#[repr(u8)]
pub enum VideoCodecId {
  Unknown = 0x00,
  Av1 = 0x01,
  H265 = 0x02,
  H264 = 0x03,
}

impl VideoCodecId {
  pub fn from_u8(value: u8) -> Option<Self> {
    match value {
      0x00 => Some(Self::Unknown),
      0x01 => Some(Self::Av1),
      0x02 => Some(Self::H265),
      0x03 => Some(Self::H264),
      _ => None,
    }
  }

  pub fn is_supported_stream_codec(self) -> bool {
    matches!(self, Self::Av1 | Self::H265 | Self::H264)
  }
}

pub const VIDEO_FLAG_KEYFRAME: u8 = 0x01;
