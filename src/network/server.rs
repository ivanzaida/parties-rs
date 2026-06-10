use std::{
  collections::VecDeque,
  fmt,
  net::SocketAddr,
  sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
  },
  time::Duration,
};

use bytes::Bytes;
use quinn::{Connection, Endpoint, VarInt};
use rustls::{
  client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier}, pki_types::{CertificateDer, ServerName, UnixTime},
  DigitallySignedStruct,
  SignatureScheme,
};
use sha2::{Digest, Sha256};
use tokio::sync::{Mutex, Notify};

use super::protocol::{
  control::{AuthIdentity, ChatSendAttachment, ScreenShareMetadata, VoiceState, MAX_CONTROL_MESSAGE_LEN}, data::{
    FileStreamRequest, ForwardedStreamAudioPacket, ForwardedVideoFrame, ForwardedVoicePacket, PacketType,
    VideoControl, VideoFrame, MAX_VIDEO_FRAME_LEN,
  }, ChannelId, ControlFrame, ControlMessageType, DecodeError, Role, UserId, VideoCodecId,
  C2S,
  S2C,
};

#[derive(Debug)]
pub enum ServerError {
  Protocol(DecodeError),
  Connection(quinn::ConnectionError),
  Connect(quinn::ConnectError),
  Write(quinn::WriteError),
  Read(quinn::ReadExactError),
  Datagram(quinn::SendDatagramError),
  Io(std::io::Error),
}

#[derive(Debug)]
pub enum ReceivedAudioPacket {
  Voice(ForwardedVoicePacket),
  Stream(ForwardedStreamAudioPacket),
  VideoControl(VideoControl),
}

#[derive(Debug)]
pub enum ReceivedVideoPacket {
  Frame(ForwardedVideoFrame),
  VideoControl(VideoControl),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoFrameSend {
  Datagram,
  StreamFallback,
  Dropped,
}

impl fmt::Display for ServerError {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      Self::Protocol(e) => write!(f, "protocol: {e}"),
      Self::Connection(e) => write!(f, "connection: {e}"),
      Self::Connect(e) => write!(f, "connect: {e}"),
      Self::Write(e) => write!(f, "write: {e}"),
      Self::Read(e) => write!(f, "read: {e}"),
      Self::Datagram(e) => write!(f, "datagram: {e}"),
      Self::Io(e) => write!(f, "io: {e}"),
    }
  }
}

impl std::error::Error for ServerError {}

impl From<DecodeError> for ServerError {
  fn from(e: DecodeError) -> Self {
    Self::Protocol(e)
  }
}
impl From<quinn::ConnectionError> for ServerError {
  fn from(e: quinn::ConnectionError) -> Self {
    Self::Connection(e)
  }
}
impl From<quinn::ConnectError> for ServerError {
  fn from(e: quinn::ConnectError) -> Self {
    Self::Connect(e)
  }
}
impl From<quinn::WriteError> for ServerError {
  fn from(e: quinn::WriteError) -> Self {
    Self::Write(e)
  }
}
impl From<quinn::ReadExactError> for ServerError {
  fn from(e: quinn::ReadExactError) -> Self {
    Self::Read(e)
  }
}
impl From<quinn::SendDatagramError> for ServerError {
  fn from(e: quinn::SendDatagramError) -> Self {
    Self::Datagram(e)
  }
}
impl From<std::io::Error> for ServerError {
  fn from(e: std::io::Error) -> Self {
    Self::Io(e)
  }
}

#[derive(Debug)]
struct AcceptAnyCert;

impl ServerCertVerifier for AcceptAnyCert {
  fn verify_server_cert(
    &self,
    _end_entity: &CertificateDer<'_>,
    _intermediates: &[CertificateDer<'_>],
    _server_name: &ServerName<'_>,
    _ocsp: &[u8],
    _now: UnixTime,
  ) -> Result<ServerCertVerified, rustls::Error> {
    Ok(ServerCertVerified::assertion())
  }

  fn verify_tls12_signature(
    &self,
    _message: &[u8],
    _cert: &CertificateDer<'_>,
    _dss: &DigitallySignedStruct,
  ) -> Result<HandshakeSignatureValid, rustls::Error> {
    Ok(HandshakeSignatureValid::assertion())
  }

  fn verify_tls13_signature(
    &self,
    _message: &[u8],
    _cert: &CertificateDer<'_>,
    _dss: &DigitallySignedStruct,
  ) -> Result<HandshakeSignatureValid, rustls::Error> {
    Ok(HandshakeSignatureValid::assertion())
  }

  fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
    rustls::crypto::ring::default_provider()
      .signature_verification_algorithms
      .supported_schemes()
  }
}

pub struct Server {
  _endpoint: Endpoint,
  connection: Connection,
  control_send: Mutex<quinn::SendStream>,
  control_recv: Mutex<quinn::RecvStream>,
  video_send: Mutex<quinn::SendStream>,
  video_recv: Mutex<quinn::RecvStream>,
  pending_video_datagrams: Mutex<VecDeque<ForwardedVideoFrame>>,
  pending_video_notify: Notify,
}

impl Server {
  pub async fn connect(addr: SocketAddr) -> Result<Self, ServerError> {
    let _ = rustls::crypto::ring::default_provider().install_default();

    let mut crypto = rustls::ClientConfig::builder()
      .dangerous()
      .with_custom_certificate_verifier(Arc::new(AcceptAnyCert))
      .with_no_client_auth();
    crypto.alpn_protocols = vec![super::protocol::ALPN.to_vec()];

    let quic_crypto = quinn::crypto::rustls::QuicClientConfig::try_from(crypto).expect("valid TLS config");
    let mut client_config = quinn::ClientConfig::new(Arc::new(quic_crypto));

    let mut transport = quinn::TransportConfig::default();
    transport.max_idle_timeout(Some(
      quinn::IdleTimeout::try_from(Duration::from_secs(60)).expect("valid timeout"),
    ));
    client_config.transport_config(Arc::new(transport));

    let bind_addr = if addr.is_ipv6() {
      SocketAddr::from(([0, 0, 0, 0, 0, 0, 0, 0], 0))
    } else {
      SocketAddr::from(([0, 0, 0, 0], 0))
    };
    let mut endpoint = Endpoint::client(bind_addr)?;
    endpoint.set_default_client_config(client_config);

    let connection = endpoint.connect(addr, "parties")?.await?;
    let (control_send, control_recv) = connection.open_bi().await?;
    let (video_send, video_recv) = connection.open_bi().await?;

    Ok(Self {
      _endpoint: endpoint,
      connection,
      control_send: Mutex::new(control_send),
      control_recv: Mutex::new(control_recv),
      video_send: Mutex::new(video_send),
      video_recv: Mutex::new(video_recv),
      pending_video_datagrams: Mutex::new(VecDeque::new()),
      pending_video_notify: Notify::new(),
    })
  }

  #[allow(dead_code)]
  pub fn connection(&self) -> &Connection {
    &self.connection
  }

  pub fn disconnect(&self) {
    self.connection.close(VarInt::from_u32(0), b"client disconnect");
    self.pending_video_notify.notify_waiters();
  }

  pub fn wake_video_datagram_reader(&self) {
    self.pending_video_notify.notify_waiters();
  }

  pub fn certificate_fingerprint(&self) -> Option<String> {
    let identity = self.connection.peer_identity()?;
    let certificates = identity.downcast::<Vec<CertificateDer<'static>>>().ok()?;
    let certificate = certificates.first()?;
    Some(hex_fingerprint(certificate.as_ref()))
  }

  async fn send_control(&self, msg: C2S) -> Result<(), ServerError> {
    let frame = msg.encode()?;
    let bytes = frame.encode()?;
    self.control_send.lock().await.write_all(&bytes).await?;
    Ok(())
  }

  pub async fn recv(&self) -> Result<S2C, ServerError> {
    let mut recv = self.control_recv.lock().await;

    let mut len_buf = [0u8; 4];
    recv.read_exact(&mut len_buf).await?;
    let msg_len = u32::from_le_bytes(len_buf) as usize;

    if !(2..=MAX_CONTROL_MESSAGE_LEN).contains(&msg_len) {
      return Err(
        DecodeError::InvalidLength {
          len: msg_len,
          max: MAX_CONTROL_MESSAGE_LEN,
        }
        .into(),
      );
    }

    let mut msg_buf = vec![0u8; msg_len];
    recv.read_exact(&mut msg_buf).await?;

    let raw_ty = u16::from_le_bytes([msg_buf[0], msg_buf[1]]);
    let ty = ControlMessageType::from_u16(raw_ty).ok_or(DecodeError::InvalidMessageType(raw_ty))?;

    let frame = ControlFrame {
      ty,
      payload: msg_buf[2..].to_vec(),
    };

    Ok(S2C::decode(&frame)?)
  }

  // -- auth --

  pub async fn authenticate(&self, identity: AuthIdentity) -> Result<(), ServerError> {
    self.send_control(C2S::Auth(identity)).await
  }

  pub async fn authenticate_legacy(&self, identity: AuthIdentity) -> Result<(), ServerError> {
    self.send_control(C2S::AuthLegacy(identity)).await
  }

  // -- channels --

  pub async fn join_channel(&self, channel_id: ChannelId) -> Result<(), ServerError> {
    self.send_control(C2S::ChannelJoin { channel_id }).await
  }

  pub async fn leave_channel(&self) -> Result<(), ServerError> {
    self.send_control(C2S::ChannelLeave).await
  }

  // -- voice --

  pub async fn update_voice_state(&self, muted: bool, deafened: bool) -> Result<(), ServerError> {
    self
      .send_control(C2S::VoiceStateUpdate(VoiceState { muted, deafened }))
      .await
  }

  pub fn send_voice(&self, sequence: u16, opus: &[u8]) -> Result<(), ServerError> {
    let mut data = Vec::with_capacity(1 + 2 + opus.len());
    data.push(PacketType::Voice as u8);
    data.extend_from_slice(&sequence.to_le_bytes());
    data.extend_from_slice(opus);
    self.connection.send_datagram(data.into())?;
    Ok(())
  }

  #[allow(dead_code)]
  pub async fn recv_voice(&self) -> Result<ForwardedVoicePacket, ServerError> {
    loop {
      match self.recv_audio().await? {
        ReceivedAudioPacket::Voice(packet) => return Ok(packet),
        ReceivedAudioPacket::Stream(_) => {}
        ReceivedAudioPacket::VideoControl(_) => {}
      }
    }
  }

  pub async fn recv_audio(&self) -> Result<ReceivedAudioPacket, ServerError> {
    loop {
      let data = self.connection.read_datagram().await?;
      match decode_datagram(data)? {
        DecodedDatagram::Voice(packet) => return Ok(ReceivedAudioPacket::Voice(packet)),
        DecodedDatagram::StreamAudio(packet) => return Ok(ReceivedAudioPacket::Stream(packet)),
        DecodedDatagram::VideoControl(control) => return Ok(ReceivedAudioPacket::VideoControl(control)),
        DecodedDatagram::Video(packet) => {
          self.pending_video_datagrams.lock().await.push_back(packet);
          self.pending_video_notify.notify_one();
        }
      }
    }
  }

  // -- screen share --

  pub async fn start_screen_share(&self, codec: VideoCodecId, width: u16, height: u16) -> Result<(), ServerError> {
    validate_video_codec(codec)?;
    self
      .send_control(C2S::ScreenShareStart(ScreenShareMetadata { codec, width, height }))
      .await
  }

  pub async fn stop_screen_share(&self) -> Result<(), ServerError> {
    self.send_control(C2S::ScreenShareStop).await
  }

  pub async fn view_screen_share(&self, target_user_id: UserId) -> Result<(), ServerError> {
    self.send_control(C2S::ScreenShareView { target_user_id }).await
  }

  pub async fn unsubscribe_screen_share(&self) -> Result<(), ServerError> {
    self.send_control(C2S::ScreenShareView { target_user_id: 0 }).await
  }

  #[allow(dead_code)]
  pub async fn update_screen_share(&self, codec: VideoCodecId, width: u16, height: u16) -> Result<(), ServerError> {
    validate_video_codec(codec)?;
    self
      .send_control(C2S::ScreenShareUpdate(ScreenShareMetadata { codec, width, height }))
      .await
  }

  pub fn send_video_control(&self, control: VideoControl) -> Result<(), ServerError> {
    let data = control.encode_datagram();
    self.connection.send_datagram(data.into())?;
    Ok(())
  }

  pub async fn send_video_control_stream(&self, control: VideoControl) -> Result<(), ServerError> {
    self.send_video_packet(&control.encode_datagram()).await
  }

  pub fn request_keyframe(&self, user_id: UserId) -> Result<(), ServerError> {
    self.send_video_control(VideoControl::Pli { user_id })
  }

  pub async fn request_keyframe_stream(&self, user_id: UserId) -> Result<(), ServerError> {
    self.send_video_control_stream(VideoControl::Pli { user_id }).await
  }

  pub async fn send_video_packet(&self, packet: &[u8]) -> Result<(), ServerError> {
    let framed = encode_video_stream_packet(packet)?;
    self.video_send.lock().await.write_all(&framed).await?;
    Ok(())
  }

  #[allow(dead_code)]
  pub async fn send_video_frame(&self, frame: VideoFrame) -> Result<(), ServerError> {
    validate_video_codec(frame.codec)?;
    self.send_video_packet(&frame.encode_packet()).await
  }

  pub async fn send_live_video_frame(&self, frame: VideoFrame) -> Result<VideoFrameSend, ServerError> {
    validate_video_codec(frame.codec)?;
    let packet = frame.encode_packet();
    match self.connection.send_datagram(packet.clone().into()) {
      Ok(()) => Ok(VideoFrameSend::Datagram),
      Err(
        quinn::SendDatagramError::TooLarge
        | quinn::SendDatagramError::UnsupportedByPeer
        | quinn::SendDatagramError::Disabled,
      ) => {
        self.send_video_packet(&packet).await?;
        Ok(VideoFrameSend::StreamFallback)
      }
      Err(error) => Err(ServerError::Datagram(error)),
    }
  }

  pub fn send_stream_audio(&self, opus: &[u8]) -> Result<(), ServerError> {
    let mut data = Vec::with_capacity(1 + opus.len());
    data.push(PacketType::StreamAudio as u8);
    data.extend_from_slice(opus);
    self.connection.send_datagram(data.into())?;
    Ok(())
  }

  pub async fn recv_video_packet(&self) -> Result<Vec<u8>, ServerError> {
    let mut recv = self.video_recv.lock().await;

    let mut len_buf = [0u8; 4];
    recv.read_exact(&mut len_buf).await?;
    let packet_len = u32::from_le_bytes(len_buf) as usize;
    validate_video_stream_packet_len(packet_len)?;

    let mut packet = vec![0u8; packet_len];
    recv.read_exact(&mut packet).await?;
    Ok(packet)
  }

  pub async fn recv_video(&self) -> Result<ReceivedVideoPacket, ServerError> {
    decode_video_stream_packet(self.recv_video_packet().await?)
  }

  #[allow(dead_code)]
  pub async fn recv_video_frame(&self) -> Result<ForwardedVideoFrame, ServerError> {
    loop {
      match self.recv_video().await? {
        ReceivedVideoPacket::Frame(packet) => return Ok(packet),
        ReceivedVideoPacket::VideoControl(_) => {}
      }
    }
  }

  pub async fn recv_forwarded_video_datagram_until(
    &self,
    stop: &AtomicBool,
  ) -> Result<Option<ForwardedVideoFrame>, ServerError> {
    loop {
      if stop.load(Ordering::Relaxed) {
        return Ok(None);
      }
      if let Some(packet) = self.pending_video_datagrams.lock().await.pop_front() {
        return Ok(Some(packet));
      }
      if let Some(error) = self.connection.close_reason() {
        return Err(ServerError::Connection(error));
      }
      self.pending_video_notify.notified().await;
    }
  }

  // -- admin: channels --

  pub async fn create_channel(&self, name: String, max_users: u32) -> Result<(), ServerError> {
    self.send_control(C2S::AdminCreateChannel { name, max_users }).await
  }

  pub async fn delete_channel(&self, channel_id: ChannelId) -> Result<(), ServerError> {
    self.send_control(C2S::AdminDeleteChannel { channel_id }).await
  }

  pub async fn rename_channel(&self, channel_id: ChannelId, new_name: String) -> Result<(), ServerError> {
    self
      .send_control(C2S::AdminRenameChannel { channel_id, new_name })
      .await
  }

  // -- admin: users --

  pub async fn set_role(&self, target_user_id: UserId, role: Role) -> Result<(), ServerError> {
    self.send_control(C2S::AdminSetRole { target_user_id, role }).await
  }

  pub async fn kick_user(&self, target_user_id: UserId) -> Result<(), ServerError> {
    self.send_control(C2S::AdminKickUser { target_user_id }).await
  }

  pub async fn set_user_voice_state(
    &self,
    target_user_id: UserId,
    muted: bool,
    deafened: bool,
  ) -> Result<(), ServerError> {
    self
      .send_control(C2S::AdminSetUserVoiceState {
        target_user_id,
        muted,
        deafened,
      })
      .await
  }

  pub async fn disconnect_user_from_voice(&self, target_user_id: UserId) -> Result<(), ServerError> {
    self.send_control(C2S::AdminDisconnectUser { target_user_id }).await
  }

  // -- chat --

  pub async fn send_chat(
    &self,
    channel_id: ChannelId,
    text: String,
    attachments: Vec<ChatSendAttachment>,
  ) -> Result<(), ServerError> {
    self
      .send_control(C2S::ChatSend {
        channel_id,
        text,
        attachments,
      })
      .await
  }

  pub async fn send_chat_text(&self, channel_id: ChannelId, text: String) -> Result<(), ServerError> {
    self.send_chat(channel_id, text, Vec::new()).await
  }

  pub async fn request_chat_history(
    &self,
    channel_id: ChannelId,
    before_id: u64,
    limit: u16,
  ) -> Result<(), ServerError> {
    self
      .send_control(C2S::ChatHistoryReq {
        channel_id,
        before_id,
        limit,
      })
      .await
  }

  #[allow(dead_code)]
  pub async fn pin_message(&self, message_id: u64) -> Result<(), ServerError> {
    self.send_control(C2S::ChatPin { message_id }).await
  }

  #[allow(dead_code)]
  pub async fn unpin_message(&self, message_id: u64) -> Result<(), ServerError> {
    self.send_control(C2S::ChatUnpin { message_id }).await
  }

  #[allow(dead_code)]
  pub async fn delete_message(&self, message_id: u64) -> Result<(), ServerError> {
    self.send_control(C2S::ChatDelete { message_id }).await
  }

  #[allow(dead_code)]
  pub async fn upload_file(&self, attachment_id: u64, data: Vec<u8>) -> Result<(), ServerError> {
    let payload = FileStreamRequest::Upload { attachment_id, data }.encode();
    let mut stream = self.connection.open_uni().await?;
    stream.write_all(&payload).await?;
    stream
      .finish()
      .map_err(|_| ServerError::Write(quinn::WriteError::ClosedStream))?;
    Ok(())
  }

  #[allow(dead_code)]
  pub async fn download_file(&self, attachment_id: u64) -> Result<(), ServerError> {
    let payload = FileStreamRequest::Download { attachment_id }.encode();
    let mut stream = self.connection.open_uni().await?;
    stream.write_all(&payload).await?;
    stream
      .finish()
      .map_err(|_| ServerError::Write(quinn::WriteError::ClosedStream))?;
    Ok(())
  }

  #[allow(dead_code)]
  pub async fn search_chat(
    &self,
    channel_id: ChannelId,
    query: String,
    before_id: u64,
    limit: u16,
  ) -> Result<(), ServerError> {
    self
      .send_control(C2S::ChatSearch {
        channel_id,
        query,
        before_id,
        limit,
      })
      .await
  }

  #[allow(dead_code)]
  pub async fn request_pinned_messages(&self, channel_id: ChannelId) -> Result<(), ServerError> {
    self.send_control(C2S::ChatPinnedReq { channel_id }).await
  }

  // -- admin: text channels --

  pub async fn create_text_channel(&self, name: String) -> Result<(), ServerError> {
    self.send_control(C2S::AdminCreateTextChannel { name }).await
  }

  pub async fn delete_text_channel(&self, channel_id: ChannelId) -> Result<(), ServerError> {
    self.send_control(C2S::AdminDeleteTextChannel { channel_id }).await
  }

  // -- keepalive --

  pub async fn ping(&self) -> Result<(), ServerError> {
    self.send_control(C2S::KeepalivePing).await
  }
}

fn hex_fingerprint(bytes: &[u8]) -> String {
  let digest = Sha256::digest(bytes);
  let mut out = String::with_capacity(95);

  for (index, byte) in digest.iter().enumerate() {
    if index > 0 {
      out.push(':');
    }
    out.push(hex_char(byte >> 4));
    out.push(hex_char(byte & 0x0f));
  }

  out
}

fn hex_char(value: u8) -> char {
  match value {
    0..=9 => (b'0' + value) as char,
    10..=15 => (b'a' + value - 10) as char,
    _ => '0',
  }
}

fn encode_video_stream_packet(packet: &[u8]) -> Result<Vec<u8>, DecodeError> {
  validate_video_stream_packet_len(packet.len())?;

  let mut bytes = Vec::with_capacity(4 + packet.len());
  bytes.extend_from_slice(&(packet.len() as u32).to_le_bytes());
  bytes.extend_from_slice(packet);
  Ok(bytes)
}

fn validate_video_stream_packet_len(len: usize) -> Result<(), DecodeError> {
  if (1..=MAX_VIDEO_FRAME_LEN).contains(&len) {
    Ok(())
  } else {
    Err(DecodeError::InvalidLength {
      len,
      max: MAX_VIDEO_FRAME_LEN,
    })
  }
}

fn validate_video_codec(codec: VideoCodecId) -> Result<(), DecodeError> {
  if codec.is_supported_stream_codec() {
    Ok(())
  } else {
    Err(DecodeError::InvalidEnumValue {
      field: "video codec",
      value: codec as u8,
    })
  }
}

fn decode_video_stream_packet(packet: Vec<u8>) -> Result<ReceivedVideoPacket, ServerError> {
  let Some(packet_type) = packet.first().copied() else {
    return Err(ServerError::Protocol(DecodeError::UnexpectedEof {
      needed: 1,
      remaining: 0,
    }));
  };

  match PacketType::from_u8(packet_type).ok_or(DecodeError::InvalidPacketType(packet_type))? {
    PacketType::VideoFrame => ForwardedVideoFrame::decode_owned(packet)
      .map(ReceivedVideoPacket::Frame)
      .map_err(ServerError::Protocol),
    PacketType::VideoControl => VideoControl::decode_datagram(&packet)
      .map(ReceivedVideoPacket::VideoControl)
      .map_err(ServerError::Protocol),
    _ => Err(ServerError::Protocol(DecodeError::InvalidPacketType(packet_type))),
  }
}

#[derive(Debug)]
enum DecodedDatagram {
  Voice(ForwardedVoicePacket),
  StreamAudio(ForwardedStreamAudioPacket),
  Video(ForwardedVideoFrame),
  VideoControl(VideoControl),
}

fn decode_datagram(data: Bytes) -> Result<DecodedDatagram, DecodeError> {
  let Some(packet_type) = data.first().copied() else {
    return Err(DecodeError::UnexpectedEof {
      needed: 1,
      remaining: 0,
    });
  };

  match PacketType::from_u8(packet_type).ok_or(DecodeError::InvalidPacketType(packet_type))? {
    PacketType::Voice => ForwardedVoicePacket::decode_bytes(data).map(DecodedDatagram::Voice),
    PacketType::VideoFrame => ForwardedVideoFrame::decode(data.as_ref()).map(DecodedDatagram::Video),
    PacketType::StreamAudio => ForwardedStreamAudioPacket::decode_bytes(data).map(DecodedDatagram::StreamAudio),
    PacketType::VideoControl => VideoControl::decode_datagram(data.as_ref()).map(DecodedDatagram::VideoControl),
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn video_stream_packets_are_len_prefixed() {
    let packet = [0x02, 0x11, 0x22, 0x33];
    let framed = encode_video_stream_packet(&packet).unwrap();

    assert_eq!(&framed[..4], &(packet.len() as u32).to_le_bytes());
    assert_eq!(&framed[4..], &packet);
  }

  #[test]
  fn video_stream_packet_rejects_zero_len() {
    let err = encode_video_stream_packet(&[]).unwrap_err();

    assert_eq!(
      err,
      DecodeError::InvalidLength {
        len: 0,
        max: MAX_VIDEO_FRAME_LEN
      }
    );
  }

  #[test]
  fn video_stream_packet_rejects_oversized_len() {
    let err = validate_video_stream_packet_len(MAX_VIDEO_FRAME_LEN + 1).unwrap_err();

    assert_eq!(
      err,
      DecodeError::InvalidLength {
        len: MAX_VIDEO_FRAME_LEN + 1,
        max: MAX_VIDEO_FRAME_LEN
      }
    );
  }

  #[test]
  fn video_stream_decoder_routes_forwarded_video_packets() {
    let packet = vec![
      PacketType::VideoFrame as u8,
      29,
      0,
      0,
      0,
      7,
      0,
      0,
      0,
      11,
      0,
      0,
      0,
      1,
      128,
      2,
      224,
      1,
      VideoCodecId::H264 as u8,
      1,
      2,
      3,
    ];
    let ReceivedVideoPacket::Frame(decoded) = decode_video_stream_packet(packet).unwrap() else {
      panic!("expected video frame packet");
    };

    assert_eq!(decoded.sender_id, 29);
    assert_eq!(decoded.frame.frame_number, 7);
    assert_eq!(decoded.frame.width, 640);
    assert_eq!(decoded.frame.height, 480);
    assert_eq!(decoded.frame.codec, VideoCodecId::H264);
    assert_eq!(decoded.frame.encoded, vec![1, 2, 3]);
  }

  #[test]
  fn video_stream_decoder_routes_video_control_packets() {
    let packet = VideoControl::Pli { user_id: 42 }.encode_datagram();
    let ReceivedVideoPacket::VideoControl(VideoControl::Pli { user_id }) = decode_video_stream_packet(packet).unwrap()
    else {
      panic!("expected video control packet");
    };

    assert_eq!(user_id, 42);
  }

  #[test]
  fn datagram_decoder_routes_forwarded_stream_audio_packets() {
    let DecodedDatagram::StreamAudio(decoded) = decode_datagram(Bytes::from_static(&[
      PacketType::StreamAudio as u8,
      7,
      0,
      0,
      0,
      1,
      2,
      3,
    ]))
    .unwrap() else {
      panic!("expected stream audio datagram");
    };

    assert_eq!(decoded.sender_id, 7);
    assert_eq!(decoded.opus.as_ref(), &[1, 2, 3]);
  }

  #[test]
  fn voice_datagram_decoder_rejects_unknown_packet_type() {
    assert_eq!(
      decode_datagram(Bytes::from_static(&[0xff])).unwrap_err(),
      DecodeError::InvalidPacketType(0xff)
    );
  }

  #[test]
  fn voice_datagram_decoder_accepts_forwarded_voice_packets() {
    let packet = [PacketType::Voice as u8, 42, 0, 0, 0, 9, 0, 1, 2, 3];
    let DecodedDatagram::Voice(decoded) = decode_datagram(Bytes::copy_from_slice(&packet)).unwrap() else {
      panic!("expected voice datagram");
    };

    assert_eq!(decoded.sender_id, 42);
    assert_eq!(decoded.sequence, 9);
    assert_eq!(decoded.opus.as_ref(), &[1, 2, 3]);
  }

  #[test]
  fn datagram_decoder_routes_forwarded_video_packets() {
    let packet = [
      PacketType::VideoFrame as u8,
      29,
      0,
      0,
      0,
      7,
      0,
      0,
      0,
      11,
      0,
      0,
      0,
      1,
      128,
      2,
      224,
      1,
      VideoCodecId::H264 as u8,
      1,
      2,
      3,
    ];
    let DecodedDatagram::Video(decoded) = decode_datagram(Bytes::copy_from_slice(&packet)).unwrap() else {
      panic!("expected video datagram");
    };

    assert_eq!(decoded.sender_id, 29);
    assert_eq!(decoded.frame.frame_number, 7);
    assert_eq!(decoded.frame.width, 640);
    assert_eq!(decoded.frame.height, 480);
    assert_eq!(decoded.frame.codec, VideoCodecId::H264);
    assert_eq!(decoded.frame.encoded, vec![1, 2, 3]);
  }
}
