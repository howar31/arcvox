//! Embedded and native engines are made by the app, never by hand: one
//! engine per installed model file, carrying the model's id and the
//! catalogue's name, created when the model arrives and removed with it;
//! one engine per platform service that is there, with a fixed id and
//! name. Only engines for network services are made by hand.

use std::collections::{HashMap, HashSet};

use crate::config::{Capabilities, Config, FirstRunDefaults, ProviderConfig, ProviderKind};
use crate::models::{InstalledModel, Manifest};
use crate::types::ProviderId;

/// Brings the app-made engines in line with what is there: ids and names
/// follow the models and the services, a model or service without an engine
/// gets one, an engine whose model or service is gone is removed unless a
/// profile still names it (that profile keeps its choice and is reported
/// as missing its engine's model or service).
pub fn sync_engines(
    config: &mut Config,
    installed: &[InstalledModel],
    manifest: &Manifest,
    native: &FirstRunDefaults,
) {
    normalise_ids(config);
    ensure_referenced_engines(config, manifest);
    let fixed: HashSet<ProviderId> = config
        .providers
        .iter()
        .filter(|p| p.kind == ProviderKind::Http)
        .map(|p| p.id.clone())
        .collect();
    for native_id in native.native_ids() {
        let Some(made) = ProviderConfig::native_engine(native_id) else {
            continue;
        };
        if fixed.contains(&made.id) {
            continue;
        }
        match config.providers.iter_mut().find(|p| p.id == made.id) {
            Some(row) => *row = made,
            None => config.providers.push(made),
        }
    }
    for m in installed {
        let id = ProviderId(m.id.clone());
        // A hand-made engine that happens to carry the model's id is left
        // alone; the model then has no engine of its own.
        if fixed.contains(&id) {
            continue;
        }
        let label = model_label(m, manifest);
        match config.providers.iter_mut().find(|p| p.id == id) {
            Some(row) => {
                row.label = label;
                row.runtime = Some(m.runtime);
                row.capabilities = Capabilities::for_runtime(m.runtime);
                if m.custom {
                    row.model_id = None;
                    row.model_path = Some(m.path.clone());
                } else {
                    row.model_id = Some(m.id.clone());
                    row.model_path = None;
                }
            }
            None => config.providers.push(engine_for(m, label)),
        }
    }
    let referenced = referenced_ids(config);
    let present: HashSet<&str> = installed.iter().map(|m| m.id.as_str()).collect();
    let services: HashSet<&str> = native.native_ids().into_iter().collect();
    config.providers.retain(|p| match p.kind {
        ProviderKind::Http => true,
        ProviderKind::Embedded => present.contains(p.id.0.as_str()) || referenced.contains(&p.id),
        ProviderKind::Native => {
            p.native_id
                .as_deref()
                .is_some_and(|id| services.contains(id))
                || referenced.contains(&p.id)
        }
    });
    // An engine kept for a profile follows the catalogue too, so the name
    // and the role are right before the model is downloaded.
    for row in &mut config.providers {
        if row.kind != ProviderKind::Embedded || present.contains(row.id.0.as_str()) {
            continue;
        }
        if let Some(entry) = row.model_id.as_deref().and_then(|id| manifest.get(id)) {
            row.label = entry.label.clone();
            row.runtime = Some(entry.runtime);
            row.capabilities = Capabilities::for_runtime(entry.runtime);
        }
    }
}

/// Makes the app-made engines a profile names and that are not there yet:
/// a catalogue model's (listed as not downloaded until the file arrives)
/// or a platform service's. A template names such engines, so a profile
/// made or reset from one is complete at once; an id that is neither is
/// left for validation to report.
pub fn ensure_referenced_engines(config: &mut Config, manifest: &Manifest) {
    let wanted: Vec<ProviderId> = referenced_ids(config)
        .into_iter()
        .filter(|id| config.provider(id).is_none())
        .collect();
    for id in wanted {
        if let Some(entry) = manifest.get(&id.0) {
            config.providers.push(ProviderConfig::embedded(
                &id.0,
                &entry.label,
                entry.runtime,
                &id.0,
            ));
        } else if let Some(made) = ProviderConfig::native_engine_by_id(&id.0) {
            config.providers.push(made);
        }
    }
}

/// The name an installed model's engine shows: the catalogue's label, or the
/// file name of an imported file.
fn model_label(m: &InstalledModel, manifest: &Manifest) -> String {
    if m.custom {
        return m
            .path
            .file_name()
            .map(|f| f.to_string_lossy().into_owned())
            .unwrap_or_else(|| m.id.clone());
    }
    manifest
        .get(&m.id)
        .map(|e| e.label.clone())
        .unwrap_or_else(|| m.id.clone())
}

fn engine_for(m: &InstalledModel, label: String) -> ProviderConfig {
    let mut p = ProviderConfig::embedded(&m.id, &label, m.runtime, &m.id);
    if m.custom {
        p.model_id = None;
        p.model_path = Some(m.path.clone());
    }
    p
}

fn referenced_ids(config: &Config) -> HashSet<ProviderId> {
    config
        .profiles
        .iter()
        .flat_map(|p| std::iter::once(p.asr_provider.clone()).chain(p.llm_provider.clone()))
        .collect()
}

/// The id an app-made engine carries: for an embedded engine its catalogue
/// model's id, or `custom/<file name>` of the file it loads; for a native
/// engine the fixed id of its service. None for a hand-made engine, and for
/// an engine naming neither (validation refuses such a config anyway).
fn made_id_of(p: &ProviderConfig) -> Option<String> {
    match p.kind {
        ProviderKind::Http => None,
        ProviderKind::Native => p
            .native_id
            .as_deref()
            .and_then(ProviderConfig::native_engine)
            .map(|made| made.id.0),
        ProviderKind::Embedded => {
            if let Some(id) = &p.model_id {
                return Some(id.clone());
            }
            let file = p.model_path.as_ref()?.file_name()?;
            Some(format!("custom/{}", file.to_string_lossy()))
        }
    }
}

/// Renames every app-made engine to its model's or service's id and merges
/// engines of the same model or service into the first of them; the
/// profiles follow. Engines from before the rule, and from a settings file
/// written then, arrive with ids of their own.
fn normalise_ids(config: &mut Config) {
    let fixed: HashSet<String> = config
        .providers
        .iter()
        .filter(|p| p.kind == ProviderKind::Http)
        .map(|p| p.id.0.clone())
        .collect();
    let mut renames: HashMap<ProviderId, ProviderId> = HashMap::new();
    let mut taken: HashSet<String> = HashSet::new();
    let mut kept = Vec::with_capacity(config.providers.len());
    for mut p in config.providers.drain(..) {
        let target = match made_id_of(&p) {
            Some(t) if !fixed.contains(&t) => t,
            _ => {
                kept.push(p);
                continue;
            }
        };
        if !taken.insert(target.clone()) {
            renames.insert(p.id, ProviderId(target));
            continue;
        }
        if p.id.0 != target {
            let old = std::mem::replace(&mut p.id, ProviderId(target));
            renames.insert(old, p.id.clone());
        }
        kept.push(p);
    }
    config.providers = kept;
    if renames.is_empty() {
        return;
    }
    for profile in &mut config.profiles {
        if let Some(new) = renames.get(&profile.asr_provider) {
            profile.asr_provider = new.clone();
        }
        if let Some(new) = profile.llm_provider.as_ref().and_then(|id| renames.get(id)) {
            profile.llm_provider = Some(new.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Runtime, MODEL_ID_DEFAULT_ASR, MODEL_ID_DEFAULT_LLM};
    use std::path::PathBuf;

    fn manifest() -> Manifest {
        Manifest::parse(
            r#"{"schema_version":1,"models":[
              {"id":"breeze-asr-25-q5_0","runtime":"whisper","role":"asr","label":"Breeze-ASR-25 (q5_0)","url":"https://x/breeze-q5_0.bin","sha256":"","size_bytes":1,"license":"Apache-2.0","recommended":true},
              {"id":"gemma-3-4b-it-q4_k_m","runtime":"llama","role":"llm","label":"Gemma 3 4B (Q4_K_M)","url":"https://x/gemma.gguf","sha256":"","size_bytes":1,"license":"Gemma","recommended":true},
              {"id":"turbo-q5_0","runtime":"whisper","role":"asr","label":"Whisper turbo (q5_0)","url":"https://x/turbo.bin","sha256":"","size_bytes":1,"license":"MIT"}
            ]}"#,
        )
        .unwrap()
    }

    fn installed(id: &str, runtime: Runtime) -> InstalledModel {
        let custom = id.starts_with("custom/");
        let path = if custom {
            PathBuf::from("/models").join(id)
        } else {
            PathBuf::from("/models").join(id).join("model.bin")
        };
        InstalledModel {
            id: id.into(),
            runtime,
            path,
            size_bytes: 1,
            custom,
        }
    }

    fn ids(c: &Config) -> Vec<&str> {
        c.providers.iter().map(|p| p.id.0.as_str()).collect()
    }

    fn none() -> FirstRunDefaults {
        FirstRunDefaults::default()
    }

    fn both() -> FirstRunDefaults {
        FirstRunDefaults {
            apple_dictation: true,
            apple_foundation_models: true,
        }
    }

    #[test]
    fn platform_services_bring_their_engines_and_take_them_away() {
        let mut c = Config::first_run();
        sync_engines(&mut c, &[], &manifest(), &both());
        assert_eq!(
            ids(&c),
            [
                MODEL_ID_DEFAULT_ASR,
                MODEL_ID_DEFAULT_LLM,
                "apple-dictation",
                "apple-fm"
            ]
        );
        let d = c.provider(&ProviderId("apple-dictation".into())).unwrap();
        assert_eq!(d.label, "Apple Dictation");
        assert_eq!(d.kind, ProviderKind::Native);
        assert!(d.capabilities.transcribe && !d.capabilities.chat);
        // A service the platform no longer reports loses its engine unless a
        // profile uses it; the language model it replaced, not installed
        // and now unused, goes too.
        c.profiles[0].llm_provider = Some(ProviderId("apple-fm".into()));
        sync_engines(&mut c, &[], &manifest(), &none());
        assert_eq!(ids(&c), [MODEL_ID_DEFAULT_ASR, "apple-fm"]);
        assert!(crate::config::validate(&c).is_ok());
    }

    #[test]
    fn a_platform_engine_with_an_old_id_or_name_is_put_right() {
        let mut c = Config::first_run_with(&both());
        let fm = c
            .providers
            .iter_mut()
            .find(|p| p.id.0 == "apple-fm")
            .unwrap();
        fm.id = ProviderId("apple-foundation".into());
        fm.label = "Apple Foundation".into();
        c.profiles[0].llm_provider = Some(ProviderId("apple-foundation".into()));
        sync_engines(&mut c, &[], &manifest(), &both());
        let fm = c.provider(&ProviderId("apple-fm".into())).unwrap();
        assert_eq!(fm.label, "Apple Foundation Models");
        assert_eq!(c.profiles[0].llm_provider.as_ref().unwrap().0, "apple-fm");
        assert!(c.provider(&ProviderId("apple-foundation".into())).is_none());
    }

    #[test]
    fn a_first_run_gets_its_names_from_the_catalogue_before_any_download() {
        let mut c = Config::first_run();
        sync_engines(&mut c, &[], &manifest(), &none());
        assert_eq!(ids(&c), [MODEL_ID_DEFAULT_ASR, MODEL_ID_DEFAULT_LLM]);
        assert_eq!(c.providers[0].label, "Breeze-ASR-25 (q5_0)");
        assert_eq!(c.providers[1].label, "Gemma 3 4B (Q4_K_M)");
        assert!(crate::config::validate(&c).is_ok());
    }

    #[test]
    fn an_installed_model_gets_an_engine_and_loses_it_with_the_file() {
        let mut c = Config::first_run();
        let turbo = installed("turbo-q5_0", Runtime::Whisper);
        sync_engines(&mut c, std::slice::from_ref(&turbo), &manifest(), &none());
        assert_eq!(
            ids(&c),
            [MODEL_ID_DEFAULT_ASR, MODEL_ID_DEFAULT_LLM, "turbo-q5_0"]
        );
        let t = c.provider(&ProviderId("turbo-q5_0".into())).unwrap();
        assert_eq!(t.label, "Whisper turbo (q5_0)");
        assert_eq!(t.kind, ProviderKind::Embedded);
        assert_eq!(t.model_id.as_deref(), Some("turbo-q5_0"));
        assert!(t.capabilities.transcribe && !t.capabilities.chat);
        // Unused and gone: the engine goes too. The default pair stays for
        // the default profile although neither model is installed.
        sync_engines(&mut c, &[], &manifest(), &none());
        assert_eq!(ids(&c), [MODEL_ID_DEFAULT_ASR, MODEL_ID_DEFAULT_LLM]);
    }

    #[test]
    fn an_engine_a_profile_uses_outlives_its_model() {
        let mut c = Config::first_run();
        let turbo = installed("turbo-q5_0", Runtime::Whisper);
        sync_engines(&mut c, std::slice::from_ref(&turbo), &manifest(), &none());
        c.profiles[0].asr_provider = ProviderId("turbo-q5_0".into());
        sync_engines(&mut c, &[], &manifest(), &none());
        assert!(c.provider(&ProviderId("turbo-q5_0".into())).is_some());
        assert_eq!(c.profiles[0].asr_provider.0, "turbo-q5_0");
    }

    #[test]
    fn an_imported_file_is_named_by_its_file_name_and_loaded_by_path() {
        let mut c = Config::first_run();
        let own = installed("custom/My Model.gguf", Runtime::Llama);
        sync_engines(&mut c, std::slice::from_ref(&own), &manifest(), &none());
        let p = c
            .provider(&ProviderId("custom/My Model.gguf".into()))
            .unwrap();
        assert_eq!(p.label, "My Model.gguf");
        assert_eq!(p.model_id, None);
        assert_eq!(p.model_path.as_deref(), Some(own.path.as_path()));
        assert!(p.capabilities.chat);
    }

    #[test]
    fn engines_from_before_the_rule_take_their_models_ids_and_the_profiles_follow() {
        let mut c = Config::first_run();
        // Ids chosen by hand, one model twice, and a file named by its
        // bare file name as a settings file writes it.
        c.providers[0].id = ProviderId("breeze".into());
        c.providers[1].id = ProviderId("gemma".into());
        let mut again = c.providers[1].clone();
        again.id = ProviderId("gemma-fast".into());
        c.providers.push(again);
        let mut own = c.providers[0].clone();
        own.id = ProviderId("mine".into());
        own.model_id = None;
        own.model_path = Some(PathBuf::from("mine.bin"));
        c.providers.push(own);
        c.profiles[0].asr_provider = ProviderId("breeze".into());
        c.profiles[0].llm_provider = Some(ProviderId("gemma-fast".into()));
        let mut second = c.profiles[0].clone();
        second.id = crate::types::ProfileId("mail".into());
        second.is_default = false;
        second.asr_provider = ProviderId("mine".into());
        second.llm_provider = Some(ProviderId("gemma".into()));
        c.profiles.push(second);

        let mine = installed("custom/mine.bin", Runtime::Whisper);
        sync_engines(&mut c, std::slice::from_ref(&mine), &manifest(), &none());
        assert_eq!(
            ids(&c),
            [
                MODEL_ID_DEFAULT_ASR,
                MODEL_ID_DEFAULT_LLM,
                "custom/mine.bin"
            ]
        );
        assert_eq!(c.profiles[0].asr_provider.0, MODEL_ID_DEFAULT_ASR);
        assert_eq!(
            c.profiles[0].llm_provider.as_ref().unwrap().0,
            MODEL_ID_DEFAULT_LLM
        );
        assert_eq!(c.profiles[1].asr_provider.0, "custom/mine.bin");
        assert_eq!(
            c.profiles[1].llm_provider.as_ref().unwrap().0,
            MODEL_ID_DEFAULT_LLM
        );
        let own = c.provider(&ProviderId("custom/mine.bin".into())).unwrap();
        assert_eq!(own.model_path.as_deref(), Some(mine.path.as_path()));
        assert!(crate::config::validate(&c).is_ok());
    }

    #[test]
    fn a_hand_made_engine_with_a_models_id_is_left_alone() {
        let mut c = Config::first_run();
        c.providers.push(ProviderConfig::http_test_provider(
            "turbo-q5_0",
            "http://localhost:1/v1",
            true,
            false,
        ));
        let turbo = installed("turbo-q5_0", Runtime::Whisper);
        sync_engines(&mut c, std::slice::from_ref(&turbo), &manifest(), &none());
        let t = c.provider(&ProviderId("turbo-q5_0".into())).unwrap();
        assert_eq!(t.kind, ProviderKind::Http);
        assert_eq!(c.providers.len(), 3);
    }

    #[test]
    fn a_second_sync_changes_nothing() {
        let mut c = Config::first_run();
        let files = [
            installed(MODEL_ID_DEFAULT_ASR, Runtime::Whisper),
            installed("custom/x.bin", Runtime::Whisper),
        ];
        sync_engines(&mut c, &files, &manifest(), &none());
        let once = c.clone();
        sync_engines(&mut c, &files, &manifest(), &none());
        assert_eq!(c, once);
    }

    #[test]
    fn a_profile_naming_a_missing_template_engine_gets_it_made() {
        let mut c = Config::first_run();
        c.providers.clear();
        c.profiles[0].asr_provider = ProviderId("turbo-q5_0".into());
        c.profiles[0].llm_provider = Some(ProviderId("apple-fm".into()));
        ensure_referenced_engines(&mut c, &manifest());
        assert_eq!(ids(&c), ["turbo-q5_0", "apple-fm"]);
        let t = c.provider(&ProviderId("turbo-q5_0".into())).unwrap();
        assert_eq!(t.label, "Whisper turbo (q5_0)");
        assert!(t.capabilities.transcribe);
        assert_eq!(
            c.provider(&ProviderId("apple-fm".into())).unwrap().kind,
            ProviderKind::Native
        );
        assert!(crate::config::validate(&c).is_ok());
        // An id the app cannot make is left alone.
        c.profiles[0].asr_provider = ProviderId("someone-elses".into());
        ensure_referenced_engines(&mut c, &manifest());
        assert!(c.provider(&ProviderId("someone-elses".into())).is_none());
    }
}
