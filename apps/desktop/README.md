# Arcvox desktop (macOS)

Menu-bar dictation client. Hold the fn key to dictate; double-tap to lock hands-free dictation; press Esc to cancel.

## Build

Requirements: Rust stable, Node 24, Xcode command line tools, a code-signing identity (TCC permissions are bound to the signature; an unsigned build asks for permissions again after every rebuild).

    cd apps/desktop
    npm install
    APPLE_SIGNING_IDENTITY="Developer ID Application: ..." npx tauri build --debug
    open src-tauri/target/debug/bundle/macos/Arcvox.app

## Permissions

Microphone (prompted on first recording) and Accessibility (System Settings > Privacy & Security > Accessibility). The fn key must be set to "Do Nothing" in System Settings > Keyboard.

## Tests

    npm run check && npm test
    cargo test -p arcvox-desktop
