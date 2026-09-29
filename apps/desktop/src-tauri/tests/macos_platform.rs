//! Exercises the real macOS APIs. Run by hand with
//! `cargo test -p arcvox-desktop --test macos_platform -- --ignored --nocapture`
//! from a terminal that has Accessibility permission (the test binary
//! inherits the terminal's grant).
#![cfg(target_os = "macos")]

use arcvox_core::SecretStore;
use arcvox_desktop::audio::{capture::CpalSource, AudioMsg, AudioSource};
use arcvox_desktop::platform::macos::MacPlatform;
use arcvox_desktop::platform::{ClipboardSnapshot, Platform};
use arcvox_desktop::secrets::KeychainStore;
use arcvox_desktop::settings::MicrophoneChoice;

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
    p.clipboard_write_text("arcvox-test-before");
    let snap = p.clipboard_snapshot();
    assert!(!snap.items.is_empty());
    p.clipboard_write_text("arcvox-test-during");
    assert_eq!(p.clipboard_text().as_deref(), Some("arcvox-test-during"));
    p.clipboard_restore(snap);
    assert_eq!(p.clipboard_text().as_deref(), Some("arcvox-test-before"));
    p.clipboard_restore(ClipboardSnapshot::default());
    assert_eq!(p.clipboard_text(), None);
    p.clipboard_restore(original);
}

#[test]
#[ignore]
fn keychain_round_trip() {
    let s = KeychainStore::new();
    let key = "arcvox-test-key";
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

fn zh_tw_fixture() -> arcvox_core::types::AudioClip {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../core/arcvox-core/tests/fixtures/zh-tw-sample.wav"
    );
    let mut r = hound::WavReader::open(path).unwrap();
    let format = r.spec();
    assert_eq!(
        (format.channels, format.bits_per_sample),
        (1, 16),
        "{format:?}"
    );
    let samples = r
        .samples::<i16>()
        .map(|s| s.unwrap() as f32 / 32768.0)
        .collect();
    arcvox_core::types::AudioClip {
        samples,
        sample_rate: format.sample_rate,
    }
}

/// Reserves and installs Apple's zh_TW dictation asset for this binary. Run it
/// once before `apple_dictation_transcribes_fixture`.
#[test]
#[ignore]
fn apple_dictation_installs_zh_tw() {
    use arcvox_desktop::platform::macos::apple::{
        dictation_install, dictation_status, NativeStatus,
    };
    // The bridge must not run on the main thread.
    std::thread::spawn(|| {
        let mut last = -1i64;
        dictation_install("zh_TW", |p| {
            let pct = (p * 100.0) as i64;
            if pct / 10 != last / 10 {
                eprintln!("progress {pct}%");
                last = pct;
            }
        })
        .unwrap();
        assert_eq!(dictation_status("zh_TW"), NativeStatus::Ready);
    })
    .join()
    .unwrap();
}

#[tokio::test]
#[ignore]
async fn apple_dictation_transcribes_fixture() {
    use arcvox_core::providers::{AsrHint, AsrProvider};
    use arcvox_desktop::platform::macos::apple::{
        dictation_status, AppleAsrProvider, NativeStatus,
    };
    let status = tokio::task::spawn_blocking(|| dictation_status("zh_TW"))
        .await
        .unwrap();
    if status != NativeStatus::Ready {
        eprintln!("dictation not ready ({status:?}); skipping");
        return;
    }
    let p = AppleAsrProvider::new(
        "zh_TW".into(),
        "Apple".into(),
        std::time::Duration::from_secs(60),
    );
    let t = p
        .transcribe(
            &zh_tw_fixture(),
            &AsrHint {
                language: Some("zh-TW".into()),
                prompt: Some("Arcvox".into()),
            },
        )
        .await
        .unwrap();
    eprintln!("transcript: {t:?}");
    assert!(!t.text.trim().is_empty(), "{t:?}");
}

#[tokio::test]
#[ignore]
async fn apple_dictation_empty_clip_is_empty_text() {
    use arcvox_core::providers::{AsrHint, AsrProvider};
    use arcvox_desktop::platform::macos::apple::AppleAsrProvider;
    let p = AppleAsrProvider::new(
        "zh_TW".into(),
        "Apple".into(),
        std::time::Duration::from_secs(10),
    );
    let clip = arcvox_core::types::AudioClip {
        samples: Vec::new(),
        sample_rate: 16_000,
    };
    let t = p.transcribe(&clip, &AsrHint::default()).await.unwrap();
    assert_eq!(t.text, "");
}

/// A timeout sets the cancel flag and drops it while the bridge may still be
/// running; the bridge must stop reading it before its call returns.
#[tokio::test]
#[ignore]
async fn apple_dictation_timeout_cancels_cleanly() {
    use arcvox_core::providers::{AsrHint, AsrProvider};
    use arcvox_core::types::ErrorCode;
    use arcvox_desktop::platform::macos::apple::{
        dictation_status, AppleAsrProvider, NativeStatus,
    };
    let status = tokio::task::spawn_blocking(|| dictation_status("zh_TW"))
        .await
        .unwrap();
    if status != NativeStatus::Ready {
        eprintln!("dictation not ready ({status:?}); skipping");
        return;
    }
    let p = AppleAsrProvider::new(
        "zh_TW".into(),
        "Apple".into(),
        std::time::Duration::from_millis(5),
    );
    for _ in 0..5 {
        let e = p
            .transcribe(&zh_tw_fixture(), &AsrHint::default())
            .await
            .unwrap_err();
        assert_eq!(e.code, ErrorCode::ProviderTimeout, "{e:?}");
    }
    // Let the abandoned bridge calls wind down inside the process.
    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
}
