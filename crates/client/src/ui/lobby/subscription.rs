use std::sync::atomic::{AtomicU64, Ordering};

use lurq::{
  app::{component::DevtoolsInspectable, ctx::Ctx},
  core::Store,
};

use super::session_identity::session_address;
use crate::session::{LobbySnapshot, LobbyState, ServerSession};

pub(super) struct LobbyModelSubscription {
  applied_generation: AtomicU64,
}

impl LobbyModelSubscription {
  pub(super) fn new(_ctx: &mut Ctx) -> Self {
    Self {
      applied_generation: AtomicU64::new(u64::MAX),
    }
  }

  pub(super) fn next_model<M, F>(&self, ctx: &mut Ctx, session: ServerSession, select: F) -> Option<(u64, M)>
  where
    M: DevtoolsInspectable + Clone + PartialEq + Send + Sync + 'static,
    F: Fn(&LobbySnapshot) -> M + Clone + Send + Sync + 'static,
  {
    let session_key = session_address(&session);
    let stream_session = session.clone();
    let stream_select = select.clone();
    let update = ctx.stream(
      session_key.clone(),
      move |_session_key, emitter: lurq::app::ctx::StreamEmitter<(u64, M), String>| {
        let session = stream_session.clone();
        let select = stream_select.clone();
        async move {
          let mut receiver = session.subscribe_lobby_updates();
          loop {
            let (snapshot, closed) = match receiver.changed().await {
              Ok(()) => (receiver.borrow().clone(), false),
              Err(_) => (
                LobbySnapshot {
                  generation: 0,
                  lobby: session.lobby(),
                },
                true,
              ),
            };
            if !emitter.emit((snapshot.generation, select(&snapshot))) {
              break;
            }
            if closed {
              break;
            }
          }
        }
      },
    );
    let state = update.state().get();
    if state.is_fulfilled()
      && let Some((snapshot_generation, model)) = state.data
    {
      if self.applied_generation.load(Ordering::Relaxed) == snapshot_generation {
        return None;
      }
      self.applied_generation.store(snapshot_generation, Ordering::Relaxed);
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
