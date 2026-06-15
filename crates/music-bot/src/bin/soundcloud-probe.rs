use std::{collections::HashMap, env, fs};

fn main() {
  let Some(url) = env::args().nth(1) else {
    eprintln!("usage: cargo run --package music-bot --bin soundcloud-probe -- <soundcloud-url>");
    std::process::exit(2);
  };

  let dotenv = read_dotenv();
  let client_id = read_var("SOUNDCLOUD_CLIENT_ID", &dotenv);
  let client_secret = read_var("SOUNDCLOUD_CLIENT_SECRET", &dotenv);

  let (client_id, client_secret) = match (client_id, client_secret) {
    (Some(client_id), Some(client_secret)) => (client_id, client_secret),
    _ => {
      eprintln!("SOUNDCLOUD_CLIENT_ID and SOUNDCLOUD_CLIENT_SECRET must be set in env or .env");
      std::process::exit(2);
    }
  };

  let queue_probe = match music_bot::probe_soundcloud_queue(&url, &client_id, &client_secret) {
    Ok(queue_probe) => queue_probe,
    Err(error) => {
      eprintln!("error: {error}");
      std::process::exit(1);
    }
  };

  if queue_probe.len() > 1 {
    println!("playlist_tracks: {}", queue_probe.len());
    for (index, track) in queue_probe.iter().take(20).enumerate() {
      println!(
        "{}. {} : {}",
        index + 1,
        track.title,
        format_duration(track.duration_ms)
      );
      println!("   {}", track.url);
    }
    if queue_probe.len() > 20 {
      println!("... {} more", queue_probe.len() - 20);
    }
    return;
  }

  let audio_url = queue_probe.first().map(|track| track.url.as_str()).unwrap_or(&url);
  match music_bot::probe_soundcloud_url(audio_url, &client_id, &client_secret) {
    Ok(probe) => {
      println!("title: {}", probe.title);
      println!("source_url: {}", probe.source_url);
      println!(
        "container_hint: {}",
        probe.container_hint.unwrap_or_else(|| "unknown".to_owned())
      );
      println!("downloaded_bytes: {}", probe.byte_len);
      println!("decoded_samples: {}", probe.decoded_samples);
    }
    Err(error) => {
      eprintln!("error: {error}");
      std::process::exit(1);
    }
  }
}

fn read_var(name: &str, dotenv: &HashMap<String, String>) -> Option<String> {
  env::var(name)
    .ok()
    .filter(|value| !value.trim().is_empty())
    .or_else(|| dotenv.get(name).cloned().filter(|value| !value.trim().is_empty()))
}

fn format_duration(duration_ms: Option<u64>) -> String {
  let Some(duration_ms) = duration_ms else {
    return "unknown".to_owned();
  };
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

fn read_dotenv() -> HashMap<String, String> {
  let Ok(contents) = fs::read_to_string(".env") else {
    return HashMap::new();
  };

  contents
    .lines()
    .filter_map(parse_dotenv_line)
    .collect::<HashMap<_, _>>()
}

fn parse_dotenv_line(line: &str) -> Option<(String, String)> {
  let line = line.trim();
  if line.is_empty() || line.starts_with('#') {
    return None;
  }

  let (key, value) = line.split_once('=')?;
  let key = key.trim();
  if key.is_empty() {
    return None;
  }

  let value = value.trim().trim_matches('"').trim_matches('\'').to_owned();
  Some((key.to_owned(), value))
}
