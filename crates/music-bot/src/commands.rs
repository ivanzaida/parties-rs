use server_plugin::CommandDefinition;

pub(crate) fn command_definitions() -> Vec<CommandDefinition> {
  [
    CommandDefinition::new("play", "Queue audio from a SoundCloud URL.", "/play {url:string}"),
    CommandDefinition::new("stop", "Stop playback and clear the queue.", "/stop"),
    CommandDefinition::new("skip", "Skip the current track.", "/skip"),
    CommandDefinition::new("queue", "Show queued tracks.", "/queue"),
    CommandDefinition::new("nowplaying", "Show the current track.", "/nowplaying"),
  ]
  .into()
}
