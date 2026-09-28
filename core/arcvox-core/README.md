# arcvox-core

Platform-independent core of Arcvox: configuration, ASR and LLM providers, the processing pipeline, history, and the event protocol. Desktop clients link this crate in-process; the server wraps the same crate.

## Layout

- `types`, `protocol`: shared data and the versioned event stream.
- `config`: `config.json` store with validation, migration, atomic writes, and the editable base prompt.
- `providers`: `AsrProvider` and `LlmProvider` traits; HTTP (OpenAI-compatible) implementations; probe; registry.
- `steps`, `pipeline`: per-profile step chain (asr, opencc, llm, rules) with degradation.
- `history`: SQLite utterance history with retention.
- `engine`, `session`: public entry point.

## Tests

- `cargo test -p arcvox-core` runs unit, contract (wiremock), and protocol snapshot tests.
- `ARCVOX_LIVE=1 cargo test -p arcvox-core --test live -- --nocapture` runs the pipeline against local Speaches and Ollama; see `tests/live.rs` for the environment variables.

## Data locations

Config and prompts: `~/Library/Application Support/Arcvox` (macOS), `%APPDATA%\Arcvox` (Windows). History: same on macOS, `%LOCALAPPDATA%\Arcvox` on Windows. Recordings: `~/Library/Caches/Arcvox` (macOS), `%LOCALAPPDATA%\Arcvox` (Windows). Never under the install directory.

## License

Apache-2.0. See `../LICENSE`.
