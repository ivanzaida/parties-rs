use std::sync::Arc;

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
  pub source: ChatCommandSource,
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
      source: ChatCommandSource::Local,
    }
  }

  pub fn server_advertised(name: String, description: String, usage: String) -> Self {
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
      source: ChatCommandSource::Server,
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
