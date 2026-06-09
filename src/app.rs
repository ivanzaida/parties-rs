use std::time::Duration;

use lurq::{
  app::{
    component::{Component, ComponentInfo, DevtoolsInspectable},
    ctx::Ctx,
  },
  components::{Column, Stack},
  core::Signal,
  layout::{Alignment, layout_kind::Justify},
  node::{BackgroundColor, Element, dimension::Dimension},
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
    voice_controls::{VoiceControlAction, apply_voice_control},
  },
  session::ServerSession,
  storage::Storage,
  theme,
  ui::{
    app_chrome::{AppChrome, CUSTOM_MACOS_CHROME, CUSTOM_WINDOW_CHROME, modal_layer, window_affordance_layers},
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

pub struct App {
  router: RouterHandle,
  session: ServerSession,
  storage: Signal<Option<Storage>>,
  settings_open: Signal<bool>,
  settings_page: Signal<SettingsPage>,
  window_affordances_open: Signal<bool>,
  active_toggle_hotkeys: Signal<Vec<String>>,
  global_hotkeys: GlobalVoiceHotkeys,
}

#[derive(Clone)]
pub struct AppProps {
  pub tokio: tokio::runtime::Handle,
  pub startup_storage: Option<Storage>,
  pub startup_error: Option<String>,
  pub session: ServerSession,
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
    let loading_storage = storage.clone();
    let startup_error = props.startup_error.clone();
    let router = ctx.router(
      Routes::new()
        .route(ROUTE_LOADING, move |ctx| {
          ctx.mount::<LoadingIdentityScreen>(LoadingIdentityScreenProps {
            storage: loading_storage.clone(),
            startup_error: startup_error.clone(),
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
    Self {
      router,
      session,
      storage,
      settings_open: ctx.signal(false),
      settings_page: ctx.signal(SettingsPage::Overview),
      window_affordances_open: ctx.signal(true),
      active_toggle_hotkeys: ctx.signal(Vec::new()),
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
    let settings = storage.as_ref().and_then(|storage| storage.load_settings().ok());
    if let Some(settings) = settings.as_ref() {
      self.session.set_notification_audio_settings(settings);
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
      move |action: VoiceControlAction| {
        let session = session.clone();
        async move { apply_voice_control(session, action).await }
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
    ctx.modal(settings_open, move |ctx| {
      let close_settings = settings_popup.clone();
      let window = settings_window.clone();
      ctx.provide(settings_popup.clone());
      let popup = ctx.mount::<SettingsPopup>(());
      modal_layer(ctx, popup).on_key_down(move |event| {
        if hotkeys::event_matches_hotkey(DEVTOOLS_HOTKEY, event) {
          window.open_devtools();
        } else if hotkeys::is_cancel_key(event) {
          close_settings.close();
        }
      })
    });

    for layer in window_affordance_layers(ctx) {
      ctx.modal(self.window_affordances_open.clone(), move |_| layer);
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

    root
  }
}

fn apply_storage_locale(storage: &Option<Storage>, i18n: &lurq::app::i18n::I18n) {
  if let Some(storage) = storage
    && let Ok(settings) = storage.load_settings()
  {
    i18n.set_locale(settings.locale.clone());
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
    KeyboardEvent {
      key: key.to_owned(),
      code: code.to_owned(),
      shift: false,
      ctrl,
      alt: false,
      meta: false,
      target_id: NodeId::UNASSIGNED,
    }
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
