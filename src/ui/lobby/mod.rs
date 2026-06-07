use lurq::{
  app::{component::Component, ctx::Ctx},
  components::{Column, Row, Text},
  layout::{Alignment, layout_kind::Justify},
  node::{BackgroundColor, CursorIcon, Element, Style, dimension::Dimension},
};

use crate::{
  routes::ROUTE_CHOOSE_SERVER,
  session::{ConnectedServerInfo, LobbyChannel, LobbyState, LobbyUser, ServerSession},
  theme,
  ui::{
    common::lucide_icon::{LucideIcon, LucideIconProps},
    loader::loader,
  },
};

mod rail;

use rail::{LobbyRail, LobbyRailProps, role_label_lower, server_avatar};

pub struct LobbyScreen;

type ReceiverAction = lurq::app::ctx::FutureAction<(), (), String>;

impl Component for LobbyScreen {
  type Props = ();

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let Some(session) = ctx.use_context::<ServerSession>() else {
      return empty_lobby(ctx);
    };

    let _revision = session.revision().get();
    let Some(info) = session.info() else {
      if let Some(navigator) = ctx.navigator() {
        navigator.replace(ROUTE_CHOOSE_SERVER);
      }
      return empty_lobby(ctx);
    };

    let lobby = session.lobby();
    let receiver = receiver_action(ctx, session.clone());
    if !lobby.disconnected && !lobby.receiver_running && !receiver.state().get().is_pending() {
      receiver.run(());
    }

    Row::new()
      .width(Dimension::Pct(100.0))
      .height(Dimension::Pct(100.0))
      .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceBase))
      .clip()
      .child(ctx.mount::<LobbyRail>(LobbyRailProps {
        info: info.clone(),
        lobby: lobby.clone(),
      }))
      .child(main(ctx, &info, &lobby))
      .into()
  }
}

fn receiver_action(ctx: &mut Ctx, session: ServerSession) -> ReceiverAction {
  ctx.future_action(move |()| {
    let session = session.clone();
    async move {
      session.run_lobby_receiver().await;
      Ok(())
    }
  })
}

fn main(ctx: &mut Ctx, info: &ConnectedServerInfo, lobby: &LobbyState) -> Element {
  Column::new()
    .width(Dimension::Pct(100.0))
    .height(Dimension::Pct(100.0))
    .flex(1.0)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceBase))
    .child(main_top_bar(ctx))
    .child(main_body(ctx, info, lobby))
    .into()
}

fn main_top_bar(ctx: &mut Ctx) -> Element {
  Row::new()
    .width(Dimension::Pct(100.0))
    .height(55.0)
    .align_items(Alignment::Center)
    .spacing(theme::SpacingSize::Md)
    .padding_horizontal(theme::SpacingSize::Xl)
    .child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon: "volume-2",
      size: 16.0,
      color: theme::palette().text_secondary,
    }))
    .child(Text::new(&ctx.t("lobby.title")).variant(theme::TypographyStyle::Heading))
    .into()
}

fn main_body(ctx: &mut Ctx, info: &ConnectedServerInfo, lobby: &LobbyState) -> Element {
  let selected = lobby
    .selected_channel_id
    .and_then(|id| lobby.channels.iter().find(|channel| channel.id == id));

  if lobby.channels.is_empty() {
    return empty_voice_state(ctx, lobby.last_error.as_deref());
  }

  if let Some(channel) = selected {
    return channel_detail(ctx, channel, info, lobby);
  }

  select_channel_state(ctx, lobby.last_error.as_deref())
}

fn empty_voice_state(ctx: &mut Ctx, error: Option<&str>) -> Element {
  let mut body = Column::new()
    .width(Dimension::Pct(100.0))
    .flex(1.0)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .spacing(theme::SpacingSize::Xl)
    .child(
      Row::new()
        .width(64.0)
        .height(64.0)
        .align_items(Alignment::Center)
        .justify(Justify::Center)
        .rounded(16.0)
        .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised))
        .border_inside(1.0, theme::PaletteColor::Border)
        .child(ctx.mount::<LucideIcon>(LucideIconProps {
          icon: "volume-2",
          size: 28.0,
          color: theme::palette().text_secondary,
        })),
    )
    .child(
      Column::new()
        .width(480.0)
        .align_items(Alignment::Center)
        .spacing(theme::SpacingSize::Md)
        .child(Text::new(&ctx.t("lobby.empty.title")).variant(theme::TypographyStyle::Title))
        .child(
          Text::new(&ctx.t("lobby.empty.description"))
            .variant(theme::TypographyStyle::Description)
            .text_align(Alignment::Center)
            .width(Dimension::Pct(100.0)),
        ),
    )
    .child(create_voice_button(ctx));

  if let Some(error) = error {
    body = body.child(error_notice(ctx, error));
  }

  body.into()
}

fn select_channel_state(ctx: &mut Ctx, error: Option<&str>) -> Element {
  let mut body = Column::new()
    .width(Dimension::Pct(100.0))
    .flex(1.0)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .spacing(theme::SpacingSize::Lg)
    .child(
      Text::new(&ctx.t("lobby.select.title"))
        .variant(theme::TypographyStyle::Title)
        .color(theme::PaletteColor::TextPrimary),
    )
    .child(
      Text::new(&ctx.t("lobby.select.description"))
        .variant(theme::TypographyStyle::Description)
        .color(theme::PaletteColor::TextSecondary),
    );

  if let Some(error) = error {
    body = body.child(error_notice(ctx, error));
  }

  body.into()
}

fn channel_detail(ctx: &mut Ctx, channel: &LobbyChannel, info: &ConnectedServerInfo, lobby: &LobbyState) -> Element {
  let mut users = Column::new().width(520.0).spacing(theme::SpacingSize::Sm);

  if lobby.users.is_empty() {
    users = users.child(
      Text::new(&ctx.t("lobby.users.empty"))
        .variant(theme::TypographyStyle::Description)
        .color(theme::PaletteColor::TextMuted),
    );
  } else {
    for user in &lobby.users {
      users = users.child(user_row(ctx, user, info.user_id));
    }
  }

  let mut body = Column::new()
    .width(Dimension::Pct(100.0))
    .flex(1.0)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .spacing(theme::SpacingSize::Xl)
    .child(
      Column::new()
        .width(520.0)
        .spacing(theme::SpacingSize::Sm)
        .child(Text::new(&channel.name).variant(theme::TypographyStyle::Title))
        .child(
          Text::new(&ctx.t("lobby.channel.description"))
            .variant(theme::TypographyStyle::Description)
            .color(theme::PaletteColor::TextMuted),
        ),
    )
    .child(users);

  if let Some(error) = lobby.last_error.as_deref() {
    body = body.child(error_notice(ctx, error));
  }

  body.into()
}

fn user_row(ctx: &mut Ctx, user: &LobbyUser, local_user_id: u32) -> Element {
  let local = user.user_id == local_user_id;

  Row::new()
    .width(Dimension::Pct(100.0))
    .align_items(Alignment::Center)
    .justify(Justify::SpaceBetween)
    .padding_vertical(8.0)
    .padding_horizontal(10.0)
    .rounded(theme::RadiusSize::Lg)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised))
    .border_inside(1.0, theme::PaletteColor::Border)
    .child(
      Row::new()
        .align_items(Alignment::Center)
        .spacing(theme::SpacingSize::Md)
        .child(server_avatar(&user.username, 32.0, false))
        .child(
          Column::new()
            .spacing(theme::SpacingSize::Xs)
            .child(user_name(ctx, &user.username, local))
            .child(
              Text::new(role_label_lower(user.role))
                .variant(theme::TypographyStyle::Caption)
                .color(theme::PaletteColor::TextMuted),
            ),
        ),
    )
    .child(voice_state(ctx, user))
    .into()
}

fn user_name(ctx: &mut Ctx, username: &str, local: bool) -> Element {
  let name = Text::new(username)
    .variant(theme::TypographyStyle::Description)
    .color(theme::PaletteColor::TextPrimary);

  if !local {
    return name.into();
  }

  Row::new()
    .align_items(Alignment::Center)
    .spacing(theme::SpacingSize::Xs)
    .child(name)
    .child(
      Text::new(&ctx.t("lobby.users.you"))
        .variant(theme::TypographyStyle::Caption)
        .color(theme::PaletteColor::TextMuted),
    )
    .into()
}

fn voice_state(ctx: &mut Ctx, user: &LobbyUser) -> Element {
  let icon = if user.deafened {
    "headphone-off"
  } else if user.muted {
    "mic-off"
  } else {
    "mic"
  };
  let label = if user.deafened {
    ctx.t("lobby.voice.deafened")
  } else if user.muted {
    ctx.t("lobby.voice.muted")
  } else {
    ctx.t("lobby.voice.live")
  };

  Row::new()
    .align_items(Alignment::Center)
    .spacing(theme::SpacingSize::Xs)
    .padding_vertical(5.0)
    .padding_horizontal(8.0)
    .rounded(theme::RadiusSize::Md)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceInput))
    .child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon,
      size: 13.0,
      color: theme::palette().text_muted,
    }))
    .child(
      Text::new(&label)
        .variant(theme::TypographyStyle::Caption)
        .color(theme::PaletteColor::TextMuted),
    )
    .into()
}

fn create_voice_button(ctx: &mut Ctx) -> Element {
  Row::new()
    .height(34.0)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .spacing(theme::SpacingSize::Sm)
    .padding_horizontal(theme::SpacingSize::Lg)
    .rounded(theme::RadiusSize::Md)
    .background(BackgroundColor::Palette(theme::PaletteColor::Accent))
    .cursor(CursorIcon::Pointer)
    .hovered_style(Style::new().background(BackgroundColor::Palette(theme::PaletteColor::AccentHover)))
    .child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon: "plus",
      size: 16.0,
      color: theme::palette().text_inverse,
    }))
    .child(
      Text::new(&ctx.t("lobby.empty.create"))
        .variant(theme::TypographyStyle::Button)
        .color(theme::PaletteColor::TextInverse),
    )
    .into()
}

fn error_notice(ctx: &mut Ctx, message: &str) -> Element {
  Row::new()
    .width(480.0)
    .align_items(Alignment::Center)
    .spacing(theme::SpacingSize::Sm)
    .padding(theme::SpacingSize::Md)
    .rounded(theme::RadiusSize::Md)
    .background(BackgroundColor::Palette(theme::PaletteColor::DangerMuted))
    .border_inside(1.0, theme::PaletteColor::Danger)
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
    .into()
}

fn empty_lobby(ctx: &mut Ctx) -> Element {
  Column::new()
    .width(Dimension::Pct(100.0))
    .height(Dimension::Pct(100.0))
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceBase))
    .child(loader(18.0))
    .child(
      Text::new(&ctx.t("lobby.user.disconnected"))
        .variant(theme::TypographyStyle::Description)
        .color(theme::PaletteColor::TextMuted),
    )
    .into()
}
