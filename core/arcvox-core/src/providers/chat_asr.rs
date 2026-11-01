//! Speech to text through an OpenAI-compatible `POST {base}/chat/completions`
//! whose user message carries the recording as an `input_audio` part. For
//! services that understand audio in chat but have no
//! `audio/transcriptions` endpoint (Gemini's openai/ endpoint).

use std::collections::BTreeMap;
use std::time::Duration;

use async_trait::async_trait;
use base64::Engine as _;
use serde::Deserialize;
use url::Url;

use crate::audio::encode_wav_pcm16;
use crate::providers::http_asr::request_timeout;
use crate::providers::http_common::{endpoint, send_with_retry};
use crate::providers::{AsrHint, AsrProvider, ProviderError};
use crate::types::{AudioClip, ErrorCode, Transcript};

/// Gemini takes at most 20 MB per request with the audio inline.
pub(crate) const MAX_REQUEST_BYTES: usize = 20 * 1024 * 1024;

const INSTRUCTION: &str = "Transcribe the speech in this audio exactly as spoken, \
with punctuation. Reply with the transcript only, without any comment. \
If there is no speech, reply with nothing.";

pub struct ChatAsrProvider {
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
    reasoning_effort: Option<String>,
}

impl ChatAsrProvider {
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
        reasoning_effort: Option<String>,
    ) -> Self {
        Self {
            client,
            url: endpoint(base_url, "chat/completions"),
            model: model.into(),
            api_key,
            extra_headers,
            label: label.into(),
            timeout,
            retries,
            send_language,
            send_prompt,
            reasoning_effort,
        }
    }

    fn instruction(&self, hint: &AsrHint) -> String {
        let mut text = INSTRUCTION.to_string();
        if self.send_language {
            if let Some(lang) = &hint.language {
                text.push_str(&format!(" The speech is mostly in the language `{lang}`."));
            }
        }
        if self.send_prompt {
            if let Some(p) = &hint.prompt {
                text.push_str(&format!(
                    " Text written before this recording, for the spelling of names and terms: {p}"
                ));
            }
        }
        text
    }

    fn body(&self, wav: &[u8], hint: &AsrHint) -> serde_json::Value {
        let data = base64::engine::general_purpose::STANDARD.encode(wav);
        let mut body = serde_json::json!({
            "model": self.model,
            "messages": [{
                "role": "user",
                "content": [
                    { "type": "text", "text": self.instruction(hint) },
                    { "type": "input_audio", "input_audio": { "data": data, "format": "wav" } },
                ],
            }],
            "temperature": 0,
            "stream": false,
        });
        if let Some(effort) = &self.reasoning_effort {
            body["reasoning_effort"] = serde_json::json!(effort);
        }
        body
    }
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}
#[derive(Deserialize)]
struct Choice {
    message: Msg,
}
#[derive(Deserialize)]
struct Msg {
    content: Option<String>,
}

#[async_trait]
impl AsrProvider for ChatAsrProvider {
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
                req = req.bearer_auth(key);
            }
            for (k, v) in &self.extra_headers {
                req = req.header(k, v);
            }
            req
        })
        .await?;
        let parsed: ChatResponse = resp.json().await.map_err(|e| {
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
        let choice = parsed.choices.into_iter().next().ok_or_else(|| {
            ProviderError::new(
                ErrorCode::ProviderError,
                format!("{}: no choices in response", self.label),
            )
        })?;
        Ok(Transcript {
            // No text is no speech, which the session handles like an empty
            // transcript from any other engine.
            text: choice
                .message
                .content
                .unwrap_or_default()
                .trim()
                .to_string(),
            language: None,
            segments: Vec::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wiremock::matchers::{body_partial_json, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn clip(secs: usize) -> AudioClip {
        AudioClip {
            samples: vec![0.1; 16_000 * secs],
            sample_rate: 16_000,
        }
    }

    fn provider(server: &MockServer, lang: bool, prompt: bool) -> ChatAsrProvider {
        let base = Url::parse(&format!("{}/v1beta/openai/", server.uri())).unwrap();
        ChatAsrProvider::new(
            reqwest::Client::new(),
            &base,
            "gemini-flash",
            Some("k1".into()),
            BTreeMap::new(),
            "Gemini",
            Duration::from_secs(5),
            1,
            lang,
            prompt,
            Some("low".into()),
        )
    }

    fn ok_body(text: Option<&str>) -> serde_json::Value {
        json!({"choices":[{"message":{"role":"assistant","content":text}}]})
    }

    #[tokio::test]
    async fn sends_the_audio_inline_with_language_and_terms() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1beta/openai/chat/completions"))
            .and(header("authorization", "Bearer k1"))
            .and(body_partial_json(
                json!({"model":"gemini-flash","stream":false}),
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(ok_body(Some(" 你好。\n"))))
            .expect(1)
            .mount(&server)
            .await;
        let hint = AsrHint {
            language: Some("zh-TW".into()),
            prompt: Some("Arcvox, 語音".into()),
            ..Default::default()
        };
        let out = provider(&server, true, true)
            .transcribe(&clip(1), &hint)
            .await
            .unwrap();
        assert_eq!(out.text, "你好。");

        let req = &server.received_requests().await.unwrap()[0];
        let body: serde_json::Value = serde_json::from_slice(&req.body).unwrap();
        let parts = &body["messages"][0]["content"];
        let text = parts[0]["text"].as_str().unwrap();
        assert!(text.contains("`zh-TW`"), "{text}");
        assert!(text.contains("Arcvox, 語音"), "{text}");
        assert_eq!(parts[1]["type"], "input_audio");
        assert_eq!(parts[1]["input_audio"]["format"], "wav");
        assert_eq!(body["reasoning_effort"], "low");
        let wav = base64::engine::general_purpose::STANDARD
            .decode(parts[1]["input_audio"]["data"].as_str().unwrap())
            .unwrap();
        assert_eq!(&wav[..4], b"RIFF");
    }

    #[tokio::test]
    async fn leaves_out_language_and_terms_when_the_capability_is_off() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(ok_body(Some("x"))))
            .mount(&server)
            .await;
        let hint = AsrHint {
            language: Some("zh-TW".into()),
            prompt: Some("P-term".into()),
            ..Default::default()
        };
        provider(&server, false, false)
            .transcribe(&clip(1), &hint)
            .await
            .unwrap();
        let body: serde_json::Value =
            serde_json::from_slice(&server.received_requests().await.unwrap()[0].body).unwrap();
        let text = body["messages"][0]["content"][0]["text"].as_str().unwrap();
        assert!(
            !text.contains("zh-TW") && !text.contains("P-term"),
            "{text}"
        );
    }

    #[tokio::test]
    async fn no_content_is_an_empty_transcript() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(ok_body(None)))
            .mount(&server)
            .await;
        let out = provider(&server, true, true)
            .transcribe(&clip(1), &AsrHint::default())
            .await
            .unwrap();
        assert_eq!(out.text, "");
    }

    #[tokio::test]
    async fn a_recording_over_the_request_limit_is_refused_before_sending() {
        let server = MockServer::start().await;
        // 16 kHz PCM16 is 32 KB a second, a third more in base64: 10 minutes
        // is about 25 MB.
        let err = provider(&server, true, true)
            .transcribe(&clip(600), &AsrHint::default())
            .await
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::ProviderError);
        assert!(err.message.contains("too long"), "{}", err.message);
        assert!(server.received_requests().await.unwrap().is_empty());
    }
}
