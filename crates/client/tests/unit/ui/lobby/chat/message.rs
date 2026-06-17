use super::*;

#[test]
fn bare_https_url_becomes_markdown_link_without_wrapper() {
  let url = "https://example.com/path?q=1";
  let text = format!("open ({url}).");

  assert_eq!(chat_markdown_source(&text), format!("open ([{url}](<{url}>))."));
}

#[test]
fn bare_domain_becomes_https_markdown_link() {
  assert_eq!(
    chat_markdown_source("visit example.com"),
    "visit [example.com](<https://example.com>)"
  );
}

#[test]
fn regular_markdown_is_preserved_when_no_bare_links_exist() {
  assert_eq!(chat_markdown_source("**bold** text"), "**bold** text");
}

#[test]
fn ordered_dot_marker_is_escaped_to_keep_chat_line_layout() {
  assert_eq!(chat_markdown_source("1. first"), "1\\. first");
}

#[test]
fn ordered_paren_marker_is_escaped_to_keep_chat_line_layout() {
  assert_eq!(chat_markdown_source("1) first"), "1\\) first");
}

#[test]
fn music_queue_lines_are_not_rendered_as_markdown_lists() {
  let source = "Queued playlist: 2 tracks  \n1) [first](https://example.com/first) : 3:06  \n2) second : 3:34";

  assert_eq!(
    chat_markdown_source(source),
    "Queued playlist: 2 tracks  \n1\\) [first](https://example.com/first) : 3:06  \n2\\) second : 3:34"
  );
}

#[test]
fn chat_markdown_source_is_deterministic() {
  let first = chat_markdown_source("visit example.com");
  let second = chat_markdown_source("visit example.com");

  assert_eq!(first, second);
}
