# Plugin Architecture

Plugin support is defined in `crates/server-plugin`.

## ABI Layer

The `abi` module mirrors the native server plugin ABI. It defines:

- API version header.
- Session, user, channel, chat, command, and bot types.
- Host callbacks.
- Registration callbacks.
- Chat command and chat message hooks.

## Rust Helper Layer

The crate also provides safe wrappers for:

- Plugin manifests.
- Permissions.
- Host access.
- Command registration.
- Bot user management.
- Chat command invocation parsing.
- Plugin registration macros.

## Registration

Plugins implement `server_plugin::plugin::Plugin` and use the registration macro to export the ABI symbols expected by the host.

