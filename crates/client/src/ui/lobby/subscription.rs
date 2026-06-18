use std::sync::Arc;

use lurq::{
  app::{component::DevtoolsInspectable, ctx::Ctx},
  core::{Signal, Store},
};
use parking_lot::Mutex;
use tokio::sync::{Mutex as AsyncMutex, watch};

use super::session_identity::session_address;
use crate::session::{LobbySnapshot, LobbyState, ServerSession};

pub(super) struct LobbyModelSubscription {
  generation: Signal<u64>,
  applied_generation: Signal<Option<u64>>,
  session_key: Mutex<Option<Option<String>>>,
  receiver: Mutex<Option<Arc<AsyncMutex<watch::Receiver<LobbySnapshot>>>>>,
}

impl LobbyModelSubscription {
  pub(super) fn new(ctx: &mut Ctx) -> Self {
    Self {
      generation: ctx.signal(0),
      applied_generation: ctx.signal(None),
      session_key: Mutex::new(None),
      receiver: Mutex::new(None),
    }
  }

  pub(super) fn next_model<M, F>(&self, ctx: &mut Ctx, session: ServerSession, select: F) -> Option<(u64, M)>
  where
    M: DevtoolsInspectable + Clone + PartialEq + Send + Sync + 'static,
    F: Fn(&LobbySnapshot) -> M + Clone + Send + Sync + 'static,
  {
    let session_key = session_address(&session);
    let receiver = {
      let mut stored_session_key = self.session_key.lock();
      let mut receiver = self.receiver.lock();
      if stored_session_key.as_ref() != Some(&session_key) {
        *stored_session_key = Some(session_key);
        *receiver = None;
        self.applied_generation.set(None);
        self.generation.set(self.generation.get_untracked().wrapping_add(1));
      }
      receiver
        .get_or_insert_with(|| Arc::new(AsyncMutex::new(session.subscribe_lobby_updates())))
        .clone()
    };
    let wait_generation = self.generation.get();
    let session = session.clone();
    let update = ctx.future(wait_generation, move |wait_generation| {
      let receiver = receiver.clone();
      let session = session.clone();
      let select = select.clone();
      async move {
        let mut receiver = receiver.lock().await;
        let snapshot = match receiver.changed().await {
          Ok(()) => receiver.borrow().clone(),
          Err(_) => LobbySnapshot {
            generation: wait_generation,
            lobby: session.lobby(),
          },
        };
        Ok::<_, String>((snapshot.generation, select(&snapshot)))
      }
    });
    let state = update.state().get();
    if state.is_fulfilled()
      && let Some((snapshot_generation, model)) = state.data
      && self.applied_generation.get_untracked() != Some(snapshot_generation)
    {
      self.applied_generation.set(Some(snapshot_generation));
      self.generation.set(wait_generation.wrapping_add(1));
      return Some((snapshot_generation, model));
    }

    None
  }
}

pub(super) fn apply_model<M>(model_store: &Store<Option<M>>, model: M)
where
  M: DevtoolsInspectable + Clone + PartialEq + Send + Sync + 'static,
{
  if model_store.with(|current| current.as_ref() != Some(&model)) {
    model_store.set(Some(model));
  }
}

pub(super) fn current_model<M, F>(session: &ServerSession, select: F) -> M
where
  F: FnOnce(&LobbyState) -> M,
{
  let lobby = session.lobby();
  select(&lobby)
}

pub(super) fn apply_current_model<M, F>(model_store: &Store<Option<M>>, session: &ServerSession, select: F)
where
  M: DevtoolsInspectable + Clone + PartialEq + Send + Sync + 'static,
  F: FnOnce(&LobbyState) -> M,
{
  apply_model(model_store, current_model(session, select));
}

pub(super) fn apply_optional_model<M>(model_store: &Store<Option<M>>, model: Option<M>)
where
  M: DevtoolsInspectable + Clone + PartialEq + Send + Sync + 'static,
{
  if model_store.with(|current| current.as_ref() != model.as_ref()) {
    model_store.set(model);
  }
}

pub(super) fn apply_current_optional_model<M, F>(model_store: &Store<Option<M>>, session: &ServerSession, select: F)
where
  M: DevtoolsInspectable + Clone + PartialEq + Send + Sync + 'static,
  F: FnOnce(&LobbyState) -> Option<M>,
{
  let lobby = session.lobby();
  apply_optional_model(model_store, select(&lobby));
}
