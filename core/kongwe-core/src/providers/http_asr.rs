//! OpenAI-compatible `POST {base}/audio/transcriptions` (multipart).
//! Covers Speaches, whisper.cpp server (when OpenAI-compatible), OpenAI, Groq.

use std::collections::BTreeMap;
use std::time::Duration;

use async_trait::async_trait;
use reqwest::multipart::{Form, Part};
use serde::Deserialize;
use url::Url;

use crate::audio::encode_wav_pcm16;
use crate::providers::http_common::{endpoint, send_with_retry};
use crate::providers::{AsrHint, AsrProvider, ProviderError};
use crate::types::{AudioClip, ErrorCode, Transcript};

pub struct HttpAsrProvider {
    client: reqwest::Client,
    url: Url,
    model: String,
    api_key: Option<String>,
    extra_headers: BTreeMap<String, String>,
    label: String,
    timeout: Duration,
    retries: u32,
    send_language: bool,
    send_prompt: bool,
}

impl HttpAsrProvider {
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
        send_prompt: bool,
    ) -> Self {
        Self {
            client,
            url: endpoint(base_url, "audio/transcriptions"),
            model: model.into(),
            api_key,
            extra_headers,
            label: label.into(),
            timeout,
            retries,
            send_language,
            send_prompt,
        }
    }

    fn form(&self, wav: &[u8], hint: &AsrHint) -> Form {
        let part = Part::bytes(wav.to_vec())
            .file_name("audio.wav")
            .mime_str("audio/wav")
            .expect("static mime");
        let mut form = Form::new()
            .part("file", part)
            .text("model", self.model.clone())
            .text("response_format", "json");
        if self.send_language {
            if let Some(lang) = &hint.language {
                form = form.text("language", whisper_language(lang));
            }
        }
        if self.send_prompt {
            if let Some(p) = &hint.prompt {
                form = form.text("prompt", p.clone());
            }
        }
        form
    }
}

/// Whisper-style servers take ISO 639-1 codes; `zh-TW` becomes `zh`.
fn whisper_language(tag: &str) -> String {
    tag.split(['-', '_'])
        .next()
        .unwrap_or(tag)
        .to_ascii_lowercase()
}

/// The configured timeout is the allowance for a short clip; a longer clip
/// gets twice its own length on top, since a server may transcribe slower
/// than real time.
fn request_timeout(base: Duration, clip_secs: f32) -> Duration {
    base + Duration::from_secs_f32(clip_secs.max(0.0) * 2.0)
}

#[derive(Deserialize)]
struct TranscriptionResponse {
    text: String,
    #[serde(default)]
    language: Option<String>,
}

#[async_trait]
impl AsrProvider for HttpAsrProvider {
    async fn transcribe(
        &self,
        audio: &AudioClip,
        hint: &AsrHint,
    ) -> Result<Transcript, ProviderError> {
        let wav = encode_wav_pcm16(audio);
        let timeout = request_timeout(self.timeout, audio.duration_secs());
        let resp = send_with_retry(&self.label, self.retries, || {
            let mut req = self
                .client
                .post(self.url.clone())
                .timeout(timeout)
                .multipart(self.form(&wav, hint));
            if let Some(key) = &self.api_key {
                req = req.bearer_auth(key);
            }
            for (k, v) in &self.extra_headers {
                req = req.header(k, v);
            }
            req
        })
        .await?;
        let parsed: TranscriptionResponse = resp.json().await.map_err(|e| {
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
        Ok(Transcript {
            text: parsed.text.trim().to_string(),
            language: parsed.language,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timeout_grows_with_the_clip_length() {
        let base = Duration::from_secs(30);
        assert_eq!(request_timeout(base, 0.0), base);
        assert_eq!(request_timeout(base, 40.0), Duration::from_secs(110));
    }
    use serde_json::json;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn clip() -> AudioClip {
        AudioClip {
            samples: vec![0.1; 16_000],
            sample_rate: 16_000,
        }
    }

    fn provider(server: &MockServer, lang: bool, prompt: bool) -> HttpAsrProvider {
        let base = Url::parse(&format!("{}/v1", server.uri())).unwrap();
        HttpAsrProvider::new(
            reqwest::Client::new(),
            &base,
            "breeze",
            None,
            BTreeMap::new(),
            "Speaches",
            Duration::from_secs(5),
            1,
            lang,
            prompt,
        )
    }

    #[tokio::test]
    async fn sends_multipart_with_file_model_language_prompt() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/audio/transcriptions"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({"text":"  你好 ","language":"zh"})),
            )
            .expect(1)
            .mount(&server)
            .await;
        let hint = AsrHint {
            language: Some("zh-TW".into()),
            prompt: Some("Kongwe, 公威".into()),
        };
        let out = provider(&server, true, true)
            .transcribe(&clip(), &hint)
            .await
            .unwrap();
        assert_eq!(
            out,
            Transcript {
                text: "你好".into(),
                language: Some("zh".into())
            }
        );

        let req = &server.received_requests().await.unwrap()[0];
        let ct = req.headers.get("content-type").unwrap().to_str().unwrap();
        assert!(ct.starts_with("multipart/form-data"), "{ct}");
        let body = String::from_utf8_lossy(&req.body);
        assert!(body.contains("name=\"file\""));
        assert!(body.contains("filename=\"audio.wav\""));
        assert!(body.contains("RIFF"));
        assert!(body.contains("name=\"model\"\r\n\r\nbreeze"));
        assert!(
            body.contains("name=\"language\"\r\n\r\nzh\r\n"),
            "zh-TW must be sent as zh"
        );
        assert!(body.contains("name=\"prompt\"\r\n\r\nKongwe, 公威"));
        assert!(body.contains("name=\"response_format\"\r\n\r\njson"));
    }

    #[tokio::test]
    async fn omits_language_and_prompt_when_capability_is_off() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/audio/transcriptions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"text":"x"})))
            .mount(&server)
            .await;
        let hint = AsrHint {
            language: Some("zh-TW".into()),
            prompt: Some("P".into()),
        };
        provider(&server, false, false)
            .transcribe(&clip(), &hint)
            .await
            .unwrap();
        let body =
            String::from_utf8_lossy(&server.received_requests().await.unwrap()[0].body).to_string();
        assert!(!body.contains("name=\"language\""));
        assert!(!body.contains("name=\"prompt\""));
    }

    #[tokio::test]
    async fn retries_5xx_by_rebuilding_multipart() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/audio/transcriptions"))
            .respond_with(ResponseTemplate::new(502))
            .up_to_n_times(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/audio/transcriptions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"text":"ok"})))
            .mount(&server)
            .await;
        let out = provider(&server, true, true)
            .transcribe(&clip(), &AsrHint::default())
            .await
            .unwrap();
        assert_eq!(out.text, "ok");
        assert_eq!(server.received_requests().await.unwrap().len(), 2);
    }

    #[test]
    fn language_tag_mapping() {
        assert_eq!(whisper_language("zh-TW"), "zh");
        assert_eq!(whisper_language("ja"), "ja");
        assert_eq!(whisper_language("EN_us"), "en");
    }
}
