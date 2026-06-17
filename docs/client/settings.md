# Settings

Settings are grouped by page under `crates/client/src/ui/settings`.

## Pages

- Overview.
- Identity.
- Saved servers.
- Audio.
- Notifications.
- Stream/video.

## Behavior

Settings are stored in SQLite and loaded through `Storage`. Some settings update live services immediately after save:

- Notification sounds and volume update session notification playback.
- Hardware video decoding updates the session video preference.
- Hotkey settings affect local and global hotkey handling.

The settings UI can be opened as a full app-contained modal from the lobby or navigated as a route.

