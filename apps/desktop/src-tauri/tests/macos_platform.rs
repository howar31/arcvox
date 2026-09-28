//! Exercises the real macOS APIs. Run by hand with
//! `cargo test -p kongwe-desktop --test macos_platform -- --ignored --nocapture`
//! from a terminal that has Accessibility permission (the test binary
//! inherits the terminal's grant).
#![cfg(target_os = "macos")]

use kongwe_core::SecretStore;
use kongwe_desktop::audio::{capture::CpalSource, AudioMsg, AudioSource};
use kongwe_desktop::platform::macos::MacPlatform;
use kongwe_desktop::platform::{ClipboardSnapshot, Platform};
use kongwe_desktop::secrets::KeychainStore;
use kongwe_desktop::settings::MicrophoneChoice;

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

#[test]
#[ignore]
fn pasteboard_round_trip_keeps_text() {
    let p = MacPlatform::new();
    // Keep the owner's real clipboard intact across this test.
    let original = p.clipboard_snapshot();
    p.clipboard_write_text("kongwe-test-before");
    let snap = p.clipboard_snapshot();
    assert!(!snap.items.is_empty());
    p.clipboard_write_text("kongwe-test-during");
    assert_eq!(p.clipboard_text().as_deref(), Some("kongwe-test-during"));
    p.clipboard_restore(snap);
    assert_eq!(p.clipboard_text().as_deref(), Some("kongwe-test-before"));
    p.clipboard_restore(ClipboardSnapshot::default());
    assert_eq!(p.clipboard_text(), None);
    p.clipboard_restore(original);
}

#[test]
#[ignore]
fn keychain_round_trip() {
    let s = KeychainStore::new();
    let key = "kongwe-test-key";
    s.delete(key).unwrap();
    assert_eq!(s.get(key).unwrap(), None);
    s.set(key, "v1").unwrap();
    assert_eq!(s.get(key).unwrap().as_deref(), Some("v1"));
    s.set(key, "v2").unwrap();
    assert_eq!(s.get(key).unwrap().as_deref(), Some("v2"));
    s.delete(key).unwrap();
    assert_eq!(s.get(key).unwrap(), None);
}

#[test]
#[ignore]
fn captures_half_a_second_from_the_builtin_microphone() {
    let src = CpalSource;
    eprintln!("devices: {:?}", src.list_devices());
    let (tx, rx) = std::sync::mpsc::channel();
    let handle = src
        .start(&MicrophoneChoice::BuiltIn, tx)
        .expect("stream starts");
    let mut total = 0usize;
    let mut rate = 0;
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(600);
    while std::time::Instant::now() < deadline {
        if let Ok(AudioMsg::Frames { rate: r, mono }) =
            rx.recv_timeout(std::time::Duration::from_millis(100))
        {
            rate = r;
            total += mono.len();
        }
    }
    handle.stop();
    eprintln!("rate {rate}, samples {total}");
    assert!(rate >= 16_000);
    assert!(total as f32 > rate as f32 * 0.3);
}
