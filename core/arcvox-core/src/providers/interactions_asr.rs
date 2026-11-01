//! Speech to text through Gemini's `POST {base}/interactions`, the route of
//! its transcription models. The recording goes inline, and `store` is off
//! because the service otherwise keeps every interaction on its side.

use std::collections::BTreeMap;
use std::time::Duration;

use async_trait::async_trait;
use base64::Engine as _;
use serde::Deserialize;
use url::Url;

use crate::audio::encode_wav_pcm16;
use crate::providers::chat_asr::MAX_REQUEST_BYTES;
use crate::providers::http_asr::request_timeout;
use crate::providers::http_common::{empty_reply, endpoint, kept_answer, send_with_retry};
use crate::providers::{AsrHint, AsrProvider, ProviderError};
use crate::types::{AudioClip, ErrorCode, Transcript, TranscriptWord};

pub struct InteractionsAsrProvider {
    client: reqwest::Client,
    url: Url,
    model: String,
    api_key: Option<String>,
    extra_headers: BTreeMap<String, String>,
    label: String,
    timeout: Duration,
    retries: u32,
    send_language: bool,
    send_terms: bool,
    mode: Option<String>,
    word_times: bool,
}

impl InteractionsAsrProvider {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        client: reqwest::Client,
        base_url: &Url,
        model: impl Into<String>,
        api_key: Option<String>,
        extra_headers: BTreeMap<String, String>,
        label: impl Into<String>,
        timeout: Duration,
        retries: u32,
        send_language: bool,
        send_terms: bool,
        mode: Option<String>,
        word_times: bool,
    ) -> Self {
        let model: String = model.into();
        Self {
            client,
            url: endpoint(base_url, "interactions"),
            // The model list names models `models/<id>`; this endpoint takes the id.
            model: model
                .strip_prefix("models/")
                .map(str::to_string)
                .unwrap_or(model),
            api_key,
            extra_headers,
            label: label.into(),
            timeout,
            retries,
            send_language,
            send_terms,
            mode,
            word_times,
        }
    }

    fn body(&self, wav: &[u8], hint: &AsrHint) -> serde_json::Value {
        let data = base64::engine::general_purpose::STANDARD.encode(wav);
        let mut config = serde_json::Map::new();
        if self.send_language {
            if let Some(lang) = &hint.language {
                config.insert("language_codes".into(), serde_json::json!([lang]));
            }
        }
        let words = self.word_times && self.mode.is_none();
        // The service refuses a vocabulary together with word offsets.
        if self.send_terms && !hint.terms.is_empty() && !words {
            config.insert("custom_vocabulary".into(), serde_json::json!(hint.terms));
        }
        if let Some(mode) = &self.mode {
            config.insert("mode".into(), serde_json::json!(mode));
        } else if words {
            config.insert(
                "mode".into(),
                serde_json::json!({ "type": "verbatim", "timestamp_granularities": ["word"] }),
            );
        }
        let mut body = serde_json::json!({
            "model": self.model,
            "input": [{ "type": "audio", "data": data, "mime_type": "audio/wav" }],
            "store": false,
        });
        if !config.is_empty() {
            body["generation_config"] = serde_json::json!({ "transcription_config": config });
        }
        body
    }
}

#[derive(Deserialize)]
struct Interaction {
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    steps: Vec<Step>,
    #[serde(default)]
    usage: Option<serde_json::Value>,
    #[serde(default)]
    errors: Vec<ServiceError>,
}
#[derive(Deserialize)]
struct ServiceError {
    #[serde(default)]
    message: Option<String>,
}
#[derive(Deserialize)]
struct Step {
    #[serde(rename = "type", default)]
    kind: String,
    #[serde(default)]
    content: Vec<Part>,
}
#[derive(Deserialize)]
struct Part {
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    annotations: Vec<Annotation>,
}
#[derive(Deserialize)]
struct Annotation {
    #[serde(rename = "type", default)]
    kind: String,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    start_offset: Option<String>,
    #[serde(default)]
    end_offset: Option<String>,
}

/// An offset as the service writes it: `0.300s`, `1s`.
fn offset_ms(offset: Option<&str>) -> Option<u32> {
    let seconds: f64 = offset?.strip_suffix('s')?.parse().ok()?;
    Some((seconds * 1000.0).round() as u32)
}

impl Interaction {
    fn text(&self) -> String {
        self.steps
            .iter()
            .filter(|s| s.kind == "model_output")
            .flat_map(|s| &s.content)
            .filter_map(|p| p.text.as_deref())
            .collect::<String>()
            .trim()
            .to_string()
    }

    fn words(&self) -> Vec<TranscriptWord> {
        self.steps
            .iter()
            .filter(|s| s.kind == "model_output")
            .flat_map(|s| &s.content)
            .flat_map(|p| &p.annotations)
            .filter(|a| a.kind == "word_info")
            .filter_map(|a| {
                Some(TranscriptWord {
                    text: a.text.clone()?,
                    start_ms: offset_ms(a.start_offset.as_deref())?,
                    end_ms: offset_ms(a.end_offset.as_deref())?,
                })
            })
            .collect()
    }
}

#[async_trait]
impl AsrProvider for InteractionsAsrProvider {
    async fn transcribe(
        &self,
        audio: &AudioClip,
        hint: &AsrHint,
    ) -> Result<Transcript, ProviderError> {
        let body = self.body(&encode_wav_pcm16(audio), hint);
        let size = body.to_string().len();
        if size > MAX_REQUEST_BYTES {
            return Err(ProviderError::new(
                ErrorCode::ProviderError,
                format!(
                    "{}: the recording is too long for this service ({} MB, the limit is {} MB)",
                    self.label,
                    size.div_ceil(1024 * 1024),
                    MAX_REQUEST_BYTES / (1024 * 1024)
                ),
            ));
        }
        let timeout = request_timeout(self.timeout, audio.duration_secs());
        let resp = send_with_retry(&self.label, self.retries, || {
            let mut req = self
                .client
                .post(self.url.clone())
                .timeout(timeout)
                .json(&body);
            if let Some(key) = &self.api_key {
                req = req.header("x-goog-api-key", key);
            }
            for (k, v) in &self.extra_headers {
                req = req.header(k, v);
            }
            req
        })
        .await?;
        let raw = resp.text().await.map_err(|e| {
            if e.is_timeout() {
                return ProviderError::new(
                    ErrorCode::ProviderTimeout,
                    format!("{}: request timed out", self.label),
                );
            }
            ProviderError::new(
                ErrorCode::ProviderError,
                format!("{}: bad transcription response: {e}", self.label),
            )
        })?;
        let parsed: Interaction = serde_json::from_str(&raw).map_err(|e| {
            ProviderError::new(
                ErrorCode::ProviderError,
                format!("{}: bad transcription response: {e}", self.label),
            )
        })?;
        if let Some(status) = parsed.status.as_deref().filter(|s| *s != "completed") {
            let mut err = ProviderError::new(
                ErrorCode::ProviderError,
                format!("{}: the interaction ended as {status}", self.label),
            )
            .with_detail(kept_answer(&raw));
            err.answer = parsed.errors.iter().find_map(|e| e.message.clone());
            return Err(err);
        }
        let text = parsed.text();
        // The recording held speech by the time it got here.
        if text.is_empty() {
            return Err(empty_reply(&self.label, &raw));
        }
        Ok(Transcript {
            text,
            words: parsed.words(),
            usage: parsed.usage,
            ..Default::default()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn clip(secs: usize) -> AudioClip {
        AudioClip {
            samples: vec![0.1; 16_000 * secs],
            sample_rate: 16_000,
        }
    }

    fn provider(server: &MockServer, lang: bool, terms: bool) -> InteractionsAsrProvider {
        let base = Url::parse(&format!("{}/v1beta/", server.uri())).unwrap();
        InteractionsAsrProvider::new(
            reqwest::Client::new(),
            &base,
            "models/gemini-3.5-transcribe",
            Some("k1".into()),
            BTreeMap::new(),
            "Gemini",
            Duration::from_secs(5),
            1,
            lang,
            terms,
            None,
            false,
        )
    }

    fn answer(text: &str) -> serde_json::Value {
        json!({
            "id": "i1",
            "status": "completed",
            "steps": [
                { "type": "user_input", "content": [{ "type": "audio" }] },
                { "type": "model_output", "content": [{ "type": "text", "text": text, "annotations": [] }] },
            ],
        })
    }

    #[tokio::test]
    async fn sends_the_audio_inline_and_reads_the_model_output() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1beta/interactions"))
            .and(header("x-goog-api-key", "k1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(answer(" 你好。\n")))
            .expect(1)
            .mount(&server)
            .await;
        let hint = AsrHint {
            language: Some("zh-TW".into()),
            prompt: Some("sample Arcvox, 語音".into()),
            terms: vec!["Arcvox".into(), "語音".into()],
        };
        let out = provider(&server, true, true)
            .transcribe(&clip(1), &hint)
            .await
            .unwrap();
        assert_eq!(out.text, "你好。");

        let req = &server.received_requests().await.unwrap()[0];
        let body: serde_json::Value = serde_json::from_slice(&req.body).unwrap();
        assert_eq!(body["model"], "gemini-3.5-transcribe");
        assert_eq!(body["store"], false);
        assert_eq!(body["input"][0]["type"], "audio");
        assert_eq!(body["input"][0]["mime_type"], "audio/wav");
        let config = &body["generation_config"]["transcription_config"];
        assert_eq!(config["language_codes"], json!(["zh-TW"]));
        assert_eq!(config["custom_vocabulary"], json!(["Arcvox", "語音"]));
        let wav = base64::engine::general_purpose::STANDARD
            .decode(body["input"][0]["data"].as_str().unwrap())
            .unwrap();
        assert_eq!(&wav[..4], b"RIFF");
    }

    #[tokio::test]
    async fn leaves_the_config_out_when_the_capabilities_are_off() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(answer("x")))
            .mount(&server)
            .await;
        let hint = AsrHint {
            language: Some("zh-TW".into()),
            prompt: None,
            terms: vec!["Arcvox".into()],
        };
        provider(&server, false, false)
            .transcribe(&clip(1), &hint)
            .await
            .unwrap();
        let body: serde_json::Value =
            serde_json::from_slice(&server.received_requests().await.unwrap()[0].body).unwrap();
        assert!(body.get("generation_config").is_none(), "{body}");
    }

    #[tokio::test]
    async fn sends_the_engines_mode() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(answer("x")))
            .mount(&server)
            .await;
        let mut p = provider(&server, false, false);
        p.mode = Some("smart".into());
        p.transcribe(&clip(1), &AsrHint::default()).await.unwrap();
        let body: serde_json::Value =
            serde_json::from_slice(&server.received_requests().await.unwrap()[0].body).unwrap();
        assert_eq!(
            body["generation_config"]["transcription_config"]["mode"],
            "smart"
        );
    }

    #[tokio::test]
    async fn no_model_output_is_an_empty_reply() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({"status":"completed","steps":[]})),
            )
            .mount(&server)
            .await;
        let err = provider(&server, true, true)
            .transcribe(&clip(1), &AsrHint::default())
            .await
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::EmptyReply);
        assert!(err.detail.is_some());
    }

    #[tokio::test]
    async fn an_interaction_that_did_not_complete_is_an_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({"status":"failed","steps":[]})),
            )
            .mount(&server)
            .await;
        let err = provider(&server, true, true)
            .transcribe(&clip(1), &AsrHint::default())
            .await
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::ProviderError);
        assert!(err.message.contains("failed"), "{}", err.message);
    }

    #[tokio::test]
    async fn word_times_ask_for_offsets_and_leave_the_vocabulary_out() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(answer("x")))
            .mount(&server)
            .await;
        let mut p = provider(&server, false, true);
        p.word_times = true;
        let hint = AsrHint {
            terms: vec!["Arcvox".into()],
            ..Default::default()
        };
        p.transcribe(&clip(1), &hint).await.unwrap();
        let body: serde_json::Value =
            serde_json::from_slice(&server.received_requests().await.unwrap()[0].body).unwrap();
        let config = &body["generation_config"]["transcription_config"];
        assert_eq!(config["mode"]["type"], "verbatim");
        assert_eq!(config["mode"]["timestamp_granularities"], json!(["word"]));
        assert!(config.get("custom_vocabulary").is_none(), "{config}");
    }

    #[tokio::test]
    async fn the_smart_mode_takes_no_word_times_and_keeps_the_vocabulary() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(answer("x")))
            .mount(&server)
            .await;
        let mut p = provider(&server, false, true);
        p.word_times = true;
        p.mode = Some("smart".into());
        let hint = AsrHint {
            terms: vec!["Arcvox".into()],
            ..Default::default()
        };
        p.transcribe(&clip(1), &hint).await.unwrap();
        let body: serde_json::Value =
            serde_json::from_slice(&server.received_requests().await.unwrap()[0].body).unwrap();
        let config = &body["generation_config"]["transcription_config"];
        assert_eq!(config["mode"], "smart");
        assert_eq!(config["custom_vocabulary"], json!(["Arcvox"]));
    }

    #[tokio::test]
    async fn the_word_offsets_come_back_in_milliseconds() {
        let server = MockServer::start().await;
        let mut body = answer("你好");
        body["steps"][1]["content"][0]["annotations"] = json!([
            { "type": "word_info", "text": "你", "start_offset": "0.300s", "end_offset": "1s", "speaker": "spk:0", "start_index": 0, "end_index": 3 },
            { "type": "word_info", "text": "好", "start_offset": "1s", "end_offset": "1.250s", "speaker": "spk:0", "start_index": 3, "end_index": 6 },
            { "type": "url_citation", "url": "x" },
        ]);
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(&server)
            .await;
        let out = provider(&server, false, false)
            .transcribe(&clip(1), &AsrHint::default())
            .await
            .unwrap();
        assert_eq!(
            out.words,
            vec![
                TranscriptWord {
                    text: "你".into(),
                    start_ms: 300,
                    end_ms: 1000
                },
                TranscriptWord {
                    text: "好".into(),
                    start_ms: 1000,
                    end_ms: 1250
                },
            ]
        );
    }

    #[tokio::test]
    async fn the_services_usage_comes_back_whole() {
        let server = MockServer::start().await;
        let usage = json!({
            "total_input_tokens": 381,
            "input_tokens_by_modality": [{ "modality": "audio", "tokens": 380 }],
            "total_output_tokens": 23,
        });
        let mut body = answer("x");
        body["usage"] = usage.clone();
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(&server)
            .await;
        let out = provider(&server, false, false)
            .transcribe(&clip(1), &AsrHint::default())
            .await
            .unwrap();
        assert_eq!(out.usage, Some(usage));
    }

    #[tokio::test]
    async fn an_interaction_that_failed_keeps_what_the_service_said() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "status": "failed",
                "steps": [],
                "errors": [{ "code": "quota", "message": "the quota is used up" }],
            })))
            .mount(&server)
            .await;
        let err = provider(&server, true, true)
            .transcribe(&clip(1), &AsrHint::default())
            .await
            .unwrap_err();
        assert_eq!(err.answer.as_deref(), Some("the quota is used up"));
        assert!(err.detail.unwrap().contains("\"quota\""));
    }

    #[tokio::test]
    async fn a_recording_over_the_request_limit_is_refused_before_sending() {
        let server = MockServer::start().await;
        let err = provider(&server, true, true)
            .transcribe(&clip(600), &AsrHint::default())
            .await
            .unwrap_err();
        assert!(err.message.contains("too long"), "{}", err.message);
        assert!(server.received_requests().await.unwrap().is_empty());
    }
}
