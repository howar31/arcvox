//! Writes the backup tree. A tree that is already there is added to and
//! updated, never reduced: what was written stays, whatever happens to the
//! rows it came from.

use std::collections::BTreeMap;
use std::path::Path;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use super::format::*;
use crate::history::{now_unix, HistoryStore};

/// What one write added or changed.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct WriteReport {
    pub rows: usize,
    pub cells: usize,
    pub recordings: usize,
    pub recording_bytes: u64,
    /// The settings file of this device was written.
    pub settings: bool,
    /// The folder was not there and was made again for this write.
    pub recreated: bool,
}

/// Brings the tree at `root` up to date with the store. `settings` is the
/// settings file of this device, written when given. A recording for which
/// `skip_audio` answers true is left for a later write.
#[allow(clippy::too_many_arguments)]
pub fn write_tree(
    store: &HistoryStore,
    root: &Path,
    cats: Categories,
    scope: Scope,
    recordings_dir: &Path,
    app_version: &str,
    settings: Option<&[u8]>,
    skip_audio: &dyn Fn(&Path) -> bool,
) -> Result<WriteReport, BackupError> {
    std::fs::create_dir_all(root).map_err(|e| io_err(root, e))?;
    if read_manifest(root)?.is_none() {
        let manifest = Manifest {
            format: BACKUP_FORMAT.into(),
            version: BACKUP_VERSION,
        };
        write_atomic(&root.join(MANIFEST_NAME), &to_json(&manifest)?)?;
    }
    let mut report = WriteReport::default();
    for device in store.devices()? {
        if scope == Scope::ThisDevice && !device.this {
            continue;
        }
        let dir = root.join(DEVICES_DIR).join(&device.id);
        write_meta(
            &dir,
            &device.id,
            &device.name,
            &device.platform,
            app_version,
        )?;

        if cats.history {
            let mut months: BTreeMap<String, Vec<HistoryLine>> = BTreeMap::new();
            for (mut line, source) in store.export_rows(&device.id, recordings_dir)? {
                let month = month_of(line.ts);
                let rel = format!("recordings/{month}/{}.wav", line.origin_id);
                let target = dir.join(&rel);
                if cats.recordings && !target.exists() {
                    let source = source.filter(|p| {
                        crate::history::inside(p, recordings_dir) && p.is_file() && !skip_audio(p)
                    });
                    if let Some(source) = source {
                        report.recording_bytes += copy_once(&source, &target)?;
                        report.recordings += 1;
                    }
                }
                if target.exists() {
                    line.audio = Some(rel);
                }
                months.entry(month).or_default().push(line);
            }
            for (month, lines) in months {
                report.rows += merge_file(
                    &dir.join("history").join(format!("{month}.jsonl")),
                    lines,
                    |l| l.origin_id,
                    |l| l.updated,
                )?;
            }
        }
        if cats.stats {
            let mut months: BTreeMap<String, Vec<StatsLine>> = BTreeMap::new();
            for cell in store.export_cells(&device.id)? {
                months.entry(month_of(cell.hour)).or_default().push(cell);
            }
            for (month, cells) in months {
                report.cells += merge_file(
                    &dir.join("stats").join(format!("{month}.jsonl")),
                    cells,
                    |c| (c.hour, c.app_name.clone(), c.profile_id.clone()),
                    |c| c.updated,
                )?;
            }
        }
        if let (true, true, Some(bytes)) = (cats.settings, device.this, settings) {
            let path = dir.join(SETTINGS_FILE);
            if std::fs::read(&path).ok().as_deref() != Some(bytes) {
                write_atomic(&path, bytes)?;
                report.settings = true;
            }
        }
    }
    Ok(report)
}

fn to_json<T: Serialize>(value: &T) -> Result<Vec<u8>, BackupError> {
    serde_json::to_vec_pretty(value).map_err(|e| BackupError::Json(e.to_string()))
}

/// The device's own file. Rewritten only when what it says changed.
fn write_meta(
    dir: &Path,
    id: &str,
    name: &str,
    platform: &str,
    app_version: &str,
) -> Result<(), BackupError> {
    let path = dir.join(DEVICE_FILE);
    let old: Option<DeviceMeta> = std::fs::read(&path)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok());
    let same = old.is_some_and(|o| {
        o.id == id && o.name == name && o.platform == platform && o.app_version == app_version
    });
    if same {
        return Ok(());
    }
    let meta = DeviceMeta {
        id: id.into(),
        name: name.into(),
        platform: platform.into(),
        app_version: app_version.into(),
        updated_at: now_unix(),
    };
    write_atomic(&path, &to_json(&meta)?)
}

/// Copies a recording into the tree under a temporary name first. Returns
/// its size.
fn copy_once(source: &Path, target: &Path) -> Result<u64, BackupError> {
    let dir = target.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(dir).map_err(|e| io_err(dir, e))?;
    let part = target.with_extension("wav.part");
    let copied = std::fs::copy(source, &part).and_then(|n| {
        std::fs::rename(&part, target)?;
        Ok(n)
    });
    copied.map_err(|e| {
        let _ = std::fs::remove_file(&part);
        io_err(target, e)
    })
}

/// Merges `local` into the lines of the file at `path`: a line that is not
/// in `local` stays, and so does a line as new as its local counterpart.
/// Returns how many lines were added or replaced; the file is written only
/// when that is not zero.
fn merge_file<T, K>(
    path: &Path,
    local: Vec<T>,
    key: impl Fn(&T) -> K,
    updated: impl Fn(&T) -> i64,
) -> Result<usize, BackupError>
where
    T: Serialize + DeserializeOwned + PartialEq,
    K: Ord,
{
    let mut all: BTreeMap<K, T> = BTreeMap::new();
    // Lines this version cannot read are kept as they are.
    let mut unread: Vec<String> = Vec::new();
    match std::fs::read_to_string(path) {
        Ok(text) => {
            for line in text.lines().filter(|l| !l.trim().is_empty()) {
                match serde_json::from_str::<T>(line) {
                    Ok(v) => {
                        all.insert(key(&v), v);
                    }
                    Err(_) => unread.push(line.to_string()),
                }
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(io_err(path, e)),
    }
    let mut changed = 0;
    for v in local {
        let k = key(&v);
        let keep_old = all.get(&k).is_some_and(|old| {
            updated(old) > updated(&v) || (updated(old) == updated(&v) && *old == v)
        });
        if !keep_old {
            all.insert(k, v);
            changed += 1;
        }
    }
    if changed == 0 {
        return Ok(0);
    }
    let mut out = String::new();
    for v in all.values() {
        out.push_str(&serde_json::to_string(v).map_err(|e| BackupError::Json(e.to_string()))?);
        out.push('\n');
    }
    for line in unread {
        out.push_str(&line);
        out.push('\n');
    }
    write_atomic(path, out.as_bytes())?;
    Ok(changed)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::history::{HistoryStore, NewUtterance, TEST_DEVICE};
    use crate::types::{AudioClip, ErrorCode, ProfileId};
    use rusqlite::params;
    use std::path::PathBuf;

    pub(crate) const SEP: i64 = 1_788_220_800; // 2026-09-01T00:00:00Z
    pub(crate) const OCT: i64 = 1_790_812_800; // 2026-10-01T00:00:00Z

    pub(crate) fn utt(ts: i64, text: &str, audio: Option<PathBuf>) -> NewUtterance {
        NewUtterance {
            ts,
            profile_id: ProfileId("default".into()),
            app_name: Some("Notes".into()),
            raw_text: "raw".into(),
            refined_text: text.into(),
            final_text: text.into(),
            degraded: false,
            audio_path: audio,
            asr_ms: 100,
            llm_ms: Some(200),
            audio_ms: Some(2000),
            dict_fixes: 1,
            snippets: 0,
            llm_changed: true,
            stages: Vec::new(),
        }
    }

    pub(crate) fn wav(dir: &Path, name: &str) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(
            &path,
            crate::audio::encode_wav_pcm16(&AudioClip {
                samples: vec![0.0; 1600],
                sample_rate: 16000,
            }),
        )
        .unwrap();
        path
    }

    pub(crate) fn all() -> Categories {
        Categories {
            history: true,
            stats: true,
            recordings: true,
            settings: true,
        }
    }

    pub(crate) fn text_only() -> Categories {
        Categories {
            recordings: false,
            ..all()
        }
    }

    pub(crate) fn write(
        store: &HistoryStore,
        root: &Path,
        cats: Categories,
        rec: &Path,
    ) -> WriteReport {
        write_tree(
            store,
            root,
            cats,
            Scope::AllDevices,
            rec,
            "0.1.0",
            None,
            &|_| false,
        )
        .unwrap()
    }

    fn device_dir(root: &Path) -> PathBuf {
        root.join("devices").join(TEST_DEVICE)
    }

    fn files(root: &Path) -> Vec<(PathBuf, std::time::SystemTime)> {
        fn walk(dir: &Path, out: &mut Vec<(PathBuf, std::time::SystemTime)>) {
            for e in std::fs::read_dir(dir).unwrap().flatten() {
                let p = e.path();
                if p.is_dir() {
                    walk(&p, out);
                } else {
                    out.push((p.clone(), e.metadata().unwrap().modified().unwrap()));
                }
            }
        }
        let mut out = Vec::new();
        walk(root, &mut out);
        out.sort();
        out
    }

    #[test]
    fn a_tree_has_the_manifest_the_device_and_month_files() {
        let tmp = tempfile::tempdir().unwrap();
        let s = HistoryStore::open_in_memory().unwrap();
        s.insert(utt(OCT + 5, "second", None)).unwrap();
        s.insert(utt(SEP + 5, "first", None)).unwrap();
        s.insert(utt(OCT + 9, "third", None)).unwrap();
        let report = write(&s, tmp.path(), text_only(), Path::new(""));
        assert_eq!((report.rows, report.recordings), (3, 0));
        assert!(report.cells >= 2);

        let manifest: Manifest =
            serde_json::from_slice(&std::fs::read(tmp.path().join(MANIFEST_NAME)).unwrap())
                .unwrap();
        assert_eq!(
            (manifest.format.as_str(), manifest.version),
            (BACKUP_FORMAT, BACKUP_VERSION)
        );
        let dir = device_dir(tmp.path());
        let meta: DeviceMeta =
            serde_json::from_slice(&std::fs::read(dir.join("device.json")).unwrap()).unwrap();
        assert_eq!(
            (
                meta.id.as_str(),
                meta.name.as_str(),
                meta.app_version.as_str()
            ),
            (TEST_DEVICE, "Test", "0.1.0")
        );
        let sep = std::fs::read_to_string(dir.join("history/2026-09.jsonl")).unwrap();
        assert_eq!(sep.lines().count(), 1);
        let oct = std::fs::read_to_string(dir.join("history/2026-10.jsonl")).unwrap();
        let lines: Vec<HistoryLine> = oct
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(
            lines.iter().map(|l| l.origin_id).collect::<Vec<_>>(),
            vec![1, 3]
        );
        assert_eq!(lines[0].final_text, "second");
        assert_eq!(lines[0].audio, None);
        assert!(dir.join("stats/2026-09.jsonl").exists());
        assert!(dir.join("stats/2026-10.jsonl").exists());
        assert!(!dir.join("settings.json").exists());
    }

    #[test]
    fn a_second_write_without_changes_touches_no_file() {
        let tmp = tempfile::tempdir().unwrap();
        let s = HistoryStore::open_in_memory().unwrap();
        s.insert(utt(OCT, "a", None)).unwrap();
        write(&s, tmp.path(), text_only(), Path::new(""));
        let before = files(tmp.path());
        std::thread::sleep(std::time::Duration::from_millis(20));
        let report = write(&s, tmp.path(), text_only(), Path::new(""));
        assert_eq!(report, WriteReport::default());
        assert_eq!(files(tmp.path()), before);
    }

    #[test]
    fn lines_of_rows_deleted_locally_stay() {
        let tmp = tempfile::tempdir().unwrap();
        let s = HistoryStore::open_in_memory().unwrap();
        let id = s.insert(utt(OCT, "gone here", None)).unwrap();
        s.insert(utt(OCT + 1, "kept", None)).unwrap();
        write(&s, tmp.path(), text_only(), Path::new(""));
        s.delete(id).unwrap();
        s.insert(utt(OCT + 2, "new", None)).unwrap();
        write(&s, tmp.path(), text_only(), Path::new(""));
        let text =
            std::fs::read_to_string(device_dir(tmp.path()).join("history/2026-10.jsonl")).unwrap();
        assert_eq!(text.lines().count(), 3);
        assert!(text.contains("gone here"));
    }

    #[test]
    fn a_newer_local_row_replaces_its_line() {
        let tmp = tempfile::tempdir().unwrap();
        let s = HistoryStore::open_in_memory().unwrap();
        let id = s.insert(utt(OCT, "before", None)).unwrap();
        write(&s, tmp.path(), text_only(), Path::new(""));
        std::thread::sleep(std::time::Duration::from_millis(5));
        s.set_final(id, "after").unwrap();
        let report = write(&s, tmp.path(), text_only(), Path::new(""));
        assert_eq!(report.rows, 1);
        let text =
            std::fs::read_to_string(device_dir(tmp.path()).join("history/2026-10.jsonl")).unwrap();
        assert_eq!(text.lines().count(), 1);
        assert!(text.contains(r#""final_text":"after""#));
        assert!(!text.contains(r#""final_text":"before""#));
    }

    #[test]
    fn a_line_that_does_not_parse_is_kept() {
        let tmp = tempfile::tempdir().unwrap();
        let s = HistoryStore::open_in_memory().unwrap();
        s.insert(utt(OCT, "a", None)).unwrap();
        write(&s, tmp.path(), text_only(), Path::new(""));
        let file = device_dir(tmp.path()).join("history/2026-10.jsonl");
        let mut text = std::fs::read_to_string(&file).unwrap();
        text.push_str("garbage that is not a row\n");
        std::fs::write(&file, text).unwrap();
        s.insert(utt(OCT + 1, "b", None)).unwrap();
        write(&s, tmp.path(), text_only(), Path::new(""));
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(text.contains("garbage that is not a row"));
        assert_eq!(text.lines().count(), 3);
    }

    #[test]
    fn recordings_are_copied_once_and_never_removed() {
        let tmp = tempfile::tempdir().unwrap();
        let rec = tmp.path().join("rec");
        std::fs::create_dir_all(&rec).unwrap();
        let root = tmp.path().join("backup");
        let s = HistoryStore::open_in_memory().unwrap();
        let id = s
            .insert(utt(OCT, "spoken", Some(wav(&rec, "a.wav"))))
            .unwrap();
        let report = write(&s, &root, all(), &rec);
        assert_eq!(report.recordings, 1);
        assert!(report.recording_bytes > 0);
        let copy = device_dir(&root).join("recordings/2026-10/1.wav");
        assert!(copy.is_file());
        let line =
            std::fs::read_to_string(device_dir(&root).join("history/2026-10.jsonl")).unwrap();
        assert!(line.contains("recordings/2026-10/1.wav"));

        assert_eq!(write(&s, &root, all(), &rec).recordings, 0);
        s.delete_row(id, &rec).unwrap();
        write(&s, &root, all(), &rec);
        assert!(copy.is_file());
        // Without the recordings ticked, a copy that is there is still named.
        let s2 = HistoryStore::open_in_memory().unwrap();
        s2.insert(utt(OCT, "spoken", None)).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(5));
        s2.set_final(crate::types::HistoryId(1), "edited").unwrap();
        write(&s2, &root, text_only(), &rec);
        let line =
            std::fs::read_to_string(device_dir(&root).join("history/2026-10.jsonl")).unwrap();
        assert!(line.contains("edited") && line.contains("recordings/2026-10/1.wav"));
    }

    #[test]
    fn a_recording_still_in_use_is_left_for_later() {
        let tmp = tempfile::tempdir().unwrap();
        let rec = tmp.path().join("rec");
        std::fs::create_dir_all(&rec).unwrap();
        let root = tmp.path().join("backup");
        let s = HistoryStore::open_in_memory().unwrap();
        let path = wav(&rec, "a.wav");
        s.insert(utt(OCT, "spoken", Some(path.clone()))).unwrap();
        let busy = path.clone();
        let report = write_tree(
            &s,
            &root,
            all(),
            Scope::AllDevices,
            &rec,
            "0.1.0",
            None,
            &move |p| p == busy,
        )
        .unwrap();
        assert_eq!(report.recordings, 0);
        assert!(!device_dir(&root).join("recordings").exists());
    }

    #[test]
    fn rows_still_being_processed_are_left_out() {
        let tmp = tempfile::tempdir().unwrap();
        let s = HistoryStore::open_in_memory().unwrap();
        s.insert_failed(crate::history::FailedUtterance {
            ts: OCT,
            profile_id: ProfileId("default".into()),
            app_name: None,
            audio_path: Some(PathBuf::from("/r/a.wav")),
            audio_ms: None,
            code: ErrorCode::Interrupted,
            error: "did not finish".into(),
            raw_text: String::new(),
            final_text: String::new(),
        })
        .unwrap();
        assert_eq!(write(&s, tmp.path(), text_only(), Path::new("")).rows, 0);
        assert!(!device_dir(tmp.path()).join("history").exists());
    }

    pub(crate) fn foreign(s: &HistoryStore, device: &str, name: &str, origin: i64, ts: i64) {
        let conn = s.connection();
        conn.execute(
            "INSERT INTO devices (id, name, platform) VALUES (?1, ?2, 'macos') ON CONFLICT (id) DO NOTHING",
            params![device, name],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO utterances (ts, profile_id, raw_text, refined_text, final_text, device_id, origin_id, updated)
             VALUES (?1, 'default', 'r', 'f', 'f', ?2, ?3, ?4)",
            params![ts, device, origin, ts * 1000],
        )
        .unwrap();
    }

    #[test]
    fn the_scope_decides_which_devices_are_written() {
        let tmp = tempfile::tempdir().unwrap();
        let s = HistoryStore::open_in_memory().unwrap();
        s.insert(utt(OCT, "mine", None)).unwrap();
        foreign(&s, "other-1", "Other", 7, OCT);
        let one = tmp.path().join("one");
        write_tree(
            &s,
            &one,
            text_only(),
            Scope::ThisDevice,
            Path::new(""),
            "0.1.0",
            None,
            &|_| false,
        )
        .unwrap();
        assert!(one.join("devices").join(TEST_DEVICE).is_dir());
        assert!(!one.join("devices/other-1").exists());
        let both = tmp.path().join("both");
        write(&s, &both, text_only(), Path::new(""));
        assert!(both.join("devices/other-1/history/2026-10.jsonl").is_file());
    }

    #[test]
    fn a_settings_snapshot_goes_to_this_device_only() {
        let tmp = tempfile::tempdir().unwrap();
        let s = HistoryStore::open_in_memory().unwrap();
        foreign(&s, "other-1", "Other", 7, OCT);
        let snapshot = |bytes: &[u8]| {
            write_tree(
                &s,
                tmp.path(),
                all(),
                Scope::AllDevices,
                Path::new(""),
                "0.1.0",
                Some(bytes),
                &|_| false,
            )
            .unwrap()
        };
        assert!(snapshot(b"{\"a\":2}").settings);
        assert!(snapshot(b"{\"a\":1}").settings);
        assert!(!snapshot(b"{\"a\":1}").settings);
        assert_eq!(
            std::fs::read(device_dir(tmp.path()).join("settings.json")).unwrap(),
            b"{\"a\":1}"
        );
        assert!(!tmp.path().join("devices/other-1/settings.json").exists());
    }

    #[test]
    fn a_folder_that_cannot_be_written_reports_and_leaves_no_temp() {
        let tmp = tempfile::tempdir().unwrap();
        let s = HistoryStore::open_in_memory().unwrap();
        s.insert(utt(OCT, "a", None)).unwrap();
        let file = tmp.path().join("plain-file");
        std::fs::write(&file, b"x").unwrap();
        let under_file = write_tree(
            &s,
            &file.join("backup"),
            text_only(),
            Scope::AllDevices,
            Path::new(""),
            "0.1.0",
            None,
            &|_| false,
        );
        assert!(matches!(under_file, Err(BackupError::Io { .. })));

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let locked = tmp.path().join("locked");
            std::fs::create_dir_all(&locked).unwrap();
            std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o555)).unwrap();
            let got = write_tree(
                &s,
                &locked,
                text_only(),
                Scope::AllDevices,
                Path::new(""),
                "0.1.0",
                None,
                &|_| false,
            );
            std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
            assert!(matches!(got, Err(BackupError::Io { .. })));
            assert_eq!(std::fs::read_dir(&locked).unwrap().count(), 0);
        }
    }

    #[test]
    fn a_folder_of_another_kind_or_a_newer_version_is_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let s = HistoryStore::open_in_memory().unwrap();
        s.insert(utt(OCT, "a", None)).unwrap();
        std::fs::write(
            tmp.path().join(MANIFEST_NAME),
            br#"{"format":"arcvox-backup","version":2}"#,
        )
        .unwrap();
        let got = write_tree(
            &s,
            tmp.path(),
            text_only(),
            Scope::AllDevices,
            Path::new(""),
            "0.1.0",
            None,
            &|_| false,
        );
        assert!(matches!(got, Err(BackupError::Newer(2))));
        assert!(!tmp.path().join("devices").exists());
        std::fs::write(
            tmp.path().join(MANIFEST_NAME),
            br#"{"format":"something","version":1}"#,
        )
        .unwrap();
        let got = write_tree(
            &s,
            tmp.path(),
            text_only(),
            Scope::AllDevices,
            Path::new(""),
            "0.1.0",
            None,
            &|_| false,
        );
        assert!(matches!(got, Err(BackupError::NotABackup(_))));
    }
}
