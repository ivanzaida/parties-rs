use lurq::{
  app::{component::Component, ctx::Ctx},
  components::Column,
  core::Signal,
  layout::{Alignment, layout_kind::Justify},
  node::{BackgroundColor, Element, dimension::Dimension},
};

use crate::{
  screens::{
    self,
    shared::{
      STEP_CHOOSE_SERVER, STEP_CONNECT_SERVER, STEP_IDENTITY_SETUP, STEP_IMPORT_PRIVATE_KEY, STEP_RESTORE_IDENTITY,
      STEP_SEED_PHRASE,
    },
  },
  storage::Storage,
  theme,
};

const STEP_LOADING: u8 = u8::MAX;

#[derive(Clone, PartialEq, lurq::DevtoolsInspectable)]
struct StartupData {
  storage: Option<Storage>,
  initial_step: u8,
}

fn load_startup_data_sync() -> Result<StartupData, String> {
  match Storage::open_default() {
    Ok(storage) => {
      let has_identity = storage.has_identity().unwrap_or_else(|error| {
        eprintln!("failed to load identity: {error}");
        false
      });
      let initial_step = if has_identity {
        STEP_CHOOSE_SERVER
      } else {
        STEP_IDENTITY_SETUP
      };

      Ok(StartupData {
        storage: Some(storage),
        initial_step,
      })
    }
    Err(error) => {
      eprintln!("failed to open storage: {error}");
      Ok(StartupData {
        storage: None,
        initial_step: STEP_IDENTITY_SETUP,
      })
    }
  }
}

async fn load_startup_data() -> Result<StartupData, String> {
  tokio::task::spawn_blocking(load_startup_data_sync)
    .await
    .map_err(|error| error.to_string())?
}

pub struct App {
  step: Signal<u8>,
}

impl Component for App {
  type Props = ();

  fn create(ctx: &mut Ctx) -> Self {
    Self {
      step: ctx.signal(STEP_LOADING),
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let startup = ctx.future((), |_| load_startup_data()).state().get();
    let mut step = self.step.get();

    if let Some(startup) = startup.data.as_ref() {
      if step == STEP_LOADING {
        step = startup.initial_step;
        self.step.set(step);
      }

      if let Some(storage) = startup.storage.as_ref() {
        ctx.provide(storage.clone());
      }
    }

    ctx.provide(self.step.clone());

    let screen = match step {
      STEP_LOADING => ctx.mount::<screens::loading::LoadingScreen>(()),
      STEP_SEED_PHRASE => ctx.mount::<screens::identity::seed_phrase_display::SeedPhraseDisplay>(()),
      STEP_IMPORT_PRIVATE_KEY => ctx.mount::<screens::identity::import_private_key::ImportPrivateKey>(()),
      STEP_RESTORE_IDENTITY => ctx.mount::<screens::identity::restore_identity::RestoreIdentity>(()),
      STEP_CHOOSE_SERVER => ctx.mount::<screens::server_select::ServerSelect>(()),
      STEP_CONNECT_SERVER => ctx.mount::<screens::server_connect::ServerConnect>(()),
      _ => ctx.mount::<screens::identity::setup::IdentitySetup>(()),
    };

    Column::new()
      .width(Dimension::Pct(100.0))
      .height(Dimension::Pct(100.0))
      .background(BackgroundColor::Palette(theme::BG_PRIMARY))
      .align_items(Alignment::Center)
      .justify(Justify::Center)
      .clip()
      .child(screen)
  }
}
