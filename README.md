# Arcvox

AI voice dictation for Traditional Chinese (Taiwan), English, and Japanese. Offline or self-hosted, with user-editable prompts.

## Layout

- `core/` Rust core crate `arcvox-core` (Apache-2.0)
- `apps/desktop/` Tauri 2 desktop client (macOS first, Apache-2.0)
- `apps/ios/` iOS client (planned)
- `server/` self-hosted engine server (planned, AGPL-3.0)

## Models

Arcvox can run speech recognition and text rewriting on the machine with downloaded model files. Manage them in Settings > Models. On macOS 26, Apple's built-in dictation and language model can be used with no download; the language model requires Apple Intelligence to be turned on. The Apple dictation language is installed from Settings > Providers with the "Install language" button and is shared with the system.

Model files are stored in the models directory shown on the Models page. Custom files can be imported: ggml `.bin` for speech recognition, GGUF for text. Each model's license is shown in the model list.

## License

Layered open core. The Rust core and the desktop and iOS clients are Apache-2.0 (see `LICENSE` and `core/LICENSE`). The self-hosted engine server under `server/` is AGPL-3.0 and carries its own license file. The hosted service's operations layer is not part of this repository.

## Trademark

"Arcvox" is a trademark of the project owner. Forks must use a different name and logo.
