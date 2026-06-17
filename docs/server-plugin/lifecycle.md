# Server Plugin Lifecycle

1. The host loads a dynamic library and reads `plugin.toml`.
2. The host checks `api_version`.
3. The plugin registration symbol initializes the plugin instance.
4. The plugin receives host callbacks and manifest variables.
5. The plugin registers commands or other hooks.
6. Runtime callbacks invoke plugin methods.
7. Shutdown tears down background workers and clears plugin state.

Plugins should keep host handles only through the safe wrapper types exposed by `server-plugin`.

