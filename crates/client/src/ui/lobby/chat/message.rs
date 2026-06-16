use std::{process::Command, time::Instant};

use lurq::{
  app::{
    component::{Component, ComponentInfo, DevtoolsFormatter, DevtoolsInspectable},
    ctx::Ctx,
  },
  components::{Column, Markdown, MarkdownProps, Row, Text},
  layout::Alignment,
  node::{BackgroundColor, Element, dimension::Dimension},
};

use super::timeline::format_chat_time;
use crate::{
  network::protocol::control::ChatMessage as ProtocolChatMessage,
  theme,
  ui::{
    common::lucide_icon::{LucideIcon, LucideIconProps},
    lobby::{rail::server_avatar, shared::user_display_name},
  },
};

#[derive(Clone, PartialEq)]
pub(super) struct ChatMessageProps {
  pub(super) message: ProtocolChatMessage,
  pub(super) local_user_id: u32,
  pub(super) debug_user_ids: bool,
}

impl DevtoolsInspectable for ChatMessageProps {
  fn inspect(&self, formatter: &mut DevtoolsFormatter<'_>) {
    formatter.buffer_mut().push(ComponentInfo::with_value(
      "id",
      std::any::type_name::<u64>(),
      self.message.id.to_string(),
    ));
    formatter.buffer_mut().push(ComponentInfo::with_value(
      "sender_id",
      std::any::type_name::<u32>(),
      self.message.sender_id.to_string(),
    ));
    formatter.buffer_mut().push(ComponentInfo::with_value(
      "text_len",
      std::any::type_name::<usize>(),
      self.message.text.len().to_string(),
    ));
  }
}

pub(super) struct ChatMessage;

impl Component for ChatMessage {
  type Props = ChatMessageProps;

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let render_start = Instant::now();
    let props = ctx.props::<Self::Props>().clone();
    let message = &props.message;
    let local = message.sender_id == props.local_user_id;
    let timestamp = format_chat_time(message.timestamp);
    let sender_name = user_display_name(message.sender_id, &message.sender_name, props.debug_user_ids);

    let element = Row::new()
      .width(Dimension::Pct(100.0))
      .align_items(Alignment::Start)
      .spacing(theme::SpacingSize::Md)
      .child(server_avatar(&message.sender_name, 36.0, false))
      .child(
        Column::new()
          .width(Dimension::Pct(100.0))
          .min_width(0.0)
          .flex(1.0)
          .spacing(theme::SpacingSize::Xs)
          .child(
            Row::new()
              .align_items(Alignment::Center)
              .spacing(theme::SpacingSize::Sm)
              .child(
                Text::new(&sender_name)
                  .variant(theme::TypographyStyle::Button)
                  .color(theme::PaletteColor::TextPrimary)
                  .selectable(true),
              )
              .child(chat_sender_badge(ctx, local))
              .child(
                Text::new(&timestamp)
                  .variant(theme::TypographyStyle::Caption)
                  .color(theme::PaletteColor::TextMuted)
                  .selectable(true),
              )
              .child(chat_message_id_badge(message.id, props.debug_user_ids))
              .child(pinned_badge(ctx, message.pinned)),
          )
          .child(chat_message_text(ctx, &message.text)),
      );
    let elapsed = render_start.elapsed();
    tracing::trace!(
      target: "chat-profile",
      "[chat-profile] chat_message_render id={} sender={} text_len={} ms={:.3}",
      message.id,
      message.sender_id,
      message.text.len(),
      elapsed.as_secs_f64() * 1000.0,
    );
    element
  }
}

fn chat_message_text(ctx: &mut Ctx, text: &str) -> Element {
  let mut style = ctx.theme().typography().description.clone();
  style.color = theme::palette().text_secondary;
  let source = chat_markdown_source(text);

  Column::new()
    .width(Dimension::Pct(100.0))
    .min_width(0.0)
    .clip()
    .child(
      ctx.mount::<Markdown>(
        MarkdownProps::new(source)
          .style(style)
          .width(Dimension::Pct(100.0))
          .selectable(true)
          .on_link_click(|link| open_link_in_browser(&browser_url_for_link(link.destination()))),
      ),
    )
    .into()
}

fn chat_sender_badge(ctx: &mut Ctx, local: bool) -> Element {
  if !local {
    return Row::new().into();
  }

  Text::new(&ctx.t("lobby.users.you"))
    .variant(theme::TypographyStyle::Caption)
    .color(theme::PaletteColor::TextMuted)
    .into()
}

fn chat_message_id_badge(message_id: u64, debug_user_ids: bool) -> Element {
  if !debug_user_ids {
    return Row::new().into();
  }

  Text::new(&format!("[msg:{message_id}]"))
    .variant(theme::TypographyStyle::Caption)
    .color(theme::PaletteColor::TextMuted)
    .selectable(true)
    .into()
}

fn pinned_badge(ctx: &mut Ctx, pinned: bool) -> Element {
  if !pinned {
    return Row::new().into();
  }

  Row::new()
    .align_items(Alignment::Center)
    .spacing(4.0)
    .padding_vertical(3.0)
    .padding_horizontal(6.0)
    .rounded(theme::RadiusSize::Sm)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised))
    .child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon: "pin",
      size: 11.0,
      color: theme::palette().text_muted,
    }))
    .child(
      Text::new(&ctx.t("lobby.text_channel.pinned"))
        .variant(theme::TypographyStyle::FieldLabel)
        .color(theme::PaletteColor::TextMuted),
    )
    .into()
}

#[derive(Clone, Copy)]
struct MessageTextRange {
  start: usize,
  end: usize,
  link: bool,
}

fn chat_markdown_source(text: &str) -> String {
  let started_at = Instant::now();
  let text = compact_chat_markdown_blocks(text);
  let ranges = message_text_ranges(&text);
  if ranges.iter().all(|range| !range.link) {
    let source = text.into_owned();
    tracing::trace!(
      target: "chat-profile",
      "[chat-profile] markdown_source_compute text_len={} links=false ms={:.3}",
      source.len(),
      started_at.elapsed().as_secs_f64() * 1000.0,
    );
    return source;
  }

  let mut output = String::with_capacity(text.len() + ranges.len() * 8);
  for range in ranges {
    let part = &text[range.start..range.end];
    if range.link {
      let url = browser_url_for_link(part);
      output.push('[');
      push_markdown_link_label(&mut output, part);
      output.push_str("](");
      push_markdown_link_destination(&mut output, &url);
      output.push(')');
    } else {
      output.push_str(part);
    }
  }
  tracing::trace!(
    target: "chat-profile",
    "[chat-profile] markdown_source_compute text_len={} links=true ms={:.3}",
    text.len(),
    started_at.elapsed().as_secs_f64() * 1000.0,
  );
  output
}

fn compact_chat_markdown_blocks(text: &str) -> std::borrow::Cow<'_, str> {
  if !text.contains('\n') && !text.lines().any(line_starts_with_markdown_list_marker) {
    return std::borrow::Cow::Borrowed(text);
  }

  let mut output = String::with_capacity(text.len() + 8);
  for line in text.split_inclusive('\n') {
    let (line, ending) = line.strip_suffix('\n').map_or((line, ""), |line| (line, "\n"));
    push_compact_chat_markdown_line(&mut output, line);
    if ending.is_empty() {
      continue;
    }
    if !line.ends_with("  ") {
      output.push_str("  ");
    }
    output.push_str(ending);
  }
  std::borrow::Cow::Owned(output)
}

fn line_starts_with_markdown_list_marker(line: &str) -> bool {
  let line = line.trim_start();
  line.starts_with("- ") || line.starts_with("* ") || ordered_markdown_marker_delimiter(line).is_some()
}

fn push_compact_chat_markdown_line(output: &mut String, line: &str) {
  let indent_len = line.len() - line.trim_start().len();
  let (indent, content) = line.split_at(indent_len);
  output.push_str(indent);

  if content.starts_with("- ") || content.starts_with("* ") {
    output.push('\\');
    output.push_str(content);
    return;
  }

  if let Some((delimiter_index, delimiter)) = ordered_markdown_marker_delimiter(content) {
    output.push_str(&content[..delimiter_index]);
    output.push('\\');
    output.push(delimiter);
    output.push_str(&content[delimiter_index + delimiter.len_utf8()..]);
    return;
  }

  output.push_str(content);
}

fn ordered_markdown_marker_delimiter(line: &str) -> Option<(usize, char)> {
  let delimiter_index = line.find(['.', ')'])?;
  if delimiter_index == 0 || !line[..delimiter_index].chars().all(|ch| ch.is_ascii_digit()) {
    return None;
  }
  let delimiter = line[delimiter_index..].chars().next()?;
  let marker_len = delimiter_index + delimiter.len_utf8();
  line[marker_len..]
    .starts_with([' ', '\t'])
    .then_some((delimiter_index, delimiter))
}

fn message_text_ranges(text: &str) -> Vec<MessageTextRange> {
  let mut ranges = Vec::new();
  let mut emitted_until = 0;
  let mut token_start = None;

  for (index, ch) in text.char_indices() {
    if ch.is_whitespace() {
      if let Some(start) = token_start.take() {
        if emitted_until < start {
          push_message_range(&mut ranges, emitted_until, start, false);
        }
        push_message_token_range(text, start, index, &mut ranges);
        emitted_until = index;
      }
    } else if token_start.is_none() {
      token_start = Some(index);
    }
  }

  if let Some(start) = token_start {
    if emitted_until < start {
      push_message_range(&mut ranges, emitted_until, start, false);
    }
    push_message_token_range(text, start, text.len(), &mut ranges);
    emitted_until = text.len();
  }

  if emitted_until < text.len() {
    push_message_range(&mut ranges, emitted_until, text.len(), false);
  }

  if ranges.is_empty() {
    ranges.push(MessageTextRange {
      start: 0,
      end: text.len(),
      link: false,
    });
  }

  ranges
}

fn push_message_range(ranges: &mut Vec<MessageTextRange>, start: usize, end: usize, link: bool) {
  if start == end {
    return;
  }

  if !link
    && let Some(last) = ranges.last_mut()
    && !last.link
    && last.end == start
  {
    last.end = end;
    return;
  }

  ranges.push(MessageTextRange { start, end, link });
}

fn push_message_token_range(text: &str, start: usize, end: usize, ranges: &mut Vec<MessageTextRange>) {
  let token = &text[start..end];
  let link_start = leading_link_start(token);
  let link_candidate = &token[link_start..];
  let link_len = trimmed_link_len(link_candidate);

  if link_len > 0 && is_link_candidate(&link_candidate[..link_len]) {
    if link_start > 0 {
      push_message_range(ranges, start, start + link_start, false);
    }
    push_message_range(ranges, start + link_start, start + link_start + link_len, true);
    if link_start + link_len < token.len() {
      push_message_range(ranges, start + link_start + link_len, end, false);
    }
  } else {
    push_message_range(ranges, start, end, false);
  }
}

fn leading_link_start(token: &str) -> usize {
  let mut start = 0;
  while start < token.len() {
    let Some(ch) = token[start..].chars().next() else {
      break;
    };
    if matches!(ch, '(' | '[' | '{' | '<' | '"' | '\'') {
      start += ch.len_utf8();
    } else {
      break;
    }
  }
  start
}

fn trimmed_link_len(token: &str) -> usize {
  let mut len = token.len();
  while len > 0 {
    let Some(ch) = token[..len].chars().next_back() else {
      break;
    };
    if matches!(
      ch,
      '.' | ',' | ';' | ':' | '!' | '?' | ')' | ']' | '}' | '>' | '"' | '\''
    ) {
      len -= ch.len_utf8();
    } else {
      break;
    }
  }
  len
}

fn is_link_candidate(token: &str) -> bool {
  if token.starts_with("http://") || token.starts_with("https://") || token.starts_with("www.") {
    return true;
  }

  let Some(dot) = token.rfind('.') else {
    return false;
  };
  if dot == 0 || dot + 1 >= token.len() {
    return false;
  }

  let host_end = token.find('/').unwrap_or(token.len());
  let host = &token[..host_end];
  let Some(tld) = host.rsplit('.').next() else {
    return false;
  };

  host.contains('.')
    && tld.len() >= 2
    && tld.chars().all(|ch| ch.is_ascii_alphabetic())
    && host
      .chars()
      .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-'))
}

fn push_markdown_link_label(output: &mut String, label: &str) {
  for ch in label.chars() {
    if matches!(ch, '\\' | '[' | ']') {
      output.push('\\');
    }
    output.push(ch);
  }
}

fn push_markdown_link_destination(output: &mut String, destination: &str) {
  output.push('<');
  for ch in destination.chars() {
    match ch {
      '<' => output.push_str("%3C"),
      '>' => output.push_str("%3E"),
      _ => output.push(ch),
    }
  }
  output.push('>');
}

fn browser_url_for_link(link: &str) -> String {
  if link.starts_with("http://") || link.starts_with("https://") {
    link.to_owned()
  } else {
    format!("https://{link}")
  }
}

fn open_link_in_browser(url: &str) {
  #[cfg(target_os = "windows")]
  let _ = Command::new("rundll32")
    .arg("url.dll,FileProtocolHandler")
    .arg(url)
    .spawn();

  #[cfg(target_os = "macos")]
  let _ = Command::new("open").arg(url).spawn();

  #[cfg(all(unix, not(target_os = "macos")))]
  let _ = Command::new("xdg-open").arg(url).spawn();
}

#[cfg(test)]
mod tests {
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
}
