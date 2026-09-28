//! Exercises the real macOS APIs. Run by hand with
//! `cargo test -p arcvox-desktop --test macos_platform -- --ignored --nocapture`
//! from a terminal that has Accessibility permission (the test binary
//! inherits the terminal's grant).
#![cfg(target_os = "macos")]

use arcvox_desktop::platform::macos::MacPlatform;
use arcvox_desktop::platform::Platform;

#[test]
#[ignore]
fn frontmost_app_is_reported() {
    let p = MacPlatform::new();
    let f = p.frontmost_app().expect("some app is frontmost");
    eprintln!("{f:?}");
    assert!(f.pid > 0);
    assert!(f.bundle_id.is_some() || f.name.is_some());
}

#[test]
#[ignore]
fn secure_input_is_off_in_a_plain_terminal() {
    assert!(!MacPlatform::new().secure_input_active());
}

#[test]
#[ignore]
fn accessibility_trust_query_does_not_prompt() {
    let trusted = MacPlatform::new().accessibility_trusted(false);
    eprintln!("trusted = {trusted}");
}
