use lurq::{
  components::{Column, Row},
  core::Signal,
  layout::{Alignment, layout_kind::Justify, text_style::FontWeight},
  node::{BackgroundColor, CursorIcon, Style, color::Color, dimension::Dimension},
};

use super::{LobbyCommand, LobbyCommandAction, ui::mono_label};
use crate::{
  screens::shared::{self, ROUTE_CHOOSE_SERVER, styled_text},
  theme,
};

fn control_button(label: &str, icon: &str, active: bool, danger: bool) -> Row {
  let palette = theme::palette();
  let background = if danger {
    palette.danger_muted
  } else if active {
    palette.success_muted
  } else {
    palette.surface_raised
  };
  let border = if danger {
    palette.danger
  } else if active {
    palette.border_strong
  } else {
    palette.border
  };
  let color = if danger {
    palette.danger
  } else if active {
    palette.success
  } else {
    palette.text_secondary
  };

  Row::new()
    .height(30.0)
    .flex(1.0)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .spacing(6.0)
    .rounded(4.0)
    .background(background)
    .border_inside(1.0, border)
    .cursor(CursorIcon::Pointer)
    .hovered_style(Style::new().background(if danger {
      BackgroundColor::Palette(theme::PaletteColor::DangerMuted)
    } else {
      BackgroundColor::Palette(theme::PaletteColor::SurfaceInput)
    }))
    .child(shared::icon(icon, 13.0, color))
    .child(styled_text(label, "Inter", 10.0, FontWeight::Bold, color, 1.2))
}

pub(super) fn local_dock(
  user_label: &str,
  muted: Signal<bool>,
  deafened: Signal<bool>,
  sharing: Signal<bool>,
  command_action: LobbyCommandAction,
  navigator: Option<lurq::router::Navigator>,
) -> Column {
  let muted_now = muted.get();
  let deafened_now = deafened.get();
  let sharing_now = sharing.get();

  Column::new()
    .width(Dimension::Pct(100.0))
    .spacing(9.0)
    .padding(10.0)
    .rounded(6.0)
    .background(Color::from_hex("#111316"))
    .border_inside(1.0, theme::PaletteColor::Border)
    .child(
      Row::new()
        .width(Dimension::Pct(100.0))
        .align_items(Alignment::Center)
        .spacing(8.0)
        .child(shared::dot(theme::palette().success))
        .child(
          Column::new()
            .spacing(1.0)
            .flex(1.0)
            .child(styled_text(
              user_label,
              "Inter",
              12.0,
              FontWeight::Bold,
              theme::palette().text_primary,
              1.2,
            ))
            .child(mono_label("Local voice", 9.0, theme::palette().text_muted)),
        ),
    )
    .child(
      Row::new()
        .width(Dimension::Pct(100.0))
        .spacing(6.0)
        .child(
          control_button(
            if muted_now { "Muted" } else { "Mic" },
            if muted_now { "mic-off" } else { "mic" },
            muted_now,
            false,
          )
          .on_click({
            let muted = muted.clone();
            let deafened = deafened.clone();
            let command_action = command_action.clone();
            move |_| {
              let next_muted = !muted.get_untracked();
              let deafened = deafened.get_untracked();
              muted.set(next_muted);
              command_action.run(LobbyCommand::VoiceState {
                muted: next_muted,
                deafened,
              });
            }
          }),
        )
        .child(control_button("Deaf", "headphones", deafened_now, false).on_click({
          let muted = muted.clone();
          let deafened = deafened.clone();
          let command_action = command_action.clone();
          move |_| {
            let muted = muted.get_untracked();
            let next_deafened = !deafened.get_untracked();
            deafened.set(next_deafened);
            command_action.run(LobbyCommand::VoiceState {
              muted,
              deafened: next_deafened,
            });
          }
        })),
    )
    .child(
      Row::new()
        .width(Dimension::Pct(100.0))
        .spacing(6.0)
        .child(control_button("Share", "monitor", sharing_now, false).on_click({
          let sharing = sharing.clone();
          let command_action = command_action.clone();
          move |_| {
            let next_sharing = !sharing.get_untracked();
            sharing.set(next_sharing);
            command_action.run(if next_sharing {
              LobbyCommand::StartScreenShare
            } else {
              LobbyCommand::StopScreenShare
            });
          }
        }))
        .child(control_button("Leave", "chevron-right", false, true).on_click({
          let command_action = command_action.clone();
          move |_| {
            command_action.run(LobbyCommand::LeaveChannel);
            if let Some(navigator) = &navigator {
              navigator.replace(ROUTE_CHOOSE_SERVER);
            }
          }
        })),
    )
}
