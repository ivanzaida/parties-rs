use lurq::{
  components::{Column, Row, Text},
  core::Signal,
  layout::{Alignment, text_style::FontWeight},
  node::{BackgroundColor, CursorIcon, Style, dimension::Dimension},
};

use super::{LobbyCommand, LobbyCommandAction, dock::local_dock, ui::mono_label, users::user_row};
use crate::{
  screens::shared::{self, styled_text},
  session::{LobbyState, ServerSession},
  theme,
};

const NAV_WIDTH: f32 = 250.0;

fn nav_header(server_name: &str) -> Row {
  Row::new()
    .width(Dimension::Pct(100.0))
    .height(34.0)
    .align_items(Alignment::Center)
    .spacing(8.0)
    .child(
      styled_text(
        server_name,
        "Inter",
        14.0,
        FontWeight::Bold,
        theme::palette().text_primary,
        1.2,
      )
      .flex(1.0),
    )
    .child(shared::icon("settings", 15.0, theme::palette().text_muted))
}

fn voice_summary(
  connected: bool,
  channel_selected: bool,
  channel_key_received: bool,
  keepalive_ok: bool,
  muted: bool,
  deafened: bool,
) -> Column {
  let state = match (connected, muted, deafened) {
    (false, ..) => "No voice session",
    (true, true, true) => "Muted / deafened",
    (true, true, false) => "Muted / undeafened",
    (true, false, true) => "Unmuted / deafened",
    (true, false, false) => "Unmuted / undeafened",
  };
  let description = if connected && channel_selected && channel_key_received {
    format!("Channel key received. Voice state: {state}.")
  } else if connected && channel_selected {
    format!("Waiting for channel key. Voice state: {state}.")
  } else if connected {
    "Select a voice channel to join.".to_owned()
  } else {
    "Connect to a server to join voice.".to_owned()
  };

  Column::new()
    .width(Dimension::Pct(100.0))
    .spacing(8.0)
    .padding(10.0)
    .rounded(6.0)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised))
    .border_inside(1.0, theme::PaletteColor::Border)
    .child(
      Row::new()
        .width(Dimension::Pct(100.0))
        .align_items(Alignment::Center)
        .spacing(8.0)
        .child(shared::dot(if connected {
          theme::palette().success
        } else {
          theme::palette().text_muted
        }))
        .child(styled_text(
          if connected { "Voice connected" } else { "Disconnected" },
          "Inter",
          12.0,
          FontWeight::Bold,
          theme::palette().text_primary,
          1.2,
        ))
        .child(Row::new().height(1.0).flex(1.0))
        .child(mono_label(
          if !connected {
            "--"
          } else if keepalive_ok {
            "ok"
          } else {
            "sync"
          },
          10.0,
          if connected && keepalive_ok {
            theme::palette().success
          } else {
            theme::palette().text_muted
          },
        )),
    )
    .child(styled_text(
      &description,
      "Inter",
      11.0,
      FontWeight::Normal,
      theme::palette().text_secondary,
      1.2,
    ))
}

fn section_label(label: &str) -> Text {
  mono_label(label, 10.0, theme::palette().text_muted)
}

fn nav_item(icon: &str, label: &str, count: &str, active: bool) -> Row {
  let palette = theme::palette();
  let background = if active {
    palette.success_muted
  } else {
    palette.surface_panel
  };
  let border = if active {
    palette.border_strong
  } else {
    palette.surface_panel
  };
  let accent = if active { palette.success } else { palette.text_muted };
  let text = if active {
    palette.text_primary
  } else {
    palette.text_secondary
  };

  Row::new()
    .width(Dimension::Pct(100.0))
    .height(32.0)
    .align_items(Alignment::Center)
    .spacing(8.0)
    .padding_horizontal(10.0)
    .rounded(5.0)
    .background(background)
    .border_inside(1.0, border)
    .cursor(CursorIcon::Pointer)
    .hovered_style(Style::new().background(theme::PaletteColor::SurfaceRaised))
    .child(shared::icon(
      if active { "chevron-down" } else { "chevron-right" },
      12.0,
      palette.text_muted,
    ))
    .child(shared::icon(icon, 14.0, accent))
    .child(styled_text(label, "Inter", 12.0, FontWeight::Bold, text, 1.2).flex(1.0))
    .child(mono_label(count, 10.0, accent))
}

fn stream_item(icon: &str, label: &str, state: &str, active: bool) -> Row {
  let palette = theme::palette();
  Row::new()
    .width(Dimension::Pct(100.0))
    .height(32.0)
    .align_items(Alignment::Center)
    .spacing(8.0)
    .padding_horizontal(10.0)
    .rounded(5.0)
    .background(if active {
      palette.success_muted
    } else {
      palette.surface_panel
    })
    .border_inside(
      1.0,
      if active {
        palette.border_strong
      } else {
        palette.surface_panel
      },
    )
    .cursor(CursorIcon::Pointer)
    .hovered_style(Style::new().background(theme::PaletteColor::SurfaceRaised))
    .child(shared::icon(icon, 14.0, theme::palette().text_muted))
    .child(styled_text(
      label,
      "Inter",
      12.0,
      FontWeight::Medium,
      theme::palette().text_secondary,
      1.2,
    ))
    .child(Row::new().height(1.0).flex(1.0))
    .child(mono_label(state, 10.0, theme::palette().text_muted))
}

pub(super) fn navigation(
  server_name: &str,
  user_label: &str,
  muted: Signal<bool>,
  deafened: Signal<bool>,
  sharing: Signal<bool>,
  connected: bool,
  own_user_id: Option<u32>,
  lobby: &LobbyState,
  session: ServerSession,
  command_action: LobbyCommandAction,
  navigator: Option<lurq::router::Navigator>,
) -> Column {
  let selected_channel_id = lobby.selected_channel_id;
  let selected_channel = lobby
    .channels
    .iter()
    .find(|channel| Some(channel.id) == selected_channel_id);
  let mut nav = Column::new()
    .width(NAV_WIDTH)
    .height(Dimension::Pct(100.0))
    .spacing(12.0)
    .padding_vertical(14.0)
    .padding_horizontal(12.0)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfacePanel))
    .border_inside(1.0, theme::PaletteColor::Border)
    .child(nav_header(server_name))
    .child(voice_summary(
      connected,
      selected_channel.is_some(),
      selected_channel.is_some_and(|channel| channel.key_received),
      lobby.keepalive_ok,
      muted.get(),
      deafened.get(),
    ))
    .child(section_label("CHANNELS"));

  if lobby.channels.is_empty() {
    let (label, state) = if let Some(error) = lobby.last_error.as_deref() {
      (error, "error")
    } else if lobby.channel_list_received {
      ("No voice channels", "empty")
    } else if lobby.receiver_running {
      ("Waiting for channel list", "sync")
    } else {
      ("Starting lobby receiver", "init")
    };
    nav = nav.child(stream_item("volume-2", label, state, false));
  } else {
    let mut rendered_selected_users = false;
    for channel in &lobby.channels {
      let active = Some(channel.id) == selected_channel_id;
      let session_for_click = session.clone();
      let command_action = command_action.clone();
      let channel_id = channel.id;
      let count = channel.user_count.to_string();
      nav = nav.child(nav_item("volume-2", &channel.name, &count, active).on_click(move |_| {
        session_for_click.select_channel(channel_id);
        command_action.run(LobbyCommand::JoinChannel(channel_id));
      }));
      if active {
        rendered_selected_users = true;
        if lobby.users.is_empty() {
          nav = nav.child(stream_item("mic", "No users in channel", "--", false));
        } else {
          for user in &lobby.users {
            nav = nav.child(user_row(user, own_user_id));
          }
        }
      }
    }
    if selected_channel_id.is_some() && !rendered_selected_users {
      if lobby.users.is_empty() {
        nav = nav.child(stream_item("mic", "Selected channel pending", "--", false));
      } else {
        for user in &lobby.users {
          nav = nav.child(user_row(user, own_user_id));
        }
      }
    } else if selected_channel_id.is_none() {
      nav = nav.child(stream_item("mic", "Select a voice channel", "--", false));
    }
  }

  if lobby.channels.is_empty() {
    nav = nav.child(stream_item("mic", "Join unavailable", "--", false));
  }

  nav
    .child(section_label("STREAMS"))
    .with_children(if lobby.screen_shares.is_empty() {
      vec![stream_item("monitor", "No live streams", "idle", false)]
    } else {
      lobby
        .screen_shares
        .iter()
        .map(|share| {
          let active = lobby.watching_user_id == Some(share.sharer_user_id);
          let session_for_click = session.clone();
          let command_action = command_action.clone();
          let sharer_user_id = share.sharer_user_id;
          let label = lobby
            .users
            .iter()
            .find(|user| user.user_id == share.sharer_user_id)
            .map(|user| format!("{} screen", user.username))
            .unwrap_or_else(|| format!("user #{} screen", share.sharer_user_id));
          stream_item("monitor", &label, "live", active).on_click(move |_| {
            let next_watching = if active { None } else { Some(sharer_user_id) };
            session_for_click.set_watching_user(next_watching);
            command_action.run(if active {
              LobbyCommand::UnsubscribeScreenShare
            } else {
              LobbyCommand::ViewScreenShare(sharer_user_id)
            });
          })
        })
        .collect::<Vec<_>>()
    })
    .child(Row::new().height(1.0).flex(1.0))
    .child(local_dock(
      user_label,
      muted,
      deafened,
      sharing,
      command_action,
      navigator,
    ))
}
