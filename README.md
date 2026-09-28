# Arcvox

AI voice dictation for Traditional Chinese (Taiwan), English, and Japanese. Offline or self-hosted, with user-editable prompts.

## Layout

- `core/` Rust core crate `arcvox-core` (Apache-2.0)
- `apps/desktop/` Tauri 2 desktop client (planned)
- `apps/ios/` iOS client (planned)
- `server/` self-hosted engine server (planned, AGPL-3.0)

## License

Layered open core. The Rust core and the desktop and iOS clients are Apache-2.0 (see `LICENSE` and `core/LICENSE`). The self-hosted engine server under `server/` is AGPL-3.0 and carries its own license file. The hosted service's operations layer is not part of this repository.

## Trademark

"Arcvox" is a trademark of the project owner. Forks must use a different name and logo.
