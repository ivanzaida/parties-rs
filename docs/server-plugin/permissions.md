# Server Plugin Permissions

Implemented permissions:

- `read_sessions`
- `read_users`
- `read_channels`
- `read_chat`
- `moderate_chat`
- `create_chat_commands`
- `create_bot_users`
- `send_bot_chat`
- `join_bot_voice`
- `send_bot_audio`

Plugins declare permissions in `plugin.toml`. The host decides whether to allow a plugin and which requested permissions are granted.

