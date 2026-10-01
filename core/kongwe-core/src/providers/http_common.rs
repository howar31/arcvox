//! Shared HTTP behavior: base URL handling, retry policy, status mapping.

use reqwest::{RequestBuilder, Response, StatusCode};
use url::Url;

use crate::providers::ProviderError;
use crate::types::ErrorCode;

/// Ensures the path ends with `/` so `Url::join("chat/completions")` appends
/// instead of replacing the last segment (`/v1` would otherwise be dropped).
pub fn normalize_base_url(mut url: Url) -> Url {
    if !url.path().ends_with('/') {
        let path = format!("{}/", url.path());
        url.set_path(&path);
    }
    url
}

pub fn endpoint(base: &Url, rel: &str) -> Url {
    normalize_base_url(base.clone())
        .join(rel)
        .expect("relative endpoint path is valid")
}

/// Sends the request built by `build`. A connection error, a 5xx answer or
/// a timeout is tried again, up to `retries` more times; a 4xx answer is
/// not, since asking again gives the same result. The error of the last
/// attempt says how many retries were made.
pub async fn send_with_retry<F>(
    label: &str,
    retries: u32,
    build: F,
) -> Result<Response, ProviderError>
where
    F: Fn() -> RequestBuilder,
{
    let mut retried = 0u32;
    loop {
        let (err, again) = match build().send().await {
            Ok(resp) if resp.status().is_success() => return Ok(resp),
            Ok(resp) => {
                let status = resp.status();
                if status.is_server_error() && retried < retries {
                    retried += 1;
                    continue;
                }
                let body = resp.text().await.unwrap_or_default();
                (
                    classify_status(label, status, &body),
                    status.is_server_error(),
                )
            }
            Err(err) if err.is_timeout() => (
                ProviderError::new(
                    ErrorCode::ProviderTimeout,
                    format!("{label}: request timed out"),
                ),
                true,
            ),
            Err(err) if err.is_connect() => (
                ProviderError::new(ErrorCode::ProviderUnreachable, format!("{label}: {err}"))
                    .with_hint(format!(
                        "check that {label} is running and the base URL is correct"
                    )),
                true,
            ),
            Err(err) => (
                ProviderError::new(ErrorCode::ProviderError, format!("{label}: {err}")),
                false,
            ),
        };
        if again && retried < retries {
            retried += 1;
            continue;
        }
        return Err(after_retries(err, retried));
    }
}

fn after_retries(mut err: ProviderError, retried: u32) -> ProviderError {
    if retried > 0 {
        err.message = format!("{} (after {retried} retries)", err.message);
    }
    err
}

pub fn classify_status(label: &str, status: StatusCode, body: &str) -> ProviderError {
    let snippet: String = body.chars().take(200).collect();
    match status.as_u16() {
        401 | 403 => ProviderError::new(
            ErrorCode::ProviderUnauthorized,
            format!("{label}: HTTP {status}"),
        )
        .with_hint("check the API key"),
        // Servers answer 404 for an unknown model as well as for a wrong
        // path; the body tells them apart.
        404 if body.to_ascii_lowercase().contains("model") => ProviderError::new(
            ErrorCode::ProviderError,
            format!("{label}: HTTP 404 {snippet}"),
        )
        .with_hint("model not found on the server; check the model name of this engine"),
        404 => ProviderError::new(
            ErrorCode::ProviderError,
            format!("{label}: HTTP 404 {snippet}"),
        )
        .with_hint("endpoint not found; the base URL usually ends with /v1"),
        _ => ProviderError::new(
            ErrorCode::ProviderError,
            format!("{label}: HTTP {status} {snippet}"),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn joins_paths_without_dropping_v1() {
        for base in ["http://localhost:11434/v1", "http://localhost:11434/v1/"] {
            let url = endpoint(&Url::parse(base).unwrap(), "chat/completions");
            assert_eq!(
                url.as_str(),
                "http://localhost:11434/v1/chat/completions",
                "base {base}"
            );
        }
        let url = endpoint(
            &Url::parse("https://generativelanguage.googleapis.com/v1beta/openai/").unwrap(),
            "models",
        );
        assert_eq!(
            url.as_str(),
            "https://generativelanguage.googleapis.com/v1beta/openai/models"
        );
        let url = endpoint(
            &Url::parse("http://localhost:8000").unwrap(),
            "audio/transcriptions",
        );
        assert_eq!(url.as_str(), "http://localhost:8000/audio/transcriptions");
    }

    #[test]
    fn a_404_naming_a_model_points_at_the_model_name() {
        let e = classify_status(
            "Speaches",
            StatusCode::NOT_FOUND,
            r#"{"detail":"Model 'x/y' is not installed locally."}"#,
        );
        assert!(e.hint.as_deref().unwrap().contains("model not found"));
        let e = classify_status(
            "Speaches",
            StatusCode::NOT_FOUND,
            r#"{"detail":"Not Found"}"#,
        );
        assert!(e.hint.as_deref().unwrap().contains("base URL"));
    }

    #[tokio::test]
    async fn retries_once_on_5xx_then_succeeds() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/x"))
            .respond_with(ResponseTemplate::new(503))
            .up_to_n_times(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/x"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;
        let client = reqwest::Client::new();
        let url = format!("{}/x", server.uri());
        let resp = send_with_retry("test", 1, || client.get(&url))
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        assert_eq!(server.received_requests().await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn a_timeout_is_tried_again_and_the_error_says_how_often() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/x"))
            .respond_with(
                ResponseTemplate::new(200).set_delay(std::time::Duration::from_millis(300)),
            )
            .mount(&server)
            .await;
        let client = reqwest::Client::new();
        let url = format!("{}/x", server.uri());
        let err = send_with_retry("test", 2, || {
            client
                .get(&url)
                .timeout(std::time::Duration::from_millis(50))
        })
        .await
        .unwrap_err();
        assert_eq!(err.code, ErrorCode::ProviderTimeout);
        assert!(
            err.message.ends_with("(after 2 retries)"),
            "{}",
            err.message
        );
        assert_eq!(server.received_requests().await.unwrap().len(), 3);
    }

    #[tokio::test]
    async fn no_retries_means_one_attempt() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/x"))
            .respond_with(ResponseTemplate::new(503))
            .mount(&server)
            .await;
        let client = reqwest::Client::new();
        let url = format!("{}/x", server.uri());
        let err = send_with_retry("test", 0, || client.get(&url))
            .await
            .unwrap_err();
        assert!(!err.message.contains("retries"), "{}", err.message);
        assert_eq!(server.received_requests().await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn does_not_retry_4xx_and_maps_401() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/x"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;
        let client = reqwest::Client::new();
        let url = format!("{}/x", server.uri());
        let err = send_with_retry("test", 1, || client.get(&url))
            .await
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::ProviderUnauthorized);
        assert_eq!(server.received_requests().await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn timeout_maps_without_retry() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/x"))
            .respond_with(
                ResponseTemplate::new(200).set_delay(std::time::Duration::from_millis(300)),
            )
            .mount(&server)
            .await;
        let client = reqwest::Client::new();
        let url = format!("{}/x", server.uri());
        let err = send_with_retry("test", 0, || {
            client
                .get(&url)
                .timeout(std::time::Duration::from_millis(50))
        })
        .await
        .unwrap_err();
        assert_eq!(err.code, ErrorCode::ProviderTimeout);
        assert_eq!(server.received_requests().await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn unreachable_maps_with_hint_after_one_retry() {
        // Port 9 (discard) is closed on macOS by default; connection is refused.
        let client = reqwest::Client::new();
        let err = send_with_retry("Ollama", 1, || client.get("http://127.0.0.1:9/x"))
            .await
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::ProviderUnreachable);
        assert!(err.hint.as_deref().unwrap().contains("Ollama"));
    }
}
