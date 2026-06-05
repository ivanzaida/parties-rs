use lurq::{
  components::{Column, Row},
  layout::{Alignment, layout_kind::Justify, text_style::FontWeight},
  node::{BackgroundColor, dimension::Dimension},
};

use super::ui::{badge, mono_label};
use crate::{
  screens::shared::{self, styled_text},
  session::{LobbyChannel, LobbyState},
  theme,
};

fn stage_pill(label: &str, active: bool) -> Row {
  badge(
    label,
    if active {
      theme::palette().success
    } else {
      theme::palette().text_secondary
    },
    if active {
      BackgroundColor::Palette(theme::PaletteColor::SuccessMuted)
    } else {
      BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised)
    },
  )
}

fn selected_channel(lobby: &LobbyState) -> Option<&LobbyChannel> {
  lobby
    .selected_channel_id
    .and_then(|id| lobby.channels.iter().find(|channel| channel.id == id))
}

fn empty_channel_status(lobby: &LobbyState) -> &'static str {
  if lobby.last_error.is_some() {
    "server state error"
  } else if lobby.channel_list_received && lobby.channels.is_empty() {
    "no voice channels on this server"
  } else if lobby.channel_list_received {
    "select a voice channel to join"
  } else if lobby.receiver_running {
    "waiting for server channel list"
  } else {
    "starting lobby receiver"
  }
}

fn stage_header(lobby: &LobbyState, channel: Option<&LobbyChannel>) -> Row {
  let fallback_title = lobby
    .selected_channel_id
    .map(|channel_id| format!("Channel #{channel_id}"));
  let title = channel
    .map(|channel| channel.name.as_str().to_owned())
    .or(fallback_title)
    .unwrap_or_else(|| "No channel".to_owned());
  let subtitle = channel
    .map(|channel| format!("{} users / max {}", channel.user_count, channel.max_users))
    .or_else(|| {
      lobby
        .selected_channel_id
        .map(|_| format!("{} users", lobby.users.len()))
    })
    .unwrap_or_else(|| empty_channel_status(lobby).to_owned());
  let encrypted = channel.is_some_and(|channel| channel.key_received);

  Row::new()
    .width(Dimension::Pct(100.0))
    .height(48.0)
    .align_items(Alignment::Center)
    .spacing(12.0)
    .child(
      Column::new()
        .spacing(2.0)
        .flex(1.0)
        .child(styled_text(
          &title,
          "Inter",
          22.0,
          FontWeight::Bold,
          theme::palette().text_primary,
          1.2,
        ))
        .child(styled_text(
          &subtitle,
          "Inter",
          12.0,
          FontWeight::Medium,
          theme::palette().text_muted,
          1.2,
        )),
    )
    .child(stage_pill("VOICE", false))
    .child(stage_pill(if encrypted { "ENCRYPTED" } else { "NO KEY" }, encrypted))
}

fn compact_summary(lobby: &LobbyState, channel: Option<&LobbyChannel>, stream_count: usize) -> Row {
  let summary = channel
    .map(|channel| {
      format!(
        "voice channel / {} users / {} / {} stream{}",
        channel.user_count,
        if channel.key_received {
          "key received"
        } else {
          "waiting for key"
        },
        stream_count,
        if stream_count == 1 { "" } else { "s" }
      )
    })
    .or_else(|| {
      lobby.selected_channel_id.map(|_| {
        format!(
          "voice channel / {} users / metadata pending / no stream selected",
          lobby.users.len()
        )
      })
    })
    .unwrap_or_else(|| format!("{} / no stream selected", empty_channel_status(lobby)));

  Row::new()
    .width(Dimension::Pct(100.0))
    .height(34.0)
    .align_items(Alignment::Center)
    .padding_horizontal(10.0)
    .rounded(5.0)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceBase))
    .border_inside(1.0, theme::PaletteColor::Border)
    .child(mono_label(&summary, 10.0, theme::palette().text_muted))
}

fn voice_canvas(lobby: &LobbyState, channel: Option<&LobbyChannel>, sharing: bool, stream_count: usize) -> Column {
  let title = if sharing {
    "Screen share ready".to_owned()
  } else {
    channel.map(|channel| channel.name.clone()).unwrap_or_else(|| {
      if let Some(channel_id) = lobby.selected_channel_id {
        format!("Channel #{channel_id}")
      } else if lobby.channel_list_received && !lobby.channels.is_empty() {
        "Select a channel".to_owned()
      } else {
        "Waiting for server".to_owned()
      }
    })
  };
  let state = if sharing {
    "sharing controls armed".to_owned()
  } else if stream_count > 0 {
    format!(
      "{stream_count} stream{} available",
      if stream_count == 1 { "" } else { "s" }
    )
  } else if channel.is_some() {
    "in voice / not watching".to_owned()
  } else if lobby.selected_channel_id.is_some() {
    "in voice / metadata pending".to_owned()
  } else if lobby.channel_list_received && !lobby.channels.is_empty() {
    "not joined".to_owned()
  } else {
    "channel list pending".to_owned()
  };

  Column::new()
    .width(Dimension::Pct(100.0))
    .height(Dimension::Pct(100.0))
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .spacing(10.0)
    .rounded(8.0)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceBase))
    .border_inside(1.0, theme::PaletteColor::Border)
    .child(
      Row::new()
        .width(48.0)
        .height(48.0)
        .align_items(Alignment::Center)
        .justify(Justify::Center)
        .rounded(8.0)
        .background(BackgroundColor::Palette(theme::PaletteColor::SurfacePanel))
        .border_inside(1.0, theme::PaletteColor::Border)
        .child(shared::icon(
          if sharing || stream_count > 0 {
            "monitor"
          } else {
            "list-tree"
          },
          18.0,
          theme::palette().success,
        )),
    )
    .child(styled_text(
      &title,
      "Inter",
      18.0,
      FontWeight::Bold,
      theme::palette().text_primary,
      1.2,
    ))
    .child(mono_label(&state, 11.0, theme::palette().text_muted))
}

pub(super) fn protocol_stage(lobby: &LobbyState, sharing: bool) -> Column {
  let channel = selected_channel(lobby);

  Column::new()
    .flex(1.0)
    .height(Dimension::Pct(100.0))
    .spacing(14.0)
    .padding(18.0)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceBase))
    .child(stage_header(lobby, channel))
    .child(
      Column::new()
        .width(Dimension::Pct(100.0))
        .flex(1.0)
        .spacing(10.0)
        .padding(14.0)
        .rounded(8.0)
        .background(BackgroundColor::Palette(theme::PaletteColor::SurfacePanel))
        .border_inside(1.0, theme::PaletteColor::Border)
        .child(
          Row::new()
            .width(Dimension::Pct(100.0))
            .align_items(Alignment::Center)
            .spacing(10.0)
            .child(shared::icon("list-tree", 16.0, theme::palette().success))
            .child(styled_text(
              channel.map(|channel| channel.name.as_str()).unwrap_or("No channel"),
              "Inter",
              15.0,
              FontWeight::Bold,
              theme::palette().text_primary,
              1.2,
            ))
            .child(Row::new().height(1.0).flex(1.0))
            .child(mono_label("voice", 11.0, theme::palette().text_muted)),
        )
        .child(compact_summary(lobby, channel, lobby.screen_shares.len()))
        .child(voice_canvas(lobby, channel, sharing, lobby.screen_shares.len())),
    )
}
