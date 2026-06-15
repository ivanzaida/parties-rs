use std::{
  sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
  },
  time::Duration,
};

use lurq::{
  app::{
    component::{Component, ComponentInfo, DevtoolsInspectable},
    ctx::{Ctx, Modal, Root},
  },
  components::{Column, Row, Stack, Text},
  core::Signal,
  layout::{Alignment, layout_kind::Justify},
  node::{BackgroundColor, CursorIcon, Element, Style, color::Color, dimension::Dimension},
  router::{RouterHandle, Routes},
};

use crate::{
  routes::{
    ROUTE_CHOOSE_SERVER, ROUTE_CONNECT_SERVER, ROUTE_IDENTITY_SETUP, ROUTE_IMPORT_PRIVATE_KEY, ROUTE_LOADING,
    ROUTE_LOBBY, ROUTE_RESTORE_IDENTITY, ROUTE_SEED_PHRASE, ROUTE_SERVER_SETTINGS, ROUTE_SERVER_SETTINGS_CHANNELS,
    ROUTE_SERVER_SETTINGS_MEMBERS, ROUTE_SERVER_SETTINGS_ROLES, ROUTE_SETTINGS, ROUTE_SETTINGS_AUDIO,
    ROUTE_SETTINGS_IDENTITY, ROUTE_SETTINGS_NOTIFICATIONS, ROUTE_SETTINGS_SERVERS, ROUTE_SETTINGS_STREAM,
  },
  services::{
    global_hotkeys::GlobalVoiceHotkeys,
    hotkeys,
    updater::{StartupUpdateStatus, restart_into_update, run_startup_update_check},
    voice_controls::{VoiceControlAction, apply_voice_control},
  },
  session::ServerSession,
  storage::{Storage, WindowState},
  theme,
  ui::{
    app_chrome::{
      AppChrome, CHROME_HEIGHT, CUSTOM_MACOS_CHROME, CUSTOM_WINDOW_CHROME, modal_layer, window_affordance_layers,
    },
    common::lucide_icon::{LucideIcon, LucideIconProps},
    connect_server::ConnectServerScreen,
    identity_seed::IdentitySeedScreen,
    identity_setup::IdentitySetupScreen,
    import_identity::ImportIdentityScreen,
    loading_identity::{LoadingIdentityScreen, LoadingIdentityScreenProps},
    lobby::LobbyScreen,
    restore_identity::RestoreIdentityScreen,
    server_settings::{ServerSettingsPage, ServerSettingsScreen},
    servers::SavedServersScreen,
    settings::{
      SettingsAudioScreen, SettingsIdentityScreen, SettingsNotificationsScreen, SettingsOverviewScreen, SettingsPage,
      SettingsPopup, SettingsPopupHandle, SettingsSavedServersScreen, SettingsStreamScreen,
    },
  },
};

const DEVTOOLS_HOTKEY: &str = "Ctrl+Shift+F12";
const MACOS_WINDOW_CORNER_RADIUS: f32 = 10.0;
const UPDATE_POLL_INTERVAL: Duration = Duration::from_secs(60);
const UPDATE_PILL_MARGIN: f32 = 16.0;
const UPDATE_PILL_TOP_GAP: f32 = 12.0;
const UPDATE_PILL_HEIGHT: f32 = 40.0;

pub struct App {
  router: RouterHandle,
  session: ServerSession,
  storage: Signal<Option<Storage>>,
  settings_open: Signal<bool>,
  settings_page: Signal<SettingsPage>,
  active_toggle_hotkeys: Signal<Vec<String>>,
  update_status: Signal<StartupUpdateStatus>,
  startup_full_screen: bool,
  startup_full_screen_applied: Signal<bool>,
  window_state_tracker: Option<WindowStateTracker>,
  global_hotkeys: GlobalVoiceHotkeys,
}

#[derive(Clone)]
pub struct WindowStateTracker {
  pub current: Arc<Mutex<WindowState>>,
  pub last_saved: Arc<Mutex<Option<WindowState>>>,
}

#[derive(Clone)]
pub struct AppProps {
  pub tokio: tokio::runtime::Handle,
  pub startup_storage: Option<Storage>,
  pub startup_error: Option<String>,
  pub session: ServerSession,
  pub startup_full_screen: bool,
  pub window_state_tracker: Option<WindowStateTracker>,
}

impl PartialEq for AppProps {
  fn eq(&self, _other: &Self) -> bool {
    true
  }
}

impl DevtoolsInspectable for AppProps {
  fn write_info(&self, buffer: &mut Vec<ComponentInfo>) {
    buffer.push(ComponentInfo::new(
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
    let i18n = ctx.i18n().clone();
    apply_storage_locale(&storage.get_untracked(), &i18n);
    ctx.watch(&storage, move |storage| {
      apply_storage_locale(storage, &i18n);
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
        .route(ROUTE_IDENTITY_SETUP, |ctx| ctx.mount::<IdentitySetupScreen>(()))
        .route(ROUTE_SEED_PHRASE, |ctx| ctx.mount::<IdentitySeedScreen>(()))
        .route(ROUTE_IMPORT_PRIVATE_KEY, |ctx| ctx.mount::<ImportIdentityScreen>(()))
        .route(ROUTE_RESTORE_IDENTITY, |ctx| ctx.mount::<RestoreIdentityScreen>(()))
        .route(ROUTE_CHOOSE_SERVER, |ctx| ctx.mount::<SavedServersScreen>(()))
        .route(ROUTE_CONNECT_SERVER, |ctx| ctx.mount::<ConnectServerScreen>(()))
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
      settings_open: ctx.signal(false),
      settings_page: ctx.signal(SettingsPage::Overview),
      active_toggle_hotkeys: ctx.signal(Vec::new()),
      update_status,
      startup_full_screen: props.startup_full_screen,
      startup_full_screen_applied: ctx.signal(false),
      window_state_tracker: props.window_state_tracker.clone(),
      global_hotkeys,
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    ctx.provide(self.session.clone());
    ctx.provide(self.global_hotkeys.clone());
    let settings_popup = SettingsPopupHandle::new(self.settings_open.clone(), self.settings_page.clone());
    ctx.provide(settings_popup.clone());
    let storage = self.storage.get();
    if let Some(storage) = storage.clone() {
      ctx.provide(storage);
    }
    let startup_window = ctx.window();
    if self.startup_full_screen && !self.startup_full_screen_applied.get_untracked() {
      self.startup_full_screen_applied.set(true);
      startup_window.set_full_screen(true);
    } else {
      self.sync_window_full_screen(storage.as_ref(), startup_window.is_full_screen);
    }
    let settings = storage.as_ref().and_then(|storage| storage.load_settings().ok());
    if let Some(settings) = settings.as_ref() {
      self.session.set_notification_audio_settings(settings);
      self
        .session
        .set_video_hardware_decoding(settings.video_hardware_decoding);
    }
    let mute_hotkey = settings
      .as_ref()
      .map(|settings| settings.hotkey_toggle_mute.clone())
      .unwrap_or_default();
    let deafen_hotkey = settings
      .as_ref()
      .map(|settings| settings.hotkey_toggle_deafen.clone())
      .unwrap_or_default();
    let push_to_talk_enabled = settings.as_ref().is_some_and(|settings| settings.push_to_talk);
    let push_to_talk_hotkey = settings
      .as_ref()
      .map(|settings| settings.hotkey_push_to_talk.clone())
      .unwrap_or_default();
    let app_focused = ctx.window().is_focused;
    let settings_active = self.settings_open.get() || self.router.path().get().starts_with(ROUTE_SETTINGS);
    let local_hotkeys_enabled = app_focused && !settings_active;
    let global_hotkeys_enabled = !app_focused;
    let global_mouse_hotkeys_enabled = !settings_active;
    self
      .global_hotkeys
      .update_settings(settings.as_ref(), global_hotkeys_enabled, global_mouse_hotkeys_enabled);
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

    if CUSTOM_WINDOW_CHROME {
      content = content.child(ctx.mount::<AppChrome>(()));
    }

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

    let window = ctx.window();
    if CUSTOM_MACOS_CHROME && !window.is_maximized && !window.is_full_screen {
      root = root.rounded(MACOS_WINDOW_CORNER_RADIUS);
    }

    if CUSTOM_WINDOW_CHROME {
      root = root.border_inside(1.0, theme::PaletteColor::Border);
    }

    let settings_open = self.settings_open.clone();
    let settings_window = ctx.window();
    if settings_open.get() {
      let close_settings = settings_popup.clone();
      let window = settings_window.clone();
      ctx.provide(settings_popup.clone());
      let popup = ctx.mount::<SettingsPopup>(());
      let settings_layer = modal_layer(ctx, popup).on_key_down(move |event| {
        if hotkeys::event_matches_hotkey(DEVTOOLS_HOTKEY, event) {
          window.open_devtools();
        } else if hotkeys::is_cancel_key(event) {
          close_settings.close();
        }
      });
      root = root.child(Modal::new(settings_layer).open(settings_open).target(Root));
    }

    for layer in window_affordance_layers(ctx) {
      root = root.child(layer);
    }

    if local_hotkeys_enabled {
      let window = ctx.window();
      let voice_hotkey = voice_hotkey.clone();
      let ptt_session = self.session.clone();
      let ptt_down_hotkey = push_to_talk_hotkey.clone();
      let mute_down_hotkey = mute_hotkey.clone();
      let deafen_down_hotkey = deafen_hotkey.clone();
      let active_toggle_hotkeys = self.active_toggle_hotkeys.clone();
      root = root.on_key_down(move |event| {
        if hotkeys::event_matches_hotkey(DEVTOOLS_HOTKEY, event) {
          window.open_devtools();
        } else if push_to_talk_enabled && hotkeys::event_matches_hotkey(&ptt_down_hotkey, event) {
          ptt_session.set_push_to_talk_active(true);
        } else if hotkeys::event_matches_hotkey(&mute_down_hotkey, event) {
          if activate_toggle_hotkey(&active_toggle_hotkeys, &mute_down_hotkey) {
            voice_hotkey.run(VoiceControlAction::ToggleMute);
          }
        } else if hotkeys::event_matches_hotkey(&deafen_down_hotkey, event) {
          if activate_toggle_hotkey(&active_toggle_hotkeys, &deafen_down_hotkey) {
            voice_hotkey.run(VoiceControlAction::ToggleDeafen);
          }
        }
      });

      let ptt_session = self.session.clone();
      let active_toggle_hotkeys = self.active_toggle_hotkeys.clone();
      root = root.on_key_up(move |event| {
        if push_to_talk_enabled && hotkeys::event_releases_hotkey(&push_to_talk_hotkey, event) {
          ptt_session.set_push_to_talk_active(false);
        }
        release_toggle_hotkey(&active_toggle_hotkeys, &mute_hotkey, event);
        release_toggle_hotkey(&active_toggle_hotkeys, &deafen_hotkey, event);
      });
    } else {
      let window = ctx.window();
      root = root.on_key_down(move |event| {
        if hotkeys::event_matches_hotkey(DEVTOOLS_HOTKEY, event) {
          window.open_devtools();
        }
      });
    }

    let update_status = self.update_status.get();
    if let Some(pill) = self.global_update_pill(ctx, &update_status) {
      root = root.child(pill);
    }

    root
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
    let y = if CUSTOM_WINDOW_CHROME {
      CHROME_HEIGHT + UPDATE_PILL_TOP_GAP
    } else {
      UPDATE_PILL_TOP_GAP
    };
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
  fn sync_window_full_screen(&self, storage: Option<&Storage>, full_screen: bool) {
    let (Some(storage), Some(tracker)) = (storage, self.window_state_tracker.as_ref()) else {
      return;
    };

    let state = {
      let mut state = tracker.current.lock().expect("window state lock poisoned");
      if state.full_screen == full_screen {
        return;
      }
      state.full_screen = full_screen;
      *state
    };
    let mut last_saved = tracker.last_saved.lock().expect("window state lock poisoned");
    if *last_saved == Some(state) {
      return;
    }
    if storage.save_window_state(state).is_ok() {
      *last_saved = Some(state);
    }
  }
}

fn apply_storage_locale(storage: &Option<Storage>, i18n: &lurq::app::i18n::I18n) {
  if let Some(storage) = storage
    && let Ok(settings) = storage.load_settings()
  {
    i18n.set_locale(settings.locale.clone());
  }
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

fn activate_toggle_hotkey(active_hotkeys: &Signal<Vec<String>>, hotkey: &str) -> bool {
  let key = hotkey_key(hotkey);
  if key.is_empty() {
    return false;
  }

  let mut active = active_hotkeys.get_untracked();
  if active.iter().any(|existing| existing == &key) {
    return false;
  }

  active.push(key);
  active_hotkeys.set(active);
  true
}

fn release_toggle_hotkey(active_hotkeys: &Signal<Vec<String>>, hotkey: &str, event: &lurq::app::events::KeyboardEvent) {
  if !hotkeys::event_releases_hotkey(hotkey, event) {
    return;
  }

  let key = hotkey_key(hotkey);
  active_hotkeys.set(
    active_hotkeys
      .get_untracked()
      .into_iter()
      .filter(|existing| existing != &key)
      .collect(),
  );
}

fn hotkey_key(hotkey: &str) -> String {
  hotkey.trim().to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
  use lurq::{app::events::KeyboardEvent, core::NodeId};

  use super::*;

  fn key_event(key: &str, code: &str, ctrl: bool) -> KeyboardEvent {
    KeyboardEvent::new(key, code, false, ctrl, false, false, NodeId::UNASSIGNED)
  }

  #[test]
  fn toggle_hotkey_activation_is_released_by_key_up() {
    let active = Signal::new(Vec::new());

    assert!(activate_toggle_hotkey(&active, "Ctrl+M"));
    assert!(!activate_toggle_hotkey(&active, "Ctrl+M"));

    release_toggle_hotkey(&active, "Ctrl+M", &key_event("M", "KeyM", true));
    assert!(activate_toggle_hotkey(&active, "Ctrl+M"));
  }
}
