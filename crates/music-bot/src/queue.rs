use std::collections::VecDeque;

use server_plugin::ChannelId;

use crate::sources::{model::SourceRequest, registry::SourceRegistry};

const MAX_QUEUE_MESSAGE_BYTES: usize = 3_800;
const MAX_QUEUE_NEXT_TRACKS: usize = 5;
const MARKDOWN_LINE_BREAK: &str = "  \n";

#[derive(Clone)]
pub(crate) struct Track {
  pub(crate) title: String,
  pub(crate) duration_ms: Option<u64>,
  pub(crate) source: SourceRequest,
}

impl Track {
  #[cfg(test)]
  pub(crate) fn parse(input: &str, sources: &SourceRegistry) -> Result<Self, String> {
    let source = sources.parse(input)?;
    Ok(Self::from_source(source))
  }

  pub(crate) fn parse_many(input: &str, sources: &SourceRegistry) -> Result<Vec<Self>, String> {
    Ok(
      sources
        .parse_many(input)?
        .into_iter()
        .map(Self::from_source)
        .collect::<Vec<_>>(),
    )
  }

  fn from_source(source: SourceRequest) -> Self {
    Self {
      title: source.loading_title.clone(),
      duration_ms: source.duration_ms,
      source,
    }
  }

  pub(crate) fn summary(&self) -> TrackSummary {
    TrackSummary {
      title: self.title.clone(),
      url: self.source.url.clone(),
      duration_ms: self.duration_ms,
    }
  }

  pub(crate) fn markdown_link(&self) -> String {
    self.summary().markdown_link()
  }

  pub(crate) fn markdown_link_with_duration(&self) -> String {
    self.summary().markdown_link_with_duration()
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
  pub(crate) current_started_at: Option<std::time::Instant>,
  pub(crate) queue: VecDeque<QueuedTrack>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TrackSummary {
  pub(crate) title: String,
  pub(crate) url: String,
  pub(crate) duration_ms: Option<u64>,
}

impl TrackSummary {
  #[cfg(test)]
  pub(crate) fn new(title: &str, url: &str) -> Self {
    Self {
      title: title.to_owned(),
      url: url.to_owned(),
      duration_ms: Some(180_000),
    }
  }

  pub(crate) fn markdown_link(&self) -> String {
    format!(
      "[{}]({})",
      escape_markdown_link_text(&self.title),
      escape_markdown_link_url(&self.url)
    )
  }

  pub(crate) fn markdown_link_with_duration(&self) -> String {
    format!(
      "{} : {}",
      self.markdown_link(),
      format_optional_duration(self.duration_ms)
    )
  }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PlaybackSnapshot {
  pub(crate) current: Option<TrackSummary>,
  pub(crate) current_elapsed_ms: Option<u64>,
  pub(crate) queue: Vec<TrackSummary>,
}

impl PlaybackSnapshot {
  pub(crate) fn queue_message(&self) -> String {
    if self.current.is_none() && self.queue.is_empty() {
      return "Queue is empty.".to_owned();
    }

    let mut lines = Vec::new();
    if let Some(current) = self.current.as_ref() {
      lines.push(format!(
        "Playing: {} - {}",
        current.markdown_link(),
        format_playback_progress(self.current_elapsed_ms.unwrap_or(0), current.duration_ms)
      ));
    }

    let mut omitted = 0usize;
    for (index, item) in self.queue.iter().enumerate() {
      if index >= MAX_QUEUE_NEXT_TRACKS {
        omitted = self.queue.len() - index;
        break;
      }

      let line = format!("{}) {}", index + 1, item.markdown_link_with_duration());
      let remaining_count = self.queue.len() - index;
      let omitted_line = format!("... {remaining_count} more");
      if projected_message_len(&lines, &line) + omitted_line.len() + 1 > MAX_QUEUE_MESSAGE_BYTES {
        omitted = remaining_count;
        break;
      }

      lines.push(line);
    }

    if omitted > 0 {
      lines.push(format!("... {omitted} more"));
    }

    lines.join(MARKDOWN_LINE_BREAK)
  }

  pub(crate) fn now_playing_message(&self) -> String {
    self
      .current
      .as_ref()
      .map(|current| {
        format!(
          "Playing: {} - {}",
          current.markdown_link(),
          format_playback_progress(self.current_elapsed_ms.unwrap_or(0), current.duration_ms)
        )
      })
      .unwrap_or_else(|| "Nothing is playing.".to_owned())
  }
}

fn format_playback_progress(elapsed_ms: u64, duration_ms: Option<u64>) -> String {
  let elapsed_ms = duration_ms
    .map(|duration_ms| elapsed_ms.min(duration_ms))
    .unwrap_or(elapsed_ms);
  let duration = duration_ms.map(format_duration).unwrap_or_else(|| "unknown".to_owned());
  format!("{} / {duration}", format_duration(elapsed_ms))
}

pub(crate) fn format_optional_duration(duration_ms: Option<u64>) -> String {
  duration_ms.map(format_duration).unwrap_or_else(|| "unknown".to_owned())
}

fn format_duration(duration_ms: u64) -> String {
  let total_seconds = duration_ms / 1_000;
  let seconds = total_seconds % 60;
  let total_minutes = total_seconds / 60;
  let minutes = total_minutes % 60;
  let hours = total_minutes / 60;
  if hours > 0 {
    format!("{hours}:{minutes:02}:{seconds:02}")
  } else {
    format!("{minutes}:{seconds:02}")
  }
}

fn escape_markdown_link_text(text: &str) -> String {
  text
    .replace(['\r', '\n'], " ")
    .chars()
    .flat_map(|character| {
      if matches!(character, '\\' | '[' | ']') {
        Some('\\').into_iter().chain(Some(character))
      } else {
        None.into_iter().chain(Some(character))
      }
    })
    .collect()
}

fn escape_markdown_link_url(url: &str) -> String {
  clean_display_url(url).replace(' ', "%20").replace(')', "%29")
}

fn clean_display_url(url: &str) -> &str {
  if !is_soundcloud_url(url) {
    return url;
  }

  let end = url.find(['?', '#']).unwrap_or(url.len());
  &url[..end]
}

fn is_soundcloud_url(url: &str) -> bool {
  let url = url.to_ascii_lowercase();
  url.starts_with("https://soundcloud.com/")
    || url.starts_with("http://soundcloud.com/")
    || url.starts_with("https://www.soundcloud.com/")
    || url.starts_with("http://www.soundcloud.com/")
}

fn projected_message_len(lines: &[String], next_line: &str) -> usize {
  lines.iter().map(String::len).sum::<usize>() + lines.len() * MARKDOWN_LINE_BREAK.len() + next_line.len()
}
