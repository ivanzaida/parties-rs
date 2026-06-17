# Identity

Client identity is built around local key material and seed phrases.

## Libraries

- `bip39` for seed phrase generation/restoration.
- `ed25519-dalek` for signing identity material.
- `sha2` for hashing.
- `getrandom` for secure randomness.

## User Flows

- Create a new identity.
- Show and confirm seed phrase.
- Restore identity from a seed phrase or private key.
- Persist identity locally through the storage layer.

## Server Trust

The connection layer can surface trust-on-first-use warnings when server identity information changes or needs confirmation.

