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
  merge_chat_messages_with_trim(messages, incoming, ChatMessageTrimSide::None);
}

#[derive(Clone, Copy)]
enum ChatMessageTrimSide {
  None,
  Oldest,
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
      ChatMessageTrimSide::None => {}
      ChatMessageTrimSide::Oldest => {
        messages.drain(..trim);
      }
    }
  }
}
