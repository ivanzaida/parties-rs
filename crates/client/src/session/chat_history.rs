pub const MAX_CACHED_MESSAGES_PER_CHANNEL: usize = 250;

pub trait ChatHistoryMessage {
  fn chat_id(&self) -> u64;
  fn chat_timestamp(&self) -> u64;
}

pub fn merge_chat_messages<T: ChatHistoryMessage>(messages: &mut Vec<T>, incoming: impl IntoIterator<Item = T>) {
  merge_chat_messages_with_trim(messages, incoming, ChatMessageTrimSide::Oldest);
}

pub fn merge_chat_history_messages<T: ChatHistoryMessage>(
  messages: &mut Vec<T>,
  incoming: impl IntoIterator<Item = T>,
) {
  let previous_oldest_id = messages.iter().map(ChatHistoryMessage::chat_id).min();
  let incoming = incoming.into_iter().collect::<Vec<_>>();
  let prepending_older_history = previous_oldest_id.is_some_and(|oldest_id| {
    incoming
      .iter()
      .any(|message| message.chat_id() != 0 && message.chat_id() < oldest_id)
  });
  let trim_side = if prepending_older_history {
    ChatMessageTrimSide::Newest
  } else {
    ChatMessageTrimSide::Oldest
  };

  merge_chat_messages_with_trim(messages, incoming, trim_side);
}

#[derive(Clone, Copy)]
enum ChatMessageTrimSide {
  Oldest,
  Newest,
}

fn merge_chat_messages_with_trim<T: ChatHistoryMessage>(
  messages: &mut Vec<T>,
  incoming: impl IntoIterator<Item = T>,
  trim_side: ChatMessageTrimSide,
) {
  for message in incoming {
    if let Some(existing) = messages
      .iter_mut()
      .find(|existing| existing.chat_id() == message.chat_id())
    {
      *existing = message;
    } else {
      messages.push(message);
    }
  }

  messages.sort_by_key(|message| (message.chat_timestamp(), message.chat_id()));

  if messages.len() > MAX_CACHED_MESSAGES_PER_CHANNEL {
    let trim = messages.len() - MAX_CACHED_MESSAGES_PER_CHANNEL;
    match trim_side {
      ChatMessageTrimSide::Oldest => {
        messages.drain(..trim);
      }
      ChatMessageTrimSide::Newest => {
        messages.truncate(MAX_CACHED_MESSAGES_PER_CHANNEL);
      }
    }
  }
}
