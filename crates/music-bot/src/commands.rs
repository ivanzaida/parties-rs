use server_plugin::{CommandDefinition, CommandInputDefinition};

pub(crate) fn command_definitions() -> Vec<CommandDefinition> {
  [
    CommandDefinition::new("play", "Queue audio from SoundCloud.", "/play {query:string...}").with_input(
      CommandInputDefinition::live_query("query")
        .with_min_chars(2)
        .with_debounce_ms(400)
        .with_max_results(10)
        .with_placeholder("Search SoundCloud"),
    ),
    CommandDefinition::new("stop", "Stop playback and clear the queue.", "/stop"),
    CommandDefinition::new("skip", "Skip the current track.", "/skip"),
    CommandDefinition::new("queue", "Show queued tracks.", "/queue"),
    CommandDefinition::new("nowplaying", "Show the current track.", "/nowplaying"),
  ]
  .into()
}
