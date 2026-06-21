# Server Plugin Overview

`crates/server-plugin` defines the Rust side of the Parties plugin interface. It is both an ABI contract and a helper crate for plugin authors.

The API version is currently `1.1`.

## Responsibilities

- Define C-compatible ABI structs and callbacks.
- Define plugin permissions and manifest types.
- Wrap host callbacks in safer Rust methods.
- Provide a `Plugin` trait.
- Provide a registration macro that exports the required native symbols.
