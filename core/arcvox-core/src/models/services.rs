//! Network services the catalogue knows: where each one is, whether it needs
//! a key, and how speech and text go to it. An engine that names a service
//! takes its address and request format from here, so a moved address only
//! needs a new catalogue.

use serde::{Deserialize, Serialize};
use url::Url;

use crate::config::ProviderConfig;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceKind {
    Cloud,
    /// Runs on this computer, at `127.0.0.1:{port}{path}`.
    Local,
}

/// How a request goes to a service.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Wire {
    /// `POST {base}/audio/transcriptions`, multipart.
    Transcriptions,
    /// `POST {base}/chat/completions` with the recording as an `input_audio` part.
    ChatAudio,
    /// `POST {base}/chat/completions`.
    Chat,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Suggested {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asr: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub llm: Option<String>,
}

/// Unknown fields are ignored so older builds keep reading newer catalogues.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ServicePreset {
    pub id: String,
    pub label: String,
    pub kind: ServiceKind,
    /// Cloud: the OpenAI-compatible root.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<Url>,
    /// Local: the port and the root path on it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default)]
    pub key_required: bool,
    /// Where a key for the service is made.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_url: Option<String>,
    /// How speech goes to the service; none when it does no speech recognition.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asr: Option<Wire>,
    /// How text goes to the service; none when it does no text rewriting.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub llm: Option<Wire>,
    #[serde(default)]
    pub suggested: Suggested,
}

impl ServicePreset {
    /// The root requests go to.
    pub fn base(&self) -> Option<Url> {
        match self.kind {
            ServiceKind::Cloud => self.base_url.clone(),
            ServiceKind::Local => {
                let path = self.path.as_deref().unwrap_or("/v1");
                Url::parse(&format!("http://127.0.0.1:{}{path}", self.port?)).ok()
            }
        }
    }

    /// A preset is usable when it has an address and at least one use.
    pub fn valid(&self) -> bool {
        let id_ok = !self.id.is_empty()
            && self
                .id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
        let base_ok = self
            .base()
            .is_some_and(|u| matches!(u.scheme(), "http" | "https"));
        let wires_ok = self.asr != Some(Wire::Chat)
            && matches!(self.llm, None | Some(Wire::Chat))
            && (self.asr.is_some() || self.llm.is_some());
        id_ok && base_ok && wires_ok
    }
}

/// Finds a service by id: the catalogue in use first, then the one built
/// into the app.
pub trait ServiceLookup: Send + Sync {
    fn services(&self) -> Vec<ServicePreset>;
    fn service(&self, id: &str) -> Option<ServicePreset> {
        self.services().into_iter().find(|s| s.id == id)
    }
}

/// Where an engine's requests go and in which format.
#[derive(Debug, Clone, PartialEq)]
pub struct Route {
    pub base: Url,
    pub asr: Wire,
    pub llm: Wire,
    /// The engine names a service the catalogue does not have; its stored
    /// address and the plain formats are used.
    pub service_missing: bool,
}

/// The route of a network engine: its service's, or for an engine without one
/// (or whose service is unknown) the stored address with the plain formats.
pub fn route(cfg: &ProviderConfig, lookup: Option<&dyn ServiceLookup>) -> Option<Route> {
    let found = cfg
        .service
        .as_deref()
        .and_then(|id| lookup.and_then(|l| l.service(id)));
    if let Some(s) = found {
        if let Some(base) = s.base() {
            return Some(Route {
                base,
                asr: s.asr.unwrap_or(Wire::Transcriptions),
                llm: Wire::Chat,
                service_missing: false,
            });
        }
    }
    Some(Route {
        base: cfg.base_url.clone()?,
        asr: Wire::Transcriptions,
        llm: Wire::Chat,
        service_missing: cfg.service.is_some(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Capabilities, ProviderKind};

    fn preset(json: &str) -> ServicePreset {
        serde_json::from_str(json).unwrap()
    }

    struct One(ServicePreset);
    impl ServiceLookup for One {
        fn services(&self) -> Vec<ServicePreset> {
            vec![self.0.clone()]
        }
    }

    fn http(service: Option<&str>, base: Option<&str>) -> ProviderConfig {
        ProviderConfig {
            id: crate::types::ProviderId("e".into()),
            label: "E".into(),
            kind: ProviderKind::Http,
            capabilities: Capabilities::default(),
            base_url: base.map(|b| Url::parse(b).unwrap()),
            model: Some("m".into()),
            api_key_ref: None,
            extra_headers: Default::default(),
            model_path: None,
            native_id: None,
            model_id: None,
            runtime: None,
            service: service.map(str::to_string),
            reasoning_effort: None,
        }
    }

    const GEMINI: &str = r#"{"id":"gemini","label":"Gemini","kind":"cloud",
        "base_url":"https://generativelanguage.googleapis.com/v1beta/openai/",
        "key_required":true,"asr":"chat_audio","llm":"chat","future_field":1}"#;

    #[test]
    fn a_local_service_is_on_this_computer() {
        let p =
            preset(r#"{"id":"ollama","label":"Ollama","kind":"local","port":11434,"llm":"chat"}"#);
        assert_eq!(p.base().unwrap().as_str(), "http://127.0.0.1:11434/v1");
        assert!(p.valid());
        let p =
            preset(r#"{"id":"x","label":"X","kind":"local","port":9931,"path":"/","llm":"chat"}"#);
        assert_eq!(p.base().unwrap().as_str(), "http://127.0.0.1:9931/");
    }

    #[test]
    fn invalid_presets_are_told_apart() {
        assert!(preset(GEMINI).valid());
        assert!(!preset(
            r#"{"id":"a b","label":"X","kind":"cloud","base_url":"https://x.test/","llm":"chat"}"#
        )
        .valid());
        assert!(!preset(r#"{"id":"x","label":"X","kind":"cloud","llm":"chat"}"#).valid());
        assert!(
            !preset(r#"{"id":"x","label":"X","kind":"cloud","base_url":"https://x.test/"}"#)
                .valid()
        );
        assert!(!preset(
            r#"{"id":"x","label":"X","kind":"cloud","base_url":"https://x.test/","asr":"chat"}"#
        )
        .valid());
        assert!(!preset(
            r#"{"id":"x","label":"X","kind":"cloud","base_url":"ftp://x.test/","llm":"chat"}"#
        )
        .valid());
    }

    #[test]
    fn an_engine_takes_its_services_address_and_format() {
        let lookup = One(preset(GEMINI));
        let r = route(
            &http(Some("gemini"), Some("https://old.test/v1")),
            Some(&lookup),
        )
        .unwrap();
        assert_eq!(
            r.base.as_str(),
            "https://generativelanguage.googleapis.com/v1beta/openai/"
        );
        assert_eq!(
            (r.asr, r.llm, r.service_missing),
            (Wire::ChatAudio, Wire::Chat, false)
        );
    }

    #[test]
    fn an_unknown_service_falls_back_to_the_stored_address() {
        let lookup = One(preset(GEMINI));
        let r = route(
            &http(Some("gone"), Some("https://old.test/v1")),
            Some(&lookup),
        )
        .unwrap();
        assert_eq!(r.base.as_str(), "https://old.test/v1");
        assert_eq!((r.asr, r.service_missing), (Wire::Transcriptions, true));
        let r = route(&http(None, Some("http://localhost:8000/v1")), None).unwrap();
        assert!(!r.service_missing);
        assert!(route(&http(Some("gone"), None), Some(&lookup)).is_none());
    }
}
