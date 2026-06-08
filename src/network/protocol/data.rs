use super::{BinaryReader, BinaryWriter, DecodeError, DecodeResult, UserId, VIDEO_FLAG_KEYFRAME, VideoCodecId};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum PacketType {
  Voice = 0x01,
  VideoFrame = 0x02,
  VideoControl = 0x03,
  StreamAudio = 0x04,
}

impl PacketType {
  pub fn from_u8(value: u8) -> Option<Self> {
    match value {
      0x01 => Some(Self::Voice),
      0x02 => Some(Self::VideoFrame),
      0x03 => Some(Self::VideoControl),
      0x04 => Some(Self::StreamAudio),
      _ => None,
    }
  }
}

pub const STREAM_TYPE_FILE_UPLOAD: u8 = 0x10;
pub const STREAM_TYPE_FILE_DOWNLOAD: u8 = 0x11;

pub const VIDEO_CTL_PLI: u8 = 0x01;
pub const VIDEO_CTL_SHARE_START: u8 = 0x02;
pub const VIDEO_CTL_SHARE_STOP: u8 = 0x03;

pub const MAX_VIDEO_FRAME_LEN: usize = 4 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoicePacket {
  pub sequence: u16,
  pub opus: Vec<u8>,
}

impl VoicePacket {
  pub fn encode(&self) -> Vec<u8> {
    let mut w = BinaryWriter::new();
    w.write_u8(PacketType::Voice as u8);
    w.write_u16(self.sequence);
    w.write_bytes(&self.opus);
    w.into_bytes()
  }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForwardedVoicePacket {
  pub sender_id: UserId,
  pub sequence: u16,
  pub opus: Vec<u8>,
}

impl ForwardedVoicePacket {
  pub fn decode(bytes: &[u8]) -> DecodeResult<Self> {
    let mut r = BinaryReader::new(bytes);
    expect_packet_type(&mut r, PacketType::Voice)?;
    let sender_id = r.read_u32()?;
    let sequence = r.read_u16()?;
    let opus = r.read_remaining().to_vec();
    Ok(Self {
      sender_id,
      sequence,
      opus,
    })
  }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoFrame {
  pub frame_number: u32,
  pub timestamp: u32,
  pub keyframe: bool,
  pub width: u16,
  pub height: u16,
  pub codec: VideoCodecId,
  pub encoded: Vec<u8>,
}

impl VideoFrame {
  pub fn encode_packet(&self) -> Vec<u8> {
    let mut w = BinaryWriter::new();
    w.write_u8(PacketType::VideoFrame as u8);
    w.write_u32(self.frame_number);
    w.write_u32(self.timestamp);
    w.write_u8(if self.keyframe { VIDEO_FLAG_KEYFRAME } else { 0 });
    w.write_u16(self.width);
    w.write_u16(self.height);
    w.write_u8(self.codec as u8);
    w.write_bytes(&self.encoded);
    w.into_bytes()
  }

  pub fn decode_payload(bytes: &[u8]) -> DecodeResult<Self> {
    let mut r = BinaryReader::new(bytes);
    let frame_number = r.read_u32()?;
    let timestamp = r.read_u32()?;
    let flags = r.read_u8()?;
    let width = r.read_u16()?;
    let height = r.read_u16()?;
    let raw_codec = r.read_u8()?;
    let codec = VideoCodecId::from_u8(raw_codec).ok_or(DecodeError::InvalidEnumValue {
      field: "video codec",
      value: raw_codec,
    })?;
    if !codec.is_supported_stream_codec() {
      return Err(DecodeError::InvalidEnumValue {
        field: "video codec",
        value: raw_codec,
      });
    }
    let encoded = r.read_remaining().to_vec();
    Ok(Self {
      frame_number,
      timestamp,
      keyframe: flags & VIDEO_FLAG_KEYFRAME != 0,
      width,
      height,
      codec,
      encoded,
    })
  }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForwardedVideoFrame {
  pub sender_id: UserId,
  pub frame: VideoFrame,
}

impl ForwardedVideoFrame {
  pub fn decode(bytes: &[u8]) -> DecodeResult<Self> {
    let mut r = BinaryReader::new(bytes);
    expect_packet_type(&mut r, PacketType::VideoFrame)?;
    let sender_id = r.read_u32()?;
    let frame = VideoFrame::decode_payload(r.read_remaining())?;
    Ok(Self { sender_id, frame })
  }

  pub fn decode_owned(mut bytes: Vec<u8>) -> DecodeResult<Self> {
    const HEADER_LEN: usize = 1 + 4 + 4 + 4 + 1 + 2 + 2 + 1;
    if bytes.len() < HEADER_LEN {
      return Err(DecodeError::UnexpectedEof {
        needed: HEADER_LEN,
        remaining: bytes.len(),
      });
    }
    if bytes[0] != PacketType::VideoFrame as u8 {
      return Err(DecodeError::InvalidPacketType(bytes[0]));
    }

    let sender_id = u32::from_le_bytes([bytes[1], bytes[2], bytes[3], bytes[4]]);
    let frame_number = u32::from_le_bytes([bytes[5], bytes[6], bytes[7], bytes[8]]);
    let timestamp = u32::from_le_bytes([bytes[9], bytes[10], bytes[11], bytes[12]]);
    let flags = bytes[13];
    let width = u16::from_le_bytes([bytes[14], bytes[15]]);
    let height = u16::from_le_bytes([bytes[16], bytes[17]]);
    let raw_codec = bytes[18];
    let codec = VideoCodecId::from_u8(raw_codec).ok_or(DecodeError::InvalidEnumValue {
      field: "video codec",
      value: raw_codec,
    })?;
    if !codec.is_supported_stream_codec() {
      return Err(DecodeError::InvalidEnumValue {
        field: "video codec",
        value: raw_codec,
      });
    }

    bytes.drain(..HEADER_LEN);
    let encoded = bytes;
    Ok(Self {
      sender_id,
      frame: VideoFrame {
        frame_number,
        timestamp,
        keyframe: flags & VIDEO_FLAG_KEYFRAME != 0,
        width,
        height,
        codec,
        encoded,
      },
    })
  }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VideoControl {
  Pli { user_id: UserId },
  ShareStart,
  ShareStop,
  Unknown { subtype: u8, payload: Vec<u8> },
}

impl VideoControl {
  pub fn encode_datagram(&self) -> Vec<u8> {
    let mut w = BinaryWriter::new();
    w.write_u8(PacketType::VideoControl as u8);
    match self {
      Self::Pli { user_id } => {
        w.write_u8(VIDEO_CTL_PLI);
        w.write_u32(*user_id);
      }
      Self::ShareStart => w.write_u8(VIDEO_CTL_SHARE_START),
      Self::ShareStop => w.write_u8(VIDEO_CTL_SHARE_STOP),
      Self::Unknown { subtype, payload } => {
        w.write_u8(*subtype);
        w.write_bytes(payload);
      }
    }
    w.into_bytes()
  }

  pub fn decode_datagram(bytes: &[u8]) -> DecodeResult<Self> {
    let mut r = BinaryReader::new(bytes);
    expect_packet_type(&mut r, PacketType::VideoControl)?;
    let subtype = r.read_u8()?;
    Ok(match subtype {
      VIDEO_CTL_PLI => Self::Pli { user_id: r.read_u32()? },
      VIDEO_CTL_SHARE_START => Self::ShareStart,
      VIDEO_CTL_SHARE_STOP => Self::ShareStop,
      _ => Self::Unknown {
        subtype,
        payload: r.read_remaining().to_vec(),
      },
    })
  }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamAudioPacket {
  pub opus: Vec<u8>,
}

impl StreamAudioPacket {
  pub fn encode(&self) -> Vec<u8> {
    let mut w = BinaryWriter::new();
    w.write_u8(PacketType::StreamAudio as u8);
    w.write_bytes(&self.opus);
    w.into_bytes()
  }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForwardedStreamAudioPacket {
  pub sender_id: UserId,
  pub opus: Vec<u8>,
}

impl ForwardedStreamAudioPacket {
  pub fn decode(bytes: &[u8]) -> DecodeResult<Self> {
    let mut r = BinaryReader::new(bytes);
    expect_packet_type(&mut r, PacketType::StreamAudio)?;
    let sender_id = r.read_u32()?;
    let opus = r.read_remaining().to_vec();
    Ok(Self { sender_id, opus })
  }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileStreamRequest {
  Upload { attachment_id: u64, data: Vec<u8> },
  Download { attachment_id: u64 },
}

impl FileStreamRequest {
  pub fn encode(&self) -> Vec<u8> {
    let mut w = BinaryWriter::new();
    match self {
      Self::Upload { attachment_id, data } => {
        w.write_u8(STREAM_TYPE_FILE_UPLOAD);
        w.write_u64(*attachment_id);
        w.write_bytes(data);
      }
      Self::Download { attachment_id } => {
        w.write_u8(STREAM_TYPE_FILE_DOWNLOAD);
        w.write_u64(*attachment_id);
      }
    }
    w.into_bytes()
  }
}

fn expect_packet_type(reader: &mut BinaryReader<'_>, expected: PacketType) -> DecodeResult<()> {
  let actual = reader.read_u8()?;
  if PacketType::from_u8(actual) == Some(expected) {
    Ok(())
  } else {
    Err(DecodeError::InvalidPacketType(actual))
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn video_control_pli_round_trips() {
    let pkt = VideoControl::Pli { user_id: 42 };
    let encoded = pkt.encode_datagram();
    assert_eq!(
      encoded,
      vec![PacketType::VideoControl as u8, VIDEO_CTL_PLI, 42, 0, 0, 0]
    );
    assert_eq!(VideoControl::decode_datagram(&encoded).unwrap(), pkt);
  }

  #[test]
  fn forwarded_video_frame_decode_owned_reuses_payload_buffer() {
    let mut encoded = Vec::new();
    encoded.push(PacketType::VideoFrame as u8);
    encoded.extend_from_slice(&7u32.to_le_bytes());
    encoded.extend_from_slice(&11u32.to_le_bytes());
    encoded.extend_from_slice(&12u32.to_le_bytes());
    encoded.push(VIDEO_FLAG_KEYFRAME);
    encoded.extend_from_slice(&1920u16.to_le_bytes());
    encoded.extend_from_slice(&1080u16.to_le_bytes());
    encoded.push(VideoCodecId::Av1 as u8);
    encoded.extend_from_slice(&[1, 2, 3, 4]);

    let decoded = ForwardedVideoFrame::decode_owned(encoded).unwrap();

    assert_eq!(decoded.sender_id, 7);
    assert_eq!(decoded.frame.frame_number, 11);
    assert_eq!(decoded.frame.timestamp, 12);
    assert!(decoded.frame.keyframe);
    assert_eq!(decoded.frame.width, 1920);
    assert_eq!(decoded.frame.height, 1080);
    assert_eq!(decoded.frame.codec, VideoCodecId::Av1);
    assert_eq!(decoded.frame.encoded, vec![1, 2, 3, 4]);
  }
}
