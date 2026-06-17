# Code Style

## Rust

Use the repository `rustfmt.toml` and `clippy.toml`.

```powershell
cargo fmt --all
cargo clippy --workspace --all-targets
```

## Local Patterns

- Prefer existing service/session boundaries before adding new abstractions.
- Keep platform-specific behavior in platform files where possible.
- Keep hot audio/video paths allocation-aware.
- Keep UI components consistent with existing `lurq` patterns.
- Keep release artifact names stable even if crate package names change.

