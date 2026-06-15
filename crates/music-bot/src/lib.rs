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
    queue::{PlaybackSnapshot, Track},
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
      queue: Vec::new(),
    };

    assert_eq!(snapshot.queue_message(), "Queue is empty.");
    assert_eq!(snapshot.now_playing_message(), "Nothing is playing yet.");
  }

  #[test]
  fn queue_snapshot_formats_current_and_pending_items() {
    let snapshot = PlaybackSnapshot {
      current: Some("first".to_owned()),
      queue: vec!["second".to_owned(), "third".to_owned()],
    };

    assert_eq!(snapshot.queue_message(), "Now playing: first\n1. second\n2. third");
    assert_eq!(snapshot.now_playing_message(), "Now playing: first");
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
