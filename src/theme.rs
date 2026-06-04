use std::sync::Arc;

use lurq::{
  app::theme::{PaletteId, RadiusId, Theme, ThemeFonts, ThemePalette, ThemeRadii, ThemeTypography, TypographyId},
  layout::text_style::{FontWeight, TextStyle},
  node::color::Color,
};

pub const ACCENT: PaletteId = PaletteId::new(0);
pub const ACCENT_HOVER: PaletteId = PaletteId::new(1);
pub const ACCENT_MUTED: PaletteId = PaletteId::new(2);
pub const BG_PRIMARY: PaletteId = PaletteId::new(3);
pub const BG_SECONDARY: PaletteId = PaletteId::new(4);
pub const BG_TERTIARY: PaletteId = PaletteId::new(5);
pub const BG_ELEVATED: PaletteId = PaletteId::new(6);
pub const BG_INPUT: PaletteId = PaletteId::new(7);
pub const BORDER: PaletteId = PaletteId::new(8);
pub const BORDER_LIGHT: PaletteId = PaletteId::new(9);
pub const TEXT_PRIMARY: PaletteId = PaletteId::new(10);
pub const TEXT_SECONDARY: PaletteId = PaletteId::new(11);
pub const TEXT_MUTED: PaletteId = PaletteId::new(12);
pub const TEXT_INVERSE: PaletteId = PaletteId::new(13);
pub const GREEN: PaletteId = PaletteId::new(14);
pub const GREEN_MUTED: PaletteId = PaletteId::new(15);
pub const RED: PaletteId = PaletteId::new(16);
pub const RED_MUTED: PaletteId = PaletteId::new(17);
pub const ORANGE: PaletteId = PaletteId::new(18);
pub const BLUE: PaletteId = PaletteId::new(19);

pub const ACCENT_COLOR: Color = Color::new(66, 210, 139, 255);
pub const ACCENT_HOVER_COLOR: Color = Color::new(87, 224, 156, 255);
pub const ACCENT_MUTED_COLOR: Color = Color::new(16, 37, 26, 255);
pub const BG_ELEVATED_COLOR: Color = Color::new(27, 30, 35, 255);
pub const BG_INPUT_COLOR: Color = Color::new(23, 26, 30, 255);
pub const BORDER_COLOR: Color = Color::new(48, 52, 58, 255);
pub const BORDER_LIGHT_COLOR: Color = Color::new(62, 74, 64, 255);
pub const GREEN_MUTED_COLOR: Color = Color::new(16, 37, 26, 255);
pub const TEXT_PRIMARY_COLOR: Color = Color::new(244, 244, 242, 255);
pub const TEXT_SECONDARY_COLOR: Color = Color::new(183, 178, 170, 255);
pub const TEXT_MUTED_COLOR: Color = Color::new(125, 118, 108, 255);
pub const TEXT_INVERSE_COLOR: Color = Color::new(11, 12, 14, 255);

pub const RADIUS_S: RadiusId = RadiusId::new(0);
pub const RADIUS_M: RadiusId = RadiusId::new(1);
pub const RADIUS_L: RadiusId = RadiusId::new(2);

pub const TYP_HEADING: TypographyId = TypographyId::new(0);
pub const TYP_BODY: TypographyId = TypographyId::new(1);
pub const TYP_CAPTION: TypographyId = TypographyId::new(2);
pub const TYP_LABEL: TypographyId = TypographyId::new(3);
pub const TYP_MONO: TypographyId = TypographyId::new(4);
pub const TYP_TITLE: TypographyId = TypographyId::new(5);
pub const TYP_DESC: TypographyId = TypographyId::new(6);
pub const TYP_BUTTON: TypographyId = TypographyId::new(7);
pub const TYP_SECTION: TypographyId = TypographyId::new(8);
pub const TYP_FIELD_LABEL: TypographyId = TypographyId::new(9);
pub const TYP_LINK: TypographyId = TypographyId::new(10);

pub fn setup(theme: &Theme) {
  theme.set_palette(ThemePalette::from_colors([
    (ACCENT, ACCENT_COLOR),
    (ACCENT_HOVER, ACCENT_HOVER_COLOR),
    (ACCENT_MUTED, ACCENT_MUTED_COLOR),
    (BG_PRIMARY, Color::from_hex("#0B0C0E")),
    (BG_SECONDARY, Color::from_hex("#111316")),
    (BG_TERTIARY, Color::from_hex("#15171A")),
    (BG_ELEVATED, BG_ELEVATED_COLOR),
    (BG_INPUT, BG_INPUT_COLOR),
    (BORDER, BORDER_COLOR),
    (BORDER_LIGHT, BORDER_LIGHT_COLOR),
    (TEXT_PRIMARY, TEXT_PRIMARY_COLOR),
    (TEXT_SECONDARY, TEXT_SECONDARY_COLOR),
    (TEXT_MUTED, TEXT_MUTED_COLOR),
    (TEXT_INVERSE, TEXT_INVERSE_COLOR),
    (GREEN, ACCENT_COLOR),
    (GREEN_MUTED, GREEN_MUTED_COLOR),
    (RED, Color::from_hex("#F05D5E")),
    (RED_MUTED, Color::from_hex("#2A1A1C")),
    (ORANGE, Color::from_hex("#D6B25E")),
    (BLUE, Color::from_hex("#69A7FF")),
  ]));

  theme.set_radii(ThemeRadii::from_values([
    (RADIUS_S, 3.0),
    (RADIUS_M, 5.0),
    (RADIUS_L, 6.0),
  ]));

  let text_primary = Color::from_hex("#F4F4F2");
  let text_secondary = Color::from_hex("#B7B2AA");
  let text_muted = Color::from_hex("#7D766C");

  let inter: Arc<str> = Arc::from("Inter");
  let jetbrains: Arc<str> = Arc::from("JetBrains Mono");

  let heading = TextStyle {
    font_family: inter.clone(),
    font_size: 15.0,
    line_height: 1.2,
    weight: FontWeight::Bold,
    color: text_primary.clone(),
    ..TextStyle::default()
  };
  let body = TextStyle {
    font_family: inter.clone(),
    font_size: 12.0,
    line_height: 1.2,
    weight: FontWeight::Normal,
    color: text_secondary.clone(),
    ..TextStyle::default()
  };
  let mono = TextStyle {
    font_family: jetbrains,
    font_size: 12.0,
    line_height: 1.2,
    weight: FontWeight::Normal,
    color: text_primary,
    ..TextStyle::default()
  };

  theme.set_fonts(ThemeFonts {
    heading: heading.clone(),
    body: body.clone(),
    mono: mono.clone(),
  });

  theme.set_typography(ThemeTypography::from_styles([
    (TYP_HEADING, heading),
    (TYP_BODY, body),
    (
      TYP_CAPTION,
      TextStyle {
        font_family: inter.clone(),
        font_size: 10.0,
        line_height: 1.2,
        weight: FontWeight::Bold,
        color: text_muted,
        ..TextStyle::default()
      },
    ),
    (
      TYP_LABEL,
      TextStyle {
        font_family: inter.clone(),
        font_size: 10.0,
        line_height: 1.2,
        weight: FontWeight::Bold,
        color: text_secondary,
        ..TextStyle::default()
      },
    ),
    (TYP_MONO, mono),
    (
      TYP_TITLE,
      TextStyle {
        font_family: inter.clone(),
        font_size: 24.0,
        line_height: 1.2,
        weight: FontWeight::Bold,
        color: text_primary.clone(),
        ..TextStyle::default()
      },
    ),
    (
      TYP_DESC,
      TextStyle {
        font_family: inter.clone(),
        font_size: 13.0,
        line_height: 1.4,
        weight: FontWeight::Normal,
        color: text_secondary.clone(),
        ..TextStyle::default()
      },
    ),
    (
      TYP_BUTTON,
      TextStyle {
        font_family: inter.clone(),
        font_size: 13.0,
        line_height: 1.2,
        weight: FontWeight::Bold,
        color: text_primary.clone(),
        ..TextStyle::default()
      },
    ),
    (
      TYP_SECTION,
      TextStyle {
        font_family: inter.clone(),
        font_size: 11.0,
        line_height: 1.2,
        weight: FontWeight::Bold,
        color: text_muted.clone(),
        ..TextStyle::default()
      },
    ),
    (
      TYP_FIELD_LABEL,
      TextStyle {
        font_family: inter.clone(),
        font_size: 12.0,
        line_height: 1.2,
        weight: FontWeight::Normal,
        color: text_secondary.clone(),
        ..TextStyle::default()
      },
    ),
    (
      TYP_LINK,
      TextStyle {
        font_family: inter,
        font_size: 12.0,
        line_height: 1.2,
        weight: FontWeight::Normal,
        color: text_muted,
        ..TextStyle::default()
      },
    ),
  ]));
}
