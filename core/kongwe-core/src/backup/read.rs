//! Reads what a backup tree holds, without changing anything.

use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::Serialize;

use super::format::*;

/// A device found in a backup.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BackupDevice {
    pub id: String,
    pub name: String,
    pub platform: String,
    pub rows: u64,
    /// The first and the last month with history rows, as `2026-10`.
    pub first_month: Option<String>,
    pub last_month: Option<String>,
    pub recordings: u64,
    pub recording_bytes: u64,
    pub has_stats: bool,
    pub has_settings: bool,
    /// The device asking is this one.
    pub this: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BackupInfo {
    pub version: u32,
    pub devices: Vec<BackupDevice>,
}

/// The devices of a tree whose directory and own file agree on the id, by
/// name. Fails when `root` is not a backup this version can read.
pub(crate) fn device_dirs(root: &Path) -> Result<(u32, Vec<(DeviceMeta, PathBuf)>), BackupError> {
    let manifest = read_manifest(root)?
        .ok_or_else(|| BackupError::NotABackup(format!("no {MANIFEST_NAME}")))?;
    let mut out = Vec::new();
    let devices = root.join(DEVICES_DIR);
    let entries = match std::fs::read_dir(&devices) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok((manifest.version, out)),
        Err(e) => return Err(io_err(&devices, e)),
    };
    for entry in entries.flatten() {
        let dir = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if !dir.is_dir() || !is_plain_id(&name) {
            continue;
        }
        let meta: Option<DeviceMeta> = std::fs::read(dir.join(DEVICE_FILE))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok());
        if let Some(meta) = meta.filter(|m| m.id == name) {
            out.push((meta, dir));
        }
    }
    out.sort_by(|a, b| (&a.0.name, &a.0.id).cmp(&(&b.0.name, &b.0.id)));
    Ok((manifest.version, out))
}

/// The month files (`2026-10.jsonl`) of a directory, oldest first.
pub(crate) fn month_files(dir: &Path) -> Vec<(String, PathBuf)> {
    let mut out: Vec<(String, PathBuf)> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            let stem = path.file_stem()?.to_str()?.to_string();
            let is_month = stem.len() == 7
                && stem.as_bytes()[4] == b'-'
                && stem
                    .bytes()
                    .enumerate()
                    .all(|(i, b)| i == 4 || b.is_ascii_digit());
            (is_month && path.extension().is_some_and(|x| x == "jsonl") && path.is_file())
                .then_some((stem, path))
        })
        .collect();
    out.sort();
    out
}

/// The lines of a file that parse, and how many did not.
pub(crate) fn read_lines<T: DeserializeOwned>(path: &Path) -> Result<(Vec<T>, usize), BackupError> {
    let text = std::fs::read_to_string(path).map_err(|e| io_err(path, e))?;
    let mut bad = 0;
    let mut out = Vec::new();
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        match serde_json::from_str::<T>(line) {
            Ok(v) => out.push(v),
            Err(_) => bad += 1,
        }
    }
    Ok((out, bad))
}

/// What the tree at `root` holds. `this_device` marks the asking device.
pub fn inspect(root: &Path, this_device: &str) -> Result<BackupInfo, BackupError> {
    let (version, dirs) = device_dirs(root)?;
    let mut devices = Vec::new();
    for (meta, dir) in dirs {
        let history = month_files(&dir.join("history"));
        let mut rows = 0;
        for (_, file) in &history {
            rows += read_lines::<HistoryLine>(file)?.0.len() as u64;
        }
        let (mut recordings, mut recording_bytes) = (0, 0);
        for month in std::fs::read_dir(dir.join("recordings"))
            .into_iter()
            .flatten()
            .flatten()
        {
            for file in std::fs::read_dir(month.path())
                .into_iter()
                .flatten()
                .flatten()
            {
                let is_wav = file.path().extension().is_some_and(|x| x == "wav");
                if let (true, Ok(m)) = (is_wav, file.metadata()) {
                    recordings += 1;
                    recording_bytes += m.len();
                }
            }
        }
        devices.push(BackupDevice {
            this: meta.id == this_device,
            id: meta.id,
            name: meta.name,
            platform: meta.platform,
            rows,
            first_month: history.first().map(|(m, _)| m.clone()),
            last_month: history.last().map(|(m, _)| m.clone()),
            recordings,
            recording_bytes,
            has_stats: !month_files(&dir.join("stats")).is_empty(),
            has_settings: dir.join(SETTINGS_FILE).is_file(),
        });
    }
    Ok(BackupInfo { version, devices })
}
