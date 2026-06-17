use std::fmt;

pub type DecodeResult<T> = Result<T, DecodeError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeError {
  UnexpectedEof { needed: usize, remaining: usize },
  StringTooLong { len: usize },
  InvalidUtf8,
  InvalidLength { len: usize, max: usize },
  InvalidMessageType(u16),
  InvalidPacketType(u8),
  InvalidEnumValue { field: &'static str, value: u8 },
  TrailingBytes(usize),
}

impl fmt::Display for DecodeError {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      Self::UnexpectedEof { needed, remaining } => {
        write!(f, "unexpected eof: needed {needed} bytes, had {remaining}")
      }
      Self::StringTooLong { len } => write!(f, "string too long for u16 length: {len}"),
      Self::InvalidUtf8 => write!(f, "invalid utf-8 string"),
      Self::InvalidLength { len, max } => write!(f, "invalid length {len}, max {max}"),
      Self::InvalidMessageType(value) => write!(f, "invalid message type 0x{value:04x}"),
      Self::InvalidPacketType(value) => write!(f, "invalid packet type 0x{value:02x}"),
      Self::InvalidEnumValue { field, value } => {
        write!(f, "invalid {field} value 0x{value:02x}")
      }
      Self::TrailingBytes(len) => write!(f, "{len} trailing byte(s)"),
    }
  }
}

impl std::error::Error for DecodeError {}

#[derive(Debug, Default, Clone)]
pub struct BinaryWriter {
  bytes: Vec<u8>,
}

impl BinaryWriter {
  pub fn new() -> Self {
    Self::default()
  }

  pub fn into_bytes(self) -> Vec<u8> {
    self.bytes
  }

  pub fn as_slice(&self) -> &[u8] {
    &self.bytes
  }

  pub fn len(&self) -> usize {
    self.bytes.len()
  }

  pub fn write_u8(&mut self, value: u8) {
    self.bytes.push(value);
  }

  pub fn write_u16(&mut self, value: u16) {
    self.bytes.extend_from_slice(&value.to_le_bytes());
  }

  pub fn write_u32(&mut self, value: u32) {
    self.bytes.extend_from_slice(&value.to_le_bytes());
  }

  pub fn write_u64(&mut self, value: u64) {
    self.bytes.extend_from_slice(&value.to_le_bytes());
  }

  pub fn write_bytes(&mut self, bytes: &[u8]) {
    self.bytes.extend_from_slice(bytes);
  }

  pub fn write_string(&mut self, value: &str) -> DecodeResult<()> {
    let len = value.len();
    if len > u16::MAX as usize {
      return Err(DecodeError::StringTooLong { len });
    }
    self.write_u16(len as u16);
    self.write_bytes(value.as_bytes());
    Ok(())
  }
}

#[derive(Debug, Clone)]
pub struct BinaryReader<'a> {
  bytes: &'a [u8],
  pos: usize,
}

impl<'a> BinaryReader<'a> {
  pub fn new(bytes: &'a [u8]) -> Self {
    Self { bytes, pos: 0 }
  }

  pub fn remaining(&self) -> usize {
    self.bytes.len().saturating_sub(self.pos)
  }

  pub fn has_remaining(&self, len: usize) -> bool {
    self.remaining() >= len
  }

  pub fn finish(self) -> DecodeResult<()> {
    let remaining = self.remaining();
    if remaining == 0 {
      Ok(())
    } else {
      Err(DecodeError::TrailingBytes(remaining))
    }
  }

  pub fn read_u8(&mut self) -> DecodeResult<u8> {
    let bytes = self.read_exact(1)?;
    Ok(bytes[0])
  }

  pub fn read_u16(&mut self) -> DecodeResult<u16> {
    let bytes = self.read_exact(2)?;
    Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
  }

  pub fn read_u32(&mut self) -> DecodeResult<u32> {
    let bytes = self.read_exact(4)?;
    Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
  }

  pub fn read_u64(&mut self) -> DecodeResult<u64> {
    let bytes = self.read_exact(8)?;
    Ok(u64::from_le_bytes([
      bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
    ]))
  }

  pub fn read_array<const N: usize>(&mut self) -> DecodeResult<[u8; N]> {
    let bytes = self.read_exact(N)?;
    let mut out = [0; N];
    out.copy_from_slice(bytes);
    Ok(out)
  }

  pub fn read_bytes(&mut self, len: usize) -> DecodeResult<&'a [u8]> {
    self.read_exact(len)
  }

  pub fn read_remaining(&mut self) -> &'a [u8] {
    let bytes = &self.bytes[self.pos..];
    self.pos = self.bytes.len();
    bytes
  }

  pub fn read_string(&mut self) -> DecodeResult<String> {
    let len = self.read_u16()? as usize;
    let bytes = self.read_exact(len)?;
    std::str::from_utf8(bytes)
      .map(str::to_owned)
      .map_err(|_| DecodeError::InvalidUtf8)
  }

  fn read_exact(&mut self, len: usize) -> DecodeResult<&'a [u8]> {
    let remaining = self.remaining();
    if remaining < len {
      return Err(DecodeError::UnexpectedEof { needed: len, remaining });
    }
    let start = self.pos;
    self.pos += len;
    Ok(&self.bytes[start..self.pos])
  }
}

#[cfg(test)]
#[path = "../../../tests/unit/network/protocol/codec.rs"]
mod tests;
