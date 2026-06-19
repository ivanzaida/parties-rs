use std::{
  sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
  },
  time::Duration,
};

use lurq::{
  app::{
    component::{Component, ComponentInfo, DevtoolsFormatter, DevtoolsInspectable},
    ctx::{Ctx, Modal},
    events::KeyboardEvent,
  },
  components::{Column, Row, Stack, Text},
  core::{Signal, Store},
  layout::{Alignment, layout_kind::Justify},
  node::{BackgroundColor, CursorIcon, Element, Style, color::Color, dimension::Dimension},
  router::{RouterHandle, Routes},
};

use crate::{
  identity::LocalIdentity,
  routes::{
    ROUTE_CHOOSE_SERVER, ROUTE_CONNECT_SERVER, ROUTE_IDENTITY_SETUP, ROUTE_IMPORT_LEGACY_CONFIG,
    ROUTE_IMPORT_PRIVATE_KEY, ROUTE_LOADING, ROUTE_LOBBY, ROUTE_RESTORE_IDENTITY, ROUTE_SEED_PHRASE,
    ROUTE_SENTRY_REPORTS, ROUTE_SERVER_SETTINGS, ROUTE_SERVER_SETTINGS_CHANNELS, ROUTE_SERVER_SETTINGS_MEMBERS,
    ROUTE_SERVER_SETTINGS_ROLES, ROUTE_SETTINGS, ROUTE_SETTINGS_AUDIO, ROUTE_SETTINGS_IDENTITY,
    ROUTE_SETTINGS_NOTIFICATIONS, ROUTE_SETTINGS_SERVERS, ROUTE_SETTINGS_STREAM, ROUTE_TOFU_WARNING,
  },
  services::{
    global_hotkeys::GlobalVoiceHotkeys,
    hotkeys, logger,
    updater::{StartupUpdateStatus, restart_into_update, run_startup_update_check},
    voice_controls::{VoiceControlAction, apply_voice_control},
  },
  session::ServerSession,
  storage::{
    AppAudioSettings, AppDebugModeEnabled, AppDisplayName, AppHotkeySettings, AppLocale, AppSentryReportsEnabled,
    AppSettings, AppSettingsUpdater, AppStreamSettings, AppVideoSettings, Storage, StoredServer, UserAudioPreferences,
  },
  theme,
  ui::{
    app_chrome::{FrameRateSignal, modal_layer, wrap_window_chrome},
    common::lucide_icon::{LucideIcon, LucideIconProps},
    connect_server::ConnectServerScreen,
    identity_seed::IdentitySeedScreen,
    identity_setup::IdentitySetupScreen,
    import_identity::ImportIdentityScreen,
    import_legacy_config::ImportLegacyConfigScreen,
    loading_identity::{LoadingIdentityScreen, LoadingIdentityScreenProps},
    lobby::LobbyScreen,
    restore_identity::RestoreIdentityScreen,
    sentry_reports::SentryReportsScreen,
    server_settings::{ServerSettingsPage, ServerSettingsScreen},
    servers::SavedServersScreen,
    settings::{
      SettingsAudioScreen, SettingsIdentityScreen, SettingsNotificationsScreen, SettingsOverviewScreen, SettingsPage,
      SettingsPopup, SettingsPopupHandle, SettingsSavedServersScreen, SettingsStreamScreen,
    },
    tofu_warning::TofuWarningScreen,
  },
};

const UPDATE_POLL_INTERVAL: Duration = Duration::from_secs(60);
const UPDATE_PILL_MARGIN: f32 = 16.0;
const UPDATE_PILL_TOP_GAP: f32 = 12.0;
const UPDATE_PILL_HEIGHT: f32 = 40.0;

pub struct App {
  router: RouterHandle,
  session: ServerSession,
  storage: Signal<Option<Storage>>,
  settings: Store<AppSettings>,
  display_name: Store<AppDisplayName>,
  debug_mode_enabled: Store<AppDebugModeEnabled>,
  sentry_reports_enabled: Store<AppSentryReportsEnabled>,
  locale: Store<AppLocale>,
  hotkey_settings: Store<AppHotkeySettings>,
  audio_settings: Store<AppAudioSettings>,
  stream_settings: Store<AppStreamSettings>,
  video_settings: Store<AppVideoSettings>,
  servers: Store<Vec<StoredServer>>,
  identity: Store<Option<LocalIdentity>>,
  user_audio_preferences: Store<UserAudioPreferences>,
  settings_open: Signal<bool>,
  settings_page: Signal<SettingsPage>,
  active_toggle_hotkeys: Store<Vec<String>>,
  update_status: Signal<StartupUpdateStatus>,
  frame_rate: FrameRateSignal,
  startup_full_screen: bool,
  startup_full_screen_applied: Signal<bool>,
  last_full_screen: Signal<bool>,
  global_hotkeys: GlobalVoiceHotkeys,
}

#[derive(Clone)]
pub struct AppProps {
  pub tokio: tokio::runtime::Handle,
  pub startup_storage: Option<Storage>,
  pub startup_error: Option<String>,
  pub session: ServerSession,
  pub frame_rate: FrameRateSignal,
  pub startup_full_screen: bool,
}

impl PartialEq for AppProps {
  fn eq(&self, _other: &Self) -> bool {
    true
  }
}

impl DevtoolsInspectable for AppProps {
  fn inspect(&self, formatter: &mut DevtoolsFormatter<'_>) {
    formatter.buffer_mut().push(ComponentInfo::new(
      "tokio",
      std::any::type_name::<tokio::runtime::Handle>(),
    ));
  }
}

impl Component for App {
  type Props = AppProps;

  fn create(ctx: &mut Ctx) -> Self {
    let props = ctx.props::<Self::Props>().clone();
    let tokio = props.tokio.clone();
    let storage = ctx.signal(props.startup_storage.clone());
    let startup_settings = load_settings_from_storage(props.startup_storage.as_ref());
    let startup_servers = load_servers_from_storage(props.startup_storage.as_ref());
    let startup_identity = load_identity_from_storage(props.startup_storage.as_ref());
    let startup_user_audio_preferences = load_user_audio_preferences_from_storage(props.startup_storage.as_ref());
    let settings = ctx.store(startup_settings);
    let display_name = ctx.store(display_name_setting(&settings.get()));
    let debug_mode_enabled = ctx.store(debug_mode_setting(&settings.get()));
    let sentry_reports_enabled = ctx.store(sentry_reports_setting(&settings.get()));
    let locale = ctx.store(locale_setting(&settings.get()));
    let hotkey_settings = ctx.store(hotkey_settings(&settings.get()));
    let audio_settings = ctx.store(audio_settings(&settings.get()));
    let stream_settings = ctx.store(stream_settings(&settings.get()));
    let video_settings = ctx.store(video_settings(&settings.get()));
    let servers = ctx.store(startup_servers);
    let identity = ctx.store(startup_identity);
    let user_audio_preferences = ctx.store(startup_user_audio_preferences);
    let i18n = ctx.i18n().clone();
    apply_settings_locale(&settings.get(), &i18n);
    ctx.watch(&storage, {
      let settings = settings.clone();
      let display_name = display_name.clone();
      let debug_mode_enabled = debug_mode_enabled.clone();
      let sentry_reports_enabled = sentry_reports_enabled.clone();
      let locale = locale.clone();
      let hotkey_settings = hotkey_settings.clone();
      let audio_settings = audio_settings.clone();
      let stream_settings = stream_settings.clone();
      let video_settings = video_settings.clone();
      let servers = servers.clone();
      let identity = identity.clone();
      let user_audio_preferences = user_audio_preferences.clone();
      let i18n = i18n.clone();
      move |storage| {
        let next_settings = load_settings_from_storage(storage.as_ref());
        settings.set(next_settings.clone());
        sync_focused_settings(
          &next_settings,
          &display_name,
          &debug_mode_enabled,
          &sentry_reports_enabled,
          &locale,
          &hotkey_settings,
          &audio_settings,
          &stream_settings,
          &video_settings,
        );
        servers.set(load_servers_from_storage(storage.as_ref()));
        identity.set(load_identity_from_storage(storage.as_ref()));
        user_audio_preferences.set(load_user_audio_preferences_from_storage(storage.as_ref()));
        apply_settings_locale(&next_settings, &i18n);
      }
    });
    let update_status = ctx.signal(StartupUpdateStatus::Idle);
    let loading_storage = storage.clone();
    let startup_error = props.startup_error.clone();
    let loading_update_status = update_status.clone();
    let router = ctx.router(
      Routes::new()
        .route(ROUTE_LOADING, move |ctx| {
          ctx.mount::<LoadingIdentityScreen>(LoadingIdentityScreenProps {
            storage: loading_storage.clone(),
            startup_error: startup_error.clone(),
            update_status: loading_update_status.clone(),
          })
        })
        .route(ROUTE_SENTRY_REPORTS, |ctx| ctx.mount::<SentryReportsScreen>(()))
        .route(ROUTE_IDENTITY_SETUP, |ctx| ctx.mount::<IdentitySetupScreen>(()))
        .route(ROUTE_SEED_PHRASE, |ctx| ctx.mount::<IdentitySeedScreen>(()))
        .route(ROUTE_IMPORT_PRIVATE_KEY, |ctx| ctx.mount::<ImportIdentityScreen>(()))
        .route(ROUTE_IMPORT_LEGACY_CONFIG, |ctx| {
          ctx.mount::<ImportLegacyConfigScreen>(())
        })
        .route(ROUTE_RESTORE_IDENTITY, |ctx| ctx.mount::<RestoreIdentityScreen>(()))
        .route(ROUTE_CHOOSE_SERVER, |ctx| ctx.mount::<SavedServersScreen>(()))
        .route(ROUTE_CONNECT_SERVER, |ctx| ctx.mount::<ConnectServerScreen>(()))
        .route(ROUTE_TOFU_WARNING, |ctx| ctx.mount::<TofuWarningScreen>(()))
        .route(ROUTE_LOBBY, |ctx| ctx.mount::<LobbyScreen>(()))
        .route(ROUTE_SERVER_SETTINGS, |ctx| {
          ctx.mount::<ServerSettingsScreen>(ServerSettingsPage::Server)
        })
        .route(ROUTE_SERVER_SETTINGS_CHANNELS, |ctx| {
          ctx.mount::<ServerSettingsScreen>(ServerSettingsPage::Channels)
        })
        .route(ROUTE_SERVER_SETTINGS_MEMBERS, |ctx| {
          ctx.mount::<ServerSettingsScreen>(ServerSettingsPage::Members)
        })
        .route(ROUTE_SERVER_SETTINGS_ROLES, |ctx| {
          ctx.mount::<ServerSettingsScreen>(ServerSettingsPage::Roles)
        })
        .route(ROUTE_SETTINGS, |ctx| ctx.mount::<SettingsOverviewScreen>(()))
        .route(ROUTE_SETTINGS_IDENTITY, |ctx| ctx.mount::<SettingsIdentityScreen>(()))
        .route(ROUTE_SETTINGS_SERVERS, |ctx| {
          ctx.mount::<SettingsSavedServersScreen>(())
        })
        .route(ROUTE_SETTINGS_AUDIO, |ctx| ctx.mount::<SettingsAudioScreen>(()))
        .route(ROUTE_SETTINGS_NOTIFICATIONS, |ctx| {
          ctx.mount::<SettingsNotificationsScreen>(())
        })
        .route(ROUTE_SETTINGS_STREAM, |ctx| ctx.mount::<SettingsStreamScreen>(()))
        .fallback(|ctx| ctx.mount::<IdentitySetupScreen>(())),
    );
    router.replace(ROUTE_LOADING);
    let session = props.session.clone();
    let global_hotkeys = GlobalVoiceHotkeys::new(session.clone(), tokio);
    let poll_global_hotkeys = global_hotkeys.clone();
    let interval = ctx.create_interval(Duration::from_millis(16), move || {
      poll_global_hotkeys.poll_events();
    });
    interval.start();
    let update_poll_in_flight = Arc::new(AtomicBool::new(false));
    let initial_update_status = update_status.clone();
    let initial_update_tokio = props.tokio.clone();
    let initial_update_gate = update_poll_in_flight.clone();
    if !initial_update_gate.swap(true, Ordering::AcqRel) {
      initial_update_tokio.spawn(async move {
        let _ = run_startup_update_check(initial_update_status).await;
        initial_update_gate.store(false, Ordering::Release);
      });
    }
    let update_poll_status = update_status.clone();
    let update_poll_tokio = props.tokio.clone();
    let update_poll_gate = update_poll_in_flight.clone();
    let update_interval = ctx.create_interval(UPDATE_POLL_INTERVAL, move || {
      if update_poll_gate.swap(true, Ordering::AcqRel) {
        return;
      }

      let status = update_poll_status.get_untracked();
      if update_status_blocks_poll(&status) {
        update_poll_gate.store(false, Ordering::Release);
        return;
      }

      let status = update_poll_status.clone();
      let gate = update_poll_gate.clone();
      update_poll_tokio.spawn(async move {
        let _ = run_startup_update_check(status).await;
        gate.store(false, Ordering::Release);
      });
    });
    update_interval.start();
    Self {
      router,
      session,
      storage,
      settings,
      display_name,
      debug_mode_enabled,
      sentry_reports_enabled,
      locale,
      hotkey_settings,
      audio_settings,
      stream_settings,
      video_settings,
      servers,
      identity,
      user_audio_preferences,
      settings_open: ctx.signal(false),
      settings_page: ctx.signal(SettingsPage::Overview),
      active_toggle_hotkeys: ctx.store(Vec::new()),
      update_status,
      frame_rate: props.frame_rate.clone(),
      startup_full_screen: props.startup_full_screen,
      startup_full_screen_applied: ctx.signal(false),
      last_full_screen: ctx.signal(props.startup_full_screen),
      global_hotkeys,
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let storage = self.storage.get();
    ctx.provide(self.session.clone());
    ctx.provide(self.global_hotkeys.clone());
    ctx.provide(self.settings.clone());
    ctx.provide(AppSettingsUpdater::new(self.settings.clone(), storage.clone()));
    ctx.provide(self.display_name.clone());
    ctx.provide(self.debug_mode_enabled.clone());
    ctx.provide(self.sentry_reports_enabled.clone());
    ctx.provide(self.locale.clone());
    ctx.provide(self.hotkey_settings.clone());
    ctx.provide(self.audio_settings.clone());
    ctx.provide(self.stream_settings.clone());
    ctx.provide(self.video_settings.clone());
    ctx.provide(self.servers.clone());
    ctx.provide(self.identity.clone());
    ctx.provide(self.user_audio_preferences.clone());
    let settings_popup = SettingsPopupHandle::new(self.settings_open.clone(), self.settings_page.clone());
    ctx.provide(settings_popup.clone());
    if let Some(storage) = storage.clone() {
      ctx.provide(storage);
    }
    if self.identity.with(Option::is_none) {
      let path = self.router.path().get();
      if route_requires_identity(path.as_ref()) {
        if self.settings_open.get_untracked() {
          self.settings_open.set(false);
        }
        if self.session.info().is_some() {
          self.session.disconnect();
        }
        self.router.replace(ROUTE_IDENTITY_SETUP);
      }
    }
    let startup_window = ctx.window();
    if self.startup_full_screen && !self.startup_full_screen_applied.get_untracked() {
      self.startup_full_screen_applied.set(true);
      startup_window.set_full_screen(true);
    } else {
      self.sync_window_full_screen(ctx, startup_window.is_full_screen);
    }
    let settings = self.settings.get();
    sync_focused_settings(
      &settings,
      &self.display_name,
      &self.debug_mode_enabled,
      &self.sentry_reports_enabled,
      &self.locale,
      &self.hotkey_settings,
      &self.audio_settings,
      &self.stream_settings,
      &self.video_settings,
    );
    apply_settings_locale(&settings, ctx.i18n());
    logger::apply_sentry_reports_enabled(settings.sentry_reports_enabled);
    self.session.set_notification_audio_settings(&audio_settings(&settings));
    self
      .session
      .set_video_hardware_decoding(settings.video_hardware_decoding);
    let voice_hotkeys = hotkey_settings(&settings);
    let mute_hotkey = voice_hotkeys.toggle_mute.clone();
    let deafen_hotkey = voice_hotkeys.toggle_deafen.clone();
    let push_to_talk_enabled = settings.push_to_talk;
    let push_to_talk_hotkey = voice_hotkeys.push_to_talk.clone();
    let app_focused = ctx.window().is_focused;
    let settings_active = self.settings_open.get() || self.router.path().get().starts_with(ROUTE_SETTINGS);
    let local_hotkeys_enabled = app_focused && !settings_active;
    let global_hotkeys_enabled = !app_focused;
    let global_mouse_hotkeys_enabled = !settings_active;
    self.global_hotkeys.update_settings(
      Some(&voice_hotkeys),
      push_to_talk_enabled,
      global_hotkeys_enabled,
      global_mouse_hotkeys_enabled,
    );
    let voice_hotkey = ctx.future_action({
      let session = self.session.clone();
      let no_connected_server = ctx.t("lobby.error.no_connected_server").to_string();
      move |action: VoiceControlAction| {
        let session = session.clone();
        let no_connected_server = no_connected_server.clone();
        async move { apply_voice_control(session, action, no_connected_server).await }
      }
    });

    let mut content = Column::new()
      .width(Dimension::Pct(100.0))
      .height(Dimension::Pct(100.0))
      .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceBase))
      .align_items(Alignment::Stretch)
      .justify(Justify::Start)
      .clip();

    content = content.child(
      Column::new()
        .width(Dimension::Pct(100.0))
        .flex(1.0)
        .clip()
        .child(lurq::components::Router::mount(ctx, self.router.clone())),
    );

    let mut root = Stack::new()
      .width(Dimension::Pct(100.0))
      .height(Dimension::Pct(100.0))
      .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceBase))
      .clip()
      .child(content);

    let settings_open = self.settings_open.clone();
    if settings_open.get() {
      let close_settings = settings_popup.clone();
      ctx.provide(settings_popup.clone());
      let popup = ctx.mount::<SettingsPopup>(());
      let settings_layer = modal_layer(ctx, popup).on_key_down(move |event: KeyboardEvent| {
        if hotkeys::is_cancel_key(&event) {
          close_settings.close();
        }
      });
      root = root.child(Modal::new(settings_layer).open(settings_open));
    }

    if local_hotkeys_enabled {
      let voice_hotkey = voice_hotkey.clone();
      let ptt_session = self.session.clone();
      let ptt_down_hotkey = push_to_talk_hotkey.clone();
      let mute_down_hotkey = mute_hotkey.clone();
      let deafen_down_hotkey = deafen_hotkey.clone();
      let active_toggle_hotkeys = self.active_toggle_hotkeys.clone();
      root = root.on_key_down(move |event: KeyboardEvent| {
        if push_to_talk_enabled && hotkeys::event_matches_hotkey(&ptt_down_hotkey, &event) {
          ptt_session.set_push_to_talk_active(true);
        } else if hotkeys::event_matches_hotkey(&mute_down_hotkey, &event) {
          if activate_toggle_hotkey(&active_toggle_hotkeys, &mute_down_hotkey) {
            voice_hotkey.run(VoiceControlAction::ToggleMute);
          }
        } else if hotkeys::event_matches_hotkey(&deafen_down_hotkey, &event) {
          if activate_toggle_hotkey(&active_toggle_hotkeys, &deafen_down_hotkey) {
            voice_hotkey.run(VoiceControlAction::ToggleDeafen);
          }
        }
      });

      let ptt_session = self.session.clone();
      let active_toggle_hotkeys = self.active_toggle_hotkeys.clone();
      root = root.on_key_up(move |event: KeyboardEvent| {
        if push_to_talk_enabled && hotkeys::event_releases_hotkey(&push_to_talk_hotkey, &event) {
          ptt_session.set_push_to_talk_active(false);
        }
        release_toggle_hotkey(&active_toggle_hotkeys, &mute_hotkey, &event);
        release_toggle_hotkey(&active_toggle_hotkeys, &deafen_hotkey, &event);
      });
    }

    let update_status = self.update_status.get();
    if let Some(pill) = self.global_update_pill(ctx, &update_status) {
      root = root.child(pill);
    }

    wrap_window_chrome(ctx, root, self.frame_rate.clone(), self.session.clone())
  }
}

impl App {
  fn global_update_pill(&self, ctx: &mut Ctx, status: &StartupUpdateStatus) -> Option<Element> {
    let width = match status {
      StartupUpdateStatus::Downloading { .. } => 216.0,
      StartupUpdateStatus::Staging { .. } => 204.0,
      StartupUpdateStatus::Ready { .. } => 224.0,
      _ => return None,
    };
    let window = ctx.window();
    let x = (window.logical_width() - width - UPDATE_PILL_MARGIN).max(UPDATE_PILL_MARGIN);
    let y = UPDATE_PILL_TOP_GAP;
    let title = update_pill_title(ctx, status);
    let detail = update_pill_detail(ctx, status);

    let mut pill = Row::new()
      .absolute(x, y, width, UPDATE_PILL_HEIGHT)
      .align_items(Alignment::Center)
      .spacing(9.0)
      .padding_horizontal(12.0)
      .rounded(8.0)
      .background(BackgroundColor::Palette(update_pill_background(status)))
      .border_inside(1.0, BackgroundColor::Color(update_pill_border_color(status)))
      .child(
        Row::new()
          .width(24.0)
          .height(24.0)
          .align_items(Alignment::Center)
          .justify(Justify::Center)
          .rounded(12.0)
          .background(BackgroundColor::Color(update_pill_icon_background(status)))
          .child(ctx.mount::<LucideIcon>(LucideIconProps {
            icon: update_pill_icon(status),
            size: 14.0,
            color: update_pill_icon_color(status),
          })),
      )
      .child(
        Column::new()
          .flex(1.0)
          .spacing(1.0)
          .child(
            Text::new(&title)
              .variant(theme::TypographyStyle::Caption)
              .color(update_pill_text_color(status))
              .nowrap(),
          )
          .child(
            Text::new(&detail)
              .variant(theme::TypographyStyle::Label)
              .color(theme::PaletteColor::TextMuted)
              .nowrap(),
          ),
      );

    if let StartupUpdateStatus::Ready { staged_executable, .. } = status {
      let staged_executable = staged_executable.clone();
      let update_status = self.update_status.clone();
      let storage = self.storage.get_untracked();
      let session = self.session.clone();
      pill = pill
        .cursor(CursorIcon::Pointer)
        .hovered_style(Style::new().background(BackgroundColor::Palette(theme::PaletteColor::SurfaceInput)))
        .active_style(Style::new().background(BackgroundColor::Palette(theme::PaletteColor::SurfaceInput)))
        .on_click(move |_| {
          if let Err(error) = restart_into_update(&staged_executable, storage.as_ref(), Some(&session)) {
            update_status.set(StartupUpdateStatus::Failed(error));
          }
        });
    }

    Some(pill.into())
  }
}

impl App {
  fn sync_window_full_screen(&self, ctx: &Ctx, full_screen: bool) {
    if self.last_full_screen.get_untracked() == full_screen {
      return;
    }
    self.last_full_screen.set(full_screen);
    if let Err(error) = ctx.set_persistent_value("window.full_screen", full_screen) {
      tracing::debug!(target: "window::state", "failed to save full screen state: {error}");
    }
  }
}

fn load_settings_from_storage(storage: Option<&Storage>) -> AppSettings {
  storage
    .and_then(|storage| storage.load_settings().ok())
    .unwrap_or_default()
}

fn load_servers_from_storage(storage: Option<&Storage>) -> Vec<StoredServer> {
  storage
    .and_then(|storage| storage.load_servers().ok())
    .unwrap_or_default()
}

fn load_identity_from_storage(storage: Option<&Storage>) -> Option<LocalIdentity> {
  storage.and_then(|storage| storage.load_identity().ok()).flatten()
}

fn load_user_audio_preferences_from_storage(storage: Option<&Storage>) -> UserAudioPreferences {
  storage
    .and_then(|storage| storage.load_user_audio_preferences().ok())
    .unwrap_or_default()
}

fn display_name_setting(settings: &AppSettings) -> AppDisplayName {
  AppDisplayName {
    value: settings.display_name.clone(),
  }
}

fn debug_mode_setting(settings: &AppSettings) -> AppDebugModeEnabled {
  AppDebugModeEnabled {
    value: settings.debug_mode_enabled,
  }
}

fn sentry_reports_setting(settings: &AppSettings) -> AppSentryReportsEnabled {
  AppSentryReportsEnabled {
    value: settings.sentry_reports_enabled,
  }
}

fn locale_setting(settings: &AppSettings) -> AppLocale {
  AppLocale {
    value: settings.locale.clone(),
  }
}

fn hotkey_settings(settings: &AppSettings) -> AppHotkeySettings {
  AppHotkeySettings {
    push_to_talk: settings.hotkey_push_to_talk.clone(),
    toggle_mute: settings.hotkey_toggle_mute.clone(),
    toggle_deafen: settings.hotkey_toggle_deafen.clone(),
  }
}

fn audio_settings(settings: &AppSettings) -> AppAudioSettings {
  AppAudioSettings::from(settings)
}

fn stream_settings(settings: &AppSettings) -> AppStreamSettings {
  AppStreamSettings {
    video_codec: settings.video_codec.clone(),
    video_scale_percent: settings.video_scale_percent,
    video_fps: settings.video_fps,
    video_bitrate_mbps: settings.video_bitrate_mbps,
  }
}

fn video_settings(settings: &AppSettings) -> AppVideoSettings {
  AppVideoSettings::from(settings)
}

fn sync_focused_settings(
  settings: &AppSettings,
  display_name: &Store<AppDisplayName>,
  debug_mode_enabled: &Store<AppDebugModeEnabled>,
  sentry_reports_enabled: &Store<AppSentryReportsEnabled>,
  locale: &Store<AppLocale>,
  hotkey_settings_store: &Store<AppHotkeySettings>,
  audio_settings_store: &Store<AppAudioSettings>,
  stream_settings_store: &Store<AppStreamSettings>,
  video_settings_store: &Store<AppVideoSettings>,
) {
  let next_display_name = display_name_setting(settings);
  if display_name.with(|current| current != &next_display_name) {
    display_name.set(next_display_name);
  }

  let next_debug_mode = debug_mode_setting(settings);
  if debug_mode_enabled.with(|current| current != &next_debug_mode) {
    debug_mode_enabled.set(next_debug_mode);
  }

  let next_sentry_reports = sentry_reports_setting(settings);
  if sentry_reports_enabled.with(|current| current != &next_sentry_reports) {
    sentry_reports_enabled.set(next_sentry_reports);
  }

  let next_locale = locale_setting(settings);
  if locale.with(|current| current != &next_locale) {
    locale.set(next_locale);
  }

  let next_hotkey_settings = hotkey_settings(settings);
  if hotkey_settings_store.with(|current| current != &next_hotkey_settings) {
    hotkey_settings_store.set(next_hotkey_settings);
  }

  let next_audio_settings = audio_settings(settings);
  if audio_settings_store.with(|current| current != &next_audio_settings) {
    audio_settings_store.set(next_audio_settings);
  }

  let next_stream_settings = stream_settings(settings);
  if stream_settings_store.with(|current| current != &next_stream_settings) {
    stream_settings_store.set(next_stream_settings);
  }

  let next_video_settings = video_settings(settings);
  if video_settings_store.with(|current| current != &next_video_settings) {
    video_settings_store.set(next_video_settings);
  }
}

fn apply_settings_locale(settings: &AppSettings, i18n: &lurq::app::i18n::I18n) {
  i18n.set_locale(settings.locale.clone());
}

fn update_status_blocks_poll(status: &StartupUpdateStatus) -> bool {
  matches!(
    status,
    StartupUpdateStatus::Checking
      | StartupUpdateStatus::Downloading { .. }
      | StartupUpdateStatus::Staging { .. }
      | StartupUpdateStatus::Ready { .. }
  )
}

fn update_pill_title(ctx: &Ctx, status: &StartupUpdateStatus) -> Arc<str> {
  match status {
    StartupUpdateStatus::Downloading { .. } | StartupUpdateStatus::Staging { .. } => ctx.t("app.update_pill.available"),
    StartupUpdateStatus::Ready { .. } => ctx.t("app.update_pill.restart"),
    _ => Arc::from(""),
  }
}

fn update_pill_detail(ctx: &Ctx, status: &StartupUpdateStatus) -> Arc<str> {
  match status {
    StartupUpdateStatus::Downloading {
      version,
      downloaded,
      total,
    } => {
      if let Some(total) = total.filter(|total| *total > 0) {
        let percent = ((*downloaded as f64 / total as f64) * 100.0).clamp(0.0, 100.0);
        ctx.t_args(
          "app.update_pill.downloading_percent",
          [("version", version.clone()), ("percent", format!("{percent:.0}"))],
        )
      } else {
        ctx.t_args("app.update_pill.downloading", [("version", version.clone())])
      }
    }
    StartupUpdateStatus::Staging { version } => ctx.t_args("app.update_pill.preparing", [("version", version.clone())]),
    StartupUpdateStatus::Ready { version, .. } => ctx.t_args("app.update_pill.launch", [("version", version.clone())]),
    _ => Arc::from(""),
  }
}

fn update_pill_icon(status: &StartupUpdateStatus) -> &'static str {
  match status {
    StartupUpdateStatus::Ready { .. } => "rotate-cw",
    StartupUpdateStatus::Staging { .. } => "loader",
    _ => "refresh-cw",
  }
}

fn update_pill_background(status: &StartupUpdateStatus) -> theme::PaletteColor {
  match status {
    StartupUpdateStatus::Ready { .. } => theme::PaletteColor::SuccessMuted,
    _ => theme::PaletteColor::InfoMuted,
  }
}

fn update_pill_border_color(status: &StartupUpdateStatus) -> Color {
  match status {
    StartupUpdateStatus::Ready { .. } => theme::palette().success.with_opacity(0.5),
    _ => theme::palette().info.with_opacity(0.5),
  }
}

fn update_pill_icon_background(status: &StartupUpdateStatus) -> Color {
  match status {
    StartupUpdateStatus::Ready { .. } => theme::palette().success.with_opacity(0.16),
    _ => theme::palette().info.with_opacity(0.16),
  }
}

fn update_pill_icon_color(status: &StartupUpdateStatus) -> Color {
  match status {
    StartupUpdateStatus::Ready { .. } => theme::palette().success,
    _ => theme::palette().info,
  }
}

fn update_pill_text_color(status: &StartupUpdateStatus) -> theme::PaletteColor {
  match status {
    StartupUpdateStatus::Ready { .. } => theme::PaletteColor::Success,
    _ => theme::PaletteColor::Info,
  }
}

fn activate_toggle_hotkey(active_hotkeys: &Store<Vec<String>>, hotkey: &str) -> bool {
  let key = hotkey_key(hotkey);
  if key.is_empty() {
    return false;
  }

  let mut active = active_hotkeys.get();
  if active.iter().any(|existing| existing == &key) {
    return false;
  }

  active.push(key);
  active_hotkeys.set(active);
  true
}

fn release_toggle_hotkey(active_hotkeys: &Store<Vec<String>>, hotkey: &str, event: &lurq::app::events::KeyboardEvent) {
  if !hotkeys::event_releases_hotkey(hotkey, event) {
    return;
  }

  let key = hotkey_key(hotkey);
  active_hotkeys.set(
    active_hotkeys
      .get()
      .into_iter()
      .filter(|existing| existing != &key)
      .collect(),
  );
}

fn hotkey_key(hotkey: &str) -> String {
  hotkey.trim().to_ascii_lowercase()
}

fn route_requires_identity(path: &str) -> bool {
  !(path == ROUTE_LOADING
    || path == ROUTE_SENTRY_REPORTS
    || path == ROUTE_IDENTITY_SETUP
    || path == ROUTE_SEED_PHRASE
    || path == ROUTE_IMPORT_PRIVATE_KEY
    || path == ROUTE_IMPORT_LEGACY_CONFIG
    || path == ROUTE_RESTORE_IDENTITY)
}

#[cfg(test)]
#[path = "../tests/unit/app.rs"]
mod tests;
