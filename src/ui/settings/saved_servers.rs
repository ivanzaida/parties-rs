use std::sync::Arc;

use lurq::{
  app::{component::Component, ctx::Ctx},
  clipboard,
  components::{Column, Rect, Row, Text},
  core::Signal,
  layout::{
    Alignment,
    layout_kind::Justify,
    text_style::{FontWeight, TextStyle},
  },
  node::{BackgroundColor, CursorIcon, Element, Style, color::Color, dimension::Dimension},
};

use crate::{
  network::protocol::Role,
  routes::ROUTE_CONNECT_SERVER,
  storage::{Storage, StoredServer},
  theme,
  ui::{
    common::{
      confirm_modal::{ConfirmAction, ConfirmModal, ConfirmModalProps},
      lucide_icon::{LucideIcon, LucideIconProps},
    },
    connect_server::ConnectOrigin,
    settings::shell::{SettingsPage, muted_notice, page_stack, screen, value_text},
  },
};

pub struct SettingsSavedServersScreen {
  menu_open: Signal<bool>,
  menu_address: Signal<Option<String>>,
  forget_open: Signal<bool>,
  forget_address: Signal<Option<String>>,
}

impl Component for SettingsSavedServersScreen {
  type Props = ();

  fn create(ctx: &mut Ctx) -> Self {
    Self {
      menu_open: ctx.signal(false),
      menu_address: ctx.signal(None::<String>),
      forget_open: ctx.signal(false),
      forget_address: ctx.signal(None::<String>),
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let storage = ctx.use_context::<Storage>();
    let servers = storage
      .as_ref()
      .and_then(|storage| storage.load_servers().ok())
      .unwrap_or_default();
    let count = servers.len();
    let menu_address = self.menu_address.get();
    let pending_address = self.forget_address.get();

    let mut list = Column::new()
      .width(Dimension::Pct(100.0))
      .spacing(theme::SpacingSize::Lg);

    if servers.is_empty() {
      list = list.child(empty_state(ctx).into());
    } else {
      for server in servers {
        list = list.child(server_row(
          ctx,
          &server,
          self.menu_open.clone(),
          self.menu_address.clone(),
        ));
      }
    }

    if let Some(address) = menu_address
      && let Some(server) = storage
        .as_ref()
        .and_then(|storage| storage.load_server(&address).ok())
        .flatten()
    {
      let menu = server_action_menu(
        ctx,
        &server,
        self.menu_open.clone(),
        self.forget_open.clone(),
        self.forget_address.clone(),
      );
      ctx.modal(self.menu_open.clone(), move |_| menu);
    }

    if let Some(address) = pending_address {
      let server_name = storage
        .as_ref()
        .and_then(|storage| storage.load_server(&address).ok())
        .flatten()
        .map(|server| display_name(&server).to_owned())
        .unwrap_or_else(|| address.clone());
      let confirm_storage = storage.clone();
      let confirm_address = address.clone();
      let on_confirm: ConfirmAction = std::sync::Arc::new(move || {
        if let Some(storage) = confirm_storage.as_ref() {
          let _ = storage.delete_server(&confirm_address);
        }
      });
      let props = ConfirmModalProps {
        open: self.forget_open.clone(),
        icon: "trash-2",
        title: ctx.t_args("settings.servers.confirm_forget.title", [("server", server_name)]),
        body: ctx.t("settings.servers.confirm_forget.body"),
        warning: None,
        cancel_label: ctx.t("common.action.cancel"),
        confirm_label: ctx.t("settings.servers.confirm_forget.confirm"),
        on_confirm,
      };
      ctx.modal(self.forget_open.clone(), move |ctx| ctx.mount::<ConfirmModal>(props));
    }

    let notice_title = ctx.t("settings.servers.notice.title");
    let notice_description = ctx.t_args("settings.servers.notice.description", [("count", count.to_string())]);
    let notice = muted_notice(ctx, &notice_title, &notice_description);
    let content = page_stack().child(servers_header(ctx)).child(list).child(notice);

    screen(ctx, SettingsPage::Servers, content)
  }
}

fn servers_header(ctx: &mut Ctx) -> impl Into<Element> {
  Row::new()
    .width(Dimension::Pct(100.0))
    .align_items(Alignment::End)
    .justify(Justify::SpaceBetween)
    .spacing(theme::SpacingSize::Lg)
    .child(
      Column::new()
        .flex(1.0)
        .spacing(theme::SpacingSize::Sm)
        .child(Text::new(&ctx.t("settings.servers.title")).variant(theme::TypographyStyle::Title))
        .child(
          Text::new(&ctx.t("settings.servers.description"))
            .variant(theme::TypographyStyle::Description)
            .width(Dimension::Pct(100.0)),
        ),
    )
    .child(add_server_button(ctx))
}

fn add_server_button(ctx: &mut Ctx) -> impl Into<Element> {
  let navigator = ctx.navigator();
  let mut button = Row::new()
    .height(34.0)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .spacing(theme::SpacingSize::Sm)
    .padding_horizontal(theme::SpacingSize::Lg)
    .rounded(theme::RadiusSize::Md)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised))
    .border_inside(1.0, theme::PaletteColor::BorderStrong)
    .cursor(CursorIcon::Pointer)
    .hovered_style(Style::new().background(BackgroundColor::Palette(theme::PaletteColor::SurfaceInput)))
    .active_style(Style::new().background(BackgroundColor::Palette(theme::PaletteColor::SurfaceInput)))
    .child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon: "plus",
      size: 16.0,
      color: theme::palette().text_secondary,
    }))
    .child(
      Text::new(&ctx.t("settings.servers.action.add"))
        .variant(theme::TypographyStyle::Button)
        .color(theme::PaletteColor::TextPrimary),
    );

  if let Some(navigator) = navigator {
    button = button.on_click(move |_| navigator.push_with_state(ROUTE_CONNECT_SERVER, ConnectOrigin::Settings));
  }

  button
}

fn empty_state(ctx: &mut Ctx) -> impl Into<Element> {
  Column::new()
    .width(Dimension::Pct(100.0))
    .align_items(Alignment::Center)
    .spacing(theme::SpacingSize::Lg)
    .padding_vertical(theme::SpacingSize::Section)
    .padding_horizontal(theme::SpacingSize::Xl)
    .rounded(theme::RadiusSize::Lg)
    .border_inside(1.0, theme::PaletteColor::Border)
    .child(
      Row::new()
        .width(64.0)
        .height(64.0)
        .align_items(Alignment::Center)
        .justify(Justify::Center)
        .rounded(theme::RadiusSize::Lg)
        .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised))
        .child(ctx.mount::<LucideIcon>(LucideIconProps {
          icon: "server",
          size: 28.0,
          color: theme::palette().text_secondary,
        })),
    )
    .child(Text::new(&ctx.t("settings.servers.empty")).variant(theme::TypographyStyle::Heading))
}

fn server_row(
  ctx: &mut Ctx,
  server: &StoredServer,
  menu_open: Signal<bool>,
  menu_address: Signal<Option<String>>,
) -> impl Into<Element> {
  let name = display_name(server);
  let address = server.address.clone();

  Row::new()
    .width(Dimension::Pct(100.0))
    .align_items(Alignment::Center)
    .spacing(theme::SpacingSize::Lg)
    .padding_vertical(theme::SpacingSize::Lg)
    .padding_horizontal(theme::SpacingSize::Xl)
    .rounded(theme::RadiusSize::Lg)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfacePanel))
    .border_inside(1.0, theme::PaletteColor::Border)
    .child(avatar(name))
    .child(
      Column::new()
        .flex(1.0)
        .spacing(theme::SpacingSize::Xs)
        .child(
          Row::new()
            .align_items(Alignment::Center)
            .spacing(theme::SpacingSize::Sm)
            .child(Text::new(name).variant(theme::TypographyStyle::Heading))
            .child(role_chip(role_label(server.role))),
        )
        .child(value_text(&server.address)),
    )
    .child({
      menu_trigger(ctx).on_click(move |_| {
        menu_address.set(Some(address.clone()));
        menu_open.set(true);
      })
    })
}

fn server_action_menu(
  ctx: &mut Ctx,
  server: &StoredServer,
  menu_open: Signal<bool>,
  forget_open: Signal<bool>,
  forget_address: Signal<Option<String>>,
) -> Element {
  let window = ctx.window();
  let address = server.address.clone();
  let copy_address = address.clone();
  let forget_address_value = address.clone();
  let close_connect = menu_open.clone();
  let close_copy = menu_open.clone();
  let close_fingerprint = menu_open.clone();
  let close_forget = menu_open.clone();
  let close_scrim = menu_open.clone();

  Column::new()
    .width(window.logical_width())
    .height(window.logical_height())
    .align_items(Alignment::End)
    .padding_top(196.0)
    .padding_right(34.0)
    .background(BackgroundColor::Color(Color::from_hex("#00000059")))
    .cursor(CursorIcon::Pointer)
    .on_click(move |_| close_scrim.set(false))
    .child(
      Column::new()
        .width(240.0)
        .spacing(2.0)
        .padding(6.0)
        .rounded(10.0)
        .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised))
        .border_inside(1.0, BackgroundColor::Color(Color::from_hex("#3A4047")))
        .child(
          menu_item(ctx, "plug", &ctx.t("settings.servers.menu.connect"), false, true)
            .on_click(move |_| close_connect.set(false)),
        )
        .child(
          menu_item(ctx, "copy", &ctx.t("settings.servers.menu.copy_address"), false, false).on_click(move |_| {
            let _ = clipboard::copy_to_clipboard(&copy_address);
            close_copy.set(false);
          }),
        )
        .child(
          menu_item(
            ctx,
            "shield-check",
            &ctx.t("settings.servers.menu.view_fingerprint"),
            false,
            false,
          )
          .on_click(move |_| close_fingerprint.set(false)),
        )
        .child(
          Rect::new(1.0, 1.0)
            .width(Dimension::Pct(100.0))
            .background(BackgroundColor::Palette(theme::PaletteColor::Border)),
        )
        .child(
          menu_item(ctx, "trash-2", &ctx.t("settings.servers.menu.forget"), true, false).on_click(move |_| {
            close_forget.set(false);
            forget_address.set(Some(forget_address_value.clone()));
            forget_open.set(true);
          }),
        ),
    )
    .into()
}

fn menu_trigger(ctx: &mut Ctx) -> Row {
  Row::new()
    .width(34.0)
    .height(34.0)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .rounded(theme::RadiusSize::Md)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised))
    .border_inside(1.0, theme::PaletteColor::BorderStrong)
    .cursor(CursorIcon::Pointer)
    .hovered_style(Style::new().background(BackgroundColor::Palette(theme::PaletteColor::SurfaceInput)))
    .active_style(Style::new().background(BackgroundColor::Palette(theme::PaletteColor::SurfaceInput)))
    .child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon: "ellipsis",
      size: 16.0,
      color: theme::palette().text_secondary,
    }))
}

fn menu_item(ctx: &mut Ctx, icon: &'static str, label: &str, danger: bool, active: bool) -> Row {
  let color = if danger {
    theme::palette().danger
  } else {
    theme::palette().text_primary
  };
  let icon_color = if danger {
    theme::palette().danger
  } else {
    theme::palette().text_secondary
  };

  Row::new()
    .width(Dimension::Pct(100.0))
    .align_items(Alignment::Center)
    .spacing(10.0)
    .padding_vertical(9.0)
    .padding_horizontal(10.0)
    .rounded(6.0)
    .background(if active {
      BackgroundColor::Color(Color::from_hex("#232830"))
    } else {
      BackgroundColor::Color(Color::from_hex("#00000000"))
    })
    .cursor(CursorIcon::Pointer)
    .hovered_style(Style::new().background(BackgroundColor::Color(Color::from_hex("#232830"))))
    .active_style(Style::new().background(BackgroundColor::Color(Color::from_hex("#232830"))))
    .child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon,
      size: 16.0,
      color: icon_color,
    }))
    .child(Text::styled(
      label,
      TextStyle {
        font_family: Arc::from("Inter"),
        font_size: 14.0,
        line_height: 1.2,
        weight: FontWeight::Medium,
        color,
        ..TextStyle::default()
      },
    ))
}

fn display_name(server: &StoredServer) -> &str {
  if server.server_name.trim().is_empty() {
    server.address.as_str()
  } else {
    server.server_name.as_str()
  }
}

fn avatar(name: &str) -> impl Into<Element> {
  let letter = name
    .chars()
    .find(|ch| ch.is_alphanumeric())
    .map(|ch| ch.to_uppercase().to_string())
    .unwrap_or_else(|| "?".to_owned());

  Row::new()
    .width(48.0)
    .height(48.0)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .rounded(theme::RadiusSize::Lg)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised))
    .child(Text::new(&letter).variant(theme::TypographyStyle::Heading))
}

fn role_chip(label: &str) -> impl Into<Element> {
  Row::new()
    .height(22.0)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .padding_horizontal(theme::SpacingSize::Sm)
    .rounded(theme::RadiusSize::Md)
    .border_inside(1.0, theme::PaletteColor::BorderStrong)
    .child(
      Text::new(label)
        .variant(theme::TypographyStyle::Caption)
        .color(theme::PaletteColor::TextSecondary),
    )
}

fn role_label(role: Role) -> &'static str {
  match role {
    Role::Owner | Role::Admin => "ADMIN",
    Role::Moderator => "MOD",
    Role::User => "MEMBER",
  }
}
