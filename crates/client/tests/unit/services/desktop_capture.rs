use super::{display_window_app_name, source_window_name};

#[test]
fn display_window_app_name_strips_windows_executable_path() {
  assert_eq!(
    display_window_app_name(r#"C:\Program Files\Google\Chrome\Application\chrome.exe"#),
    "chrome"
  );
}

#[test]
fn display_window_app_name_strips_quoted_path_and_extension() {
  assert_eq!(display_window_app_name(r#""C:\Apps\Discord.EXE""#), "Discord");
}

#[test]
fn source_window_name_uses_sanitized_app_name_with_title() {
  let app = display_window_app_name(r#"C:\Program Files\App\app.exe"#);
  assert_eq!(source_window_name(&app, "Project"), Some("app - Project".to_owned()));
}

#[test]
fn source_window_name_prefers_title_when_app_matches_title() {
  assert_eq!(source_window_name("Settings", "Settings"), Some("Settings".to_owned()));
}

#[test]
fn source_window_name_prefers_title_for_helper_processes() {
  assert_eq!(source_window_name("steamwebhelper", "Steam"), Some("Steam".to_owned()));
}

#[test]
fn source_window_name_keeps_app_prefix_for_specific_titles() {
  assert_eq!(source_window_name("Code", "main.rs"), Some("Code - main.rs".to_owned()));
}
