use super::download::{self, DownloadProgress};
use super::manifest::{safe_segment, Manifest, ManifestSource};
use super::*;
use crate::config::{ProviderConfig, Runtime};
use crate::history::MoveReport;
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
    /// Where the model files are: the default under the cache, or the
    /// folder the user chose.
    models_dir: RwLock<PathBuf>,
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
            models_dir: RwLock::new(dirs.models_dir()),
            client,
            bundled: Manifest::parse(bundled_manifest).expect("bundled model manifest is valid"),
            manifest_url,
            current: RwLock::new(None),
            resolved: tokio::sync::Mutex::new(false),
            downloads: Mutex::new(HashMap::new()),
            progress,
        }
    }

    /// The folder the model files are in.
    pub fn models_dir(&self) -> PathBuf {
        self.models_dir.read().expect("models dir lock").clone()
    }

    /// Uses `dir` as the folder the files are in, without moving anything:
    /// the folder the user chose earlier, at launch.
    pub fn set_models_dir(&self, dir: PathBuf) {
        *self.models_dir.write().expect("models dir lock") = dir;
    }

    /// Whether the files are in the default folder.
    pub fn in_default_dir(&self) -> bool {
        self.models_dir() == self.dirs.models_dir()
    }

    /// Keeps the model files in `dir` from now on (the default folder for
    /// `None`), moving what is in the current folder there: every model
    /// folder, the imported files and partial downloads. An item that cannot
    /// be moved, or whose name is taken, stays behind and is counted in
    /// `left`. Loaded models must have been unloaded first.
    pub fn move_to(&self, dir: Option<&Path>) -> Result<MoveReport, ModelError> {
        let to = dir
            .map(Path::to_path_buf)
            .unwrap_or_else(|| self.dirs.models_dir());
        let from = self.models_dir();
        if to == from {
            return Ok(MoveReport::default());
        }
        std::fs::create_dir_all(&to)?;
        // A folder that cannot be written to is refused before anything moves.
        let probe = to.join(".arcvox-write-test");
        std::fs::write(&probe, b"")?;
        let _ = std::fs::remove_file(&probe);
        let report = move_items(&from, &to);
        *self.models_dir.write().expect("models dir lock") = to;
        Ok(report)
    }

    /// Earlier builds kept the model files in the cache directory, which may
    /// be cleared. While the default folder is in use and still empty, the
    /// files move out of the old place once; a folder that is a link was
    /// arranged by hand and is left alone. Returns what moved.
    pub fn adopt_legacy_dir(&self) -> Result<MoveReport, ModelError> {
        let legacy = self.dirs.legacy_models_dir();
        let to = self.models_dir();
        if !self.in_default_dir() || legacy == to {
            return Ok(MoveReport::default());
        }
        let Ok(meta) = std::fs::symlink_metadata(&legacy) else {
            return Ok(MoveReport::default());
        };
        if !meta.is_dir() {
            return Ok(MoveReport::default());
        }
        let occupied = std::fs::read_dir(&to)
            .into_iter()
            .flatten()
            .flatten()
            .any(|e| safe_segment(&e.file_name().to_string_lossy()));
        if occupied {
            return Ok(MoveReport::default());
        }
        std::fs::create_dir_all(&to)?;
        let report = move_items(&legacy, &to);
        // Goes only when nothing is left in it.
        let _ = std::fs::remove_dir(&legacy);
        Ok(report)
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
        let models = self.models_dir();
        if safe_segment(id) && safe_segment(&file) {
            models.join(id).join(file)
        } else {
            models.join(".invalid").join(".invalid")
        }
    }

    pub fn installed(&self) -> Vec<InstalledModel> {
        let manifest = self.known_manifest();
        let mut out = Vec::new();
        for entry in std::fs::read_dir(self.models_dir())
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
        let mut custom: Vec<_> = std::fs::read_dir(self.models_dir().join(CUSTOM_DIR))
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
        let models = self.models_dir();
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
        let dir = self.models_dir().join(CUSTOM_DIR);
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
            let dir = self.models_dir();
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
        if dest.starts_with(self.models_dir().join(".invalid")) {
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

/// Moves every item directly inside `from` into `to`; an item that cannot
/// be moved, or whose name is taken, stays behind and is counted in `left`.
fn move_items(from: &Path, to: &Path) -> MoveReport {
    let mut report = MoveReport::default();
    for entry in std::fs::read_dir(from).into_iter().flatten().flatten() {
        let name = entry.file_name();
        if !safe_segment(&name.to_string_lossy()) {
            continue;
        }
        let old = entry.path();
        let new = to.join(&name);
        // A rename works within one volume; across volumes the item is copied.
        let moved = !new.exists()
            && (std::fs::rename(&old, &new).is_ok()
                || (copy_item(&old, &new).is_ok() && remove_item(&old).is_ok()));
        if moved {
            report.moved += 1;
        } else {
            report.left += 1;
        }
    }
    report
}

/// Copies a file or a folder tree.
fn copy_item(from: &Path, to: &Path) -> std::io::Result<()> {
    if from.is_dir() {
        std::fs::create_dir_all(to)?;
        for entry in std::fs::read_dir(from)? {
            let entry = entry?;
            copy_item(&entry.path(), &to.join(entry.file_name()))?;
        }
        Ok(())
    } else {
        std::fs::copy(from, to).map(|_| ())
    }
}

fn remove_item(path: &Path) -> std::io::Result<()> {
    if path.is_dir() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let mut cfg = ProviderConfig::embedded("e", "E", Runtime::Whisper, "a");
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
        let cfg = ProviderConfig::embedded("e", "E", Runtime::Llama, "r");
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
        let cfg = ProviderConfig::embedded("e", "E", Runtime::Llama, "r");
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

    #[test]
    fn moving_the_folder_takes_every_item_and_back() {
        let tmp = tempfile::tempdir().unwrap();
        let (dirs, s) = store(tmp.path());
        let a = dirs.models_dir().join("a");
        std::fs::create_dir_all(&a).unwrap();
        std::fs::write(a.join("ggml-a.bin"), b"ggml").unwrap();
        let custom = dirs.models_dir().join("custom");
        std::fs::create_dir_all(&custom).unwrap();
        std::fs::write(custom.join("mine.gguf"), b"GGUF\x03\x00\x00\x00").unwrap();
        std::fs::write(dirs.models_dir().join(".hidden"), b"").unwrap();
        assert!(s.in_default_dir());

        let chosen = tmp.path().join("elsewhere").join("models");
        let report = s.move_to(Some(&chosen)).unwrap();
        assert_eq!((report.moved, report.left), (2, 0));
        assert_eq!(s.models_dir(), chosen);
        assert!(!s.in_default_dir());
        assert!(chosen.join("a").join("ggml-a.bin").is_file());
        assert!(chosen.join("custom").join("mine.gguf").is_file());
        assert!(!a.exists());
        let ids: Vec<String> = s.installed().into_iter().map(|m| m.id).collect();
        assert_eq!(ids, ["a", "custom/mine.gguf"]);
        assert_eq!(s.model_path("a"), chosen.join("a").join("ggml-a.bin"));

        // An item whose name is taken stays behind.
        std::fs::create_dir_all(dirs.models_dir().join("a")).unwrap();
        std::fs::write(dirs.models_dir().join("a").join("other.bin"), b"x").unwrap();
        std::fs::write(chosen.join("b.part"), b"").unwrap();
        let report = s.move_to(None).unwrap();
        assert_eq!((report.moved, report.left), (2, 1));
        assert!(s.in_default_dir());
        assert!(dirs.models_dir().join("custom").join("mine.gguf").is_file());
        assert!(
            chosen.join("a").join("ggml-a.bin").is_file(),
            "left where it was"
        );
        // Nothing to do when the folder is the current one.
        let report = s.move_to(None).unwrap();
        assert_eq!((report.moved, report.left), (0, 0));
    }

    #[test]
    fn files_of_an_earlier_build_move_out_of_the_cache_once() {
        let tmp = tempfile::tempdir().unwrap();
        let (dirs, s) = store(tmp.path());
        let legacy = dirs.legacy_models_dir();
        std::fs::create_dir_all(legacy.join("a")).unwrap();
        std::fs::write(legacy.join("a").join("ggml-a.bin"), b"ggml").unwrap();
        let report = s.adopt_legacy_dir().unwrap();
        assert_eq!((report.moved, report.left), (1, 0));
        assert!(dirs.models_dir().join("a").join("ggml-a.bin").is_file());
        assert!(!legacy.exists(), "the emptied old folder goes");
        // Nothing to adopt the second time, and a non-empty default folder
        // is never written into from the old place.
        std::fs::create_dir_all(legacy.join("b")).unwrap();
        let report = s.adopt_legacy_dir().unwrap();
        assert_eq!((report.moved, report.left), (0, 0));
        assert!(legacy.join("b").is_dir());
    }

    #[cfg(unix)]
    #[test]
    fn a_linked_cache_folder_is_left_alone() {
        let tmp = tempfile::tempdir().unwrap();
        let (dirs, s) = store(tmp.path());
        let real = tmp.path().join("real");
        std::fs::create_dir_all(real.join("a")).unwrap();
        std::fs::write(real.join("a").join("ggml-a.bin"), b"ggml").unwrap();
        let _ = std::fs::remove_dir(dirs.legacy_models_dir());
        std::fs::create_dir_all(dirs.cache_dir.clone()).unwrap();
        std::os::unix::fs::symlink(&real, dirs.legacy_models_dir()).unwrap();
        let report = s.adopt_legacy_dir().unwrap();
        assert_eq!((report.moved, report.left), (0, 0));
        assert!(real.join("a").join("ggml-a.bin").is_file());
    }
}
