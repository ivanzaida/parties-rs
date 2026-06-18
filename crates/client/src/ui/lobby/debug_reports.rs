use std::time::{SystemTime, UNIX_EPOCH};

use crate::{
  network::protocol::{ChannelId, UserId, VideoCodecId},
  session::{LobbyScreenShare, LobbyState, LobbyUser, ServerSession},
};

pub(super) fn debug_user_report(session: &ServerSession, user_id: UserId) -> String {
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

pub(super) fn debug_voice_report(session: &ServerSession, user_id: UserId) -> String {
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

pub(super) fn debug_stream_report(session: &ServerSession, user_id: UserId) -> String {
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

pub(super) fn debug_channel_report(session: &ServerSession) -> String {
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

pub(super) fn debug_audio_receivers_report(session: &ServerSession) -> String {
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

pub(super) fn debug_video_receivers_report(session: &ServerSession) -> String {
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
