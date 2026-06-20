use std::{
  collections::VecDeque,
  fmt,
  net::SocketAddr,
  sync::{
    Arc, Mutex as StdMutex,
    atomic::{AtomicBool, Ordering},
  },
  time::{Duration, Instant},
};

use bytes::Bytes;
use quinn::{Connection, Endpoint, VarInt};
use rustls::{
  DigitallySignedStruct, SignatureScheme,
  client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
  pki_types::{CertificateDer, ServerName, UnixTime},
};
use sha2::{Digest, Sha256};
use tokio::{
  io::AsyncWriteExt,
  sync::{Mutex, Notify},
};

use super::protocol::{
  C2S, ChannelId, ControlFrame, ControlMessageType, DecodeError, Role, S2C, UserId, VideoCodecId,
  control::{AuthIdentity, ChatSendAttachment, MAX_CONTROL_MESSAGE_LEN, ScreenShareMetadata, VoiceState},
  data::{
    FileStreamRequest, ForwardedStreamAudioPacket, ForwardedVideoFrame, ForwardedVoicePacket, MAX_VIDEO_FRAME_LEN,
    PacketType, VideoControl, VideoFrame,
  },
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

#[derive(Debug)]
pub struct ReceivedVideoDatagram {
  pub packet: ForwardedVideoFrame,
  pub received_at: Instant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoFrameSend {
  Datagram,
  StreamFallback,
  Dropped,
}

const MAX_PENDING_VIDEO_DATAGRAMS: usize = 24;
const VIDEO_STREAM_FLOW_CONTROL_WINDOW_BYTES: u64 = 64 * 1024 * 1024;
const VIDEO_STREAM_RECEIVE_JITTER_THRESHOLD: Duration = Duration::from_millis(45);
const VIDEO_STREAM_RECEIVE_CADENCE_LOG_INTERVAL: Duration = Duration::from_secs(1);

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
  video_recv_stats: StdMutex<VideoStreamReceiveStats>,
  pending_audio_datagrams: Mutex<VecDeque<ReceivedAudioPacket>>,
  pending_audio_notify: Notify,
  pending_video_datagrams: Mutex<VecDeque<ReceivedVideoDatagram>>,
  pending_video_notify: Notify,
}

#[derive(Debug)]
struct VideoStreamReceiveStats {
  started_at: Instant,
  last_packet_at: Option<Instant>,
  packets: u64,
  bytes_total: u64,
  bytes_max: usize,
  gap_total: Duration,
  gap_max: Duration,
  lock_wait_max: Duration,
  len_read_max: Duration,
  body_read_max: Duration,
  total_read_max: Duration,
  total_read_total: Duration,
}

impl VideoStreamReceiveStats {
  fn new(now: Instant) -> Self {
    Self {
      started_at: now,
      last_packet_at: None,
      packets: 0,
      bytes_total: 0,
      bytes_max: 0,
      gap_total: Duration::ZERO,
      gap_max: Duration::ZERO,
      lock_wait_max: Duration::ZERO,
      len_read_max: Duration::ZERO,
      body_read_max: Duration::ZERO,
      total_read_max: Duration::ZERO,
      total_read_total: Duration::ZERO,
    }
  }
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
    transport.stream_receive_window(VarInt::from_u32(VIDEO_STREAM_FLOW_CONTROL_WINDOW_BYTES as u32));
    transport.send_window(VIDEO_STREAM_FLOW_CONTROL_WINDOW_BYTES);
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
      video_recv_stats: StdMutex::new(VideoStreamReceiveStats::new(Instant::now())),
      pending_audio_datagrams: Mutex::new(VecDeque::new()),
      pending_audio_notify: Notify::new(),
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
    self.pending_audio_notify.notify_waiters();
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
    let log_control = should_log_control_frame(frame.ty);
    if log_control {
      tracing::debug!(
        target: "network::control",
        "[network/control] sending control frame: type={:?} payload_bytes={}",
        frame.ty,
        frame.payload.len()
      );
    }
    if let Err(error) = self.control_send.lock().await.write_all(&bytes).await {
      if log_control {
        tracing::debug!(
          target: "network::control",
          "[network/control] failed to send control frame: type={:?} error={error}",
          frame.ty
        );
      }
      return Err(error.into());
    }
    if log_control {
      tracing::debug!(target: "network::control", "[network/control] sent control frame: type={:?}", frame.ty);
    }
    Ok(())
  }

  pub async fn recv(&self) -> Result<S2C, ServerError> {
    let mut recv = self.control_recv.lock().await;

    loop {
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
      let Some(ty) = ControlMessageType::from_u16(raw_ty) else {
        tracing::debug!(target: "network", "[network] ignoring unknown control message type 0x{raw_ty:04x}: payload_bytes={}", msg_len - 2);
        continue;
      };

      let frame = ControlFrame {
        ty,
        payload: msg_buf[2..].to_vec(),
      };

      match S2C::decode(&frame) {
        Ok(message) => {
          if should_log_control_frame(frame.ty) {
            tracing::debug!(
              target: "network::control",
              "[network/control] received control frame: type={:?} payload_bytes={}",
              frame.ty,
              frame.payload.len()
            );
          }
          return Ok(message);
        }
        Err(DecodeError::InvalidMessageType(value)) => {
          tracing::debug!(target: "network", "[network] ignoring unsupported server control message type 0x{value:04x}: payload_bytes={}", msg_len - 2);
        }
        Err(error) => return Err(error.into()),
      }
    }
  }

  // -- auth --

  pub async fn authenticate(&self, identity: AuthIdentity) -> Result<(), ServerError> {
    self.send_control(C2S::Auth(identity)).await
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
      let notified = self.pending_audio_notify.notified();
      if let Some(packet) = self.pending_audio_datagrams.lock().await.pop_front() {
        return Ok(packet);
      }
      if let Some(error) = self.connection.close_reason() {
        return Err(ServerError::Connection(error));
      }
      notified.await;
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
    validate_video_stream_packet_len(packet.len())?;
    let mut framed = Vec::with_capacity(4 + packet.len());
    framed.extend_from_slice(&(packet.len() as u32).to_le_bytes());
    framed.extend_from_slice(packet);
    let mut send = self.video_send.lock().await;
    send.write_all(&framed).await?;
    send.flush().await?;
    Ok(())
  }

  #[allow(dead_code)]
  pub async fn send_video_frame(&self, frame: VideoFrame) -> Result<(), ServerError> {
    validate_video_codec(frame.codec)?;
    let packet = frame.encode_packet();
    self.send_video_packet(&packet).await
  }

  pub async fn send_live_video_frame(&self, frame: &VideoFrame) -> Result<VideoFrameSend, ServerError> {
    validate_video_codec(frame.codec)?;
    let packet = frame.encode_packet();
    match self.connection.send_datagram(packet.clone()) {
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
    let started_at = Instant::now();
    let mut recv = self.video_recv.lock().await;
    let lock_wait = started_at.elapsed();

    let mut len_buf = [0u8; 4];
    let len_started_at = Instant::now();
    recv.read_exact(&mut len_buf).await?;
    let len_read = len_started_at.elapsed();
    let packet_len = u32::from_le_bytes(len_buf) as usize;
    validate_video_stream_packet_len(packet_len)?;

    let mut packet = vec![0u8; packet_len];
    let body_started_at = Instant::now();
    recv.read_exact(&mut packet).await?;
    let body_read = body_started_at.elapsed();
    let total_read = started_at.elapsed();
    drop(recv);

    self.log_video_stream_receive(packet_len, lock_wait, len_read, body_read, total_read);
    Ok(packet)
  }

  fn log_video_stream_receive(
    &self,
    packet_len: usize,
    lock_wait: Duration,
    len_read: Duration,
    body_read: Duration,
    total_read: Duration,
  ) {
    let now = Instant::now();
    let mut stats = match self.video_recv_stats.lock() {
      Ok(stats) => stats,
      Err(error) => error.into_inner(),
    };

    let gap = stats.last_packet_at.map(|last| now.duration_since(last));
    stats.last_packet_at = Some(now);
    stats.packets += 1;
    stats.bytes_total += packet_len as u64;
    stats.bytes_max = stats.bytes_max.max(packet_len);
    if let Some(gap) = gap {
      stats.gap_total += gap;
      stats.gap_max = stats.gap_max.max(gap);
    }
    stats.lock_wait_max = stats.lock_wait_max.max(lock_wait);
    stats.len_read_max = stats.len_read_max.max(len_read);
    stats.body_read_max = stats.body_read_max.max(body_read);
    stats.total_read_max = stats.total_read_max.max(total_read);
    stats.total_read_total += total_read;

    if gap.is_some_and(|gap| gap >= VIDEO_STREAM_RECEIVE_JITTER_THRESHOLD)
      || len_read >= VIDEO_STREAM_RECEIVE_JITTER_THRESHOLD
      || body_read >= VIDEO_STREAM_RECEIVE_JITTER_THRESHOLD
      || total_read >= VIDEO_STREAM_RECEIVE_JITTER_THRESHOLD
    {
      tracing::debug!(
        target: "video::watch",
        "watched stream packet receive jitter packet_bytes={} gap_ms={:.1} total_read_ms={:.1} lock_wait_ms={:.1} len_read_ms={:.1} body_read_ms={:.1}",
        packet_len,
        gap.map(duration_ms).unwrap_or(0.0),
        duration_ms(total_read),
        duration_ms(lock_wait),
        duration_ms(len_read),
        duration_ms(body_read),
      );
    }

    if now.duration_since(stats.started_at) < VIDEO_STREAM_RECEIVE_CADENCE_LOG_INTERVAL {
      return;
    }

    let packets = stats.packets.max(1);
    tracing::debug!(
      target: "video::watch",
      "watched stream packet receive cadence packets={} bytes_avg={} bytes_max={} gap_avg_ms={:.1} gap_max_ms={:.1} total_read_avg_ms={:.1} total_read_max_ms={:.1} lock_wait_max_ms={:.1} len_read_max_ms={:.1} body_read_max_ms={:.1}",
      stats.packets,
      stats.bytes_total / packets,
      stats.bytes_max,
      duration_ms(stats.gap_total / packets as u32),
      duration_ms(stats.gap_max),
      duration_ms(stats.total_read_total / packets as u32),
      duration_ms(stats.total_read_max),
      duration_ms(stats.lock_wait_max),
      duration_ms(stats.len_read_max),
      duration_ms(stats.body_read_max),
    );

    *stats = VideoStreamReceiveStats::new(now);
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
  ) -> Result<Option<ReceivedVideoDatagram>, ServerError> {
    loop {
      if stop.load(Ordering::Relaxed) {
        return Ok(None);
      }
      let notified = self.pending_video_notify.notified();
      if let Some(packet) = self.pending_video_datagrams.lock().await.pop_front() {
        return Ok(Some(packet));
      }
      if let Some(error) = self.connection.close_reason() {
        return Err(ServerError::Connection(error));
      }
      if stop.load(Ordering::Relaxed) {
        return Ok(None);
      }
      notified.await;
    }
  }

  pub async fn run_datagram_demuxer(&self) {
    tracing::debug!(target: "network", "[network] datagram demuxer started");
    loop {
      match self.connection.read_datagram().await {
        Ok(data) => {
          let received_at = Instant::now();
          match decode_datagram(data) {
            Ok(datagram) => self.dispatch_datagram(datagram, received_at).await,
            Err(error) => tracing::debug!(target: "network", "[network] ignored malformed datagram: {error}"),
          }
        }
        Err(error) => {
          tracing::debug!(target: "network", "[network] datagram demuxer stopped: {error}");
          self.connection.close(VarInt::from_u32(0), b"datagram demuxer stopped");
          self.pending_audio_notify.notify_waiters();
          self.pending_video_notify.notify_waiters();
          break;
        }
      }
    }
  }

  async fn dispatch_datagram(&self, datagram: DecodedDatagram, received_at: Instant) {
    match datagram {
      DecodedDatagram::Voice(packet) => {
        self
          .pending_audio_datagrams
          .lock()
          .await
          .push_back(ReceivedAudioPacket::Voice(packet));
        self.pending_audio_notify.notify_one();
      }
      DecodedDatagram::StreamAudio(packet) => {
        self
          .pending_audio_datagrams
          .lock()
          .await
          .push_back(ReceivedAudioPacket::Stream(packet));
        self.pending_audio_notify.notify_one();
      }
      DecodedDatagram::VideoControl(control) => {
        self
          .pending_audio_datagrams
          .lock()
          .await
          .push_back(ReceivedAudioPacket::VideoControl(control));
        self.pending_audio_notify.notify_one();
      }
      DecodedDatagram::Video(packet) => {
        let mut pending = self.pending_video_datagrams.lock().await;
        if pending.len() >= MAX_PENDING_VIDEO_DATAGRAMS {
          pending.pop_front();
          tracing::debug!(
            target: "video",
            "[video] dropped pending stale video datagram to preserve latency: max_queue={MAX_PENDING_VIDEO_DATAGRAMS}"
          );
        }
        pending.push_back(ReceivedVideoDatagram { packet, received_at });
        self.pending_video_notify.notify_one();
      }
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

fn should_log_control_frame(ty: ControlMessageType) -> bool {
  matches!(
    ty,
    ControlMessageType::AuthIdentity
      | ControlMessageType::AuthResponse
      | ControlMessageType::ChannelJoin
      | ControlMessageType::ChannelLeave
      | ControlMessageType::UserJoinedChannel
      | ControlMessageType::UserLeftChannel
      | ControlMessageType::VoiceStateUpdate
      | ControlMessageType::UserVoiceState
      | ControlMessageType::ServerError
  )
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

#[cfg(test)]
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

fn duration_ms(duration: Duration) -> f64 {
  duration.as_secs_f64() * 1000.0
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
#[path = "../../tests/unit/network/server.rs"]
mod tests;
