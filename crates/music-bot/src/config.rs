use server_plugin::{PluginError, plugin::Context};

const SOUNDCLOUD_CLIENT_ID_VARIABLE: &str = "soundcloud_client_id";
const SOUNDCLOUD_CLIENT_SECRET_VARIABLE: &str = "soundcloud_client_secret";

pub(crate) struct BotConfig {
  pub(crate) soundcloud: SoundCloudConfig,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SoundCloudConfig {
  pub(crate) client_id: String,
  pub(crate) client_secret: String,
}

impl BotConfig {
  pub(crate) fn from_context(context: &Context<'_>) -> Result<Self, PluginError> {
    Ok(Self {
      soundcloud: SoundCloudConfig {
        client_id: plugin_variable(context, SOUNDCLOUD_CLIENT_ID_VARIABLE)?,
        client_secret: plugin_variable(context, SOUNDCLOUD_CLIENT_SECRET_VARIABLE)?,
      },
    })
  }
}

fn plugin_variable(context: &Context<'_>, key: &'static str) -> Result<String, PluginError> {
  context
    .variables()?
    .into_iter()
    .find(|variable| variable.key == key)
    .map(|variable| variable.value.trim().to_owned())
    .filter(|value| !value.is_empty())
    .ok_or(PluginError::MissingPluginVariable(key))
}
