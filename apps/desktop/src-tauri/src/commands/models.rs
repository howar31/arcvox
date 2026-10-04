use std::path::PathBuf;

use arcvox_core::models::{
    InstalledModel, ManifestSource, ModelEntry, ModelError, ModelRole, ModelStore,
};
use arcvox_core::Engine;
use serde::Serialize;
use tauri::State;

use super::{CmdError, CmdResult};
use crate::app::AppState;

#[derive(Debug, Clone, Serialize)]
pub struct ManifestView {
    pub source: ManifestSource,
    pub models: Vec<ModelEntry>,
}

/// Downloads the recommended speech and language models in that order,
/// skipping the ones already on disk. Stops at the first failure.
pub async fn download_recommended(store: &ModelStore) -> Result<(), ModelError> {
    let (manifest, _) = store.manifest().await;
    for role in [ModelRole::Asr, ModelRole::Llm] {
        let Some(entry) = manifest.recommended(role) else {
            continue;
        };
        if store.installed().iter().any(|m| m.id == entry.id) {
            continue;
        }
        store.download(&entry.id).await?;
    }
    Ok(())
}

#[tauri::command]
pub async fn models_manifest(state: State<'_, AppState>) -> CmdResult<ManifestView> {
    let (manifest, source) = state.models.manifest().await;
    Ok(ManifestView {
        source,
        models: manifest.models,
    })
}

/// The catalogue available without waiting on the network, so the page can
/// render before `models_manifest` settles.
#[tauri::command]
pub fn models_manifest_now(state: State<'_, AppState>) -> CmdResult<ManifestView> {
    let (manifest, source) = state.models.manifest_now();
    Ok(ManifestView {
        source,
        models: manifest.models,
    })
}

#[tauri::command]
pub fn models_installed(state: State<'_, AppState>) -> CmdResult<Vec<InstalledModel>> {
    Ok(state.models.installed())
}

#[tauri::command]
pub async fn models_download(id: String, state: State<'_, AppState>) -> CmdResult<InstalledModel> {
    Ok(state.models.download(&id).await?)
}

#[tauri::command]
pub async fn models_download_recommended(state: State<'_, AppState>) -> CmdResult<()> {
    Ok(download_recommended(&state.models).await?)
}

#[tauri::command]
pub fn models_cancel(id: String, state: State<'_, AppState>) -> CmdResult<()> {
    state.models.cancel(&id);
    Ok(())
}

/// Deletes the model file, then drops and unloads every cached provider
/// that loaded it so its memory is freed now.
pub async fn delete(store: &ModelStore, engine: Option<&Engine>, id: &str) -> CmdResult<()> {
    store.delete(id)?;
    if let Some(e) = engine {
        e.evict_model(id).await;
    }
    Ok(())
}

#[tauri::command]
pub async fn models_delete(id: String, state: State<'_, AppState>) -> CmdResult<()> {
    let engine = state.engine.lock().unwrap().clone();
    delete(&state.models, engine.as_ref(), &id).await
}

#[tauri::command]
pub fn models_import(path: String, state: State<'_, AppState>) -> CmdResult<InstalledModel> {
    let path = PathBuf::from(path);
    if !path.is_file() {
        return Err(CmdError {
            message: format!("{} is not a file", path.display()),
        });
    }
    Ok(state.models.import(&path)?)
}

#[tauri::command]
pub fn models_dir(state: State<'_, AppState>) -> CmdResult<String> {
    Ok(state.dirs.models_dir().to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use arcvox_core::models::{Manifest, ModelRole, ModelStore};
    use arcvox_core::DataDir;
    use sha2::{Digest, Sha256};
    use wiremock::matchers::path;
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn bundled_manifest_parses_and_has_the_defaults() {
        let m = Manifest::parse(crate::providers::BUNDLED_MANIFEST).unwrap();
        assert_eq!(
            m.recommended(ModelRole::Asr).unwrap().id,
            arcvox_core::config::MODEL_ID_DEFAULT_ASR
        );
        assert_eq!(
            m.recommended(ModelRole::Llm).unwrap().id,
            arcvox_core::config::MODEL_ID_DEFAULT_LLM
        );
        for e in &m.models {
            assert_eq!(e.sha256.len(), 64, "{}", e.id);
            assert!(e.size_bytes > 0);
            assert!(e.url.starts_with("https://"));
        }
    }

    fn entry(id: &str, runtime: &str, role: &str, url: &str, body: &[u8]) -> String {
        let sha = format!("{:x}", Sha256::digest(body));
        format!(
            r#"{{"id":"{id}","runtime":"{runtime}","role":"{role}","label":"{id}","description":"","languages":[],"url":"{url}","sha256":"{sha}","size_bytes":{},"license":"MIT","recommended":true,"min_ram_gb":1}}"#,
            body.len()
        )
    }

    #[tokio::test]
    async fn recommended_ids_are_downloaded_in_order() {
        let server = MockServer::start().await;
        let a: &[u8] = b"ggml-model-a";
        let b: &[u8] = b"GGUF\x03\x00\x00\x00model-b";
        for (p, body) in [("/ggml-a.bin", a), ("/b.gguf", b)] {
            Mock::given(path(p))
                .respond_with(ResponseTemplate::new(200).set_body_bytes(body))
                .mount(&server)
                .await;
        }
        let manifest = format!(
            r#"{{"schema_version":1,"models":[{},{}]}}"#,
            entry(
                "a",
                "whisper",
                "asr",
                &format!("{}/ggml-a.bin", server.uri()),
                a
            ),
            entry("b", "llama", "llm", &format!("{}/b.gguf", server.uri()), b),
        );
        let tmp = tempfile::tempdir().unwrap();
        let dirs = DataDir::custom(tmp.path());
        dirs.ensure().unwrap();
        let store = ModelStore::new(
            &dirs,
            reqwest::Client::new(),
            Box::leak(manifest.into_boxed_str()),
            None,
        );
        download_recommended(&store).await.unwrap();
        let ids: Vec<String> = store.installed().into_iter().map(|m| m.id).collect();
        assert_eq!(ids, ["a", "b"]);
        // A second run finds both installed and downloads nothing.
        download_recommended(&store).await.unwrap();
    }

    #[derive(Default)]
    struct CountingLlm {
        unloads: std::sync::atomic::AtomicUsize,
    }
    #[async_trait::async_trait]
    impl arcvox_core::providers::LlmProvider for CountingLlm {
        async fn complete(
            &self,
            _: Vec<arcvox_core::types::ChatMessage>,
            _: &arcvox_core::providers::LlmOptions,
        ) -> Result<String, arcvox_core::providers::ProviderError> {
            Ok("ok".into())
        }
        async fn unload(&self) {
            self.unloads
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
    }

    struct CountingFactory(std::sync::Arc<CountingLlm>);
    #[async_trait::async_trait]
    impl arcvox_core::ProviderFactory for CountingFactory {
        fn kind(&self) -> arcvox_core::config::ProviderKind {
            arcvox_core::config::ProviderKind::Embedded
        }
        fn asr(
            &self,
            _: &arcvox_core::config::ProviderConfig,
            _: &arcvox_core::config::General,
        ) -> Result<
            std::sync::Arc<dyn arcvox_core::providers::AsrProvider>,
            arcvox_core::providers::ProviderError,
        > {
            Err(arcvox_core::providers::ProviderError::new(
                arcvox_core::types::ErrorCode::Internal,
                "no speech model in this test",
            ))
        }
        fn llm(
            &self,
            _: &arcvox_core::config::ProviderConfig,
            _: &arcvox_core::config::General,
        ) -> Result<
            std::sync::Arc<dyn arcvox_core::providers::LlmProvider>,
            arcvox_core::providers::ProviderError,
        > {
            Ok(self.0.clone())
        }
        async fn probe(
            &self,
            _: &arcvox_core::config::ProviderConfig,
        ) -> arcvox_core::providers::probe::ProbeResult {
            arcvox_core::providers::probe::ProbeResult {
                ok: true,
                failure: None,
            }
        }
    }

    #[tokio::test]
    async fn delete_evicts_the_loaded_provider() {
        use std::sync::atomic::Ordering;
        let tmp = tempfile::tempdir().unwrap();
        let dirs = DataDir::custom(tmp.path().join("data"));
        let llm = std::sync::Arc::new(CountingLlm::default());
        let engine = arcvox_core::Engine::open_with(
            dirs.clone(),
            arcvox_core::EngineOptions {
                secrets: std::sync::Arc::new(arcvox_core::MemorySecretStore::default()),
                factories: vec![std::sync::Arc::new(CountingFactory(llm.clone()))],
                first_run: arcvox_core::FirstRunDefaults::default(),
                identity: arcvox_core::DeviceOptions::default(),
            },
        )
        .await
        .unwrap();
        let store = ModelStore::new(
            &dirs,
            reqwest::Client::new(),
            crate::providers::BUNDLED_MANIFEST,
            None,
        );
        let source = tmp.path().join("x.gguf");
        std::fs::write(&source, b"GGUF\x03\x00\x00\x00weights").unwrap();
        let installed = store.import(&source).unwrap();
        assert_eq!(installed.id, "custom/x.gguf");
        let file = dirs.models_dir().join("custom").join("x.gguf");
        assert!(file.is_file());

        let llm_id = engine.config().current().profiles[0]
            .llm_provider
            .clone()
            .unwrap();
        engine
            .config()
            .update(|c| {
                let p = c.providers.iter_mut().find(|p| p.id == llm_id).unwrap();
                p.model_path = Some(file.clone());
                p.model_id = None;
            })
            .unwrap();
        let run = engine.bench_llm(&llm_id, "text", false).await;
        assert!(run.error.is_none(), "{:?}", run.error);
        assert_eq!(llm.unloads.load(Ordering::SeqCst), 0);

        delete(&store, Some(&engine), "custom/x.gguf")
            .await
            .unwrap();
        assert_eq!(llm.unloads.load(Ordering::SeqCst), 1);
        assert!(!file.exists());
    }

    #[test]
    fn published_manifest_matches_bundled() {
        let bundled = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/resources/models-manifest.json"
        ))
        .unwrap();
        let published = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../../docs/models/manifest.json"
        ))
        .unwrap();
        assert_eq!(bundled, published);
    }
}
