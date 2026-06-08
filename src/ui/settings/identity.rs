use std::sync::Arc;

use lurq::{
  app::{component::Component, ctx::Ctx},
  clipboard,
  components::{Column, Row, Text, TextInput},
  core::Signal,
  layout::{
    Alignment,
    layout_kind::Justify,
    text_style::{FontWeight, TextStyle},
  },
  node::{BackgroundColor, CursorIcon, Element, Style, border::Border, color::Color, dimension::Dimension},
};

use crate::{
  identity::{public_key_fingerprint, secret_key_to_hex},
  routes::ROUTE_IDENTITY_SETUP,
  storage::{AppSettings, Storage},
  theme,
  ui::{
    common::{
      confirm_modal::{ConfirmAction, ConfirmModal, ConfirmModalProps},
      lucide_icon::{LucideIcon, LucideIconProps},
    },
    settings::shell::{SettingsPage, header, page_stack, screen},
  },
};

pub struct SettingsIdentityScreen {
  public_id_copied: Signal<bool>,
  recovery_revealed: Signal<bool>,
  recovery_copied: Signal<bool>,
  private_key_copied: Signal<bool>,
  remove_open: Signal<bool>,
  display_name: String,
}

impl Component for SettingsIdentityScreen {
  type Props = ();

  fn create(ctx: &mut Ctx) -> Self {
    let storage = ctx.use_context::<Storage>();
    let settings = storage
      .as_ref()
      .and_then(|storage| storage.load_settings().ok())
      .unwrap_or_else(AppSettings::default);
    let display_name = settings.display_name;

    Self {
      public_id_copied: ctx.signal(false),
      recovery_revealed: ctx.signal(false),
      recovery_copied: ctx.signal(false),
      private_key_copied: ctx.signal(false),
      remove_open: ctx.signal(false),
      display_name,
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let storage = ctx.use_context::<Storage>();
    let identity = storage
      .as_ref()
      .and_then(|storage| storage.load_identity().ok())
      .flatten();
    let navigator = ctx.navigator();
    let public_id = identity
      .as_ref()
      .map(|identity| short_hex(&identity.public_key))
      .unwrap_or_else(|| ctx.t("settings.identity.missing").to_string());
    let fingerprint = identity
      .as_ref()
      .map(|identity| public_key_fingerprint(&identity.public_key))
      .unwrap_or_else(|| ctx.t("settings.identity.missing").to_string());
    let recovery_phrase = identity.as_ref().and_then(|identity| identity.seed_phrase.clone());
    let private_key = identity
      .as_ref()
      .map(|identity| secret_key_to_hex(&identity.secret_key));
    let public_id_label = ctx.t("settings.identity.public_id");
    let fingerprint_label = ctx.t("settings.identity.fingerprint");
    let recovery_label = ctx.t("settings.identity.recovery");
    let recovery_description = ctx.t("settings.identity.recovery.description");
    let recovery_unavailable = ctx.t("settings.identity.recovery.unavailable");
    let export_label = ctx.t("settings.identity.export");
    let export_description = ctx.t("settings.identity.export.description");
    let remove_label = ctx.t("settings.identity.remove");
    let remove_description = ctx.t("settings.identity.remove.description");
    let reveal_action = ctx.t("settings.identity.action.reveal");
    let copy_action = ctx.t("settings.identity.action.copy");
    let copied_action = ctx.t("settings.identity.action.copied");
    let export_action = ctx.t("settings.identity.action.export");
    let remove_action = ctx.t("settings.identity.action.remove");
    let unavailable_action = ctx.t("settings.identity.action.unavailable");
    let public_id_copied = self.public_id_copied.get();
    let public_id_copied_signal = self.public_id_copied.clone();
    let public_id_to_copy = public_id.clone();
    let public_copy = copy_button(ctx, public_id_copied)
      .on_click(move |_| {
        if clipboard::copy_to_clipboard(&public_id_to_copy) {
          public_id_copied_signal.set(true);
        }
      })
      .into();
    let fingerprint_verified = verified_chip(ctx);
    let recovery_revealed = self.recovery_revealed.get();
    let recovery_copied = self.recovery_copied.get();
    let private_key_copied = self.private_key_copied.get();
    let recovery_subtitle = if recovery_phrase.is_none() {
      recovery_unavailable.to_string()
    } else if recovery_revealed {
      recovery_phrase.clone().unwrap_or_default()
    } else {
      recovery_description.to_string()
    };
    let recovery_mono = recovery_revealed && recovery_phrase.is_some();

    let reveal_button: Element = if let Some(phrase) = recovery_phrase.clone() {
      if recovery_revealed {
        let recovery_copied_signal = self.recovery_copied.clone();
        let (icon, label, tone) = if recovery_copied {
          ("check", copied_action.as_ref(), IdentityActionTone::Success)
        } else {
          ("copy", copy_action.as_ref(), IdentityActionTone::Neutral)
        };

        identity_action_button(ctx, icon, label, tone)
          .on_click(move |_| {
            if clipboard::copy_to_clipboard(&phrase) {
              recovery_copied_signal.set(true);
            }
          })
          .into()
      } else {
        let recovery_revealed_signal = self.recovery_revealed.clone();
        identity_action_button(ctx, "eye", &reveal_action, IdentityActionTone::Neutral)
          .on_click(move |_| recovery_revealed_signal.set(true))
          .into()
      }
    } else {
      identity_action_button(ctx, "eye-off", &unavailable_action, IdentityActionTone::Disabled).into()
    };

    let export_button: Element = if let Some(private_key) = private_key {
      let private_key_copied_signal = self.private_key_copied.clone();
      let (icon, label, tone) = if private_key_copied {
        ("check", copied_action.as_ref(), IdentityActionTone::Success)
      } else {
        ("key-round", export_action.as_ref(), IdentityActionTone::Neutral)
      };

      identity_action_button(ctx, icon, label, tone)
        .on_click(move |_| {
          if clipboard::copy_to_clipboard(&private_key) {
            private_key_copied_signal.set(true);
          }
        })
        .into()
    } else {
      identity_action_button(ctx, "key-round", &unavailable_action, IdentityActionTone::Disabled).into()
    };
    let remove_open_signal = self.remove_open.clone();
    let remove_button = identity_action_button(ctx, "trash-2", &remove_action, IdentityActionTone::Danger)
      .on_click(move |_| remove_open_signal.set(true))
      .into();

    let confirm_storage = storage.clone();
    let confirm_navigator = navigator.clone();
    let on_remove: ConfirmAction = Arc::new(move || {
      if let Some(storage) = confirm_storage.as_ref() {
        let _ = storage.delete_identity();
      }
      if let Some(navigator) = confirm_navigator.as_ref() {
        navigator.replace(ROUTE_IDENTITY_SETUP);
      }
    });
    let confirm_props = ConfirmModalProps {
      open: self.remove_open.clone(),
      icon: "trash-2",
      title: ctx.t("settings.identity.confirm_remove.title"),
      body: ctx.t("settings.identity.confirm_remove.body"),
      warning: Some(ctx.t("settings.identity.confirm_remove.warning")),
      cancel_label: ctx.t("common.action.cancel"),
      confirm_label: ctx.t("settings.identity.confirm_remove.confirm"),
      on_confirm: on_remove,
    };
    ctx.modal(self.remove_open.clone(), move |ctx| {
      ctx.mount::<ConfirmModal>(confirm_props)
    });

    let content = page_stack(ctx)
      .child(header(
        &ctx.t("settings.identity.title"),
        &ctx.t("settings.identity.description"),
      ))
      .child(identity_section_label(
        &ctx.t("settings.identity.section.public"),
        false,
      ))
      .child(
        Column::new()
          .width(Dimension::Pct(100.0))
          .child(ctx.mount::<DisplayNameSetting>(DisplayNameSettingProps {
            initial_value: self.display_name.clone(),
          }))
          .child(identity_row(&public_id_label, &public_id, true, public_copy, false))
          .child(identity_row(
            &fingerprint_label,
            &fingerprint,
            true,
            fingerprint_verified,
            false,
          )),
      )
      .child(identity_section_label(
        &ctx.t("settings.identity.section.backup"),
        false,
      ))
      .child(
        Column::new()
          .width(Dimension::Pct(100.0))
          .child(identity_row(
            &recovery_label,
            &recovery_subtitle,
            recovery_mono,
            reveal_button,
            false,
          ))
          .child(identity_row(
            &export_label,
            &export_description,
            false,
            export_button,
            false,
          )),
      )
      .child(identity_section_label(&ctx.t("settings.identity.section.danger"), true))
      .child(Column::new().width(Dimension::Pct(100.0)).child(identity_row(
        &remove_label,
        &remove_description,
        false,
        remove_button,
        true,
      )));

    screen(ctx, SettingsPage::Identity, content)
  }
}

#[derive(Clone, lurq::DevtoolsInspectable)]
struct DisplayNameSettingProps {
  initial_value: String,
}

impl PartialEq for DisplayNameSettingProps {
  fn eq(&self, other: &Self) -> bool {
    self.initial_value == other.initial_value
  }
}

struct DisplayNameSetting {
  value: Signal<String>,
}

impl Component for DisplayNameSetting {
  type Props = DisplayNameSettingProps;

  fn create(ctx: &mut Ctx) -> Self {
    Self {
      value: ctx.signal(ctx.props::<Self::Props>().initial_value.clone()),
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    identity_row(
      &ctx.t("settings.identity.display_name"),
      &ctx.t("settings.identity.display_name.description"),
      false,
      display_name_input(
        self.value.clone(),
        &ctx.t("settings.identity.display_name.placeholder"),
        ctx.use_context::<Storage>(),
      )
      .into(),
      false,
    )
  }
}

fn identity_row(title: &str, subtitle: &str, subtitle_mono: bool, trailing: Element, danger: bool) -> Element {
  Row::new()
    .width(Dimension::Pct(100.0))
    .align_items(Alignment::Center)
    .spacing(16.0)
    .padding_vertical(18.0)
    .border_bottom(Border::inside(
      1.0,
      if danger {
        theme::PaletteColor::Danger
      } else {
        theme::PaletteColor::Border
      },
    ))
    .child(
      Column::new()
        .flex(1.0)
        .spacing(theme::SpacingSize::Xs)
        .child(Text::styled(title, row_title_style()))
        .child(Text::styled(
          subtitle,
          if subtitle_mono {
            row_mono_subtitle_style()
          } else {
            row_subtitle_style()
          },
        )),
    )
    .child(trailing)
    .into()
}

fn identity_section_label(label: &str, danger: bool) -> Element {
  Row::new()
    .width(Dimension::Pct(100.0))
    .padding_bottom(2.0)
    .child(Text::styled(
      label,
      TextStyle {
        font_family: Arc::from("Inter"),
        font_size: 11.0,
        line_height: 1.2,
        weight: FontWeight::Bold,
        color: if danger {
          theme::palette().danger
        } else {
          theme::palette().text_muted
        },
        ..TextStyle::default()
      },
    ))
    .into()
}

fn copy_button(ctx: &mut Ctx, copied: bool) -> Row {
  let (icon, border, icon_color, hover_background) = if copied {
    (
      "check",
      BackgroundColor::Color(theme::palette().success.with_opacity(0.4)),
      theme::palette().success,
      BackgroundColor::Palette(theme::PaletteColor::SuccessMuted),
    )
  } else {
    (
      "copy",
      BackgroundColor::Color(Color::from_hex("#3A4047")),
      theme::palette().text_secondary,
      BackgroundColor::Palette(theme::PaletteColor::SurfaceInput),
    )
  };

  Row::new()
    .width(36.0)
    .height(36.0)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .rounded(theme::RadiusSize::Md)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised))
    .border_inside(1.0, border)
    .cursor(CursorIcon::Pointer)
    .hovered_style(Style::new().background(hover_background))
    .child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon,
      size: 16.0,
      color: icon_color,
    }))
}

fn display_name_input(value: Signal<String>, placeholder: &str, storage: Option<Storage>) -> Row {
  let mut placeholder_style = row_subtitle_style();
  placeholder_style.color = theme::palette().text_muted.with_opacity(0.55);
  let mut input = TextInput::styled(value.clone(), row_subtitle_style())
    .placeholder(placeholder)
    .placeholder_style(placeholder_style)
    .single_line()
    .flex(1.0)
    .background(BackgroundColor::Color(Color::from_hex("#00000000")))
    .caret_color(theme::PaletteColor::Accent);

  if let Some(storage) = storage {
    let value = value.clone();
    input = input.on_blur(move || {
      let mut settings = storage.load_settings().unwrap_or_default();
      settings.display_name = value.get_untracked();
      let _ = storage.save_settings(&settings);
    });
  }

  Row::new()
    .width(220.0)
    .height(36.0)
    .align_items(Alignment::Center)
    .padding_horizontal(theme::SpacingSize::Lg)
    .rounded(theme::RadiusSize::Md)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceInput))
    .border_inside(1.0, theme::PaletteColor::Border)
    .child(input)
}

fn verified_chip(ctx: &mut Ctx) -> Element {
  Row::new()
    .height(24.0)
    .align_items(Alignment::Center)
    .spacing(5.0)
    .padding_horizontal(9.0)
    .rounded(theme::RadiusSize::Md)
    .background(BackgroundColor::Palette(theme::PaletteColor::SuccessMuted))
    .border_inside(1.0, BackgroundColor::Color(theme::palette().success.with_opacity(0.4)))
    .child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon: "check",
      size: 12.0,
      color: theme::palette().success,
    }))
    .child(Text::styled(
      &ctx.t("settings.identity.status.verified"),
      TextStyle {
        font_family: Arc::from("Inter"),
        font_size: 10.0,
        line_height: 1.2,
        weight: FontWeight::Bold,
        color: theme::palette().success,
        ..TextStyle::default()
      },
    ))
    .into()
}

#[derive(Clone, Copy)]
enum IdentityActionTone {
  Neutral,
  Danger,
  Success,
  Disabled,
}

fn identity_action_button(ctx: &mut Ctx, icon: &'static str, label: &str, tone: IdentityActionTone) -> Row {
  let (background, border, text_color, icon_color, hover) = match tone {
    IdentityActionTone::Neutral => (
      BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised),
      BackgroundColor::Color(Color::from_hex("#3A4047")),
      theme::palette().text_primary,
      theme::palette().text_secondary,
      BackgroundColor::Palette(theme::PaletteColor::SurfaceInput),
    ),
    IdentityActionTone::Danger => (
      BackgroundColor::Color(Color::from_hex("#00000000")),
      BackgroundColor::Color(theme::palette().danger.with_opacity(0.5)),
      theme::palette().danger,
      theme::palette().danger,
      BackgroundColor::Palette(theme::PaletteColor::DangerMuted),
    ),
    IdentityActionTone::Success => (
      BackgroundColor::Color(Color::from_hex("#00000000")),
      BackgroundColor::Color(theme::palette().success.with_opacity(0.4)),
      theme::palette().success,
      theme::palette().success,
      BackgroundColor::Palette(theme::PaletteColor::SuccessMuted),
    ),
    IdentityActionTone::Disabled => (
      BackgroundColor::Palette(theme::PaletteColor::SurfaceInput),
      BackgroundColor::Palette(theme::PaletteColor::Border),
      theme::palette().text_muted,
      theme::palette().text_muted,
      BackgroundColor::Palette(theme::PaletteColor::SurfaceInput),
    ),
  };
  let enabled = !matches!(tone, IdentityActionTone::Disabled);

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
      .hovered_style(Style::new().background(hover.clone()))
      .active_style(Style::new().background(hover))
  } else {
    button
  };

  button
    .child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon,
      size: 16.0,
      color: icon_color,
    }))
    .child(Text::styled(
      label,
      TextStyle {
        font_family: Arc::from("Inter"),
        font_size: 13.0,
        line_height: 1.2,
        weight: FontWeight::Bold,
        color: text_color,
        ..TextStyle::default()
      },
    ))
}

fn row_title_style() -> TextStyle {
  TextStyle {
    font_family: Arc::from("Inter"),
    font_size: 14.0,
    line_height: 1.2,
    weight: FontWeight::Bold,
    color: theme::palette().text_primary,
    ..TextStyle::default()
  }
}

fn row_subtitle_style() -> TextStyle {
  TextStyle {
    font_family: Arc::from("Inter"),
    font_size: 13.0,
    line_height: 1.2,
    color: theme::palette().text_muted,
    ..TextStyle::default()
  }
}

fn row_mono_subtitle_style() -> TextStyle {
  TextStyle {
    font_family: Arc::from("JetBrains Mono"),
    font_size: 13.0,
    line_height: 1.2,
    color: theme::palette().text_muted,
    ..TextStyle::default()
  }
}

fn short_hex(bytes: &[u8]) -> String {
  let mut out = String::from("pk_");
  for byte in bytes.iter().take(8) {
    out.push(hex_char(byte >> 4));
    out.push(hex_char(byte & 0x0f));
  }
  out.push_str("...");
  out
}

fn hex_char(value: u8) -> char {
  match value {
    0..=9 => (b'0' + value) as char,
    10..=15 => (b'a' + value - 10) as char,
    _ => '?',
  }
}
