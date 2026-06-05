use lurq::{
  app::{component::Component, ctx::Ctx},
  components::{Column, Text},
  node::{BackgroundColor, Element, dimension::Dimension},
};

use crate::{screens::shared, session::ServerSession, theme};

pub struct Lobby;

impl Component for Lobby {
  type Props = ();

  fn create(_: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let session = ctx.use_context::<ServerSession>().unwrap_or_default();
    let info = session.info();
    let server_name = info
      .as_ref()
      .map(|info| info.server_name.clone())
      .unwrap_or_else(|| ctx.t("lobby.server.unknown"));
    let user_label = info
      .as_ref()
      .map(|info| {
        ctx.t_args(
          "lobby.user",
          [
            ("user_id", info.user_id.to_string()),
            ("role", format!("{:?}", info.role)),
          ],
        )
      })
      .unwrap_or_else(|| ctx.t("lobby.user.disconnected"));

    Column::new()
      .width(Dimension::Pct(100.0))
      .height(Dimension::Pct(100.0))
      .align_items(lurq::layout::Alignment::Center)
      .justify(lurq::layout::layout_kind::Justify::Center)
      .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceBase))
      .child(
        Column::new()
          .width(shared::CARD_WIDTH)
          .spacing(12.0)
          .padding(18.0)
          .rounded(8.0)
          .background(BackgroundColor::Palette(theme::PaletteColor::SurfacePanel))
          .border_inside(1.0, theme::PaletteColor::Border)
          .child(Text::new(&ctx.t("lobby.title")).variant(theme::TypographyStyle::Caption))
          .child(Text::new(&server_name).variant(theme::TypographyStyle::Title))
          .child(Text::new(&user_label).variant(theme::TypographyStyle::Description)),
      )
  }
}
