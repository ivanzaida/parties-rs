use lurq::{
  app::{component::Component, ctx::Ctx},
  components::Column,
  core::Signal,
  layout::{Alignment, layout_kind::Justify},
  node::{BackgroundColor, Element, dimension::Dimension},
  router::{RouterHandle, Routes},
};

use crate::{
  routes::{ROUTE_IDENTITY_SETUP, ROUTE_LOADING, ROUTE_RESTORE_IDENTITY, ROUTE_SEED_PHRASE},
  session::ServerSession,
  storage::Storage,
  theme,
  ui::{
    identity_seed::IdentitySeedScreen,
    identity_setup::IdentitySetupScreen,
    loading_identity::{LoadingIdentityScreen, LoadingIdentityScreenProps},
    restore_identity::RestoreIdentityScreen,
  },
};

pub struct App {
  router: RouterHandle,
  session: ServerSession,
  storage: Signal<Option<Storage>>,
}

impl Component for App {
  type Props = ();

  fn create(ctx: &mut Ctx) -> Self {
    let storage = ctx.signal(None::<Storage>);
    let loading_storage = storage.clone();
    let router = ctx.router(
      Routes::new()
        .route(ROUTE_LOADING, move |ctx| {
          ctx.mount::<LoadingIdentityScreen>(LoadingIdentityScreenProps {
            storage: loading_storage.clone(),
          })
        })
        .route(ROUTE_IDENTITY_SETUP, |ctx| ctx.mount::<IdentitySetupScreen>(()))
        .route(ROUTE_SEED_PHRASE, |ctx| ctx.mount::<IdentitySeedScreen>(()))
        .route(ROUTE_RESTORE_IDENTITY, |ctx| ctx.mount::<RestoreIdentityScreen>(()))
        .fallback(|ctx| ctx.mount::<IdentitySetupScreen>(())),
    );
    router.replace(ROUTE_LOADING);
    Self {
      router,
      session: ServerSession::default(),
      storage,
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    ctx.provide(self.session.clone());
    if let Some(storage) = self.storage.get() {
      ctx.provide(storage);
    }

    Column::new()
      .width(Dimension::Pct(100.0))
      .height(Dimension::Pct(100.0))
      .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceBase))
      .align_items(Alignment::Stretch)
      .justify(Justify::Start)
      .clip()
      .child(lurq::components::Router::mount(ctx, self.router.clone()))
  }
}
