//! OpenAI-compatible `POST {base}/chat/completions`. Covers Ollama, LM
//! Studio, vLLM, OpenAI, Gemini (openai/ endpoint), Groq, and the Kongwe server.

use std::collections::BTreeMap;

use async_trait::async_trait;
use serde::Deserialize;
use url::Url;

use crate::providers::http_common::{endpoint, send_with_retry};
use crate::providers::{LlmOptions, LlmProvider, ProviderError};
use crate::types::{ChatMessage, ErrorCode};

pub struct HttpLlmProvider {
    client: reqwest::Client,
    url: Url,
    model: String,
    api_key: Option<String>,
    extra_headers: BTreeMap<String, String>,
    label: String,
}

impl HttpLlmProvider {
    pub fn new(
        client: reqwest::Client,
        base_url: &Url,
        model: impl Into<String>,
        api_key: Option<String>,
        extra_headers: BTreeMap<String, String>,
        label: impl Into<String>,
    ) -> Self {
        Self {
            client,
            url: endpoint(base_url, "chat/completions"),
            model: model.into(),
            api_key,
            extra_headers,
            label: label.into(),
        }
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
impl LlmProvider for HttpLlmProvider {
    async fn complete(
        &self,
        messages: Vec<ChatMessage>,
        opts: &LlmOptions,
    ) -> Result<String, ProviderError> {
        let body = serde_json::json!({
            "model": self.model,
            "messages": messages,
            "temperature": opts.temperature,
            "max_tokens": opts.max_tokens,
            "stream": false,
        });
        let resp = send_with_retry(&self.label, || {
            let mut req = self
                .client
                .post(self.url.clone())
                .timeout(opts.timeout)
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
            ProviderError::new(
                ErrorCode::ProviderError,
                format!("{}: bad chat response: {e}", self.label),
            )
        })?;
        let content = parsed
            .choices
            .into_iter()
            .next()
            .and_then(|c| c.message.content)
            .ok_or_else(|| {
                ProviderError::new(
                    ErrorCode::ProviderError,
                    format!("{}: no choices in response", self.label),
                )
            })?;
        Ok(content)
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use serde_json::json;
    use wiremock::matchers::{body_partial_json, header, header_exists, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn opts() -> LlmOptions {
        LlmOptions {
            temperature: 0.2,
            max_tokens: Some(64),
            timeout: Duration::from_secs(5),
        }
    }

    fn ok_body(text: &str) -> serde_json::Value {
        json!({"choices":[{"message":{"role":"assistant","content":text}}]})
    }

    #[tokio::test]
    async fn posts_messages_with_bearer_and_parses_content() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .and(header("authorization", "Bearer k1"))
            .and(header("x-extra", "1"))
            .and(body_partial_json(json!({"model":"gemma3","stream":false,"messages":[{"role":"system","content":"S"},{"role":"user","content":"U"}]})))
            .respond_with(ResponseTemplate::new(200).set_body_json(ok_body("整理後")))
            .expect(1)
            .mount(&server)
            .await;
        let base = Url::parse(&format!("{}/v1", server.uri())).unwrap();
        let headers = BTreeMap::from([("x-extra".to_string(), "1".to_string())]);
        let p = HttpLlmProvider::new(
            reqwest::Client::new(),
            &base,
            "gemma3",
            Some("k1".into()),
            headers,
            "Ollama",
        );
        let out = p
            .complete(
                vec![ChatMessage::system("S"), ChatMessage::user("U")],
                &opts(),
            )
            .await
            .unwrap();
        assert_eq!(out, "整理後");
    }

    #[tokio::test]
    async fn omits_authorization_without_key() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .and(header_exists("authorization"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(ok_body("x")))
            .mount(&server)
            .await;
        let base = Url::parse(&format!("{}/v1", server.uri())).unwrap();
        let p = HttpLlmProvider::new(
            reqwest::Client::new(),
            &base,
            "m",
            None,
            BTreeMap::new(),
            "Ollama",
        );
        assert_eq!(
            p.complete(vec![ChatMessage::user("U")], &opts())
                .await
                .unwrap(),
            "x"
        );
    }

    #[tokio::test]
    async fn malformed_response_is_provider_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"choices":[]})))
            .mount(&server)
            .await;
        let base = Url::parse(&format!("{}/v1", server.uri())).unwrap();
        let p = HttpLlmProvider::new(
            reqwest::Client::new(),
            &base,
            "m",
            None,
            BTreeMap::new(),
            "Ollama",
        );
        let err = p
            .complete(vec![ChatMessage::user("U")], &opts())
            .await
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::ProviderError);
    }
}
