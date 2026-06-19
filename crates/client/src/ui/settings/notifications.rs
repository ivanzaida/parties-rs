use std::{fs, sync::Arc};

use lurq::{
  app::{
    component::{Component, ComponentInfo, DevtoolsFormatter, DevtoolsInspectable},
    ctx::{Ctx, Modal, Root},
    events::{MouseButton, MouseEvent},
  },
  components::{Button, Column, Row, ScrollVertical, Stack, Text, TextOverflow},
  core::{Signal, Store},
  layout::{Alignment, layout_kind::Justify, text_style::TextStyle},
  node::{BackgroundColor, CursorIcon, Element, Style, color::Color, dimension::Dimension},
};

use crate::{
  services::notifications::{self, NotificationSound},
  session::ServerSession,
  storage::{AppAudioSettings, AppSettings, AppSettingsUpdater},
  theme,
  ui::{
    app_chrome::{CHROME_HEIGHT, content_height, modal_y},
    common::{
      lucide_icon::{LucideIcon, LucideIconProps},
      percent_slider::{PercentSlider, PercentSliderProps, PercentSliderSaveAction},
    },
    settings::{
      audio::{
        AUDIO_CONTROL_VALUE_SPACING, AUDIO_CONTROL_VALUE_WIDTH, AUDIO_CONTROL_WIDTH, audio_row, audio_scrollbar_style,
        audio_section_label,
      },
      shell::{SettingsPage, header, page_stack, screen_full, settings_content_padding, settings_section_spacing},
    },
  },
};

const NOTIFICATION_SLIDER_WIDTH: f32 = AUDIO_CONTROL_WIDTH - AUDIO_CONTROL_VALUE_WIDTH - AUDIO_CONTROL_VALUE_SPACING;
const NOTIFICATION_ACTION_MENU_WIDTH: f32 = 188.0;
const NOTIFICATION_ACTION_MENU_HEIGHT: f32 = 116.0;
const NOTIFICATION_ACTION_BUTTON_SIZE: f32 = 32.0;

pub struct SettingsNotificationsScreen {
  notification_volume: i32,
  notification_sound_overrides: String,
}

impl Component for SettingsNotificationsScreen {
  type Props = ();

  fn create(ctx: &mut Ctx) -> Self {
    let settings = ctx
      .use_context::<Store<AppAudioSettings>>()
      .map(|settings| settings.get())
      .unwrap_or_else(AppAudioSettings::default);

    Self {
      notification_volume: settings.notification_volume.clamp(0, 100),
      notification_sound_overrides: settings.notification_sound_overrides,
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let settings_updater = ctx.use_context::<AppSettingsUpdater>();
    let session = ctx.use_context::<ServerSession>();
    let (padding_x, padding_y) = settings_content_padding(ctx);
    let section_spacing = settings_section_spacing(ctx);
    let content = ScrollVertical::new(
      page_stack(ctx)
        .padding_vertical(padding_y)
        .padding_horizontal(padding_x)
        .child(header(
          &ctx.t("settings.notifications.title"),
          &ctx.t("settings.notifications.description"),
        ))
        .child(
          Column::new()
            .width(Dimension::Pct(100.0))
            .spacing(section_spacing)
            .child(
              Column::new()
                .width(Dimension::Pct(100.0))
                .spacing(12.0)
                .child(audio_section_label(&ctx.t("settings.notifications.section.playback")))
                .child(ctx.mount::<NotificationVolumeSetting>(NotificationVolumeSettingProps {
                  initial_value: self.notification_volume,
                  on_blur: notification_volume_save_action(settings_updater.clone(), session.clone()),
                })),
            )
            .child(
              Column::new()
                .width(Dimension::Pct(100.0))
                .spacing(12.0)
                .child(audio_section_label(&ctx.t("settings.notifications.section.outgoing")))
                .child(outgoing_voice_join_sound_setting(
                  ctx,
                  &self.notification_sound_overrides,
                )),
            )
            .child(
              Column::new()
                .width(Dimension::Pct(100.0))
                .spacing(12.0)
                .child(audio_section_label(&ctx.t("settings.notifications.section.sounds")))
                .child(notification_sound_setting(
                  ctx,
                  NotificationSound::VoiceJoin,
                  "settings.notifications.sound.voice_join",
                  "settings.notifications.sound.voice_join.description",
                  &self.notification_sound_overrides,
                ))
                .child(notification_sound_setting(
                  ctx,
                  NotificationSound::VoiceLeave,
                  "settings.notifications.sound.voice_leave",
                  "settings.notifications.sound.voice_leave.description",
                  &self.notification_sound_overrides,
                ))
                .child(notification_sound_setting(
                  ctx,
                  NotificationSound::ChatMessage,
                  "settings.notifications.sound.chat_message",
                  "settings.notifications.sound.chat_message.description",
                  &self.notification_sound_overrides,
                ))
                .child(notification_sound_setting(
                  ctx,
                  NotificationSound::Mention,
                  "settings.notifications.sound.mention",
                  "settings.notifications.sound.mention.description",
                  &self.notification_sound_overrides,
                ))
                .child(notification_sound_setting(
                  ctx,
                  NotificationSound::UserKicked,
                  "settings.notifications.sound.user_kicked",
                  "settings.notifications.sound.user_kicked.description",
                  &self.notification_sound_overrides,
                ))
                .child(notification_sound_setting(
                  ctx,
                  NotificationSound::StreamStarted,
                  "settings.notifications.sound.stream_started",
                  "settings.notifications.sound.stream_started.description",
                  &self.notification_sound_overrides,
                ))
                .child(notification_sound_setting(
                  ctx,
                  NotificationSound::StreamEnded,
                  "settings.notifications.sound.stream_ended",
                  "settings.notifications.sound.stream_ended.description",
                  &self.notification_sound_overrides,
                ))
                .child(notification_sound_setting(
                  ctx,
                  NotificationSound::ConnectionLost,
                  "settings.notifications.sound.connection_lost",
                  "settings.notifications.sound.connection_lost.description",
                  &self.notification_sound_overrides,
                ))
                .child(notification_sound_setting(
                  ctx,
                  NotificationSound::ModerationAction,
                  "settings.notifications.sound.moderation",
                  "settings.notifications.sound.moderation.description",
                  &self.notification_sound_overrides,
                )),
            ),
        ),
    )
    .width(Dimension::Pct(100.0))
    .height(Dimension::Pct(100.0))
    .scrollbar(audio_scrollbar_style())
    .scrollbar_hovered(|mut style| {
      let palette = theme::palette();
      style.thumb_color = palette.accent_hover;
      style.track_color = palette.surface_input.with_opacity(0.75);
      style
    });

    screen_full(ctx, SettingsPage::Notifications, content)
  }
}

#[derive(Clone)]
struct NotificationVolumeSettingProps {
  initial_value: i32,
  on_blur: PercentSliderSaveAction,
}

impl PartialEq for NotificationVolumeSettingProps {
  fn eq(&self, other: &Self) -> bool {
    self.initial_value == other.initial_value
  }
}

impl DevtoolsInspectable for NotificationVolumeSettingProps {
  fn inspect(&self, formatter: &mut DevtoolsFormatter<'_>) {
    formatter.buffer_mut().push(ComponentInfo::with_value(
      "initial_value",
      std::any::type_name::<i32>(),
      self.initial_value.to_string(),
    ));
  }
}

struct NotificationVolumeSetting;

impl Component for NotificationVolumeSetting {
  type Props = NotificationVolumeSettingProps;

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let props = ctx.props::<Self::Props>().clone();
    audio_row(
      &ctx.t("settings.notifications.volume"),
      &ctx.t("settings.notifications.volume.description"),
      ctx.mount::<PercentSlider>(PercentSliderProps {
        initial_value: props.initial_value,
        control_width: AUDIO_CONTROL_WIDTH,
        track_width: NOTIFICATION_SLIDER_WIDTH,
        value_width: AUDIO_CONTROL_VALUE_WIDTH,
        value_spacing: AUDIO_CONTROL_VALUE_SPACING,
        on_blur: props.on_blur,
      }),
      true,
    )
  }
}

#[derive(Clone)]
struct NotificationSoundSettingProps {
  sound: NotificationSound,
  title_key: &'static str,
  description_key: &'static str,
  initial_overrides: String,
}

impl PartialEq for NotificationSoundSettingProps {
  fn eq(&self, other: &Self) -> bool {
    self.sound == other.sound
      && self.title_key == other.title_key
      && self.description_key == other.description_key
      && self.initial_overrides == other.initial_overrides
  }
}

impl DevtoolsInspectable for NotificationSoundSettingProps {
  fn inspect(&self, formatter: &mut DevtoolsFormatter<'_>) {
    formatter.buffer_mut().push(ComponentInfo::with_value(
      "sound",
      std::any::type_name::<NotificationSound>(),
      format!("{:?}", self.sound),
    ));
  }
}

struct NotificationSoundSetting {
  value: Signal<String>,
  menu_open: Signal<bool>,
  menu_anchor: Signal<Option<(f32, f32)>>,
}

impl Component for NotificationSoundSetting {
  type Props = NotificationSoundSettingProps;

  fn create(ctx: &mut Ctx) -> Self {
    let props = ctx.props::<Self::Props>();
    Self {
      value: ctx
        .signal(notifications::notification_sound_override(&props.initial_overrides, props.sound).unwrap_or_default()),
      menu_open: ctx.signal(false),
      menu_anchor: ctx.signal(None::<(f32, f32)>),
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let props = ctx.props::<Self::Props>().clone();
    let settings_updater = ctx.use_context::<AppSettingsUpdater>();
    let audio_settings_store = ctx.use_context::<Store<AppAudioSettings>>();
    let session = ctx.use_context::<ServerSession>();
    let custom = self.value.get() == notifications::SOUND_CHOICE_CUSTOM;
    let custom_exists = custom && notifications::custom_sound_exists(props.sound);
    let custom_missing = custom && !custom_exists;
    let status = if custom_exists {
      ctx
        .t_args(
          "settings.notifications.custom_file",
          [(
            "file",
            format!("audio/{}", notifications::notification_sound_file_name(props.sound)),
          )],
        )
        .to_string()
    } else if custom {
      ctx.t("settings.notifications.custom_missing").to_string()
    } else {
      ctx.t("settings.notifications.default").to_string()
    };

    let mut action_modal = None;
    if self.menu_open.get() {
      action_modal = Some(
        Modal::new(notification_sound_action_overlay(
          ctx,
          props.sound,
          self.value.clone(),
          self.menu_open.clone(),
          self.menu_anchor.clone(),
          self.menu_anchor.get(),
          settings_updater.clone(),
          audio_settings_store.clone(),
          session.clone(),
          settings_updater.as_ref().is_none_or(|settings| !settings.has_storage()),
        ))
        .open(self.menu_open.clone())
        .target(Root),
      );
    }

    let row = audio_row(
      &ctx.t(props.title_key),
      &ctx.t(props.description_key),
      notification_sound_controls(
        ctx,
        self.menu_open.clone(),
        self.menu_anchor.clone(),
        &status,
        custom_exists,
        custom_missing,
      ),
      true,
    );
    if let Some(action_modal) = action_modal {
      return Column::new()
        .width(Dimension::Pct(100.0))
        .child(row)
        .child(action_modal)
        .into();
    }
    row
  }
}

fn notification_sound_setting(
  ctx: &mut Ctx,
  sound: NotificationSound,
  title_key: &'static str,
  description_key: &'static str,
  initial_overrides: &str,
) -> Element {
  ctx.mount::<NotificationSoundSetting>(NotificationSoundSettingProps {
    sound,
    title_key,
    description_key,
    initial_overrides: initial_overrides.to_owned(),
  })
}

#[derive(Clone)]
struct OutgoingVoiceJoinSoundSettingProps {
  initial_overrides: String,
}

impl PartialEq for OutgoingVoiceJoinSoundSettingProps {
  fn eq(&self, other: &Self) -> bool {
    self.initial_overrides == other.initial_overrides
  }
}

impl DevtoolsInspectable for OutgoingVoiceJoinSoundSettingProps {
  fn inspect(&self, formatter: &mut DevtoolsFormatter<'_>) {
    formatter.buffer_mut().push(ComponentInfo::with_value(
      "key",
      std::any::type_name::<&'static str>(),
      notifications::OUTGOING_VOICE_JOIN_SOUND_KEY.to_owned(),
    ));
  }
}

struct OutgoingVoiceJoinSoundSetting {
  value: Signal<String>,
  menu_open: Signal<bool>,
  menu_anchor: Signal<Option<(f32, f32)>>,
}

impl Component for OutgoingVoiceJoinSoundSetting {
  type Props = OutgoingVoiceJoinSoundSettingProps;

  fn create(ctx: &mut Ctx) -> Self {
    let props = ctx.props::<Self::Props>();
    Self {
      value: ctx
        .signal(notifications::outgoing_voice_join_sound_override(&props.initial_overrides).unwrap_or_default()),
      menu_open: ctx.signal(false),
      menu_anchor: ctx.signal(None::<(f32, f32)>),
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let settings_updater = ctx.use_context::<AppSettingsUpdater>();
    let audio_settings_store = ctx.use_context::<Store<AppAudioSettings>>();
    let session = ctx.use_context::<ServerSession>();
    let custom = self.value.get() == notifications::SOUND_CHOICE_CUSTOM;
    let custom_exists = custom && notifications::outgoing_voice_join_sound_exists();
    let custom_missing = custom && !custom_exists;
    let status = if custom_exists {
      ctx
        .t_args(
          "settings.notifications.custom_file",
          [(
            "file",
            format!("audio/{}", notifications::OUTGOING_VOICE_JOIN_SOUND_FILE_NAME),
          )],
        )
        .to_string()
    } else if custom {
      ctx.t("settings.notifications.custom_missing").to_string()
    } else {
      ctx.t("settings.notifications.none").to_string()
    };

    let mut action_modal = None;
    if self.menu_open.get() {
      action_modal = Some(
        Modal::new(outgoing_voice_join_sound_action_overlay(
          ctx,
          self.value.clone(),
          self.menu_open.clone(),
          self.menu_anchor.clone(),
          self.menu_anchor.get(),
          settings_updater.clone(),
          audio_settings_store.clone(),
          session.clone(),
          custom_exists,
          settings_updater.as_ref().is_none_or(|settings| !settings.has_storage()),
        ))
        .open(self.menu_open.clone())
        .target(Root),
      );
    }

    let row = audio_row(
      &ctx.t("settings.notifications.outgoing_voice_join"),
      &ctx.t("settings.notifications.outgoing_voice_join.description"),
      notification_sound_controls(
        ctx,
        self.menu_open.clone(),
        self.menu_anchor.clone(),
        &status,
        custom_exists,
        custom_missing,
      ),
      true,
    );
    if let Some(action_modal) = action_modal {
      return Column::new()
        .width(Dimension::Pct(100.0))
        .child(row)
        .child(action_modal)
        .into();
    }
    row
  }
}

fn outgoing_voice_join_sound_setting(ctx: &mut Ctx, initial_overrides: &str) -> Element {
  ctx.mount::<OutgoingVoiceJoinSoundSetting>(OutgoingVoiceJoinSoundSettingProps {
    initial_overrides: initial_overrides.to_owned(),
  })
}

fn notification_sound_controls(
  ctx: &mut Ctx,
  menu_open: Signal<bool>,
  menu_anchor: Signal<Option<(f32, f32)>>,
  status: &str,
  custom_exists: bool,
  custom_missing: bool,
) -> Element {
  let toggle_menu_open = menu_open.clone();
  let toggle_menu_anchor = menu_anchor.clone();
  let scale = ctx.window().scale_factor.max(f32::EPSILON);
  let trigger = notification_icon_button(ctx, "ellipsis", true).on_click(move |event: MouseEvent| {
    toggle_menu_anchor.set(Some((event.x / scale, event.y / scale)));
    toggle_menu_open.set(!toggle_menu_open.get());
  });

  Row::new()
    .width(AUDIO_CONTROL_WIDTH)
    .align_items(Alignment::Center)
    .justify(Justify::End)
    .spacing(8.0)
    .child(notification_status(status, custom_exists, custom_missing))
    .child(trigger)
    .into()
}

fn notification_sound_action_overlay(
  ctx: &mut Ctx,
  sound: NotificationSound,
  value: Signal<String>,
  menu_open: Signal<bool>,
  menu_anchor: Signal<Option<(f32, f32)>>,
  anchor: Option<(f32, f32)>,
  settings_updater: Option<AppSettingsUpdater>,
  audio_settings_store: Option<Store<AppAudioSettings>>,
  session: Option<ServerSession>,
  disabled: bool,
) -> Element {
  let window = ctx.window();
  let window_width = window.logical_width();
  let modal_height = content_height(ctx);
  let (menu_left, menu_top) = notification_action_menu_position(anchor, window_width, modal_height);
  let close_left_open = menu_open.clone();
  let close_left_anchor = menu_anchor.clone();
  let close_right_open = menu_open.clone();
  let close_right_anchor = menu_anchor.clone();
  let close_middle_open = menu_open.clone();
  let close_middle_anchor = menu_anchor.clone();

  Stack::new()
    .width(window_width)
    .height(modal_height)
    .absolute(0.0, 0.0, window_width, modal_height)
    .child(
      Row::new()
        .width(window_width)
        .height(modal_height)
        .background(BackgroundColor::Color(Color::from_hex("#00000000")))
        .on_click(move |_| close_notification_menu(close_left_open.clone(), close_left_anchor.clone()))
        .on_mouse_click(MouseButton::Right, move |_| {
          close_notification_menu(close_right_open.clone(), close_right_anchor.clone())
        })
        .on_mouse_click(MouseButton::Middle, move |_| {
          close_notification_menu(close_middle_open.clone(), close_middle_anchor.clone())
        }),
    )
    .child(
      notification_sound_action_menu(
        ctx,
        sound,
        value,
        menu_open,
        menu_anchor,
        settings_updater,
        audio_settings_store,
        session,
        disabled,
      )
      .absolute_position(menu_left, menu_top),
    )
    .into()
}

#[allow(clippy::too_many_arguments)]
fn outgoing_voice_join_sound_action_overlay(
  ctx: &mut Ctx,
  value: Signal<String>,
  menu_open: Signal<bool>,
  menu_anchor: Signal<Option<(f32, f32)>>,
  anchor: Option<(f32, f32)>,
  settings_updater: Option<AppSettingsUpdater>,
  audio_settings_store: Option<Store<AppAudioSettings>>,
  session: Option<ServerSession>,
  custom_exists: bool,
  disabled: bool,
) -> Element {
  let window = ctx.window();
  let window_width = window.logical_width();
  let modal_height = content_height(ctx);
  let (menu_left, menu_top) = notification_action_menu_position(anchor, window_width, modal_height);
  let close_left_open = menu_open.clone();
  let close_left_anchor = menu_anchor.clone();
  let close_right_open = menu_open.clone();
  let close_right_anchor = menu_anchor.clone();
  let close_middle_open = menu_open.clone();
  let close_middle_anchor = menu_anchor.clone();

  Stack::new()
    .width(window_width)
    .height(modal_height)
    .absolute(0.0, 0.0, window_width, modal_height)
    .child(
      Row::new()
        .width(window_width)
        .height(modal_height)
        .background(BackgroundColor::Color(Color::from_hex("#00000000")))
        .on_click(move |_| close_notification_menu(close_left_open.clone(), close_left_anchor.clone()))
        .on_mouse_click(MouseButton::Right, move |_| {
          close_notification_menu(close_right_open.clone(), close_right_anchor.clone())
        })
        .on_mouse_click(MouseButton::Middle, move |_| {
          close_notification_menu(close_middle_open.clone(), close_middle_anchor.clone())
        }),
    )
    .child(
      outgoing_voice_join_sound_action_menu(
        ctx,
        value,
        menu_open,
        menu_anchor,
        settings_updater,
        audio_settings_store,
        session,
        custom_exists,
        disabled,
      )
      .absolute_position(menu_left, menu_top),
    )
    .into()
}

fn notification_action_menu_position(anchor: Option<(f32, f32)>, window_width: f32, modal_height: f32) -> (f32, f32) {
  const EDGE_PADDING: f32 = 8.0;
  const MENU_GAP: f32 = 6.0;
  let (anchor_x, anchor_y) = anchor.unwrap_or((
    window_width - NOTIFICATION_ACTION_MENU_WIDTH - 40.0,
    CHROME_HEIGHT + 220.0,
  ));
  let menu_left = (anchor_x - NOTIFICATION_ACTION_MENU_WIDTH + NOTIFICATION_ACTION_BUTTON_SIZE).clamp(
    EDGE_PADDING,
    (window_width - NOTIFICATION_ACTION_MENU_WIDTH - EDGE_PADDING).max(EDGE_PADDING),
  );
  let below = modal_y(anchor_y) + NOTIFICATION_ACTION_BUTTON_SIZE + MENU_GAP;
  let above = modal_y(anchor_y) - NOTIFICATION_ACTION_MENU_HEIGHT - MENU_GAP;
  let menu_top = if below + NOTIFICATION_ACTION_MENU_HEIGHT > modal_height - EDGE_PADDING {
    above.max(EDGE_PADDING)
  } else {
    below
  }
  .clamp(
    EDGE_PADDING,
    (modal_height - NOTIFICATION_ACTION_MENU_HEIGHT - EDGE_PADDING).max(EDGE_PADDING),
  );

  (menu_left, menu_top)
}

fn close_notification_menu(menu_open: Signal<bool>, menu_anchor: Signal<Option<(f32, f32)>>) {
  menu_open.set(false);
  menu_anchor.set(None);
}

fn notification_sound_action_menu(
  ctx: &mut Ctx,
  sound: NotificationSound,
  value: Signal<String>,
  menu_open: Signal<bool>,
  menu_anchor: Signal<Option<(f32, f32)>>,
  settings_updater: Option<AppSettingsUpdater>,
  audio_settings_store: Option<Store<AppAudioSettings>>,
  session: Option<ServerSession>,
  disabled: bool,
) -> Column {
  let play_label = ctx.t("settings.notifications.action.play").to_string();
  let choose_label = ctx.t("settings.notifications.action.choose_mp3").to_string();
  let reset_label = ctx.t("settings.notifications.action.reset").to_string();

  let play_audio_settings_store = audio_settings_store;
  let close_play = menu_open.clone();
  let close_play_anchor = menu_anchor.clone();
  let play = notification_menu_item(ctx, "play", &play_label, false, true).on_click(move |_| {
    close_notification_menu(close_play.clone(), close_play_anchor.clone());
    let settings = play_audio_settings_store
      .as_ref()
      .map(|settings| settings.get())
      .unwrap_or_else(AppAudioSettings::default);
    notifications::play(
      sound,
      notifications::NotificationAudioSettings::from_audio_settings(&settings),
    );
  });

  let choose_settings_updater = settings_updater.clone();
  let choose_session = session.clone();
  let choose_value = value.clone();
  let close_choose = menu_open.clone();
  let close_choose_anchor = menu_anchor.clone();
  let mp3_filter_label = ctx.t("settings.notifications.file_filter.mp3_audio").to_string();
  let mut choose = notification_menu_item(ctx, "plus", &choose_label, false, !disabled);
  if !disabled {
    choose = choose.on_click(move |_| {
      close_notification_menu(close_choose.clone(), close_choose_anchor.clone());
      let audio_dir = notifications::custom_audio_dir();
      let _ = fs::create_dir_all(&audio_dir);
      let Some(path) = rfd::FileDialog::new()
        .add_filter(&mp3_filter_label, &["mp3"])
        .set_directory(audio_dir)
        .pick_file()
      else {
        return;
      };

      if let Err(error) = notifications::install_custom_sound(sound, &path) {
        tracing::error!(target: "notifications", "[notifications] failed to install custom notification sound: {error}");
        return;
      }

      choose_value.set(notifications::SOUND_CHOICE_CUSTOM.to_owned());
      if let Some(settings_updater) = choose_settings_updater.as_ref() {
        let settings =
          save_notification_sound_override(settings_updater, sound, notifications::SOUND_CHOICE_CUSTOM);
        if let Some(session) = choose_session.as_ref() {
          session.set_notification_audio_settings(&AppAudioSettings::from(&settings));
        }
      }
    });
  }

  let reset_settings_updater = settings_updater;
  let reset_session = session;
  let reset_value = value.clone();
  let close_reset = menu_open;
  let close_reset_anchor = menu_anchor;
  let mut reset = notification_menu_item(ctx, "x", &reset_label, true, !disabled);
  if !disabled {
    reset = reset.on_click(move |_| {
      close_notification_menu(close_reset.clone(), close_reset_anchor.clone());
      reset_value.set(String::new());
      if let Some(settings_updater) = reset_settings_updater.as_ref() {
        let settings = save_notification_sound_override(settings_updater, sound, notifications::SOUND_CHOICE_DEFAULT);
        if let Some(session) = reset_session.as_ref() {
          session.set_notification_audio_settings(&AppAudioSettings::from(&settings));
        }
      }
    });
  }

  Column::new()
    .width(NOTIFICATION_ACTION_MENU_WIDTH)
    .spacing(2.0)
    .padding(6.0)
    .rounded(8.0)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised))
    .border_inside(1.0, BackgroundColor::Palette(theme::PaletteColor::BorderStrong))
    .child(play)
    .child(choose)
    .child(reset)
}

fn outgoing_voice_join_sound_action_menu(
  ctx: &mut Ctx,
  value: Signal<String>,
  menu_open: Signal<bool>,
  menu_anchor: Signal<Option<(f32, f32)>>,
  settings_updater: Option<AppSettingsUpdater>,
  audio_settings_store: Option<Store<AppAudioSettings>>,
  session: Option<ServerSession>,
  custom_exists: bool,
  disabled: bool,
) -> Column {
  let play_label = ctx.t("settings.notifications.action.play").to_string();
  let choose_label = ctx.t("settings.notifications.action.choose_mp3").to_string();
  let reset_label = ctx.t("settings.notifications.action.reset").to_string();

  let play_audio_settings_store = audio_settings_store;
  let close_play = menu_open.clone();
  let close_play_anchor = menu_anchor.clone();
  let mut play = notification_menu_item(ctx, "play", &play_label, false, custom_exists);
  if custom_exists {
    play = play.on_click(move |_| {
      close_notification_menu(close_play.clone(), close_play_anchor.clone());
      let settings = play_audio_settings_store
        .as_ref()
        .map(|settings| settings.get())
        .unwrap_or_else(AppAudioSettings::default);
      notifications::play_outgoing_voice_join(notifications::NotificationAudioSettings::from_audio_settings(&settings));
    });
  }

  let choose_settings_updater = settings_updater.clone();
  let choose_session = session.clone();
  let choose_value = value.clone();
  let close_choose = menu_open.clone();
  let close_choose_anchor = menu_anchor.clone();
  let mp3_filter_label = ctx.t("settings.notifications.file_filter.mp3_audio").to_string();
  let mut choose = notification_menu_item(ctx, "plus", &choose_label, false, !disabled);
  if !disabled {
    choose = choose.on_click(move |_| {
      close_notification_menu(close_choose.clone(), close_choose_anchor.clone());
      let audio_dir = notifications::custom_audio_dir();
      let _ = fs::create_dir_all(&audio_dir);
      let Some(path) = rfd::FileDialog::new()
        .add_filter(&mp3_filter_label, &["mp3"])
        .set_directory(audio_dir)
        .pick_file()
      else {
        return;
      };

      if let Err(error) = notifications::install_outgoing_voice_join_sound(&path) {
        tracing::error!(target: "notifications", "[notifications] failed to install outgoing voice join sound: {error}");
        return;
      }

      choose_value.set(notifications::SOUND_CHOICE_CUSTOM.to_owned());
      if let Some(settings_updater) = choose_settings_updater.as_ref() {
        let settings =
          save_outgoing_voice_join_sound_override(settings_updater, notifications::SOUND_CHOICE_CUSTOM);
        if let Some(session) = choose_session.as_ref() {
          session.set_notification_audio_settings(&AppAudioSettings::from(&settings));
        }
      }
    });
  }

  let reset_settings_updater = settings_updater;
  let reset_session = session;
  let reset_value = value.clone();
  let close_reset = menu_open;
  let close_reset_anchor = menu_anchor;
  let mut reset = notification_menu_item(ctx, "x", &reset_label, true, !disabled);
  if !disabled {
    reset = reset.on_click(move |_| {
      close_notification_menu(close_reset.clone(), close_reset_anchor.clone());
      reset_value.set(String::new());
      if let Some(settings_updater) = reset_settings_updater.as_ref() {
        let settings = save_outgoing_voice_join_sound_override(settings_updater, notifications::SOUND_CHOICE_DEFAULT);
        if let Some(session) = reset_session.as_ref() {
          session.set_notification_audio_settings(&AppAudioSettings::from(&settings));
        }
      }
    });
  }

  Column::new()
    .width(NOTIFICATION_ACTION_MENU_WIDTH)
    .spacing(2.0)
    .padding(6.0)
    .rounded(8.0)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised))
    .border_inside(1.0, BackgroundColor::Palette(theme::PaletteColor::BorderStrong))
    .child(play)
    .child(choose)
    .child(reset)
}

fn notification_menu_item(ctx: &mut Ctx, icon: &'static str, label: &str, danger: bool, enabled: bool) -> Row {
  let palette = theme::palette();
  let color = if !enabled {
    palette.text_muted
  } else if danger {
    palette.danger
  } else {
    palette.text_secondary
  };

  Row::new()
    .width(Dimension::Pct(100.0))
    .height(34.0)
    .align_items(Alignment::Center)
    .spacing(10.0)
    .padding_horizontal(10.0)
    .rounded(6.0)
    .cursor(if enabled {
      CursorIcon::Pointer
    } else {
      CursorIcon::Default
    })
    .hovered_style(Style::new().background(BackgroundColor::Palette(theme::PaletteColor::SurfaceInput)))
    .active_style(Style::new().background(BackgroundColor::Palette(theme::PaletteColor::SurfaceInput)))
    .child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon,
      size: 14.0,
      color,
    }))
    .child(Text::styled(
      label,
      TextStyle {
        font_family: Arc::from("Inter"),
        font_size: 13.0,
        line_height: 1.2,
        color,
        ..TextStyle::default()
      },
    ))
}

fn notification_status(status: &str, custom_exists: bool, custom_missing: bool) -> Element {
  let (background, border, color) = if custom_exists {
    (
      theme::PaletteColor::AccentMuted,
      theme::PaletteColor::Accent,
      theme::palette().accent,
    )
  } else if custom_missing {
    (
      theme::PaletteColor::DangerMuted,
      theme::PaletteColor::Danger,
      theme::palette().danger,
    )
  } else {
    (
      theme::PaletteColor::SurfaceInput,
      theme::PaletteColor::Border,
      theme::palette().text_muted,
    )
  };

  Row::new()
    .height(28.0)
    .max_width(206.0)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .padding_horizontal(10.0)
    .rounded(theme::RadiusSize::Md)
    .background(BackgroundColor::Palette(background))
    .border_inside(1.0, border)
    .child(
      Text::styled(status, notification_status_style(color))
        .nowrap()
        .text_overflow(TextOverflow::Elipsis)
        .min_width(0.0),
    )
    .into()
}

fn notification_icon_button(ctx: &mut Ctx, icon: &'static str, enabled: bool) -> Button {
  Button::empty()
    .button()
    .size(NOTIFICATION_ACTION_BUTTON_SIZE, NOTIFICATION_ACTION_BUTTON_SIZE)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .rounded(theme::RadiusSize::Md)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceInput))
    .border_inside(1.0, theme::PaletteColor::Border)
    .cursor(if enabled {
      CursorIcon::Pointer
    } else {
      CursorIcon::Default
    })
    .hovered_style(Style::new().background(BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised)))
    .active_style(Style::new().background(BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised)))
    .child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon,
      size: 14.0,
      color: if enabled {
        theme::palette().text_secondary
      } else {
        theme::palette().text_muted
      },
    }))
}

fn notification_status_style(color: Color) -> TextStyle {
  TextStyle {
    font_family: Arc::from("JetBrains Mono"),
    font_size: 10.0,
    line_height: 1.2,
    color,
    ..TextStyle::default()
  }
}

fn notification_volume_save_action(
  settings_updater: Option<AppSettingsUpdater>,
  session: Option<ServerSession>,
) -> PercentSliderSaveAction {
  Arc::new(move |value| {
    if let Some(settings_updater) = settings_updater.as_ref() {
      let settings = settings_updater.update(|settings| {
        settings.notification_volume = value.clamp(0, 100);
      });
      if let Some(session) = session.as_ref() {
        session.set_notification_audio_settings(&AppAudioSettings::from(&settings));
      }
    }
  })
}

fn save_notification_sound_override(
  settings_updater: &AppSettingsUpdater,
  sound: NotificationSound,
  value: impl AsRef<str>,
) -> AppSettings {
  settings_updater.update(|settings| {
    settings.notification_sound_overrides = set_sound_override_key(
      &settings.notification_sound_overrides,
      notifications::notification_sound_key(sound),
      value.as_ref(),
    );
  })
}

fn save_outgoing_voice_join_sound_override(
  settings_updater: &AppSettingsUpdater,
  value: impl AsRef<str>,
) -> AppSettings {
  settings_updater.update(|settings| {
    settings.notification_sound_overrides = set_sound_override_key(
      &settings.notification_sound_overrides,
      notifications::OUTGOING_VOICE_JOIN_SOUND_KEY,
      value.as_ref(),
    );
  })
}

fn set_sound_override_key(overrides: &str, key: &str, value: &str) -> String {
  let mut object = serde_json::from_str::<serde_json::Value>(overrides)
    .ok()
    .and_then(|value| value.as_object().cloned())
    .unwrap_or_default();
  let value = value.trim();
  if value.is_empty() {
    object.remove(key);
  } else {
    object.insert(key.to_owned(), serde_json::Value::String(value.to_owned()));
  }

  if object.is_empty() {
    String::new()
  } else {
    serde_json::Value::Object(object).to_string()
  }
}
