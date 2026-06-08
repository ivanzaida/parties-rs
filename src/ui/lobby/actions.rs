use lurq::app::ctx::Ctx;

use super::{
  ChatHistoryAction, ChatHistoryRequest, ReceiverAction, ReconnectAction, ReconnectRequest, SendChatAction,
  SendChatInput, StartStreamAction, StartStreamInput, StopStreamAction, StopWatchingAction, WatchStreamAction,
};
use crate::{
  network::protocol::VideoCodecId,
  services::{logger, video::VideoBroadcastConfig},
  session::{ConnectedServerInfo, ServerSession},
  storage::{Storage, StoredServer},
  ui::connect_server::connect_and_store,
};

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
  ctx.future_action(move |request: ChatHistoryRequest| {
    let session = session.clone();
    async move {
      let server = session.server().ok_or_else(|| "No connected server.".to_owned())?;
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
  ctx.future_action(move |input: SendChatInput| {
    let session = session.clone();
    async move {
      let text = input.text.trim().to_owned();
      if text.is_empty() {
        return Ok(());
      }

      let server = session.server().ok_or_else(|| "No connected server.".to_owned())?;
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
  ctx.future_action(move |input: StartStreamInput| {
    let storage = storage.clone();
    let session = session.clone();
    async move {
      let server = session.server().ok_or_else(|| "No connected server.".to_owned())?;
      let settings = storage
        .as_ref()
        .map(|storage| storage.load_settings().map_err(|error| error.to_string()))
        .transpose()?
        .unwrap_or_default();
      let codec = stream_codec_id(&settings.video_codec)?;
      if input.width == 0 || input.height == 0 {
        return Err("Selected stream source has no capture dimensions.".to_owned());
      }
      let (width, height) = scaled_stream_dimensions(input.width, input.height, settings.video_scale_percent);
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
      };
      logger::log(&format!(
        "[video] starting broadcast: source={:?}/{} source_size={}x{} output={}x{} codec={:?} fps={} bitrate={}kbps",
        config.source_kind,
        config.source_id,
        config.source_width,
        config.source_height,
        config.output_width,
        config.output_height,
        config.codec,
        config.fps,
        config.bitrate_kbps
      ));
      if let Err(error) = session.start_video_broadcast(config) {
        logger::log(&format!("[video] failed to start local broadcaster: {error}"));
        return Err(error);
      }
      if let Err(error) = server.start_screen_share(codec, width, height).await {
        let error = error.to_string();
        logger::log(&format!("[video] server rejected screen-share start: {error}"));
        session.stop_video_broadcast();
        return Err(error);
      }
      logger::log(&format!(
        "[video] screen-share start announced: codec={codec:?} size={width}x{height}"
      ));
      Ok(())
    }
  })
}

fn stream_codec_id(codec: &str) -> Result<VideoCodecId, String> {
  match codec.trim().to_ascii_lowercase().replace([' ', '.', '-'], "").as_str() {
    "av1" => Ok(VideoCodecId::Av1),
    "h265" | "hevc" => Ok(VideoCodecId::H265),
    "h264" | "avc" => Ok(VideoCodecId::H264),
    _ => Err("Video codec must be AV1, H.265, or H.264.".to_owned()),
  }
}

fn scaled_stream_dimensions(width: u16, height: u16, scale_percent: i32) -> (u16, u16) {
  let scale = scale_percent.clamp(10, 100) as u32;
  let scaled_width = (u32::from(width) * scale / 100).clamp(1, u32::from(u16::MAX));
  let scaled_height = (u32::from(height) * scale / 100).clamp(1, u32::from(u16::MAX));
  (even_dimension(scaled_width), even_dimension(scaled_height))
}

fn even_dimension(value: u32) -> u16 {
  (value.clamp(2, u32::from(u16::MAX)) as u16) & !1u16
}

pub(super) fn stop_stream_action(ctx: &mut Ctx, session: ServerSession) -> StopStreamAction {
  ctx.future_action(move |()| {
    let session = session.clone();
    async move {
      logger::log("[video] stopping broadcast");
      session.stop_video_broadcast();
      let server = session.server().ok_or_else(|| "No connected server.".to_owned())?;
      if let Err(error) = server.stop_screen_share().await {
        let error = error.to_string();
        logger::log(&format!("[video] failed to announce screen-share stop: {error}"));
        return Err(error);
      }
      logger::log("[video] screen-share stop announced");
      Ok(())
    }
  })
}

pub(super) fn watch_stream_action(ctx: &mut Ctx, session: ServerSession) -> WatchStreamAction {
  ctx.future_action(move |user_id| {
    let session = session.clone();
    async move {
      if session.info().is_some_and(|info| info.user_id == user_id) {
        session.set_watching_user(Some(user_id));
        return Ok(());
      }

      let server = session.server().ok_or_else(|| "No connected server.".to_owned())?;
      server
        .view_screen_share(user_id)
        .await
        .map_err(|error| error.to_string())?;
      if let Err(error) = server.request_keyframe(user_id) {
        return Err(error.to_string());
      }
      session.set_watching_user(Some(user_id));
      Ok(())
    }
  })
}

pub(super) fn stop_watching_action(ctx: &mut Ctx, session: ServerSession) -> StopWatchingAction {
  ctx.future_action(move |()| {
    let session = session.clone();
    async move {
      let watching_local_stream = session
        .lobby()
        .watching_user_id
        .is_some_and(|user_id| session.info().is_some_and(|info| info.user_id == user_id));

      if watching_local_stream {
        session.set_watching_user(None);
        return Ok(());
      }

      let server = session.server().ok_or_else(|| "No connected server.".to_owned())?;
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
  ctx.future_action(move |request: ReconnectRequest| {
    let storage = storage.clone();
    let session = session.clone();
    async move {
      if request.delay_ms > 0 {
        tokio::time::sleep(std::time::Duration::from_millis(request.delay_ms)).await;
      }

      let storage = storage.ok_or_else(|| "Local storage is unavailable.".to_owned())?;
      let server = storage
        .load_server(&request.address)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "Saved server credentials were not found.".to_owned())?;
      reconnect_saved_server(server, storage, session).await
    }
  })
}

async fn reconnect_saved_server(
  server: StoredServer,
  storage: Storage,
  session: ServerSession,
) -> Result<ConnectedServerInfo, String> {
  let display_name = if server.display_name.trim().is_empty() {
    storage.load_settings().map_err(|error| error.to_string())?.display_name
  } else {
    server.display_name.clone()
  };

  connect_and_store(
    server.address,
    server.server_password,
    display_name,
    Some(storage),
    Some(session),
  )
  .await
}
