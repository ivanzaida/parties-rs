# Testing

## Whole Workspace

```powershell
cargo test --workspace
```

## Client Chat Tests

```powershell
cargo test -p client --test chat
```

## Protocol And Plugin Tests

Protocol tests live beside protocol modules. Plugin ABI and helper tests live in `crates/server-plugin/src/lib.rs`.

## Music Bot Tests

Music bot queue, command, and source behavior is tested in `crates/music-bot`.

