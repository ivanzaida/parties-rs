#[path = "../../src/session/chat_history.rs"]
mod chat_history;

use chat_history::{
  ChatHistoryMessage, MAX_CACHED_MESSAGES_PER_CHANNEL, merge_chat_history_messages, merge_chat_messages,
};

#[derive(Clone, Debug, PartialEq, Eq)]
struct TestMessage {
  id: u64,
  timestamp: u64,
  text: String,
}

impl ChatHistoryMessage for TestMessage {
  fn chat_id(&self) -> u64 {
    self.id
  }

  fn chat_timestamp(&self) -> u64 {
    self.timestamp
  }
}

fn chat_message(id: u64) -> TestMessage {
  TestMessage {
    id,
    timestamp: id,
    text: format!("message {id}"),
  }
}

#[test]
fn prepended_history_keeps_loaded_history_and_latest_messages() {
  let mut messages = (51..=300).map(chat_message).collect::<Vec<_>>();

  merge_chat_history_messages(&mut messages, (1..=50).map(chat_message));

  assert_eq!(messages.len(), 300);
  assert_eq!(messages.first().map(|message| message.id), Some(1));
  assert_eq!(messages.last().map(|message| message.id), Some(300));
}

#[test]
fn live_messages_keep_newest_cache_window() {
  let mut messages = (1..=MAX_CACHED_MESSAGES_PER_CHANNEL as u64)
    .map(chat_message)
    .collect::<Vec<_>>();

  merge_chat_messages(
    &mut messages,
    [chat_message(MAX_CACHED_MESSAGES_PER_CHANNEL as u64 + 1)],
  );

  assert_eq!(messages.len(), MAX_CACHED_MESSAGES_PER_CHANNEL);
  assert_eq!(messages.first().map(|message| message.id), Some(2));
  assert_eq!(
    messages.last().map(|message| message.id),
    Some(MAX_CACHED_MESSAGES_PER_CHANNEL as u64 + 1)
  );
}

#[test]
fn history_merge_dedupes_by_id_and_sorts_by_timestamp_then_id() {
  let mut messages = vec![chat_message(20), chat_message(10)];
  let mut replacement = chat_message(10);
  replacement.timestamp = 30;
  replacement.text = "edited".to_owned();

  merge_chat_history_messages(&mut messages, [chat_message(5), replacement]);

  let ids = messages.iter().map(|message| message.id).collect::<Vec<_>>();
  assert_eq!(ids, vec![5, 20, 10]);
  assert_eq!(
    messages
      .iter()
      .find(|message| message.id == 10)
      .map(|message| message.text.as_str()),
    Some("edited")
  );
}
