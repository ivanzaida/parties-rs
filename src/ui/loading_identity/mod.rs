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
  routes::{ROUTE_CHOOSE_SERVER, ROUTE_IDENTITY_SETUP, ROUTE_LOBBY},
  services::{
    startup::{StartupProgress, StartupProgressLabels, load_startup_data},
    updater::{StartupUpdateStatus, restart_into_update, run_startup_update_check},
  },
  session::ServerSession,
  storage::{AppSettings, Storage},
  theme,
  ui::{
    brand_logo::logo_mark,
    common::lucide_icon::{LucideIcon, LucideIconProps},
    connect_server::{ConnectErrorCopy, connect_and_store},
    loader::loader,
  },
};

const MINIMUM_LOADING_VISIBLE: Duration = Duration::from_millis(0);
const PREVIEW_RETRY_LOADING_DURATION: Duration = Duration::from_millis(0);
const PREVIEW_LOADING_ERROR: bool = false;

#[derive(Clone, lurq::DevtoolsInspectable)]
pub struct LoadingIdentityScreenProps {
  pub storage: Signal<Option<Storage>>,
  pub startup_error: Option<String>,
  pub update_status: Signal<StartupUpdateStatus>,
}

impl PartialEq for LoadingIdentityScreenProps {
  fn eq(&self, other: &Self) -> bool {
    self.storage.id() == other.storage.id()
      && self.startup_error == other.startup_error
      && self.update_status.id() == other.update_status.id()
  }
}

pub struct LoadingIdentityScreen {
  retry_nonce: Signal<u64>,
  progress: Signal<StartupProgress>,
  minimum_visible: Signal<bool>,
  minimum_visible_timeout: Timeout,
  navigated: Signal<bool>,
  resume_started: Signal<bool>,
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
      resume_started: ctx.signal(false),
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
    let update = ctx
      .future(retry_nonce, {
        let update_status = props.update_status.clone();
        move |_| run_startup_update_check(update_status.clone())
      })
      .state()
      .get();
    let startup = if retry_nonce == 0 && props.startup_error.is_some() {
      None
    } else {
      Some(
        ctx
          .future(retry_nonce, {
            let progress = self.progress.clone();
            let startup_copy = copy.startup.clone();
            let initial_storage = props.storage.get_untracked();
            move |_| load_startup_data(progress.clone(), startup_copy.clone(), initial_storage.clone())
          })
          .state()
          .get(),
      )
    };
    let session = ctx.use_context::<ServerSession>();
    let resume_errors = ConnectErrorCopy::from_ctx(ctx);
    let restore_update_resume = ctx.future_action(move |storage: Storage| {
      let session = session.clone();
      let errors = resume_errors.clone();
      async move { restore_update_resume_after_restart(storage, session, errors).await }
    });
    let restore_update_resume_state = restore_update_resume.state().get();
    let startup_error = startup.as_ref().and_then(|startup| startup.error.clone());
    let initial_error = (retry_nonce == 0).then(|| props.startup_error.clone()).flatten();
    let progress_error = startup_error.as_ref().or(initial_error.as_ref());
    let progress = if let Some(error) = progress_error {
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
      initial_error.or(startup_error)
    };
    let update_status = props.update_status.get();
    let update_blocks_startup = update.is_pending() || matches!(update_status, StartupUpdateStatus::Ready { .. });

    if !PREVIEW_LOADING_ERROR
      && let Some(data) = startup.as_ref().and_then(|startup| startup.data.as_ref())
      && self.minimum_visible.get()
      && !update_blocks_startup
      && !self.navigated.get_untracked()
    {
      if props.storage.get_untracked() != data.storage {
        props.storage.set(data.storage.clone());
      }
      let mut waiting_for_resume = false;
      if data.has_identity
        && !self.resume_started.get_untracked()
        && let Some(storage) = data.storage.clone()
      {
        self.resume_started.set(true);
        restore_update_resume.run(storage);
        waiting_for_resume = true;
      }
      if data.has_identity
        && self.resume_started.get_untracked()
        && !restore_update_resume_state.is_fulfilled()
        && !restore_update_resume_state.is_rejected()
      {
        waiting_for_resume = true;
      }

      if !waiting_for_resume {
        self.navigated.set(true);
        if let Some(navigator) = ctx.navigator() {
          let route = if restore_update_resume_state.data == Some(true) {
            ROUTE_LOBBY
          } else if data.has_identity {
            ROUTE_CHOOSE_SERVER
          } else {
            ROUTE_IDENTITY_SETUP
          };
          navigator.replace(route);
        }
      }
    }

    let content = if let Some(error) = error {
      self.failure_screen(ctx, &error, self.progress.clone(), &copy)
    } else {
      let displayed_progress = update_progress(ctx, &update_status, update.is_pending()).unwrap_or(progress);
      self.loading_screen(
        ctx,
        &displayed_progress,
        &copy,
        &props.update_status,
        &update_status,
        update.is_pending(),
      )
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

async fn restore_update_resume_after_restart(
  storage: Storage,
  session: Option<ServerSession>,
  errors: ConnectErrorCopy,
) -> Result<bool, String> {
  let resume = match storage.take_update_resume_state() {
    Ok(Some(resume)) => resume,
    Ok(None) => return Ok(false),
    Err(error) => {
      tracing::warn!(target: "updater", "[updater] failed to load restart resume target: {error}");
      return Ok(false);
    }
  };
  let Some(session) = session else {
    tracing::warn!(target: "updater", "[updater] skipped restart resume: no session context");
    return Ok(false);
  };
  let server = match storage.load_server(&resume.server_address) {
    Ok(Some(server)) => server,
    Ok(None) => {
      tracing::warn!(
        target: "updater",
        "[updater] skipped restart resume: saved server missing address={}",
        resume.server_address
      );
      return Ok(false);
    }
    Err(error) => {
      tracing::warn!(target: "updater", "[updater] failed to load restart resume server: {error}");
      return Ok(false);
    }
  };
  let settings = storage.load_settings().unwrap_or_else(|_| AppSettings::default());
  let display_name = if server.display_name.trim().is_empty() {
    settings.display_name.clone()
  } else {
    server.display_name.clone()
  };

  tracing::info!(
    target: "updater",
    "[updater] restoring server after update restart: address={} voice_channel={:?}",
    server.address,
    resume.voice_channel_id
  );
  if let Err(error) = connect_and_store(
    server.address.clone(),
    server.server_password,
    display_name,
    Some(storage.clone()),
    Some(session.clone()),
    errors,
  )
  .await
  {
    tracing::warn!(target: "updater", "[updater] failed to restore server after update restart: {error}");
    return Ok(false);
  }

  let Some(channel_id) = resume.voice_channel_id else {
    return Ok(true);
  };
  let Some(connected_server) = session.server() else {
    tracing::warn!(target: "updater", "[updater] skipped voice channel restore after update restart: no connected server");
    return Ok(true);
  };
  if let Err(error) = connected_server.join_channel(channel_id).await {
    tracing::warn!(
      target: "updater",
      "[updater] failed to rejoin voice channel after update restart: channel={} error={}",
      channel_id,
      error
    );
    return Ok(true);
  }

  let (mut muted, deafened) = (resume.muted, resume.deafened);
  if deafened {
    muted = true;
  }
  session.select_channel(channel_id);
  if let Err(error) = connected_server.update_voice_state(muted, deafened).await {
    tracing::warn!(
      target: "updater",
      "[updater] failed to restore voice state after update restart: channel={} error={}",
      channel_id,
      error
    );
    return Ok(true);
  }
  session.set_local_voice_state(muted, deafened);
  match session.start_voice(settings, "") {
    Ok(()) => tracing::info!(
      target: "updater",
      "[updater] restored voice channel after update restart: channel={} muted={} deafened={}",
      channel_id,
      muted,
      deafened
    ),
    Err(error) => tracing::warn!(
      target: "updater",
      "[updater] failed to restart voice capture after update restart: channel={} error={}",
      channel_id,
      error
    ),
  }

  Ok(true)
}

impl LoadingIdentityScreen {
  fn loading_screen(
    &self,
    ctx: &mut Ctx,
    progress: &StartupProgress,
    copy: &LoadingIdentityCopy,
    update_status_signal: &Signal<StartupUpdateStatus>,
    update_status: &StartupUpdateStatus,
    update_pending: bool,
  ) -> Element {
    let mut content = Column::new()
      .align_items(Alignment::Center)
      .spacing(28.px())
      .child(self.brand(ctx, copy))
      .child(self.progress_group(ctx.theme(), progress));

    if startup_update_panel_visible(update_status, update_pending) {
      content = content.child(self.startup_update_panel(ctx, update_status_signal, update_status, update_pending));
    }

    content.into()
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

  fn brand_mark(&self, _ctx: &mut Ctx) -> impl Into<Element> {
    Column::new()
      .width(68.px())
      .height(68.px())
      .align_items(Alignment::Center)
      .justify(Justify::Center)
      .child(logo_mark(68.px(), 18.0))
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

  fn startup_update_panel(
    &self,
    ctx: &mut Ctx,
    update_status_signal: &Signal<StartupUpdateStatus>,
    status: &StartupUpdateStatus,
    update_pending: bool,
  ) -> impl Into<Element> {
    let version = update_version(status).unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_owned());
    let (title, description) = update_panel_copy(ctx, status, update_pending);
    let ready_path = match status {
      StartupUpdateStatus::Ready { staged_executable, .. } => Some(staged_executable.clone()),
      _ => None,
    };
    let mut panel = Column::new()
      .width(520.px())
      .spacing(16.px())
      .padding_vertical(18.px())
      .padding_horizontal(18.px())
      .rounded(8.0)
      .background(BackgroundColor::Palette(theme::PaletteColor::SurfacePanel))
      .border_inside(1.0, theme::PaletteColor::Border)
      .child(self.update_panel_header(ctx, &version))
      .child(
        Column::new()
          .width(Pct(100.0))
          .spacing(6.px())
          .child(Text::new(&title).variant(theme::TypographyStyle::Heading))
          .child(
            Text::new(&description)
              .variant(theme::TypographyStyle::Link)
              .width(Pct(100.0)),
          ),
      )
      .child(self.update_steps(ctx, status, update_pending));

    if let Some(staged_executable) = ready_path {
      let update_status = update_status_signal.clone();
      let storage = ctx.props::<LoadingIdentityScreenProps>().storage.get_untracked();
      let session = ctx.use_context::<ServerSession>();
      panel = panel.child(Row::new().align_items(Alignment::Center).child(
        self.restart_update_button(ctx, status).on_click(move |_| {
          if let Err(error) = restart_into_update(&staged_executable, storage.as_ref(), session.as_ref()) {
            update_status.set(StartupUpdateStatus::Failed(error));
          }
        }),
      ));
    }

    panel
  }

  fn update_panel_header(&self, ctx: &mut Ctx, version: &str) -> impl Into<Element> {
    Row::new()
      .width(Pct(100.0))
      .align_items(Alignment::Center)
      .spacing(12.px())
      .child(
        Row::new()
          .align_items(Alignment::Center)
          .spacing(7.px())
          .padding_vertical(5.px())
          .padding_horizontal(9.px())
          .rounded(5.0)
          .background(BackgroundColor::Palette(theme::PaletteColor::InfoMuted))
          .border_inside(1.0, BackgroundColor::Color(theme::palette().info.with_opacity(0.35)))
          .child(ctx.mount::<LucideIcon>(LucideIconProps {
            icon: "refresh-cw",
            size: 13.0,
            color: theme::palette().info,
          }))
          .child(
            Text::new(&ctx.t("loading_identity.update.badge"))
              .variant(theme::TypographyStyle::FieldLabel)
              .color(PaletteColor::Info),
          ),
      )
      .child(
        Text::new(&ctx.t_args(
          "loading_identity.update.version_transition",
          [
            ("current", env!("CARGO_PKG_VERSION").to_owned()),
            ("version", version.to_owned()),
          ],
        ))
        .variant(theme::TypographyStyle::Mono)
        .color(PaletteColor::TextMuted),
      )
  }

  fn update_steps(&self, ctx: &mut Ctx, status: &StartupUpdateStatus, update_pending: bool) -> impl Into<Element> {
    let download_description = match status {
      StartupUpdateStatus::Downloading { downloaded, total, .. } => download_progress_label(ctx, *downloaded, *total),
      StartupUpdateStatus::Staging { .. } | StartupUpdateStatus::Ready { .. } => {
        ctx.t("loading_identity.update.step.download.complete")
      }
      _ if update_pending => ctx.t("loading_identity.update.step.download.pending"),
      _ => ctx.t("loading_identity.update.step.waiting_release_check"),
    };
    let (check_state, download_state, restart_state) = update_step_states(status, update_pending);

    Column::new()
      .width(Pct(100.0))
      .spacing(12.px())
      .child(self.update_step(
        ctx,
        &ctx.t("loading_identity.update.step.found.title"),
        &update_found_description(ctx, status, update_pending),
        "check",
        check_state,
      ))
      .child(self.update_step(
        ctx,
        &ctx.t("loading_identity.update.step.download.title"),
        &download_description,
        "refresh-cw",
        download_state,
      ))
      .child(self.update_step(
        ctx,
        &ctx.t("loading_identity.update.step.restart.title"),
        &ctx.t("loading_identity.update.step.restart.description"),
        "rotate-cw",
        restart_state,
      ))
  }

  fn update_step(
    &self,
    ctx: &mut Ctx,
    title: &str,
    description: &str,
    icon: &'static str,
    state: UpdateStepState,
  ) -> impl Into<Element> {
    let (background, border, icon_color) = match state {
      UpdateStepState::Done => (
        theme::PaletteColor::SuccessMuted,
        BackgroundColor::Color(theme::palette().success.with_opacity(0.4)),
        theme::palette().success,
      ),
      UpdateStepState::Active => (
        theme::PaletteColor::InfoMuted,
        BackgroundColor::Color(theme::palette().info.with_opacity(0.4)),
        theme::palette().info,
      ),
      UpdateStepState::Idle => (
        theme::PaletteColor::SurfaceRaised,
        BackgroundColor::Palette(theme::PaletteColor::Border),
        theme::palette().text_secondary,
      ),
    };

    Row::new()
      .width(Pct(100.0))
      .align_items(Alignment::Center)
      .spacing(12.px())
      .child(
        Row::new()
          .width(34.px())
          .height(34.px())
          .align_items(Alignment::Center)
          .justify(Justify::Center)
          .rounded(7.0)
          .background(BackgroundColor::Palette(background))
          .border_inside(1.0, border)
          .child(ctx.mount::<LucideIcon>(LucideIconProps {
            icon,
            size: 16.0,
            color: icon_color,
          })),
      )
      .child(
        Column::new()
          .flex(1.0)
          .spacing(3.px())
          .child(Text::new(title).variant(theme::TypographyStyle::Button))
          .child(
            Text::new(description)
              .variant(theme::TypographyStyle::Link)
              .color(PaletteColor::TextMuted)
              .width(Pct(100.0)),
          ),
      )
  }

  fn restart_update_button(&self, ctx: &mut Ctx, status: &StartupUpdateStatus) -> Row {
    Row::new()
      .height(34.px())
      .align_items(Alignment::Center)
      .justify(Justify::Center)
      .spacing(8.px())
      .padding_horizontal(14.px())
      .background(BackgroundColor::Palette(theme::PaletteColor::Accent))
      .border_inside(1.0, theme::PaletteColor::Accent)
      .rounded(5.0)
      .cursor(CursorIcon::Pointer)
      .hovered_style(Style::new().background(BackgroundColor::Palette(theme::PaletteColor::AccentHover)))
      .child(ctx.mount::<LucideIcon>(LucideIconProps {
        icon: "rotate-cw",
        size: 15.0,
        color: theme::palette().text_inverse,
      }))
      .child(
        Text::new(&ctx.t_args(
          "loading_identity.update.action.restart_launch",
          [("version", update_version(status).unwrap_or_default())],
        ))
        .variant(theme::TypographyStyle::Button)
        .color(PaletteColor::TextInverse),
      )
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
    let resume_started = self.resume_started.clone();
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
            resume_started.set(false);
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum UpdateStepState {
  Done,
  Active,
  Idle,
}

fn startup_update_panel_visible(status: &StartupUpdateStatus, update_pending: bool) -> bool {
  update_pending
    || matches!(
      status,
      StartupUpdateStatus::Checking
        | StartupUpdateStatus::Downloading { .. }
        | StartupUpdateStatus::Staging { .. }
        | StartupUpdateStatus::Ready { .. }
    )
}

fn update_progress(ctx: &Ctx, status: &StartupUpdateStatus, update_pending: bool) -> Option<StartupProgress> {
  match status {
    StartupUpdateStatus::Idle if update_pending => Some(StartupProgress::new(
      0.18,
      ctx.t("loading_identity.update.progress.checking"),
    )),
    StartupUpdateStatus::Checking => Some(StartupProgress::new(
      0.22,
      ctx.t("loading_identity.update.progress.checking"),
    )),
    StartupUpdateStatus::Downloading { downloaded, total, .. } => {
      let ratio = total
        .filter(|total| *total > 0)
        .map(|total| (*downloaded as f32 / total as f32).clamp(0.0, 1.0))
        .unwrap_or(0.35);
      Some(StartupProgress::new(
        0.28 + ratio * 0.5,
        ctx.t_args(
          "loading_identity.update.progress.downloading",
          [("percent", percent_value(ratio))],
        ),
      ))
    }
    StartupUpdateStatus::Staging { .. } => Some(StartupProgress::new(
      0.9,
      ctx.t("loading_identity.update.progress.preparing"),
    )),
    StartupUpdateStatus::Ready { .. } => Some(StartupProgress::new(
      1.0,
      ctx.t("loading_identity.update.progress.ready"),
    )),
    _ => None,
  }
}

fn update_version(status: &StartupUpdateStatus) -> Option<String> {
  match status {
    StartupUpdateStatus::Downloading { version, .. }
    | StartupUpdateStatus::Staging { version }
    | StartupUpdateStatus::Ready { version, .. } => Some(version.clone()),
    _ => None,
  }
}

fn update_panel_copy(ctx: &Ctx, status: &StartupUpdateStatus, update_pending: bool) -> (Arc<str>, Arc<str>) {
  match status {
    StartupUpdateStatus::Ready { version, .. } => (
      ctx.t_args(
        "loading_identity.update.panel.ready.title",
        [("version", version.clone())],
      ),
      ctx.t_args(
        "loading_identity.update.panel.ready.description",
        [("current", env!("CARGO_PKG_VERSION").to_owned())],
      ),
    ),
    StartupUpdateStatus::Staging { version } => (
      ctx.t_args(
        "loading_identity.update.panel.staging.title",
        [("version", version.clone())],
      ),
      ctx.t("loading_identity.update.panel.staging.description"),
    ),
    StartupUpdateStatus::Downloading { version, .. } => (
      ctx.t_args(
        "loading_identity.update.panel.downloading.title",
        [("version", version.clone())],
      ),
      ctx.t("loading_identity.update.panel.downloading.description"),
    ),
    StartupUpdateStatus::Checking | StartupUpdateStatus::Idle if update_pending => (
      ctx.t("loading_identity.update.panel.checking.title"),
      ctx.t("loading_identity.update.panel.checking.description"),
    ),
    _ => (
      ctx.t("loading_identity.update.panel.checking.title"),
      ctx.t("loading_identity.update.panel.checking.description"),
    ),
  }
}

fn update_step_states(
  status: &StartupUpdateStatus,
  update_pending: bool,
) -> (UpdateStepState, UpdateStepState, UpdateStepState) {
  match status {
    StartupUpdateStatus::Ready { .. } => (UpdateStepState::Done, UpdateStepState::Done, UpdateStepState::Active),
    StartupUpdateStatus::Staging { .. } => (UpdateStepState::Done, UpdateStepState::Done, UpdateStepState::Active),
    StartupUpdateStatus::Downloading { .. } => (UpdateStepState::Done, UpdateStepState::Active, UpdateStepState::Idle),
    StartupUpdateStatus::Checking | StartupUpdateStatus::Idle if update_pending => {
      (UpdateStepState::Active, UpdateStepState::Idle, UpdateStepState::Idle)
    }
    _ => (UpdateStepState::Idle, UpdateStepState::Idle, UpdateStepState::Idle),
  }
}

fn update_found_description(ctx: &Ctx, status: &StartupUpdateStatus, update_pending: bool) -> Arc<str> {
  match status {
    StartupUpdateStatus::Downloading { version, total, .. } => {
      let size = total
        .map(|total| format_bytes(ctx, total))
        .unwrap_or_else(|| ctx.t("loading_identity.update.size.unknown"));
      ctx.t_args(
        "loading_identity.update.step.found.description_with_size",
        [("version", version.clone()), ("size", size.to_string())],
      )
    }
    StartupUpdateStatus::Staging { version } | StartupUpdateStatus::Ready { version, .. } => ctx.t_args(
      "loading_identity.update.step.found.description",
      [("version", version.clone())],
    ),
    StartupUpdateStatus::Checking | StartupUpdateStatus::Idle if update_pending => {
      ctx.t("loading_identity.update.step.found.contacting")
    }
    _ => ctx.t("loading_identity.update.step.waiting_release_check"),
  }
}

fn download_progress_label(ctx: &Ctx, downloaded: u64, total: Option<u64>) -> Arc<str> {
  if let Some(total) = total.filter(|total| *total > 0) {
    let ratio = (downloaded as f32 / total as f32).clamp(0.0, 1.0);
    return ctx.t_args(
      "loading_identity.update.download_progress.with_total",
      [
        ("downloaded", format_bytes(ctx, downloaded).to_string()),
        ("total", format_bytes(ctx, total).to_string()),
        ("percent", percent_label(ratio)),
      ],
    );
  }

  ctx.t_args(
    "loading_identity.update.download_progress.without_total",
    [("downloaded", format_bytes(ctx, downloaded).to_string())],
  )
}

fn percent_label(ratio: f32) -> String {
  format!("{}%", percent_value(ratio))
}

fn percent_value(ratio: f32) -> String {
  format!("{:.0}", ratio.clamp(0.0, 1.0) * 100.0)
}

fn format_bytes(ctx: &Ctx, bytes: u64) -> Arc<str> {
  const MIB: f64 = 1024.0 * 1024.0;
  const KIB: f64 = 1024.0;
  if bytes >= 1024 * 1024 {
    ctx.t_args(
      "loading_identity.update.size.mib",
      [("value", format!("{:.1}", bytes as f64 / MIB))],
    )
  } else if bytes >= 1024 {
    ctx.t_args(
      "loading_identity.update.size.kib",
      [("value", format!("{:.1}", bytes as f64 / KIB))],
    )
  } else {
    ctx.t_args("loading_identity.update.size.bytes", [("value", bytes.to_string())])
  }
}
