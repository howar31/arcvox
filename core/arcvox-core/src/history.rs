//! Utterance history in SQLite. Three texts per row: raw ASR, refined, and
//! the user's final text. Recordings are optional files referenced by path.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use rusqlite::{params, Connection, OptionalExtension};

use crate::types::{HistoryId, ProfileId};

#[derive(Debug, thiserror::Error)]
pub enum HistoryError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone, PartialEq)]
pub struct NewUtterance {
    pub ts: i64,
    pub profile_id: ProfileId,
    pub app_name: Option<String>,
    pub raw_text: String,
    pub refined_text: String,
    pub final_text: String,
    pub degraded: bool,
    pub audio_path: Option<PathBuf>,
    pub asr_ms: u64,
    pub llm_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Utterance {
    pub id: HistoryId,
    pub ts: i64,
    pub profile_id: ProfileId,
    pub app_name: Option<String>,
    pub raw_text: String,
    pub refined_text: String,
    pub final_text: String,
    pub degraded: bool,
    pub audio_path: Option<PathBuf>,
    pub asr_ms: u64,
    pub llm_ms: Option<u64>,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct PruneReport {
    pub rows_deleted: usize,
    pub recordings_deleted: usize,
}

pub struct HistoryStore {
    conn: Mutex<Connection>,
}

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS utterances (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    ts INTEGER NOT NULL,
    profile_id TEXT NOT NULL,
    app_name TEXT,
    raw_text TEXT NOT NULL,
    refined_text TEXT NOT NULL,
    final_text TEXT NOT NULL,
    degraded INTEGER NOT NULL DEFAULT 0,
    audio_path TEXT,
    asr_ms INTEGER NOT NULL DEFAULT 0,
    llm_ms INTEGER
);
CREATE INDEX IF NOT EXISTS utterances_ts ON utterances(ts);
";

pub fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

impl HistoryStore {
    pub fn open(path: &Path) -> Result<Self, HistoryError> {
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn open_in_memory() -> Result<Self, HistoryError> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn insert(&self, u: NewUtterance) -> Result<HistoryId, HistoryError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO utterances (ts, profile_id, app_name, raw_text, refined_text, final_text, degraded, audio_path, asr_ms, llm_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                u.ts,
                u.profile_id.0,
                u.app_name,
                u.raw_text,
                u.refined_text,
                u.final_text,
                u.degraded as i64,
                u.audio_path.as_ref().map(|p| p.to_string_lossy().to_string()),
                u.asr_ms as i64,
                u.llm_ms.map(|v| v as i64),
            ],
        )?;
        Ok(HistoryId(conn.last_insert_rowid()))
    }

    pub fn set_final(&self, id: HistoryId, text: &str) -> Result<(), HistoryError> {
        self.conn.lock().unwrap().execute(
            "UPDATE utterances SET final_text = ?1 WHERE id = ?2",
            params![text, id.0],
        )?;
        Ok(())
    }

    pub fn get(&self, id: HistoryId) -> Result<Option<Utterance>, HistoryError> {
        let conn = self.conn.lock().unwrap();
        let row = conn
            .query_row(&format!("{SELECT} WHERE id = ?1"), params![id.0], map_row)
            .optional()?;
        Ok(row)
    }

    pub fn list_recent(&self, limit: usize) -> Result<Vec<Utterance>, HistoryError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(&format!("{SELECT} ORDER BY ts DESC, id DESC LIMIT ?1"))?;
        let rows = stmt.query_map(params![limit as i64], map_row)?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Deletes rows older than `history_days` and recording files older than
    /// `recording_days` (the row keeps its text; `audio_path` is cleared).
    pub fn prune(
        &self,
        history_days: u32,
        recording_days: u32,
        recordings_dir: &Path,
    ) -> Result<PruneReport, HistoryError> {
        let now = now_unix();
        let history_cutoff = now - history_days as i64 * 86_400;
        let recording_cutoff = now - recording_days as i64 * 86_400;
        let conn = self.conn.lock().unwrap();
        let mut report = PruneReport::default();

        let mut stmt = conn.prepare(
            "SELECT id, audio_path FROM utterances WHERE audio_path IS NOT NULL AND ts < ?1",
        )?;
        let old: Vec<(i64, String)> = stmt
            .query_map(params![recording_cutoff], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<Result<_, _>>()?;
        drop(stmt);
        for (id, path) in old {
            let p = PathBuf::from(&path);
            if p.starts_with(recordings_dir) && p.exists() {
                std::fs::remove_file(&p)?;
                report.recordings_deleted += 1;
            }
            conn.execute(
                "UPDATE utterances SET audio_path = NULL WHERE id = ?1",
                params![id],
            )?;
        }

        let mut stmt = conn.prepare(
            "SELECT audio_path FROM utterances WHERE audio_path IS NOT NULL AND ts < ?1",
        )?;
        let doomed: Vec<String> = stmt
            .query_map(params![history_cutoff], |r| r.get(0))?
            .collect::<Result<_, _>>()?;
        drop(stmt);
        for path in doomed {
            let p = PathBuf::from(&path);
            if p.starts_with(recordings_dir) && p.exists() {
                std::fs::remove_file(&p)?;
                report.recordings_deleted += 1;
            }
        }
        report.rows_deleted = conn.execute(
            "DELETE FROM utterances WHERE ts < ?1",
            params![history_cutoff],
        )?;
        Ok(report)
    }

    pub fn clear_all(&self, recordings_dir: &Path) -> Result<(), HistoryError> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM utterances", [])?;
        if recordings_dir.is_dir() {
            for entry in std::fs::read_dir(recordings_dir)? {
                let entry = entry?;
                if entry
                    .path()
                    .extension()
                    .map(|e| e == "wav")
                    .unwrap_or(false)
                {
                    std::fs::remove_file(entry.path())?;
                }
            }
        }
        Ok(())
    }
}

const SELECT: &str = "SELECT id, ts, profile_id, app_name, raw_text, refined_text, final_text, degraded, audio_path, asr_ms, llm_ms FROM utterances";

fn map_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Utterance> {
    Ok(Utterance {
        id: HistoryId(r.get(0)?),
        ts: r.get(1)?,
        profile_id: ProfileId(r.get(2)?),
        app_name: r.get(3)?,
        raw_text: r.get(4)?,
        refined_text: r.get(5)?,
        final_text: r.get(6)?,
        degraded: r.get::<_, i64>(7)? != 0,
        audio_path: r.get::<_, Option<String>>(8)?.map(PathBuf::from),
        asr_ms: r.get::<_, i64>(9)? as u64,
        llm_ms: r.get::<_, Option<i64>>(10)?.map(|v| v as u64),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn new(ts: i64, audio: Option<PathBuf>) -> NewUtterance {
        NewUtterance {
            ts,
            profile_id: ProfileId("default".into()),
            app_name: Some("TextEdit".into()),
            raw_text: "raw".into(),
            refined_text: "refined".into(),
            final_text: "refined".into(),
            degraded: false,
            audio_path: audio,
            asr_ms: 120,
            llm_ms: Some(300),
        }
    }

    #[test]
    fn insert_get_set_final_list() {
        let s = HistoryStore::open_in_memory().unwrap();
        let a = s.insert(new(100, None)).unwrap();
        let b = s.insert(new(200, None)).unwrap();
        s.set_final(a, "edited").unwrap();
        let got = s.get(a).unwrap().unwrap();
        assert_eq!(got.final_text, "edited");
        assert_eq!(got.raw_text, "raw");
        assert_eq!(got.llm_ms, Some(300));
        let recent = s.list_recent(10).unwrap();
        assert_eq!(recent.iter().map(|u| u.id).collect::<Vec<_>>(), vec![b, a]);
        assert_eq!(s.get(HistoryId(999)).unwrap(), None);
    }

    #[test]
    fn prune_removes_old_rows_and_old_recordings_only_inside_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let rec = tmp.path().join("recordings");
        std::fs::create_dir_all(&rec).unwrap();
        let old_wav = rec.join("old.wav");
        let mid_wav = rec.join("mid.wav");
        let outside = tmp.path().join("outside.wav");
        for p in [&old_wav, &mid_wav, &outside] {
            std::fs::write(p, b"x").unwrap();
        }
        let now = now_unix();
        let s = HistoryStore::open_in_memory().unwrap();
        s.insert(new(now - 100 * 86_400, Some(old_wav.clone())))
            .unwrap();
        let mid = s
            .insert(new(now - 10 * 86_400, Some(mid_wav.clone())))
            .unwrap();
        let fresh = s.insert(new(now, Some(outside.clone()))).unwrap();
        let report = s.prune(90, 7, &rec).unwrap();
        assert_eq!(
            report,
            PruneReport {
                rows_deleted: 1,
                recordings_deleted: 2
            }
        );
        assert!(!old_wav.exists());
        assert!(!mid_wav.exists());
        assert!(
            outside.exists(),
            "files outside the recordings dir are never touched"
        );
        assert_eq!(
            s.get(mid).unwrap().unwrap().audio_path,
            None,
            "recording pruned but text kept"
        );
        assert!(s.get(fresh).unwrap().is_some());
        assert_eq!(s.list_recent(10).unwrap().len(), 2);
    }

    #[test]
    fn clear_all_empties_table_and_wavs() {
        let tmp = tempfile::tempdir().unwrap();
        let rec = tmp.path().join("recordings");
        std::fs::create_dir_all(&rec).unwrap();
        std::fs::write(rec.join("a.wav"), b"x").unwrap();
        std::fs::write(rec.join("keep.txt"), b"x").unwrap();
        let s = HistoryStore::open_in_memory().unwrap();
        s.insert(new(1, None)).unwrap();
        s.clear_all(&rec).unwrap();
        assert!(s.list_recent(10).unwrap().is_empty());
        assert!(!rec.join("a.wav").exists());
        assert!(rec.join("keep.txt").exists());
    }

    #[test]
    fn open_on_disk_creates_file() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("history.sqlite");
        let s = HistoryStore::open(&path).unwrap();
        s.insert(new(1, None)).unwrap();
        assert!(path.exists());
    }
}
