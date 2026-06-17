use std::sync::Arc;

use lurq::{
  app::{
    component::{Component, ComponentInfo, DevtoolsFormatter, DevtoolsInspectable},
    ctx::{Ctx, Modal, Root},
    events::MouseEvent,
  },
  clipboard,
  components::{Column, Rect, Row, ScrollVertical, Text, TextInput},
  core::Signal,
  layout::{
    Alignment,
    layout_kind::Justify,
    scrollbar::{ScrollBarPlacement, ScrollBarStyle},
    text_style::{FontWeight, TextStyle},
  },
  node::{BackgroundColor, CursorIcon, Element, Style, color::Color, dimension::Dimension},
};

use crate::{
  network::protocol::Role,
  routes::ROUTE_CONNECT_SERVER,
  session::ConnectedServerInfo,
  storage::{Storage, StoredServer},
  theme,
  ui::{
    app_chrome::{content_height, modal_y},
    common::{
      confirm_modal::{ConfirmAction, ConfirmModal, ConfirmModalProps},
      lucide_icon::{LucideIcon, LucideIconProps},
    },
    connect_server::{ConnectErrorCopy, ConnectOrigin, ConnectServerRouteState, test_connection},
    settings::shell::{SettingsPage, SettingsPopupHandle, muted_notice, page_stack, screen, value_text},
  },
};

const VISIBLE_SERVER_COUNT: usize = 5;
const SERVER_LIST_MAX_HEIGHT: f32 = 440.0;

pub struct SettingsSavedServersScreen {
  menu_open: Signal<bool>,
  menu_address: Signal<Option<String>>,
  menu_anchor_y: Signal<Option<f32>>,
  forget_open: Signal<bool>,
  forget_address: Signal<Option<String>>,
  fingerprint_open: Signal<bool>,
  fingerprint_address: Signal<Option<String>>,
  edit_open: Signal<bool>,
  edit_address: Signal<Option<String>>,
}

impl Component for SettingsSavedServersScreen {
  type Props = ();

  fn create(ctx: &mut Ctx) -> Self {
    Self {
      menu_open: ctx.signal(false),
      menu_address: ctx.signal(None::<String>),
      menu_anchor_y: ctx.signal(None::<f32>),
      forget_open: ctx.signal(false),
      forget_address: ctx.signal(None::<String>),
      fingerprint_open: ctx.signal(false),
      fingerprint_address: ctx.signal(None::<String>),
      edit_open: ctx.signal(false),
      edit_address: ctx.signal(None::<String>),
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
    let fingerprint_address = self.fingerprint_address.get();
    let edit_address = self.edit_address.get();

    let mut list = Column::new()
      .width(Dimension::Pct(100.0))
      .spacing(theme::SpacingSize::Lg)
      .padding_right(16.0);

    if servers.is_empty() {
      list = list.child(empty_state(ctx).into());
    } else {
      for server in servers {
        list = list.child(server_row(
          ctx,
          &server,
          self.menu_open.clone(),
          self.menu_address.clone(),
          self.menu_anchor_y.clone(),
        ));
      }
    }

    let mut modals: Vec<Element> = Vec::new();

    if let Some(address) = menu_address
      && let Some(server) = storage
        .as_ref()
        .and_then(|storage| storage.load_server(&address).ok())
        .flatten()
    {
      let menu = server_action_menu(
        ctx,
        &server,
        self.menu_anchor_y.get(),
        self.menu_open.clone(),
        self.fingerprint_open.clone(),
        self.fingerprint_address.clone(),
        self.forget_open.clone(),
        self.forget_address.clone(),
        self.edit_open.clone(),
        self.edit_address.clone(),
      );
      modals.push(Modal::new(menu).open(self.menu_open.clone()).target(Root).into());
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
      modals.push(
        Modal::new(ctx.mount::<ConfirmModal>(props))
          .open(self.forget_open.clone())
          .target(Root)
          .into(),
      );
    }

    if let Some(address) = fingerprint_address
      && let Some(server) = storage
        .as_ref()
        .and_then(|storage| storage.load_server(&address).ok())
        .flatten()
    {
      let open = self.fingerprint_open.clone();
      modals.push(
        Modal::new(fingerprint_modal(ctx, &server, open.clone()))
          .open(self.fingerprint_open.clone())
          .target(Root)
          .into(),
      );
    }

    if let Some(address) = edit_address
      && let Some(server) = storage
        .as_ref()
        .and_then(|storage| storage.load_server(&address).ok())
        .flatten()
    {
      let props = EditSavedServerModalProps {
        open: self.edit_open.clone(),
        server,
      };
      modals.push(
        Modal::new(ctx.mount::<EditSavedServerModal>(props))
          .open(self.edit_open.clone())
          .target(Root)
          .into(),
      );
    }

    let notice_title = ctx.t("settings.servers.notice.title");
    let notice_description = ctx.t_args("settings.servers.notice.description", [("count", count.to_string())]);
    let notice = muted_notice(ctx, &notice_title, &notice_description);
    let mut content = page_stack(ctx)
      .child(servers_header(ctx))
      .child(server_list_view(list, count))
      .child(notice);
    for modal in modals {
      content = content.child(modal);
    }

    screen(ctx, SettingsPage::Servers, content)
  }
}

fn server_list_view(list: Column, count: usize) -> Element {
  if count <= VISIBLE_SERVER_COUNT {
    return list.into();
  }

  ScrollVertical::new(list)
    .width(Dimension::Pct(100.0))
    .height(SERVER_LIST_MAX_HEIGHT)
    .scrollbar(server_list_scrollbar_style())
    .scrollbar_hovered(|mut style| {
      let palette = theme::palette();
      style.thumb_color = palette.accent_hover;
      style.track_color = palette.surface_input.with_opacity(0.75);
      style
    })
    .into()
}

fn server_list_scrollbar_style() -> ScrollBarStyle {
  let palette = theme::palette();
  ScrollBarStyle {
    width: 8.0,
    min_thumb_length: 32.0,
    track_color: palette.surface_input.with_opacity(0.55),
    thumb_color: palette.accent,
    thumb_radius: 4.0,
    track_radius: 4.0,
    padding: 0.0,
    placement: ScrollBarPlacement::Reserved,
    ..ScrollBarStyle::default()
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
  let settings_popup = ctx
    .use_context::<SettingsPopupHandle>()
    .filter(SettingsPopupHandle::is_open);
  let mut button = Row::new()
    .height(34.0)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .spacing(theme::SpacingSize::Sm)
    .padding_horizontal(theme::SpacingSize::Lg)
    .rounded(theme::RadiusSize::Md)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised))
    .border_inside(1.0, theme::PaletteColor::Accent)
    .cursor(CursorIcon::Pointer)
    .hovered_style(Style::new().background(BackgroundColor::Palette(theme::PaletteColor::AccentMuted)))
    .active_style(Style::new().background(BackgroundColor::Palette(theme::PaletteColor::AccentMuted)))
    .child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon: "plus",
      size: 16.0,
      color: theme::palette().accent,
    }))
    .child(
      Text::new(&ctx.t("settings.servers.action.add"))
        .variant(theme::TypographyStyle::Button)
        .color(theme::PaletteColor::TextPrimary),
    );

  if let Some(navigator) = navigator {
    button = button.on_click(move |_| {
      let origin = if settings_popup.as_ref().is_some() {
        ConnectOrigin::SettingsPopup
      } else {
        ConnectOrigin::Settings
      };
      navigator.push_with_state(ROUTE_CONNECT_SERVER, ConnectServerRouteState::new(origin));
      if let Some(settings_popup) = settings_popup.as_ref() {
        settings_popup.close();
      }
    });
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
  menu_anchor_y: Signal<Option<f32>>,
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
      let scale = ctx.window().scale_factor.max(f32::EPSILON);
      menu_trigger(ctx).on_click(move |event: MouseEvent| {
        menu_address.set(Some(address.clone()));
        menu_anchor_y.set(Some(event.y / scale));
        menu_open.set(true);
      })
    })
}

fn server_action_menu(
  ctx: &mut Ctx,
  server: &StoredServer,
  anchor_y: Option<f32>,
  menu_open: Signal<bool>,
  fingerprint_open: Signal<bool>,
  fingerprint_address: Signal<Option<String>>,
  forget_open: Signal<bool>,
  forget_address: Signal<Option<String>>,
  edit_open: Signal<bool>,
  edit_address: Signal<Option<String>>,
) -> Element {
  let window = ctx.window();
  let window_width = window.logical_width();
  let modal_height = content_height(ctx);
  let menu_top = server_action_menu_top(anchor_y.map(modal_y), modal_height);
  let address = server.address.clone();
  let edit_address_value = address.clone();
  let copy_address = address.clone();
  let fingerprint_address_value = address.clone();
  let forget_address_value = address.clone();
  let close_edit = menu_open.clone();
  let close_copy = menu_open.clone();
  let close_fingerprint = menu_open.clone();
  let close_forget = menu_open.clone();
  let close_scrim = menu_open.clone();

  Column::new()
    .width(window_width)
    .height(modal_height)
    .absolute(0.0, 0.0, window_width, modal_height)
    .align_items(Alignment::End)
    .padding_top(menu_top)
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
          menu_item(ctx, "settings", &ctx.t("settings.servers.menu.edit"), false, true).on_click(move |_| {
            close_edit.set(false);
            edit_address.set(Some(edit_address_value.clone()));
            edit_open.set(true);
          }),
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
          .on_click(move |_| {
            close_fingerprint.set(false);
            fingerprint_address.set(Some(fingerprint_address_value.clone()));
            fingerprint_open.set(true);
          }),
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

fn server_action_menu_top(anchor_y: Option<f32>, window_height: f32) -> f32 {
  const FALLBACK_TOP: f32 = 196.0;
  const MENU_HEIGHT: f32 = 174.0;
  const EDGE_PADDING: f32 = 16.0;
  const TRIGGER_OFFSET: f32 = 10.0;

  let top = anchor_y.map(|y| y - TRIGGER_OFFSET).unwrap_or(FALLBACK_TOP);
  let max_top = (window_height - MENU_HEIGHT - EDGE_PADDING).max(EDGE_PADDING);
  top.clamp(EDGE_PADDING, max_top)
}

type EditServerInput = (String, String, String);
type EditServerTestAction = lurq::app::ctx::FutureAction<EditServerInput, ConnectedServerInfo, String>;
type EditServerSaveAction = lurq::app::ctx::FutureAction<EditServerInput, (), String>;

#[derive(Clone)]
struct EditSavedServerModalProps {
  open: Signal<bool>,
  server: StoredServer,
}

impl PartialEq for EditSavedServerModalProps {
  fn eq(&self, other: &Self) -> bool {
    self.server == other.server
  }
}

impl DevtoolsInspectable for EditSavedServerModalProps {
  fn inspect(&self, formatter: &mut DevtoolsFormatter<'_>) {
    formatter.buffer_mut().push(ComponentInfo::with_value(
      "server",
      std::any::type_name::<String>(),
      self.server.address.clone(),
    ));
  }
}

struct EditSavedServerModal {
  loaded_address: Signal<String>,
  address: Signal<String>,
  seed: Signal<String>,
  display_name: Signal<String>,
}

impl Component for EditSavedServerModal {
  type Props = EditSavedServerModalProps;

  fn create(ctx: &mut Ctx) -> Self {
    let props = ctx.props::<Self::Props>().clone();
    Self {
      loaded_address: ctx.signal(props.server.address.clone()),
      address: ctx.signal(props.server.address),
      seed: ctx.signal(props.server.server_password),
      display_name: ctx.signal(props.server.display_name),
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let props = ctx.props::<Self::Props>().clone();
    if self.loaded_address.get_untracked() != props.server.address {
      self.loaded_address.set(props.server.address.clone());
      self.address.set(props.server.address.clone());
      self.seed.set(props.server.server_password.clone());
      self.display_name.set(props.server.display_name.clone());
    }

    let test = edit_server_test_action(ctx);
    let save = edit_server_save_action(ctx, props.server.clone());
    let test_state = test.state().get();
    let save_state = save.state().get();
    if save_state.data.is_some() {
      props.open.set(false);
    }
    let error = save_state.error.as_deref().or(test_state.error.as_deref());
    let test_result = test_state.data.clone();

    edit_server_modal(
      ctx,
      props.open,
      self.address.clone(),
      self.seed.clone(),
      self.display_name.clone(),
      &test,
      &save,
      test_state.is_pending(),
      save_state.is_pending(),
      error,
      test_result.as_ref(),
    )
  }
}

fn edit_server_test_action(ctx: &mut Ctx) -> EditServerTestAction {
  let storage = ctx.use_context::<Storage>();
  let errors = ConnectErrorCopy::from_ctx(ctx);
  ctx.future_action(move |(address, seed, display_name): EditServerInput| {
    let storage = storage.clone();
    let errors = errors.clone();
    async move {
      let info = test_connection(address, seed, display_name, storage, errors).await?;
      Ok(info)
    }
  })
}

fn edit_server_save_action(ctx: &mut Ctx, server: StoredServer) -> EditServerSaveAction {
  let storage = ctx.use_context::<Storage>();
  let storage_unavailable = ctx.t("connect_server.error.storage_unavailable").to_string();
  ctx.future_action(move |(address, seed, display_name): EditServerInput| {
    let storage = storage.clone();
    let server = server.clone();
    let storage_unavailable = storage_unavailable.clone();
    async move {
      let storage = storage.ok_or(storage_unavailable)?;
      let old_address = server.address.clone();
      let new_address = address.trim().to_owned();
      storage
        .save_server(&StoredServer {
          address: new_address.clone(),
          server_name: server.server_name,
          user_id: server.user_id,
          role: server.role,
          certificate_fingerprint: server.certificate_fingerprint,
          server_password: seed,
          display_name: display_name.trim().to_owned(),
        })
        .map_err(|error| error.to_string())?;
      if old_address != new_address {
        storage.delete_server(&old_address).map_err(|error| error.to_string())?;
      }
      Ok(())
    }
  })
}

fn edit_server_modal(
  ctx: &mut Ctx,
  open: Signal<bool>,
  address: Signal<String>,
  seed: Signal<String>,
  display_name: Signal<String>,
  test: &EditServerTestAction,
  save: &EditServerSaveAction,
  testing: bool,
  saving: bool,
  error: Option<&str>,
  test_result: Option<&ConnectedServerInfo>,
) -> Element {
  let window = ctx.window();
  let window_width = window.logical_width();
  let modal_height = content_height(ctx);
  let panel_width = (window_width - 32.0).min(600.0).max(320.0);
  let busy = testing || saving;
  let can_submit = !address.get().trim().is_empty() && !display_name.get().trim().is_empty() && !busy;
  let close = open.clone();
  let test_submit = test.clone();
  let test_address = address.clone();
  let test_seed = seed.clone();
  let test_display_name = display_name.clone();
  let save_submit = save.clone();
  let save_address = address.clone();
  let save_seed = seed.clone();
  let save_display_name = display_name.clone();
  let mut panel = Column::new()
    .width(panel_width)
    .spacing(18.0)
    .padding(28.0)
    .rounded(12.0)
    .background(BackgroundColor::Color(Color::from_hex("#15171A")))
    .border_inside(1.0, BackgroundColor::Color(Color::from_hex("#3A4047")))
    .child(
      Column::new()
        .width(Dimension::Pct(100.0))
        .spacing(6.0)
        .child(Text::new(&ctx.t("settings.servers.edit.title")).variant(theme::TypographyStyle::Title))
        .child(
          Text::new(&ctx.t("settings.servers.edit.description"))
            .variant(theme::TypographyStyle::Description)
            .color(theme::PaletteColor::TextSecondary)
            .width(Dimension::Pct(100.0)),
        ),
    )
    .child(edit_server_field(
      ctx,
      &ctx.t("connect_server.address.label"),
      address.clone(),
      &ctx.t("connect_server.address.placeholder"),
      "globe",
      "edit_server_address",
      1,
    ))
    .child(edit_server_field(
      ctx,
      &ctx.t("connect_server.display_name.label"),
      display_name.clone(),
      &ctx.t("connect_server.display_name.placeholder"),
      "user",
      "edit_server_display_name",
      2,
    ))
    .child(edit_server_field(
      ctx,
      &ctx.t("connect_server.seed.label"),
      seed.clone(),
      &ctx.t("connect_server.seed.placeholder"),
      "eye",
      "edit_server_seed",
      3,
    ));

  if testing {
    panel = panel.child(testing_banner(ctx, &address.get()));
  } else if let Some(error) = error {
    panel = panel.child(edit_error_banner(ctx, error));
  } else if let Some(info) = test_result {
    panel = panel.child(edit_connection_info_banner(ctx, info));
  }

  panel = panel.child(
    Row::new()
      .width(Dimension::Pct(100.0))
      .align_items(Alignment::Center)
      .justify(Justify::SpaceBetween)
      .child(
        edit_modal_button(&ctx.t("common.action.cancel"), false, true).on_click(move |_| {
          close.set(false);
        }),
      )
      .child(
        Row::new()
          .align_items(Alignment::Center)
          .justify(Justify::End)
          .spacing(theme::SpacingSize::Sm)
          .child(
            edit_modal_button(
              &ctx.t(if testing {
                "settings.servers.edit.testing"
              } else {
                "settings.servers.edit.test"
              }),
              false,
              can_submit,
            )
            .on_click(move |_| {
              if can_submit {
                test_submit.run((
                  test_address.get_untracked(),
                  test_seed.get_untracked(),
                  test_display_name.get_untracked(),
                ));
              }
            }),
          )
          .child(
            edit_modal_button(
              &ctx.t(if saving {
                "settings.servers.edit.saving"
              } else {
                "settings.servers.edit.save"
              }),
              true,
              can_submit,
            )
            .on_click(move |_| {
              if can_submit {
                save_submit.run((
                  save_address.get_untracked(),
                  save_seed.get_untracked(),
                  save_display_name.get_untracked(),
                ));
              }
            }),
          ),
      ),
  );

  Row::new()
    .width(window_width)
    .height(modal_height)
    .absolute(0.0, 0.0, window_width, modal_height)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .background(BackgroundColor::Color(Color::from_hex("#00000099")))
    .child(panel)
    .into()
}

fn edit_server_field(
  ctx: &mut Ctx,
  label: &str,
  value: Signal<String>,
  placeholder: &str,
  icon: &'static str,
  name: &'static str,
  tab_index: i32,
) -> Element {
  Column::new()
    .width(Dimension::Pct(100.0))
    .spacing(8.0)
    .child(
      Text::new(label)
        .variant(theme::TypographyStyle::FieldLabel)
        .color(theme::PaletteColor::TextMuted),
    )
    .child(edit_input_box(ctx, value, placeholder, icon, name, tab_index))
    .into()
}

fn edit_input_box(
  ctx: &mut Ctx,
  value: Signal<String>,
  placeholder: &str,
  icon: &'static str,
  name: &'static str,
  tab_index: i32,
) -> Row {
  let text_style = ctx.theme().typography().mono.clone();
  let mut placeholder_style = text_style.clone();
  placeholder_style.color = theme::palette().text_muted.with_opacity(0.55);

  Row::new()
    .width(Dimension::Pct(100.0))
    .height(46.0)
    .align_items(Alignment::Center)
    .rounded(theme::RadiusSize::Md)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceInput))
    .border_inside(1.0, theme::PaletteColor::Border)
    .child(
      TextInput::styled(value, text_style)
        .placeholder(placeholder)
        .placeholder_style(placeholder_style)
        .single_line()
        .name(name)
        .flex(1.0)
        .height(Dimension::Pct(100.0))
        .padding_left(theme::SpacingSize::Lg)
        .padding_right(theme::SpacingSize::Sm)
        .tab_index(tab_index)
        .background(BackgroundColor::Color(Color::from_hex("#00000000")))
        .caret_color(theme::PaletteColor::Accent),
    )
    .child(
      Row::new()
        .height(Dimension::Pct(100.0))
        .align_items(Alignment::Center)
        .justify(Justify::Center)
        .padding_left(theme::SpacingSize::Sm)
        .padding_right(theme::SpacingSize::Lg)
        .child(ctx.mount::<LucideIcon>(LucideIconProps {
          icon,
          size: 16.0,
          color: theme::palette().text_muted,
        })),
    )
}

fn testing_banner(ctx: &mut Ctx, address: &str) -> Element {
  let host = address.split(':').next().unwrap_or(address).to_owned();
  Row::new()
    .width(Dimension::Pct(100.0))
    .align_items(Alignment::Center)
    .spacing(theme::SpacingSize::Md)
    .padding_vertical(theme::SpacingSize::Lg)
    .padding_horizontal(16.0)
    .rounded(theme::RadiusSize::Lg)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceInput))
    .border_inside(1.0, theme::PaletteColor::Border)
    .child(crate::ui::loader::loader(16.0))
    .child(
      Text::new(&ctx.t_args("connect_server.authenticating", [("host", host)]))
        .variant(theme::TypographyStyle::Description)
        .color(theme::PaletteColor::TextSecondary)
        .width(Dimension::Pct(100.0))
        .flex(1.0),
    )
    .into()
}

fn edit_error_banner(ctx: &mut Ctx, message: &str) -> Element {
  Row::new()
    .width(Dimension::Pct(100.0))
    .align_items(Alignment::Center)
    .spacing(theme::SpacingSize::Md)
    .padding_vertical(theme::SpacingSize::Lg)
    .padding_horizontal(16.0)
    .rounded(theme::RadiusSize::Lg)
    .background(BackgroundColor::Palette(theme::PaletteColor::DangerMuted))
    .border_inside(1.0, theme::PaletteColor::Danger)
    .child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon: "triangle-alert",
      size: 16.0,
      color: theme::palette().danger,
    }))
    .child(
      Text::new(message)
        .variant(theme::TypographyStyle::Description)
        .color(theme::PaletteColor::Danger)
        .width(Dimension::Pct(100.0))
        .flex(1.0),
    )
    .into()
}

fn edit_connection_info_banner(ctx: &mut Ctx, info: &ConnectedServerInfo) -> Element {
  let fingerprint = if info.certificate_fingerprint.trim().is_empty() {
    ctx.t("settings.servers.edit.fingerprint_empty").to_string()
  } else {
    ctx
      .t_args(
        "settings.servers.edit.fingerprint",
        [("fingerprint", info.certificate_fingerprint.clone())],
      )
      .to_string()
  };

  Row::new()
    .width(Dimension::Pct(100.0))
    .align_items(Alignment::Start)
    .spacing(theme::SpacingSize::Md)
    .padding_vertical(theme::SpacingSize::Lg)
    .padding_horizontal(16.0)
    .rounded(theme::RadiusSize::Lg)
    .background(BackgroundColor::Palette(theme::PaletteColor::SuccessMuted))
    .border_inside(1.0, BackgroundColor::Color(theme::palette().success.with_opacity(0.4)))
    .child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon: "circle-check",
      size: 16.0,
      color: theme::palette().success,
    }))
    .child(
      Column::new()
        .flex(1.0)
        .spacing(4.0)
        .child(
          Text::new(&ctx.t("settings.servers.edit.connected"))
            .variant(theme::TypographyStyle::Description)
            .color(theme::PaletteColor::TextPrimary)
            .width(Dimension::Pct(100.0)),
        )
        .child(
          Text::new(&ctx.t_args(
            "settings.servers.edit.connected_body",
            [
              ("server", info.server_name.clone()),
              ("address", info.address.clone()),
              ("user", info.user_id.to_string()),
              ("role", role_label(info.role).to_owned()),
            ],
          ))
          .variant(theme::TypographyStyle::Description)
          .color(theme::PaletteColor::TextSecondary)
          .width(Dimension::Pct(100.0)),
        )
        .child(
          Text::new(&fingerprint)
            .variant(theme::TypographyStyle::Description)
            .color(theme::PaletteColor::TextSecondary)
            .width(Dimension::Pct(100.0)),
        ),
    )
    .into()
}

fn edit_modal_button(label: &str, primary: bool, enabled: bool) -> Row {
  let palette = theme::palette();
  let (background, border, text_color, hover) = if primary {
    (
      BackgroundColor::Palette(theme::PaletteColor::Accent),
      BackgroundColor::Palette(theme::PaletteColor::Accent),
      palette.text_inverse,
      BackgroundColor::Palette(theme::PaletteColor::AccentHover),
    )
  } else {
    (
      BackgroundColor::Color(Color::from_hex("#00000000")),
      BackgroundColor::Palette(theme::PaletteColor::Border),
      palette.text_secondary,
      BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised),
    )
  };
  let mut button = Row::new()
    .height(34.0)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .padding_horizontal(16.0)
    .rounded(theme::RadiusSize::Md)
    .background(background)
    .border_inside(1.0, border)
    .child(
      Text::new(label)
        .variant(theme::TypographyStyle::Button)
        .color(text_color),
    );

  if enabled {
    button = button
      .cursor(CursorIcon::Pointer)
      .hovered_style(Style::new().background(hover.clone()))
      .active_style(Style::new().background(hover));
  }

  button
}

fn fingerprint_modal(ctx: &mut Ctx, server: &StoredServer, open: Signal<bool>) -> Element {
  let window = ctx.window();
  let window_width = window.logical_width();
  let modal_height = content_height(ctx);
  let panel_width = (window_width - 32.0).min(480.0).max(300.0);
  let fingerprint = server.certificate_fingerprint.trim().to_owned();
  let display_fingerprint = if fingerprint.is_empty() {
    ctx.t("settings.servers.fingerprint.empty").to_string()
  } else {
    fingerprint.clone()
  };
  let copy_fingerprint = fingerprint.clone();
  let close_signal = open.clone();

  Column::new()
    .width(window_width)
    .height(modal_height)
    .absolute(0.0, 0.0, window_width, modal_height)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .background(BackgroundColor::Color(Color::from_hex("#00000099")))
    .child(
      Column::new()
        .width(panel_width)
        .spacing(16.0)
        .padding(24.0)
        .rounded(12.0)
        .background(BackgroundColor::Palette(theme::PaletteColor::SurfacePanel))
        .border_inside(1.0, BackgroundColor::Color(Color::from_hex("#3A4047")))
        .child(
          Row::new()
            .width(44.0)
            .height(44.0)
            .align_items(Alignment::Center)
            .justify(Justify::Center)
            .rounded(10.0)
            .background(BackgroundColor::Palette(theme::PaletteColor::AccentMuted))
            .child(ctx.mount::<LucideIcon>(LucideIconProps {
              icon: "shield-check",
              size: 20.0,
              color: theme::palette().accent,
            })),
        )
        .child(
          Column::new()
            .width(Dimension::Pct(100.0))
            .spacing(8.0)
            .child(Text::styled(
              &ctx.t("settings.servers.fingerprint.title"),
              fingerprint_title_style(),
            ))
            .child(Text::styled(
              &ctx.t_args(
                "settings.servers.fingerprint.description",
                [("server", display_name(server).to_owned())],
              ),
              fingerprint_body_style(),
            )),
        )
        .child(
          Column::new()
            .width(Dimension::Pct(100.0))
            .spacing(8.0)
            .padding(14.0)
            .rounded(8.0)
            .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceInput))
            .border_inside(1.0, theme::PaletteColor::Border)
            .child(Text::styled(&display_fingerprint, fingerprint_value_style()).width(Dimension::Pct(100.0))),
        )
        .child(
          Row::new()
            .width(Dimension::Pct(100.0))
            .align_items(Alignment::Center)
            .justify(Justify::End)
            .spacing(10.0)
            .child(
              fingerprint_modal_button(ctx, None, &ctx.t("settings.servers.fingerprint.close"), false)
                .on_click(move |_| close_signal.set(false)),
            )
            .child(
              fingerprint_modal_button(ctx, Some("copy"), &ctx.t("settings.servers.fingerprint.copy"), true).on_click(
                move |_| {
                  if !copy_fingerprint.is_empty() {
                    let _ = clipboard::copy_to_clipboard(&copy_fingerprint);
                  }
                },
              ),
            ),
        ),
    )
    .into()
}

fn fingerprint_modal_button(ctx: &mut Ctx, icon: Option<&'static str>, label: &str, primary: bool) -> Row {
  let (background, border, text_color, icon_color, hover) = if primary {
    (
      BackgroundColor::Palette(theme::PaletteColor::Accent),
      BackgroundColor::Palette(theme::PaletteColor::Accent),
      theme::palette().text_inverse,
      theme::palette().text_inverse,
      BackgroundColor::Palette(theme::PaletteColor::AccentHover),
    )
  } else {
    (
      BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised),
      BackgroundColor::Palette(theme::PaletteColor::Border),
      theme::palette().text_primary,
      theme::palette().text_secondary,
      BackgroundColor::Palette(theme::PaletteColor::SurfaceInput),
    )
  };

  let mut button = Row::new()
    .height(34.0)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .spacing(7.0)
    .padding_horizontal(14.0)
    .rounded(theme::RadiusSize::Md)
    .background(background)
    .border_inside(1.0, border)
    .cursor(CursorIcon::Pointer)
    .hovered_style(Style::new().background(hover.clone()))
    .active_style(Style::new().background(hover));

  if let Some(icon) = icon {
    button = button.child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon,
      size: 16.0,
      color: icon_color,
    }));
  }

  button.child(Text::styled(label, fingerprint_button_style(text_color)))
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

fn fingerprint_title_style() -> TextStyle {
  TextStyle {
    font_family: Arc::from("Inter"),
    font_size: 18.0,
    line_height: 1.2,
    weight: FontWeight::Bold,
    color: theme::palette().text_primary,
    ..TextStyle::default()
  }
}

fn fingerprint_body_style() -> TextStyle {
  TextStyle {
    font_family: Arc::from("Inter"),
    font_size: 14.0,
    line_height: 1.45,
    color: theme::palette().text_secondary,
    ..TextStyle::default()
  }
}

fn fingerprint_value_style() -> TextStyle {
  TextStyle {
    font_family: Arc::from("JetBrains Mono"),
    font_size: 12.0,
    line_height: 1.45,
    color: theme::palette().text_primary,
    ..TextStyle::default()
  }
}

fn fingerprint_button_style(color: Color) -> TextStyle {
  TextStyle {
    font_family: Arc::from("Inter"),
    font_size: 13.0,
    line_height: 1.2,
    weight: FontWeight::Bold,
    color,
    ..TextStyle::default()
  }
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
