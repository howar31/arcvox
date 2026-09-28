# kongwe-core

Platform-independent core of Kongwe: configuration, ASR and LLM providers, the processing pipeline, history, and the event protocol. Desktop clients link this crate in-process; the server wraps the same crate.

## Layout

- `types`, `protocol`: shared data and the versioned event stream.
- `config`: `config.json` store with validation, migration, atomic writes, and the editable base prompt.
- `providers`: `AsrProvider` and `LlmProvider` traits; HTTP (OpenAI-compatible) implementations; probe; registry.
- `steps`, `pipeline`: per-profile step chain (asr, opencc, llm, rules) with degradation.
- `history`: SQLite utterance history with retention.
- `engine`, `session`: public entry point.

## Tests

- `cargo test -p kongwe-core` runs unit, contract (wiremock), and protocol snapshot tests.
- `KONGWE_LIVE=1 cargo test -p kongwe-core --test live -- --nocapture` runs the pipeline against local Speaches and Ollama; see `tests/live.rs` for the environment variables.

## Data locations

Config and prompts: `~/Library/Application Support/Kongwe` (macOS), `%APPDATA%\Kongwe` (Windows). History: same on macOS, `%LOCALAPPDATA%\Kongwe` on Windows. Recordings: `~/Library/Caches/Kongwe` (macOS), `%LOCALAPPDATA%\Kongwe` (Windows). Never under the install directory.

## License

Apache-2.0. See `../LICENSE`.
