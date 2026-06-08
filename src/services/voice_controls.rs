use crate::session::ServerSession;

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub enum VoiceControlAction {
  ToggleMute,
  ToggleDeafen,
  LeaveChannel,
}

pub async fn apply_voice_control(session: ServerSession, action: VoiceControlAction) -> Result<(), String> {
  let server = session.server().ok_or_else(|| "No connected server.".to_owned())?;
  let (mut muted, mut deafened) = session.local_voice_state().unwrap_or((false, false));

  match action {
    VoiceControlAction::LeaveChannel => {
      server.leave_channel().await.map_err(|error| error.to_string())?;
      session.leave_channel_locally();
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

  server
    .update_voice_state(muted, deafened)
    .await
    .map_err(|error| error.to_string())?;
  session.set_local_voice_state(muted, deafened);
  Ok(())
}
