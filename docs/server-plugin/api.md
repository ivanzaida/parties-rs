# Server Plugin API

Plugins implement `server_plugin::plugin::Plugin`.

Important hooks:

- `register`
- `shutdown`
- `on_chat_command`
- chat moderation/message hooks exposed through the ABI

The host wrapper exposes operations such as:

- Register chat commands.
- Create bot users.
- Set bot display names.
- Join bot voice.
- Send bot chat.
- Send bot audio.
- Read sessions, users, and channels.
- Find users by name.

Command definitions use `CommandDefinition::new(name, description, usage)` and can include a minimum role.

