//! The settings file: profiles, engines, dictionary, base prompt and the
//! client's own settings as one JSON document, each kind in a section of
//! its own. A file may hold any subset of the sections.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::config::validate::fits;
use crate::config::{
    Config, DefaultEngines, DictionaryEntry, EngineRole, General, Profile, ProviderConfig,
    ProviderKind,
};
use crate::engine::{Engine, EngineError};
use crate::history::now_unix;
use crate::types::ProviderId;

pub const SETTINGS_FORMAT: &str = "arcvox-settings";
pub const SETTINGS_VERSION: u32 = 1;
/// Written and read with `providers`: the default pair is a choice among
/// the engines, not a section of its own.
const DEFAULT_ENGINES_SECTION: &str = "default_engines";

/// The sections the core fills and applies. `general` is shared with the
/// client: the core's part is its `core` member.
pub const CORE_SECTIONS: [&str; 5] = [
    "profiles",
    "providers",
    "dictionary",
    "base_prompt",
    "general",
];
/// The sections that are settings: how the app behaves on a device.
pub const SETTING_SECTIONS: [&str; 4] = ["recording", "insertion", "general", "appearance"];
/// The sections that are content: what the user made.
pub const CONTENT_SECTIONS: [&str; 4] = ["profiles", "providers", "dictionary", "base_prompt"];
/// Every section this version knows, the client's included.
pub const ALL_SECTIONS: [&str; 8] = [
    "profiles",
    "providers",
    "dictionary",
    "base_prompt",
    "recording",
    "insertion",
    "general",
    "appearance",
];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SettingsFile {
    pub format: String,
    pub version: u32,
    /// Seconds since the epoch.
    #[serde(default)]
    pub exported_at: i64,
    #[serde(default)]
    pub app_version: String,
    /// The platform whose application ids the file uses.
    #[serde(default)]
    pub platform: String,
    #[serde(default)]
    pub device_name: String,
    #[serde(default)]
    pub sections: BTreeMap<String, Value>,
}

#[derive(Debug, thiserror::Error)]
pub enum SettingsFileError {
    #[error("not a settings file: {0}")]
    NotSettings(String),
    #[error("the settings file was made by a newer version (format {0})")]
    Newer(u32),
    #[error("the section `{name}` cannot be read: {reason}")]
    Section { name: String, reason: String },
}

/// One thing the user should know after an import.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Note {
    /// The engine names an API key that is not stored on this machine.
    NeedsKey { engine: String },
    /// The profile named an engine of the wrong kind for the role; it
    /// follows the default now.
    EngineReplaced { profile: String, role: String },
    /// The default engine for the role named nothing that can serve it;
    /// the factory one, else the first fitting engine, was set.
    DefaultEngineReset { role: String },
    /// The engine cannot run on this machine.
    EngineUnavailable { engine: String },
    /// The file has a section this version does not know.
    UnknownSection { name: String },
    /// Application ids in the settings that no installed application has.
    AppsNotInstalled { names: Vec<String> },
}

/// What an import did.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SettingsOutcome {
    /// The sections that were replaced, in the order of `ALL_SECTIONS`.
    pub applied: Vec<String>,
    pub notes: Vec<Note>,
    /// The files as they were before, each with the copy made of it.
    #[serde(skip)]
    pub copies: Vec<(PathBuf, PathBuf)>,
}

impl SettingsFile {
    pub fn new(app_version: &str, platform: &str, device_name: &str) -> SettingsFile {
        SettingsFile {
            format: SETTINGS_FORMAT.into(),
            version: SETTINGS_VERSION,
            exported_at: now_unix(),
            app_version: app_version.into(),
            platform: platform.into(),
            device_name: device_name.into(),
            sections: BTreeMap::new(),
        }
    }

    /// Reads a settings file; a file of another kind or of a newer version
    /// is refused.
    pub fn parse(bytes: &[u8]) -> Result<SettingsFile, SettingsFileError> {
        let value: Value = serde_json::from_slice(bytes)
            .map_err(|e| SettingsFileError::NotSettings(e.to_string()))?;
        if value.get("format").and_then(Value::as_str) != Some(SETTINGS_FORMAT) {
            return Err(SettingsFileError::NotSettings(
                "the file does not say it is a settings file".into(),
            ));
        }
        let version = value.get("version").and_then(Value::as_u64).unwrap_or(0) as u32;
        if version > SETTINGS_VERSION {
            return Err(SettingsFileError::Newer(version));
        }
        serde_json::from_value(value).map_err(|e| SettingsFileError::NotSettings(e.to_string()))
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        serde_json::to_vec_pretty(self).expect("a settings file serializes")
    }

    /// The names of sections this version does not know.
    pub fn unknown_sections(&self) -> Vec<String> {
        self.sections
            .keys()
            .filter(|k| {
                !ALL_SECTIONS.contains(&k.as_str()) && k.as_str() != DEFAULT_ENGINES_SECTION
            })
            .cloned()
            .collect()
    }
}

fn section<T: serde::de::DeserializeOwned>(
    name: &str,
    value: &Value,
) -> Result<T, SettingsFileError> {
    serde_json::from_value(value.clone()).map_err(|e| SettingsFileError::Section {
        name: name.into(),
        reason: e.to_string(),
    })
}

/// The core's sections of a file, read and checked for shape.
#[derive(Default)]
struct Picked {
    profiles: Option<Vec<Profile>>,
    providers: Option<Vec<ProviderConfig>>,
    /// Travels with the engines: a file from before the default pair
    /// existed has none, and the pair in place stays.
    default_engines: Option<DefaultEngines>,
    dictionary: Option<Vec<DictionaryEntry>>,
    base_prompt: Option<String>,
    general: Option<General>,
}

impl Picked {
    fn read(file: &SettingsFile, sections: &[String]) -> Result<Picked, SettingsFileError> {
        let get = |name: &str| {
            sections
                .iter()
                .any(|s| s == name)
                .then(|| file.sections.get(name))
                .flatten()
        };
        let mut picked = Picked::default();
        if let Some(v) = get("profiles") {
            picked.profiles = Some(section("profiles", v)?);
        }
        if let Some(v) = get("providers") {
            picked.providers = Some(section("providers", v)?);
            if let Some(d) = file.sections.get(DEFAULT_ENGINES_SECTION) {
                picked.default_engines = Some(section(DEFAULT_ENGINES_SECTION, d)?);
            }
        }
        if let Some(v) = get("dictionary") {
            picked.dictionary = Some(section("dictionary", v)?);
        }
        if let Some(v) = get("base_prompt") {
            picked.base_prompt = Some(section("base_prompt", v)?);
        }
        if let Some(core) = get("general").and_then(|v| v.get("core")) {
            picked.general = Some(section("general", core)?);
        }
        Ok(picked)
    }

    fn applied(&self) -> Vec<String> {
        [
            ("profiles", self.profiles.is_some()),
            ("providers", self.providers.is_some()),
            ("dictionary", self.dictionary.is_some()),
            ("base_prompt", self.base_prompt.is_some()),
            ("general", self.general.is_some()),
        ]
        .into_iter()
        .filter(|(_, on)| *on)
        .map(|(name, _)| name.to_string())
        .collect()
    }
}

/// Makes exactly one profile the default, the default pair name engines
/// that can serve their roles (else the factory engine, made if need be),
/// and no profile name an engine of the wrong kind. A profile naming an
/// engine that is gone is left as it is: the default stands in until the
/// engine is back.
fn repair(config: &mut Config, notes: &mut Vec<Note>) {
    if !config.profiles.iter().any(|p| p.is_default) {
        if let Some(first) = config.profiles.first_mut() {
            first.is_default = true;
        }
    }
    let mut seen_default = false;
    for p in &mut config.profiles {
        if p.is_default && seen_default {
            p.is_default = false;
        }
        seen_default |= p.is_default;
    }
    let factory = DefaultEngines::default();
    let mut defaults = config.default_engines.clone();
    for (role, chosen, name) in [
        (EngineRole::Asr, &mut defaults.asr, "asr"),
        (EngineRole::Llm, &mut defaults.llm, "llm"),
    ] {
        let serves = |id: &ProviderId| config.provider(id).is_some_and(|e| fits(e, role));
        if serves(chosen) {
            continue;
        }
        // Back to the factory engine, made as on the first launch when the
        // file did not carry it.
        let factory_id = factory.for_role(role).clone();
        if !serves(&factory_id) {
            config.providers.retain(|e| e.id != factory_id);
            config.providers.push(ProviderConfig::factory_engine(role));
        }
        *chosen = factory_id;
        notes.push(Note::DefaultEngineReset { role: name.into() });
    }
    config.default_engines = defaults;
    let wrong_kind: Vec<(usize, EngineRole, &str)> = config
        .profiles
        .iter()
        .enumerate()
        .flat_map(|(i, p)| {
            [
                (EngineRole::Asr, &p.asr_provider, "asr"),
                (EngineRole::Llm, &p.llm_provider, "llm"),
            ]
            .into_iter()
            .filter(|(role, named, _)| {
                named
                    .as_ref()
                    .and_then(|id| config.provider(id))
                    .is_some_and(|e| !fits(e, *role))
            })
            .map(move |(role, _, name)| (i, role, name))
        })
        .collect();
    for (i, role, name) in wrong_kind {
        let p = &mut config.profiles[i];
        match role {
            EngineRole::Asr => p.asr_provider = None,
            EngineRole::Llm => p.llm_provider = None,
        }
        notes.push(Note::EngineReplaced {
            profile: p.name.clone(),
            role: name.into(),
        });
    }
}

impl Engine {
    /// The core's part of the named sections, for a settings file. API keys
    /// are never part of it: an engine carries only the name of its key.
    pub fn settings_export(
        &self,
        sections: &[String],
    ) -> Result<BTreeMap<String, Value>, EngineError> {
        let config = self.config().current();
        let has = |name: &str| sections.iter().any(|s| s == name);
        let mut out = BTreeMap::new();
        if has("profiles") {
            out.insert(
                "profiles".to_string(),
                serde_json::to_value(&config.profiles).expect("profiles serialize"),
            );
        }
        if has("providers") {
            // A header may carry a credential, and the folder of a model file
            // is a place on this machine: the names stay, the values do not.
            let engines: Vec<ProviderConfig> = config
                .providers
                .iter()
                .cloned()
                .map(|mut e| {
                    e.extra_headers.values_mut().for_each(String::clear);
                    e.model_path = e.model_path.and_then(|p| p.file_name().map(PathBuf::from));
                    e
                })
                .collect();
            out.insert(
                "providers".to_string(),
                serde_json::to_value(&engines).expect("engines serialize"),
            );
            out.insert(
                DEFAULT_ENGINES_SECTION.to_string(),
                serde_json::to_value(&config.default_engines).expect("default engines serialize"),
            );
        }
        if has("dictionary") {
            out.insert(
                "dictionary".to_string(),
                serde_json::to_value(&config.dictionary).expect("dictionary serializes"),
            );
        }
        if has("base_prompt") {
            out.insert(
                "base_prompt".to_string(),
                Value::String(self.config().base_prompt()?),
            );
        }
        if has("general") {
            let mut general = serde_json::to_value(&config.general).expect("general serializes");
            // The folders of the recordings and the models are places on this machine.
            if let Some(map) = general.as_object_mut() {
                map.remove("recordings_dir");
                map.remove("models_dir");
            }
            out.insert(
                "general".to_string(),
                serde_json::json!({ "core": general }),
            );
        }
        Ok(out)
    }

    /// Replaces the core's part of each named section the file has. The
    /// files are copied aside first; when the result does not validate,
    /// nothing is changed.
    pub async fn settings_import(
        &self,
        file: &SettingsFile,
        sections: &[String],
    ) -> Result<SettingsOutcome, EngineError> {
        let picked = Picked::read(file, sections)?;
        let applied = picked.applied();
        let mut notes: Vec<Note> = file
            .unknown_sections()
            .into_iter()
            .map(|name| Note::UnknownSection { name })
            .collect();
        if applied.is_empty() {
            return Ok(SettingsOutcome {
                applied,
                notes,
                copies: Vec::new(),
            });
        }
        let copies = self.config().backup_copies()?;
        let engines_changed = picked.providers.is_some();
        let prompt = picked.base_prompt.clone();
        let mut repaired = Vec::new();
        let next = self.config().update(|c| {
            if let Some(v) = picked.profiles {
                c.profiles = v;
            }
            if let Some(v) = picked.providers {
                c.providers = v;
            }
            if let Some(v) = picked.default_engines {
                c.default_engines = v;
            }
            if let Some(v) = picked.dictionary {
                c.dictionary = v;
            }
            if let Some(v) = picked.general {
                let recordings_dir = c.general.recordings_dir.take();
                let models_dir = c.general.models_dir.take();
                c.general = General {
                    recordings_dir,
                    models_dir,
                    ..v
                };
            }
            repair(c, &mut repaired);
        })?;
        notes.extend(repaired);
        if let Some(text) = prompt {
            self.config().set_base_prompt(&text)?;
        }
        if engines_changed {
            for e in &next.providers {
                let has_key = match &e.api_key_ref {
                    Some(key) => matches!(self.secrets().get(key), Ok(Some(_))),
                    None => true,
                };
                if !has_key {
                    notes.push(Note::NeedsKey {
                        engine: e.label.clone(),
                    });
                }
                // An engine of another platform. One that exists here and is
                // not ready yet says so on the engines page.
                if e.kind == ProviderKind::Native && !self.has_factory(e.kind) {
                    notes.push(Note::EngineUnavailable {
                        engine: e.label.clone(),
                    });
                }
            }
        }
        Ok(SettingsOutcome {
            applied,
            notes,
            copies,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, EngineSource, ProviderConfig};
    use crate::engine::EngineError;
    use crate::secrets::MemorySecretStore;
    use crate::types::{ProfileId, ProviderId};
    use crate::{DataDir, Engine};
    use serde_json::json;
    use std::sync::Arc;

    async fn engine(root: &std::path::Path) -> Engine {
        Engine::open(
            DataDir::custom(root),
            Arc::new(MemorySecretStore::default()),
        )
        .await
        .unwrap()
    }

    fn all() -> Vec<String> {
        CORE_SECTIONS.iter().map(|s| s.to_string()).collect()
    }

    fn only(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    fn file_of(e: &Engine, sections: &[String]) -> SettingsFile {
        let mut file = SettingsFile::new("0.1.0", "macos", "Anna");
        file.sections = e.settings_export(sections).unwrap();
        file
    }

    /// An engine whose config differs from the first-run one in every section.
    async fn changed(root: &std::path::Path) -> Engine {
        let e = engine(root).await;
        e.config()
            .update(|c| {
                c.general.retry_count = 3;
                c.general.typing_chars_per_min = 77;
                c.general.recordings_dir = Some("/somewhere/else".into());
                c.dictionary.push(crate::config::DictionaryEntry {
                    term: "Arcvox".into(),
                    aliases: vec!["雅客".into()],
                    asr_hint: true,
                });
                let mut mail = c.profiles[0].clone();
                mail.id = ProfileId("mail".into());
                mail.name = "Mail".into();
                mail.is_default = false;
                mail.matcher.app_ids = vec!["com.apple.mail".into()];
                mail.prompt_addition = "formal".into();
                c.profiles.push(mail);
            })
            .unwrap();
        e.config().set_base_prompt("my prompt").unwrap();
        e
    }

    #[test]
    fn a_file_of_another_kind_is_refused() {
        for text in [
            "{}",
            "not json",
            r#"{"format":"arcvox-backup","version":1}"#,
            r#"{"schema_version":1,"general":{},"providers":[],"profiles":[],"dictionary":[]}"#,
        ] {
            let got = SettingsFile::parse(text.as_bytes());
            assert!(
                matches!(got, Err(SettingsFileError::NotSettings(_))),
                "{text}"
            );
        }
    }

    #[test]
    fn the_two_kinds_together_are_every_section() {
        let mut both: Vec<&str> = SETTING_SECTIONS
            .iter()
            .chain(&CONTENT_SECTIONS)
            .copied()
            .collect();
        let mut all = ALL_SECTIONS.to_vec();
        both.sort();
        all.sort();
        assert_eq!(both, all);
    }

    #[test]
    fn a_newer_version_is_refused() {
        let got = SettingsFile::parse(br#"{"format":"arcvox-settings","version":2,"sections":{}}"#);
        assert!(matches!(got, Err(SettingsFileError::Newer(2))));
    }

    #[test]
    fn a_written_file_reads_back() {
        let mut file = SettingsFile::new("0.1.0", "macos", "Anna");
        file.sections.insert("base_prompt".into(), json!("p"));
        let back = SettingsFile::parse(&file.to_bytes()).unwrap();
        assert_eq!(back, file);
        assert!(back.exported_at > 0);
    }

    #[tokio::test]
    async fn every_core_section_round_trips() {
        let tmp = tempfile::tempdir().unwrap();
        let a = changed(&tmp.path().join("a")).await;
        let file = file_of(&a, &all());
        assert!(!serde_json::to_string(&file)
            .unwrap()
            .contains("/somewhere/else"));

        let b = engine(&tmp.path().join("b")).await;
        let outcome = b.settings_import(&file, &all()).await.unwrap();
        assert_eq!(outcome.applied, all());
        let (ca, cb) = (a.config().current(), b.config().current());
        assert_eq!(cb.profiles, ca.profiles);
        assert_eq!(cb.providers, ca.providers);
        assert_eq!(cb.dictionary, ca.dictionary);
        assert_eq!(cb.general.retry_count, 3);
        assert_eq!(cb.general.typing_chars_per_min, 77);
        assert_eq!(cb.general.recordings_dir, None);
        assert_eq!(b.config().base_prompt().unwrap(), "my prompt");
    }

    #[tokio::test]
    async fn sections_not_picked_stay() {
        let tmp = tempfile::tempdir().unwrap();
        let a = changed(&tmp.path().join("a")).await;
        let file = file_of(&a, &all());
        let b = engine(&tmp.path().join("b")).await;
        let before = b.config().current();
        let outcome = b
            .settings_import(&file, &only(&["dictionary"]))
            .await
            .unwrap();
        assert_eq!(outcome.applied, only(&["dictionary"]));
        let after = b.config().current();
        assert_eq!(after.dictionary.len(), 1);
        assert_eq!(after.profiles, before.profiles);
        assert_eq!(after.general, before.general);
        assert_ne!(b.config().base_prompt().unwrap(), "my prompt");
    }

    #[tokio::test]
    async fn a_section_the_file_lacks_is_not_applied() {
        let tmp = tempfile::tempdir().unwrap();
        let a = changed(&tmp.path().join("a")).await;
        let file = file_of(&a, &only(&["dictionary"]));
        let b = engine(&tmp.path().join("b")).await;
        let outcome = b.settings_import(&file, &all()).await.unwrap();
        assert_eq!(outcome.applied, only(&["dictionary"]));
        assert_eq!(b.config().current().profiles.len(), 1);
    }

    #[tokio::test]
    async fn a_file_with_one_profile_is_valid() {
        let tmp = tempfile::tempdir().unwrap();
        let a = changed(&tmp.path().join("a")).await;
        let mut file = SettingsFile::new("0.1.0", "macos", "Anna");
        let mail = a.config().current().profiles[1].clone();
        file.sections.insert("profiles".into(), json!([mail]));
        let b = engine(&tmp.path().join("b")).await;
        b.settings_import(&SettingsFile::parse(&file.to_bytes()).unwrap(), &all())
            .await
            .unwrap();
        let profiles = b.config().current().profiles.clone();
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].id, ProfileId("mail".into()));
        assert!(profiles[0].is_default);
    }

    fn http(id: &str, transcribe: bool) -> ProviderConfig {
        ProviderConfig::http_test_provider(id, "http://localhost:9/v1", transcribe, !transcribe)
    }

    #[tokio::test]
    async fn profiles_alone_keep_their_engines_and_run_on_the_default_meanwhile() {
        let tmp = tempfile::tempdir().unwrap();
        let a = engine(&tmp.path().join("a")).await;
        a.config()
            .update(|c| {
                c.providers.push(http("far-asr", true));
                c.providers.push(http("far-llm", false));
                c.profiles[0].asr_provider = Some(ProviderId("far-asr".into()));
                c.profiles[0].llm_provider = Some(ProviderId("far-llm".into()));
            })
            .unwrap();
        let file = file_of(&a, &only(&["profiles"]));
        let b = engine(&tmp.path().join("b")).await;
        let defaults = b.config().current().default_engines.clone();
        let outcome = b
            .settings_import(&file, &only(&["profiles"]))
            .await
            .unwrap();
        let p = b.config().current().profiles[0].clone();
        assert_eq!(p.asr_provider, Some(ProviderId("far-asr".into())));
        assert_eq!(p.llm_provider, Some(ProviderId("far-llm".into())));
        let engines = b.resolve_engines(&p);
        assert_eq!(engines.asr.id, defaults.asr);
        assert_eq!(
            engines.asr.source,
            EngineSource::Fallback {
                wanted: ProviderId("far-asr".into())
            }
        );
        assert_eq!(engines.llm.as_ref().unwrap().id, defaults.llm);
        assert!(
            !outcome
                .notes
                .iter()
                .any(|n| matches!(n, Note::EngineReplaced { .. })),
            "{:?}",
            outcome.notes
        );
    }

    #[tokio::test]
    async fn an_engine_of_the_wrong_kind_is_dropped_and_the_default_pair_repaired() {
        let tmp = tempfile::tempdir().unwrap();
        let a = engine(&tmp.path().join("a")).await;
        let mut file = SettingsFile::new("0.1.0", "macos", "Anna");
        let mut p = a.config().current().profiles[0].clone();
        // The file's profile names the chat engine for speech.
        p.asr_provider = Some(ProviderId("local-ollama".into()));
        file.sections.insert("profiles".into(), json!([p]));
        file.sections.insert(
            "providers".into(),
            json!([http("far-asr", true), http("local-ollama", false)]),
        );
        file.sections.insert(
            "default_engines".into(),
            json!({ "asr": "far-asr", "llm": "nowhere" }),
        );
        let file = SettingsFile::parse(&file.to_bytes()).unwrap();
        let outcome = a.settings_import(&file, &all()).await.unwrap();
        let c = a.config().current();
        assert_eq!(c.profiles[0].asr_provider, None, "follows the default now");
        assert_eq!(c.default_engines.asr, ProviderId("far-asr".into()));
        // The pair named a chat engine that is not there: back to the
        // factory one, made because the file did not carry it.
        assert_eq!(
            c.default_engines.llm,
            ProviderId(crate::config::MODEL_ID_DEFAULT_LLM.into())
        );
        assert!(c.provider(&c.default_engines.llm).is_some());
        assert!(outcome.notes.iter().any(|n| matches!(
            n,
            Note::EngineReplaced { role, .. } if role == "asr"
        )));
        assert!(outcome.notes.iter().any(|n| matches!(
            n,
            Note::DefaultEngineReset { role } if role == "llm"
        )));
    }

    #[tokio::test]
    async fn engines_alone_keep_every_profile_usable() {
        let tmp = tempfile::tempdir().unwrap();
        let a = engine(&tmp.path().join("a")).await;
        a.config()
            .update(|c| {
                c.providers = vec![http("far-asr", true), http("far-llm", false)];
                c.default_engines.asr = ProviderId("far-asr".into());
                c.default_engines.llm = ProviderId("far-llm".into());
            })
            .unwrap();
        let file = file_of(&a, &only(&["providers"]));
        assert!(file.sections.contains_key("default_engines"));
        let b = changed(&tmp.path().join("b")).await;
        let outcome = b
            .settings_import(&file, &only(&["providers"]))
            .await
            .unwrap();
        let c = b.config().current();
        assert_eq!(c.profiles.len(), 2);
        assert_eq!(c.default_engines.asr, ProviderId("far-asr".into()));
        assert_eq!(c.default_engines.llm, ProviderId("far-llm".into()));
        for p in &c.profiles {
            let engines = b.resolve_engines(p);
            assert_eq!(engines.asr.id, ProviderId("far-asr".into()));
            assert_eq!(
                engines.llm.map(|e| e.id),
                Some(ProviderId("far-llm".into()))
            );
        }
        crate::config::validate(&c).unwrap();
        assert!(outcome
            .notes
            .iter()
            .all(|n| !matches!(n, Note::EngineReplaced { .. })));
    }

    #[tokio::test]
    async fn an_engine_with_a_key_reference_asks_for_the_key() {
        let tmp = tempfile::tempdir().unwrap();
        let a = engine(&tmp.path().join("a")).await;
        a.config()
            .update(|c| {
                let mut cloud = http("cloud", false);
                cloud.label = "Cloud".into();
                cloud.api_key_ref = Some("cloud-key".into());
                c.providers.push(cloud);
            })
            .unwrap();
        let file = file_of(&a, &only(&["providers"]));
        assert!(!serde_json::to_string(&file).unwrap().contains("sk-"));
        let b = engine(&tmp.path().join("b")).await;
        let outcome = b
            .settings_import(&file, &only(&["providers"]))
            .await
            .unwrap();
        assert!(
            outcome.notes.contains(&Note::NeedsKey {
                engine: "Cloud".into()
            }),
            "{:?}",
            outcome.notes
        );
    }

    #[tokio::test]
    async fn header_values_and_model_paths_are_not_written() {
        let tmp = tempfile::tempdir().unwrap();
        let a = engine(&tmp.path().join("a")).await;
        a.config()
            .update(|c| {
                let mut cloud = http("cloud", false);
                cloud
                    .extra_headers
                    .insert("x-api-key".into(), "header-secret-value".into());
                cloud.model_path = Some("/home/someone/models/private.gguf".into());
                c.providers.push(cloud);
            })
            .unwrap();
        let text = serde_json::to_string(&file_of(&a, &all())).unwrap();
        assert!(!text.contains("header-secret-value"));
        assert!(!text.contains("/home/someone"));
        // The name of the header stays, so the engine page shows what to fill in.
        assert!(text.contains("x-api-key"));
    }

    /// A platform engine that exists here and is not ready (a language still
    /// to be downloaded, for example).
    struct NotReady;
    #[async_trait::async_trait]
    impl crate::ProviderFactory for NotReady {
        fn kind(&self) -> crate::config::ProviderKind {
            crate::config::ProviderKind::Native
        }
        fn asr(
            &self,
            _: &ProviderConfig,
            _: &crate::config::General,
        ) -> Result<Arc<dyn crate::providers::AsrProvider>, crate::providers::ProviderError>
        {
            Ok(crate::steps::asr::fakes::FakeAsr::ok("hi"))
        }
        fn llm(
            &self,
            _: &ProviderConfig,
            _: &crate::config::General,
        ) -> Result<Arc<dyn crate::providers::LlmProvider>, crate::providers::ProviderError>
        {
            Ok(crate::steps::llm::fakes::FakeLlm::ok("hi"))
        }
        async fn probe(&self, _: &ProviderConfig) -> crate::providers::probe::ProbeResult {
            crate::providers::probe::ProbeResult {
                ok: false,
                failure: None,
            }
        }
    }

    fn native_file() -> SettingsFile {
        let mut file = SettingsFile::new("0.1.0", "macos", "Anna");
        let mut engines = Config::first_run().providers;
        engines.push(ProviderConfig::native(
            "platform-asr",
            "Platform dictation",
            "apple.dictation",
            crate::config::Capabilities {
                transcribe: true,
                ..Default::default()
            },
        ));
        file.sections.insert("providers".into(), json!(engines));
        file
    }

    #[tokio::test]
    async fn a_platform_engine_this_build_lacks_is_named() {
        let tmp = tempfile::tempdir().unwrap();
        let b = engine(tmp.path()).await;
        let outcome = b
            .settings_import(&native_file(), &only(&["providers"]))
            .await
            .unwrap();
        assert_eq!(
            outcome.notes,
            vec![Note::EngineUnavailable {
                engine: "Platform dictation".into()
            }]
        );
    }

    #[tokio::test]
    async fn a_platform_engine_that_is_only_not_ready_is_not_named() {
        let tmp = tempfile::tempdir().unwrap();
        let b = Engine::open_with(
            DataDir::custom(tmp.path()),
            crate::EngineOptions {
                secrets: Arc::new(MemorySecretStore::default()),
                factories: vec![Arc::new(NotReady)],
                first_run: Default::default(),
                identity: Default::default(),
            },
        )
        .await
        .unwrap();
        let outcome = b
            .settings_import(&native_file(), &only(&["providers"]))
            .await
            .unwrap();
        assert_eq!(outcome.notes, vec![]);
    }

    #[tokio::test]
    async fn a_section_of_the_wrong_shape_changes_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let b = changed(&tmp.path().join("b")).await;
        let before = b.config().current();
        let mut file = SettingsFile::new("0.1.0", "macos", "Anna");
        file.sections.insert("profiles".into(), json!(5));
        file.sections
            .insert("base_prompt".into(), json!("other prompt"));
        let got = b.settings_import(&file, &all()).await;
        assert!(matches!(
            got,
            Err(EngineError::Settings(SettingsFileError::Section { .. }))
        ));
        assert_eq!(b.config().current(), before);
        assert_eq!(b.config().base_prompt().unwrap(), "my prompt");
    }

    #[tokio::test]
    async fn an_import_that_does_not_validate_changes_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let b = changed(&tmp.path().join("b")).await;
        let before = b.config().current();
        let mut file = SettingsFile::new("0.1.0", "macos", "Anna");
        // Two profiles with one id: nothing repairs that.
        let p = before.profiles[0].clone();
        file.sections
            .insert("profiles".into(), json!([p.clone(), p]));
        file.sections
            .insert("base_prompt".into(), json!("other prompt"));
        let got = b.settings_import(&file, &all()).await;
        assert!(got.is_err());
        assert_eq!(b.config().current(), before);
        assert_eq!(b.config().base_prompt().unwrap(), "my prompt");
    }

    #[tokio::test]
    async fn copies_are_made_before_the_change_and_can_be_put_back() {
        let tmp = tempfile::tempdir().unwrap();
        let a = changed(&tmp.path().join("a")).await;
        let file = file_of(&a, &all());
        let b = engine(&tmp.path().join("b")).await;
        let before = b.config().current();
        let prompt = b.config().base_prompt().unwrap();
        let outcome = b.settings_import(&file, &all()).await.unwrap();
        assert_eq!(outcome.copies.len(), 2);
        let kept: Config =
            serde_json::from_slice(&std::fs::read(&outcome.copies[0].1).unwrap()).unwrap();
        assert_eq!(kept, *before);

        b.config().restore_copies(&outcome.copies).unwrap();
        assert_eq!(b.config().current(), before);
        assert_eq!(b.config().base_prompt().unwrap(), prompt);
    }

    #[tokio::test]
    async fn sections_of_a_later_version_are_named() {
        let tmp = tempfile::tempdir().unwrap();
        let b = engine(&tmp.path().join("b")).await;
        let mut file = SettingsFile::new("0.1.0", "macos", "Anna");
        file.sections.insert("gestures".into(), json!({}));
        file.sections.insert("recording".into(), json!({}));
        let outcome = b.settings_import(&file, &all()).await.unwrap();
        assert_eq!(
            outcome.notes,
            vec![Note::UnknownSection {
                name: "gestures".into()
            }]
        );
        assert!(outcome.applied.is_empty());
    }
}
