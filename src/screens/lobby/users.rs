use lurq::{
  components::{Rect, Row},
  layout::{Alignment, text_style::FontWeight},
  node::{BackgroundColor, color::Color, dimension::Dimension, padding::Padding},
};

use super::ui::mono_label;
use crate::{
  network::protocol::Role,
  screens::shared::{self, styled_text},
  session::LobbyUser,
  theme,
};

fn user_color(user_id: u32) -> Color {
  const COLORS: [Color; 5] = [
    Color::new(110, 168, 216, 255),
    Color::new(214, 178, 94, 255),
    Color::new(105, 167, 255, 255),
    Color::new(240, 93, 94, 255),
    Color::new(183, 178, 170, 255),
  ];
  COLORS[(user_id as usize) % COLORS.len()]
}

fn role_label(role: Role) -> Option<&'static str> {
  match role {
    Role::Owner => Some("Owner"),
    Role::Admin => Some("Admin"),
    Role::Moderator => Some("Moderator"),
    Role::User => None,
  }
}

pub(super) fn user_row(user: &LobbyUser, own_user_id: Option<u32>) -> Row {
  let palette = theme::palette();
  let active = own_user_id == Some(user.user_id);
  let text = if active {
    palette.text_primary
  } else {
    palette.text_secondary
  };
  let state_icon = if user.deafened {
    "headphones"
  } else if user.muted {
    "mic-off"
  } else {
    "mic"
  };
  let state_color = if user.muted || user.deafened {
    palette.danger
  } else {
    palette.success
  };

  let mut row = Row::new()
    .width(Dimension::Pct(100.0))
    .height(24.0)
    .align_items(Alignment::Center)
    .spacing(7.0)
    .padding_custom(Padding::new().left(28.0).right(10.0))
    .rounded(4.0)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfacePanel))
    .child(Rect::new(6.0, 6.0).rounded(3.0).background(user_color(user.user_id)))
    .child(styled_text(
      &user.username,
      "Inter",
      11.0,
      FontWeight::Medium,
      text,
      1.2,
    ))
    .child(Row::new().height(1.0).flex(1.0));

  if let Some(role) = role_label(user.role) {
    row = row.child(mono_label(role, 9.0, palette.text_muted));
  }

  row.child(shared::icon(state_icon, 12.0, state_color))
}
