//! Runs against real model files. Ignored by default.
//! KONGWE_MODELS_DIR=/path cargo test -p kongwe-inference --features metal --test real_models -- --ignored --nocapture
use kongwe_core::providers::{AsrHint, AsrProvider, LlmOptions, LlmProvider};
use kongwe_core::types::{AudioClip, ChatMessage, ErrorCode};
use kongwe_inference::llama::LlamaLlmProvider;
use kongwe_inference::whisper::WhisperAsrProvider;
use std::path::PathBuf;
use std::time::Duration;

fn models_dir() -> Option<PathBuf> {
    std::env::var_os("KONGWE_MODELS_DIR").map(PathBuf::from)
}

fn fixture() -> AudioClip {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../kongwe-core/tests/fixtures/zh-tw-sample.wav"
    );
    let mut r = hound::WavReader::open(path).unwrap();
    let samples = r
        .samples::<i16>()
        .map(|s| s.unwrap() as f32 / 32768.0)
        .collect();
    AudioClip {
        samples,
        sample_rate: 16_000,
    }
}

#[tokio::test]
#[ignore]
async fn whisper_tiny_transcribes_fixture() {
    let Some(dir) = models_dir() else { return };
    let p = WhisperAsrProvider::new(
        dir.join("ggml-tiny-q5_1.bin"),
        "tiny".into(),
        Duration::from_secs(60),
        true,
    );
    let t = p
        .transcribe(
            &fixture(),
            &AsrHint {
                language: Some("zh-TW".into()),
                prompt: None,
            },
        )
        .await
        .unwrap();
    println!("transcript: {t:?}");
    assert!(!t.text.trim().is_empty(), "{t:?}");
}

#[tokio::test]
#[ignore]
async fn abort_flag_stops_transcription() {
    let Some(dir) = models_dir() else { return };
    let p = WhisperAsrProvider::new(
        dir.join("ggml-tiny-q5_1.bin"),
        "tiny".into(),
        Duration::from_millis(1),
        true,
    );
    // A 1 ms timeout forces the abort path; the call must return Cancelled or ProviderTimeout, never hang.
    let err = p
        .transcribe(&fixture(), &AsrHint::default())
        .await
        .unwrap_err();
    assert!(
        matches!(
            err.code,
            kongwe_core::types::ErrorCode::ProviderTimeout
                | kongwe_core::types::ErrorCode::Cancelled
        ),
        "{err:?}"
    );
}

#[tokio::test]
#[ignore]
async fn truncated_model_is_model_corrupt() {
    let Some(dir) = models_dir() else { return };
    let bytes = std::fs::read(dir.join("ggml-tiny-q5_1.bin")).unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let f = tmp.path().join("truncated.bin");
    std::fs::write(&f, &bytes[..bytes.len() / 2]).unwrap();
    // CPU only: a failed load on the Metal backend leaves device buffers behind that
    // ggml asserts on at process exit.
    let p = WhisperAsrProvider::new(f, "tiny".into(), Duration::from_secs(60), false);
    let err = p
        .transcribe(&fixture(), &AsrHint::default())
        .await
        .unwrap_err();
    assert_eq!(
        err.code,
        kongwe_core::types::ErrorCode::ModelCorrupt,
        "{err:?}"
    );
}

#[tokio::test]
#[ignore]
async fn llama_small_model_completes() {
    let Some(dir) = models_dir() else { return };
    let p = LlamaLlmProvider::new(
        dir.join("SmolLM2-135M-Instruct-Q8_0.gguf"),
        "small".into(),
        true,
    );
    let out = p
        .complete(
            vec![
                ChatMessage::system("Reply with one word."),
                ChatMessage::user("Say hello."),
            ],
            &LlmOptions {
                temperature: 0.0,
                max_tokens: Some(16),
                timeout: Duration::from_secs(60),
            },
        )
        .await
        .unwrap();
    println!("completion: {out:?}");
    assert!(!out.trim().is_empty());
}

#[tokio::test]
#[ignore]
async fn cancel_flag_stops_generation() {
    let Some(dir) = models_dir() else { return };
    let p = LlamaLlmProvider::new(
        dir.join("SmolLM2-135M-Instruct-Q8_0.gguf"),
        "small".into(),
        true,
    );
    p.load().await.unwrap();
    let err = p
        .complete(
            vec![ChatMessage::user("Write a very long story.")],
            &LlmOptions {
                temperature: 0.7,
                max_tokens: Some(4096),
                timeout: Duration::from_millis(200),
            },
        )
        .await
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::ProviderTimeout, "{err:?}");
}
