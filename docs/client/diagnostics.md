# Diagnostics

Diagnostics include logs, profiling, Sentry, and platform-specific debug helpers.

## Logging

Logging uses `tracing`, `tracing-subscriber`, `tracing-appender`, and `tracing-log`.

Relevant env vars:

- `PARTIES_LOG_FILTER`
- `PARTIES_LOG`
- `RUST_LOG`
- `PARTIES_LOG_DOMAIN`
- `PARTIES_LOG_DOMAINS`
- `PARTIES_LOG_LEVEL`

## Sentry

The client initializes Sentry unless disabled. See [Sentry](../development/sentry.md).

## Profiling

`PARTIES_PROFILE` enables profiler output. `PARTIES_PROFILE_INTERVAL_MS` controls the interval.

## Windows Symbols

Windows diagnostics can use `_NT_SYMBOL_PATH` for symbol lookup.

