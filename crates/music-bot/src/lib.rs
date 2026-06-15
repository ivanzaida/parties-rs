mod audio;
mod bot;
mod commands;
mod config;
mod player;
mod probe;
mod queue;
mod sources;

use bot::MusicBot;
pub use probe::{SoundCloudProbe, SoundCloudQueueProbe, probe_soundcloud_queue, probe_soundcloud_url};
use server_plugin::plugin;

plugin::register!(MusicBot);

#[cfg(test)]
mod tests {
  use server_plugin::is_valid_command_name;

  use crate::{
    commands::command_definitions,
    queue::{PlaybackSnapshot, Track, TrackSummary},
    sources::{model::SourceKind, registry::SourceRegistry, soundcloud::SoundCloudTokenProvider},
  };

  fn sources() -> SourceRegistry {
    SourceRegistry::new(SoundCloudTokenProvider::new_for_tests("OAuth test-token"))
  }

  #[test]
  fn starter_commands_are_valid() {
    for command in command_definitions() {
      assert!(is_valid_command_name(&command.name));
    }
  }

  #[test]
  fn queue_snapshot_formats_empty_state() {
    let snapshot = PlaybackSnapshot {
      current: None,
      current_elapsed_ms: None,
      queue: Vec::new(),
    };

    assert_eq!(snapshot.queue_message(), "Queue is empty.");
    assert_eq!(snapshot.now_playing_message(), "Nothing is playing.");
  }

  #[test]
  fn queue_snapshot_formats_current_and_pending_items() {
    let snapshot = PlaybackSnapshot {
      current: Some(TrackSummary::new("first", "https://soundcloud.com/artist/first")),
      current_elapsed_ms: Some(42_000),
      queue: vec![
        TrackSummary::new("second", "https://soundcloud.com/artist/second"),
        TrackSummary::new("third", "https://soundcloud.com/artist/third"),
      ],
    };

    assert_eq!(
      snapshot.queue_message(),
      "Playing: [first](https://soundcloud.com/artist/first) - 0:42 / 3:00  \n1) [second](https://soundcloud.com/artist/second) : 3:00  \n2) [third](https://soundcloud.com/artist/third) : 3:00"
    );
    assert_eq!(
      snapshot.now_playing_message(),
      "Playing: [first](https://soundcloud.com/artist/first) - 0:42 / 3:00"
    );
  }

  #[test]
  fn track_markdown_links_escape_markdown_control_characters() {
    let summary = TrackSummary::new(
      "A [demo] \\ track",
      "https://soundcloud.com/a/path)with space?utm_source=id_123",
    );

    assert_eq!(
      summary.markdown_link(),
      "[A \\[demo\\] \\\\ track](https://soundcloud.com/a/path%29with%20space)"
    );
  }

  #[test]
  fn queue_snapshot_caps_large_markdown_queue() {
    let snapshot = PlaybackSnapshot {
      current: Some(TrackSummary::new(
        "first",
        "https://soundcloud.com/artist/first?utm_source=id_123",
      )),
      current_elapsed_ms: Some(0),
      queue: (0..100)
        .map(|index| {
          TrackSummary::new(
            &format!("track {index} with a somewhat long title"),
            &format!("https://soundcloud.com/artist/track-{index}?utm_medium=api&utm_campaign=social_sharing"),
          )
        })
        .collect(),
    };

    let message = snapshot.queue_message();

    assert!(message.len() <= 3_800);
    assert!(message.contains("... 95 more"));
    assert!(message.contains("[first](https://soundcloud.com/artist/first)"));
    assert!(!message.contains("utm_"));
  }

  #[test]
  fn soundcloud_urls_parse_as_soundcloud_tracks() {
    let track = Track::parse("https://soundcloud.com/artist/track", &sources()).unwrap();

    assert_eq!(track.title, "SoundCloud URL");
    assert_eq!(track.source.kind, SourceKind::SoundCloud);
    assert_eq!(track.source.url, "https://soundcloud.com/artist/track");
  }

  #[test]
  fn unsupported_sources_are_rejected() {
    assert!(Track::parse("never gonna give you up", &sources()).is_err());
  }
}
