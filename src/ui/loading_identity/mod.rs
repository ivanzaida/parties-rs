use std::{sync::Arc, time::Duration};

use lurq::{
  app::{
    component::Component,
    ctx::{Ctx, Timeout},
    theme::{PaletteColor, Theme},
  },
  components::{Column, Rect, Row, Text},
  core::Signal,
  layout::{Alignment, layout_kind::Justify},
  node::{
    BackgroundColor, CursorIcon, Element, Style,
    dimension::{Dimension::Pct, IntoDimension},
  },
};

use crate::{
  routes::{ROUTE_CHOOSE_SERVER, ROUTE_IDENTITY_SETUP},
  services::startup::{StartupProgress, StartupProgressLabels, load_startup_data},
  storage::Storage,
  theme,
  ui::{
    common::lucide_icon::{LucideIcon, LucideIconProps},
    loader::loader,
  },
};

const MINIMUM_LOADING_VISIBLE: Duration = Duration::from_millis(3000);
const PREVIEW_RETRY_LOADING_DURATION: Duration = Duration::from_millis(1600);
const PREVIEW_LOADING_ERROR: bool = false;

#[derive(Clone, lurq::DevtoolsInspectable)]
pub struct LoadingIdentityScreenProps {
  pub storage: Signal<Option<Storage>>,
}

impl PartialEq for LoadingIdentityScreenProps {
  fn eq(&self, other: &Self) -> bool {
    self.storage.id() == other.storage.id()
  }
}

pub struct LoadingIdentityScreen {
  retry_nonce: Signal<u64>,
  progress: Signal<StartupProgress>,
  minimum_visible: Signal<bool>,
  minimum_visible_timeout: Timeout,
  navigated: Signal<bool>,
  preview_error_visible: Signal<bool>,
  preview_error_timeout: Timeout,
}

#[derive(Clone)]
struct LoadingIdentityCopy {
  app_name: Arc<str>,
  subtitle: Arc<str>,
  failure_title: Arc<str>,
  failure_description: Arc<str>,
  retry: Arc<str>,
  open_data_folder: Arc<str>,
  startup: StartupProgressLabels,
}

impl LoadingIdentityCopy {
  fn from_ctx(ctx: &Ctx) -> Self {
    Self {
      app_name: ctx.t("common.app_name"),
      subtitle: ctx.t("loading_identity.subtitle"),
      failure_title: ctx.t("loading_identity.failure.title"),
      failure_description: ctx.t("loading_identity.failure.description"),
      retry: ctx.t("loading_identity.action.retry"),
      open_data_folder: ctx.t("loading_identity.action.open_data_folder"),
      startup: StartupProgressLabels {
        starting: ctx.t("loading_identity.progress.starting"),
        opening_storage: ctx.t("loading_identity.progress.opening_storage"),
        checking_identity: ctx.t("loading_identity.progress.checking_identity"),
        loading_servers: ctx.t("loading_identity.progress.loading_servers"),
        preparing_workspace: ctx.t("loading_identity.progress.preparing_workspace"),
        opening_workspace: ctx.t("loading_identity.progress.opening_workspace"),
      },
    }
  }
}

impl Component for LoadingIdentityScreen {
  type Props = LoadingIdentityScreenProps;

  fn create(ctx: &mut Ctx) -> Self {
    let minimum_visible = ctx.signal(false);
    let minimum_visible_timeout = ctx.create_timeout(MINIMUM_LOADING_VISIBLE, {
      let minimum_visible = minimum_visible.clone();
      move || minimum_visible.set(true)
    });
    let preview_error_visible = ctx.signal(true);
    let preview_error_timeout = ctx.create_timeout(PREVIEW_RETRY_LOADING_DURATION, {
      let preview_error_visible = preview_error_visible.clone();
      move || preview_error_visible.set(true)
    });

    Self {
      retry_nonce: ctx.signal(0),
      progress: ctx.signal(StartupProgress::new(0.08, ctx.t("loading_identity.progress.starting"))),
      minimum_visible,
      minimum_visible_timeout,
      navigated: ctx.signal(false),
      preview_error_visible,
      preview_error_timeout,
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    if !self.minimum_visible.get_untracked() && !self.minimum_visible_timeout.is_active() {
      self.minimum_visible_timeout.start();
    }

    let props = ctx.props::<Self::Props>().clone();
    let copy = LoadingIdentityCopy::from_ctx(ctx);
    let retry_nonce = self.retry_nonce.get();
    let startup = ctx
      .future(retry_nonce, {
        let progress = self.progress.clone();
        let startup_copy = copy.startup.clone();
        move |_| load_startup_data(progress.clone(), startup_copy.clone())
      })
      .state()
      .get();
    let progress = if let Some(error) = startup.error.as_ref() {
      StartupProgress::new(
        1.0,
        ctx.t_args("loading_identity.progress.storage_failed", [("error", error.clone())]),
      )
    } else {
      self.progress.get()
    };
    let preview_error = ctx.t("loading_identity.preview.storage_error");
    let error = if PREVIEW_LOADING_ERROR {
      self.preview_error_visible.get().then(|| preview_error.to_string())
    } else {
      startup.error
    };

    if !PREVIEW_LOADING_ERROR
      && let Some(data) = startup.data.as_ref()
      && self.minimum_visible.get()
      && !self.navigated.get_untracked()
    {
      self.navigated.set(true);
      if props.storage.get_untracked() != data.storage {
        props.storage.set(data.storage.clone());
      }
      if let Some(navigator) = ctx.navigator() {
        let route = if data.has_identity {
          ROUTE_CHOOSE_SERVER
        } else {
          ROUTE_IDENTITY_SETUP
        };
        navigator.replace(route);
      }
    }

    let content = if let Some(error) = error {
      self.failure_screen(ctx, &error, self.progress.clone(), &copy)
    } else {
      self.loading_screen(ctx, &progress, &copy)
    };

    Column::new()
      .width(Pct(100.0))
      .height(Pct(100.0))
      .flex(1.0)
      .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceBase))
      .align_items(Alignment::Center)
      .justify(Justify::Center)
      .clip()
      .child(content)
  }
}

impl LoadingIdentityScreen {
  fn loading_screen(&self, ctx: &mut Ctx, progress: &StartupProgress, copy: &LoadingIdentityCopy) -> Element {
    Column::new()
      .align_items(Alignment::Center)
      .spacing(28.px())
      .child(self.brand(ctx, copy))
      .child(self.progress_group(ctx.theme(), progress))
      .into()
  }

  fn brand(&self, ctx: &mut Ctx, copy: &LoadingIdentityCopy) -> impl Into<Element> {
    Column::new()
      .align_items(Alignment::Center)
      .spacing(18.px())
      .child(self.brand_mark(ctx))
      .child(
        Column::new()
          .align_items(Alignment::Center)
          .spacing(6.px())
          .child(Text::new(&copy.app_name).variant(theme::TypographyStyle::Title))
          .child(
            Text::new(&copy.subtitle)
              .variant(theme::TypographyStyle::Description)
              .color(PaletteColor::TextMuted),
          ),
      )
  }

  fn brand_mark(&self, ctx: &mut Ctx) -> impl Into<Element> {
    Column::new()
      .width(68.px())
      .height(68.px())
      .align_items(Alignment::Center)
      .justify(Justify::Center)
      .background(BackgroundColor::Palette(theme::PaletteColor::Accent))
      .rounded(18.0)
      .child(ctx.mount::<LucideIcon>(LucideIconProps {
        icon: "volume-2",
        size: 30.0,
        color: theme::palette().text_inverse,
      }))
  }

  fn progress_group(&self, theme: &Theme, progress: &StartupProgress) -> impl Into<Element> {
    let progress_width = 440.0 * progress.ratio;
    let progress_color = BackgroundColor::Palette(theme::PaletteColor::Accent);

    Column::new()
      .align_items(Alignment::Center)
      .spacing(16.px())
      .child(
        Row::new()
          .width(440.px())
          .height(5.px())
          .align_items(Alignment::Center)
          .background(PaletteColor::Extra("surface_hover".into()))
          .rounded(3.0)
          .clip()
          .child(Rect::new(progress_width, 5.0).rounded(3.0).background(progress_color)),
      )
      .child(
        Row::new()
          .align_items(Alignment::Center)
          .spacing(10.px())
          .child(self.spinner(theme))
          .child(Text::new(&progress.label).variant(theme::TypographyStyle::Description)),
      )
  }

  fn spinner(&self, _theme: &Theme) -> impl Into<Element> {
    loader(16.px())
  }

  fn failure_screen(
    &self,
    ctx: &mut Ctx,
    error: &str,
    progress: Signal<StartupProgress>,
    copy: &LoadingIdentityCopy,
  ) -> Element {
    Column::new()
      .align_items(Alignment::Center)
      .spacing(26.px())
      .child(
        Column::new()
          .align_items(Alignment::Center)
          .spacing(20.px())
          .child(
            Column::new()
              .width(68.px())
              .height(68.px())
              .align_items(Alignment::Center)
              .justify(Justify::Center)
              .background(BackgroundColor::Palette(theme::PaletteColor::DangerMuted))
              .border_inside(1.0, theme::PaletteColor::Danger)
              .rounded(16.0)
              .child(ctx.mount::<LucideIcon>(LucideIconProps {
                icon: "database",
                size: 28.0,
                color: theme::palette().danger,
              })),
          )
          .child(
            Column::new()
              .align_items(Alignment::Center)
              .spacing(10.px())
              .child(Text::new(&copy.failure_title).variant(theme::TypographyStyle::Title))
              .child(
                Text::new(&copy.failure_description)
                  .variant(theme::TypographyStyle::Description)
                  .width(490.px()),
              ),
          ),
      )
      .child(self.error_notice(ctx, error))
      .child(self.failure_actions(ctx, progress, copy))
      .into()
  }

  fn error_notice(&self, ctx: &mut Ctx, error: &str) -> impl Into<Element> {
    let (code, detail) = storage_error_copy(error);

    Row::new()
      .width(560.px())
      .align_items(Alignment::Center)
      .spacing(12.px())
      .padding_vertical(16.px())
      .padding_horizontal(18.px())
      .background(BackgroundColor::Palette(theme::PaletteColor::DangerMuted))
      .border_inside(1.0, theme::PaletteColor::Danger)
      .rounded(6.0)
      .child(ctx.mount::<LucideIcon>(LucideIconProps {
        icon: "triangle-alert",
        size: 16.0,
        color: theme::palette().danger,
      }))
      .child(
        Column::new()
          .flex(1.0)
          .spacing(6.px())
          .child(
            Text::new(&code)
              .variant(theme::TypographyStyle::Button)
              .color(PaletteColor::Danger),
          )
          .child(
            Text::new(&detail)
              .variant(theme::TypographyStyle::Description)
              .width(Pct(100.0)),
          ),
      )
  }

  fn failure_actions(
    &self,
    ctx: &mut Ctx,
    progress: Signal<StartupProgress>,
    copy: &LoadingIdentityCopy,
  ) -> impl Into<Element> {
    let retry_nonce = self.retry_nonce.clone();
    let minimum_visible = self.minimum_visible.clone();
    let minimum_visible_timeout = self.minimum_visible_timeout.clone();
    let navigated = self.navigated.clone();
    let preview_error_visible = self.preview_error_visible.clone();
    let preview_error_timeout = self.preview_error_timeout.clone();
    let starting = copy.startup.starting.clone();

    Row::new()
      .width(560.px())
      .spacing(14.px())
      .child(
        self
          .failure_button(ctx, &copy.retry, Some("rotate-cw"), true)
          .on_click(move |_| {
            if PREVIEW_LOADING_ERROR {
              preview_error_visible.set(false);
              preview_error_timeout.restart();
            }
            minimum_visible.set(false);
            minimum_visible_timeout.restart();
            navigated.set(false);
            progress.set(StartupProgress::new(0.08, starting.clone()));
            retry_nonce.update(|nonce| *nonce = nonce.wrapping_add(1));
          }),
      )
      .child(
        self
          .failure_button(ctx, &copy.open_data_folder, None, false)
          .on_click(|_| {
            let _ = Storage::open_default_data_dir();
          }),
      )
  }

  fn failure_button(&self, ctx: &mut Ctx, label: &str, icon: Option<&'static str>, primary: bool) -> Row {
    let background = if primary {
      BackgroundColor::Palette(theme::PaletteColor::Accent)
    } else {
      BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised)
    };
    let hover_background = if primary {
      BackgroundColor::Palette(theme::PaletteColor::AccentHover)
    } else {
      BackgroundColor::Palette(PaletteColor::Extra("surface_hover".into()))
    };
    let border = if primary {
      theme::PaletteColor::Accent
    } else {
      theme::PaletteColor::BorderStrong
    };
    let label_color = if primary {
      PaletteColor::TextInverse
    } else {
      PaletteColor::TextPrimary
    };
    let icon_color = if primary {
      theme::palette().text_inverse
    } else {
      theme::palette().text_secondary
    };
    let mut button = Row::new()
      .flex(1.0)
      .height(34.px())
      .align_items(Alignment::Center)
      .justify(Justify::Center)
      .spacing(8.px())
      .background(background)
      .border_inside(1.0, border)
      .rounded(5.0)
      .cursor(CursorIcon::Pointer)
      .hovered_style(Style::new().background(hover_background));

    if let Some(icon) = icon {
      button = button.child(ctx.mount::<LucideIcon>(LucideIconProps {
        icon,
        size: 15.0,
        color: icon_color,
      }));
    }

    button.child(
      Text::new(label)
        .variant(theme::TypographyStyle::Button)
        .color(label_color),
    )
  }
}

fn storage_error_copy(error: &str) -> (String, String) {
  let lower = error.to_ascii_lowercase();
  let code = if lower.contains("cantopen") || lower.contains("unable to open") {
    "SQLITE_CANTOPEN"
  } else if lower.starts_with("sqlite:") {
    "SQLITE_ERROR"
  } else if lower.starts_with("io:") {
    "IO_ERROR"
  } else {
    "STORAGE_ERROR"
  };
  let detail = error
    .split_once(':')
    .map(|(_, detail)| detail.trim())
    .filter(|detail| !detail.is_empty())
    .unwrap_or(error);

  (code.to_owned(), detail.to_owned())
}
