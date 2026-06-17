# Troubleshooting

## Windows Build Is Slow

Cold Windows builds can spend most time compiling the native video bridge and heavy video/network dependencies. Cache key changes from `Cargo.lock` or native video files can trigger longer rebuilds.

## Sentry Says Project Not Found

Check:

- `SENTRY_ORG` is the org slug.
- `SENTRY_PROJECT` is the project slug.
- `SENTRY_URL` is the base URL, not an org/project URL.
- Token has project read/write/release access.

Run:

```powershell
npm exec --yes @sentry/cli -- info
npm exec --yes @sentry/cli -- projects list --org $env:SENTRY_ORG
```

## No Windows PDB Files

Set:

```powershell
$env:CARGO_PROFILE_RELEASE_DEBUG = "2"
```

Then rebuild release.

## macOS Video Payload Changes

Read [Weak Spots](../weak-spots.md) before changing H.264/H.265 payload format.

