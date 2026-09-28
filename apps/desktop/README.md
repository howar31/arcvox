# Kongwe desktop (macOS)

Menu-bar dictation client. Hold the fn key to dictate; double-tap to lock hands-free dictation; press Esc to cancel.

## Build and run

Requirements: Rust stable, Node 24, Xcode command line tools, a code-signing identity (TCC permissions are bound to the signature; an unsigned build asks for permissions again after every rebuild).

    cd apps/desktop
    npm install
    APPLE_SIGNING_IDENTITY="Developer ID Application: ..." npx tauri build --debug
    open ../../target/debug/bundle/macos/Kongwe.app

The bundle is built into the repository root's `target/` directory, not under `apps/desktop/src-tauri/target`, because `apps/desktop/src-tauri` is a member of the Cargo workspace defined at the repository root.

## Permissions

- Microphone: requested by the system on the first recording; the prompt must be allowed.
- Accessibility: grant it in System Settings > Privacy & Security > Accessibility for the built `Kongwe.app`. Without it, the client cannot detect the fn key or insert text into other applications; the tray shows a status, Settings opens to the General page, and the client rechecks accessibility every 5 seconds until it is granted.
- fn key: set to "Do Nothing" in System Settings > Keyboard so the key reaches the client instead of being intercepted by the OS.

## Tests

    npm run check && npm test
    cargo test -p kongwe-desktop

## Data locations

Under `~/Library/Application Support/Kongwe/`:

- `config.json`: core engine configuration (providers, profiles, prompts), managed by `kongwe-core`.
- `client.json`: client-only settings (hotkeys, insertion methods, context opt-ins, UI language), managed by the desktop client.
- `history.sqlite`: dictation history.
- `prompts/`: saved prompt files referenced from profiles.

Under `~/Library/Caches/Kongwe/`:

- `recordings/`: cached audio recordings, pruned on the configured retention schedule.
