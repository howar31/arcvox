use super::download::{self, DownloadProgress};
use super::manifest::{safe_segment, Manifest, ManifestSource};
use super::*;
use crate::config::{ProviderConfig, Runtime};
use crate::paths::DataDir;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;
use tokio::sync::broadcast;
use url::Url;

const CUSTOM_DIR: &str = "custom";
const MANIFEST_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct InstalledModel {
    /// Catalogue id, or `custom/<file name>` for an imported file.
    pub id: String,
    pub runtime: Runtime,
    pub path: PathBuf,
    pub size_bytes: u64,
    pub custom: bool,
}

pub struct ModelStore {
    pub(crate) dirs: DataDir,
    pub(crate) client: reqwest::Client,
    bundled: Manifest,
    manifest_url: Option<Url>,
    /// The manifest `manifest()` settled on; read by the synchronous methods.
    current: RwLock<Option<(Manifest, ManifestSource)>>,
    /// Serialises `manifest()`; holds whether the source order has been tried.
    resolved: tokio::sync::Mutex<bool>,
    /// Cancel flags of in-flight downloads, keyed by model id.
    pub(crate) downloads: Mutex<HashMap<String, Arc<AtomicBool>>>,
    pub(crate) progress: broadcast::Sender<DownloadProgress>,
}

impl ModelStore {
    pub fn new(
        dirs: &DataDir,
        client: reqwest::Client,
        bundled_manifest: &'static str,
        manifest_url: Option<Url>,
    ) -> ModelStore {
        let (progress, _) = broadcast::channel(64);
        ModelStore {
            dirs: dirs.clone(),
            client,
            bundled: Manifest::parse(bundled_manifest).expect("bundled model manifest is valid"),
            manifest_url,
            current: RwLock::new(None),
            resolved: tokio::sync::Mutex::new(false),
            downloads: Mutex::new(HashMap::new()),
            progress,
        }
    }

    /// Manifest for synchronous callers: the one `manifest()` settled on; before
    /// that (or while it is still fetching) the on-disk cached copy, else the bundled one.
    fn known_manifest(&self) -> Manifest {
        self.manifest_now().0
    }

    /// The manifest available without waiting on the network, with its source.
    pub fn manifest_now(&self) -> (Manifest, ManifestSource) {
        if let Some(found) = self.current.read().expect("manifest lock").as_ref() {
            return found.clone();
        }
        match self.read_cached() {
            Some(m) => (m, ManifestSource::Cached),
            None => (self.bundled.clone(), ManifestSource::Bundled),
        }
    }

    fn read_cached(&self) -> Option<Manifest> {
        let text = std::fs::read_to_string(self.dirs.manifest_cache_path()).ok()?;
        Manifest::parse(&text).ok()
    }

    /// `cache_dir/models/<id>/<file name from url>`. Not checked for existence.
    /// An id outside the catalogue maps to `<id>/<id>.bin`. An id or file name
    /// that is not a single safe path segment maps to a fixed path that no model
    /// can occupy, so the result never leaves `models_dir()`.
    pub fn model_path(&self, id: &str) -> PathBuf {
        self.path_in(&self.known_manifest(), id)
    }

    fn path_in(&self, manifest: &Manifest, id: &str) -> PathBuf {
        let file = manifest
            .get(id)
            .map(|e| e.file_name())
            .unwrap_or_else(|| format!("{id}.bin"));
        let models = self.dirs.models_dir();
        if safe_segment(id) && safe_segment(&file) {
            models.join(id).join(file)
        } else {
            models.join(".invalid").join(".invalid")
        }
    }

    pub fn installed(&self) -> Vec<InstalledModel> {
        let manifest = self.known_manifest();
        let mut out = Vec::new();
        for entry in std::fs::read_dir(self.dirs.models_dir())
            .into_iter()
            .flatten()
            .flatten()
        {
            let id = entry.file_name().to_string_lossy().into_owned();
            if id == CUSTOM_DIR || !safe_segment(&id) || !entry.path().is_dir() {
                continue;
            }
            let found = match manifest.get(&id) {
                Some(e) => {
                    let path = self.path_in(&manifest, &id);
                    regular_size(&path).map(|size| (path, size, e.runtime))
                }
                // Left over from a catalogue that no longer lists it: accept a lone file.
                None => lone_file(&entry.path()).and_then(|(path, size)| {
                    let runtime = magic::detect(&path).ok().flatten()?;
                    Some((path, size, runtime))
                }),
            };
            if let Some((path, size_bytes, runtime)) = found {
                out.push(InstalledModel {
                    id,
                    runtime,
                    path,
                    size_bytes,
                    custom: false,
                });
            }
        }
        out.sort_by(|a, b| a.id.cmp(&b.id));
        let mut custom: Vec<_> = std::fs::read_dir(self.dirs.models_dir().join(CUSTOM_DIR))
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                if !safe_segment(&name) {
                    return None;
                }
                let path = e.path();
                let size_bytes = regular_size(&path)?;
                let runtime = magic::detect(&path).ok().flatten()?;
                Some(InstalledModel {
                    id: format!("{CUSTOM_DIR}/{name}"),
                    runtime,
                    path,
                    size_bytes,
                    custom: true,
                })
            })
            .collect();
        custom.sort_by(|a, b| a.id.cmp(&b.id));
        out.extend(custom);
        out
    }

    /// `model_path` wins over `model_id`; the file must exist either way.
    pub fn resolve(&self, cfg: &ProviderConfig) -> Result<PathBuf, ModelError> {
        if let Some(p) = &cfg.model_path {
            return if p.is_file() {
                Ok(p.clone())
            } else {
                Err(ModelError::NotInstalled(p.display().to_string()))
            };
        }
        let id = cfg.model_id.as_deref().unwrap_or("");
        let manifest = self.known_manifest();
        if manifest.get(id).is_none() {
            return Err(ModelError::NotInCatalogue(id.to_string()));
        }
        let path = self.path_in(&manifest, id);
        if path.is_file() {
            Ok(path)
        } else {
            Err(ModelError::NotInstalled(id.to_string()))
        }
    }

    pub fn delete(&self, id: &str) -> Result<(), ModelError> {
        let models = self.dirs.models_dir();
        if let Some(name) = id.strip_prefix("custom/") {
            if !safe_segment(name) {
                return Err(ModelError::NotInstalled(id.to_string()));
            }
            return std::fs::remove_file(models.join(CUSTOM_DIR).join(name)).map_err(Into::into);
        }
        if !safe_segment(id) || id == CUSTOM_DIR {
            return Err(ModelError::NotInCatalogue(id.to_string()));
        }
        std::fs::remove_dir_all(models.join(id)).map_err(Into::into)
    }

    /// Copies a user-supplied ggml or GGUF file into `custom/`, replacing a file of the same name.
    pub fn import(&self, source: &Path) -> Result<InstalledModel, ModelError> {
        let runtime = magic::detect(source)?.ok_or(ModelError::UnknownFormat)?;
        let name = source
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .filter(|n| safe_segment(n))
            .ok_or(ModelError::UnknownFormat)?;
        let dir = self.dirs.models_dir().join(CUSTOM_DIR);
        std::fs::create_dir_all(&dir)?;
        let path = dir.join(&name);
        let same_file = matches!(
            (source.canonicalize(), path.canonicalize()),
            (Ok(a), Ok(b)) if a == b
        );
        if !same_file {
            // A temp name keeps a half-copied file out of the listing.
            let tmp = dir.join(format!(".{name}.tmp"));
            let copied = std::fs::copy(source, &tmp).and_then(|_| std::fs::rename(&tmp, &path));
            if let Err(e) = copied {
                let _ = std::fs::remove_file(&tmp);
                return Err(e.into());
            }
        }
        let size_bytes = std::fs::metadata(&path)?.len();
        Ok(InstalledModel {
            id: format!("{CUSTOM_DIR}/{name}"),
            runtime,
            path,
            size_bytes,
            custom: true,
        })
    }

    pub fn subscribe(&self) -> broadcast::Receiver<DownloadProgress> {
        self.progress.subscribe()
    }

    /// Requests cancellation of a running download; the partial file is kept for a later resume.
    pub fn cancel(&self, id: &str) {
        if let Some(c) = self.downloads.lock().expect("downloads lock").get(id) {
            c.store(true, Ordering::SeqCst);
        }
    }

    /// Bytes available to this user on the volume holding the model directory.
    pub fn free_disk_bytes(&self) -> std::io::Result<u64> {
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt;
            let dir = self.dirs.models_dir();
            std::fs::create_dir_all(&dir)?;
            let c = std::ffi::CString::new(dir.as_os_str().as_bytes())
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
            let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
            // SAFETY: `c` is a valid NUL-terminated path and `st` is a writable statvfs.
            if unsafe { libc::statvfs(c.as_ptr(), &mut st) } != 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok((st.f_bavail as u64).saturating_mul(st.f_frsize as u64))
        }
        #[cfg(not(unix))]
        {
            // The Windows query arrives with the Windows client.
            Ok(u64::MAX)
        }
    }

    /// Downloads a catalogue model into its store path. One download per id at a time.
    pub async fn download(&self, id: &str) -> Result<InstalledModel, ModelError> {
        let (manifest, _) = self.manifest().await;
        let entry = manifest
            .get(id)
            .ok_or_else(|| ModelError::NotInCatalogue(id.into()))?
            .clone();
        let (cancel, guard) = {
            let mut d = self.downloads.lock().expect("downloads lock");
            if d.contains_key(id) {
                return Err(ModelError::Http("already downloading".into()));
            }
            let c = Arc::new(AtomicBool::new(false));
            d.insert(id.to_string(), c.clone());
            (
                c,
                InFlight {
                    map: &self.downloads,
                    id: id.to_string(),
                },
            )
        };
        let result = self.download_inner(&manifest, &entry, cancel).await;
        drop(guard);
        let _ = self.progress.send(DownloadProgress {
            id: id.into(),
            received: if result.is_ok() { entry.size_bytes } else { 0 },
            total: entry.size_bytes,
            done: true,
            error: result.as_ref().err().map(|e| e.to_string()),
        });
        result
    }

    async fn download_inner(
        &self,
        manifest: &Manifest,
        entry: &ModelEntry,
        cancel: Arc<AtomicBool>,
    ) -> Result<InstalledModel, ModelError> {
        let dest = self.path_in(manifest, &entry.id);
        if dest.starts_with(self.dirs.models_dir().join(".invalid")) {
            return Err(ModelError::Manifest(format!(
                "model `{}` has an unsafe id or file name",
                entry.id
            )));
        }
        let ext = dest.extension().and_then(|e| e.to_str()).unwrap_or("bin");
        let part = dest.with_extension(format!("{ext}.part"));
        let dir = dest.parent().expect("model path has a parent");
        tokio::fs::create_dir_all(dir).await?;
        let existing = tokio::fs::metadata(&part)
            .await
            .map(|m| m.len())
            .unwrap_or(0);
        let free = self.free_disk_bytes()?;
        // Keep headroom beyond the model itself.
        let needed = entry
            .size_bytes
            .saturating_sub(existing)
            .saturating_add(512 * 1024 * 1024);
        if free < needed {
            return Err(ModelError::InsufficientDisk { needed, free });
        }
        let tx = self.progress.clone();
        let id = entry.id.clone();
        download::fetch_to(&self.client, entry, &part, cancel, |received, total| {
            let _ = tx.send(DownloadProgress {
                id: id.clone(),
                received,
                total,
                done: false,
                error: None,
            });
        })
        .await?;
        let p = part.clone();
        let actual = tokio::task::spawn_blocking(move || download::sha256_file(&p))
            .await
            .expect("hash task")?;
        if !actual.eq_ignore_ascii_case(&entry.sha256) {
            let _ = tokio::fs::remove_file(&part).await;
            return Err(ModelError::Sha256Mismatch {
                expected: entry.sha256.clone(),
                actual,
            });
        }
        tokio::fs::rename(&part, &dest).await?;
        Ok(InstalledModel {
            id: entry.id.clone(),
            runtime: entry.runtime,
            path: dest,
            size_bytes: entry.size_bytes,
            custom: false,
        })
    }

    /// Remote, else the last cached copy, else the bundled one. The result is kept
    /// in memory, so the network is tried once per store.
    pub async fn manifest(&self) -> (Manifest, ManifestSource) {
        let mut tried = self.resolved.lock().await;
        if *tried {
            if let Some(found) = self.current.read().expect("manifest lock").as_ref() {
                return found.clone();
            }
        }
        let found = self.load_manifest().await;
        *self.current.write().expect("manifest lock") = Some(found.clone());
        *tried = true;
        found
    }

    async fn load_manifest(&self) -> (Manifest, ManifestSource) {
        if let Some(url) = &self.manifest_url {
            if let Some(m) = self.fetch_remote(url).await {
                return (m, ManifestSource::Remote);
            }
        }
        match self.read_cached() {
            Some(m) => (m, ManifestSource::Cached),
            None => (self.bundled.clone(), ManifestSource::Bundled),
        }
    }

    async fn fetch_remote(&self, url: &Url) -> Option<Manifest> {
        let resp = self
            .client
            .get(url.clone())
            .timeout(MANIFEST_TIMEOUT)
            .send()
            .await
            .ok()?
            .error_for_status()
            .ok()?;
        let text = resp.text().await.ok()?;
        let m = Manifest::parse(&text).ok()?;
        // Cache only text that parsed; a failed write only loses the offline copy.
        let _ = std::fs::write(self.dirs.manifest_cache_path(), &text);
        Some(m)
    }
}

/// Clears a download's in-flight entry when dropped, including when the future is dropped mid-way.
struct InFlight<'a> {
    map: &'a Mutex<HashMap<String, Arc<AtomicBool>>>,
    id: String,
}

impl Drop for InFlight<'_> {
    fn drop(&mut self) {
        if let Ok(mut d) = self.map.lock() {
            d.remove(&self.id);
        }
    }
}

fn regular_size(path: &Path) -> Option<u64> {
    std::fs::metadata(path)
        .ok()
        .filter(|m| m.is_file())
        .map(|m| m.len())
}

/// The only regular file directly inside `dir`; `None` for zero or several.
fn lone_file(dir: &Path) -> Option<(PathBuf, u64)> {
    let mut files = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter_map(|e| regular_size(&e.path()).map(|s| (e.path(), s)));
    let first = files.next()?;
    files.next().is_none().then_some(first)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Capabilities;

    fn store(tmp: &Path) -> (DataDir, ModelStore) {
        let dirs = DataDir::custom(tmp);
        dirs.ensure().unwrap();
        let s = ModelStore::new(&dirs, reqwest::Client::new(), manifest::tests::SAMPLE, None);
        (dirs, s)
    }

    #[test]
    fn installed_lists_catalogue_files_and_custom_imports() {
        let tmp = tempfile::tempdir().unwrap();
        let (dirs, s) = store(tmp.path());
        assert!(s.installed().is_empty());
        let p = s.model_path("a");
        assert_eq!(p, dirs.models_dir().join("a").join("ggml-a.bin"));
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, b"lmgg0000").unwrap();
        let src = tmp.path().join("mine.gguf");
        std::fs::write(&src, b"GGUF\x03\x00\x00\x00").unwrap();
        let imported = s.import(&src).unwrap();
        assert!(imported.custom);
        assert_eq!(imported.runtime, Runtime::Llama);
        assert_eq!(
            imported.path,
            dirs.models_dir().join("custom").join("mine.gguf")
        );
        let ids: Vec<String> = s.installed().into_iter().map(|m| m.id).collect();
        assert_eq!(ids, ["a", "custom/mine.gguf"]);
    }

    #[test]
    fn import_rejects_unknown_format() {
        let tmp = tempfile::tempdir().unwrap();
        let (_d, s) = store(tmp.path());
        let src = tmp.path().join("x.bin");
        std::fs::write(&src, b"nope").unwrap();
        assert!(matches!(s.import(&src), Err(ModelError::UnknownFormat)));
    }

    #[test]
    fn resolve_prefers_model_path_then_catalogue() {
        let tmp = tempfile::tempdir().unwrap();
        let (_d, s) = store(tmp.path());
        let mut cfg =
            ProviderConfig::embedded("e", "E", Runtime::Whisper, "a", Capabilities::default());
        assert!(matches!(s.resolve(&cfg), Err(ModelError::NotInstalled(_))));
        let p = s.model_path("a");
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, b"lmgg").unwrap();
        assert_eq!(s.resolve(&cfg).unwrap(), p);
        let custom = tmp.path().join("c.bin");
        std::fs::write(&custom, b"lmgg").unwrap();
        cfg.model_path = Some(custom.clone());
        assert_eq!(s.resolve(&cfg).unwrap(), custom);
        cfg.model_path = None;
        cfg.model_id = Some("zzz".into());
        assert!(matches!(
            s.resolve(&cfg),
            Err(ModelError::NotInCatalogue(_))
        ));
    }

    #[test]
    fn delete_removes_the_model_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let (_d, s) = store(tmp.path());
        let p = s.model_path("a");
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, b"lmgg").unwrap();
        s.delete("a").unwrap();
        assert!(!p.parent().unwrap().exists());
        assert!(s.installed().is_empty());
    }

    #[tokio::test]
    async fn manifest_falls_back_to_cache_then_bundled() {
        let tmp = tempfile::tempdir().unwrap();
        let dirs = DataDir::custom(tmp.path());
        dirs.ensure().unwrap();
        let bad = url::Url::parse("http://127.0.0.1:9/manifest.json").unwrap();
        let s = ModelStore::new(
            &dirs,
            reqwest::Client::new(),
            manifest::tests::SAMPLE,
            Some(bad.clone()),
        );
        let (m, src) = s.manifest().await;
        assert_eq!(src, ManifestSource::Bundled);
        assert_eq!(m.models.len(), 2);
        std::fs::write(
            dirs.manifest_cache_path(),
            r#"{"schema_version":1,"models":[]}"#,
        )
        .unwrap();
        let s = ModelStore::new(
            &dirs,
            reqwest::Client::new(),
            manifest::tests::SAMPLE,
            Some(bad),
        );
        let (m, src) = s.manifest().await;
        assert_eq!(src, ManifestSource::Cached);
        assert!(m.models.is_empty());
    }

    #[tokio::test]
    async fn manifest_now_does_not_wait_for_the_network() {
        let tmp = tempfile::tempdir().unwrap();
        let (dirs, s) = store(tmp.path());
        assert_eq!(s.manifest_now().1, ManifestSource::Bundled);
        std::fs::write(
            dirs.manifest_cache_path(),
            r#"{"schema_version":1,"models":[]}"#,
        )
        .unwrap();
        let (m, src) = s.manifest_now();
        assert_eq!(src, ManifestSource::Cached);
        assert!(m.models.is_empty());
        let settled = s.manifest().await;
        assert_eq!(s.manifest_now(), settled);
    }

    #[tokio::test]
    async fn remote_manifest_is_used_and_cached() {
        use wiremock::{matchers::path, Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        Mock::given(path("/manifest.json"))
            .respond_with(ResponseTemplate::new(200).set_body_string(manifest::tests::SAMPLE))
            .mount(&server)
            .await;
        let tmp = tempfile::tempdir().unwrap();
        let dirs = DataDir::custom(tmp.path());
        dirs.ensure().unwrap();
        let s = ModelStore::new(
            &dirs,
            reqwest::Client::new(),
            r#"{"schema_version":1,"models":[]}"#,
            Some(url::Url::parse(&format!("{}/manifest.json", server.uri())).unwrap()),
        );
        let (m, src) = s.manifest().await;
        assert_eq!(src, ManifestSource::Remote);
        assert_eq!(m.models.len(), 2);
        assert!(dirs.manifest_cache_path().exists());
    }

    const OTHER: &str = r#"{"schema_version":1,"models":[{"id":"r","runtime":"llama","role":"llm","label":"R","url":"https://h/z/r-file.gguf","sha256":"22","size_bytes":5,"license":"MIT"}]}"#;

    #[tokio::test]
    async fn sync_methods_use_the_fetched_manifest() {
        use wiremock::{matchers::path, Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        Mock::given(path("/m.json"))
            .respond_with(ResponseTemplate::new(200).set_body_string(OTHER))
            .mount(&server)
            .await;
        let tmp = tempfile::tempdir().unwrap();
        let dirs = DataDir::custom(tmp.path());
        dirs.ensure().unwrap();
        let url = url::Url::parse(&format!("{}/m.json", server.uri())).unwrap();
        let s = ModelStore::new(
            &dirs,
            reqwest::Client::new(),
            manifest::tests::SAMPLE,
            Some(url),
        );
        assert_eq!(s.model_path("r"), dirs.models_dir().join("r").join("r.bin"));
        s.manifest().await;
        let p = s.model_path("r");
        assert_eq!(p, dirs.models_dir().join("r").join("r-file.gguf"));
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, b"GGUF").unwrap();
        let cfg = ProviderConfig::embedded("e", "E", Runtime::Llama, "r", Capabilities::default());
        assert_eq!(s.resolve(&cfg).unwrap(), p);
        assert_eq!(s.installed()[0].id, "r");
    }

    #[test]
    fn sync_methods_use_the_cached_manifest_without_a_fetch() {
        let tmp = tempfile::tempdir().unwrap();
        let (dirs, s) = store(tmp.path());
        std::fs::write(dirs.manifest_cache_path(), OTHER).unwrap();
        assert_eq!(
            s.model_path("r"),
            dirs.models_dir().join("r").join("r-file.gguf")
        );
        let cfg = ProviderConfig::embedded("e", "E", Runtime::Llama, "r", Capabilities::default());
        assert!(matches!(s.resolve(&cfg), Err(ModelError::NotInstalled(_))));
    }

    #[tokio::test]
    async fn sync_methods_use_the_cache_while_a_fetch_is_running() {
        use wiremock::{matchers::path, Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        Mock::given(path("/m.json"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(manifest::tests::SAMPLE)
                    .set_delay(std::time::Duration::from_millis(400)),
            )
            .mount(&server)
            .await;
        let tmp = tempfile::tempdir().unwrap();
        let dirs = DataDir::custom(tmp.path());
        dirs.ensure().unwrap();
        std::fs::write(dirs.manifest_cache_path(), OTHER).unwrap();
        let url = url::Url::parse(&format!("{}/m.json", server.uri())).unwrap();
        let s = ModelStore::new(
            &dirs,
            reqwest::Client::new(),
            manifest::tests::SAMPLE,
            Some(url),
        );
        let probe = async {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            s.model_path("r")
        };
        let ((_, src), during) = tokio::join!(s.manifest(), probe);
        assert_eq!(src, ManifestSource::Remote);
        assert_eq!(during, dirs.models_dir().join("r").join("r-file.gguf"));
    }

    #[test]
    fn paths_never_leave_the_models_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let (dirs, s) = store(tmp.path());
        for id in ["../../x", "a/b", "..", "a\\b"] {
            assert!(s.model_path(id).starts_with(dirs.models_dir()), "{id}");
            assert!(!s.model_path(id).to_string_lossy().contains(".."), "{id}");
        }
        let victim = dirs.models_dir().parent().unwrap().join("victim");
        std::fs::create_dir_all(&victim).unwrap();
        for id in ["../victim", "custom/../../victim", "custom", "custom/a/b"] {
            assert!(s.delete(id).is_err(), "{id}");
        }
        assert!(victim.exists());
        let hidden = tmp.path().join(".hidden.gguf");
        std::fs::write(&hidden, b"GGUF\x03\x00\x00\x00").unwrap();
        assert!(matches!(s.import(&hidden), Err(ModelError::UnknownFormat)));
    }

    #[test]
    fn installed_lists_directories_missing_from_the_manifest() {
        let tmp = tempfile::tempdir().unwrap();
        let (dirs, s) = store(tmp.path());
        let gone = dirs.models_dir().join("gone");
        std::fs::create_dir_all(&gone).unwrap();
        std::fs::write(gone.join("old.gguf"), b"GGUF\x03\x00\x00\x00").unwrap();
        let two = dirs.models_dir().join("two");
        std::fs::create_dir_all(&two).unwrap();
        std::fs::write(two.join("a.gguf"), b"GGUF").unwrap();
        std::fs::write(two.join("b.gguf"), b"GGUF").unwrap();
        let empty = dirs.models_dir().join("empty");
        std::fs::create_dir_all(&empty).unwrap();
        let listed = s.installed();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, "gone");
        assert_eq!(listed[0].runtime, Runtime::Llama);
        assert_eq!(listed[0].path, gone.join("old.gguf"));
        s.delete("gone").unwrap();
        assert!(s.installed().is_empty());
    }

    #[test]
    fn import_overwrites_and_tolerates_reimporting_the_stored_file() {
        let tmp = tempfile::tempdir().unwrap();
        let (_d, s) = store(tmp.path());
        let src = tmp.path().join("m.gguf");
        std::fs::write(&src, b"GGUF\x03\x00\x00\x00one").unwrap();
        let first = s.import(&src).unwrap();
        std::fs::write(&src, b"GGUF\x03\x00\x00\x00second").unwrap();
        let second = s.import(&src).unwrap();
        assert_eq!(second.size_bytes, 14);
        let again = s.import(&second.path).unwrap();
        assert_eq!(again.path, first.path);
        assert_eq!(
            std::fs::read(&again.path).unwrap(),
            b"GGUF\x03\x00\x00\x00second"
        );
        assert_eq!(s.installed().len(), 1);
    }
}
