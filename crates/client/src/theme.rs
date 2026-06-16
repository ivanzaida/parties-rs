use std::{collections::HashMap, sync::Arc};

pub use lurq::app::theme::{PaletteColor, RadiusSize, SpacingSize, TypographyStyle};
use lurq::{
  app::theme::{
    FormButtonRole, FormButtonTheme, FormFieldTheme, FormInputTheme, FormTextRole, FormTheme, Theme, ThemeFonts,
    ThemeMarkdown, ThemePalette, ThemeRadii, ThemeSpacing, ThemeTypography,
  },
  layout::text_style::{FontWeight, TextStyle},
  node::{color::Color, dimension::Dimension, padding::Padding, spacing_value::SpacingValue},
};

pub fn palette() -> ThemePalette {
  ThemePalette {
    accent: Color::from_hex("#6EA8D8"),
    accent_hover: Color::from_hex("#7DB7E6"),
    accent_muted: Color::from_hex("#121A23"),
    surface_base: Color::from_hex("#0B0C0E"),
    surface_panel: Color::from_hex("#15171A"),
    surface_raised: Color::from_hex("#1B1E23"),
    surface_input: Color::from_hex("#171A1E"),
    border: Color::from_hex("#30343A"),
    border_strong: Color::from_hex("#3A4047"),
    border_focus: Color::from_hex("#6EA8D8"),
    text_primary: Color::from_hex("#F4F4F2"),
    text_secondary: Color::from_hex("#B7B2AA"),
    text_muted: Color::from_hex("#7D766C"),
    text_inverse: Color::from_hex("#0B0C0E"),
    success: Color::from_hex("#42D28B"),
    success_muted: Color::from_hex("#10251A"),
    warning: Color::from_hex("#D6B25E"),
    warning_muted: Color::from_hex("#2B2418"),
    danger: Color::from_hex("#FF6B5F"),
    danger_muted: Color::from_hex("#2A1A1C"),
    info: Color::from_hex("#69A7FF"),
    info_muted: Color::from_hex("#162233"),
    extra: HashMap::from_iter([("surface_hover".into(), Color::from_hex("#232830"))]),
  }
}

pub fn setup(theme: &Theme) {
  let palette = palette();
  theme.set_palette(palette.clone());
  theme.set_markdown(markdown(&palette));

  theme.set_spacing(ThemeSpacing {
    xs: Dimension::Px(4.0),
    sm: Dimension::Px(7.0),
    md: Dimension::Px(10.0),
    lg: Dimension::Px(14.0),
    xl: Dimension::Px(20.0),
    section: Dimension::Px(28.0),
  });

  theme.set_radii(ThemeRadii {
    sm: 3.0,
    md: 5.0,
    lg: 6.0,
  });

  let inter: Arc<str> = Arc::from("Inter");
  let jetbrains: Arc<str> = Arc::from("JetBrains Mono");

  let heading = TextStyle {
    font_family: inter.clone(),
    font_size: 15.0,
    line_height: 1.2,
    weight: FontWeight::Bold,
    color: palette.text_primary,
    ..TextStyle::default()
  };
  let body = TextStyle {
    font_family: inter.clone(),
    font_size: 12.0,
    line_height: 1.2,
    weight: FontWeight::Normal,
    color: palette.text_secondary,
    ..TextStyle::default()
  };
  let mono = TextStyle {
    font_family: jetbrains.clone(),
    font_size: 12.0,
    line_height: 1.2,
    weight: FontWeight::Normal,
    color: palette.text_primary,
    ..TextStyle::default()
  };

  theme.set_fonts(ThemeFonts {
    heading: heading.clone(),
    body: body.clone(),
    mono: mono.clone(),
  });

  theme.set_typography(ThemeTypography {
    heading,
    body,
    caption: TextStyle {
      font_family: inter.clone(),
      font_size: 12.0,
      line_height: 1.2,
      weight: FontWeight::Bold,
      color: palette.text_muted,
      ..TextStyle::default()
    },
    label: TextStyle {
      font_family: inter.clone(),
      font_size: 10.0,
      line_height: 1.2,
      weight: FontWeight::Bold,
      color: palette.text_secondary,
      ..TextStyle::default()
    },
    mono,
    title: TextStyle {
      font_family: inter.clone(),
      font_size: 26.0,
      line_height: 1.2,
      weight: FontWeight::Bold,
      color: palette.text_primary,
      ..TextStyle::default()
    },
    description: TextStyle {
      font_family: inter.clone(),
      font_size: 14.0,
      line_height: 1.5,
      weight: FontWeight::Normal,
      color: palette.text_secondary,
      ..TextStyle::default()
    },
    button: TextStyle {
      font_family: inter.clone(),
      font_size: 13.0,
      line_height: 1.2,
      weight: FontWeight::Bold,
      color: palette.text_primary,
      ..TextStyle::default()
    },
    field_label: TextStyle {
      font_family: jetbrains,
      font_size: 10.0,
      line_height: 1.2,
      weight: FontWeight::Bold,
      color: palette.text_muted,
      ..TextStyle::default()
    },
    link: TextStyle {
      font_family: inter,
      font_size: 13.0,
      line_height: 1.2,
      weight: FontWeight::Normal,
      color: palette.text_muted,
      ..TextStyle::default()
    },
  });

  theme.set_form(FormTheme {
    field: FormFieldTheme {
      spacing: SpacingValue::from(Dimension::Px(7.0)),
      label: FormTextRole {
        typography: TypographyStyle::FieldLabel,
        color: PaletteColor::TextMuted,
      },
      hint: FormTextRole {
        typography: TypographyStyle::Caption,
        color: PaletteColor::TextSecondary,
      },
      error: FormTextRole {
        typography: TypographyStyle::Caption,
        color: PaletteColor::Danger,
      },
    },
    input: FormInputTheme {
      height: Dimension::Px(40.0),
      padding: Padding::symmetric(10.0, 10.0),
      radius: RadiusSize::Md,
      background: PaletteColor::SurfacePanel,
      border: PaletteColor::Border,
      border_focus: PaletteColor::BorderFocus,
      background_error: PaletteColor::DangerMuted,
      border_error: PaletteColor::Danger,
      text: FormTextRole {
        typography: TypographyStyle::Mono,
        color: PaletteColor::TextPrimary,
      },
      placeholder: FormTextRole {
        typography: TypographyStyle::Mono,
        color: PaletteColor::TextSecondary,
      },
      caret: PaletteColor::Accent,
    },
    button: FormButtonTheme {
      primary: FormButtonRole {
        width: Dimension::Pct(100.0),
        height: Dimension::Px(34.0),
        padding: Padding::horizontal(0.0),
        radius: RadiusSize::Md,
        background: PaletteColor::Accent,
        border: PaletteColor::Accent,
        background_hover: PaletteColor::AccentHover,
        border_hover: PaletteColor::AccentHover,
        background_active: PaletteColor::AccentHover,
        border_active: PaletteColor::AccentHover,
        text: FormTextRole {
          typography: TypographyStyle::Button,
          color: PaletteColor::TextInverse,
        },
      },
      secondary: FormButtonRole {
        width: Dimension::Pct(100.0),
        height: Dimension::Px(34.0),
        padding: Padding::horizontal(0.0),
        radius: RadiusSize::Md,
        background: PaletteColor::SurfaceRaised,
        border: PaletteColor::Border,
        background_hover: PaletteColor::SurfaceInput,
        border_hover: PaletteColor::Border,
        background_active: PaletteColor::SurfaceInput,
        border_active: PaletteColor::Border,
        text: FormTextRole {
          typography: TypographyStyle::Button,
          color: PaletteColor::TextPrimary,
        },
      },
    },
    ..FormTheme::default()
  });
}

pub fn markdown(palette: &ThemePalette) -> ThemeMarkdown {
  let mut markdown = ThemeMarkdown::default();
  markdown.document_spacing = 2.0;
  markdown.list_item_spacing = 2.0;
  markdown.nested_block_spacing = 2.0;
  markdown.code_block_spacing = 4.0;
  markdown.blockquote_gap = 6.0;
  markdown.list_marker_gap = 4.0;
  markdown.unordered_list_marker_width = 14.0;
  markdown.ordered_list_marker_width = 28.0;

  markdown.link.text.color = Some(palette.accent);
  markdown.list_marker.text.color = Some(palette.text_muted);
  markdown.blockquote_marker.text.color = Some(palette.text_muted);
  markdown.inline_code.text.color = Some(palette.text_primary);
  markdown.code_block.text.color = Some(palette.text_primary);
  markdown.code_block_label.text.color = Some(palette.text_muted);
  markdown.table_marker.text.color = Some(palette.text_muted);
  markdown.table_header.text.color = Some(palette.text_primary);
  markdown.table_cell.text.color = Some(palette.text_secondary);

  markdown.code_block_box.background = Some(palette.surface_input);
  markdown.code_block_box.border_color = Some(palette.border);
  markdown.code_block_box.radius = Some(5.0);
  markdown.code_block_box.padding = Some(8.0);

  markdown.blockquote_box.background = Some(palette.surface_raised);
  markdown.blockquote_box.border_color = Some(palette.accent);
  markdown.blockquote_box.border_width = Some(2.0);
  markdown.blockquote_box.radius = Some(3.0);
  markdown.blockquote_box.padding = Some(8.0);

  markdown.table_box.border_color = Some(palette.border);
  markdown.table_header_box.background = Some(palette.surface_raised);
  markdown.table_header_box.padding = Some(6.0);
  markdown.table_cell_box.padding = Some(6.0);

  markdown
}
