//! Config data model. Serialized as `config.json`.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use url::Url;

use crate::types::{ProfileId, ProviderId};

pub const SCHEMA_VERSION: u32 = 1;

pub const MODEL_ID_DEFAULT_ASR: &str = "breeze-asr-25-q5_0";
pub const MODEL_ID_DEFAULT_LLM: &str = "gemma-3-4b-it-q4_k_m";
pub const NATIVE_APPLE_DICTATION: &str = "apple.dictation";
pub const NATIVE_APPLE_FM: &str = "apple.foundation_models";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub schema_version: u32,
    pub general: General,
    pub providers: Vec<ProviderConfig>,
    pub profiles: Vec<Profile>,
    pub dictionary: Vec<DictionaryEntry>,
}

/// Platform capabilities the client reports at engine start; they choose the
/// providers a fresh config starts with.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FirstRunDefaults {
    pub apple_dictation: bool,
    pub apple_foundation_models: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct General {
    pub history_retention_days: u32,
    pub keep_recordings: bool,
    pub recording_retention_days: u32,
    pub context_limit_bytes: usize,
    pub asr_timeout_secs: u64,
    pub llm_timeout_secs: u64,
    pub llm_temperature: f32,
    pub llm_max_tokens: Option<u32>,
    pub llm_idle_unload_secs: u64,
}

impl Default for General {
    fn default() -> Self {
        Self {
            history_retention_days: 90,
            keep_recordings: false,
            recording_retention_days: 7,
            context_limit_bytes: 4096,
            asr_timeout_secs: 30,
            llm_timeout_secs: 20,
            llm_temperature: 0.2,
            llm_max_tokens: Some(1024),
            llm_idle_unload_secs: 300,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Runtime {
    Whisper,
    Llama,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    Http,
    Embedded,
    Native,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Capabilities {
    pub transcribe: bool,
    pub chat: bool,
    pub streaming: bool,
    pub language_hint: bool,
    pub hotwords: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderConfig {
    pub id: ProviderId,
    pub label: String,
    pub kind: ProviderKind,
    #[serde(default)]
    pub capabilities: Capabilities,
    /// Http: base URL as the user entered it, for example `http://localhost:11434/v1`.
    #[serde(default)]
    pub base_url: Option<Url>,
    #[serde(default)]
    pub model: Option<String>,
    /// Name of the keychain entry holding the API key. Never the key itself.
    #[serde(default)]
    pub api_key_ref: Option<String>,
    #[serde(default)]
    pub extra_headers: BTreeMap<String, String>,
    /// Embedded (phase 2).
    #[serde(default)]
    pub model_path: Option<PathBuf>,
    /// Native (phase 2).
    #[serde(default)]
    pub native_id: Option<String>,
    /// Embedded: id of a catalogue model under the models directory.
    /// `model_path` wins when both are set.
    #[serde(default)]
    pub model_id: Option<String>,
    /// Embedded: which engine loads the file.
    #[serde(default)]
    pub runtime: Option<Runtime>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Matcher {
    /// Exact match against `AppInfo::app_id`.
    pub app_ids: Vec<String>,
    /// Case-insensitive substring match against `AppInfo::url`.
    pub url_patterns: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Example {
    pub raw: String,
    pub refined: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "step", rename_all = "snake_case", deny_unknown_fields)]
pub enum StepSpec {
    Asr,
    #[serde(rename = "opencc")]
    OpenCc {
        /// OpenCC config name, for example `s2twp`.
        config: String,
    },
    Llm,
    Rules,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub id: ProfileId,
    pub name: String,
    #[serde(default)]
    pub is_default: bool,
    #[serde(default)]
    pub matcher: Matcher,
    pub asr_provider: ProviderId,
    #[serde(default)]
    pub llm_provider: Option<ProviderId>,
    #[serde(default)]
    pub language_hint: Option<String>,
    #[serde(default)]
    pub prompt_addition: String,
    #[serde(default)]
    pub few_shot: Vec<Example>,
    pub steps: Vec<StepSpec>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DictionaryEntry {
    pub term: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default = "default_true")]
    pub asr_hint: bool,
}

fn default_true() -> bool {
    true
}

impl Config {
    /// The config written on first launch without any native provider.
    pub fn first_run() -> Self {
        Self::first_run_with(&FirstRunDefaults::default())
    }

    /// The config written on first launch: the embedded providers, plus the
    /// native ones the platform reports, and one default profile that prefers
    /// the native providers.
    pub fn first_run_with(d: &FirstRunDefaults) -> Self {
        let mut providers = Vec::new();
        if d.apple_dictation {
            providers.push(ProviderConfig::native(
                "apple-dictation",
                "Apple Dictation",
                NATIVE_APPLE_DICTATION,
                Capabilities {
                    transcribe: true,
                    chat: false,
                    streaming: false,
                    language_hint: true,
                    hotwords: true,
                },
            ));
        }
        if d.apple_foundation_models {
            providers.push(ProviderConfig::native(
                "apple-fm",
                "Apple Foundation Models",
                NATIVE_APPLE_FM,
                Capabilities {
                    transcribe: false,
                    chat: true,
                    streaming: false,
                    language_hint: false,
                    hotwords: false,
                },
            ));
        }
        providers.push(ProviderConfig::embedded(
            "local-whisper",
            "Local speech model",
            Runtime::Whisper,
            MODEL_ID_DEFAULT_ASR,
            Capabilities {
                transcribe: true,
                chat: false,
                streaming: false,
                language_hint: true,
                hotwords: true,
            },
        ));
        providers.push(ProviderConfig::embedded(
            "local-llama",
            "Local language model",
            Runtime::Llama,
            MODEL_ID_DEFAULT_LLM,
            Capabilities {
                transcribe: false,
                chat: true,
                streaming: false,
                language_hint: false,
                hotwords: false,
            },
        ));
        let asr = if d.apple_dictation {
            "apple-dictation"
        } else {
            "local-whisper"
        };
        let llm = if d.apple_foundation_models {
            "apple-fm"
        } else {
            "local-llama"
        };
        let profile = Profile {
            id: ProfileId("default".into()),
            name: "Default".into(),
            is_default: true,
            matcher: Matcher::default(),
            asr_provider: ProviderId(asr.into()),
            llm_provider: Some(ProviderId(llm.into())),
            language_hint: Some("zh-TW".into()),
            prompt_addition: String::new(),
            few_shot: Vec::new(),
            steps: Self::default_steps(),
        };
        Self {
            schema_version: SCHEMA_VERSION,
            general: General::default(),
            providers,
            profiles: vec![profile],
            dictionary: Vec::new(),
        }
    }

    /// A first-run config whose providers are two HTTP endpoints
    /// (`local-speaches` for ASR, `local-ollama` for chat) that the default
    /// profile uses.
    #[cfg(any(test, feature = "test-util"))]
    pub fn http_test_config() -> Self {
        let mut c = Self::first_run();
        c.providers = vec![
            ProviderConfig::http_test_provider(
                "local-speaches",
                "http://localhost:8000/v1",
                true,
                false,
            ),
            ProviderConfig::http_test_provider(
                "local-ollama",
                "http://localhost:11434/v1",
                false,
                true,
            ),
        ];
        c.profiles[0].asr_provider = ProviderId("local-speaches".into());
        c.profiles[0].llm_provider = Some(ProviderId("local-ollama".into()));
        c
    }

    pub fn default_steps() -> Vec<StepSpec> {
        vec![
            StepSpec::Asr,
            StepSpec::OpenCc {
                config: "s2twp".into(),
            },
            StepSpec::Llm,
            StepSpec::OpenCc {
                config: "s2twp".into(),
            },
            StepSpec::Rules,
        ]
    }

    pub fn provider(&self, id: &ProviderId) -> Option<&ProviderConfig> {
        self.providers.iter().find(|p| &p.id == id)
    }

    /// Panics only if validation was skipped; `validate` guarantees one default.
    pub fn default_profile(&self) -> &Profile {
        self.profiles
            .iter()
            .find(|p| p.is_default)
            .expect("validated config has exactly one default profile")
    }
}

impl ProviderConfig {
    pub fn embedded(
        id: &str,
        label: &str,
        runtime: Runtime,
        model_id: &str,
        capabilities: Capabilities,
    ) -> Self {
        Self {
            id: ProviderId(id.into()),
            label: label.into(),
            kind: ProviderKind::Embedded,
            capabilities,
            base_url: None,
            model: None,
            api_key_ref: None,
            extra_headers: BTreeMap::new(),
            model_path: None,
            native_id: None,
            model_id: Some(model_id.into()),
            runtime: Some(runtime),
        }
    }

    pub fn native(id: &str, label: &str, native_id: &str, capabilities: Capabilities) -> Self {
        Self {
            id: ProviderId(id.into()),
            label: label.into(),
            kind: ProviderKind::Native,
            capabilities,
            base_url: None,
            model: None,
            api_key_ref: None,
            extra_headers: BTreeMap::new(),
            model_path: None,
            native_id: Some(native_id.into()),
            model_id: None,
            runtime: None,
        }
    }

    #[cfg(any(test, feature = "test-util"))]
    pub fn http_test_provider(id: &str, base: &str, transcribe: bool, chat: bool) -> Self {
        Self {
            id: ProviderId(id.into()),
            label: id.into(),
            kind: ProviderKind::Http,
            capabilities: Capabilities {
                transcribe,
                chat,
                streaming: false,
                language_hint: transcribe,
                hotwords: transcribe,
            },
            base_url: Some(Url::parse(base).unwrap()),
            model: Some("model".into()),
            api_key_ref: None,
            extra_headers: BTreeMap::new(),
            model_path: None,
            native_id: None,
            model_id: None,
            runtime: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_config_defaults_new_fields() {
        let json = r#"{"id":"x","label":"X","kind":"http","base_url":"http://localhost:1/v1","model":"m"}"#;
        let p: ProviderConfig = serde_json::from_str(json).unwrap();
        assert_eq!(p.model_id, None);
        assert_eq!(p.runtime, None);
    }

    #[test]
    fn runtime_serializes_lowercase() {
        assert_eq!(
            serde_json::to_string(&Runtime::Whisper).unwrap(),
            "\"whisper\""
        );
        assert_eq!(serde_json::to_string(&Runtime::Llama).unwrap(), "\"llama\"");
    }

    #[test]
    fn general_defaults_idle_unload() {
        assert_eq!(General::default().llm_idle_unload_secs, 300);
        let g: General = serde_json::from_str("{}").unwrap();
        assert_eq!(g.llm_idle_unload_secs, 300);
    }

    #[test]
    fn step_spec_json_shape() {
        let steps = Config::default_steps();
        let json = serde_json::to_value(&steps).unwrap();
        assert_eq!(json[0], serde_json::json!({"step":"asr"}));
        assert_eq!(
            json[1],
            serde_json::json!({"step":"opencc","config":"s2twp"})
        );
        assert_eq!(json[2], serde_json::json!({"step":"llm"}));
        assert_eq!(json[4], serde_json::json!({"step":"rules"}));
    }

    #[test]
    fn unknown_step_name_fails_to_parse() {
        let err = serde_json::from_str::<StepSpec>(r#"{"step":"translate"}"#).unwrap_err();
        assert!(err.to_string().contains("translate"), "{err}");
    }

    #[test]
    fn unknown_top_level_field_is_rejected() {
        let mut v = serde_json::to_value(Config::first_run()).unwrap();
        v["bogus"] = serde_json::json!(1);
        assert!(serde_json::from_value::<Config>(v).is_err());
    }

    #[test]
    fn config_round_trips() {
        let c = Config::first_run();
        let s = serde_json::to_string_pretty(&c).unwrap();
        let back: Config = serde_json::from_str(&s).unwrap();
        assert_eq!(back, c);
        assert!(!s.contains("api_key\""), "keys never appear in config");
    }

    #[test]
    fn dictionary_asr_hint_defaults_true() {
        let e: DictionaryEntry = serde_json::from_str(r#"{"term":"Arcvox"}"#).unwrap();
        assert!(e.asr_hint);
        assert!(e.aliases.is_empty());
    }

    #[test]
    fn no_native_picks_embedded_pair() {
        let c = Config::first_run_with(&FirstRunDefaults::default());
        let ids: Vec<&str> = c.providers.iter().map(|p| p.id.0.as_str()).collect();
        assert_eq!(ids, ["local-whisper", "local-llama"]);
        let d = c.profiles.iter().find(|p| p.is_default).unwrap();
        assert_eq!(d.asr_provider.0, "local-whisper");
        assert_eq!(d.llm_provider.as_ref().unwrap().0, "local-llama");
        let w = c.provider(&ProviderId("local-whisper".into())).unwrap();
        assert_eq!(w.kind, ProviderKind::Embedded);
        assert_eq!(w.runtime, Some(Runtime::Whisper));
        assert_eq!(w.model_id.as_deref(), Some("breeze-asr-25-q5_0"));
        assert!(
            w.capabilities.transcribe
                && w.capabilities.language_hint
                && w.capabilities.hotwords
                && !w.capabilities.chat
        );
        let l = c.provider(&ProviderId("local-llama".into())).unwrap();
        assert_eq!(l.model_id.as_deref(), Some("gemma-3-4b-it-q4_k_m"));
        assert!(l.capabilities.chat && !l.capabilities.transcribe);
    }

    #[test]
    fn both_native_picks_apple_pair_and_keeps_embedded() {
        let c = Config::first_run_with(&FirstRunDefaults {
            apple_dictation: true,
            apple_foundation_models: true,
        });
        let ids: Vec<&str> = c.providers.iter().map(|p| p.id.0.as_str()).collect();
        assert_eq!(
            ids,
            [
                "apple-dictation",
                "apple-fm",
                "local-whisper",
                "local-llama"
            ]
        );
        let d = c.profiles.iter().find(|p| p.is_default).unwrap();
        assert_eq!(d.asr_provider.0, "apple-dictation");
        assert_eq!(d.llm_provider.as_ref().unwrap().0, "apple-fm");
        let a = c.provider(&ProviderId("apple-dictation".into())).unwrap();
        assert_eq!(a.kind, ProviderKind::Native);
        assert_eq!(a.native_id.as_deref(), Some("apple.dictation"));
        assert_eq!(
            c.provider(&ProviderId("apple-fm".into()))
                .unwrap()
                .native_id
                .as_deref(),
            Some("apple.foundation_models")
        );
    }

    #[test]
    fn dictation_only_mixes_apple_asr_with_embedded_llm() {
        let c = Config::first_run_with(&FirstRunDefaults {
            apple_dictation: true,
            apple_foundation_models: false,
        });
        let d = c.profiles.iter().find(|p| p.is_default).unwrap();
        assert_eq!(d.asr_provider.0, "apple-dictation");
        assert_eq!(d.llm_provider.as_ref().unwrap().0, "local-llama");
        assert!(c.provider(&ProviderId("apple-fm".into())).is_none());
    }

    #[test]
    fn every_first_run_variant_validates() {
        for (a, f) in [(false, false), (true, false), (false, true), (true, true)] {
            let c = Config::first_run_with(&FirstRunDefaults {
                apple_dictation: a,
                apple_foundation_models: f,
            });
            crate::config::validate(&c).unwrap();
        }
    }
}
