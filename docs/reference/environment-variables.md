# Environment Variables

## Client Runtime

| Variable | Purpose |
| --- | --- |
| `PARTIES_AEC_DELAY_MS` | Echo-cancellation delay tuning. |
| `PARTIES_LOG_FILTER` | Tracing filter override. |
| `PARTIES_LOG` | Tracing filter override. |
| `RUST_LOG` | Tracing filter fallback. |
| `PARTIES_LOG_DOMAIN` | Log domain filter. |
| `PARTIES_LOG_DOMAINS` | Log domain filter. |
| `PARTIES_LOG_LEVEL` | Log level override. |
| `PARTIES_PROFILE` | Enable profiler output. |
| `PARTIES_PROFILE_INTERVAL_MS` | Profiler interval. |
| `PARTIES_UPDATER_SIMULATE` | Simulate update availability. |
| `PARTIES_UPDATER_SIMULATE_VERSION` | Simulated update version. |

## Runtime Sentry

| Variable | Purpose |
| --- | --- |
| `PARTIES_SENTRY_DISABLED` | Disable Sentry when truthy. |
| `SENTRY_DISABLED` | Disable Sentry when truthy. |
| `PARTIES_SENTRY` | Disable Sentry when explicitly false. |
| `PARTIES_SENTRY_DSN` | Runtime Sentry DSN override. |
| `SENTRY_DSN` | Runtime Sentry DSN override. |
| `PARTIES_SENTRY_RELEASE` | Runtime Sentry release name. |
| `SENTRY_RELEASE` | Runtime Sentry release name. |
| `PARTIES_SENTRY_ENVIRONMENT` | Runtime Sentry environment. |
| `PARTIES_ENVIRONMENT` | Runtime Sentry environment. |
| `SENTRY_ENVIRONMENT` | Runtime Sentry environment. |
| `PARTIES_SENTRY_DEBUG` | Enable Sentry debug mode. |
| `SENTRY_DEBUG` | Enable Sentry debug mode. |

## Video

| Variable | Purpose |
| --- | --- |
| `PARTIES_SIMULATE_UNSUPPORTED_AV1` | Force unsupported macOS AV1 VideoToolbox path for testing. |
| `PARTIES_MACOS_ALLOW_CPU_VIDEO_FALLBACK` | Allow macOS CPU video fallback. |
| `PARTIES_MACOS_SOFTWARE_AV1` | Documented AV1 fallback switch; see current video implementation before relying on it. |

## CI And Build

| Variable | Purpose |
| --- | --- |
| `CARGO_PROFILE_RELEASE_DEBUG` | Set to `2` on Windows release builds to emit PDBs. |
| `SENTRY_AUTH_TOKEN` | Sentry CLI token for debug file upload. |
| `SENTRY_ORG` | Sentry organization slug. |
| `SENTRY_PROJECT` | Sentry project slug. |
| `SENTRY_URL` | Sentry base URL. |
| `SPARKLE_PUBLIC_KEY` | Public key embedded in macOS app bundle. |
| `SPARKLE_ED_PRIVATE_KEY` | Private key used to sign appcast update entries. |
| `MACOS_CERTIFICATE_P12` | Base64 Developer ID certificate. |
| `MACOS_CERTIFICATE_PASSWORD` | Certificate password. |
| `MACOS_APPLE_ID` | Apple ID for notarization. |
| `MACOS_APP_PASSWORD` | App-specific password for notarization. |
| `MACOS_TEAM_ID` | Apple team ID for notarization. |

## Music Bot

| Variable | Purpose |
| --- | --- |
| `SOUNDCLOUD_CLIENT_ID` | SoundCloud API client ID used by the plugin. |
| `SOUNDCLOUD_CLIENT_SECRET` | SoundCloud API client secret used by the plugin. |

