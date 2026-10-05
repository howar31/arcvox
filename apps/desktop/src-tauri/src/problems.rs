//! What is wrong with the profiles and the engines as they are set up: the
//! menu-bar menu and the settings window report the same list.

use std::collections::HashSet;
use std::path::PathBuf;

use arcvox_core::config::{Config, ProviderConfig, ProviderKind};
use arcvox_core::models::{InstalledModel, ModelStore};
use arcvox_core::Engine;

use crate::ui::tray::Notice;
use crate::ui::TauriUi;

/// Whether the file an engine runs from is there. Engines that run
/// elsewhere, or come with the system, have none.
fn ready(p: &ProviderConfig, ids: &HashSet<&str>, paths: &HashSet<&PathBuf>) -> bool {
    if p.kind != ProviderKind::Embedded {
        return true;
    }
    match (&p.model_id, &p.model_path) {
        (Some(id), _) => ids.contains(id.as_str()),
        (None, Some(path)) => paths.contains(path),
        (None, None) => false,
    }
}

/// One notice for every profile that names an engine that is gone or whose
/// model is not there, and one for every engine whose own file is gone.
pub fn notices(config: &Config, installed: &[InstalledModel]) -> Vec<Notice> {
    let ids: HashSet<&str> = installed.iter().map(|m| m.id.as_str()).collect();
    let paths: HashSet<&PathBuf> = installed.iter().map(|m| &m.path).collect();
    let mut out = Vec::new();
    for profile in &config.profiles {
        let used = std::iter::once(&profile.asr_provider).chain(profile.llm_provider.iter());
        let engines: Vec<Option<&ProviderConfig>> = used
            .map(|id| config.providers.iter().find(|p| &p.id == id))
            .collect();
        if engines.iter().any(Option::is_none) {
            out.push(Notice::ProfileEngineMissing(profile.name.clone()));
        } else if engines.iter().flatten().any(|p| !ready(p, &ids, &paths)) {
            out.push(Notice::ProfileModelMissing(profile.name.clone()));
        }
    }
    for p in &config.providers {
        if p.model_id.is_none() && p.model_path.is_some() && !ready(p, &ids, &paths) {
            out.push(Notice::ModelFileMissing(p.label.clone()));
        }
    }
    out
}

/// Reads the profiles, the engines and the installed models again and
/// reports what is wrong with them.
pub fn refresh(engine: Option<&Engine>, models: &ModelStore, ui: &TauriUi) {
    let found = match engine {
        Some(e) => notices(&e.config().current(), &models.installed()),
        None => Vec::new(),
    };
    ui.set_engine_problems(found);
}

#[cfg(test)]
mod tests {
    use super::*;
    use arcvox_core::config::Runtime;

    fn config() -> Config {
        serde_json::from_value(serde_json::json!({
            "schema_version": 1,
            "general": {},
            "providers": [
                { "id": "remote", "label": "Remote", "kind": "http", "base_url": "http://localhost:1/v1" },
                { "id": "local", "label": "Local", "kind": "embedded", "model_id": "small" },
                { "id": "own", "label": "Own file", "kind": "embedded", "model_path": "/models/own.bin" },
            ],
            "profiles": [
                { "id": "a", "name": "Default", "is_default": true, "asr_provider": "local", "llm_provider": "remote", "steps": [] },
            ],
            "dictionary": [],
        }))
        .unwrap()
    }

    fn model(id: &str, path: &str) -> InstalledModel {
        InstalledModel {
            id: id.into(),
            runtime: Runtime::Whisper,
            path: path.into(),
            size_bytes: 1,
            custom: false,
        }
    }

    #[test]
    fn nothing_is_wrong_while_every_file_is_there() {
        let installed = [
            model("small", "/m/small"),
            model("custom/own.bin", "/models/own.bin"),
        ];
        assert!(notices(&config(), &installed).is_empty());
    }

    #[test]
    fn a_profile_whose_model_is_not_there_is_named() {
        let installed = [model("custom/own.bin", "/models/own.bin")];
        assert_eq!(
            notices(&config(), &installed),
            vec![Notice::ProfileModelMissing("Default".into())]
        );
    }

    #[test]
    fn a_profile_whose_engine_is_gone_is_named_once() {
        let mut c = config();
        c.providers.retain(|p| p.label == "Own file");
        let installed = [model("custom/own.bin", "/models/own.bin")];
        assert_eq!(
            notices(&c, &installed),
            vec![Notice::ProfileEngineMissing("Default".into())]
        );
    }

    #[test]
    fn an_engine_whose_own_file_is_gone_is_named_even_when_no_profile_uses_it() {
        let installed = [model("small", "/m/small")];
        assert_eq!(
            notices(&config(), &installed),
            vec![Notice::ModelFileMissing("Own file".into())]
        );
    }
}
