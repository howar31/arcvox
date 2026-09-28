//! On-disk config store: JSON file, schema migration, atomic writes, and a
//! watch channel so clients refresh when config changes.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio::sync::watch;

use crate::config::base_prompt::DEFAULT_BASE_PROMPT;
use crate::config::model::{Config, SCHEMA_VERSION};
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
}

impl ConfigStore {
    /// Loads `config.json`, writing the first-run config when it is missing.
    pub fn open(dirs: &DataDir) -> Result<Self, StoreError> {
        dirs.ensure().map_err(|e| io_err(&dirs.config_dir, e))?;
        let path = dirs.config_path();
        let config = if path.exists() {
            let raw = std::fs::read_to_string(&path).map_err(|e| io_err(&path, e))?;
            match serde_json::from_str::<serde_json::Value>(&raw)
                .map_err(|e| e.to_string())
                .and_then(migrate)
                .and_then(|v| serde_json::from_value::<Config>(v).map_err(|e| e.to_string()))
            {
                Ok(config) => {
                    validate(&config)?;
                    config
                }
                Err(reason) => {
                    let backup = broken_backup_path(&path);
                    std::fs::rename(&path, &backup).map_err(|e| io_err(&backup, e))?;
                    return Err(StoreError::Broken { backup, reason });
                }
            }
        } else {
            let config = Config::first_run();
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
    pub fn update(&self, f: impl FnOnce(&mut Config)) -> Result<Arc<Config>, StoreError> {
        let mut next = (*self.current()).clone();
        f(&mut next);
        next.schema_version = SCHEMA_VERSION;
        validate(&next)?;
        write_atomic(
            &self.dirs.config_path(),
            serde_json::to_string_pretty(&next).unwrap().as_bytes(),
        )?;
        let next = Arc::new(next);
        let _ = self.tx.send(next.clone());
        Ok(next)
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
        write_atomic(&self.dirs.base_prompt_path(), text.as_bytes())
    }

    pub fn restore_base_prompt(&self) -> Result<String, StoreError> {
        self.set_base_prompt(DEFAULT_BASE_PROMPT)?;
        Ok(DEFAULT_BASE_PROMPT.to_string())
    }
}

/// Schema ladder. Version 1 is the only one so far. A higher version is
/// refused so an older build never rewrites a newer file; `open` moves the
/// file aside and reports the reason.
fn migrate(value: serde_json::Value) -> Result<serde_json::Value, String> {
    let version = value
        .get("schema_version")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;
    match version {
        SCHEMA_VERSION => Ok(value),
        v if v > SCHEMA_VERSION => Err(StoreError::UnsupportedSchema(v).to_string()),
        v => Err(format!("schema_version {v} has no migration path")),
    }
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
        let StoreError::Broken { backup, reason } = err else {
            panic!("expected Broken, got {err}")
        };
        assert!(reason.contains("99"), "{reason}");
        assert_eq!(std::fs::read_to_string(backup).unwrap(), raw);
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
}
