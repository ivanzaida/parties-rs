# Updates

The client can check for updates, stage an executable, and restart into the staged update.

## macOS

CI packages a signed and notarized DMG and generates a Sparkle appcast. The app bundle contains Sparkle metadata including feed URL and public key.

## Simulation

Update behavior can be simulated with:

- `PARTIES_UPDATER_SIMULATE`
- `PARTIES_UPDATER_SIMULATE_VERSION`

