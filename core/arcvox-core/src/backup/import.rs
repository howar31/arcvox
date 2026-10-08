//! Merges a backup tree into the store. Nothing here is replaced by
//! something older, so the same tree can come in any number of times.

use std::path::{Component, Path, PathBuf};

use serde::Serialize;

use super::format::*;
use super::read::{device_dirs, month_files, read_lines};
use crate::history::{now_unix, HistoryStore};

/// What an import did.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ImportReport {
    pub devices: usize,
    /// History rows that were not here, that were older here, and that were
    /// already here.
    pub added: usize,
    pub updated: usize,
    pub skipped: usize,
    /// Statistics buckets added or replaced.
    pub cells: usize,
    /// Recordings copied in.
    pub recordings: usize,
    /// Rows that name a recording the backup does not have.
    pub missing_recordings: usize,
    /// Lines that could not be read.
    pub bad_lines: usize,
    /// Added rows older than the history retention: the next prune removes them.
    pub expiring: usize,
}

/// The recording a line names, when it is a file inside the device's
/// directory.
fn audio_in(dir: &Path, rel: &str) -> Option<PathBuf> {
    let rel = Path::new(rel);
    let plain = rel.components().all(|c| matches!(c, Component::Normal(_)));
    let path = dir.join(rel);
    (plain && !rel.as_os_str().is_empty() && path.is_file()).then_some(path)
}

fn copy_in(source: &Path, target: &Path) -> Result<(), BackupError> {
    let part = target.with_extension("wav.part");
    std::fs::copy(source, &part)
        .and_then(|_| std::fs::rename(&part, target))
        .map_err(|e| {
            let _ = std::fs::remove_file(&part);
            io_err(target, e)
        })
}

/// Brings the named devices of the tree at `root` into the store.
/// Recordings go to `recordings_dir`. `history_retention_days` only counts
/// the rows the retention will remove again; 0 keeps everything.
pub fn import_tree(
    store: &HistoryStore,
    root: &Path,
    devices: &[String],
    cats: Categories,
    recordings_dir: &Path,
    history_retention_days: u32,
) -> Result<ImportReport, BackupError> {
    let (_, dirs) = device_dirs(root)?;
    let cutoff = match history_retention_days {
        0 => i64::MIN,
        d => now_unix() - i64::from(d) * 86_400,
    };
    let mut report = ImportReport::default();
    for (meta, dir) in dirs {
        if !devices.contains(&meta.id) {
            continue;
        }
        report.devices += 1;
        store.register_device(&meta)?;
        if cats.history {
            let prefix: String = meta.id.chars().take(8).collect();
            if cats.recordings {
                std::fs::create_dir_all(recordings_dir).map_err(|e| io_err(recordings_dir, e))?;
            }
            // A row that has its recording here needs no second copy of it.
            let have_audio = store.origins_with_audio(&meta.id)?;
            let mut rows = Vec::new();
            for (_, file) in month_files(&dir.join("history")) {
                let (lines, bad) = read_lines::<HistoryLine>(&file)?;
                report.bad_lines += bad;
                for line in lines {
                    let mut audio = None;
                    let wanted = cats.recordings && !have_audio.contains(&line.origin_id);
                    if let (true, Some(rel)) = (wanted, line.audio.as_deref()) {
                        match audio_in(&dir, rel) {
                            Some(source) => {
                                let target =
                                    recordings_dir.join(format!("{prefix}-{}.wav", line.origin_id));
                                if !target.exists() {
                                    copy_in(&source, &target)?;
                                    report.recordings += 1;
                                }
                                audio = Some(target);
                            }
                            None => report.missing_recordings += 1,
                        }
                    }
                    rows.push((line, audio));
                }
            }
            let merged = store.merge_rows(&meta.id, &rows, cutoff)?;
            report.added += merged.added;
            report.updated += merged.updated;
            report.skipped += merged.skipped;
            report.expiring += merged.expiring;
        }
        if cats.stats {
            let mut cells = Vec::new();
            for (_, file) in month_files(&dir.join("stats")) {
                let (lines, bad) = read_lines::<StatsLine>(&file)?;
                report.bad_lines += bad;
                cells.extend(lines);
            }
            report.cells += store.merge_cells(&meta.id, &cells)?;
        }
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backup::read::inspect;
    use crate::backup::write::tests::{all, foreign, text_only, utt, wav, write, OCT, SEP};
    use crate::backup::write::write_tree;
    use crate::history::{HistoryStore, TEST_DEVICE};
    use std::path::PathBuf;

    /// A store that is another installation: its own rows carry `device`.
    fn store_of(device: &str, name: &str) -> HistoryStore {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(crate::history::SCHEMA).unwrap();
        let d = crate::history::DeviceRef {
            id: device,
            name,
            platform: "macos",
        };
        crate::history::upgrade(&conn, Path::new(""), &d).unwrap();
        HistoryStore::from_connection(conn, device)
    }

    fn import(
        store: &HistoryStore,
        root: &Path,
        devices: &[&str],
        cats: Categories,
        rec: &Path,
    ) -> ImportReport {
        let devices: Vec<String> = devices.iter().map(|d| d.to_string()).collect();
        import_tree(store, root, &devices, cats, rec, 0).unwrap()
    }

    fn texts(store: &HistoryStore) -> Vec<(i64, String, Option<String>)> {
        let mut rows: Vec<_> = store
            .list(1000, None, false)
            .unwrap()
            .into_iter()
            .map(|u| (u.ts, u.final_text, u.app_name))
            .collect();
        rows.sort();
        rows
    }

    #[test]
    fn a_tree_written_then_imported_into_an_empty_store_is_equal() {
        let tmp = tempfile::tempdir().unwrap();
        let a = store_of("dev-a", "Anna");
        let one = a.insert(utt(SEP + 10, "one", None)).unwrap();
        a.insert(utt(OCT + 10, "two", None)).unwrap();
        a.insert(utt(OCT + 4000, "three", None)).unwrap();
        // A row processed again carries its results along.
        a.record_attempt(
            one,
            &crate::history::Outcome::Done {
                redo: crate::history::Redo {
                    profile_id: crate::types::ProfileId("mail".into()),
                    raw_text: "raw".into(),
                    final_text: "one again".into(),
                    degraded: false,
                    asr_ms: 1,
                    llm_ms: None,
                    dict_fixes: 0,
                    snippets: 0,
                    llm_changed: false,
                },
                asr_label: "Breeze".into(),
                llm_label: None,
            },
            true,
        )
        .unwrap();
        write(&a, tmp.path(), text_only(), Path::new(""));

        let b = HistoryStore::open_in_memory().unwrap();
        let report = import(&b, tmp.path(), &["dev-a"], text_only(), Path::new(""));
        assert_eq!(
            (report.devices, report.added, report.updated, report.skipped),
            (1, 3, 0, 0)
        );
        assert_eq!(texts(&b), texts(&a));
        let imported = b.list_recent(10).unwrap();
        let again = imported
            .iter()
            .find(|u| u.final_text == "one again")
            .unwrap();
        assert_eq!(again.redone, 1);
        let attempts = b.attempts(again.id).unwrap();
        assert_eq!(attempts.len(), 2);
        assert!(attempts[0].chosen && !attempts[0].origin);
        assert_eq!(attempts[0].asr_label.as_deref(), Some("Breeze"));
        assert!(attempts[1].origin);
        assert_eq!(b.stats(0, 0).unwrap().totals, a.stats(0, 0).unwrap().totals);
        let devices = b.devices().unwrap();
        assert_eq!(devices.len(), 2);
        assert_eq!(
            (
                devices[1].id.as_str(),
                devices[1].name.as_str(),
                devices[1].rows
            ),
            ("dev-a", "Anna", 3)
        );
        assert_eq!(devices[0].rows, 0);
    }

    #[test]
    fn importing_twice_changes_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let a = store_of("dev-a", "Anna");
        a.insert(utt(OCT, "one", None)).unwrap();
        write(&a, tmp.path(), text_only(), Path::new(""));
        let b = HistoryStore::open_in_memory().unwrap();
        b.insert(utt(OCT + 1, "mine", None)).unwrap();
        import(&b, tmp.path(), &["dev-a"], text_only(), Path::new(""));
        let rows = texts(&b);
        let totals = b.stats(0, 0).unwrap().totals;
        let again = import(&b, tmp.path(), &["dev-a"], text_only(), Path::new(""));
        assert_eq!(
            (again.added, again.updated, again.skipped, again.cells),
            (0, 0, 1, 0)
        );
        assert_eq!(texts(&b), rows);
        assert_eq!(b.stats(0, 0).unwrap().totals, totals);
    }

    #[test]
    fn two_devices_merge_without_loss() {
        let tmp = tempfile::tempdir().unwrap();
        let a = store_of("dev-a", "Anna");
        a.insert(utt(OCT + 10, "from a", None)).unwrap();
        let b = store_of("dev-b", "Ben");
        b.insert(utt(OCT + 10, "from b", None)).unwrap();
        b.insert(utt(OCT + 20, "from b too", None)).unwrap();
        for s in [&a, &b] {
            write_tree(
                s,
                tmp.path(),
                text_only(),
                Scope::ThisDevice,
                Path::new(""),
                "0.1.0",
                None,
                &|_| false,
            )
            .unwrap();
        }
        let c = HistoryStore::open_in_memory().unwrap();
        let report = import(
            &c,
            tmp.path(),
            &["dev-a", "dev-b"],
            text_only(),
            Path::new(""),
        );
        assert_eq!((report.devices, report.added), (2, 3));
        assert_eq!(c.devices().unwrap().len(), 3);
        // Both devices have a row with origin id 1; neither replaced the other.
        assert_eq!(texts(&c).len(), 3);
        assert_eq!(c.stats(0, 0).unwrap().totals.count, 3);
    }

    #[test]
    fn only_the_devices_asked_for_come_in() {
        let tmp = tempfile::tempdir().unwrap();
        let a = HistoryStore::open_in_memory().unwrap();
        foreign(&a, "dev-x", "X", 1, OCT);
        foreign(&a, "dev-y", "Y", 1, OCT);
        write(&a, tmp.path(), text_only(), Path::new(""));
        let b = store_of("dev-b", "Ben");
        let report = import(&b, tmp.path(), &["dev-y"], text_only(), Path::new(""));
        assert_eq!((report.devices, report.added), (1, 1));
        assert_eq!(
            b.devices()
                .unwrap()
                .iter()
                .map(|d| d.id.clone())
                .collect::<Vec<_>>(),
            vec!["dev-b", "dev-y"]
        );
    }

    #[test]
    fn a_newer_line_updates_an_older_row_and_an_older_line_does_not() {
        let tmp = tempfile::tempdir().unwrap();
        let a = store_of("dev-a", "Anna");
        let id = a.insert(utt(OCT, "first", None)).unwrap();
        write(&a, tmp.path(), text_only(), Path::new(""));
        let b = HistoryStore::open_in_memory().unwrap();
        import(&b, tmp.path(), &["dev-a"], text_only(), Path::new(""));

        std::thread::sleep(std::time::Duration::from_millis(5));
        a.set_final(id, "edited on a").unwrap();
        let newer = tmp.path().join("newer");
        write(&a, &newer, text_only(), Path::new(""));
        let report = import(&b, &newer, &["dev-a"], text_only(), Path::new(""));
        assert_eq!((report.added, report.updated), (0, 1));
        assert_eq!(texts(&b)[0].1, "edited on a");

        // The first, older tree again: the row stays as it is.
        let report = import(&b, tmp.path(), &["dev-a"], text_only(), Path::new(""));
        assert_eq!((report.added, report.updated, report.skipped), (0, 0, 1));
        assert_eq!(texts(&b)[0].1, "edited on a");
    }

    #[test]
    fn an_older_cell_does_not_replace_a_newer_one() {
        let tmp = tempfile::tempdir().unwrap();
        let a = store_of("dev-a", "Anna");
        a.insert(utt(OCT, "one", None)).unwrap();
        let old = tmp.path().join("old");
        write(&a, &old, text_only(), Path::new(""));
        std::thread::sleep(std::time::Duration::from_millis(5));
        a.insert(utt(OCT + 1, "two", None)).unwrap();
        let new = tmp.path().join("new");
        write(&a, &new, text_only(), Path::new(""));

        let b = HistoryStore::open_in_memory().unwrap();
        let stats_only = Categories {
            history: false,
            stats: true,
            recordings: false,
            settings: false,
        };
        import(&b, &new, &["dev-a"], stats_only, Path::new(""));
        assert_eq!(b.stats(0, 0).unwrap().totals.count, 2);
        let report = import(&b, &old, &["dev-a"], stats_only, Path::new(""));
        assert_eq!(report.cells, 0);
        assert_eq!(b.stats(0, 0).unwrap().totals.count, 2);
        assert_eq!(texts(&b).len(), 0);
    }

    #[test]
    fn recordings_come_in_under_a_name_of_their_device() {
        let tmp = tempfile::tempdir().unwrap();
        let rec_a = tmp.path().join("rec-a");
        let rec_b = tmp.path().join("rec-b");
        std::fs::create_dir_all(&rec_a).unwrap();
        std::fs::create_dir_all(&rec_b).unwrap();
        let a = store_of("0f8b6a1e-aaaa", "Anna");
        a.insert(utt(OCT, "spoken", Some(wav(&rec_a, "x.wav"))))
            .unwrap();
        let root = tmp.path().join("backup");
        write(&a, &root, all(), &rec_a);

        let b = HistoryStore::open_in_memory().unwrap();
        b.set_recordings_dir(&rec_b);
        let without = import(&b, &root, &["0f8b6a1e-aaaa"], text_only(), &rec_b);
        assert_eq!((without.added, without.recordings), (1, 0));
        assert_eq!(b.list(10, None, true).unwrap().len(), 0);
        assert_eq!(std::fs::read_dir(&rec_b).unwrap().count(), 0);

        let with = import(&b, &root, &["0f8b6a1e-aaaa"], all(), &rec_b);
        assert_eq!((with.added, with.recordings), (0, 1));
        let row = &b.list(10, None, true).unwrap()[0];
        assert_eq!(row.audio_path, Some(rec_b.join("0f8b6a1e-1.wav")));
        assert!(rec_b.join("0f8b6a1e-1.wav").is_file());
        // Once more: the file is there, nothing is copied.
        assert_eq!(
            import(&b, &root, &["0f8b6a1e-aaaa"], all(), &rec_b).recordings,
            0
        );
    }

    #[test]
    fn a_row_that_has_its_recording_gets_no_second_copy() {
        let tmp = tempfile::tempdir().unwrap();
        let rec = tmp.path().join("rec");
        std::fs::create_dir_all(&rec).unwrap();
        let a = HistoryStore::open_in_memory().unwrap();
        a.insert(utt(OCT, "spoken", Some(wav(&rec, "x.wav"))))
            .unwrap();
        let root = tmp.path().join("backup");
        write(&a, &root, all(), &rec);
        let report = import(&a, &root, &[TEST_DEVICE], all(), &rec);
        assert_eq!((report.added, report.recordings), (0, 0));
        assert_eq!(std::fs::read_dir(&rec).unwrap().count(), 1);
    }

    fn hand_made(root: &Path, device: &str, inner_id: &str, lines: &[&str]) {
        std::fs::create_dir_all(root.join("devices").join(device).join("history")).unwrap();
        std::fs::write(
            root.join(MANIFEST_NAME),
            br#"{"format":"arcvox-backup","version":1}"#,
        )
        .unwrap();
        std::fs::write(
            root.join("devices").join(device).join("device.json"),
            format!(r#"{{"id":"{inner_id}","name":"Hand","platform":"macos"}}"#),
        )
        .unwrap();
        std::fs::write(
            root.join("devices")
                .join(device)
                .join("history/2026-10.jsonl"),
            lines.join("\n"),
        )
        .unwrap();
    }

    #[test]
    fn audio_outside_the_backup_is_not_copied() {
        let tmp = tempfile::tempdir().unwrap();
        let rec = tmp.path().join("rec");
        std::fs::create_dir_all(&rec).unwrap();
        let secret = wav(tmp.path(), "secret.wav");
        let root = tmp.path().join("backup");
        let abs = secret.to_string_lossy().replace('\\', "/");
        hand_made(
            &root,
            "dev-h",
            "dev-h",
            &[
                r#"{"origin_id":1,"ts":1790812800,"updated":5,"profile_id":"default","final_text":"a","audio":"../../../secret.wav"}"#,
                &format!(
                    r#"{{"origin_id":2,"ts":1790812801,"updated":5,"profile_id":"default","final_text":"b","audio":"{abs}"}}"#
                ),
                r#"{"origin_id":3,"ts":1790812802,"updated":5,"profile_id":"default","final_text":"c","audio":"recordings/2026-10/3.wav"}"#,
            ],
        );
        let b = HistoryStore::open_in_memory().unwrap();
        let report = import(&b, &root, &["dev-h"], all(), &rec);
        assert_eq!(
            (report.added, report.recordings, report.missing_recordings),
            (3, 0, 3)
        );
        assert_eq!(b.list(10, None, true).unwrap().len(), 0);
        assert_eq!(std::fs::read_dir(&rec).unwrap().count(), 0);
    }

    #[test]
    fn a_device_directory_with_another_id_inside_is_skipped() {
        let tmp = tempfile::tempdir().unwrap();
        let line = r#"{"origin_id":1,"ts":1790812800,"updated":5,"profile_id":"default","final_text":"a"}"#;
        hand_made(tmp.path(), "dev-h", "someone-else", &[line]);
        let b = HistoryStore::open_in_memory().unwrap();
        let report = import(
            &b,
            tmp.path(),
            &["dev-h", "someone-else"],
            text_only(),
            Path::new(""),
        );
        assert_eq!((report.devices, report.added), (0, 0));
        assert_eq!(inspect(tmp.path(), TEST_DEVICE).unwrap().devices.len(), 0);
    }

    #[test]
    fn a_folder_without_a_manifest_or_of_a_newer_version_is_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let b = HistoryStore::open_in_memory().unwrap();
        let none = import_tree(&b, tmp.path(), &[], text_only(), Path::new(""), 0);
        assert!(matches!(none, Err(BackupError::NotABackup(_))));
        assert!(matches!(
            inspect(tmp.path(), TEST_DEVICE),
            Err(BackupError::NotABackup(_))
        ));
        std::fs::write(
            tmp.path().join(MANIFEST_NAME),
            br#"{"format":"arcvox-backup","version":9}"#,
        )
        .unwrap();
        let newer = import_tree(&b, tmp.path(), &[], text_only(), Path::new(""), 0);
        assert!(matches!(newer, Err(BackupError::Newer(9))));
    }

    #[test]
    fn damaged_lines_are_counted_and_the_rest_comes_in() {
        let tmp = tempfile::tempdir().unwrap();
        hand_made(
            tmp.path(),
            "dev-h",
            "dev-h",
            &[
                r#"{"origin_id":1,"ts":1790812800,"updated":5,"profile_id":"default","final_text":"a"}"#,
                "not json",
                r#"{"origin_id":2,"ts":1790812900}"#,
                r#"{"origin_id":3,"ts":1790812901,"updated":5,"profile_id":"default","final_text":"c"}"#,
            ],
        );
        let b = HistoryStore::open_in_memory().unwrap();
        let report = import(&b, tmp.path(), &["dev-h"], text_only(), Path::new(""));
        assert_eq!((report.added, report.bad_lines), (2, 2));
    }

    #[test]
    fn rows_older_than_the_retention_are_counted() {
        let tmp = tempfile::tempdir().unwrap();
        let a = store_of("dev-a", "Anna");
        let now = crate::history::now_unix();
        a.insert(utt(now - 200 * 86_400, "old", None)).unwrap();
        a.insert(utt(now - 10, "new", None)).unwrap();
        write(&a, tmp.path(), text_only(), Path::new(""));
        let devices = vec!["dev-a".to_string()];
        let b = HistoryStore::open_in_memory().unwrap();
        assert_eq!(
            import_tree(&b, tmp.path(), &devices, text_only(), Path::new(""), 90)
                .unwrap()
                .expiring,
            1
        );
        let c = HistoryStore::open_in_memory().unwrap();
        assert_eq!(
            import_tree(&c, tmp.path(), &devices, text_only(), Path::new(""), 0)
                .unwrap()
                .expiring,
            0
        );
    }

    #[test]
    fn a_name_given_here_is_kept() {
        let tmp = tempfile::tempdir().unwrap();
        let a = store_of("dev-a", "Anna");
        a.insert(utt(OCT, "one", None)).unwrap();
        write(&a, tmp.path(), text_only(), Path::new(""));
        let b = HistoryStore::open_in_memory().unwrap();
        import(&b, tmp.path(), &["dev-a"], text_only(), Path::new(""));
        b.set_device_name("dev-a", "Office").unwrap();
        import(&b, tmp.path(), &["dev-a"], text_only(), Path::new(""));
        assert_eq!(b.devices().unwrap()[1].name, "Office");
    }

    #[test]
    fn inspect_lists_devices_with_counts_and_months() {
        let tmp = tempfile::tempdir().unwrap();
        let rec = tmp.path().join("rec");
        std::fs::create_dir_all(&rec).unwrap();
        let a = HistoryStore::open_in_memory().unwrap();
        a.insert(utt(SEP, "one", Some(wav(&rec, "a.wav")))).unwrap();
        a.insert(utt(OCT, "two", None)).unwrap();
        foreign(&a, "dev-x", "Xavier", 1, OCT);
        let root = tmp.path().join("backup");
        write_tree(
            &a,
            &root,
            all(),
            Scope::AllDevices,
            &rec,
            "0.1.0",
            Some(b"{}"),
            &|_| false,
        )
        .unwrap();
        let info = inspect(&root, TEST_DEVICE).unwrap();
        assert_eq!(info.version, 1);
        let me = info.devices.iter().find(|d| d.id == TEST_DEVICE).unwrap();
        assert_eq!(
            (
                me.rows,
                me.recordings,
                me.this,
                me.has_stats,
                me.has_settings
            ),
            (2, 1, true, true, true)
        );
        assert!(me.recording_bytes > 0);
        assert_eq!(
            (me.first_month.as_deref(), me.last_month.as_deref()),
            (Some("2026-09"), Some("2026-10"))
        );
        let x = info.devices.iter().find(|d| d.id == "dev-x").unwrap();
        assert_eq!(
            (
                x.name.as_str(),
                x.rows,
                x.recordings,
                x.this,
                x.has_settings
            ),
            ("Xavier", 1, 0, false, false)
        );
        let _unused: PathBuf = root;
    }
}
