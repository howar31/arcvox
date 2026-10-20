//! The backup tree: one directory per device, a file of history rows and a
//! file of statistics buckets per month, recordings beside them.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::history::HistoryError;

pub const BACKUP_FORMAT: &str = "arcvox-backup";
pub const BACKUP_VERSION: u32 = 1;
pub const MANIFEST_NAME: &str = "arcvox-backup.json";
pub const DEVICES_DIR: &str = "devices";
pub const DEVICE_FILE: &str = "device.json";
pub const SETTINGS_FILE: &str = "settings.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub format: String,
    pub version: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceMeta {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub platform: String,
    #[serde(default)]
    pub app_version: String,
    /// When this file was last written, in seconds since the epoch.
    #[serde(default)]
    pub updated_at: i64,
}

/// One history row. Fields a later version adds are ignored on reading.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryLine {
    /// The row's id on the device that made it.
    pub origin_id: i64,
    pub ts: i64,
    /// When the row last changed, in milliseconds since the epoch.
    #[serde(default)]
    pub updated: i64,
    pub profile_id: String,
    #[serde(default)]
    pub app_name: Option<String>,
    #[serde(default)]
    pub raw_text: String,
    #[serde(default)]
    pub refined_text: String,
    #[serde(default)]
    pub final_text: String,
    /// What was said, as the user corrected it; absent while uncorrected.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub heard_text: Option<String>,
    #[serde(default)]
    pub degraded: bool,
    #[serde(default)]
    pub asr_ms: u64,
    #[serde(default)]
    pub llm_ms: Option<u64>,
    #[serde(default)]
    pub audio_ms: Option<u64>,
    #[serde(default)]
    pub dict_fixes: u32,
    #[serde(default)]
    pub snippets: u32,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub error_code: Option<String>,
    /// The recording, as a path inside the device's directory with `/`
    /// separators.
    #[serde(default)]
    pub audio: Option<String>,
    /// Every result the row had, oldest first; empty for a row never processed again.
    #[serde(default)]
    pub attempts: Vec<AttemptLine>,
    /// What each stage of the row's own run did, in order; empty for a row
    /// from before the stages were kept.
    #[serde(default)]
    pub stages: Vec<StageLine>,
}

/// One result of a row: what processing its recording gave, or how it
/// failed. The chosen one is what the row shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttemptLine {
    pub ts: i64,
    pub profile_id: String,
    #[serde(default)]
    pub asr_label: Option<String>,
    #[serde(default)]
    pub llm_label: Option<String>,
    #[serde(default)]
    pub raw_text: String,
    #[serde(default)]
    pub final_text: String,
    #[serde(default)]
    pub degraded: bool,
    #[serde(default)]
    pub asr_ms: u64,
    #[serde(default)]
    pub llm_ms: Option<u64>,
    #[serde(default)]
    pub dict_fixes: u32,
    #[serde(default)]
    pub snippets: u32,
    #[serde(default)]
    pub llm_changed: bool,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub error_code: Option<String>,
    /// The result the row had before it was first processed again.
    #[serde(default)]
    pub origin: bool,
    #[serde(default)]
    pub chosen: bool,
    /// The user's correction of this result's text, when there is one.
    #[serde(default)]
    pub corrected_text: Option<String>,
    /// What each stage of this result's run did, in order.
    #[serde(default)]
    pub stages: Vec<StageLine>,
}

/// What one stage did in one run. The kind is kept as a name, so a stage a
/// later version adds reads and is left out, not refused.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StageLine {
    pub position: u32,
    pub kind: String,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub ms: Option<u64>,
    #[serde(default)]
    pub detail: serde_json::Value,
}

/// One statistics bucket: its key, when it last changed, and its counters
/// by column name.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StatsLine {
    pub hour: i64,
    pub app_name: String,
    pub profile_id: String,
    #[serde(default)]
    pub updated: i64,
    #[serde(flatten)]
    pub counters: BTreeMap<String, serde_json::Value>,
}

impl StatsLine {
    /// A counter by column name; 0 when the line does not have it.
    pub fn counter(&self, name: &str) -> i64 {
        self.counters
            .get(name)
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(0)
    }
}

/// What a backup holds.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Categories {
    pub history: bool,
    pub stats: bool,
    pub recordings: bool,
    pub settings: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// Only the device the store belongs to.
    ThisDevice,
    /// Every device with rows in the store.
    AllDevices,
}

#[derive(Debug, thiserror::Error)]
pub enum BackupError {
    #[error("io error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error(transparent)]
    History(#[from] HistoryError),
    #[error("not a records file or folder: {0}")]
    NotABackup(String),
    #[error("the records were written by a newer version (format {0})")]
    Newer(u32),
    #[error("a dictation is being processed")]
    Busy,
    #[error("json: {0}")]
    Json(String),
}

pub(crate) fn io_err(path: &Path, source: std::io::Error) -> BackupError {
    BackupError::Io {
        path: path.to_path_buf(),
        source,
    }
}

/// The month a time falls in, in UTC, as `2026-10`.
pub fn month_of(ts: i64) -> String {
    // Days to a civil date, with the year starting in March.
    let z = ts.div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}")
}

/// Whether `s` can be a directory name of a device: letters, digits and
/// hyphens only.
pub fn is_plain_id(s: &str) -> bool {
    (1..=64).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

/// Reads the manifest of a tree. `Ok(None)` when there is none.
pub(crate) fn read_manifest(root: &Path) -> Result<Option<Manifest>, BackupError> {
    let path = root.join(MANIFEST_NAME);
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(io_err(&path, e)),
    };
    let manifest: Manifest =
        serde_json::from_slice(&bytes).map_err(|e| BackupError::NotABackup(e.to_string()))?;
    if manifest.format != BACKUP_FORMAT {
        return Err(BackupError::NotABackup(format!(
            "format is `{}`",
            manifest.format
        )));
    }
    if manifest.version > BACKUP_VERSION {
        return Err(BackupError::Newer(manifest.version));
    }
    Ok(Some(manifest))
}

/// Writes through a temporary file in the same directory, so a reader never
/// sees half a file.
pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), BackupError> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(dir).map_err(|e| io_err(dir, e))?;
    let mut tmp = tempfile::NamedTempFile::new_in(dir).map_err(|e| io_err(dir, e))?;
    tmp.write_all(bytes).map_err(|e| io_err(path, e))?;
    tmp.persist(path).map_err(|e| io_err(path, e.error))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn month_of_known_dates() {
        assert_eq!(month_of(0), "1970-01");
        assert_eq!(month_of(-1), "1969-12");
        assert_eq!(month_of(951_782_400), "2000-02"); // 2000-02-29
        assert_eq!(month_of(951_868_800), "2000-03");
        assert_eq!(month_of(1_788_220_800), "2026-09");
        assert_eq!(month_of(1_790_812_799), "2026-09");
        assert_eq!(month_of(1_790_812_800), "2026-10");
    }

    #[test]
    fn plain_ids() {
        assert!(is_plain_id("0f8b6a1e-3c1d-4b0a-9d39-0d7a1c2b3e4f"));
        assert!(is_plain_id("test-device"));
        for bad in ["", "..", "a/b", "a\\b", "a.b", " a", &"x".repeat(65)] {
            assert!(!is_plain_id(bad), "{bad:?}");
        }
    }

    #[test]
    fn a_line_with_fields_of_a_later_version_still_reads() {
        let line: HistoryLine = serde_json::from_str(
            r#"{"origin_id":4,"ts":10,"profile_id":"default","later":{"x":1}}"#,
        )
        .unwrap();
        assert_eq!(
            (line.origin_id, line.updated, line.final_text.as_str()),
            (4, 0, "")
        );
        let cell: StatsLine = serde_json::from_str(
            r#"{"hour":3600,"app_name":"","profile_id":"default","count":2,"later":"text"}"#,
        )
        .unwrap();
        assert_eq!(cell.counter("count"), 2);
        assert_eq!(cell.counter("chars"), 0);
    }
}
