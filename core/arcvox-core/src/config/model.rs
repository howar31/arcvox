//! Config data model. Serialized as `config.json`.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use url::Url;

use crate::types::{ProfileId, ProviderId};

pub const SCHEMA_VERSION: u32 = 4;

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
    /// The engines a profile uses when it names none of its own.
    #[serde(default)]
    pub default_engines: DefaultEngines,
    /// Suggested terms the user turned down, never suggested again.
    #[serde(default)]
    pub dictionary_dismissed: Vec<Dismissed>,
}

/// The one pair of engines every profile falls back on: the one used when
/// a profile follows the default, and the one used when the engine a
/// profile names is gone or cannot run. The factory value is the
/// recommended local pair.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct DefaultEngines {
    pub asr: ProviderId,
    pub llm: ProviderId,
}

impl Default for DefaultEngines {
    fn default() -> Self {
        Self {
            asr: ProviderId(MODEL_ID_DEFAULT_ASR.into()),
            llm: ProviderId(MODEL_ID_DEFAULT_LLM.into()),
        }
    }
}

impl DefaultEngines {
    pub fn for_role(&self, role: EngineRole) -> &ProviderId {
        match role {
            EngineRole::Asr => &self.asr,
            EngineRole::Llm => &self.llm,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EngineRole {
    Asr,
    Llm,
}

/// Where the engine a profile runs with came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "source", rename_all = "snake_case")]
pub enum EngineSource {
    /// The profile names it and it can run.
    Explicit,
    /// The profile names none; the default is used.
    Default,
    /// The profile names `wanted`, which is gone or cannot run; the
    /// default is used until it is back.
    Fallback { wanted: ProviderId },
}

/// The engine a profile runs with for one role, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EngineChoice {
    pub id: ProviderId,
    #[serde(flatten)]
    pub source: EngineSource,
}

/// The engines a profile runs with now. `llm` is None when the profile
/// has no rewrite step, `translate` None when it has no translation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedEngines {
    pub asr: EngineChoice,
    pub llm: Option<EngineChoice>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub translate: Option<EngineChoice>,
}

impl ResolvedEngines {
    /// The ids in use, the rewrite's and the translation's only when there is one.
    pub fn ids(&self) -> impl Iterator<Item = &ProviderId> {
        std::iter::once(&self.asr.id)
            .chain(self.llm.as_ref().map(|c| &c.id))
            .chain(self.translate.as_ref().map(|c| &c.id))
    }

    /// Whether any role runs on the default instead of a named engine.
    pub fn any_fallback(&self) -> bool {
        let fell = |c: &Option<EngineChoice>| {
            c.as_ref()
                .is_some_and(|c| matches!(c.source, EngineSource::Fallback { .. }))
        };
        matches!(self.asr.source, EngineSource::Fallback { .. })
            || fell(&self.llm)
            || fell(&self.translate)
    }
}

/// Platform capabilities the client reports at engine start; they choose the
/// providers a fresh config starts with.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FirstRunDefaults {
    pub apple_dictation: bool,
    pub apple_foundation_models: bool,
}

impl FirstRunDefaults {
    /// The platform services that are there, as native ids.
    pub fn native_ids(&self) -> Vec<&'static str> {
        let mut ids = Vec::new();
        if self.apple_dictation {
            ids.push(NATIVE_APPLE_DICTATION);
        }
        if self.apple_foundation_models {
            ids.push(NATIVE_APPLE_FM);
        }
        ids
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct General {
    /// Whether a dictation leaves a history row at all. Off, nothing of a
    /// dictation is kept: no row, so no recording, stages or reruns either;
    /// the statistics still count it when they are kept.
    pub keep_history: bool,
    pub history_retention_days: u32,
    /// Whether what each stage of a dictation did is written.
    pub keep_stages: bool,
    /// How long what each stage of a dictation did is kept; the row itself
    /// follows `history_retention_days`. 0 keeps it as long as the row.
    pub stage_retention_days: u32,
    pub keep_recordings: bool,
    pub recording_retention_days: u32,
    /// Whether dictations are counted into the hourly statistics.
    pub keep_stats: bool,
    pub context_limit_bytes: usize,
    pub asr_timeout_secs: u64,
    pub llm_timeout_secs: u64,
    pub llm_temperature: f32,
    /// The least room the rewrite may take; a long transcript raises it.
    /// `None` leaves the length to the model.
    pub llm_max_tokens: Option<u32>,
    pub llm_idle_unload_secs: u64,
    /// Load the profile's language model in the background as soon as a
    /// recording starts, so the load overlaps the speech instead of being
    /// waited for after it; a cancelled recording then loaded it for
    /// nothing.
    pub llm_preload_on_record: bool,
    /// How many more times a request to an engine over the network is made
    /// after a connection error, a server error or a timeout.
    pub retry_count: u32,
    /// Characters a minute the saved-time estimate assumes for typing.
    pub typing_chars_per_min: u32,
    /// The folder recordings are kept in; the default one when unset.
    pub recordings_dir: Option<std::path::PathBuf>,
    /// The folder model files are kept in; the default one when unset.
    pub models_dir: Option<std::path::PathBuf>,
}

impl General {
    /// Recordings are files of history rows: without rows there is nothing
    /// to hang them on, so they are kept only while the history is.
    pub fn keeps_recordings(&self) -> bool {
        self.keep_history && self.keep_recordings
    }

    /// What a dictation's row insert writes.
    pub fn keep(&self) -> crate::history::Keep {
        crate::history::Keep {
            row: self.keep_history,
            stages: self.keep_stages,
            stats: self.keep_stats,
        }
    }
}

impl Default for General {
    fn default() -> Self {
        Self {
            keep_history: true,
            history_retention_days: 365,
            keep_stages: true,
            stage_retention_days: 90,
            keep_recordings: false,
            recording_retention_days: 30,
            keep_stats: true,
            context_limit_bytes: 4096,
            asr_timeout_secs: 30,
            llm_timeout_secs: 20,
            llm_temperature: 0.2,
            llm_max_tokens: Some(1024),
            llm_idle_unload_secs: 300,
            llm_preload_on_record: true,
            retry_count: 1,
            typing_chars_per_min: crate::stats::TYPING_CHARS_PER_MIN,
            recordings_dir: None,
            models_dir: None,
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
        /// OpenCC config name, for example `s2tw` (characters only) or
        /// `s2twp` (characters and Taiwan phrasing).
        config: String,
        /// Quotation marks follow the script too (Simplified and
        /// Traditional punctuation differ); ignored by the regional configs.
        #[serde(default)]
        punctuation: bool,
    },
    /// The rewrite module. A module that is off keeps its settings and
    /// does not run; the pages show it as not enabled.
    Llm {
        #[serde(default, skip_serializing_if = "no")]
        off: bool,
    },
    /// The fixed rules at the end: the dictionary's aliases and the text
    /// rules. A step written before the rules existed reads as dictionary
    /// on, the trailing stop kept, the other rules on.
    Rules {
        #[serde(default, skip_serializing_if = "no")]
        off: bool,
        #[serde(default = "yes")]
        dictionary: bool,
        #[serde(default)]
        trailing_stop: bool,
        #[serde(default = "yes")]
        question_mark: bool,
        #[serde(default = "yes")]
        question_mark_inside: bool,
        #[serde(default = "yes")]
        spacing: bool,
        #[serde(default = "yes")]
        punctuation_width: bool,
    },
    /// The translation module: the tidied text is turned into `target`, a
    /// language or a style as the user wrote it. `prompt` is the profile's
    /// own instruction text, None for the built-in one; `provider` names
    /// the engine, None follows the rewrite engine (the default rewrite
    /// engine when the profile has no rewrite).
    Translate {
        #[serde(default, skip_serializing_if = "no")]
        off: bool,
        target: String,
        /// The profile's own writing for the translation, after the rules.
        #[serde(default, skip_serializing_if = "String::is_empty")]
        addition: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider: Option<ProviderId>,
    },
}

fn no(b: &bool) -> bool {
    !*b
}

fn yes() -> bool {
    true
}

impl StepSpec {
    /// Whether this is a module the profile has turned off: it keeps its
    /// settings and does not run.
    pub fn is_off(&self) -> bool {
        matches!(
            self,
            StepSpec::Llm { off: true }
                | StepSpec::Rules { off: true, .. }
                | StepSpec::Translate { off: true, .. }
        )
    }

    /// The rules step as the shipped profile has it: everything on.
    pub fn rules_default() -> StepSpec {
        StepSpec::Rules {
            off: false,
            dictionary: true,
            trailing_stop: true,
            question_mark: true,
            question_mark_inside: true,
            spacing: true,
            punctuation_width: true,
        }
    }
}

/// The preset that is a copy of the factory profile: the everyday style.
pub const FACTORY_PRESET: &str = "casual";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub id: ProfileId,
    pub name: String,
    #[serde(default)]
    pub is_default: bool,
    #[serde(default)]
    pub matcher: Matcher,
    /// The speech engine the profile names; None follows the default.
    #[serde(default)]
    pub asr_provider: Option<ProviderId>,
    /// The rewrite engine the profile names; None follows the default.
    /// Whether a rewrite runs at all is the steps' business.
    #[serde(default)]
    pub llm_provider: Option<ProviderId>,
    #[serde(default)]
    pub language_hint: Option<String>,
    #[serde(default)]
    pub prompt_addition: String,
    #[serde(default)]
    pub few_shot: Vec<Example>,
    pub steps: Vec<StepSpec>,
    /// The built-in preset the profile was made from, which a reset returns
    /// it to. None for profiles from before presets were recorded.
    #[serde(default)]
    pub preset: Option<String>,
    /// The revision of the template's writing style the profile last took
    /// (`ProfilePreset::revision`). Below the template's current revision
    /// the template has moved on since; 0 for a profile written before
    /// revisions were recorded.
    #[serde(default)]
    pub preset_revision: u32,
    /// The dictionary's hinted terms are given to the recognition engine.
    #[serde(default = "default_true")]
    pub asr_dictionary_hint: bool,
    /// The dictionary's terms are listed in the rewrite prompt.
    #[serde(default = "default_true")]
    pub llm_dictionary_terms: bool,
    /// The front window's title is given to the rewrite model as background.
    #[serde(default = "default_true")]
    pub llm_window_title: bool,
    /// A rewrite much shorter than its input is refused and the input kept.
    #[serde(default = "default_true")]
    pub llm_short_fallback: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DictionaryEntry {
    pub term: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default = "default_true")]
    pub asr_hint: bool,
    /// Where the entry came from; None for entries from before this was kept.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<EntryOrigin>,
    /// What the term was heard as when it was suggested.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub heard: Vec<String>,
    /// Unix seconds when the entry was added and last changed; None for
    /// entries from before this was kept.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub added: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated: Option<i64>,
}

impl Default for DictionaryEntry {
    fn default() -> Self {
        Self {
            term: String::new(),
            aliases: Vec::new(),
            asr_hint: true,
            origin: None,
            heard: Vec::new(),
            added: None,
            updated: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryOrigin {
    /// Typed in by the user.
    Manual,
    /// Taken from the Dictionary page's suggestions.
    Suggestion,
}

/// A suggestion the user turned down: it is not shown again.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Dismissed {
    pub term: String,
    /// What the term was heard as when it was turned down.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub heard: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<i64>,
}

// Earlier files hold the dismissed terms as plain strings.
impl<'de> Deserialize<'de> for Dismissed {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Full {
            term: String,
            #[serde(default)]
            heard: Vec<String>,
            #[serde(default)]
            at: Option<i64>,
        }
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Either {
            Term(String),
            Full(Full),
        }
        Ok(match Either::deserialize(d)? {
            Either::Term(term) => Dismissed {
                term,
                heard: Vec::new(),
                at: None,
            },
            Either::Full(f) => Dismissed {
                term: f.term,
                heard: f.heard,
                at: f.at,
            },
        })
    }
}

fn default_true() -> bool {
    true
}

impl Profile {
    /// Whether the profile rewrites at all.
    pub fn has_llm_step(&self) -> bool {
        self.steps
            .iter()
            .any(|s| matches!(s, StepSpec::Llm { off: false }))
    }

    /// The translation module's settings, when the profile has one that is
    /// on: the target, the profile's own addition and the engine it names.
    pub fn translate_step(&self) -> Option<(&str, &str, Option<&ProviderId>)> {
        self.steps.iter().find_map(|s| match s {
            StepSpec::Translate {
                off: false,
                target,
                addition,
                provider,
            } => Some((target.as_str(), addition.as_str(), provider.as_ref())),
            _ => None,
        })
    }
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
        let mut providers: Vec<ProviderConfig> = d
            .native_ids()
            .into_iter()
            .filter_map(ProviderConfig::native_engine)
            .collect();
        // The engines of the recommended pair exist before the models are
        // downloaded, so the default pair is complete from the start; the
        // catalogue gives them their names at the first sync.
        providers.push(ProviderConfig::factory_engine(EngineRole::Asr));
        providers.push(ProviderConfig::factory_engine(EngineRole::Llm));
        let profile = Self::factory_profile();
        Self {
            schema_version: SCHEMA_VERSION,
            general: General::default(),
            providers,
            profiles: vec![profile],
            dictionary: Vec::new(),
            default_engines: DefaultEngines::default(),
            dictionary_dismissed: Vec::new(),
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
        c.default_engines = DefaultEngines {
            asr: ProviderId("local-speaches".into()),
            llm: ProviderId("local-ollama".into()),
        };
        c
    }

    /// The profile a new installation starts with: a copy of the factory
    /// template, naming the engines that template was tuned with.
    pub fn factory_profile() -> Profile {
        let casual = crate::profile::preset(FACTORY_PRESET).expect("the factory preset exists");
        let mut p = Profile::from_preset(casual, "Default".into(), &[]);
        p.id = ProfileId("default".into());
        p.is_default = true;
        p
    }

    pub fn default_steps() -> Vec<StepSpec> {
        vec![
            StepSpec::Asr,
            StepSpec::OpenCc {
                config: "s2tw".into(),
                punctuation: false,
            },
            StepSpec::Llm { off: false },
            StepSpec::OpenCc {
                config: "s2tw".into(),
                punctuation: false,
            },
            StepSpec::rules_default(),
        ]
    }

    pub fn provider(&self, id: &ProviderId) -> Option<&ProviderConfig> {
        self.providers.iter().find(|p| &p.id == id)
    }

    /// The engines a profile runs with now: the one it names when that one
    /// is there and `usable` says it can run, else the default. The
    /// profile's own value is never changed by this; the named engine is
    /// used again as soon as it is back.
    pub fn resolve_engines(
        &self,
        profile: &Profile,
        usable: impl Fn(&ProviderConfig) -> bool,
    ) -> ResolvedEngines {
        let choose = |named: &Option<ProviderId>, role: EngineRole| {
            let fallback = self.default_engines.for_role(role).clone();
            match named {
                None => EngineChoice {
                    id: fallback,
                    source: EngineSource::Default,
                },
                Some(id) => match self.provider(id) {
                    Some(cfg) if usable(cfg) => EngineChoice {
                        id: id.clone(),
                        source: EngineSource::Explicit,
                    },
                    _ => EngineChoice {
                        id: fallback,
                        source: EngineSource::Fallback { wanted: id.clone() },
                    },
                },
            }
        };
        let llm = profile
            .has_llm_step()
            .then(|| choose(&profile.llm_provider, EngineRole::Llm));
        // The translation follows the rewrite engine unless it names one;
        // without a rewrite it follows the default rewrite engine.
        let translate = profile.translate_step().map(|(_, _, named)| match named {
            Some(id) => choose(&Some(id.clone()), EngineRole::Llm),
            None => match &llm {
                Some(c) => EngineChoice {
                    id: c.id.clone(),
                    source: EngineSource::Default,
                },
                None => choose(&None, EngineRole::Llm),
            },
        });
        ResolvedEngines {
            asr: choose(&profile.asr_provider, EngineRole::Asr),
            llm,
            translate,
        }
    }

    /// Every engine id the profiles and the default pair name, whether or
    /// not such an engine exists.
    pub fn referenced_engine_ids(&self) -> std::collections::HashSet<ProviderId> {
        let mut ids: std::collections::HashSet<ProviderId> = self
            .profiles
            .iter()
            .flat_map(|p| p.asr_provider.iter().chain(p.llm_provider.iter()))
            .cloned()
            .collect();
        ids.insert(self.default_engines.asr.clone());
        ids.insert(self.default_engines.llm.clone());
        ids
    }

    /// Panics only if validation was skipped; `validate` guarantees one default.
    pub fn default_profile(&self) -> &Profile {
        self.profiles
            .iter()
            .find(|p| p.is_default)
            .expect("validated config has exactly one default profile")
    }
}

impl Capabilities {
    /// What an embedded model can do, given the uses its file allows: a
    /// whisper.cpp model takes a language hint and hot words; a llama.cpp
    /// model transcribing takes the hot words as context, and is not told
    /// the language.
    pub fn for_model(runtime: Runtime, transcribe: bool, chat: bool) -> Self {
        Self {
            transcribe,
            chat,
            streaming: false,
            language_hint: transcribe && runtime == Runtime::Whisper,
            hotwords: transcribe,
        }
    }

    /// What an engine of the runtime does by default: a speech model
    /// transcribes with a language hint and hot words, a text model chats.
    pub fn for_runtime(runtime: Runtime) -> Self {
        let speech = runtime == Runtime::Whisper;
        Self::for_model(runtime, speech, !speech)
    }
}

impl ProviderConfig {
    /// An embedded engine that loads the catalogue model `model_id`.
    /// The engine of the factory default pair for the role, as it is before
    /// the catalogue names it.
    pub fn factory_engine(role: EngineRole) -> Self {
        match role {
            EngineRole::Asr => Self::embedded(
                MODEL_ID_DEFAULT_ASR,
                "Local speech model",
                Runtime::Whisper,
                MODEL_ID_DEFAULT_ASR,
            ),
            EngineRole::Llm => Self::embedded(
                MODEL_ID_DEFAULT_LLM,
                "Local language model",
                Runtime::Llama,
                MODEL_ID_DEFAULT_LLM,
            ),
        }
    }

    pub fn embedded(id: &str, label: &str, runtime: Runtime, model_id: &str) -> Self {
        Self {
            id: ProviderId(id.into()),
            label: label.into(),
            kind: ProviderKind::Embedded,
            capabilities: Capabilities::for_runtime(runtime),
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

    /// The engine of a platform service, as the app makes it: a fixed id,
    /// an English name (the service's own) and the service's capabilities.
    /// None for a service this build does not know.
    pub fn native_engine(native_id: &str) -> Option<Self> {
        let (id, label, runtime) = match native_id {
            NATIVE_APPLE_DICTATION => ("apple-dictation", "Apple Dictation", Runtime::Whisper),
            NATIVE_APPLE_FM => ("apple-fm", "Apple Foundation Models", Runtime::Llama),
            _ => return None,
        };
        Some(Self::native(
            id,
            label,
            native_id,
            Capabilities::for_runtime(runtime),
        ))
    }

    /// The engine of a platform service by the fixed id the app gives it.
    pub fn native_engine_by_id(id: &str) -> Option<Self> {
        [NATIVE_APPLE_DICTATION, NATIVE_APPLE_FM]
            .into_iter()
            .filter_map(Self::native_engine)
            .find(|made| made.id.0 == id)
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
    fn profile_switches_default_to_on_when_the_file_has_none() {
        let json = r#"{"id":"p","name":"P","asr_provider":"a","steps":[{"step":"asr"}]}"#;
        let p: Profile = serde_json::from_str(json).unwrap();
        assert!(p.asr_dictionary_hint);
        assert!(p.llm_dictionary_terms);
        assert!(p.llm_window_title);
        assert!(p.llm_short_fallback);
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
        assert!(General::default().llm_preload_on_record);
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
            serde_json::json!({"step":"opencc","config":"s2tw","punctuation":false})
        );
        assert_eq!(json[2], serde_json::json!({"step":"llm"}));
        assert_eq!(
            json[4],
            serde_json::json!({"step":"rules","dictionary":true,"trailing_stop":true,"question_mark":true,"question_mark_inside":true,"spacing":true,"punctuation_width":true})
        );
    }

    #[test]
    fn a_rules_step_from_before_the_text_rules_keeps_the_trailing_stop() {
        let step: StepSpec = serde_json::from_value(serde_json::json!({"step":"rules"})).unwrap();
        assert_eq!(
            step,
            StepSpec::Rules {
                off: false,
                dictionary: true,
                trailing_stop: false,
                question_mark: true,
                question_mark_inside: true,
                spacing: true,
                punctuation_width: true,
            }
        );
        // A module that is off keeps its settings and is written with the flag only when set.
        let off: StepSpec =
            serde_json::from_value(serde_json::json!({"step":"llm","off":true})).unwrap();
        assert!(off.is_off());
        assert_eq!(
            serde_json::to_value(&off).unwrap(),
            serde_json::json!({"step":"llm","off":true})
        );
        assert_eq!(
            serde_json::to_value(StepSpec::Llm { off: false }).unwrap(),
            serde_json::json!({"step":"llm"})
        );
    }

    #[test]
    fn unknown_step_name_fails_to_parse() {
        let err = serde_json::from_str::<StepSpec>(r#"{"step":"summarise"}"#).unwrap_err();
        assert!(err.to_string().contains("summarise"), "{err}");
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
    fn engines_resolve_to_the_named_one_when_it_can_run_else_the_default() {
        let mut c = Config::http_test_config();
        let far = ProviderConfig::http_test_provider("far", "http://localhost:9/v1", true, true);
        c.providers.push(far);
        let mut p = c.profiles[0].clone();
        // A profile that follows the default.
        p.asr_provider = None;
        p.llm_provider = None;
        let all = |_: &ProviderConfig| true;

        let r = c.resolve_engines(&p, all);
        assert_eq!(r.asr.id.0, "local-speaches");
        assert_eq!(r.asr.source, EngineSource::Default);
        assert_eq!(r.llm.as_ref().unwrap().id.0, "local-ollama");
        assert_eq!(r.llm.as_ref().unwrap().source, EngineSource::Default);

        p.asr_provider = Some(ProviderId("far".into()));
        p.llm_provider = Some(ProviderId("far".into()));
        let r = c.resolve_engines(&p, all);
        assert_eq!(r.asr.id.0, "far");
        assert_eq!(r.asr.source, EngineSource::Explicit);
        assert_eq!(r.llm.as_ref().unwrap().source, EngineSource::Explicit);
        assert!(!r.any_fallback());

        // Named but cannot run: the default, and the name is kept.
        let r = c.resolve_engines(&p, |cfg| cfg.id.0 != "far");
        assert_eq!(r.asr.id.0, "local-speaches");
        assert_eq!(
            r.asr.source,
            EngineSource::Fallback {
                wanted: ProviderId("far".into())
            }
        );
        assert!(r.any_fallback());
        assert_eq!(p.asr_provider, Some(ProviderId("far".into())));

        // Named but gone.
        p.llm_provider = Some(ProviderId("nowhere".into()));
        let r = c.resolve_engines(&p, all);
        assert_eq!(r.llm.as_ref().unwrap().id.0, "local-ollama");
        assert!(matches!(
            r.llm.as_ref().unwrap().source,
            EngineSource::Fallback { .. }
        ));

        // No rewrite step: no rewrite engine, whatever is named.
        p.steps.retain(|s| !matches!(s, StepSpec::Llm { .. }));
        let r = c.resolve_engines(&p, all);
        assert_eq!(r.llm, None);
        assert_eq!(r.ids().count(), 1);
        // A rewrite that is off counts as none.
        p.steps.push(StepSpec::Llm { off: true });
        assert_eq!(c.resolve_engines(&p, all).llm, None);
    }

    #[test]
    fn referenced_ids_cover_the_profiles_and_the_default_pair() {
        let mut c = Config::http_test_config();
        c.profiles[0].asr_provider = Some(ProviderId("x".into()));
        c.profiles[0].llm_provider = None;
        let ids = c.referenced_engine_ids();
        for id in ["x", "local-speaches", "local-ollama"] {
            assert!(ids.contains(&ProviderId(id.into())), "{id}");
        }
        assert_eq!(ids.len(), 3);
    }

    #[test]
    fn a_config_without_the_pair_or_profile_engines_reads_with_the_factory_pair() {
        let value = serde_json::json!({
            "schema_version": 3, "general": {}, "providers": [], "dictionary": [],
            "profiles": [{ "id": "a", "name": "A", "is_default": true, "steps": [] }]
        });
        let c: Config = serde_json::from_value(value).unwrap();
        assert_eq!(c.default_engines, DefaultEngines::default());
        assert_eq!(c.profiles[0].asr_provider, None);
    }

    #[test]
    fn no_native_picks_embedded_pair() {
        let c = Config::first_run_with(&FirstRunDefaults::default());
        let ids: Vec<&str> = c.providers.iter().map(|p| p.id.0.as_str()).collect();
        assert_eq!(ids, [MODEL_ID_DEFAULT_ASR, MODEL_ID_DEFAULT_LLM]);
        let d = c.profiles.iter().find(|p| p.is_default).unwrap();
        assert_eq!(
            d.asr_provider.as_ref().map(|p| p.0.as_str()),
            Some(MODEL_ID_DEFAULT_ASR)
        );
        assert_eq!(
            d.llm_provider.as_ref().map(|p| p.0.as_str()),
            Some(MODEL_ID_DEFAULT_LLM)
        );
        assert_eq!(c.default_engines.asr.0, MODEL_ID_DEFAULT_ASR);
        assert_eq!(c.default_engines.llm.0, MODEL_ID_DEFAULT_LLM);
        let w = c
            .provider(&ProviderId(MODEL_ID_DEFAULT_ASR.into()))
            .unwrap();
        assert_eq!(w.kind, ProviderKind::Embedded);
        assert_eq!(w.runtime, Some(Runtime::Whisper));
        assert_eq!(w.model_id.as_deref(), Some("breeze-asr-25-q5_0"));
        assert!(
            w.capabilities.transcribe
                && w.capabilities.language_hint
                && w.capabilities.hotwords
                && !w.capabilities.chat
        );
        let l = c
            .provider(&ProviderId(MODEL_ID_DEFAULT_LLM.into()))
            .unwrap();
        assert_eq!(l.model_id.as_deref(), Some("gemma-3-4b-it-q4_k_m"));
        assert!(l.capabilities.chat && !l.capabilities.transcribe);
    }

    #[test]
    fn both_native_creates_apple_providers_but_defaults_to_the_local_pair() {
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
                MODEL_ID_DEFAULT_ASR,
                MODEL_ID_DEFAULT_LLM
            ]
        );
        assert_eq!(c.default_engines, DefaultEngines::default());
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
    fn dictation_only_creates_the_dictation_provider_and_keeps_the_local_pair() {
        let c = Config::first_run_with(&FirstRunDefaults {
            apple_dictation: true,
            apple_foundation_models: false,
        });
        assert_eq!(c.default_engines, DefaultEngines::default());
        assert!(c.provider(&ProviderId("apple-dictation".into())).is_some());
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
