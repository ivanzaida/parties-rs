mod dock;
mod navigation;
mod rail;
mod stage;
mod ui;
mod users;

use lurq::{
  app::{
    component::Component,
    ctx::{Ctx, FutureAction},
  },
  components::Row,
  core::Signal,
  node::{BackgroundColor, Element, dimension::Dimension},
};

use self::{navigation::navigation, rail::server_rail, stage::protocol_stage};
use crate::{
  network::protocol::{ChannelId, UserId, VideoCodecId},
  session::ServerSession,
  theme,
};

#[derive(Clone)]
pub(super) enum LobbyCommand {
  JoinChannel(ChannelId),
  VoiceState { muted: bool, deafened: bool },
  StartScreenShare,
  StopScreenShare,
  ViewScreenShare(UserId),
  UnsubscribeScreenShare,
  LeaveChannel,
}

pub(super) type LobbyCommandAction = FutureAction<LobbyCommand, (), String>;

pub struct Lobby {
  muted: Signal<bool>,
  deafened: Signal<bool>,
  sharing: Signal<bool>,
}

impl Component for Lobby {
  type Props = ();

  fn create(ctx: &mut Ctx) -> Self {
    Self {
      muted: ctx.signal(false),
      deafened: ctx.signal(false),
      sharing: ctx.signal(false),
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let navigator = ctx.navigator();
    let session = ctx.use_context::<ServerSession>().unwrap_or_default();
    let _revision = session.revision().get();
    let info = session.info();
    let receiver_key = info
      .as_ref()
      .map(|info| (info.address.clone(), info.certificate_fingerprint.clone(), info.user_id));
    ctx
      .future(receiver_key, {
        let session = session.clone();
        move |_| {
          let session = session.clone();
          async move {
            session.run_lobby_receiver().await;
            Ok::<(), String>(())
          }
        }
      })
      .state();
    let command_action = ctx.future_action({
      let session = session.clone();
      move |command: LobbyCommand| {
        let session = session.clone();
        async move {
          let Some(server) = session.server() else {
            return Ok(());
          };
          let should_clear_session = matches!(&command, LobbyCommand::LeaveChannel);
          let result = match command {
            LobbyCommand::JoinChannel(channel_id) => server.join_channel(channel_id).await,
            LobbyCommand::VoiceState { muted, deafened } => server.update_voice_state(muted, deafened).await,
            LobbyCommand::StartScreenShare => server.start_screen_share(VideoCodecId::H264, 1280, 720).await,
            LobbyCommand::StopScreenShare => server.stop_screen_share().await,
            LobbyCommand::ViewScreenShare(user_id) => server.view_screen_share(user_id).await,
            LobbyCommand::UnsubscribeScreenShare => server.unsubscribe_screen_share().await,
            LobbyCommand::LeaveChannel => server.leave_channel().await,
          };
          if let Err(error) = result {
            let message = error.to_string();
            session.mark_lobby_error(message.clone());
            return Err(message);
          }
          if should_clear_session {
            session.clear();
          }
          Ok(())
        }
      }
    });
    let lobby = session.lobby();
    let connected = info.is_some();
    let own_user_id = info.as_ref().map(|info| info.user_id);
    let server_name = info
      .as_ref()
      .map(|info| info.server_name.clone())
      .unwrap_or_else(|| ctx.t("lobby.server.unknown"));
    let user_label = info
      .as_ref()
      .map(|info| format!("you #{}", info.user_id))
      .unwrap_or_else(|| ctx.t("lobby.user.disconnected"));

    Row::new()
      .width(Dimension::Pct(100.0))
      .height(Dimension::Pct(100.0))
      .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceBase))
      .clip()
      .child(server_rail())
      .child(navigation(
        &server_name,
        &user_label,
        self.muted.clone(),
        self.deafened.clone(),
        self.sharing.clone(),
        connected,
        own_user_id,
        &lobby,
        session,
        command_action,
        navigator,
      ))
      .child(protocol_stage(&lobby, self.sharing.get()))
  }
}
