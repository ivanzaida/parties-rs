use lurq::{
  app::{component::Component, ctx::Ctx},
  components::Column,
  layout::{Alignment, layout_kind::Justify},
  node::{BackgroundColor, Element, dimension::Dimension},
  router::{RouterHandle, Routes},
};

use crate::{
  screens::{
    self,
    shared::{
      ROUTE_CHOOSE_SERVER, ROUTE_CONNECT_SERVER, ROUTE_IDENTITY_SETUP, ROUTE_IMPORT_PRIVATE_KEY, ROUTE_LOADING,
      ROUTE_LOBBY, ROUTE_RESTORE_IDENTITY, ROUTE_SEED_PHRASE, ROUTE_TOFU_WARNING,
    },
  },
  session::ServerSession,
  storage::Storage,
  theme,
};

#[derive(Clone, PartialEq, lurq::DevtoolsInspectable)]
struct StartupData {
  storage: Option<Storage>,
  initial_route: String,
}

fn load_startup_data_sync() -> Result<StartupData, String> {
  match Storage::open_default() {
    Ok(storage) => {
      let has_identity = storage.has_identity().unwrap_or(false);
      let initial_route = if has_identity {
        ROUTE_CHOOSE_SERVER
      } else {
        ROUTE_IDENTITY_SETUP
      };

      Ok(StartupData {
        storage: Some(storage),
        initial_route: initial_route.to_owned(),
      })
    }
    Err(_) => Ok(StartupData {
      storage: None,
      initial_route: ROUTE_IDENTITY_SETUP.to_owned(),
    }),
  }
}

async fn load_startup_data() -> Result<StartupData, String> {
  tokio::task::spawn_blocking(load_startup_data_sync)
    .await
    .map_err(|error| error.to_string())?
}

pub struct App {
  router: RouterHandle,
  session: ServerSession,
}

impl Component for App {
  type Props = ();

  fn create(ctx: &mut Ctx) -> Self {
    let router = ctx.router(
      Routes::new()
        .route(ROUTE_LOADING, |ctx| ctx.mount::<screens::loading::LoadingScreen>(()))
        .route(ROUTE_SEED_PHRASE, |ctx| {
          ctx.mount::<screens::identity::seed_phrase_display::SeedPhraseDisplay>(())
        })
        .route(ROUTE_IMPORT_PRIVATE_KEY, |ctx| {
          ctx.mount::<screens::identity::import_private_key::ImportPrivateKey>(())
        })
        .route(ROUTE_RESTORE_IDENTITY, |ctx| {
          ctx.mount::<screens::identity::restore_identity::RestoreIdentity>(())
        })
        .route(ROUTE_CHOOSE_SERVER, |ctx| {
          ctx.mount::<screens::server_select::ServerSelect>(())
        })
        .route(ROUTE_CONNECT_SERVER, |ctx| {
          ctx.mount::<screens::server_connect::ServerConnect>(())
        })
        .route(ROUTE_TOFU_WARNING, |ctx| {
          ctx.mount::<screens::tofu_warning::TofuWarningScreen>(())
        })
        .route(ROUTE_LOBBY, |ctx| ctx.mount::<screens::lobby::Lobby>(()))
        .fallback(|ctx| ctx.mount::<screens::identity::setup::IdentitySetup>(())),
    );
    router.replace(ROUTE_LOADING);
    Self {
      router,
      session: ServerSession::default(),
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let startup = ctx.future((), |_| load_startup_data()).state().get();

    if let Some(startup) = startup.data.as_ref() {
      if self.router.path().get() == ROUTE_LOADING {
        self.router.replace(startup.initial_route.clone());
      }

      if let Some(storage) = startup.storage.as_ref() {
        ctx.provide(storage.clone());
      }
    }
    ctx.provide(self.session.clone());

    Column::new()
      .width(Dimension::Pct(100.0))
      .height(Dimension::Pct(100.0))
      .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceBase))
      .align_items(Alignment::Center)
      .justify(Justify::Center)
      .clip()
      .child(lurq::components::Router::mount(ctx, self.router.clone()))
  }
}
