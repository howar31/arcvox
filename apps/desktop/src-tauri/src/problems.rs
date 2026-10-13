//! What is wrong with the profiles and the engines as they are set up: the
//! menu-bar menu and the settings window report the same list.

use arcvox_core::config::{Config, EngineRole, ProviderConfig};
use arcvox_core::Engine;

use crate::ui::tray::Notice;
use crate::ui::TauriUi;

/// One notice for every profile that runs on the default instead of the
/// engine it names, one for each engine of the default pair that cannot
/// run, and one for every engine whose own file is gone. `usable` says
/// whether an engine can run now.
pub fn notices(config: &Config, usable: impl Fn(&ProviderConfig) -> bool) -> Vec<Notice> {
    let mut out = Vec::new();
    for profile in &config.profiles {
        if config.resolve_engines(profile, &usable).any_fallback() {
            out.push(Notice::ProfileOnDefault(profile.name.clone()));
        }
    }
    for (role, id) in [
        (EngineRole::Asr, &config.default_engines.asr),
        (EngineRole::Llm, &config.default_engines.llm),
    ] {
        if !config.provider(id).is_some_and(&usable) {
            out.push(match role {
                EngineRole::Asr => Notice::DefaultAsrNotReady,
                EngineRole::Llm => Notice::DefaultLlmNotReady,
            });
        }
    }
    for p in &config.providers {
        if p.model_id.is_none() && p.model_path.is_some() && !usable(p) {
            out.push(Notice::ModelFileMissing(p.label.clone()));
        }
    }
    out
}

/// Reads the profiles and the engines again and reports what is wrong
/// with them.
pub fn refresh(engine: Option<&Engine>, ui: &TauriUi) {
    let found = match engine {
        Some(e) => notices(&e.config().current(), |cfg| e.can_run(cfg)),
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
                { "id": "a", "name": "Default", "is_default": true, "asr_provider": "local", "llm_provider": "remote", "steps": [{ "step": "asr" }, { "step": "llm" }] },
                { "id": "b", "name": "Follower", "steps": [{ "step": "asr" }] },
            ],
            "dictionary": [],
            "default_engines": { "asr": "remote", "llm": "remote" }
        }))
        .unwrap()
    }

    /// The engines whose files are there, by id.
    fn have<'a>(ids: &'a [&'a str]) -> impl Fn(&ProviderConfig) -> bool + 'a {
        move |p| p.model_id.is_none() && p.model_path.is_none() || ids.contains(&p.id.0.as_str())
    }

    #[test]
    fn nothing_is_wrong_while_every_file_is_there() {
        assert!(notices(&config(), have(&["local", "own"])).is_empty());
    }

    #[test]
    fn a_profile_whose_named_engine_cannot_run_is_on_the_default() {
        assert_eq!(
            notices(&config(), have(&["own"])),
            vec![Notice::ProfileOnDefault("Default".into())]
        );
    }

    #[test]
    fn a_profile_whose_engine_is_gone_is_on_the_default_too() {
        let mut c = config();
        c.providers.retain(|p| p.id.0 != "local");
        assert_eq!(
            notices(&c, have(&["own"])),
            vec![Notice::ProfileOnDefault("Default".into())]
        );
    }

    #[test]
    fn a_default_engine_that_cannot_run_is_named_by_its_role() {
        let mut c = config();
        c.default_engines.asr = ProviderId("local".into());
        let found = notices(&c, have(&["own"]));
        assert!(found.contains(&Notice::DefaultAsrNotReady), "{found:?}");
        assert!(!found.contains(&Notice::DefaultLlmNotReady));
        // The follower runs on the default too, but that is the pair's
        // problem, not the profile's.
        assert!(!found.contains(&Notice::ProfileOnDefault("Follower".into())));
    }

    #[test]
    fn an_engine_whose_own_file_is_gone_is_named_even_when_no_profile_uses_it() {
        assert_eq!(
            notices(&config(), have(&["local"])),
            vec![Notice::ModelFileMissing("Own file".into())]
        );
    }
}
