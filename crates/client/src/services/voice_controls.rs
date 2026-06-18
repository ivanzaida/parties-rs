use crate::session::ServerSession;

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub enum VoiceControlAction {
  ToggleMute,
  ToggleDeafen,
  LeaveChannel,
}

pub async fn apply_voice_control(
  session: ServerSession,
  action: VoiceControlAction,
  no_connected_server: String,
) -> Result<(), String> {
  let server = session.server().ok_or_else(|| no_connected_server.clone())?;
  let (mut muted, mut deafened) = session.local_voice_state().unwrap_or((false, false));
  let previous_voice_state = (muted, deafened);

  match action {
    VoiceControlAction::LeaveChannel => {
      session.leave_channel_locally();
      server.leave_channel().await.map_err(|error| error.to_string())?;
      return Ok(());
    }
    VoiceControlAction::ToggleMute => {
      if deafened && muted {
        return Ok(());
      }
      muted = !muted;
    }
    VoiceControlAction::ToggleDeafen => {
      if deafened {
        deafened = false;
        muted = session.take_muted_before_deafen().unwrap_or(muted);
      } else {
        session.remember_muted_before_deafen(muted);
        deafened = true;
        muted = true;
      }
    }
  }

  session.set_local_voice_state(muted, deafened);
  if let Err(error) = server.update_voice_state(muted, deafened).await {
    session.set_local_voice_state(previous_voice_state.0, previous_voice_state.1);
    return Err(error.to_string());
  }
  if !muted && !deafened {
    session.ensure_voice_capture_started(&no_connected_server)?;
  }
  session.play_local_voice_state_change_notification();
  Ok(())
}
