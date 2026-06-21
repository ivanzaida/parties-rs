use std::sync::Arc;

use crate::network::protocol::control::{ChatCommandInputInfo, ChatCommandInputMode};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChatCommandInvocation {
  pub name: Arc<str>,
  pub arguments: Vec<Arc<str>>,
  pub source: ChatCommandSource,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandDefinition {
  pub name: Arc<str>,
  pub description_key: Arc<str>,
  pub description_is_i18n_key: bool,
  pub usage: Arc<str>,
  pub inputs: Arc<[CommandInputDefinition]>,
  pub source: ChatCommandSource,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandInputDefinition {
  pub argument_name: Arc<str>,
  pub mode: ChatCommandInputMode,
  pub min_chars: u16,
  pub debounce_ms: u16,
  pub max_results: u16,
  pub placeholder: Arc<str>,
}

impl From<ChatCommandInputInfo> for CommandInputDefinition {
  fn from(input: ChatCommandInputInfo) -> Self {
    Self {
      argument_name: Arc::from(input.argument_name),
      mode: input.mode,
      min_chars: input.min_chars,
      debounce_ms: input.debounce_ms,
      max_results: input.max_results,
      placeholder: Arc::from(input.placeholder),
    }
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChatCommandSource {
  Local,
  Server,
}

impl CommandDefinition {
  pub fn local_i18n(name: &'static str, description_key: &'static str, usage: &'static str) -> Self {
    Self {
      name: Arc::from(name),
      description_key: Arc::from(description_key),
      description_is_i18n_key: true,
      usage: Arc::from(usage),
      inputs: Arc::from([]),
      source: ChatCommandSource::Local,
    }
  }

  pub fn server_advertised(name: String, description: String, usage: String) -> Self {
    Self::server_advertised_with_inputs(name, description, usage, Vec::new())
  }

  pub fn server_advertised_with_inputs(
    name: String,
    description: String,
    usage: String,
    inputs: Vec<CommandInputDefinition>,
  ) -> Self {
    let name = if name.starts_with('/') {
      name
    } else {
      format!("/{name}")
    };
    let usage = if usage.trim().is_empty() { name.clone() } else { usage };

    Self {
      name: Arc::from(name),
      description_key: Arc::from(description),
      description_is_i18n_key: false,
      usage: Arc::from(usage),
      inputs: Arc::from(inputs),
      source: ChatCommandSource::Server,
    }
  }

  fn with_inputs(&self, inputs: Vec<CommandInputDefinition>) -> Self {
    Self {
      inputs: Arc::from(inputs),
      ..self.clone()
    }
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChatCommandParseError {
  Empty,
  UnterminatedQuotedArgument,
  Usage {
    command: String,
    usage: String,
  },
  InvalidType {
    argument: String,
    value: String,
    expected: ChatCommandExpectedType,
  },
  Unknown {
    command: String,
  },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChatCommandExpectedType {
  Number { min: String, max: String },
  Text,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ChatCommandRegistry {
  definitions: Arc<[CommandDefinition]>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChatCommandLiveQuery {
  pub command_name: String,
  pub argument_name: String,
  pub query: String,
  pub cursor_pos: u16,
  pub input: CommandInputDefinition,
}

impl ChatCommandRegistry {
  pub fn from_definitions(definitions: impl IntoIterator<Item = CommandDefinition>) -> Self {
    Self {
      definitions: definitions.into_iter().collect(),
    }
  }

  pub fn has_commands(&self) -> bool {
    !self.definitions.is_empty()
  }

  pub fn definitions(&self) -> &[CommandDefinition] {
    &self.definitions
  }

  pub fn with_server_inputs(
    &self,
    command_name: &str,
    inputs: impl IntoIterator<Item = CommandInputDefinition>,
  ) -> Self {
    let normalized_name = normalize_command_name(command_name);
    let inputs = inputs.into_iter().collect::<Vec<_>>();
    let definitions = self
      .definitions()
      .iter()
      .map(|definition| {
        if definition.source == ChatCommandSource::Server && definition.name.as_ref() == normalized_name {
          definition.with_inputs(inputs.clone())
        } else {
          definition.clone()
        }
      })
      .collect::<Vec<_>>();
    Self::from_definitions(definitions)
  }

  pub fn live_query_for_input(&self, input: &str) -> Option<ChatCommandLiveQuery> {
    let trimmed = input.trim_start();
    if !trimmed.starts_with('/') {
      return None;
    }

    let command_end = trimmed.find(char::is_whitespace)?;
    let command_name = &trimmed[..command_end];
    let definition = self.definition_for(command_name)?;
    if definition.source != ChatCommandSource::Server {
      return None;
    }

    let rest = &trimmed[command_end..];
    let argument_specs = definition
      .usage
      .split_whitespace()
      .skip(1)
      .filter_map(command_usage_argument)
      .collect::<Vec<_>>();
    let input = definition
      .inputs
      .iter()
      .find(|input| input.mode == ChatCommandInputMode::LiveQuery)
      .cloned()?;
    let argument_index = argument_specs
      .iter()
      .position(|argument| argument.name == input.argument_name.as_ref())?;
    let query = active_argument_query(rest, &argument_specs, argument_index)?;
    let cursor_pos = query.len().min(u16::MAX as usize) as u16;
    Some(ChatCommandLiveQuery {
      command_name: command_name.trim_start_matches('/').to_owned(),
      argument_name: input.argument_name.to_string(),
      query,
      cursor_pos,
      input,
    })
  }

  pub fn parse(&self, input: &str) -> Result<Option<ChatCommandInvocation>, ChatCommandParseError> {
    let trimmed = input.trim();
    if !trimmed.starts_with('/') {
      return Ok(None);
    }

    let mut tokens = command_tokens(trimmed)?;
    if tokens.is_empty() {
      return Err(ChatCommandParseError::Empty);
    }

    let command = tokens.remove(0);
    let Some(definition) = self.definition_for(command.as_ref()) else {
      return Err(ChatCommandParseError::Unknown {
        command: command.to_string(),
      });
    };

    if definition.source == ChatCommandSource::Local {
      self.validate_usage(definition, command.as_ref(), &tokens)?;
    }
    Ok(Some(ChatCommandInvocation {
      name: command,
      arguments: tokens,
      source: definition.source,
    }))
  }

  fn usage_for(&self, name: &str) -> String {
    self
      .definitions()
      .into_iter()
      .find(|definition| definition.name.as_ref() == name)
      .map(|definition| definition.usage.to_string())
      .unwrap_or_else(|| name.to_owned())
  }

  fn definition_for(&self, name: &str) -> Option<&CommandDefinition> {
    self
      .definitions()
      .iter()
      .find(|definition| definition.name.as_ref() == name)
  }

  fn validate_usage(
    &self,
    definition: &CommandDefinition,
    command: &str,
    values: &[Arc<str>],
  ) -> Result<(), ChatCommandParseError> {
    let argument_specs: Vec<_> = definition
      .usage
      .split_whitespace()
      .skip(1)
      .filter_map(command_usage_argument)
      .collect();
    let required_count = argument_specs.iter().filter(|argument| argument.required).count();
    if values.len() < required_count || values.len() > argument_specs.len() {
      return Err(ChatCommandParseError::Usage {
        command: command.to_owned(),
        usage: self.usage_for(command),
      });
    }

    for (argument, value) in argument_specs.iter().zip(values) {
      validate_argument_value(argument, value.as_ref())?;
    }

    Ok(())
  }
}

#[derive(Clone, Copy)]
struct CommandArgument<'a> {
  name: &'a str,
  ty: &'a str,
  required: bool,
}

fn command_usage_argument(part: &str) -> Option<CommandArgument<'_>> {
  let argument = part.strip_prefix('{')?.strip_suffix('}')?;
  let (name, ty) = argument.split_once(':')?;
  let required = !name.ends_with('?') && !ty.starts_with('?');
  Some(CommandArgument {
    name: name.strip_suffix('?').unwrap_or(name),
    ty: ty.strip_prefix('?').unwrap_or(ty),
    required,
  })
}

fn normalize_command_name(name: &str) -> String {
  if name.starts_with('/') {
    name.to_owned()
  } else {
    format!("/{name}")
  }
}

fn active_argument_query(rest: &str, arguments: &[CommandArgument<'_>], argument_index: usize) -> Option<String> {
  let rest = rest.trim_start();
  let argument = arguments.get(argument_index)?;
  if argument.ty.ends_with("...") {
    if argument_index == 0 {
      return Some(rest.to_owned());
    }
    return command_tokens(rest)
      .ok()
      .and_then(|tokens| tokens.get(argument_index).map(|token| token.to_string()));
  }

  command_tokens(rest)
    .ok()
    .and_then(|tokens| tokens.get(argument_index).map(|token| token.to_string()))
}

fn validate_argument_value(argument: &CommandArgument<'_>, value: &str) -> Result<(), ChatCommandParseError> {
  match argument.ty {
    "u8" => validate_number_range(argument.name, value, 0, u8::MAX as u64),
    "u16" => validate_number_range(argument.name, value, 0, u16::MAX as u64),
    "u32" => {
      let min = if argument.name == "userId" { 1 } else { 0 };
      validate_number_range(argument.name, value, min, u32::MAX as u64)
    }
    "u64" => validate_number_range(argument.name, value, 0, u64::MAX),
    "string" | "choice" | "role" => {
      if value.trim().is_empty() {
        return Err(invalid_string_value(argument.name, value));
      }
      Ok(())
    }
    _ => Ok(()),
  }
}

fn validate_number_range(argument: &str, value: &str, min: u64, max: u64) -> Result<(), ChatCommandParseError> {
  let valid = value.parse::<u64>().is_ok_and(|value| value >= min && value <= max);
  if valid {
    return Ok(());
  }

  Err(ChatCommandParseError::InvalidType {
    argument: argument.to_owned(),
    value: value.to_owned(),
    expected: ChatCommandExpectedType::Number {
      min: min.to_string(),
      max: max.to_string(),
    },
  })
}

fn invalid_string_value(argument: &str, value: &str) -> ChatCommandParseError {
  ChatCommandParseError::InvalidType {
    argument: argument.to_owned(),
    value: value.to_owned(),
    expected: ChatCommandExpectedType::Text,
  }
}

fn command_tokens(input: &str) -> Result<Vec<Arc<str>>, ChatCommandParseError> {
  let mut tokens = Vec::new();
  let mut current = String::new();
  let mut in_quotes = false;
  let mut escaped = false;

  for ch in input.chars() {
    if escaped {
      current.push(ch);
      escaped = false;
      continue;
    }

    match ch {
      '\\' if in_quotes => escaped = true,
      '"' => in_quotes = !in_quotes,
      ch if ch.is_whitespace() && !in_quotes => {
        if !current.is_empty() {
          tokens.push(Arc::from(std::mem::take(&mut current)));
        }
      }
      _ => current.push(ch),
    }
  }

  if escaped {
    current.push('\\');
  }
  if in_quotes {
    return Err(ChatCommandParseError::UnterminatedQuotedArgument);
  }
  if !current.is_empty() {
    tokens.push(Arc::from(current));
  }

  Ok(tokens)
}

#[cfg(test)]
#[path = "../../tests/unit/session/chat_commands.rs"]
mod tests;
