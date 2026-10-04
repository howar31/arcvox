//! Per-user data directories. Data never lives under the install directory.
//!
//! The folders carry the app's name (a build under another name has its own):
//! macOS: config + history + recordings in `~/Library/Application Support/<name>`,
//!        models in `~/Library/Caches/<name>`.
//! Windows: config in `%APPDATA%\<name>` (roaming),
//!          history + recordings + models in `%LOCALAPPDATA%\<name>`.
//! Linux: XDG config / data / cache dirs under the lower-case name.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataDir {
    /// `config.json`, `prompts/`.
    pub config_dir: PathBuf,
    /// `history.sqlite`, `recordings/`.
    pub data_dir: PathBuf,
    /// `models/`, `bench/`.
    pub cache_dir: PathBuf,
}

/// The folder name for an app name: as given on macOS and Windows, lower
/// case with hyphens on Linux ("Kongwe Dev" gives "kongwe-dev").
fn dir_name(app_name: &str) -> String {
    if cfg!(target_os = "linux") {
        app_name.to_lowercase().replace(' ', "-")
    } else {
        app_name.to_string()
    }
}

impl DataDir {
    /// Platform default locations for the app of this name, so a build under
    /// another name (a development copy) keeps its own data. `None` when the
    /// OS gives no home directory.
    pub fn resolve(app_name: &str) -> Option<Self> {
        let name = dir_name(app_name);
        if cfg!(target_os = "windows") {
            let config = dirs::config_dir()?.join(&name);
            let local = dirs::data_local_dir()?.join(&name);
            Some(Self {
                config_dir: config,
                data_dir: local.clone(),
                cache_dir: local,
            })
        } else if cfg!(target_os = "macos") {
            let support = dirs::data_dir()?.join(&name);
            let cache = dirs::cache_dir()?.join(&name);
            Some(Self {
                config_dir: support.clone(),
                data_dir: support,
                cache_dir: cache,
            })
        } else {
            Some(Self {
                config_dir: dirs::config_dir()?.join(&name),
                data_dir: dirs::data_dir()?.join(&name),
                cache_dir: dirs::cache_dir()?.join(&name),
            })
        }
    }

    /// Everything under one root. Used by dev builds and tests.
    pub fn custom(root: impl AsRef<Path>) -> Self {
        let root = root.as_ref();
        Self {
            config_dir: root.join("config"),
            data_dir: root.join("data"),
            cache_dir: root.join("cache"),
        }
    }

    pub fn ensure(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.config_dir)?;
        std::fs::create_dir_all(self.config_dir.join("prompts"))?;
        std::fs::create_dir_all(&self.data_dir)?;
        std::fs::create_dir_all(self.recordings_dir())?;
        std::fs::create_dir_all(self.models_dir())?;
        std::fs::create_dir_all(self.cache_dir.join("bench"))?;
        Ok(())
    }

    pub fn config_path(&self) -> PathBuf {
        self.config_dir.join("config.json")
    }
    pub fn base_prompt_path(&self) -> PathBuf {
        self.config_dir.join("prompts").join("base.md")
    }
    pub fn history_path(&self) -> PathBuf {
        self.data_dir.join("history.sqlite")
    }
    /// The identity of this installation.
    pub fn device_path(&self) -> PathBuf {
        self.data_dir.join(crate::device::FILE_NAME)
    }
    /// Where recordings are kept unless the user chose another folder:
    /// next to the history, since they are the user's data.
    pub fn recordings_dir(&self) -> PathBuf {
        self.data_dir.join("recordings")
    }
    /// Where earlier builds kept recordings.
    pub fn legacy_recordings_dir(&self) -> PathBuf {
        self.cache_dir.join("recordings")
    }
    pub fn models_dir(&self) -> PathBuf {
        self.cache_dir.join("models")
    }
    /// The clip the speed test replays, recorded or copied from history.
    pub fn bench_clip_path(&self) -> PathBuf {
        self.cache_dir.join("bench").join("clip.wav")
    }
    /// Last successfully fetched remote model manifest.
    pub fn manifest_cache_path(&self) -> PathBuf {
        self.config_dir.join("models-manifest.json")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn custom_root_splits_three_dirs() {
        let d = DataDir::custom("/tmp/kw");
        assert_eq!(d.config_path(), PathBuf::from("/tmp/kw/config/config.json"));
        assert_eq!(
            d.history_path(),
            PathBuf::from("/tmp/kw/data/history.sqlite")
        );
        assert_eq!(d.recordings_dir(), PathBuf::from("/tmp/kw/data/recordings"));
        assert_eq!(d.device_path(), PathBuf::from("/tmp/kw/data/device.json"));
        assert_eq!(
            d.legacy_recordings_dir(),
            PathBuf::from("/tmp/kw/cache/recordings")
        );
    }

    #[test]
    fn ensure_creates_all_dirs() {
        let tmp = tempfile::tempdir().unwrap();
        let d = DataDir::custom(tmp.path());
        d.ensure().unwrap();
        assert!(d.config_dir.join("prompts").is_dir());
        assert!(d.data_dir.is_dir());
        assert!(d.recordings_dir().is_dir());
        assert!(d.models_dir().is_dir());
        assert!(d.bench_clip_path().parent().unwrap().is_dir());
    }

    #[test]
    fn bench_clip_lives_in_the_cache() {
        let d = DataDir::custom("/tmp/kw");
        assert_eq!(
            d.bench_clip_path(),
            PathBuf::from("/tmp/kw/cache/bench/clip.wav")
        );
    }

    #[test]
    fn resolve_is_per_user_and_named() {
        let d = DataDir::resolve("Kongwe").expect("home dir exists on the test machine");
        for p in [&d.config_dir, &d.data_dir, &d.cache_dir] {
            assert!(p.ends_with(dir_name("Kongwe")), "{p:?}");
            assert!(p.is_absolute());
        }
    }

    #[test]
    fn another_app_name_gives_other_folders() {
        let a = DataDir::resolve("Kongwe").unwrap();
        let b = DataDir::resolve("Kongwe Dev").unwrap();
        assert_ne!(a.data_dir, b.data_dir);
        assert_ne!(a.cache_dir, b.cache_dir);
        assert_ne!(a.config_dir, b.config_dir);
    }
}
