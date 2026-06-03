use std::sync::Arc;

use lurq::app::theme::{PaletteId, RadiusId, Theme, ThemeFonts, ThemePalette, ThemeRadii, ThemeTypography, TypographyId};
use lurq::layout::text_style::{FontWeight, TextStyle};
use lurq::node::color::Color;

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
        (ACCENT, Color::from_hex("#7C6CF0")),
        (ACCENT_HOVER, Color::from_hex("#7E6FF0")),
        (ACCENT_MUTED, Color::from_hex("#6C5CE71A")),
        (BG_PRIMARY, Color::from_hex("#0D0F15")),
        (BG_SECONDARY, Color::from_hex("#151821")),
        (BG_TERTIARY, Color::from_hex("#1B1F2B")),
        (BG_ELEVATED, Color::from_hex("#242938")),
        (BG_INPUT, Color::from_hex("#1A1E2A")),
        (BORDER, Color::from_hex("#343A50")),
        (BORDER_LIGHT, Color::from_hex("#454D68")),
        (TEXT_PRIMARY, Color::from_hex("#E8ECF4")),
        (TEXT_SECONDARY, Color::from_hex("#A0A6BC")),
        (TEXT_MUTED, Color::from_hex("#6A7192")),
        (TEXT_INVERSE, Color::from_hex("#0F1117")),
        (GREEN, Color::from_hex("#00D68F")),
        (GREEN_MUTED, Color::from_hex("#00D68F26")),
        (RED, Color::from_hex("#FF4757")),
        (RED_MUTED, Color::from_hex("#FF475726")),
        (ORANGE, Color::from_hex("#FFA502")),
    ]));

    theme.set_radii(ThemeRadii::from_values([
        (RADIUS_S, 6.0),
        (RADIUS_M, 10.0),
        (RADIUS_L, 14.0),
    ]));

    let text_primary = Color::from_hex("#E8ECF4");
    let text_secondary = Color::from_hex("#A0A6BC");
    let text_muted = Color::from_hex("#6A7192");

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
        (TYP_CAPTION, TextStyle {
            font_family: inter.clone(),
            font_size: 11.0,
            line_height: 1.2,
            weight: FontWeight::Normal,
            color: text_muted,
            ..TextStyle::default()
        }),
        (TYP_LABEL, TextStyle {
            font_family: inter.clone(),
            font_size: 10.0,
            line_height: 1.2,
            weight: FontWeight::Bold,
            color: text_secondary,
            ..TextStyle::default()
        }),
        (TYP_MONO, mono),
        (TYP_TITLE, TextStyle {
            font_family: inter.clone(),
            font_size: 22.0,
            line_height: 1.2,
            weight: FontWeight::Bold,
            color: text_primary.clone(),
            ..TextStyle::default()
        }),
        (TYP_DESC, TextStyle {
            font_family: inter.clone(),
            font_size: 13.0,
            line_height: 1.4,
            weight: FontWeight::Normal,
            color: text_secondary.clone(),
            ..TextStyle::default()
        }),
        (TYP_BUTTON, TextStyle {
            font_family: inter.clone(),
            font_size: 13.0,
            line_height: 1.2,
            weight: FontWeight::Bold,
            color: text_primary.clone(),
            ..TextStyle::default()
        }),
        (TYP_SECTION, TextStyle {
            font_family: inter.clone(),
            font_size: 11.0,
            line_height: 1.2,
            weight: FontWeight::Bold,
            color: text_muted.clone(),
            ..TextStyle::default()
        }),
        (TYP_FIELD_LABEL, TextStyle {
            font_family: inter.clone(),
            font_size: 12.0,
            line_height: 1.2,
            weight: FontWeight::Normal,
            color: text_secondary.clone(),
            ..TextStyle::default()
        }),
        (TYP_LINK, TextStyle {
            font_family: inter,
            font_size: 12.0,
            line_height: 1.2,
            weight: FontWeight::Normal,
            color: text_muted,
            ..TextStyle::default()
        }),
    ]));
}
