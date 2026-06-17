# CI Secrets

## Client Release

| Secret | Required For |
| --- | --- |
| `SENTRY_AUTH_TOKEN` | Upload Windows PDB/debug files to Sentry. |
| `SENTRY_ORG` | Sentry organization slug. |
| `SENTRY_PROJECT` | Sentry project slug. |
| `SENTRY_URL` | Optional Sentry base URL. |
| `SPARKLE_PUBLIC_KEY` | Embedded in macOS app bundle. |
| `SPARKLE_ED_PRIVATE_KEY` | Signs Sparkle appcast entries. |
| `MACOS_CERTIFICATE_P12` | Developer ID certificate. |
| `MACOS_CERTIFICATE_PASSWORD` | Certificate password. |
| `MACOS_APPLE_ID` | Apple notarization account. |
| `MACOS_APP_PASSWORD` | Apple app-specific password. |
| `MACOS_TEAM_ID` | Apple developer team ID. |

## Current Sentry Values

```text
SENTRY_URL=https://sentry.lurq.dev
SENTRY_ORG=sentry
SENTRY_PROJECT=parties-rs
```

## Music Bot Release

The music bot workflow currently uses the default GitHub token for release publishing and does not require external secrets for CI packaging. Runtime SoundCloud credentials are plugin variables, not GitHub release secrets.

