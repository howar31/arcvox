//! On-disk config store: JSON file, schema migration, atomic writes, and a
//! watch channel so clients refresh when config changes.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use tokio::sync::watch;

use crate::config::base_prompt::DEFAULT_BASE_PROMPT;
use crate::config::model::{Config, FirstRunDefaults, SCHEMA_VERSION};
use crate::config::validate::{validate, ConfigError};
use crate::paths::DataDir;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("io error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("config.json could not be parsed; original moved to {backup}: {reason}")]
    Broken { backup: PathBuf, reason: String },
    #[error("config schema_version {0} is newer than this build supports ({SCHEMA_VERSION})")]
    UnsupportedSchema(u32),
    #[error(transparent)]
    Invalid(#[from] ConfigError),
}

#[derive(Debug)]
pub struct ConfigStore {
    dirs: DataDir,
    tx: watch::Sender<Arc<Config>>,
    /// Serializes the read-modify-write sequence in `update`, `set_base_prompt`,
    /// and `restore_base_prompt` so concurrent callers cannot compute their
    /// `next` value from a stale snapshot and clobber each other's write.
    write_lock: Mutex<()>,
}

impl ConfigStore {
    /// Loads `config.json`, writing the first-run config when it is missing.
    pub fn open(dirs: &DataDir) -> Result<Self, StoreError> {
        Self::open_with_defaults(dirs, &FirstRunDefaults::default())
    }

    /// Like `open`, but a missing config file is created from `defaults`. An
    /// existing file is never rewritten.
    pub fn open_with_defaults(
        dirs: &DataDir,
        defaults: &FirstRunDefaults,
    ) -> Result<Self, StoreError> {
        dirs.ensure().map_err(|e| io_err(&dirs.config_dir, e))?;
        let path = dirs.config_path();
        let config = if path.exists() {
            load_existing(&path)?
        } else {
            let config = Config::first_run_with(defaults);
            write_atomic(
                &path,
                serde_json::to_string_pretty(&config).unwrap().as_bytes(),
            )?;
            config
        };
        let prompt_path = dirs.base_prompt_path();
        if !prompt_path.exists() {
            write_atomic(&prompt_path, DEFAULT_BASE_PROMPT.as_bytes())?;
        }
        let (tx, _rx) = watch::channel(Arc::new(config));
        Ok(Self {
            dirs: dirs.clone(),
            tx,
            write_lock: Mutex::new(()),
        })
    }

    pub fn current(&self) -> Arc<Config> {
        self.tx.borrow().clone()
    }

    pub fn subscribe(&self) -> watch::Receiver<Arc<Config>> {
        self.tx.subscribe()
    }

    /// Applies `f` to a copy, validates, writes atomically, then publishes.
    /// On validation failure nothing is written and the old config stays current.
    ///
    /// Holds `write_lock` for the whole read-modify-write-publish sequence so
    /// concurrent callers serialize instead of racing on a stale `current()`.
    pub fn update(&self, f: impl FnOnce(&mut Config)) -> Result<Arc<Config>, StoreError> {
        let _guard = self
            .write_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut next = (*self.current()).clone();
        f(&mut next);
        next.schema_version = SCHEMA_VERSION;
        validate(&next)?;
        write_atomic(
            &self.dirs.config_path(),
            serde_json::to_string_pretty(&next).unwrap().as_bytes(),
        )?;
        let next = Arc::new(next);
        // `send_replace` (unlike `send`) always publishes the new value, even
        // when there are currently no subscribers; `current()` must reflect
        // every successful update regardless of whether anyone is watching.
        self.tx.send_replace(next.clone());
        Ok(next)
    }

    /// Copies `config.json` and the base prompt aside, each as
    /// `<name>.bak-<seconds>`. Returns each file with its copy.
    pub fn backup_copies(&self) -> Result<Vec<(PathBuf, PathBuf)>, StoreError> {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let mut copies = Vec::new();
        for path in [self.dirs.config_path(), self.dirs.base_prompt_path()] {
            if !path.exists() {
                continue;
            }
            let name = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            let mut copy = path.with_file_name(format!("{name}.bak-{ts}"));
            let mut n = 1;
            while copy.exists() {
                n += 1;
                copy = path.with_file_name(format!("{name}.bak-{ts}-{n}"));
            }
            std::fs::copy(&path, &copy).map_err(|e| io_err(&copy, e))?;
            copies.push((path, copy));
        }
        Ok(copies)
    }

    /// Puts files back from the copies `backup_copies` made and publishes
    /// the config they hold.
    pub fn restore_copies(&self, copies: &[(PathBuf, PathBuf)]) -> Result<(), StoreError> {
        let _guard = self
            .write_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for (path, copy) in copies {
            let bytes = std::fs::read(copy).map_err(|e| io_err(copy, e))?;
            write_atomic(path, &bytes)?;
        }
        let config = load_existing(&self.dirs.config_path())?;
        self.tx.send_replace(Arc::new(config));
        Ok(())
    }

    pub fn base_prompt(&self) -> Result<String, StoreError> {
        let path = self.dirs.base_prompt_path();
        match std::fs::read_to_string(&path) {
            Ok(s) => Ok(s),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                Ok(DEFAULT_BASE_PROMPT.to_string())
            }
            Err(e) => Err(io_err(&path, e)),
        }
    }

    pub fn set_base_prompt(&self, text: &str) -> Result<(), StoreError> {
        let _guard = self
            .write_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        write_atomic(&self.dirs.base_prompt_path(), text.as_bytes())
    }

    pub fn restore_base_prompt(&self) -> Result<String, StoreError> {
        // Locks directly rather than calling `set_base_prompt` (which also
        // locks `write_lock`): `Mutex` is not reentrant, so nesting would
        // deadlock on the same thread.
        let _guard = self
            .write_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        write_atomic(
            &self.dirs.base_prompt_path(),
            DEFAULT_BASE_PROMPT.as_bytes(),
        )?;
        Ok(DEFAULT_BASE_PROMPT.to_string())
    }
}

/// Distinguishes the two ways `migrate` can fail, so `load_existing` can
/// leave a newer-than-supported file untouched instead of moving it aside
/// like a genuinely broken one.
enum MigrateError {
    /// `schema_version` is newer than this build supports.
    Unsupported(u32),
    /// `schema_version` is older than this build supports and there is no
    /// migration path for it (only version 1 exists so far).
    NoPath(u32),
}

/// Schema ladder. Version 1 is the only one so far. A higher version is
/// refused so an older build never rewrites a newer file; `open` moves the
/// file aside and reports the reason.
fn migrate(value: serde_json::Value) -> Result<serde_json::Value, MigrateError> {
    let version = value
        .get("schema_version")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;
    match version {
        SCHEMA_VERSION => Ok(value),
        v if v > SCHEMA_VERSION => Err(MigrateError::Unsupported(v)),
        v => Err(MigrateError::NoPath(v)),
    }
}

/// Reads and parses an existing `config.json`. A newer `schema_version` is
/// reported and left on disk untouched; any other failure (malformed JSON,
/// a schema_version with no migration path, a shape that does not
/// deserialize, or a validation failure) moves the file aside to
/// `config.json.broken-<ts>` and reports the reason.
fn load_existing(path: &Path) -> Result<Config, StoreError> {
    let raw = std::fs::read_to_string(path).map_err(|e| io_err(path, e))?;

    let mark_broken = |reason: String| -> Result<Config, StoreError> {
        let backup = broken_backup_path(path);
        std::fs::rename(path, &backup).map_err(|e| io_err(&backup, e))?;
        Err(StoreError::Broken { backup, reason })
    };

    let value: serde_json::Value = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(e) => return mark_broken(e.to_string()),
    };

    let migrated = match migrate(value) {
        Ok(v) => v,
        Err(MigrateError::Unsupported(v)) => return Err(StoreError::UnsupportedSchema(v)),
        Err(MigrateError::NoPath(v)) => {
            return mark_broken(format!("schema_version {v} has no migration path"));
        }
    };

    let config: Config = match serde_json::from_value(migrated) {
        Ok(c) => c,
        Err(e) => return mark_broken(e.to_string()),
    };

    validate(&config)?;
    Ok(config)
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), StoreError> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let mut tmp = tempfile::NamedTempFile::new_in(dir).map_err(|e| io_err(dir, e))?;
    tmp.write_all(bytes).map_err(|e| io_err(path, e))?;
    tmp.as_file().sync_all().map_err(|e| io_err(path, e))?;
    tmp.persist(path).map_err(|e| io_err(path, e.error))?;
    Ok(())
}

fn broken_backup_path(path: &Path) -> PathBuf {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    path.with_file_name(format!("config.json.broken-{ts}"))
}

fn io_err(path: &Path, source: std::io::Error) -> StoreError {
    StoreError::Io {
        path: path.to_path_buf(),
        source,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::model::StepSpec;

    fn store() -> (tempfile::TempDir, ConfigStore) {
        let tmp = tempfile::tempdir().unwrap();
        let dirs = DataDir::custom(tmp.path());
        let store = ConfigStore::open(&dirs).unwrap();
        (tmp, store)
    }

    #[test]
    fn first_open_writes_defaults_and_prompt() {
        let (tmp, store) = store();
        let dirs = DataDir::custom(tmp.path());
        assert!(dirs.config_path().exists());
        assert_eq!(store.base_prompt().unwrap(), DEFAULT_BASE_PROMPT);
        assert_eq!(*store.current(), Config::first_run());
    }

    #[test]
    fn update_persists_and_notifies() {
        let (tmp, store) = store();
        let mut rx = store.subscribe();
        store
            .update(|c| c.general.history_retention_days = 10)
            .unwrap();
        assert!(rx.has_changed().unwrap());
        assert_eq!(rx.borrow_and_update().general.history_retention_days, 10);
        let reopened = ConfigStore::open(&DataDir::custom(tmp.path())).unwrap();
        assert_eq!(reopened.current().general.history_retention_days, 10);
    }

    #[test]
    fn invalid_update_is_rejected_and_not_written() {
        let (tmp, store) = store();
        let before = std::fs::read_to_string(DataDir::custom(tmp.path()).config_path()).unwrap();
        let err = store
            .update(|c| c.profiles[0].is_default = false)
            .unwrap_err();
        assert!(matches!(
            err,
            StoreError::Invalid(ConfigError::DefaultProfileCount(0))
        ));
        let after = std::fs::read_to_string(DataDir::custom(tmp.path()).config_path()).unwrap();
        assert_eq!(before, after);
        assert!(store.current().profiles[0].is_default);
    }

    #[test]
    fn unknown_step_is_reported_not_overwritten() {
        let tmp = tempfile::tempdir().unwrap();
        let dirs = DataDir::custom(tmp.path());
        dirs.ensure().unwrap();
        let mut v = serde_json::to_value(Config::first_run()).unwrap();
        v["profiles"][0]["steps"][1] = serde_json::json!({"step":"translate"});
        let raw = serde_json::to_string(&v).unwrap();
        std::fs::write(dirs.config_path(), &raw).unwrap();
        let err = ConfigStore::open(&dirs).unwrap_err();
        let StoreError::Broken { backup, reason } = err else {
            panic!("expected Broken, got {err}")
        };
        assert!(reason.contains("translate"), "{reason}");
        assert!(
            !dirs.config_path().exists(),
            "broken file must be moved, not overwritten"
        );
        assert_eq!(std::fs::read_to_string(backup).unwrap(), raw);
    }

    #[test]
    fn newer_schema_is_refused_without_touching_file() {
        let tmp = tempfile::tempdir().unwrap();
        let dirs = DataDir::custom(tmp.path());
        dirs.ensure().unwrap();
        let raw = r#"{"schema_version": 99}"#;
        std::fs::write(dirs.config_path(), raw).unwrap();
        let err = ConfigStore::open(&dirs).unwrap_err();
        assert!(matches!(err, StoreError::UnsupportedSchema(99)), "{err}");
        assert!(
            dirs.config_path().exists(),
            "a newer-schema file must not be moved aside"
        );
        assert_eq!(std::fs::read_to_string(dirs.config_path()).unwrap(), raw);
    }

    #[test]
    fn base_prompt_roundtrip_and_restore() {
        let (_tmp, store) = store();
        store.set_base_prompt("custom").unwrap();
        assert_eq!(store.base_prompt().unwrap(), "custom");
        assert_eq!(store.restore_base_prompt().unwrap(), DEFAULT_BASE_PROMPT);
        assert_eq!(store.base_prompt().unwrap(), DEFAULT_BASE_PROMPT);
    }

    #[test]
    fn update_can_change_steps() {
        let (_tmp, store) = store();
        let cfg = store
            .update(|c| c.profiles[0].steps = vec![StepSpec::Asr])
            .unwrap();
        assert_eq!(cfg.profiles[0].steps, vec![StepSpec::Asr]);
    }

    /// Regression test for a lost-update race: without `write_lock` serializing
    /// `update`'s read-modify-write-publish sequence, two threads can both read
    /// the same `current()` snapshot, and the second writer's `next` overwrites
    /// the first writer's change instead of building on it.
    #[test]
    fn concurrent_updates_do_not_lose_writes() {
        let (_tmp, store) = store();
        let store = Arc::new(store);
        store
            .update(|c| c.general.history_retention_days = 0)
            .unwrap();

        const THREADS: u32 = 4;
        const INCREMENTS_PER_THREAD: u32 = 50;

        let handles: Vec<_> = (0..THREADS)
            .map(|_| {
                let store = Arc::clone(&store);
                std::thread::spawn(move || {
                    for _ in 0..INCREMENTS_PER_THREAD {
                        store
                            .update(|c| c.general.history_retention_days += 1)
                            .unwrap();
                    }
                })
            })
            .collect();

        for h in handles {
            h.join().unwrap();
        }

        assert_eq!(
            store.current().general.history_retention_days,
            THREADS * INCREMENTS_PER_THREAD
        );
    }

    #[test]
    fn open_with_defaults_writes_the_native_pair_on_first_run() {
        let tmp = tempfile::tempdir().unwrap();
        let dirs = DataDir::custom(tmp.path());
        dirs.ensure().unwrap();
        let store = ConfigStore::open_with_defaults(
            &dirs,
            &FirstRunDefaults {
                apple_dictation: true,
                apple_foundation_models: true,
            },
        )
        .unwrap();
        let has_apple = |c: &Config| c.providers.iter().any(|p| p.id.0 == "apple-dictation");
        assert!(has_apple(&store.current()));
        assert_eq!(store.current().profiles[0].asr_provider.0, "local-whisper");
        // An existing file is never rewritten by defaults.
        let again = ConfigStore::open_with_defaults(&dirs, &FirstRunDefaults::default()).unwrap();
        assert!(has_apple(&again.current()));
    }
}
