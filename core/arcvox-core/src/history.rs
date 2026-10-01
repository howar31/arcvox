//! Utterance history in SQLite. Three texts per row: raw ASR, refined, and
//! the user's final text. Recordings are optional files referenced by path.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use rusqlite::{params, Connection, OptionalExtension};

use crate::stats::{self, Stats};
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
    /// Length of the recorded audio, when known.
    pub audio_ms: Option<u64>,
    pub dict_fixes: u32,
    pub snippets: u32,
    /// The rewrite step ran, did not fail, and returned different text.
    pub llm_changed: bool,
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
    pub audio_ms: Option<u64>,
    /// Set when the dictation failed: why. Such a row has a recording and
    /// no text until it is processed again.
    pub error: Option<String>,
}

/// A dictation that failed after it was recorded.
#[derive(Debug, Clone, PartialEq)]
pub struct FailedUtterance {
    pub ts: i64,
    pub profile_id: ProfileId,
    pub app_name: Option<String>,
    pub audio_path: PathBuf,
    pub audio_ms: Option<u64>,
    pub error: String,
}

/// What processing a row's recording again produced.
#[derive(Debug, Clone, PartialEq)]
pub struct Redo {
    pub profile_id: ProfileId,
    pub raw_text: String,
    pub final_text: String,
    pub degraded: bool,
    pub asr_ms: u64,
    pub llm_ms: Option<u64>,
    pub dict_fixes: u32,
    pub snippets: u32,
    pub llm_changed: bool,
}

/// Row and recording counts of the history.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Storage {
    pub rows: u64,
    pub recordings: u64,
    pub recording_bytes: u64,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct PruneReport {
    pub rows_deleted: usize,
    pub recordings_deleted: usize,
}

/// Contract for `audio_path` on every row: it is an absolute path produced
/// by the engine under `DataDir::recordings_dir()`. `prune` and `clear_all`
/// compare paths exactly as given (via `Path::starts_with`) and never
/// canonicalize them, so callers are responsible for always storing and
/// passing absolute, already-resolved paths.
pub struct HistoryStore {
    conn: Mutex<Connection>,
}

pub(crate) const SCHEMA: &str = "
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

const STATS_SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS stats_hourly (
    hour INTEGER NOT NULL, app_name TEXT NOT NULL, profile_id TEXT NOT NULL,
    count INTEGER NOT NULL DEFAULT 0, chars INTEGER NOT NULL DEFAULT 0, audio_ms INTEGER NOT NULL DEFAULT 0,
    timed_count INTEGER NOT NULL DEFAULT 0, timed_chars INTEGER NOT NULL DEFAULT 0,
    speed_chars INTEGER NOT NULL DEFAULT 0, speed_audio_ms INTEGER NOT NULL DEFAULT 0,
    asr_ms INTEGER NOT NULL DEFAULT 0, llm_ms INTEGER NOT NULL DEFAULT 0, llm_count INTEGER NOT NULL DEFAULT 0,
    changed INTEGER NOT NULL DEFAULT 0, degraded INTEGER NOT NULL DEFAULT 0, unrewritten INTEGER NOT NULL DEFAULT 0,
    dict_fixes INTEGER NOT NULL DEFAULT 0, snippets INTEGER NOT NULL DEFAULT 0,
    han INTEGER NOT NULL DEFAULT 0, latin INTEGER NOT NULL DEFAULT 0, kana INTEGER NOT NULL DEFAULT 0,
    max_chars INTEGER NOT NULL DEFAULT 0, max_audio_ms INTEGER NOT NULL DEFAULT 0,
    max_total_ms INTEGER NOT NULL DEFAULT 0, max_cpm INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (hour, app_name, profile_id)
);
CREATE TABLE IF NOT EXISTS stats_meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
";

fn has_column(conn: &Connection, column: &str) -> rusqlite::Result<bool> {
    let mut stmt = conn.prepare("SELECT 1 FROM pragma_table_info('utterances') WHERE name = ?1")?;
    stmt.exists(params![column])
}

/// Brings a database from an earlier build up to date; safe to run at every open.
pub(crate) fn upgrade(conn: &Connection, recordings_dir: &Path) -> Result<(), HistoryError> {
    for (name, ddl) in [
        (
            "audio_ms",
            "ALTER TABLE utterances ADD COLUMN audio_ms INTEGER",
        ),
        (
            "dict_fixes",
            "ALTER TABLE utterances ADD COLUMN dict_fixes INTEGER NOT NULL DEFAULT 0",
        ),
        (
            "snippets",
            "ALTER TABLE utterances ADD COLUMN snippets INTEGER NOT NULL DEFAULT 0",
        ),
        ("error", "ALTER TABLE utterances ADD COLUMN error TEXT"),
    ] {
        if !has_column(conn, name)? {
            conn.execute_batch(ddl)?;
        }
    }
    conn.execute_batch(STATS_SCHEMA)?;
    // A failed backfill leaves the history usable; it runs again at the next open.
    if let Err(e) = stats::backfill(conn, recordings_dir) {
        tracing::warn!(error = %e, "statistics backfill failed");
    }
    Ok(())
}

/// Whether `path` lies under a non-empty `dir`. Paths are compared as given;
/// one that steps up with `..` is never inside.
pub(crate) fn inside(path: &Path, dir: &Path) -> bool {
    !dir.as_os_str().is_empty()
        && path.starts_with(dir)
        && !path
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
}

fn escape_like(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if matches!(c, '\\' | '%' | '_') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

fn insert_row(conn: &Connection, u: &NewUtterance) -> rusqlite::Result<i64> {
    conn.execute(
        "INSERT INTO utterances (ts, profile_id, app_name, raw_text, refined_text, final_text, degraded, audio_path, asr_ms, llm_ms, audio_ms, dict_fixes, snippets)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
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
            u.audio_ms.map(|v| v as i64),
            u.dict_fixes as i64,
            u.snippets as i64,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

impl HistoryStore {
    /// `recordings_dir` is where the engine keeps recordings; the first open
    /// after an upgrade reads the audio length of the files there.
    pub fn open(path: &Path, recordings_dir: &Path) -> Result<Self, HistoryError> {
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        conn.execute_batch(SCHEMA)?;
        Self::upgraded(conn, recordings_dir)
    }

    pub fn open_in_memory() -> Result<Self, HistoryError> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(SCHEMA)?;
        Self::upgraded(conn, Path::new(""))
    }

    fn upgraded(conn: Connection, recordings_dir: &Path) -> Result<Self, HistoryError> {
        upgrade(&conn, recordings_dir)?;
        Ok(Self::from_connection(conn))
    }

    pub(crate) fn from_connection(conn: Connection) -> Self {
        Self {
            conn: Mutex::new(conn),
        }
    }

    /// Writes the row and adds it to its statistics bucket in one transaction.
    /// When only the bucket write fails, the row is still stored.
    pub fn insert(&self, u: NewUtterance) -> Result<HistoryId, HistoryError> {
        Ok(self
            .insert_if(u, || true)?
            .expect("an unconditional insert is always kept"))
    }

    /// `insert`, except that `keep` is asked once the row and its bucket are
    /// written and before they are committed; `false` discards both, so a
    /// dictation withdrawn at the last moment is neither stored nor counted.
    pub fn insert_if(
        &self,
        u: NewUtterance,
        keep: impl FnOnce() -> bool,
    ) -> Result<Option<HistoryId>, HistoryError> {
        let conn = self.conn.lock().unwrap();
        let tx = conn.unchecked_transaction()?;
        let id = insert_row(&tx, &u)?;
        let added = stats::add(
            &tx,
            &stats::Sample {
                ts: u.ts,
                app_name: u.app_name.as_deref().unwrap_or(""),
                profile_id: &u.profile_id.0,
                text: &u.final_text,
                audio_ms: u.audio_ms,
                asr_ms: u.asr_ms,
                llm_ms: u.llm_ms,
                changed: u.llm_changed,
                degraded: u.degraded,
                dict_fixes: u.dict_fixes,
                snippets: u.snippets,
            },
        );
        match added {
            Ok(()) => {
                if !keep() {
                    return Ok(None);
                }
                tx.commit()?;
                Ok(Some(HistoryId(id)))
            }
            Err(e) => {
                drop(tx);
                tracing::warn!(error = %e, "statistics write failed, keeping the history row");
                if !keep() {
                    return Ok(None);
                }
                Ok(Some(HistoryId(insert_row(&conn, &u)?)))
            }
        }
    }

    /// Stores a failed dictation so its recording can be processed again.
    /// It has no text and is not counted in the statistics.
    pub fn insert_failed(&self, u: FailedUtterance) -> Result<HistoryId, HistoryError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO utterances (ts, profile_id, app_name, raw_text, refined_text, final_text, audio_path, audio_ms, error)
             VALUES (?1, ?2, ?3, '', '', '', ?4, ?5, ?6)",
            params![
                u.ts,
                u.profile_id.0,
                u.app_name,
                u.audio_path.to_string_lossy().to_string(),
                u.audio_ms.map(|v| v as i64),
                u.error,
            ],
        )?;
        Ok(HistoryId(conn.last_insert_rowid()))
    }

    /// Replaces why a failed row failed, after another attempt failed too.
    /// A row that has text is left alone.
    pub fn set_error(&self, id: HistoryId, error: &str) -> Result<(), HistoryError> {
        self.conn.lock().unwrap().execute(
            "UPDATE utterances SET error = ?1 WHERE id = ?2 AND error IS NOT NULL",
            params![error, id.0],
        )?;
        Ok(())
    }

    /// Writes the result of processing a row's recording again. A row that
    /// had failed becomes an ordinary one and is counted in the statistics
    /// now, at the time it was spoken; a row that already had text is not
    /// counted a second time. False when the row no longer exists.
    pub fn apply_redo(&self, id: HistoryId, r: &Redo) -> Result<bool, HistoryError> {
        let conn = self.conn.lock().unwrap();
        let tx = conn.unchecked_transaction()?;
        let row: Option<(i64, Option<String>, Option<i64>, bool)> = tx
            .query_row(
                "SELECT ts, app_name, audio_ms, error IS NOT NULL FROM utterances WHERE id = ?1",
                params![id.0],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()?;
        let Some((ts, app_name, audio_ms, was_failed)) = row else {
            return Ok(false);
        };
        tx.execute(
            "UPDATE utterances SET profile_id = ?1, raw_text = ?2, refined_text = ?3, final_text = ?3, degraded = ?4,
                asr_ms = ?5, llm_ms = ?6, dict_fixes = ?7, snippets = ?8, error = NULL WHERE id = ?9",
            params![
                r.profile_id.0,
                r.raw_text,
                r.final_text,
                r.degraded as i64,
                r.asr_ms as i64,
                r.llm_ms.map(|v| v as i64),
                r.dict_fixes as i64,
                r.snippets as i64,
                id.0,
            ],
        )?;
        if was_failed {
            let counted = stats::add(
                &tx,
                &stats::Sample {
                    ts,
                    app_name: app_name.as_deref().unwrap_or(""),
                    profile_id: &r.profile_id.0,
                    text: &r.final_text,
                    audio_ms: audio_ms.map(|v| v as u64),
                    asr_ms: r.asr_ms,
                    llm_ms: r.llm_ms,
                    changed: r.llm_changed,
                    degraded: r.degraded,
                    dict_fixes: r.dict_fixes,
                    snippets: r.snippets,
                },
            );
            if let Err(e) = counted {
                tracing::warn!(error = %e, "statistics write failed for a retried dictation");
            }
        }
        tx.commit()?;
        Ok(true)
    }

    pub fn set_final(&self, id: HistoryId, text: &str) -> Result<(), HistoryError> {
        self.conn.lock().unwrap().execute(
            "UPDATE utterances SET final_text = ?1 WHERE id = ?2",
            params![text, id.0],
        )?;
        Ok(())
    }

    /// Removes one row. Used to unwind a history insert for a session that
    /// was cancelled after the row was already written.
    pub fn delete(&self, id: HistoryId) -> Result<(), HistoryError> {
        self.conn
            .lock()
            .unwrap()
            .execute("DELETE FROM utterances WHERE id = ?1", params![id.0])?;
        Ok(())
    }

    /// Removes one row together with its recording, when that file is inside
    /// `recordings_dir`.
    pub fn delete_row(&self, id: HistoryId, recordings_dir: &Path) -> Result<(), HistoryError> {
        let conn = self.conn.lock().unwrap();
        remove_recording(&conn, id.0, recordings_dir)?;
        conn.execute("DELETE FROM utterances WHERE id = ?1", params![id.0])?;
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn connection(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap()
    }

    pub fn get(&self, id: HistoryId) -> Result<Option<Utterance>, HistoryError> {
        let conn = self.conn.lock().unwrap();
        let row = conn
            .query_row(&format!("{SELECT} WHERE id = ?1"), params![id.0], map_row)
            .optional()?;
        Ok(row)
    }

    pub fn list_recent(&self, limit: usize) -> Result<Vec<Utterance>, HistoryError> {
        self.list(limit, None, false)
    }

    /// Newest first. `query` matches the final and the raw text literally
    /// (case-insensitive for ASCII); `with_audio` keeps rows with a recording.
    pub fn list(
        &self,
        limit: usize,
        query: Option<&str>,
        with_audio: bool,
    ) -> Result<Vec<Utterance>, HistoryError> {
        let mut clauses: Vec<&str> = Vec::new();
        let mut values: Vec<rusqlite::types::Value> = Vec::new();
        if let Some(q) = query.map(str::trim).filter(|q| !q.is_empty()) {
            clauses.push("(final_text LIKE ?1 ESCAPE '\\' OR raw_text LIKE ?1 ESCAPE '\\')");
            values.push(format!("%{}%", escape_like(q)).into());
        }
        if with_audio {
            clauses.push("audio_path IS NOT NULL");
        }
        let filter = if clauses.is_empty() {
            String::new()
        } else {
            format!(" WHERE {}", clauses.join(" AND "))
        };
        let limit_slot = values.len() + 1;
        values.push((limit as i64).into());
        let sql = format!("{SELECT}{filter} ORDER BY ts DESC, id DESC LIMIT ?{limit_slot}");
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(rusqlite::params_from_iter(values), map_row)?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Statistics of the buckets from the hour containing `since`, with days
    /// and hours of day taken at the given UTC offset.
    pub fn stats(&self, since: i64, tz_offset_secs: i32) -> Result<Stats, HistoryError> {
        self.stats_for(since, tz_offset_secs, stats::TYPING_CHARS_PER_MIN)
    }

    /// `stats` with the typing speed the saved-time estimate compares against.
    pub fn stats_for(
        &self,
        since: i64,
        tz_offset_secs: i32,
        typing_chars_per_min: u32,
    ) -> Result<Stats, HistoryError> {
        let conn = self.conn.lock().unwrap();
        Ok(stats::query(
            &conn,
            since,
            tz_offset_secs,
            typing_chars_per_min,
        )?)
    }

    /// Empties the statistics buckets; history rows are not touched.
    pub fn reset_stats(&self) -> Result<(), HistoryError> {
        self.conn
            .lock()
            .unwrap()
            .execute("DELETE FROM stats_hourly", [])?;
        Ok(())
    }

    /// Row count and the recordings of `recordings_dir` that exist.
    pub fn storage(&self, recordings_dir: &Path) -> Result<Storage, HistoryError> {
        let conn = self.conn.lock().unwrap();
        let rows: i64 = conn.query_row("SELECT COUNT(*) FROM utterances", [], |r| r.get(0))?;
        let mut out = Storage {
            rows: rows as u64,
            ..Storage::default()
        };
        let mut stmt =
            conn.prepare("SELECT audio_path FROM utterances WHERE audio_path IS NOT NULL")?;
        let paths: Vec<String> = stmt
            .query_map([], |r| r.get(0))?
            .collect::<Result<_, _>>()?;
        for p in paths {
            let p = PathBuf::from(p);
            if inside(&p, recordings_dir) {
                if let Ok(meta) = std::fs::metadata(&p) {
                    out.recordings += 1;
                    out.recording_bytes += meta.len();
                }
            }
        }
        Ok(out)
    }

    /// The recording of a row, when it has one inside `recordings_dir`.
    pub fn recording_path(
        &self,
        id: HistoryId,
        recordings_dir: &Path,
    ) -> Result<Option<PathBuf>, HistoryError> {
        let conn = self.conn.lock().unwrap();
        recording_of(&conn, id.0, recordings_dir)
    }

    /// Removes the row's recording file and clears the reference. False when the row has none.
    pub fn delete_recording(
        &self,
        id: HistoryId,
        recordings_dir: &Path,
    ) -> Result<bool, HistoryError> {
        let conn = self.conn.lock().unwrap();
        remove_recording(&conn, id.0, recordings_dir)
    }

    /// The same for every row; returns how many files were removed.
    pub fn clear_recordings(&self, recordings_dir: &Path) -> Result<usize, HistoryError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT id FROM utterances WHERE audio_path IS NOT NULL")?;
        let ids: Vec<i64> = stmt
            .query_map([], |r| r.get(0))?
            .collect::<Result<_, _>>()?;
        drop(stmt);
        let mut removed = 0;
        for id in ids {
            if remove_recording(&conn, id, recordings_dir)? {
                removed += 1;
            }
        }
        Ok(removed)
    }

    /// Deletes rows older than `history_days` and recording files older than
    /// `recording_days` (the row keeps its text; `audio_path` is cleared
    /// once the file is actually gone). A value of 0 keeps that kind forever.
    /// Recording files in `recordings_dir` that no row refers to are removed
    /// once they are older than `recording_days`.
    ///
    /// File removal is best effort and never aborts the prune: a file that
    /// is missing, lives outside `recordings_dir`, or fails to delete (for
    /// example because the path is a directory) is simply left alone, and
    /// the loop moves on to the next row. `PruneReport::recordings_deleted`
    /// only counts files this call actually removed; a row whose recording
    /// could not be removed keeps its `audio_path` so the reference is not
    /// silently lost. The `DELETE FROM utterances` for expired rows always
    /// runs, regardless of any file errors encountered along the way.
    pub fn prune(
        &self,
        history_days: u32,
        recording_days: u32,
        recordings_dir: &Path,
    ) -> Result<PruneReport, HistoryError> {
        let now = now_unix();
        // A cutoff before every timestamp selects nothing: 0 days keeps all.
        let cutoff = |days: u32| match days {
            0 => i64::MIN,
            d => now - i64::from(d) * 86_400,
        };
        let history_cutoff = cutoff(history_days);
        let recording_cutoff = cutoff(recording_days);
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
            if p.starts_with(recordings_dir) && p.exists() && std::fs::remove_file(&p).is_ok() {
                report.recordings_deleted += 1;
            }
            // Clear the reference only once the file is confirmed gone. A
            // failed removal, or a path outside recordings_dir that still
            // exists, leaves the row pointing at the still-present file.
            if !p.exists() {
                conn.execute(
                    "UPDATE utterances SET audio_path = NULL WHERE id = ?1",
                    params![id],
                )?;
            }
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
            if p.starts_with(recordings_dir) && p.exists() && std::fs::remove_file(&p).is_ok() {
                report.recordings_deleted += 1;
            }
        }
        report.rows_deleted = conn.execute(
            "DELETE FROM utterances WHERE ts < ?1",
            params![history_cutoff],
        )?;

        if recording_days > 0 {
            report.recordings_deleted +=
                sweep_unreferenced(&conn, recordings_dir, recording_cutoff)?;
        }
        Ok(report)
    }

    /// Empties the table and best-effort deletes every `*.wav` file directly
    /// inside `recordings_dir`. The table delete always runs; a directory
    /// that cannot be read, or an individual file that cannot be removed,
    /// is skipped rather than aborting the call.
    pub fn clear_all(&self, recordings_dir: &Path) -> Result<(), HistoryError> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM utterances", [])?;
        if recordings_dir.is_dir() {
            if let Ok(entries) = std::fs::read_dir(recordings_dir) {
                for entry in entries.flatten() {
                    if entry
                        .path()
                        .extension()
                        .map(|e| e == "wav")
                        .unwrap_or(false)
                    {
                        let _ = std::fs::remove_file(entry.path());
                    }
                }
            }
        }
        Ok(())
    }
}

fn recording_of(
    conn: &Connection,
    id: i64,
    recordings_dir: &Path,
) -> Result<Option<PathBuf>, HistoryError> {
    let path: Option<String> = conn
        .query_row(
            "SELECT audio_path FROM utterances WHERE id = ?1",
            params![id],
            |r| r.get(0),
        )
        .optional()?
        .flatten();
    Ok(path
        .map(PathBuf::from)
        .filter(|p| inside(p, recordings_dir) && p.is_file()))
}

/// Removes the `.wav` files directly inside `recordings_dir` that no row
/// refers to and that were last written before `cutoff`. Returns how many.
/// A file that cannot be read or removed is skipped.
fn sweep_unreferenced(
    conn: &Connection,
    recordings_dir: &Path,
    cutoff: i64,
) -> Result<usize, HistoryError> {
    let Ok(entries) = std::fs::read_dir(recordings_dir) else {
        return Ok(0);
    };
    let mut stmt =
        conn.prepare("SELECT audio_path FROM utterances WHERE audio_path IS NOT NULL")?;
    let referenced: std::collections::HashSet<PathBuf> = stmt
        .query_map([], |r| r.get::<_, String>(0))?
        .map(|p| p.map(PathBuf::from))
        .collect::<Result<_, _>>()?;
    let mut removed = 0;
    for entry in entries.flatten() {
        let path = entry.path();
        let is_wav = path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("wav"));
        if !is_wav || referenced.contains(&path) {
            continue;
        }
        let written = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64);
        if written.is_some_and(|t| t < cutoff)
            && path.is_file()
            && std::fs::remove_file(&path).is_ok()
        {
            removed += 1;
        }
    }
    Ok(removed)
}

/// Removes a row's recording file and clears the reference. True when a
/// file was removed. A reference to a file that is already gone is cleared
/// too; a file that cannot be removed keeps its reference.
fn remove_recording(
    conn: &Connection,
    id: i64,
    recordings_dir: &Path,
) -> Result<bool, HistoryError> {
    let path: Option<String> = conn
        .query_row(
            "SELECT audio_path FROM utterances WHERE id = ?1",
            params![id],
            |r| r.get(0),
        )
        .optional()?
        .flatten();
    let Some(path) = path.map(PathBuf::from) else {
        return Ok(false);
    };
    if !inside(&path, recordings_dir) {
        return Ok(false);
    }
    let removed = path.is_file() && std::fs::remove_file(&path).is_ok();
    if !path.exists() {
        conn.execute(
            "UPDATE utterances SET audio_path = NULL WHERE id = ?1",
            params![id],
        )?;
    }
    Ok(removed)
}

const SELECT: &str = "SELECT id, ts, profile_id, app_name, raw_text, refined_text, final_text, degraded, audio_path, asr_ms, llm_ms, audio_ms, error FROM utterances";

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
        audio_ms: r.get::<_, Option<i64>>(11)?.map(|v| v as u64),
        error: r.get(12)?,
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
            audio_ms: None,
            dict_fixes: 0,
            snippets: 0,
            llm_changed: false,
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
    fn delete_removes_row() {
        let s = HistoryStore::open_in_memory().unwrap();
        let id = s.insert(new(1, None)).unwrap();
        s.delete(id).unwrap();
        assert_eq!(s.get(id).unwrap(), None);
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

    fn age(path: &Path, days: u64) {
        let when = std::time::SystemTime::now() - std::time::Duration::from_secs(days * 86_400);
        std::fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(when)
            .unwrap();
    }

    #[test]
    fn prune_removes_old_recordings_that_no_row_refers_to() {
        let tmp = tempfile::tempdir().unwrap();
        let rec = tmp.path().join("recordings");
        std::fs::create_dir_all(&rec).unwrap();
        let orphan_old = rec.join("orphan-old.wav");
        let orphan_new = rec.join("orphan-new.wav");
        let kept = rec.join("kept.wav");
        let other = rec.join("notes.txt");
        for p in [&orphan_old, &orphan_new, &kept, &other] {
            std::fs::write(p, b"x").unwrap();
        }
        age(&orphan_old, 30);
        age(&kept, 30);
        age(&other, 30);
        let s = HistoryStore::open_in_memory().unwrap();
        // The row is recent, so its (old) file is still referenced.
        s.insert(new(now_unix(), Some(kept.clone()))).unwrap();
        let report = s.prune(90, 7, &rec).unwrap();
        assert_eq!(report.recordings_deleted, 1);
        assert!(!orphan_old.exists());
        assert!(orphan_new.exists());
        assert!(kept.exists());
        assert!(other.exists());
    }

    #[test]
    fn zero_retention_keeps_everything() {
        let tmp = tempfile::tempdir().unwrap();
        let rec = tmp.path().join("recordings");
        std::fs::create_dir_all(&rec).unwrap();
        let wav = rec.join("a.wav");
        let orphan = rec.join("orphan.wav");
        std::fs::write(&wav, b"x").unwrap();
        std::fs::write(&orphan, b"x").unwrap();
        age(&orphan, 4000);
        let s = HistoryStore::open_in_memory().unwrap();
        let id = s
            .insert(new(now_unix() - 4000 * 86_400, Some(wav.clone())))
            .unwrap();
        assert_eq!(s.prune(0, 0, &rec).unwrap(), PruneReport::default());
        assert!(wav.exists() && orphan.exists());
        assert_eq!(s.get(id).unwrap().unwrap().audio_path, Some(wav.clone()));

        // Rows forever, recordings for a week: the text stays, the file goes.
        let report = s.prune(0, 7, &rec).unwrap();
        assert_eq!(
            report,
            PruneReport {
                rows_deleted: 0,
                recordings_deleted: 2
            }
        );
        assert!(s.get(id).unwrap().is_some());
    }

    #[test]
    fn saved_time_follows_the_typing_speed() {
        let s = HistoryStore::open_in_memory().unwrap();
        let mut u = new(1, None);
        u.final_text = "一二三四五六七八九十".into();
        u.audio_ms = Some(5_000);
        u.asr_ms = 0;
        u.llm_ms = None;
        s.insert(u).unwrap();
        // Ten characters: 15 s of typing at 40 a minute, 6 s at 100.
        assert_eq!(s.stats_for(0, 0, 40).unwrap().saved_ms, Some(10_000));
        assert_eq!(s.stats_for(0, 0, 100).unwrap().saved_ms, Some(1_000));
    }

    #[test]
    fn a_failed_row_is_not_counted_until_a_retry_gives_it_text() {
        let s = HistoryStore::open_in_memory().unwrap();
        let id = s
            .insert_failed(FailedUtterance {
                ts: 7200,
                profile_id: ProfileId("default".into()),
                app_name: Some("Notes".into()),
                audio_path: PathBuf::from("/rec/a.wav"),
                audio_ms: Some(4000),
                error: "timed out".into(),
            })
            .unwrap();
        let row = s.get(id).unwrap().unwrap();
        assert_eq!(
            (row.final_text.as_str(), row.error.as_deref()),
            ("", Some("timed out"))
        );
        assert_eq!(s.stats(0, 0).unwrap().totals.count, 0);

        s.set_error(id, "still down").unwrap();
        assert_eq!(
            s.get(id).unwrap().unwrap().error.as_deref(),
            Some("still down")
        );

        let redo = Redo {
            profile_id: ProfileId("mail".into()),
            raw_text: "raw".into(),
            final_text: "done".into(),
            degraded: false,
            asr_ms: 10,
            llm_ms: Some(20),
            dict_fixes: 0,
            snippets: 0,
            llm_changed: true,
        };
        assert!(s.apply_redo(id, &redo).unwrap());
        let row = s.get(id).unwrap().unwrap();
        assert_eq!(
            (
                row.final_text.as_str(),
                row.raw_text.as_str(),
                row.error,
                row.profile_id.0.as_str()
            ),
            ("done", "raw", None, "mail")
        );
        assert_eq!(s.stats(0, 0).unwrap().totals.count, 1);

        // A row that has text is replaced without being counted again.
        assert!(s
            .apply_redo(
                id,
                &Redo {
                    final_text: "again".into(),
                    ..redo.clone()
                }
            )
            .unwrap());
        assert_eq!(s.get(id).unwrap().unwrap().final_text, "again");
        assert_eq!(s.stats(0, 0).unwrap().totals.count, 1);
        s.set_error(id, "ignored").unwrap();
        assert_eq!(s.get(id).unwrap().unwrap().error, None);
        assert!(!s.apply_redo(HistoryId(999), &redo).unwrap());
    }

    #[test]
    fn insert_if_discards_the_row_and_its_count_when_not_kept() {
        let s = HistoryStore::open_in_memory().unwrap();
        assert_eq!(s.insert_if(new(1, None), || false).unwrap(), None);
        assert!(s.list_recent(10).unwrap().is_empty());
        assert_eq!(s.stats(0, 0).unwrap().totals.count, 0);
        assert!(s.insert_if(new(1, None), || true).unwrap().is_some());
        assert_eq!(s.stats(0, 0).unwrap().totals.count, 1);
    }

    #[test]
    fn prune_continues_when_a_recording_cannot_be_removed() {
        let tmp = tempfile::tempdir().unwrap();
        let rec = tmp.path().join("recordings");
        std::fs::create_dir_all(&rec).unwrap();
        // A directory named like a recording: remove_file fails on it.
        let stuck = rec.join("stuck.wav");
        std::fs::create_dir_all(&stuck).unwrap();
        std::fs::write(stuck.join("inner.txt"), b"x").unwrap();
        let old_wav = rec.join("old.wav");
        std::fs::write(&old_wav, b"x").unwrap();

        let now = now_unix();
        let ts = now - 100 * 86_400;
        let s = HistoryStore::open_in_memory().unwrap();
        s.insert(new(ts, Some(stuck.clone()))).unwrap();
        s.insert(new(ts, Some(old_wav.clone()))).unwrap();
        s.insert(new(ts, None)).unwrap();

        let report = s.prune(90, 7, &rec).unwrap();
        assert_eq!(
            report,
            PruneReport {
                rows_deleted: 3,
                recordings_deleted: 1
            }
        );
        assert!(!old_wav.exists());
        assert!(stuck.exists(), "a removal failure must not stop the prune");
    }

    #[test]
    fn prune_keeps_audio_path_for_files_outside_recordings_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let rec = tmp.path().join("recordings");
        std::fs::create_dir_all(&rec).unwrap();
        let outside = tmp.path().join("outside.wav");
        std::fs::write(&outside, b"x").unwrap();

        let now = now_unix();
        let s = HistoryStore::open_in_memory().unwrap();
        let id = s
            .insert(new(now - 10 * 86_400, Some(outside.clone())))
            .unwrap();

        let report = s.prune(90, 7, &rec).unwrap();
        assert!(outside.exists());
        let got = s.get(id).unwrap().unwrap();
        assert_eq!(
            got.audio_path,
            Some(outside),
            "reference must not be silently wiped"
        );
        assert_eq!(report.recordings_deleted, 0);
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
        let s = HistoryStore::open(&path, tmp.path()).unwrap();
        s.insert(new(1, None)).unwrap();
        assert!(path.exists());
    }

    #[test]
    fn an_older_database_gains_the_new_columns_and_tables() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA).unwrap();
        conn.execute(
            "INSERT INTO utterances (ts, profile_id, raw_text, refined_text, final_text) VALUES (7200, 'default', 'a b', 'ab。', 'ab。')",
            [],
        )
        .unwrap();
        upgrade(&conn, Path::new("/nonexistent")).unwrap();
        upgrade(&conn, Path::new("/nonexistent")).unwrap();
        let s = HistoryStore::from_connection(conn);
        let got = s.list(10, None, false).unwrap();
        assert_eq!(got[0].audio_ms, None);
        assert_eq!(s.stats(0, 0).unwrap().totals.count, 1);
    }

    #[test]
    fn a_failed_bucket_write_keeps_the_row() {
        let s = HistoryStore::open_in_memory().unwrap();
        s.conn
            .lock()
            .unwrap()
            .execute_batch("DROP TABLE stats_hourly")
            .unwrap();
        let id = s.insert(new(5, None)).unwrap();
        assert!(s.get(id).unwrap().is_some());
    }

    #[test]
    fn search_matches_wildcards_literally() {
        let s = HistoryStore::open_in_memory().unwrap();
        for text in ["100% done", "a_b", "plain", "it's"] {
            let mut u = new(100, None);
            u.final_text = text.into();
            s.insert(u).unwrap();
        }
        let hit = |q: &str| {
            s.list(10, Some(q), false)
                .unwrap()
                .iter()
                .map(|u| u.final_text.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(hit("%"), vec!["100% done"]);
        assert_eq!(hit("_"), vec!["a_b"]);
        assert_eq!(hit("'"), vec!["it's"]);
        assert_eq!(
            hit("  "),
            s.list(10, None, false)
                .unwrap()
                .iter()
                .map(|u| u.final_text.clone())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn recordings_are_listed_measured_and_removed_only_inside_their_directory() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let s = HistoryStore::open_in_memory().unwrap();
        let inside = dir.path().join("a.wav");
        let stray = outside.path().join("b.wav");
        std::fs::write(&inside, [0u8; 100]).unwrap();
        std::fs::write(&stray, [0u8; 50]).unwrap();
        let a = s.insert(new(100, Some(inside.clone()))).unwrap();
        let b = s.insert(new(200, Some(stray.clone()))).unwrap();
        s.insert(new(300, None)).unwrap();
        assert_eq!(s.list(10, None, true).unwrap().len(), 2);
        assert_eq!(
            s.storage(dir.path()).unwrap(),
            Storage {
                rows: 3,
                recordings: 1,
                recording_bytes: 100
            }
        );
        assert_eq!(s.recording_path(b, dir.path()).unwrap(), None);
        assert!(!s.delete_recording(b, dir.path()).unwrap());
        assert!(stray.exists());
        assert!(s.delete_recording(a, dir.path()).unwrap());
        assert!(!inside.exists());
        assert_eq!(s.get(a).unwrap().unwrap().audio_path, None);
        assert_eq!(s.clear_recordings(dir.path()).unwrap(), 0);
    }

    #[test]
    fn clear_recordings_removes_every_file_inside_the_directory() {
        let dir = tempfile::tempdir().unwrap();
        let s = HistoryStore::open_in_memory().unwrap();
        for (i, name) in ["a.wav", "b.wav"].iter().enumerate() {
            let p = dir.path().join(name);
            std::fs::write(&p, b"x").unwrap();
            s.insert(new(i as i64, Some(p))).unwrap();
        }
        assert_eq!(s.clear_recordings(dir.path()).unwrap(), 2);
        assert!(s.list(10, None, true).unwrap().is_empty());
        assert_eq!(s.list(10, None, false).unwrap().len(), 2);
    }
    #[test]
    fn deleting_a_row_removes_its_recording_inside_the_directory_only() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let s = HistoryStore::open_in_memory().unwrap();
        let inside_file = dir.path().join("a.wav");
        let stray = outside.path().join("b.wav");
        std::fs::write(&inside_file, [0u8; 8]).unwrap();
        std::fs::write(&stray, [0u8; 8]).unwrap();
        let a = s.insert(new(100, Some(inside_file.clone()))).unwrap();
        let b = s.insert(new(200, Some(stray.clone()))).unwrap();
        s.delete_row(a, dir.path()).unwrap();
        s.delete_row(b, dir.path()).unwrap();
        assert!(!inside_file.exists());
        assert!(stray.exists());
        assert_eq!(s.list(10, None, false).unwrap().len(), 0);
    }

    #[test]
    fn a_path_that_climbs_out_of_the_directory_is_not_inside_it() {
        let dir = Path::new("/data/recordings");
        assert!(inside(Path::new("/data/recordings/a.wav"), dir));
        assert!(!inside(Path::new("/data/recordings/../config.json"), dir));
        assert!(!inside(Path::new("/data/other/a.wav"), dir));
        assert!(!inside(Path::new("/data/recordings/a.wav"), Path::new("")));
    }

    #[test]
    fn a_reference_to_a_missing_recording_is_cleared_and_the_rest_are_still_removed() {
        let dir = tempfile::tempdir().unwrap();
        let s = HistoryStore::open_in_memory().unwrap();
        let present = dir.path().join("b.wav");
        std::fs::write(&present, [0u8; 8]).unwrap();
        let gone = s.insert(new(100, Some(dir.path().join("a.wav")))).unwrap();
        s.insert(new(200, Some(present.clone()))).unwrap();
        assert_eq!(s.clear_recordings(dir.path()).unwrap(), 1);
        assert!(!present.exists());
        assert_eq!(s.get(gone).unwrap().unwrap().audio_path, None);
        assert_eq!(s.list(10, None, true).unwrap().len(), 0);
    }
}
