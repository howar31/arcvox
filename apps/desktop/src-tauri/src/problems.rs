//! What is wrong with the profiles and the engines as they are set up: the
//! menu-bar menu and the settings window report the same list.

use std::collections::BTreeMap;

use arcvox_core::checks::EngineCheck;
use arcvox_core::config::{
    Config, EngineRole, EngineSource, ProviderConfig, ProviderKind, NATIVE_APPLE_DICTATION,
};
use arcvox_core::models::ModelRole;
use arcvox_core::Engine;

use crate::ui::tray::Notice;
use crate::ui::TauriUi;

/// One notice for every profile that runs on the default instead of the
/// engine it names, one for every profile with a writing style of its own
/// whose template's style has moved on since, one for each engine of the
/// default pair that cannot run, and one for every engine whose own file is
/// gone. `usable` says whether an engine can run now for a profile in a
/// language (its hint; none for the engine as such).
pub fn notices(
    config: &Config,
    usable: impl Fn(&ProviderConfig, Option<&str>) -> bool,
) -> Vec<Notice> {
    let mut out = Vec::new();
    for profile in &config.profiles {
        let language = profile.language_hint.as_deref();
        let resolved = config.resolve_engines(profile, |c| usable(c, language));
        if !resolved.any_fallback() {
            continue;
        }
        // Apple dictation has a model per language: the profile's may be the one missing.
        let language_missing = matches!(&resolved.asr.source, EngineSource::Fallback { wanted }
            if config.provider(wanted).is_some_and(|c| c.native_id.as_deref() == Some(NATIVE_APPLE_DICTATION)));
        let (name, id) = (profile.name.clone(), profile.id.0.clone());
        out.push(if language_missing {
            Notice::ProfileLanguageMissing(name, id)
        } else {
            Notice::ProfileOnDefault(name, id)
        });
    }
    for profile in &config.profiles {
        if profile.moved_template().is_some() {
            out.push(Notice::ProfileStyleMoved(
                profile.name.clone(),
                profile.id.0.clone(),
            ));
        }
    }
    for (role, id) in [
        (EngineRole::Asr, &config.default_engines.asr),
        (EngineRole::Llm, &config.default_engines.llm),
    ] {
        let language = config.default_profile().language_hint.as_deref();
        if !config.provider(id).is_some_and(|c| usable(c, language)) {
            out.push(match role {
                EngineRole::Asr => Notice::DefaultAsrNotReady,
                EngineRole::Llm => Notice::DefaultLlmNotReady,
            });
        }
    }
    for p in &config.providers {
        if p.model_id.is_none() && p.model_path.is_some() && !usable(p, None) {
            let tab = if p.capabilities.transcribe {
                "asr"
            } else {
                "llm"
            };
            out.push(Notice::ModelFileMissing(p.label.clone(), tab));
        }
    }
    out
}

/// One notice for every network engine whose last test failed and that
/// dictation depends on: a default engine or one a profile names. Such an
/// engine is not swapped for the default.
pub fn check_notices(config: &Config, checks: &BTreeMap<String, EngineCheck>) -> Vec<Notice> {
    let depended = |p: &ProviderConfig| {
        p.id == config.default_engines.asr
            || p.id == config.default_engines.llm
            || config.profiles.iter().any(|f| {
                f.asr_provider.as_ref() == Some(&p.id) || f.llm_provider.as_ref() == Some(&p.id)
            })
    };
    config
        .providers
        .iter()
        .filter(|p| p.kind == ProviderKind::Http && depended(p))
        .filter_map(|p| {
            let check = checks.get(&p.id.0).filter(|c| c.fails(p))?;
            let tab = match check.role {
                ModelRole::Asr => "asr",
                ModelRole::Llm => "llm",
            };
            Some(Notice::EngineTestFailed(p.label.clone(), tab))
        })
        .collect()
}

/// Whether the default profile can dictate now: every engine it resolves to
/// can run.
pub fn default_profile_ready(config: &Config, usable: impl Fn(&ProviderConfig) -> bool) -> bool {
    let resolved = config.resolve_engines(config.default_profile(), &usable);
    let ready = resolved
        .ids()
        .all(|id| config.provider(id).is_some_and(&usable));
    ready
}

/// Reads the profiles and the engines again and reports what is wrong
/// with them.
pub fn refresh(engine: Option<&Engine>, ui: &std::sync::Arc<TauriUi>) {
    // Whether a system engine can run is asked of the system, which cannot
    // answer while the main thread waits for it: from there the check moves
    // to a thread of its own.
    if std::thread::current().name() == Some("main") {
        let (engine, ui) = (engine.cloned(), ui.clone());
        std::thread::spawn(move || refresh(engine.as_ref(), &ui));
        return;
    }
    let found = match engine {
        Some(e) => {
            let config = e.config().current();
            let mut found = notices(&config, |cfg, language| e.can_run_for(cfg, language));
            found.extend(check_notices(&config, &e.engine_checks()));
            found
        }
        None => Vec::new(),
    };
    ui.set_engine_problems(found);
}

#[cfg(test)]
mod tests {
    use super::*;
    use arcvox_core::types::ProviderId;

    fn config() -> Config {
        serde_json::from_value(serde_json::json!({
            "schema_version": 3,
            "general": {},
            "providers": [
                { "id": "remote", "label": "Remote", "kind": "http", "base_url": "http://localhost:1/v1", "capabilities": { "transcribe": true, "chat": true } },
                { "id": "local", "label": "Local", "kind": "embedded", "model_id": "small", "runtime": "whisper", "capabilities": { "transcribe": true } },
                { "id": "own", "label": "Own file", "kind": "embedded", "model_path": "/models/own.bin", "runtime": "whisper", "capabilities": { "transcribe": true } },
            ],
            "profiles": [
                { "id": "a", "name": "Default", "is_default": true, "preset": "blank", "asr_provider": "local", "llm_provider": "remote", "steps": [{ "step": "asr" }, { "step": "llm" }] },
                { "id": "b", "name": "Follower", "preset": "blank", "steps": [{ "step": "asr" }] },
            ],
            "dictionary": [],
            "default_engines": { "asr": "remote", "llm": "remote" }
        }))
        .unwrap()
    }

    /// The engines whose files are there, by id.
    /// The same answer whatever the profile's language.
    fn any_language(
        f: impl Fn(&ProviderConfig) -> bool,
    ) -> impl Fn(&ProviderConfig, Option<&str>) -> bool {
        move |p, _| f(p)
    }

    fn have<'a>(ids: &'a [&'a str]) -> impl Fn(&ProviderConfig) -> bool + 'a {
        move |p| p.model_id.is_none() && p.model_path.is_none() || ids.contains(&p.id.0.as_str())
    }

    #[test]
    fn a_profile_whose_apple_dictation_language_is_missing_says_so() {
        let mut c = config();
        c.providers.push(
            serde_json::from_value(serde_json::json!({ "id": "apple", "label": "Apple", "kind": "native", "native_id": NATIVE_APPLE_DICTATION, "capabilities": { "transcribe": true } }))
                .unwrap(),
        );
        c.profiles[1].asr_provider = Some(ProviderId("apple".into()));
        c.profiles[1].language_hint = Some("ja".into());
        // Japanese is not there; the language of the default profile is.
        let usable = |p: &ProviderConfig, language: Option<&str>| {
            p.id.0 != "apple" || language != Some("ja")
        };
        let found = notices(&c, usable);
        assert!(found.contains(&Notice::ProfileLanguageMissing(
            "Follower".into(),
            "b".into()
        )));
        assert!(!found
            .iter()
            .any(|n| matches!(n, Notice::ProfileOnDefault(..))));
    }

    #[test]
    fn nothing_is_wrong_while_every_file_is_there() {
        assert!(notices(&config(), any_language(have(&["local", "own"]))).is_empty());
    }

    #[test]
    fn a_profile_whose_named_engine_cannot_run_is_on_the_default() {
        assert_eq!(
            notices(&config(), any_language(have(&["own"]))),
            vec![Notice::ProfileOnDefault("Default".into(), "a".into())]
        );
    }

    #[test]
    fn a_profile_whose_engine_is_gone_is_on_the_default_too() {
        let mut c = config();
        c.providers.retain(|p| p.id.0 != "local");
        assert_eq!(
            notices(&c, any_language(have(&["own"]))),
            vec![Notice::ProfileOnDefault("Default".into(), "a".into())]
        );
    }

    #[test]
    fn a_default_engine_that_cannot_run_is_named_by_its_role() {
        let mut c = config();
        c.default_engines.asr = ProviderId("local".into());
        let found = notices(&c, any_language(have(&["own"])));
        assert!(found.contains(&Notice::DefaultAsrNotReady), "{found:?}");
        assert!(!found.contains(&Notice::DefaultLlmNotReady));
        // The follower runs on the default too, but that is the pair's
        // problem, not the profile's.
        assert!(!found.contains(&Notice::ProfileOnDefault("Follower".into(), "b".into())));
    }

    #[test]
    fn a_profile_with_its_own_style_is_named_once_its_template_moved_on() {
        let mut c = config();
        c.profiles[1].preset = Some("casual".into());
        c.profiles[1].preset_revision = 0;
        c.profiles[1].prompt_addition = "我的寫法".into();
        let found = notices(&c, any_language(have(&["local", "own"])));
        assert_eq!(
            found,
            vec![Notice::ProfileStyleMoved("Follower".into(), "b".into())]
        );
        c.profiles[1].preset_revision = arcvox_core::profile::preset("casual").unwrap().revision();
        assert!(notices(&c, any_language(have(&["local", "own"]))).is_empty());
    }

    #[test]
    fn an_engine_whose_own_file_is_gone_is_named_even_when_no_profile_uses_it() {
        assert_eq!(
            notices(&config(), any_language(have(&["local"]))),
            vec![Notice::ModelFileMissing("Own file".into(), "asr")]
        );
    }

    #[test]
    fn the_default_profile_is_ready_when_every_engine_it_resolves_to_can_run() {
        assert!(default_profile_ready(&config(), have(&["local"])));
        // Its named speech engine is gone and falls back to a default that
        // cannot run either.
        let mut c = config();
        c.default_engines.asr = ProviderId("local".into());
        assert!(!default_profile_ready(&c, have(&[])));
        // A default that can run takes over.
        assert!(default_profile_ready(&config(), have(&[])));
    }

    fn failed_check(cfg: &ProviderConfig) -> EngineCheck {
        EngineCheck {
            ok: false,
            at: 1,
            elapsed_ms: 1,
            message: None,
            detail: None,
            status: Some(429),
            role: ModelRole::Asr,
            service: cfg.service.clone(),
            base_url: cfg.base_url.as_ref().map(|u| u.to_string()),
            model: cfg.model.clone(),
            reasoning_effort: cfg.reasoning_effort.clone(),
            transcribe_mode: cfg.transcribe_mode.clone(),
            word_times: cfg.word_times,
            code: None,
            probe: arcvox_core::checks::probe_version(ModelRole::Asr),
            outdated: false,
        }
    }

    fn checks(check: EngineCheck) -> BTreeMap<String, EngineCheck> {
        BTreeMap::from([("remote".to_string(), check)])
    }

    #[test]
    fn a_failed_test_of_an_engine_in_use_is_a_notice() {
        let mut c = config();
        c.default_engines.asr = ProviderId("remote".into());
        let cfg = c.provider(&ProviderId("remote".into())).unwrap().clone();
        let found = check_notices(&c, &checks(failed_check(&cfg)));
        assert_eq!(
            found,
            vec![Notice::EngineTestFailed("Remote".into(), "asr")]
        );
    }

    #[test]
    fn a_failed_test_of_an_engine_a_profile_names_is_a_notice() {
        let mut c = config();
        c.default_engines.asr = ProviderId("local".into());
        c.profiles[0].asr_provider = Some(ProviderId("remote".into()));
        let cfg = c.provider(&ProviderId("remote".into())).unwrap().clone();
        assert_eq!(check_notices(&c, &checks(failed_check(&cfg))).len(), 1);
    }

    #[test]
    fn a_failed_test_nobody_depends_on_or_of_other_settings_is_no_notice() {
        let mut c = config();
        c.default_engines.asr = ProviderId("local".into());
        c.default_engines.llm = ProviderId("local".into());
        for p in &mut c.profiles {
            p.asr_provider = None;
            p.llm_provider = None;
        }
        let cfg = c.provider(&ProviderId("remote".into())).unwrap().clone();
        assert!(check_notices(&c, &checks(failed_check(&cfg))).is_empty());

        c.default_engines.asr = ProviderId("remote".into());
        let other = EngineCheck {
            model: Some("another".into()),
            ..failed_check(&cfg)
        };
        assert!(check_notices(&c, &checks(other)).is_empty());
        let passed = EngineCheck {
            ok: true,
            ..failed_check(&cfg)
        };
        assert!(check_notices(&c, &checks(passed)).is_empty());
    }
}
