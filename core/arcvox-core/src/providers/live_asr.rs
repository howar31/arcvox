//! Speech to text over Gemini's Live API: a WebSocket session that takes the
//! audio while it is recorded and answers with the text so far as it goes.
//! Turn detection on the service is off; the start and the end of the
//! recording mark the one turn.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use base64::Engine as _;
use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tokio::time::{sleep_until, timeout, Instant};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::protocol::CloseFrame;
use tokio_tungstenite::tungstenite::{Error as WsError, Message};
use url::Url;

use crate::providers::http_common::{empty_reply, kept_answer};
use crate::providers::live::{LiveInput as Input, LiveStream, Partial};
use crate::providers::{AsrHint, AsrProvider, ProviderError};
use crate::types::{AudioClip, ErrorCode, Transcript};
use crate::SAMPLE_RATE;

/// Samples sent in one message: 100 ms.
const CHUNK: usize = SAMPLE_RATE as usize / 10;
/// The service ends a session after this long.
pub const LIVE_LIMIT_SECS: u32 = 600;

#[derive(Clone)]
struct Settings {
    url: Url,
    model: String,
    api_key: Option<String>,
    extra_headers: BTreeMap<String, String>,
    label: String,
    timeout: Duration,
    send_terms: bool,
    mode: Option<String>,
}

pub struct LiveAsrProvider {
    settings: Arc<Settings>,
}

impl LiveAsrProvider {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        url: &Url,
        model: impl Into<String>,
        api_key: Option<String>,
        extra_headers: BTreeMap<String, String>,
        label: impl Into<String>,
        timeout: Duration,
        send_terms: bool,
        mode: Option<String>,
    ) -> Self {
        let model: String = model.into();
        Self {
            settings: Arc::new(Settings {
                url: url.clone(),
                // The session names the model `models/<id>`.
                model: if model.starts_with("models/") {
                    model
                } else {
                    format!("models/{model}")
                },
                api_key,
                extra_headers,
                label: label.into(),
                timeout,
                send_terms,
                mode,
            }),
        }
    }

    /// Opens a session; `partial` gets the text so far each time it changes.
    /// Must be called on a tokio runtime.
    fn open(&self, hint: &AsrHint, partial: Partial) -> LiveStream {
        let settings = self.settings.clone();
        let setup = setup_message(&settings, hint);
        LiveStream::spawn(move |rx| async move { run(&settings, setup, rx, partial).await })
    }
}

#[async_trait]
impl AsrProvider for LiveAsrProvider {
    /// A whole recording through one session, sent as fast as it goes.
    async fn transcribe(
        &self,
        audio: &AudioClip,
        hint: &AsrHint,
    ) -> Result<Transcript, ProviderError> {
        let stream = self.open(hint, Arc::new(|_| {}));
        for chunk in audio.samples.chunks(CHUNK) {
            stream.push(chunk);
        }
        stream.finish().await
    }

    fn open_live(&self, hint: &AsrHint, partial: Partial) -> Option<LiveStream> {
        Some(self.open(hint, partial))
    }

    fn streams(&self) -> bool {
        true
    }

    fn live_limit_secs(&self) -> Option<u32> {
        Some(LIVE_LIMIT_SECS)
    }
}

fn setup_message(s: &Settings, hint: &AsrHint) -> String {
    let mut transcription = serde_json::Map::new();
    if s.send_terms && !hint.terms.is_empty() {
        transcription.insert("customVocabulary".into(), serde_json::json!(hint.terms));
    }
    if let Some(mode) = &s.mode {
        transcription.insert("mode".into(), serde_json::json!(mode.to_uppercase()));
    }
    serde_json::json!({
        "setup": {
            "model": s.model,
            "generationConfig": { "responseModalities": ["TEXT"] },
            "inputAudioTranscription": transcription,
            "realtimeInputConfig": { "automaticActivityDetection": { "disabled": true } },
        }
    })
    .to_string()
}

fn audio_message(samples: &[f32]) -> String {
    let mut pcm = Vec::with_capacity(samples.len() * 2);
    for s in samples {
        pcm.extend_from_slice(&((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
    }
    serde_json::json!({
        "realtimeInput": {
            "audio": {
                "data": base64::engine::general_purpose::STANDARD.encode(pcm),
                "mimeType": format!("audio/pcm;rate={SAMPLE_RATE}"),
            }
        }
    })
    .to_string()
}

/// What one message from the service says.
#[derive(Debug, Default, PartialEq)]
struct Said {
    setup_complete: bool,
    interim: Option<String>,
    final_text: Option<String>,
    turn_complete: bool,
    usage: Option<serde_json::Value>,
    error: Option<String>,
    go_away: bool,
}

fn read(raw: &str) -> Option<Said> {
    let v: serde_json::Value = serde_json::from_str(raw).ok()?;
    let content = &v["serverContent"];
    let text = |key: &str| content[key]["text"].as_str().map(str::to_string);
    Some(Said {
        setup_complete: v.get("setupComplete").is_some(),
        interim: text("interimInputTranscription"),
        final_text: text("inputTranscription"),
        turn_complete: content["turnComplete"].as_bool().unwrap_or(false),
        usage: v.get("usageMetadata").cloned(),
        error: v.get("error").map(|e| {
            e["message"]
                .as_str()
                .map_or_else(|| e.to_string(), str::to_string)
        }),
        go_away: v.get("goAway").is_some(),
    })
}

fn message_text(msg: &Message) -> Option<String> {
    match msg {
        Message::Text(t) => Some(t.as_str().to_string()),
        Message::Binary(b) => String::from_utf8(b.to_vec()).ok(),
        _ => None,
    }
}

fn closed(label: &str, frame: Option<&CloseFrame>) -> ProviderError {
    let reason = frame
        .map(|f| f.reason.as_str().trim().to_string())
        .unwrap_or_default();
    let code = frame.map(|f| u16::from(f.code));
    let mut err = ProviderError::new(
        // 1008: the service refused the request, the key among others.
        if matches!(code, Some(1008)) && reason.to_lowercase().contains("key") {
            ErrorCode::ProviderUnauthorized
        } else {
            ErrorCode::ProviderError
        },
        match code {
            Some(c) => format!("{label}: the service closed the session ({c})"),
            None => format!("{label}: the service closed the session"),
        },
    );
    if !reason.is_empty() {
        err.answer = Some(reason);
    }
    err
}

fn connect_error(label: &str, e: WsError) -> ProviderError {
    match e {
        WsError::Http(resp) => {
            let status = resp.status().as_u16();
            let body = resp
                .body()
                .as_deref()
                .map(|b| String::from_utf8_lossy(b).to_string())
                .unwrap_or_default();
            let mut err = ProviderError::new(
                match status {
                    401 | 403 => ErrorCode::ProviderUnauthorized,
                    _ => ErrorCode::ProviderError,
                },
                format!("{label}: the service refused the session (HTTP {status})"),
            );
            err.status = Some(status);
            if !body.trim().is_empty() {
                err.answer = serde_json::from_str::<serde_json::Value>(&body)
                    .ok()
                    .and_then(|v| v["error"]["message"].as_str().map(str::to_string));
                err.detail = Some(kept_answer(&body));
            }
            err
        }
        other => ProviderError::new(
            ErrorCode::ProviderUnreachable,
            format!("{label}: could not reach the service: {other}"),
        ),
    }
}

async fn run(
    s: &Settings,
    setup: String,
    mut input: mpsc::UnboundedReceiver<Input>,
    partial: Partial,
) -> Result<Transcript, ProviderError> {
    let label = s.label.as_str();
    let mut req = s
        .url
        .as_str()
        .into_client_request()
        .map_err(|e| ProviderError::new(ErrorCode::InvalidConfig, format!("{label}: {e}")))?;
    let headers = req.headers_mut();
    if let Some(key) = &s.api_key {
        if let Ok(v) = HeaderValue::from_str(key) {
            headers.insert("x-goog-api-key", v);
        }
    }
    for (k, v) in &s.extra_headers {
        if let (Ok(name), Ok(value)) = (
            k.parse::<tokio_tungstenite::tungstenite::http::HeaderName>(),
            HeaderValue::from_str(v),
        ) {
            headers.insert(name, value);
        }
    }
    let late = || {
        ProviderError::new(
            ErrorCode::ProviderTimeout,
            format!("{label}: the service did not answer in time"),
        )
    };
    let (ws, _) = timeout(s.timeout, tokio_tungstenite::connect_async(req))
        .await
        .map_err(|_| late())?
        .map_err(|e| connect_error(label, e))?;
    let (mut sink, mut stream) = ws.split();
    let send_error =
        |e: WsError| ProviderError::new(ErrorCode::ProviderUnreachable, format!("{label}: {e}"));
    sink.send(Message::Text(setup.into()))
        .await
        .map_err(send_error)?;

    // The service confirms the setup before it takes audio.
    let ready = Instant::now() + s.timeout;
    loop {
        let msg = tokio::select! {
            m = stream.next() => m,
            _ = sleep_until(ready) => return Err(late()),
        };
        match msg {
            Some(Ok(Message::Close(frame))) => return Err(closed(label, frame.as_ref())),
            Some(Ok(m)) => {
                let Some(said) = message_text(&m).as_deref().and_then(read) else {
                    continue;
                };
                if let Some(e) = said.error {
                    let mut err = ProviderError::new(
                        ErrorCode::ProviderError,
                        format!("{label}: the service refused the session"),
                    );
                    err.answer = Some(e);
                    return Err(err);
                }
                if said.setup_complete {
                    break;
                }
            }
            Some(Err(e)) => return Err(send_error(e)),
            None => return Err(closed(label, None)),
        }
    }
    sink.send(Message::Text(
        r#"{"realtimeInput":{"activityStart":{}}}"#.into(),
    ))
    .await
    .map_err(send_error)?;

    let mut pending: Vec<f32> = Vec::with_capacity(CHUNK * 2);
    let mut finals = String::new();
    let mut interim = String::new();
    let mut usage = None;
    let mut ended: Option<Instant> = None;
    loop {
        tokio::select! {
            inp = input.recv(), if ended.is_none() => match inp {
                Some(Input::Audio(samples)) => {
                    pending.extend_from_slice(&samples);
                    while pending.len() >= CHUNK {
                        let chunk: Vec<f32> = pending.drain(..CHUNK).collect();
                        sink.send(Message::Text(audio_message(&chunk).into()))
                            .await
                            .map_err(send_error)?;
                    }
                }
                Some(Input::End) => {
                    if !pending.is_empty() {
                        sink.send(Message::Text(audio_message(&pending).into()))
                            .await
                            .map_err(send_error)?;
                        pending.clear();
                    }
                    sink.send(Message::Text(r#"{"realtimeInput":{"activityEnd":{}}}"#.into()))
                        .await
                        .map_err(send_error)?;
                    ended = Some(Instant::now() + s.timeout);
                }
                // Dropped without `finish`: the task is being aborted anyway.
                None => return Err(ProviderError::new(ErrorCode::Internal, "live session dropped")),
            },
            msg = stream.next() => match msg {
                Some(Ok(Message::Close(frame))) => {
                    if ended.is_some() && !finals.trim().is_empty() {
                        break;
                    }
                    return Err(closed(label, frame.as_ref()));
                }
                Some(Ok(m)) => {
                    let Some(said) = message_text(&m).as_deref().and_then(read) else {
                        continue;
                    };
                    if let Some(e) = said.error {
                        let mut err = ProviderError::new(
                            ErrorCode::ProviderError,
                            format!("{label}: the service ended the session with an error"),
                        );
                        err.answer = Some(e);
                        return Err(err);
                    }
                    let before = (finals.len(), interim.clone());
                    if let Some(t) = said.final_text {
                        finals.push_str(&t);
                        interim.clear();
                    }
                    if let Some(t) = said.interim {
                        interim = t;
                    }
                    if (finals.len(), &interim) != (before.0, &before.1) {
                        partial(format!("{finals}{interim}").trim().to_string());
                    }
                    if said.usage.is_some() {
                        usage = said.usage;
                    }
                    if said.turn_complete && ended.is_some() {
                        break;
                    }
                }
                Some(Err(e)) => return Err(send_error(e)),
                None => {
                    if ended.is_some() && !finals.trim().is_empty() {
                        break;
                    }
                    return Err(closed(label, None));
                }
            },
            _ = async { sleep_until(ended.unwrap_or_else(Instant::now)).await }, if ended.is_some() => {
                if finals.trim().is_empty() {
                    return Err(late());
                }
                break;
            }
        }
    }
    let _ = sink.send(Message::Close(None)).await;
    let mut text = finals.trim().to_string();
    // A turn that ends without a final text keeps the last partial one.
    if text.is_empty() {
        text = interim.trim().to_string();
    }
    if text.is_empty() {
        return Err(empty_reply(label, ""));
    }
    Ok(Transcript {
        text,
        usage,
        ..Default::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_setup_names_the_model_mode_and_vocabulary_and_turns_detection_off() {
        let p = LiveAsrProvider::new(
            &Url::parse("wss://g.example/ws").unwrap(),
            "gemini-3.5-transcribe-live",
            None,
            BTreeMap::new(),
            "Gemini",
            Duration::from_secs(5),
            true,
            Some("smart".into()),
        );
        let hint = AsrHint {
            terms: vec!["Arcvox".into()],
            ..Default::default()
        };
        let v: serde_json::Value =
            serde_json::from_str(&setup_message(&p.settings, &hint)).unwrap();
        let setup = &v["setup"];
        assert_eq!(setup["model"], "models/gemini-3.5-transcribe-live");
        assert_eq!(setup["inputAudioTranscription"]["mode"], "SMART");
        assert_eq!(
            setup["inputAudioTranscription"]["customVocabulary"],
            serde_json::json!(["Arcvox"])
        );
        assert_eq!(
            setup["realtimeInputConfig"]["automaticActivityDetection"]["disabled"],
            true
        );
    }

    #[test]
    fn audio_goes_as_16_bit_pcm() {
        let v: serde_json::Value = serde_json::from_str(&audio_message(&[0.0, 1.0, -2.0])).unwrap();
        let audio = &v["realtimeInput"]["audio"];
        assert_eq!(audio["mimeType"], "audio/pcm;rate=16000");
        let pcm = base64::engine::general_purpose::STANDARD
            .decode(audio["data"].as_str().unwrap())
            .unwrap();
        assert_eq!(pcm, [0, 0, 0xFF, 0x7F, 0x01, 0x80]);
    }

    #[test]
    fn reads_partial_and_final_text() {
        let said =
            read(r#"{"serverContent":{"interimInputTranscription":{"text":"你好"}}}"#).unwrap();
        assert_eq!(said.interim.as_deref(), Some("你好"));
        let said = read(
            r#"{"serverContent":{"inputTranscription":{"text":"你好。"},"turnComplete":true}}"#,
        )
        .unwrap();
        assert_eq!(said.final_text.as_deref(), Some("你好。"));
        assert!(said.turn_complete);
        assert!(read(r#"{"setupComplete":{}}"#).unwrap().setup_complete);
        assert_eq!(
            read(r#"{"error":{"message":"bad"}}"#)
                .unwrap()
                .error
                .as_deref(),
            Some("bad")
        );
    }

    /// A service that answers like the Live API: ready, a partial text
    /// after the first audio, the final text once the turn ends.
    async fn fake_service(close_early: bool) -> (Url, tokio::task::JoinHandle<Vec<String>>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = Url::parse(&format!("ws://{}/ws", listener.local_addr().unwrap())).unwrap();
        let server = tokio::spawn(async move {
            let (tcp, _) = listener.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_async(tcp).await.unwrap();
            let mut got = Vec::new();
            let mut partial_sent = false;
            while let Some(Ok(m)) = ws.next().await {
                let Message::Text(t) = m else { continue };
                let v: serde_json::Value = serde_json::from_str(t.as_str()).unwrap();
                let kind = if v.get("setup").is_some() {
                    "setup"
                } else if v["realtimeInput"].get("audio").is_some() {
                    "audio"
                } else if v["realtimeInput"].get("activityStart").is_some() {
                    "start"
                } else if v["realtimeInput"].get("activityEnd").is_some() {
                    "end"
                } else {
                    "other"
                };
                got.push(kind.to_string());
                match kind {
                    "setup" if close_early => {
                        let _ = ws
                            .send(Message::Close(Some(CloseFrame {
                                code: 1008.into(),
                                reason: "API key not valid".into(),
                            })))
                            .await;
                        break;
                    }
                    "setup" => ws
                        .send(Message::Text(r#"{"setupComplete":{}}"#.into()))
                        .await
                        .unwrap(),
                    "audio" if !partial_sent => {
                        partial_sent = true;
                        ws.send(Message::Text(
                            r#"{"serverContent":{"interimInputTranscription":{"text":"你好"}}}"#
                                .into(),
                        ))
                        .await
                        .unwrap();
                    }
                    "end" => {
                        ws.send(Message::Binary(
                            r#"{"serverContent":{"inputTranscription":{"text":"你好。"}}}"#
                                .as_bytes()
                                .to_vec()
                                .into(),
                        ))
                        .await
                        .unwrap();
                        ws.send(Message::Text(
                            r#"{"serverContent":{"turnComplete":true}}"#.into(),
                        ))
                        .await
                        .unwrap();
                    }
                    _ => {}
                }
            }
            got
        });
        (url, server)
    }

    fn provider(url: &Url) -> LiveAsrProvider {
        LiveAsrProvider::new(
            url,
            "models/gemini-3.5-transcribe-live",
            Some("k1".into()),
            BTreeMap::new(),
            "Gemini",
            Duration::from_secs(5),
            false,
            None,
        )
    }

    #[tokio::test]
    async fn a_session_gives_the_partial_text_while_recording_and_the_final_text_at_the_end() {
        let (url, server) = fake_service(false).await;
        let seen = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let partial: Partial = {
            let seen = seen.clone();
            Arc::new(move |t| seen.lock().unwrap().push(t))
        };
        let stream = provider(&url)
            .open_live(&AsrHint::default(), partial)
            .unwrap();
        // Audio pushed before the service is ready waits for it.
        stream.push(&vec![0.1; CHUNK * 3 + 5]);
        let t = stream.finish().await.unwrap();
        assert_eq!(t.text, "你好。");
        assert_eq!(
            *seen.lock().unwrap(),
            vec!["你好".to_string(), "你好。".to_string()]
        );
        let got = server.await.unwrap();
        assert_eq!(got[..2], ["setup".to_string(), "start".to_string()]);
        assert_eq!(got.iter().filter(|k| *k == "audio").count(), 4);
        assert_eq!(got.last().map(String::as_str), Some("end"));
    }

    #[tokio::test]
    async fn a_whole_recording_goes_through_one_session() {
        let (url, server) = fake_service(false).await;
        let clip = AudioClip {
            samples: vec![0.1; 16_000],
            sample_rate: 16_000,
        };
        let t = provider(&url)
            .transcribe(&clip, &AsrHint::default())
            .await
            .unwrap();
        assert_eq!(t.text, "你好。");
        assert_eq!(
            server
                .await
                .unwrap()
                .iter()
                .filter(|k| *k == "audio")
                .count(),
            10
        );
    }

    #[tokio::test]
    async fn a_session_the_service_closes_keeps_its_reason() {
        let (url, _server) = fake_service(true).await;
        let err = provider(&url)
            .transcribe(
                &AudioClip {
                    samples: vec![0.1; 1600],
                    sample_rate: 16_000,
                },
                &AsrHint::default(),
            )
            .await
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::ProviderUnauthorized);
        assert_eq!(err.answer.as_deref(), Some("API key not valid"));
    }
}
