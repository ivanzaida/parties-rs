use lurq::{
  app::{component::Component, ctx::Ctx},
  components::Column,
  core::Store,
  layout::Alignment,
  node::{dimension::Dimension, BackgroundColor, Element},
};

use crate::{pages, route::Route, theme};

pub struct App {
  route: Store<Route>,
}

impl Component for App {
  type Props = ();

  fn create(ctx: &mut Ctx) -> Self {
    let route = ctx.store(Route::IdentityGenerate);
    ctx.provide(route.clone());
    Self { route }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let content = match self.route.get() {
      Route::IdentityGenerate => ctx.mount::<pages::identity_generate::IdentityGenerate>(()),
      Route::SeedPhraseDisplay => ctx.mount::<pages::seed_phrase::SeedPhrase>(()),
      Route::IdentityRestore => ctx.mount::<pages::identity_restore::IdentityRestore>(()),
      Route::IdentityImportKey => ctx.mount::<pages::identity_import::IdentityImport>(()),
      Route::Connect => ctx.mount::<pages::connect::Connect>(()),
      Route::TofuWarning => ctx.mount::<pages::tofu_warning::TofuWarning>(()),
      Route::Lobby => ctx.mount::<pages::lobby::Lobby>(()),
      Route::LobbyScreenShare => ctx.mount::<pages::lobby_screen_share::LobbyScreenShare>(()),
      Route::Servers => ctx.mount::<pages::servers::Servers>(()),
      Route::Settings => ctx.mount::<pages::settings::Settings>(()),
      Route::SettingsIdentity => ctx.mount::<pages::settings_identity::SettingsIdentity>(()),
      Route::SettingsAppearance => ctx.mount::<pages::settings_appearance::SettingsAppearance>(()),
      Route::SettingsAbout => ctx.mount::<pages::settings_about::SettingsAbout>(()),
    };

    Column::new()
      .width(Dimension::Pct(100.0))
      .height(Dimension::Pct(100.0))
      .background(BackgroundColor::Palette(theme::BG_PRIMARY))
      .align_items(Alignment::Stretch)
      .clip()
      .child(content)
  }
}
