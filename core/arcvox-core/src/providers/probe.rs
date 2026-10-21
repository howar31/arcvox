//! Reachability and auth check for an HTTP provider: `GET {base}/models`.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::config::ProviderConfig;
use crate::providers::http_common::endpoint;
use crate::secrets::SecretStore;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProbeFailure {
    Unreachable,
    Unauthorized,
    /// The server was reached but answered with a 5xx status: it is there,
    /// and failing for the moment.
    ServerError {
        status: u16,
    },
    NotCompatible,
    MissingBaseUrl,
    MissingKey,
    ModelMissing,
    ModelCorrupt,
    NativeUnavailable {
        reason: String,
    },
    Other {
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProbeResult {
    pub ok: bool,
    pub failure: Option<ProbeFailure>,
}

impl ProbeResult {
    fn fail(f: ProbeFailure) -> Self {
        Self {
            ok: false,
            failure: Some(f),
        }
    }
}

pub async fn probe_http(
    client: &reqwest::Client,
    cfg: &ProviderConfig,
    secrets: &Arc<dyn SecretStore>,
) -> ProbeResult {
    let Some(base) = &cfg.base_url else {
        return ProbeResult::fail(ProbeFailure::MissingBaseUrl);
    };
    let key = match &cfg.api_key_ref {
        Some(r) => {
            // The keychain answers on its own time; the worker is not held.
            let (secrets, r) = (secrets.clone(), r.clone());
            let found = tokio::task::spawn_blocking(move || secrets.get(&r))
                .await
                .unwrap_or_else(|e| Err(crate::secrets::SecretError::Unavailable(e.to_string())));
            match found {
                Ok(Some(k)) => Some(k),
                Ok(None) => return ProbeResult::fail(ProbeFailure::MissingKey),
                Err(e) => {
                    return ProbeResult::fail(ProbeFailure::Other {
                        message: e.to_string(),
                    })
                }
            }
        }
        None => None,
    };
    let mut req = client
        .get(endpoint(base, "models"))
        .timeout(std::time::Duration::from_secs(5));
    if let Some(k) = key {
        req = req.bearer_auth(k);
    }
    for (k, v) in &cfg.extra_headers {
        req = req.header(k, v);
    }
    match req.send().await {
        Err(e) if e.is_connect() || e.is_timeout() => ProbeResult::fail(ProbeFailure::Unreachable),
        Err(e) => ProbeResult::fail(ProbeFailure::Other {
            message: e.to_string(),
        }),
        Ok(resp) => match resp.status().as_u16() {
            401 | 403 => ProbeResult::fail(ProbeFailure::Unauthorized),
            status @ 500..=599 => ProbeResult::fail(ProbeFailure::ServerError { status }),
            200 => match resp.json::<serde_json::Value>().await {
                Ok(v) if v.get("data").map(|d| d.is_array()).unwrap_or(false) => ProbeResult {
                    ok: true,
                    failure: None,
                },
                _ => ProbeResult::fail(ProbeFailure::NotCompatible),
            },
            _ => ProbeResult::fail(ProbeFailure::NotCompatible),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::secrets::MemorySecretStore;
    use serde_json::json;
    use url::Url;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn cfg(server: &MockServer, key_ref: Option<&str>) -> ProviderConfig {
        let mut c = Config::http_test_config().providers.remove(1);
        c.base_url = Some(Url::parse(&format!("{}/v1", server.uri())).unwrap());
        c.api_key_ref = key_ref.map(String::from);
        c
    }

    fn secrets() -> Arc<dyn SecretStore> {
        let s = MemorySecretStore::default();
        s.set("k", "secret").unwrap();
        Arc::new(s)
    }

    #[tokio::test]
    async fn ok_on_models_list_with_bearer() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .and(header("authorization", "Bearer secret"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({"data":[{"id":"gemma3"}]})),
            )
            .mount(&server)
            .await;
        let r = probe_http(
            &reqwest::Client::new(),
            &cfg(&server, Some("k")),
            &secrets(),
        )
        .await;
        assert_eq!(
            r,
            ProbeResult {
                ok: true,
                failure: None
            }
        );
    }

    #[tokio::test]
    async fn unauthorized_and_not_compatible() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;
        let r = probe_http(&reqwest::Client::new(), &cfg(&server, None), &secrets()).await;
        assert_eq!(r.failure, Some(ProbeFailure::Unauthorized));

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(ResponseTemplate::new(200).set_body_string("<html>"))
            .mount(&server)
            .await;
        let r = probe_http(&reqwest::Client::new(), &cfg(&server, None), &secrets()).await;
        assert_eq!(r.failure, Some(ProbeFailure::NotCompatible));

        // A server that is there but failing is not an incompatible one.
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(ResponseTemplate::new(503))
            .mount(&server)
            .await;
        let r = probe_http(&reqwest::Client::new(), &cfg(&server, None), &secrets()).await;
        assert_eq!(r.failure, Some(ProbeFailure::ServerError { status: 503 }));

        // A path that is not there is still the wrong kind of server.
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;
        let r = probe_http(&reqwest::Client::new(), &cfg(&server, None), &secrets()).await;
        assert_eq!(r.failure, Some(ProbeFailure::NotCompatible));
    }

    #[tokio::test]
    async fn missing_key_and_unreachable() {
        let server = MockServer::start().await;
        let r = probe_http(
            &reqwest::Client::new(),
            &cfg(&server, Some("absent")),
            &secrets(),
        )
        .await;
        assert_eq!(r.failure, Some(ProbeFailure::MissingKey));

        let mut c = cfg(&server, None);
        c.base_url = Some(Url::parse("http://127.0.0.1:9/v1").unwrap());
        let r = probe_http(&reqwest::Client::new(), &c, &secrets()).await;
        assert_eq!(r.failure, Some(ProbeFailure::Unreachable));
    }

    #[test]
    fn probe_failure_serializes_all_variants() {
        let variants = [
            ProbeFailure::Unreachable,
            ProbeFailure::Unauthorized,
            ProbeFailure::ServerError { status: 503 },
            ProbeFailure::NotCompatible,
            ProbeFailure::MissingBaseUrl,
            ProbeFailure::MissingKey,
            ProbeFailure::Other {
                message: "x".into(),
            },
        ];
        for v in variants {
            let json = serde_json::to_value(&v).unwrap();
            let back: ProbeFailure = serde_json::from_value(json).unwrap();
            assert_eq!(back, v);
        }
        assert_eq!(
            serde_json::to_value(ProbeFailure::Other {
                message: "x".into()
            })
            .unwrap(),
            json!({"kind": "other", "message": "x"})
        );
    }
}
