use std::{
  sync::Arc,
  time::{Duration, Instant},
};

use parking_lot::Mutex;

use super::video::{VideoPacketQueue, VideoReceiverDebugSnapshot};
use crate::{
  network::{
    protocol::{
      UserId,
      data::{ForwardedVideoFrame, VideoFrame},
    },
    server::Server,
  },
  services::video::{NativeVideoBackend, VideoBroadcast, VideoBroadcastConfig, VideoError, VideoFrameLoopback},
  storage::AppSettings,
};

pub(super) trait StreamWatchSession: Clone + Send + Sync + 'static {
  fn server(&self) -> Option<Arc<Server>>;
  fn reconnect_watch_target_available(&self, user_id: UserId) -> bool;
  fn set_watching_user(&self, user_id: Option<UserId>);
  fn ensure_stream_audio_playback(&self, settings: AppSettings) -> Result<(), String>;
}

pub(super) struct StreamRuntime {
  broadcast: Mutex<Option<VideoBroadcast>>,
  packet_queue: Mutex<Arc<VideoPacketQueue>>,
  pending_reconnect_watch_user_id: Mutex<Option<UserId>>,
  receiver_debug: Mutex<VideoReceiverDebugSnapshot>,
}

impl StreamRuntime {
  pub(super) fn new() -> Self {
    Self {
      broadcast: Mutex::new(None),
      packet_queue: Mutex::new(Arc::new(VideoPacketQueue::new())),
      pending_reconnect_watch_user_id: Mutex::new(None),
      receiver_debug: Mutex::new(VideoReceiverDebugSnapshot::default()),
    }
  }

  pub(super) fn reset_packet_queue(&self) -> Arc<VideoPacketQueue> {
    let queue = Arc::new(VideoPacketQueue::new());
    *self.packet_queue.lock() = queue.clone();
    *self.receiver_debug.lock() = VideoReceiverDebugSnapshot::default();
    queue
  }

  pub(super) fn current_packet_queue(&self) -> Arc<VideoPacketQueue> {
    self.packet_queue.lock().clone()
  }

  pub(super) fn push_loopback_frame(&self, sender_id: UserId, frame: VideoFrame) {
    self
      .current_packet_queue()
      .push(ForwardedVideoFrame { sender_id, frame });
  }

  pub(super) fn set_receiver_debug_snapshot(&self, snapshot: VideoReceiverDebugSnapshot) {
    *self.receiver_debug.lock() = snapshot;
  }

  pub(super) fn receiver_debug_snapshot(&self) -> VideoReceiverDebugSnapshot {
    self.receiver_debug.lock().clone()
  }

  pub(super) fn start_broadcast(
    &self,
    server: Arc<Server>,
    config: VideoBroadcastConfig,
    loopback: VideoFrameLoopback,
  ) -> Result<NativeVideoBackend, VideoError> {
    let broadcast = VideoBroadcast::start_with_loopback(server, config, Some(loopback))?;
    let backend = broadcast.backend();
    let mut stored = self.broadcast.lock();
    stored.replace(broadcast);
    Ok(backend)
  }

  pub(super) fn stop_broadcast(&self) -> bool {
    let stopped = self.broadcast.lock().take().is_some();
    stopped
  }

  pub(super) fn has_broadcast(&self) -> bool {
    self.broadcast.lock().is_some()
  }

  pub(super) fn request_local_keyframe(&self) -> bool {
    let broadcast = self.broadcast.lock();
    let Some(broadcast) = broadcast.as_ref() else {
      return false;
    };
    broadcast.request_keyframe();
    true
  }

  pub(super) fn set_pending_reconnect_watch(&self, user_id: Option<UserId>) {
    *self.pending_reconnect_watch_user_id.lock() = user_id;
  }

  pub(super) fn clear_pending_reconnect_watch(&self) {
    self.set_pending_reconnect_watch(None);
  }

  pub(super) fn has_pending_reconnect_watch(&self) -> bool {
    self.pending_reconnect_watch_user_id.lock().is_some()
  }

  pub(super) fn pending_reconnect_watch_user_id(&self) -> Option<UserId> {
    *self.pending_reconnect_watch_user_id.lock()
  }

  pub(super) fn pending_reconnect_watch_matches(&self, user_id: UserId) -> bool {
    self.pending_reconnect_watch_user_id() == Some(user_id)
  }

  pub(super) fn take_pending_reconnect_watch_if_matches(&self, user_id: UserId) -> bool {
    let mut pending = self.pending_reconnect_watch_user_id.lock();
    if *pending == Some(user_id) {
      *pending = None;
      true
    } else {
      false
    }
  }

  pub(super) async fn restore_pending_reconnect_watch<S>(&self, session: S, settings: AppSettings, timeout: Duration)
  where
    S: StreamWatchSession,
  {
    let Some(user_id) = self.pending_reconnect_watch_user_id() else {
      return;
    };

    tracing::info!(target: "video", "[video] waiting to restore watched stream after reconnect: user={user_id}");
    let started_at = Instant::now();
    loop {
      if !self.pending_reconnect_watch_matches(user_id) {
        return;
      }

      if session.reconnect_watch_target_available(user_id) {
        if !self.take_pending_reconnect_watch_if_matches(user_id) {
          return;
        }
        if let Err(error) = request_reconnect_stream_view(session.server(), user_id).await {
          tracing::warn!(target: "video", "[video] failed to restore watched stream after reconnect: user={user_id} error={error}");
          return;
        }

        session.set_watching_user(Some(user_id));
        if let Err(error) = session.ensure_stream_audio_playback(settings) {
          tracing::warn!(target: "audio::decode", "[audio:decode] stream playback unavailable after reconnect restore: {error}");
        }
        tracing::info!(target: "video", "[video] restored watched stream after reconnect: user={user_id}");
        return;
      }

      if started_at.elapsed() >= timeout {
        if self.take_pending_reconnect_watch_if_matches(user_id) {
          tracing::info!(target: "video",
            "[video] skipped watched stream restore after reconnect: user={user_id} reason=stream not advertised"
          );
        }
        return;
      }

      tokio::time::sleep(Duration::from_millis(100)).await;
    }
  }
}

async fn request_reconnect_stream_view(server: Option<Arc<Server>>, user_id: UserId) -> Result<(), String> {
  let Some(server) = server else {
    return Err("no connected server".to_owned());
  };
  tracing::info!(target: "video", "[video] requesting reconnect stream restore for user {user_id}");
  server
    .view_screen_share(user_id)
    .await
    .map_err(|error| error.to_string())?;
  match server.request_keyframe_stream(user_id).await {
    Ok(()) => {
      tracing::debug!(target: "video", "[video] keyframe requested on restored stream for user {user_id}");
    }
    Err(stream_error) => {
      tracing::warn!(target: "video",
        "[video] restored stream keyframe request failed for user {user_id}: {stream_error}; trying datagram"
      );
      if let Err(datagram_error) = server.request_keyframe(user_id) {
        return Err(datagram_error.to_string());
      }
      tracing::debug!(target: "video", "[video] restored stream keyframe requested by datagram for user {user_id}");
    }
  }
  Ok(())
}

impl Default for StreamRuntime {
  fn default() -> Self {
    Self::new()
  }
}
