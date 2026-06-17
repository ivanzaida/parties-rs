# Sentry

The client uses Sentry at runtime for crash/error reporting and CI uploads Windows debug files for symbolication.

## Runtime Client

Runtime Sentry settings are controlled by:

- `PARTIES_SENTRY_DISABLED`
- `SENTRY_DISABLED`
- `PARTIES_SENTRY`
- `PARTIES_SENTRY_DSN`
- `SENTRY_DSN`
- `PARTIES_SENTRY_RELEASE`
- `SENTRY_RELEASE`
- `PARTIES_SENTRY_ENVIRONMENT`
- `PARTIES_ENVIRONMENT`
- `SENTRY_ENVIRONMENT`
- `PARTIES_SENTRY_DEBUG`
- `SENTRY_DEBUG`

## CI Debug File Upload

Required GitHub secrets:

- `SENTRY_AUTH_TOKEN`
- `SENTRY_ORG`
- `SENTRY_PROJECT`

Optional:

- `SENTRY_URL`

For the current self-hosted setup:

```text
SENTRY_URL=https://sentry.lurq.dev
SENTRY_ORG=sentry
SENTRY_PROJECT=parties-rs
```

The token must have access equivalent to:

- `org:read`
- `project:read`
- `project:write`
- `project:releases`

## Local Validation

```powershell
npm exec --yes @sentry/cli -- info
npm exec --yes @sentry/cli -- projects list --org $env:SENTRY_ORG
```

Build PDBs:

```powershell
$env:CARGO_PROFILE_RELEASE_DEBUG = "2"
cargo build --release --target x86_64-pc-windows-msvc --package client
```

Dry-run upload processing:

```powershell
npm exec --yes @sentry/cli -- debug-files upload --no-upload --org $env:SENTRY_ORG --project $env:SENTRY_PROJECT target/x86_64-pc-windows-msvc/release
```

Real upload:

```powershell
npm exec --yes @sentry/cli -- debug-files upload --org $env:SENTRY_ORG --project $env:SENTRY_PROJECT target/x86_64-pc-windows-msvc/release
```

