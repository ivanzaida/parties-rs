use std::collections::VecDeque;

use server_plugin::ChannelId;

use crate::sources::{model::SourceRequest, registry::SourceRegistry};

#[derive(Clone)]
pub(crate) struct Track {
  pub(crate) title: String,
  pub(crate) source: SourceRequest,
}

impl Track {
  pub(crate) fn parse(input: &str, sources: &SourceRegistry) -> Result<Self, String> {
    let source = sources.parse(input)?;
    Ok(Self {
      title: source.loading_title.clone(),
      source,
    })
  }
}

#[derive(Clone)]
pub(crate) struct QueuedTrack {
  pub(crate) track: Track,
  pub(crate) text_channel_id: ChannelId,
}

#[derive(Default)]
pub(crate) struct PlayerState {
  pub(crate) current: Option<QueuedTrack>,
  pub(crate) queue: VecDeque<QueuedTrack>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PlaybackSnapshot {
  pub(crate) current: Option<String>,
  pub(crate) queue: Vec<String>,
}

impl PlaybackSnapshot {
  pub(crate) fn queue_message(&self) -> String {
    if self.current.is_none() && self.queue.is_empty() {
      return "Queue is empty.".to_owned();
    }

    let mut lines = Vec::new();
    if let Some(current) = self.current.as_ref() {
      lines.push(format!("Now playing: {current}"));
    }
    lines.extend(
      self
        .queue
        .iter()
        .enumerate()
        .map(|(index, item)| format!("{}. {item}", index + 1)),
    );
    lines.join("\n")
  }

  pub(crate) fn now_playing_message(&self) -> String {
    self
      .current
      .as_ref()
      .map(|current| format!("Now playing: {current}"))
      .unwrap_or_else(|| "Nothing is playing yet.".to_owned())
  }
}
