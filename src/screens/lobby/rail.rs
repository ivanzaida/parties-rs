use lurq::{
  components::{Column, Row},
  layout::{Alignment, layout_kind::Justify, text_style::FontWeight},
  node::{BackgroundColor, CursorIcon, Style, color::Color, dimension::Dimension},
};

use crate::{
  screens::shared::{self, styled_text},
  theme,
};

const SERVER_RAIL_WIDTH: f32 = 60.0;

fn rail_tile(label: &str, active: bool) -> Row {
  let palette = theme::palette();
  let background = if active {
    palette.success_muted
  } else {
    palette.surface_panel
  };
  let border = if active { palette.border_strong } else { palette.border };
  let color = if active {
    palette.success
  } else {
    palette.text_secondary
  };

  Row::new()
    .width(34.0)
    .height(34.0)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .rounded(7.0)
    .background(background)
    .border_inside(1.0, border)
    .cursor(CursorIcon::Pointer)
    .hovered_style(Style::new().background(theme::PaletteColor::SurfaceRaised))
    .child(styled_text(label, "Inter", 12.0, FontWeight::Bold, color, 1.2))
}

pub(super) fn server_rail() -> Column {
  Column::new()
    .width(SERVER_RAIL_WIDTH)
    .height(Dimension::Pct(100.0))
    .align_items(Alignment::Center)
    .spacing(10.0)
    .padding_vertical(14.0)
    .background(Color::from_hex("#0E1012"))
    .border_inside(1.0, theme::PaletteColor::Border)
    .child(rail_tile("P", true))
    .child(rail_tile("D", false))
    .child(rail_tile("G", false))
    .child(rail_tile("W", false))
    .child(Row::new().width(1.0).height(1.0).flex(1.0))
    .child(
      Row::new()
        .width(32.0)
        .height(32.0)
        .align_items(Alignment::Center)
        .justify(Justify::Center)
        .rounded(7.0)
        .background(BackgroundColor::Palette(theme::PaletteColor::SurfacePanel))
        .border_inside(1.0, theme::PaletteColor::Border)
        .cursor(CursorIcon::Pointer)
        .hovered_style(Style::new().background(theme::PaletteColor::SurfaceRaised))
        .child(shared::icon("plus", 14.0, theme::palette().text_secondary)),
    )
}
