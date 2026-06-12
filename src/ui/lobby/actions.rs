use std::time::Duration;

use lurq::app::ctx::Ctx;

use super::{
  ChatHistoryAction, ChatHistoryRequest, ReceiverAction, ReconnectAction, ReconnectRequest, SendChatAction,
  SendChatInput, StartStreamAction, StartStreamInput, StopStreamAction, StopWatchingAction, WatchStreamAction,
};
use crate::{
  network::protocol::{ChannelId, VideoCodecId},
  services::video::VideoBroadcastConfig,
  session::{
    ConnectedServerInfo, ServerSession,
    chat_commands::{ChatCommand, ChatCommandRegistry},
  },
  storage::{AppSettings, Storage, StoredServer},
  ui::connect_server::{ConnectErrorCopy, connect_and_store},
};

const RECONNECT_STREAM_RESTORE_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone)]
struct LobbyActionCopy {
  no_connected_server: String,
  stream_no_dimensions: String,
  video_codec_invalid: String,
  stream_start_failed: String,
  stream_too_small: String,
  storage_unavailable: String,
  saved_credentials_missing: String,
  connect_errors: ConnectErrorCopy,
}

impl LobbyActionCopy {
  fn from_ctx(ctx: &mut Ctx) -> Self {
    Self {
      no_connected_server: ctx.t("lobby.error.no_connected_server").to_string(),
      stream_no_dimensions: ctx.t("lobby.error.stream_no_dimensions").to_string(),
      video_codec_invalid: ctx.t("lobby.error.video_codec_invalid").to_string(),
      stream_start_failed: ctx.t("lobby.error.stream_start_failed").to_string(),
      stream_too_small: ctx.t("lobby.error.stream_too_small").to_string(),
      storage_unavailable: ctx.t("lobby.error.storage_unavailable").to_string(),
      saved_credentials_missing: ctx.t("lobby.error.saved_credentials_missing").to_string(),
      connect_errors: ConnectErrorCopy::from_ctx(ctx),
    }
  }

  fn stream_too_small(&self, width: u16, height: u16) -> String {
    self
      .stream_too_small
      .replace("{{width}}", &width.to_string())
      .replace("{{height}}", &height.to_string())
  }
}

pub(super) fn receiver_action(ctx: &mut Ctx, session: ServerSession) -> ReceiverAction {
  ctx.future_action(move |()| {
    let session = session.clone();
    async move {
      session.run_lobby_receiver().await;
      Ok(())
    }
  })
}

pub(super) fn chat_history_action(ctx: &mut Ctx, session: ServerSession) -> ChatHistoryAction {
  let copy = LobbyActionCopy::from_ctx(ctx);
  ctx.future_action(move |request: ChatHistoryRequest| {
    let session = session.clone();
    let copy = copy.clone();
    async move {
      let server = session.server().ok_or(copy.no_connected_server)?;
      if let Err(error) = server
        .request_chat_history(request.channel_id, request.before_id, 50)
        .await
      {
        session.finish_chat_history_request(request.channel_id, true);
        return Err(error.to_string());
      }
      Ok(())
    }
  })
}

pub(super) fn send_chat_action(ctx: &mut Ctx, session: ServerSession) -> SendChatAction {
  let copy = LobbyActionCopy::from_ctx(ctx);
  ctx.future_action(move |input: SendChatInput| {
    let session = session.clone();
    let copy = copy.clone();
    async move {
      let text = input.text.trim().to_owned();
      if text.is_empty() {
        return Ok(());
      }

      let registry = ChatCommandRegistry::new();
      match registry.parse(&text) {
        Ok(Some(ChatCommand::RestartAudioReceiver { user_id })) => {
          let restarted = session.restart_audio_receiver(user_id);
          if !restarted {
            tracing::warn!(
              target: "audio::decode",
              "[audio:decode] restart audio receiver command found no active receiver state for user {user_id}"
            );
          }
          return Ok(());
        }
        Ok(None) => {}
        Err(error) => {
          session.set_lobby_error_notice(error.clone());
          return Err(error);
        }
      }

      let server = session.server().ok_or(copy.no_connected_server)?;
      server
        .send_chat_text(input.channel_id, text)
        .await
        .map_err(|error| error.to_string())?;
      Ok(())
    }
  })
}

pub(super) fn start_stream_action(
  ctx: &mut Ctx,
  storage: Option<Storage>,
  session: ServerSession,
) -> StartStreamAction {
  let copy = LobbyActionCopy::from_ctx(ctx);
  ctx.future_action(move |input: StartStreamInput| {
    let storage = storage.clone();
    let session = session.clone();
    let copy = copy.clone();
    async move {
      let server = session.server().ok_or(copy.no_connected_server.clone())?;
      let settings = storage
        .as_ref()
        .map(|storage| storage.load_settings().map_err(|error| error.to_string()))
        .transpose()?
        .unwrap_or_default();
      let codec = stream_codec_id(&settings.video_codec, &copy.video_codec_invalid)?;
      if input.width == 0 || input.height == 0 {
        return Err(copy.stream_no_dimensions.clone());
      }
      let (width, height) =
        scaled_stream_dimensions(input.width, input.height, settings.video_scale_percent, &copy)?;
      let config = VideoBroadcastConfig {
        source_kind: input.source_kind,
        source_id: input.source_id,
        source_width: input.width,
        source_height: input.height,
        output_width: width,
        output_height: height,
        codec,
        fps: settings.video_fps.clamp(1, 120) as u32,
        bitrate_kbps: (settings.video_bitrate_mbps.max(0.1) * 1000.0).round() as u32,
        audio_enabled: input.audio_enabled,
      };
      tracing::info!(target: "video::encode",
        "[video:encode] starting broadcast: source={:?}/{} source_size={}x{} output={}x{} codec={:?} fps={} bitrate={}kbps audio={}",
        config.source_kind,
        config.source_id,
        config.source_width,
        config.source_height,
        config.output_width,
        config.output_height,
        config.codec,
        config.fps,
        config.bitrate_kbps,
        config.audio_enabled
      );
      if let Err(error) = session.start_video_broadcast(config, &copy.no_connected_server) {
        tracing::error!(target: "video::encode", "[video:encode] failed to start local broadcaster: {error}");
        let message = format!("{}: {error}", copy.stream_start_failed);
        return Err(message);
      }
      if let Err(error) = server.start_screen_share(codec, width, height).await {
        let error = error.to_string();
        tracing::warn!(target: "video", "[video] server rejected screen-share start: {error}");
        session.stop_video_broadcast();
        return Err(error);
      }
      tracing::info!(target: "video",
        "[video] screen-share start announced: codec={codec:?} size={width}x{height}"
      );
      Ok(())
    }
  })
}

fn stream_codec_id(codec: &str, invalid_error: &str) -> Result<VideoCodecId, String> {
  #[cfg(target_os = "macos")]
  if codec.trim().eq_ignore_ascii_case("av1") {
    return Ok(VideoCodecId::H265);
  }

  match codec.trim().to_ascii_lowercase().replace([' ', '.', '-'], "").as_str() {
    "av1" => Ok(VideoCodecId::Av1),
    "h265" | "hevc" => Ok(VideoCodecId::H265),
    "h264" | "avc" => Ok(VideoCodecId::H264),
    _ => Err(invalid_error.to_owned()),
  }
}

const STREAM_DIMENSION_ALIGNMENT: u32 = 16;
const MIN_STREAM_DIMENSION: u32 = 128;

fn scaled_stream_dimensions(
  width: u16,
  height: u16,
  scale_percent: i32,
  copy: &LobbyActionCopy,
) -> Result<(u16, u16), String> {
  let scale = scale_percent.clamp(10, 100) as u32;
  let scaled_width = (u32::from(width) * scale / 100).clamp(1, u32::from(u16::MAX));
  let scaled_height = (u32::from(height) * scale / 100).clamp(1, u32::from(u16::MAX));
  let width = aligned_stream_dimension(scaled_width);
  let height = aligned_stream_dimension(scaled_height);
  if u32::from(width) < MIN_STREAM_DIMENSION || u32::from(height) < MIN_STREAM_DIMENSION {
    return Err(copy.stream_too_small(width, height));
  }
  Ok((width, height))
}

fn aligned_stream_dimension(value: u32) -> u16 {
  let max_aligned = u32::from(u16::MAX) / STREAM_DIMENSION_ALIGNMENT * STREAM_DIMENSION_ALIGNMENT;
  let value = value.clamp(STREAM_DIMENSION_ALIGNMENT, max_aligned);
  (value / STREAM_DIMENSION_ALIGNMENT * STREAM_DIMENSION_ALIGNMENT) as u16
}

pub(super) fn stop_stream_action(ctx: &mut Ctx, session: ServerSession) -> StopStreamAction {
  let copy = LobbyActionCopy::from_ctx(ctx);
  ctx.future_action(move |()| {
    let session = session.clone();
    let copy = copy.clone();
    async move {
      tracing::info!(target: "video", "[video] stopping broadcast");
      session.stop_video_broadcast();
      let server = session.server().ok_or(copy.no_connected_server)?;
      if let Err(error) = server.stop_screen_share().await {
        let error = error.to_string();
        tracing::warn!(target: "video", "[video] failed to announce screen-share stop: {error}");
        return Err(error);
      }
      tracing::info!(target: "video", "[video] screen-share stop announced");
      Ok(())
    }
  })
}

pub(super) fn watch_stream_action(
  ctx: &mut Ctx,
  storage: Option<Storage>,
  session: ServerSession,
) -> WatchStreamAction {
  let copy = LobbyActionCopy::from_ctx(ctx);
  ctx.future_action(move |user_id| {
    let storage = storage.clone();
    let session = session.clone();
    let copy = copy.clone();
    async move {
      let server = session.server().ok_or(copy.no_connected_server)?;
      tracing::info!(target: "video", "[video] requesting stream view for user {user_id}");
      server
        .view_screen_share(user_id)
        .await
        .map_err(|error| error.to_string())?;
      match server.request_keyframe_stream(user_id).await {
        Ok(()) => {
          tracing::debug!(target: "video", "[video] keyframe requested on video stream for user {user_id}");
        }
        Err(stream_error) => {
          tracing::warn!(target: "video", "[video] stream keyframe request failed for user {user_id}: {stream_error}; trying datagram");
          if let Err(datagram_error) = server.request_keyframe(user_id) {
            tracing::warn!(target: "video", "[video] datagram keyframe request failed for user {user_id}: {datagram_error}");
            return Err(datagram_error.to_string());
          }
          tracing::debug!(target: "video", "[video] keyframe requested by datagram for user {user_id}");
        }
      }
      tracing::info!(target: "video", "[video] stream view active for user {user_id}");
      session.set_watching_user(Some(user_id));
      let settings = storage
        .as_ref()
        .and_then(|storage| storage.load_settings().ok())
        .unwrap_or_else(AppSettings::default);
      if let Err(error) = session.ensure_stream_audio_playback(settings) {
        tracing::warn!(target: "audio::decode", "[audio:decode] stream playback unavailable: {error}");
      }
      Ok(())
    }
  })
}

pub(super) fn stop_watching_action(ctx: &mut Ctx, session: ServerSession) -> StopWatchingAction {
  let copy = LobbyActionCopy::from_ctx(ctx);
  ctx.future_action(move |()| {
    let session = session.clone();
    let copy = copy.clone();
    async move {
      let server = session.server().ok_or(copy.no_connected_server)?;
      tracing::info!(target: "video", "[video] unsubscribing from watched stream");
      server
        .unsubscribe_screen_share()
        .await
        .map_err(|error| error.to_string())?;
      session.set_watching_user(None);
      Ok(())
    }
  })
}

pub(super) fn reconnect_action(ctx: &mut Ctx, storage: Option<Storage>, session: ServerSession) -> ReconnectAction {
  let copy = LobbyActionCopy::from_ctx(ctx);
  ctx.future_action(move |request: ReconnectRequest| {
    let storage = storage.clone();
    let session = session.clone();
    let copy = copy.clone();
    async move {
      if request.delay_ms > 0 {
        tokio::time::sleep(std::time::Duration::from_millis(request.delay_ms)).await;
      }

      let storage = storage.ok_or(copy.storage_unavailable.clone())?;
      let server = storage
        .load_server(&request.address)
        .map_err(|error| error.to_string())?
        .ok_or(copy.saved_credentials_missing.clone())?;
      reconnect_saved_server(server, storage, session, copy.connect_errors).await
    }
  })
}

async fn reconnect_saved_server(
  server: StoredServer,
  storage: Storage,
  session: ServerSession,
  errors: ConnectErrorCopy,
) -> Result<ConnectedServerInfo, String> {
  let reconnect_channel_id = session.lobby().selected_channel_id;
  let reconnect_voice_state = reconnect_channel_id.and_then(|_| session.local_voice_state());
  let display_name = if server.display_name.trim().is_empty() {
    storage.load_settings().map_err(|error| error.to_string())?.display_name
  } else {
    server.display_name.clone()
  };

  let info = connect_and_store(
    server.address,
    server.server_password,
    display_name,
    Some(storage.clone()),
    Some(session.clone()),
    errors,
  )
  .await?;

  if let Some(channel_id) = reconnect_channel_id {
    rejoin_previous_voice_channel(&session, &storage, channel_id, reconnect_voice_state).await;
  }
  if session.has_pending_reconnect_watch() {
    let restore_session = session.clone();
    let settings = storage.load_settings().unwrap_or_else(|_| AppSettings::default());
    tokio::spawn(async move {
      restore_session
        .restore_pending_reconnect_watch(settings, RECONNECT_STREAM_RESTORE_TIMEOUT)
        .await;
    });
  }

  Ok(info)
}

async fn rejoin_previous_voice_channel(
  session: &ServerSession,
  storage: &Storage,
  channel_id: ChannelId,
  voice_state: Option<(bool, bool)>,
) {
  let Some(server) = session.server() else {
    tracing::warn!(target: "lobby", "[lobby] skipped voice rejoin after reconnect: channel={channel_id} reason=no connected server");
    return;
  };
  let settings = storage.load_settings().unwrap_or_else(|_| AppSettings::default());
  let (mut muted, deafened) = voice_state.unwrap_or((settings.start_muted_when_joining, false));
  if deafened {
    muted = true;
  }

  tracing::info!(target: "lobby",
    "[lobby] rejoining previous voice channel after reconnect: channel={channel_id} muted={muted} deafened={deafened}"
  );
  if let Err(error) = server.join_channel(channel_id).await {
    tracing::warn!(target: "lobby", "[lobby] previous voice channel rejoin failed: channel={channel_id} error={error}");
    return;
  }

  session.select_channel(channel_id);
  if let Err(error) = server.update_voice_state(muted, deafened).await {
    tracing::warn!(target: "voice", "[voice] failed to announce local state after reconnect rejoin: channel={channel_id} error={error}");
    return;
  }

  session.set_local_voice_state(muted, deafened);
  match session.start_voice(settings, "") {
    Ok(()) => tracing::info!(target: "voice", "[voice] local voice capture restarted after reconnect rejoin"),
    Err(error) => tracing::warn!(target: "voice", "[voice] local voice capture failed after reconnect rejoin: {error}"),
  }
}
