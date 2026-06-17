use lurq::{
  app::{component::Component, ctx::Ctx},
  components::{Column, Row, Text},
  core::Signal,
  layout::{Alignment, layout_kind::Justify},
  node::{BackgroundColor, CursorIcon, Element, Style, color::Color, dimension::Dimension},
};
use rfd::AsyncFileDialog;

use crate::{
  routes::{ROUTE_CHOOSE_SERVER, ROUTE_IDENTITY_SETUP},
  storage::{LegacyPartiesImportSummary, Storage},
  theme,
  ui::{
    common::lucide_icon::{LucideIcon, LucideIconProps},
    onboarding_shell::{self, OnboardingIntroCopy},
  },
};

pub struct ImportLegacyConfigScreen {
  status: Signal<Option<ImportLegacyStatus>>,
  navigated: Signal<bool>,
}

#[derive(Clone, PartialEq, lurq::DevtoolsInspectable)]
enum ImportLegacyStatus {
  Error(String),
}

impl Component for ImportLegacyConfigScreen {
  type Props = ();

  fn create(ctx: &mut Ctx) -> Self {
    Self {
      status: ctx.signal(None),
      navigated: ctx.signal(false),
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    onboarding_shell::screen(
      onboarding_shell::intro(
        ctx,
        OnboardingIntroCopy {
          app_name: &ctx.t("common.app_name"),
          headline: &ctx.t("identity_import_legacy.intro.headline"),
          description: &ctx.t("identity_import_legacy.intro.description"),
          footer_note: &ctx.t("identity_import_legacy.intro.footer"),
        },
      ),
      onboarding_shell::panel(
        ctx.breakpoint(),
        Column::new()
          .width(Dimension::Pct(100.0))
          .spacing(theme::SpacingSize::Xl)
          .child(panel_header(
            &ctx.t("identity_import_legacy.overline"),
            &ctx.t("identity_import_legacy.title"),
            &ctx.t("identity_import_legacy.subtitle"),
          ))
          .child(source_panel(ctx))
          .child(self.status(ctx))
          .child(self.actions(ctx)),
      ),
    )
  }
}

impl ImportLegacyConfigScreen {
  fn status(&self, ctx: &mut Ctx) -> Element {
    match self.status.get() {
      Some(ImportLegacyStatus::Error(error)) => error_row(ctx, &error).into(),
      None => Column::new().into(),
    }
  }

  fn actions(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let navigator = ctx.navigator();
    let back_navigator = navigator.clone();
    let import_navigator = navigator;
    let storage = ctx.use_context::<Storage>();
    let status = self.status.clone();
    let import = ctx.future_action(|storage: Storage| async move {
      let Some(file) = AsyncFileDialog::new()
        .set_title("Import emcifuntik/parties config")
        .add_filter("SQLite database", &["db", "sqlite", "sqlite3"])
        .pick_file()
        .await
      else {
        return Ok(None);
      };
      let path = file.path().to_owned();
      tokio::task::spawn_blocking(move || storage.import_legacy_parties_config(path))
        .await
        .map_err(|error| error.to_string())?
        .map(Some)
        .map_err(|error| error.to_string())
    });
    let import_state = import.state().get();
    let importing = import_state.is_pending();

    if !self.navigated.get_untracked()
      && let Some(Some(summary)) = import_state.data
    {
      if summary.imported_identity {
        self.navigated.set(true);
        if let Some(navigator) = import_navigator.as_ref() {
          navigator.replace(ROUTE_CHOOSE_SERVER);
        }
      } else {
        status.set(Some(ImportLegacyStatus::Error(import_without_identity_message(
          summary,
        ))));
      }
    }

    if let Some(error) = import_state.error {
      status.set(Some(ImportLegacyStatus::Error(format!("Import failed: {error}"))));
    }
    let import_label = if importing {
      ctx.t("identity_import_legacy.action.importing")
    } else {
      ctx.t("identity_import_legacy.action.import")
    };

    Row::new()
      .width(Dimension::Pct(100.0))
      .align_items(Alignment::Center)
      .justify(Justify::SpaceBetween)
      .child(
        action_button(
          ctx,
          "arrow-left",
          &ctx.t("identity_import_legacy.action.back"),
          ButtonTone::Ghost,
        )
        .on_click(move |_| {
          if let Some(navigator) = back_navigator.as_ref() {
            navigator.replace(ROUTE_IDENTITY_SETUP);
          }
        }),
      )
      .child(
        action_button(
          ctx,
          "database",
          &import_label,
          if importing {
            ButtonTone::Disabled
          } else {
            ButtonTone::Primary
          },
        )
        .on_click(move |_| {
          if importing {
            return;
          }
          let Some(storage) = storage.as_ref() else {
            status.set(Some(ImportLegacyStatus::Error(
              "Local storage is unavailable.".to_owned(),
            )));
            return;
          };
          status.set(None);
          import.run(storage.clone());
        }),
      )
  }
}

fn source_panel(ctx: &mut Ctx) -> Row {
  Row::new()
    .width(Dimension::Pct(100.0))
    .align_items(Alignment::Start)
    .spacing(theme::SpacingSize::Lg)
    .padding_vertical(theme::SpacingSize::Lg)
    .padding_horizontal(theme::SpacingSize::Xl)
    .rounded(theme::RadiusSize::Lg)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised))
    .border_inside(1.0, theme::PaletteColor::Border)
    .child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon: "database",
      size: 20.0,
      color: theme::palette().text_secondary,
    }))
    .child(
      Column::new()
        .flex(1.0)
        .spacing(theme::SpacingSize::Xs)
        .child(Text::new(&ctx.t("identity_import_legacy.source.label")).variant(theme::TypographyStyle::Heading))
        .child(
          Text::new(&ctx.t("identity_import_legacy.source.path"))
            .variant(theme::TypographyStyle::Mono)
            .color(theme::PaletteColor::TextSecondary)
            .width(Dimension::Pct(100.0)),
        )
        .child(
          Text::new(&ctx.t("identity_import_legacy.source.detail"))
            .variant(theme::TypographyStyle::Link)
            .color(theme::PaletteColor::TextMuted)
            .width(Dimension::Pct(100.0)),
        ),
    )
}

fn import_without_identity_message(summary: LegacyPartiesImportSummary) -> String {
  format!(
    "Imported {} saved server(s), but no identity was found in the legacy config.",
    summary.imported_servers
  )
}

fn panel_header(overline: &str, title: &str, subtitle: &str) -> Column {
  Column::new()
    .width(Dimension::Pct(100.0))
    .spacing(theme::SpacingSize::Md)
    .child(
      Text::new(overline)
        .variant(theme::TypographyStyle::Caption)
        .color(theme::PaletteColor::Accent),
    )
    .child(Text::new(title).variant(theme::TypographyStyle::Title))
    .child(
      Text::new(subtitle)
        .variant(theme::TypographyStyle::Description)
        .width(Dimension::Pct(100.0))
        .max_width(430.0),
    )
}

fn error_row(ctx: &mut Ctx, message: &str) -> impl Into<Element> {
  Row::new()
    .width(Dimension::Pct(100.0))
    .align_items(Alignment::Center)
    .spacing(theme::SpacingSize::Sm)
    .child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon: "triangle-alert",
      size: 14.0,
      color: theme::palette().danger,
    }))
    .child(
      Text::new(message)
        .variant(theme::TypographyStyle::Description)
        .color(theme::PaletteColor::Danger)
        .width(Dimension::Pct(100.0)),
    )
}

#[derive(Clone, Copy)]
enum ButtonTone {
  Primary,
  Ghost,
  Disabled,
}

fn action_button(ctx: &mut Ctx, icon: &'static str, label: &str, tone: ButtonTone) -> Row {
  let (background, border, text_color, icon_color, hover_background) = match tone {
    ButtonTone::Primary => (
      BackgroundColor::Palette(theme::PaletteColor::Accent),
      BackgroundColor::Palette(theme::PaletteColor::Accent),
      theme::PaletteColor::TextInverse,
      theme::palette().text_inverse,
      BackgroundColor::Palette(theme::PaletteColor::AccentHover),
    ),
    ButtonTone::Ghost => (
      BackgroundColor::Color(Color::from_hex("#00000000")),
      BackgroundColor::Color(Color::from_hex("#00000000")),
      theme::PaletteColor::TextSecondary,
      theme::palette().text_secondary,
      BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised),
    ),
    ButtonTone::Disabled => (
      BackgroundColor::Palette(theme::PaletteColor::SurfaceInput),
      BackgroundColor::Palette(theme::PaletteColor::Border),
      theme::PaletteColor::TextMuted,
      theme::palette().text_muted,
      BackgroundColor::Palette(theme::PaletteColor::SurfaceInput),
    ),
  };
  let enabled = !matches!(tone, ButtonTone::Disabled);

  let button = Row::new()
    .height(34.0)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .spacing(theme::SpacingSize::Sm)
    .padding_horizontal(theme::SpacingSize::Lg)
    .rounded(theme::RadiusSize::Md)
    .background(background)
    .border_inside(1.0, border)
    .cursor(if enabled {
      CursorIcon::Pointer
    } else {
      CursorIcon::NotAllowed
    });

  let button = if enabled {
    button
      .hovered_style(Style::new().background(hover_background.clone()))
      .active_style(Style::new().background(hover_background))
  } else {
    button
  };

  button
    .child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon,
      size: 16.0,
      color: icon_color,
    }))
    .child(
      Text::new(label)
        .variant(theme::TypographyStyle::Button)
        .color(text_color)
        .nowrap(),
    )
}
