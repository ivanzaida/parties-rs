# Client Configuration

Most user-facing configuration is stored in SQLite through `AppSettings`.

## Settings Pages

- Overview and identity.
- Saved servers.
- Audio devices and voice controls.
- Notification sounds and volume.
- Stream/webcam/video settings.

## Live Settings

Some settings affect live services immediately:

- Notification audio settings update session voice state.
- Video hardware decoding preference updates the session.
- Hotkeys are enabled or disabled depending on focus and settings state.

## Environment Overrides

Runtime environment variables are listed in [Environment Variables](../reference/environment-variables.md).

