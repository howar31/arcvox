//! Structural validation of a Config. Runs on every load and every update.

use crate::config::model::{Config, ProviderKind, StepSpec};
use crate::types::{ProfileId, ProviderId};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ConfigError {
    #[error("duplicate id `{0}`")]
    DuplicateId(String),
    #[error("profile `{profile}` references unknown provider `{provider}`")]
    UnknownProvider {
        profile: ProfileId,
        provider: ProviderId,
    },
    #[error("provider `{provider}` cannot be used for {layer} in profile `{profile}`")]
    CapabilityMismatch {
        profile: ProfileId,
        provider: ProviderId,
        layer: &'static str,
    },
    #[error("expected exactly one default profile, found {0}")]
    DefaultProfileCount(usize),
    #[error("profile `{profile}`: {reason}")]
    StepOrder { profile: ProfileId, reason: String },
    #[error("profile `{0}` has an llm step but no llm_provider")]
    MissingLlmProvider(ProfileId),
    #[error("provider `{0}` is embedded but has no runtime or model reference")]
    EmbeddedProviderIncomplete(ProviderId),
    #[error("general.{field} must be at least {min}")]
    GeneralRange { field: &'static str, min: u64 },
    #[error("snippet `{0}` needs at least one alias")]
    SnippetWithoutAlias(String),
}

pub fn validate(config: &Config) -> Result<(), ConfigError> {
    let mut seen = std::collections::HashSet::new();
    for p in &config.providers {
        if !seen.insert(format!("provider:{}", p.id)) {
            return Err(ConfigError::DuplicateId(p.id.0.clone()));
        }
        if p.kind == ProviderKind::Embedded
            && (p.runtime.is_none() || (p.model_id.is_none() && p.model_path.is_none()))
        {
            return Err(ConfigError::EmbeddedProviderIncomplete(p.id.clone()));
        }
    }
    for p in &config.profiles {
        if !seen.insert(format!("profile:{}", p.id)) {
            return Err(ConfigError::DuplicateId(p.id.0.clone()));
        }
    }

    let defaults = config.profiles.iter().filter(|p| p.is_default).count();
    if defaults != 1 {
        return Err(ConfigError::DefaultProfileCount(defaults));
    }

    if config.general.context_limit_bytes < 256 {
        return Err(ConfigError::GeneralRange {
            field: "context_limit_bytes",
            min: 256,
        });
    }
    if config.general.asr_timeout_secs < 1 {
        return Err(ConfigError::GeneralRange {
            field: "asr_timeout_secs",
            min: 1,
        });
    }
    if config.general.typing_chars_per_min < 1 {
        return Err(ConfigError::GeneralRange {
            field: "typing_chars_per_min",
            min: 1,
        });
    }
    if config.general.llm_timeout_secs < 1 {
        return Err(ConfigError::GeneralRange {
            field: "llm_timeout_secs",
            min: 1,
        });
    }

    for e in &config.dictionary {
        if crate::dictionary::is_snippet(e) && !e.aliases.iter().any(|a| !a.trim().is_empty()) {
            let first = e
                .term
                .lines()
                .find(|l| !l.trim().is_empty())
                .unwrap_or("")
                .to_string();
            return Err(ConfigError::SnippetWithoutAlias(first));
        }
    }

    for profile in &config.profiles {
        let asr =
            config
                .provider(&profile.asr_provider)
                .ok_or_else(|| ConfigError::UnknownProvider {
                    profile: profile.id.clone(),
                    provider: profile.asr_provider.clone(),
                })?;
        if !asr.capabilities.transcribe {
            return Err(ConfigError::CapabilityMismatch {
                profile: profile.id.clone(),
                provider: asr.id.clone(),
                layer: "asr",
            });
        }

        let has_llm_step = profile.steps.iter().any(|s| matches!(s, StepSpec::Llm));
        match &profile.llm_provider {
            Some(id) => {
                let llm = config
                    .provider(id)
                    .ok_or_else(|| ConfigError::UnknownProvider {
                        profile: profile.id.clone(),
                        provider: id.clone(),
                    })?;
                if !llm.capabilities.chat {
                    return Err(ConfigError::CapabilityMismatch {
                        profile: profile.id.clone(),
                        provider: llm.id.clone(),
                        layer: "llm",
                    });
                }
            }
            None if has_llm_step => {
                return Err(ConfigError::MissingLlmProvider(profile.id.clone()))
            }
            None => {}
        }

        match profile.steps.first() {
            Some(StepSpec::Asr) => {}
            _ => {
                return Err(ConfigError::StepOrder {
                    profile: profile.id.clone(),
                    reason: "first step must be asr".into(),
                })
            }
        }
        if profile
            .steps
            .iter()
            .filter(|s| matches!(s, StepSpec::Asr))
            .count()
            > 1
        {
            return Err(ConfigError::StepOrder {
                profile: profile.id.clone(),
                reason: "asr may appear only once".into(),
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::model::{
        Capabilities, Config, DictionaryEntry, Profile, ProviderConfig, ProviderKind, Runtime,
    };

    fn base() -> Config {
        Config::http_test_config()
    }

    #[test]
    fn first_run_config_is_valid() {
        assert_eq!(validate(&Config::first_run()), Ok(()));
        assert_eq!(validate(&base()), Ok(()));
    }

    #[test]
    fn rejects_unknown_provider() {
        let mut c = base();
        c.profiles[0].asr_provider = ProviderId("nope".into());
        assert!(matches!(
            validate(&c),
            Err(ConfigError::UnknownProvider { .. })
        ));
    }

    #[test]
    fn rejects_capability_mismatch() {
        let mut c = base();
        let ollama = c
            .providers
            .iter()
            .find(|p| p.id.0 == "local-ollama")
            .unwrap()
            .id
            .clone();
        c.profiles[0].asr_provider = ollama;
        assert!(matches!(
            validate(&c),
            Err(ConfigError::CapabilityMismatch { layer: "asr", .. })
        ));
    }

    #[test]
    fn rejects_zero_or_two_defaults() {
        let mut c = base();
        c.profiles[0].is_default = false;
        assert_eq!(validate(&c), Err(ConfigError::DefaultProfileCount(0)));
        let mut c = base();
        let mut second = c.profiles[0].clone();
        second.id = ProfileId("second".into());
        c.profiles.push(second);
        assert_eq!(validate(&c), Err(ConfigError::DefaultProfileCount(2)));
    }

    #[test]
    fn rejects_llm_step_without_provider() {
        let mut c = base();
        c.profiles[0].llm_provider = None;
        assert!(matches!(
            validate(&c),
            Err(ConfigError::MissingLlmProvider(_))
        ));
    }

    #[test]
    fn allows_profile_without_llm_when_step_absent() {
        let mut c = base();
        c.profiles[0].llm_provider = None;
        c.profiles[0].steps = vec![
            StepSpec::Asr,
            StepSpec::OpenCc {
                config: "s2twp".into(),
            },
        ];
        assert_eq!(validate(&c), Ok(()));
    }

    #[test]
    fn rejects_asr_not_first_or_repeated() {
        let mut c = base();
        c.profiles[0].steps = vec![StepSpec::Llm, StepSpec::Asr];
        assert!(matches!(validate(&c), Err(ConfigError::StepOrder { .. })));
        let mut c = base();
        c.profiles[0].steps = vec![StepSpec::Asr, StepSpec::Asr];
        assert!(matches!(validate(&c), Err(ConfigError::StepOrder { .. })));
    }

    #[test]
    fn rejects_duplicate_ids() {
        let mut c = base();
        let dup = c.providers[0].clone();
        c.providers.push(dup);
        assert!(matches!(validate(&c), Err(ConfigError::DuplicateId(_))));
    }

    #[test]
    fn rejects_tiny_context_limit() {
        let mut c = base();
        c.general.context_limit_bytes = 10;
        assert!(matches!(
            validate(&c),
            Err(ConfigError::GeneralRange {
                field: "context_limit_bytes",
                ..
            })
        ));
    }

    #[test]
    fn embedded_provider_needs_runtime_and_a_model_reference() {
        let mut c = Config::first_run();
        let mut p = c.providers[0].clone();
        p.id = ProviderId("emb".into());
        p.kind = ProviderKind::Embedded;
        p.base_url = None;
        p.runtime = None;
        p.model_id = None;
        p.model_path = None;
        c.providers.push(p);
        assert_eq!(
            validate(&c).unwrap_err(),
            ConfigError::EmbeddedProviderIncomplete(ProviderId("emb".into()))
        );
        c.providers.last_mut().unwrap().runtime = Some(Runtime::Whisper);
        c.providers.last_mut().unwrap().model_id = Some("breeze-asr-25-q5_0".into());
        assert!(validate(&c).is_ok());
    }

    #[test]
    fn snippet_without_alias_is_rejected() {
        let mut c = Config::first_run();
        c.dictionary.push(DictionaryEntry {
            term: "a\nb".into(),
            aliases: vec!["".into()],
            asr_hint: true,
        });
        assert_eq!(
            validate(&c),
            Err(ConfigError::SnippetWithoutAlias("a".into()))
        );
        c.dictionary[0].aliases = vec!["k".into()];
        assert!(validate(&c).is_ok());
        c.dictionary[0] = DictionaryEntry {
            term: "\nA\nB".into(),
            aliases: Vec::new(),
            asr_hint: true,
        };
        assert_eq!(
            validate(&c),
            Err(ConfigError::SnippetWithoutAlias("A".into()))
        );
        c.dictionary[0].term = "Arcvox\n".into();
        assert!(validate(&c).is_ok());
    }

    #[test]
    fn provider_lookup_by_id() {
        let c = base();
        assert!(c.provider(&ProviderId("local-ollama".into())).is_some());
        assert!(c.provider(&ProviderId("missing".into())).is_none());
        let _ = ProviderConfig {
            id: ProviderId("x".into()),
            label: "x".into(),
            kind: ProviderKind::Http,
            capabilities: Capabilities::default(),
            base_url: None,
            model: None,
            api_key_ref: None,
            extra_headers: Default::default(),
            model_path: None,
            native_id: None,
            model_id: None,
            runtime: None,
        };
        let _: &Profile = c.default_profile();
    }
}
