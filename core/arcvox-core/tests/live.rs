//! Runs the full pipeline against real local services. Skipped unless
//! ARCVOX_LIVE=1. Requires Speaches on :8000 and Ollama on :11434 (or the
//! URLs / models given by the env vars below).
//!
//!   ARCVOX_LIVE=1 \
//!   ARCVOX_LIVE_ASR_URL=http://localhost:8000/v1 ARCVOX_LIVE_ASR_MODEL=Systran/faster-whisper-large-v3 \
//!   ARCVOX_LIVE_LLM_URL=http://localhost:11434/v1 ARCVOX_LIVE_LLM_MODEL=gemma3 \
//!   cargo test -p arcvox-core --test live -- --nocapture

use std::sync::Arc;

use arcvox_core::{DataDir, Engine, Event, MemorySecretStore, SessionRequest, SAMPLE_RATE};
use tokio_stream::StreamExt;
use url::Url;

fn env(name: &str, default: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| default.to_string())
}

fn load_fixture() -> Vec<f32> {
    let bytes = include_bytes!("fixtures/zh-tw-sample.wav");
    let reader = hound::WavReader::new(std::io::Cursor::new(&bytes[..])).expect("fixture is a wav");
    let spec = reader.spec();
    assert_eq!(spec.channels, 1, "fixture must be mono");
    assert_eq!(spec.sample_rate, SAMPLE_RATE, "fixture must be 16 kHz");
    reader
        .into_samples::<i16>()
        .map(|s| s.unwrap() as f32 / i16::MAX as f32)
        .collect()
}

#[tokio::test]
async fn dictation_round_trip_through_local_services() {
    if std::env::var("ARCVOX_LIVE").as_deref() != Ok("1") {
        eprintln!("skipped: set ARCVOX_LIVE=1 to run against local services");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let engine = Engine::open(
        DataDir::custom(tmp.path()),
        Arc::new(MemorySecretStore::default()),
    )
    .await
    .unwrap();
    engine
        .config()
        .update(|c| {
            *c = arcvox_core::config::Config::http_test_config();
            c.providers[0].base_url =
                Some(Url::parse(&env("ARCVOX_LIVE_ASR_URL", "http://localhost:8000/v1")).unwrap());
            c.providers[0].model = Some(env(
                "ARCVOX_LIVE_ASR_MODEL",
                "Systran/faster-whisper-large-v3",
            ));
            c.providers[1].base_url =
                Some(Url::parse(&env("ARCVOX_LIVE_LLM_URL", "http://localhost:11434/v1")).unwrap());
            c.providers[1].model = Some(env("ARCVOX_LIVE_LLM_MODEL", "gemma3"));
            c.general.asr_timeout_secs = 120;
            c.general.llm_timeout_secs = 120;
        })
        .unwrap();

    let session = engine.start_session(SessionRequest::default());
    session.push_audio(&load_fixture());
    session.finish();

    let mut stream = session.events();
    let mut refined = None;
    let mut degraded = true;
    while let Some(event) = stream.next().await {
        eprintln!("{}", serde_json::to_string(&event).unwrap());
        match event {
            Event::Refined { text, degraded: d } => {
                refined = Some(text);
                degraded = d;
            }
            Event::Done { .. } => break,
            _ => {}
        }
    }
    let refined = refined.expect("pipeline produced refined text");
    assert!(!degraded, "LLM step must succeed against the live service");
    assert!(
        refined.contains("四點"),
        "self-correction should keep the corrected time: {refined}"
    );
    assert!(
        !refined.contains("三點"),
        "self-correction should drop the wrong time: {refined}"
    );
    assert!(
        refined
            .chars()
            .any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c)),
        "output has CJK: {refined}"
    );
}
