//! The last real request each network engine was checked with, kept on this
//! device only (never in the config or a backup), and the scan for network
//! services running on this computer.

use std::collections::BTreeMap;
use std::io::Write as _;
use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::config::ProviderConfig;
use crate::models::{ModelRole, ServiceKind, ServicePreset};
use crate::paths::DataDir;
use crate::providers::http_common::endpoint;

/// What one real request to an engine showed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EngineCheck {
    pub ok: bool,
    /// Unix seconds.
    pub at: i64,
    pub elapsed_ms: u64,
    /// What the service answered when it refused, or why nothing came back.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// The service's answer as it came, when `message` shows only part of it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// The HTTP status of a refusal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<u16>,
    pub role: ModelRole,
    /// What the engine was set to when it was checked; a change to any of
    /// these makes the check stale.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transcribe_mode: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub word_times: bool,
    /// The failure's code.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    /// Which version of the request the check was made with.
    #[serde(default = "first_probe")]
    pub probe: u32,
    /// The request has changed since; worked out when the checks are read.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub outdated: bool,
}

impl EngineCheck {
    /// The check failed, with the request of today and the engine as it is
    /// set now.
    pub fn fails(&self, cfg: &ProviderConfig) -> bool {
        !self.ok
            && self.probe >= probe_version(self.role)
            && self.service == cfg.service
            && (cfg.service.is_some()
                || self.base_url == cfg.base_url.as_ref().map(|u| u.to_string()))
            && self.model == cfg.model
            && self.reasoning_effort == cfg.reasoning_effort
            && self.transcribe_mode == cfg.transcribe_mode
            && self.word_times == cfg.word_times
    }
}

const fn first_probe() -> u32 {
    1
}

/// The version of the real request of a role, raised when what it asks of an
/// engine changes, so that earlier results read as outdated.
pub const fn probe_version(role: ModelRole) -> u32 {
    match role {
        ModelRole::Asr => 2,
        ModelRole::Llm => 1,
    }
}

/// About two seconds of spoken English, a public-domain recording, 16 kHz
/// mono: what a speech engine is asked to transcribe when it is checked.
static SPEECH_PROBE: &[u8] = include_bytes!("../assets/speech-probe.wav");
const WAV_HEADER_BYTES: usize = 44;

pub fn speech_probe() -> crate::types::AudioClip {
    crate::types::AudioClip {
        samples: SPEECH_PROBE[WAV_HEADER_BYTES..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|b| f32::from(i16::from_le_bytes(*b)) / 32768.0)
            .collect(),
        sample_rate: 16_000,
    }
}

/// The checks by engine id, in `engine-checks.json` beside the config.
pub struct CheckStore {
    path: std::path::PathBuf,
    lock: Mutex<()>,
}

impl CheckStore {
    pub fn new(dirs: &DataDir) -> Self {
        Self {
            path: dirs.config_dir.join("engine-checks.json"),
            lock: Mutex::new(()),
        }
    }

    /// Every stored check; an unreadable file reads as none.
    pub fn all(&self) -> BTreeMap<String, EngineCheck> {
        let _g = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        read(&self.path)
    }

    pub fn put(&self, id: &str, check: EngineCheck) -> std::io::Result<()> {
        let _g = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        let mut all = read(&self.path);
        all.insert(id.to_string(), check);
        write(&self.path, &all)
    }

    /// Drops the checks of engines that are no longer configured.
    pub fn retain(&self, keep: impl Fn(&str) -> bool) -> std::io::Result<()> {
        let _g = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        let mut all = read(&self.path);
        let before = all.len();
        all.retain(|id, _| keep(id));
        if all.len() == before {
            return Ok(());
        }
        write(&self.path, &all)
    }
}

fn read(path: &Path) -> BTreeMap<String, EngineCheck> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn write(path: &Path, all: &BTreeMap<String, EngineCheck>) -> std::io::Result<()> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(dir)?;
    let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
    tmp.write_all(serde_json::to_string_pretty(all).unwrap().as_bytes())?;
    tmp.persist(path).map_err(|e| e.error)?;
    Ok(())
}

/// A network service found running on this computer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LocalService {
    pub id: String,
    pub label: String,
    pub base_url: String,
    /// Whether it answered; one that did not is listed as not running.
    pub running: bool,
    pub models: Vec<String>,
}

/// Asks each local service of the catalogue for its models, all at once,
/// each given `timeout`.
pub async fn scan_local(
    client: &reqwest::Client,
    services: &[ServicePreset],
    timeout: Duration,
) -> Vec<LocalService> {
    let tries = services
        .iter()
        .filter(|s| s.kind == ServiceKind::Local)
        .filter_map(|s| Some((s, s.base()?)))
        .map(|(s, base)| async move {
            let models = async {
                let resp = client
                    .get(endpoint(&base, "models"))
                    .timeout(timeout)
                    .send()
                    .await
                    .ok()?;
                if !resp.status().is_success() {
                    return None;
                }
                let v: serde_json::Value = resp.json().await.ok()?;
                Some(
                    v["data"]
                        .as_array()
                        .map(|list| {
                            list.iter()
                                .filter_map(|m| m["id"].as_str().map(str::to_string))
                                .collect()
                        })
                        .unwrap_or_default(),
                )
            }
            .await;
            LocalService {
                id: s.id.clone(),
                label: s.label.clone(),
                base_url: base.to_string(),
                running: models.is_some(),
                models: models.unwrap_or_default(),
            }
        });
    futures_util::future::join_all(tries).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn check(ok: bool) -> EngineCheck {
        EngineCheck {
            ok,
            at: 1,
            elapsed_ms: 2,
            message: None,
            detail: None,
            status: None,
            role: ModelRole::Asr,
            service: Some("gemini".into()),
            base_url: None,
            model: Some("m".into()),
            reasoning_effort: None,
            transcribe_mode: None,
            word_times: false,
            code: None,
            probe: probe_version(ModelRole::Asr),
            outdated: false,
        }
    }

    #[test]
    fn the_speech_probe_holds_speech() {
        let clip = speech_probe();
        assert!((clip.duration_secs() - 2.0).abs() < 0.01);
        let peak = clip.samples.iter().fold(0.0_f32, |m, s| m.max(s.abs()));
        assert!(peak > 0.3, "{peak}");
    }

    #[test]
    fn a_check_stored_before_probes_had_versions_is_the_first_one() {
        let c: EngineCheck =
            serde_json::from_str(r#"{"ok":true,"at":1,"elapsed_ms":2,"role":"asr"}"#).unwrap();
        assert_eq!(c.probe, 1);
        assert!(c.probe < probe_version(ModelRole::Asr));
    }

    #[test]
    fn checks_are_kept_per_engine_and_dropped_with_it() {
        let tmp = tempfile::tempdir().unwrap();
        let dirs = DataDir::custom(tmp.path());
        dirs.ensure().unwrap();
        let store = CheckStore::new(&dirs);
        assert!(store.all().is_empty());
        store.put("a", check(true)).unwrap();
        store.put("b", check(false)).unwrap();
        store.put("a", check(false)).unwrap();
        assert_eq!(store.all().len(), 2);
        assert!(!store.all()["a"].ok);
        store.retain(|id| id == "b").unwrap();
        assert_eq!(store.all().keys().collect::<Vec<_>>(), ["b"]);
        std::fs::write(dirs.config_dir.join("engine-checks.json"), "not json").unwrap();
        assert!(store.all().is_empty());
    }

    #[tokio::test]
    async fn a_local_scan_lists_who_answered() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"data":[{"id":"gemma3:4b"}]})),
            )
            .mount(&server)
            .await;
        let port = server.address().port();
        let presets: Vec<ServicePreset> = serde_json::from_value(serde_json::json!([
            {"id":"up","label":"Up","kind":"local","port":port,"llm":"chat"},
            {"id":"down","label":"Down","kind":"local","port":9,"llm":"chat"},
            {"id":"cloud","label":"C","kind":"cloud","base_url":"https://x.test/","llm":"chat"}
        ]))
        .unwrap();
        let found = scan_local(&reqwest::Client::new(), &presets, Duration::from_secs(1)).await;
        assert_eq!(found.len(), 2);
        assert!(found[0].running);
        assert_eq!(found[0].models, ["gemma3:4b"]);
        assert_eq!(found[1].id, "down");
        assert!(!found[1].running);
    }
}
