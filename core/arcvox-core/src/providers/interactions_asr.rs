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
use crate::providers::http_common::{endpoint, send_with_retry};
use crate::providers::{AsrHint, AsrProvider, ProviderError};
use crate::types::{AudioClip, ErrorCode, Transcript};

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
        if self.send_terms && !hint.terms.is_empty() {
            config.insert("custom_vocabulary".into(), serde_json::json!(hint.terms));
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
        let parsed: Interaction = resp.json().await.map_err(|e| {
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
        if let Some(status) = parsed.status.as_deref().filter(|s| *s != "completed") {
            return Err(ProviderError::new(
                ErrorCode::ProviderError,
                format!("{}: the interaction ended as {status}", self.label),
            ));
        }
        Ok(Transcript {
            // No text is no speech, which the session handles like an empty
            // transcript from any other engine.
            text: parsed.text(),
            language: None,
            segments: Vec::new(),
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
    async fn no_model_output_is_an_empty_transcript() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({"status":"completed","steps":[]})),
            )
            .mount(&server)
            .await;
        let out = provider(&server, true, true)
            .transcribe(&clip(1), &AsrHint::default())
            .await
            .unwrap();
        assert_eq!(out.text, "");
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
