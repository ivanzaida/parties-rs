use std::time::{Duration, SystemTime, UNIX_EPOCH};

use lurq::app::ctx::Ctx;

use super::{
  ChatHistoryAction, ChatHistoryRequest, ReceiverAction, ReconnectAction, ReconnectRequest, SendChatAction,
  SendChatInput, StartStreamAction, StartStreamInput, StopStreamAction, StopWatchingAction, WatchStreamAction,
};
use crate::{
  network::protocol::{ChannelId, UserId, VideoCodecId},
  services::video::VideoBroadcastConfig,
  session::{
    ConnectedServerInfo, LobbyScreenShare, LobbyState, LobbyUser, ServerSession,
    chat_commands::{ChatCommandExpectedType, ChatCommandInvocation, ChatCommandParseError, ChatCommandSource},
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
  chat_command_empty: String,
  chat_command_unterminated_quote: String,
  chat_command_usage: String,
  chat_command_invalid_type: String,
  chat_command_invalid_type_number: String,
  chat_command_invalid_type_text: String,
  chat_command_not_implemented: String,
  chat_command_unknown: String,
  chat_command_required: String,
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
      chat_command_empty: ctx.t("lobby.text_channel.commands.error.empty").to_string(),
      chat_command_unterminated_quote: ctx
        .t("lobby.text_channel.commands.error.unterminated_quote")
        .to_string(),
      chat_command_usage: ctx.t("lobby.text_channel.commands.error.usage").to_string(),
      chat_command_invalid_type: ctx.t("lobby.text_channel.commands.error.invalid_type").to_string(),
      chat_command_invalid_type_number: ctx
        .t("lobby.text_channel.commands.error.invalid_type_number")
        .to_string(),
      chat_command_invalid_type_text: ctx.t("lobby.text_channel.commands.error.invalid_type_text").to_string(),
      chat_command_not_implemented: ctx.t("lobby.text_channel.commands.error.not_implemented").to_string(),
      chat_command_unknown: ctx.t("lobby.text_channel.commands.error.unknown").to_string(),
      chat_command_required: ctx.t("lobby.debug_channels.error.command_required").to_string(),
      connect_errors: ConnectErrorCopy::from_ctx(ctx),
    }
  }

  fn stream_too_small(&self, width: u16, height: u16) -> String {
    self
      .stream_too_small
      .replace("{{width}}", &width.to_string())
      .replace("{{height}}", &height.to_string())
  }

  fn chat_command_parse_error(&self, error: &ChatCommandParseError) -> String {
    match error {
      ChatCommandParseError::Empty => self.chat_command_empty.clone(),
      ChatCommandParseError::UnterminatedQuotedArgument => self.chat_command_unterminated_quote.clone(),
      ChatCommandParseError::Usage { usage, .. } => self.chat_command_usage.replace("{{usage}}", usage),
      ChatCommandParseError::InvalidType { value, expected, .. } => self
        .chat_command_invalid_type
        .replace("{{value}}", value)
        .replace("{{expected}}", &self.chat_command_expected_type(expected)),
      ChatCommandParseError::Unknown { command } => self.chat_command_unknown.replace("{{command}}", command),
    }
  }

  fn chat_command_expected_type(&self, expected: &ChatCommandExpectedType) -> String {
    match expected {
      ChatCommandExpectedType::Number { min, max } => self
        .chat_command_invalid_type_number
        .replace("{{min}}", min)
        .replace("{{max}}", max),
      ChatCommandExpectedType::Text => self.chat_command_invalid_type_text.clone(),
    }
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

      if input.command_registry.has_commands() {
        match input.command_registry.parse(&text) {
          Ok(Some(invocation)) => {
            if invocation.source == ChatCommandSource::Local {
              if let Err(message) = execute_chat_command(&session, &copy, invocation) {
                return Err(report_debug_command_error(&session, message));
              }
              return Ok(());
            }
          }
          Ok(None) => {}
          Err(ChatCommandParseError::Unknown { .. })
            if input
              .command_registry
              .definitions()
              .iter()
              .any(|definition| definition.source == ChatCommandSource::Server) => {}
          Err(error) => {
            let message = copy.chat_command_parse_error(&error);
            if matches!(error, ChatCommandParseError::Unknown { .. }) {
              return Err(report_debug_command_error(&session, message));
            }
            session.set_lobby_error_notice(message.clone());
            return Err(message);
          }
        }
      }

      let Some(channel_id) = input.channel_id else {
        let message = copy.chat_command_required.clone();
        return Err(report_debug_command_error(&session, message));
      };
      let server = session.server().ok_or(copy.no_connected_server)?;
      server
        .send_chat_text(channel_id, text)
        .await
        .map_err(|error| error.to_string())?;
      Ok(())
    }
  })
}

fn report_debug_command_error(session: &ServerSession, message: String) -> String {
  session.push_debug_chat_message(message.clone());
  message
}

fn execute_chat_command(
  session: &ServerSession,
  copy: &LobbyActionCopy,
  invocation: ChatCommandInvocation,
) -> Result<(), String> {
  match invocation.name.as_ref() {
    "/restart-audio-receiver" => {
      let user_id = command_user_id(&invocation, copy, "/restart-audio-receiver {userId:Number}")?;
      let restarted = session.restart_audio_receiver(user_id);
      if !restarted {
        tracing::warn!(
          target: "audio::decode",
          "[audio:decode] restart audio receiver command found no active receiver state for user {user_id}"
        );
      }
      session.push_debug_chat_message(format!(
        "Restart audio receiver\nuserId: {user_id}\nresult: {}",
        if restarted {
          "OK receiver state restarted"
        } else {
          "WARN no active receiver state found"
        }
      ));
      Ok(())
    }
    "/debug-user" => {
      let user_id = command_user_id(&invocation, copy, "/debug-user {userId:Number}")?;
      session.push_debug_chat_message(debug_user_report(session, user_id));
      Ok(())
    }
    "/debug-voice" => {
      let user_id = command_user_id(&invocation, copy, "/debug-voice {userId:Number}")?;
      session.push_debug_chat_message(debug_voice_report(session, user_id));
      Ok(())
    }
    "/debug-my-voice" => {
      let user_id = local_user_id(session)?;
      session.push_debug_chat_message(debug_voice_report(session, user_id));
      Ok(())
    }
    "/debug-stream" => {
      let user_id = command_user_id(&invocation, copy, "/debug-stream {userId:Number}")?;
      session.push_debug_chat_message(debug_stream_report(session, user_id));
      Ok(())
    }
    "/debug-my-stream" => {
      let user_id = local_user_id(session)?;
      session.push_debug_chat_message(debug_stream_report(session, user_id));
      Ok(())
    }
    "/debug-channel" => {
      session.push_debug_chat_message(debug_channel_report(session));
      Ok(())
    }
    "/debug-audio-receivers" | "/audio-status" => {
      session.push_debug_chat_message(debug_audio_receivers_report(session));
      Ok(())
    }
    "/debug-video-receivers" | "/video-status" => {
      session.push_debug_chat_message(debug_video_receivers_report(session));
      Ok(())
    }
    command => Err(copy.chat_command_not_implemented.replace("{{command}}", command)),
  }
}

fn command_user_id(invocation: &ChatCommandInvocation, copy: &LobbyActionCopy, usage: &str) -> Result<UserId, String> {
  invocation
    .arguments
    .first()
    .and_then(|value| value.as_ref().parse().ok())
    .ok_or_else(|| copy.chat_command_usage.replace("{{usage}}", usage))
}

fn local_user_id(session: &ServerSession) -> Result<UserId, String> {
  session
    .info()
    .map(|info| info.user_id)
    .ok_or_else(|| "No connected local user.".to_owned())
}

fn debug_user_report(session: &ServerSession, user_id: UserId) -> String {
  let lobby = session.lobby();
  let info = session.info();
  let user = find_lobby_user(&lobby, user_id);
  let channel = user_voice_channel(&lobby, user_id);
  let share = screen_share_for_user(&lobby, user_id);
  let video_error = session.video_error(user_id);
  let frame_present = session.video_frame(user_id).is_some();
  let local = info.as_ref().is_some_and(|info| info.user_id == user_id);
  let mut lines = vec![format!("Debug user {user_id}")];

  lines.push(format!(
    "user: {}",
    user
      .map(user_summary)
      .unwrap_or_else(|| "FAIL not found in lobby user cache".to_owned())
  ));
  lines.push(format!("local_user: {}", status_bool(local)));
  lines.push(format!(
    "voice_channel: {}",
    channel.unwrap_or_else(|| "WARN not in a voice channel".to_owned())
  ));
  lines.push(format!(
    "voice_state: {}",
    user.map(voice_state_summary).unwrap_or_else(|| "unknown".to_owned())
  ));
  lines.push(format!(
    "stream: {}",
    share
      .map(screen_share_summary)
      .unwrap_or_else(|| "WARN not advertised".to_owned())
  ));
  lines.push(format!(
    "watching_this_user: {}",
    status_bool(lobby.watching_user_id == Some(user_id))
  ));
  lines.push(format!("video_frame_cached: {}", status_bool(frame_present)));
  lines.push(format!(
    "video_error: {}",
    video_error
      .map(|error| format!("FAIL {} - {}", error.title, error.message))
      .unwrap_or_else(|| "OK none".to_owned())
  ));
  lines.push(format!("voice_volume: {}%", session.user_volume(user_id)));
  lines.push(format!("stream_volume: {}%", session.stream_volume(user_id)));
  lines.push(format!("lobby: {}", lobby_status_summary(&lobby)));

  if local {
    let (engine, captures_voice) = session.voice_engine_status();
    lines.push(format!(
      "local_voice_engine: engine={} captures_voice={}",
      yes_no(engine),
      yes_no(captures_voice)
    ));
    lines.push(format!(
      "local_stream_broadcast: {}",
      status_bool(session.video_broadcast_active())
    ));
  }

  lines.join("\n")
}

fn debug_voice_report(session: &ServerSession, user_id: UserId) -> String {
  let lobby = session.lobby();
  let info = session.info();
  let user = find_lobby_user(&lobby, user_id);
  let local_user_id = info.as_ref().map(|info| info.user_id);
  let local_channel = local_user_id.and_then(|id| user_voice_channel_id(&lobby, id));
  let target_channel = user_voice_channel_id(&lobby, user_id);
  let same_channel = local_channel.is_some() && local_channel == target_channel;
  let local_state = session.local_voice_state().unwrap_or((false, false));
  let (engine, captures_voice) = session.voice_engine_status();
  let (voice_received, voice_queued, last_played_packet_at) = session.voice_audio_debug_counts(user_id);
  let mut lines = vec![format!("Debug voice {user_id}")];

  lines.push(format!(
    "target: {}",
    user
      .map(user_summary)
      .unwrap_or_else(|| "FAIL not found in lobby user cache".to_owned())
  ));
  lines.push(format!("local_channel_id: {}", optional_id(local_channel)));
  lines.push(format!("target_channel_id: {}", optional_id(target_channel)));
  lines.push(format!("same_voice_channel: {}", status_bool(same_channel)));
  lines.push(format!(
    "local_state: muted={} deafened={}",
    yes_no(local_state.0),
    yes_no(local_state.1)
  ));
  lines.push(format!(
    "target_state: {}",
    user.map(voice_state_summary).unwrap_or_else(|| "unknown".to_owned())
  ));
  lines.push(format!(
    "voice_engine: engine={} captures_voice={}",
    yes_no(engine),
    yes_no(captures_voice)
  ));
  lines.push(format!("target_volume: {}%", session.user_volume(user_id)));
  lines.push(format!(
    "target_normalization: {}",
    status_bool(session.user_normalization(user_id))
  ));
  lines.push(format!(
    "target_voice_packets: received={} queued={} last_played_packet={}",
    voice_received,
    voice_queued,
    packet_timestamp_label(last_played_packet_at)
  ));
  lines.push(format!(
    "can_hear_target_in_principle: {}",
    status_bool(engine && same_channel && !local_state.1 && user.is_some_and(|user| !user.muted && !user.deafened))
  ));
  lines.push(format!("lobby: {}", lobby_status_summary(&lobby)));
  lines.join("\n")
}

fn debug_stream_report(session: &ServerSession, user_id: UserId) -> String {
  let lobby = session.lobby();
  let info = session.info();
  let local = info.as_ref().is_some_and(|info| info.user_id == user_id);
  let user = find_lobby_user(&lobby, user_id);
  let share = screen_share_for_user(&lobby, user_id);
  let watching = lobby.watching_user_id == Some(user_id);
  let frame_present = session.video_frame(user_id).is_some();
  let video_error = session.video_error(user_id);
  let (stream_audio_received, stream_audio_queued, last_stream_audio_at) = session.stream_audio_debug_counts(user_id);
  let mut lines = vec![format!("Debug stream {user_id}")];

  lines.push(format!(
    "target: {}",
    user
      .map(user_summary)
      .unwrap_or_else(|| "WARN not found in lobby user cache".to_owned())
  ));
  lines.push(format!(
    "advertised_stream: {}",
    share
      .map(screen_share_summary)
      .unwrap_or_else(|| "FAIL no ScreenShareStarted state in lobby".to_owned())
  ));
  lines.push(format!("watching_this_user: {}", status_bool(watching)));
  lines.push(format!("video_frame_cached: {}", status_bool(frame_present)));
  lines.push(format!(
    "video_error: {}",
    video_error
      .map(|error| format!("FAIL {} - {}", error.title, error.message))
      .unwrap_or_else(|| "OK none".to_owned())
  ));
  lines.push(format!("stream_audio_volume: {}%", session.stream_volume(user_id)));
  lines.push(format!(
    "stream_audio_packets: received={} queued={} last_played_packet={}",
    stream_audio_received,
    stream_audio_queued,
    packet_timestamp_label(last_stream_audio_at)
  ));
  lines.push(format!(
    "voice_channel: {}",
    user_voice_channel(&lobby, user_id).unwrap_or_else(|| "WARN none".to_owned())
  ));
  if local {
    lines.push(format!(
      "local_broadcast_active: {}",
      status_bool(session.video_broadcast_active())
    ));
  }
  lines.push(format!("lobby: {}", lobby_status_summary(&lobby)));
  lines.join("\n")
}

fn debug_channel_report(session: &ServerSession) -> String {
  let lobby = session.lobby();
  let current_voice_users = lobby
    .selected_channel_id
    .and_then(|id| lobby.users_by_channel.get(&id))
    .map(|users| {
      users
        .iter()
        .map(|user| format!("{}:{}", user.user_id, user.username))
        .collect::<Vec<_>>()
        .join(", ")
    })
    .filter(|users| !users.is_empty())
    .unwrap_or_else(|| "none".to_owned());

  [
    "Debug channel".to_owned(),
    format!("selected_voice_channel_id: {}", optional_id(lobby.selected_channel_id)),
    format!(
      "stream_browser_channel_id: {}",
      optional_id(lobby.stream_browser_channel_id)
    ),
    format!(
      "selected_text_channel_id: {}",
      optional_id(lobby.selected_text_channel_id)
    ),
    format!("debug_chat_selected: {}", yes_no(lobby.debug_chat_selected)),
    format!("watching_user_id: {}", optional_id(lobby.watching_user_id)),
    format!("voice_channels: {}", lobby.channels.len()),
    format!("text_channels: {}", lobby.text_channels.len()),
    format!("users_total: {}", lobby.users.len()),
    format!(
      "users_in_voice_channels: {}",
      lobby.users_by_channel.values().map(Vec::len).sum::<usize>()
    ),
    format!("screen_shares: {}", lobby.screen_shares.len()),
    format!("current_voice_users: {current_voice_users}"),
    format!("lobby: {}", lobby_status_summary(&lobby)),
  ]
  .join("\n")
}

fn debug_audio_receivers_report(session: &ServerSession) -> String {
  let lobby = session.lobby();
  let (engine, captures_voice) = session.voice_engine_status();
  let mut lines = vec![
    "Debug audio receivers".to_owned(),
    format!(
      "voice_engine: engine={} captures_voice={}",
      yes_no(engine),
      yes_no(captures_voice)
    ),
    format!("local_voice_state: {:?}", session.local_voice_state()),
  ];

  let mut users = lobby.users_by_channel.values().flatten().collect::<Vec<_>>();
  users.sort_by_key(|user| user.user_id);
  users.dedup_by_key(|user| user.user_id);

  if users.is_empty() {
    lines.push("receivers: WARN no users in voice channels".to_owned());
  } else {
    for user in users {
      let (received, queued, last_played_packet_at) = session.voice_audio_debug_counts(user.user_id);
      lines.push(format!(
        "user {} {}: muted={} deafened={} speaking={} volume={}% voice_packets_received={} queued={} last_played_packet={}",
        user.user_id,
        user.username,
        yes_no(user.muted),
        yes_no(user.deafened),
        yes_no(user.speaking),
        session.user_volume(user.user_id),
        received,
        queued,
        packet_timestamp_label(last_played_packet_at)
      ));
    }
  }

  lines.join("\n")
}

fn debug_video_receivers_report(session: &ServerSession) -> String {
  let lobby = session.lobby();
  let receiver = session.video_receiver_debug_snapshot();
  let mut lines = vec![
    "Debug video receivers".to_owned(),
    format!("watching_user_id: {}", optional_id(lobby.watching_user_id)),
    format!(
      "local_broadcast_active: {}",
      status_bool(session.video_broadcast_active())
    ),
    format!(
      "receiver: watched={} queue_limit={} last_batch_queued={} last_batch_dropped={}",
      optional_id(receiver.watched_user_id),
      receiver.queue_limit,
      receiver.last_batch_queued,
      receiver.last_batch_dropped
    ),
    format!(
      "receiver_dropped_senders: {}",
      debug_user_count_pairs(&receiver.last_dropped_senders)
    ),
    format!(
      "receiver_awaiting_keyframes: {}",
      debug_user_ids(&receiver.awaiting_keyframes)
    ),
    format!(
      "receiver_awaiting_decoded_output: {}",
      debug_user_ids(&receiver.awaiting_decoded_output)
    ),
    format!(
      "receiver_expected_frames: {}",
      debug_user_u32_pairs(&receiver.expected_frame_numbers)
    ),
    format!(
      "receiver_received_frames: {}",
      debug_user_count_pairs(&receiver.received_counts)
    ),
    format!(
      "receiver_decoded_frames: {}",
      debug_user_count_pairs(&receiver.decoded_counts)
    ),
    format!(
      "receiver_keyframe_request_age_ms: {}",
      debug_user_u128_pairs(&receiver.keyframe_request_ages_ms)
    ),
    format!(
      "receiver_view_refresh_age_ms: {}",
      debug_user_u128_pairs(&receiver.keyframe_request_ages_ms)
    ),
  ];

  if lobby.screen_shares.is_empty() {
    lines.push("streams: WARN no advertised screen shares".to_owned());
  } else {
    for share in &lobby.screen_shares {
      let frame = session.video_frame(share.sharer_user_id).is_some();
      let error = session.video_error(share.sharer_user_id);
      lines.push(format!(
        "user {}: {} watching={} frame_cached={} error={}",
        share.sharer_user_id,
        screen_share_summary(share),
        yes_no(lobby.watching_user_id == Some(share.sharer_user_id)),
        yes_no(frame),
        error
          .map(|error| format!("FAIL {}", error.title))
          .unwrap_or_else(|| "OK none".to_owned())
      ));
    }
  }

  lines.join("\n")
}

fn debug_user_ids(ids: &[UserId]) -> String {
  if ids.is_empty() {
    return "none".to_owned();
  }
  ids
    .iter()
    .map(|user_id| user_id.to_string())
    .collect::<Vec<_>>()
    .join(", ")
}

fn debug_user_count_pairs(pairs: &[(UserId, u64)]) -> String {
  if pairs.is_empty() {
    return "none".to_owned();
  }
  pairs
    .iter()
    .map(|(user_id, value)| format!("{user_id}={value}"))
    .collect::<Vec<_>>()
    .join(", ")
}

fn debug_user_u32_pairs(pairs: &[(UserId, u32)]) -> String {
  if pairs.is_empty() {
    return "none".to_owned();
  }
  pairs
    .iter()
    .map(|(user_id, value)| format!("{user_id}={value}"))
    .collect::<Vec<_>>()
    .join(", ")
}

fn debug_user_u128_pairs(pairs: &[(UserId, u128)]) -> String {
  if pairs.is_empty() {
    return "none".to_owned();
  }
  pairs
    .iter()
    .map(|(user_id, value)| format!("{user_id}={value}"))
    .collect::<Vec<_>>()
    .join(", ")
}

fn find_lobby_user(lobby: &LobbyState, user_id: UserId) -> Option<&LobbyUser> {
  lobby
    .users
    .iter()
    .chain(lobby.users_by_channel.values().flatten())
    .find(|user| user.user_id == user_id)
}

fn user_voice_channel_id(lobby: &LobbyState, user_id: UserId) -> Option<ChannelId> {
  lobby
    .users_by_channel
    .iter()
    .find_map(|(channel_id, users)| users.iter().any(|user| user.user_id == user_id).then_some(*channel_id))
}

fn user_voice_channel(lobby: &LobbyState, user_id: UserId) -> Option<String> {
  let channel_id = user_voice_channel_id(lobby, user_id)?;
  let name = lobby
    .channels
    .iter()
    .find(|channel| channel.id == channel_id)
    .map(|channel| channel.name.as_str())
    .unwrap_or("unknown");
  Some(format!("{name} ({channel_id})"))
}

fn screen_share_for_user(lobby: &LobbyState, user_id: UserId) -> Option<&LobbyScreenShare> {
  lobby.screen_shares.iter().find(|share| share.sharer_user_id == user_id)
}

fn user_summary(user: &LobbyUser) -> String {
  format!("OK {} role={:?}", user.username, user.role)
}

fn voice_state_summary(user: &LobbyUser) -> String {
  format!(
    "muted={} deafened={} speaking={}",
    yes_no(user.muted),
    yes_no(user.deafened),
    yes_no(user.speaking)
  )
}

fn screen_share_summary(share: &LobbyScreenShare) -> String {
  format!(
    "OK codec={} size={}x{} supported={}",
    video_codec_label(share.metadata.codec),
    share.metadata.width,
    share.metadata.height,
    yes_no(share.metadata.codec.is_supported_stream_codec())
  )
}

fn lobby_status_summary(lobby: &LobbyState) -> String {
  format!(
    "disconnected={} receiver_running={} keepalive_ok={} ping_ms={} warning={} last_error={}",
    yes_no(lobby.disconnected),
    yes_no(lobby.receiver_running),
    yes_no(lobby.keepalive_ok),
    lobby
      .ping_ms
      .map(|ping| ping.to_string())
      .unwrap_or_else(|| "none".to_owned()),
    lobby
      .connection_warning
      .as_ref()
      .map(|warning| format!("{:?}", warning.kind))
      .unwrap_or_else(|| "none".to_owned()),
    lobby.last_error.as_deref().unwrap_or("none")
  )
}

fn video_codec_label(codec: VideoCodecId) -> &'static str {
  match codec {
    VideoCodecId::Unknown => "unknown",
    VideoCodecId::Av1 => "AV1",
    VideoCodecId::H265 => "H.265",
    VideoCodecId::H264 => "H.264",
  }
}

fn optional_id(value: Option<u32>) -> String {
  value
    .map(|value| value.to_string())
    .unwrap_or_else(|| "none".to_owned())
}

fn yes_no(value: bool) -> &'static str {
  if value { "yes" } else { "no" }
}

fn packet_timestamp_label(timestamp: Option<SystemTime>) -> String {
  let Some(timestamp) = timestamp else {
    return "none".to_owned();
  };

  let unix_ms = timestamp
    .duration_since(UNIX_EPOCH)
    .map(|duration| duration.as_millis())
    .unwrap_or(0);
  let age_ms = SystemTime::now()
    .duration_since(timestamp)
    .map(|duration| duration.as_millis())
    .unwrap_or(0);
  format!("unix_ms={unix_ms} age_ms={age_ms}")
}

fn status_bool(value: bool) -> &'static str {
  if value { "OK yes" } else { "WARN no" }
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
    Ok(()) => tracing::info!(target: "voice", "[voice] local voice engine restarted after reconnect rejoin"),
    Err(error) => tracing::warn!(target: "voice", "[voice] local voice engine failed after reconnect rejoin: {error}"),
  }
}
