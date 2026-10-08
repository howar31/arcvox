//! Utterance history in SQLite. Three texts per row: raw ASR, refined, and
//! the user's final text. Recordings are optional files referenced by path.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use rusqlite::{params, Connection, OptionalExtension};

use crate::backup::format::{AttemptLine, DeviceMeta, HistoryLine, StatsLine};
use crate::stats::{self, Stats};
use crate::types::{ErrorCode, HistoryId, ProfileId};

#[derive(Debug, thiserror::Error)]
pub enum HistoryError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("the rows of this device are not removed as a device")]
    OwnDevice,
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
    /// The kind of failure, as `ErrorCode::name`; None for rows stored
    /// before the kind was kept.
    pub error_code: Option<String>,
    /// How many times the recording was processed again.
    pub redone: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct AttemptId(pub i64);

/// One result a row had: what processing its recording gave, or how it
/// failed. The chosen one is what the row shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attempt {
    pub id: AttemptId,
    pub utterance_id: HistoryId,
    pub ts: i64,
    pub profile_id: ProfileId,
    pub asr_label: Option<String>,
    pub llm_label: Option<String>,
    pub raw_text: String,
    pub final_text: String,
    pub degraded: bool,
    pub asr_ms: u64,
    pub llm_ms: Option<u64>,
    pub dict_fixes: u32,
    pub snippets: u32,
    pub llm_changed: bool,
    pub error: Option<String>,
    pub error_code: Option<String>,
    /// The result the row had before it was first processed again.
    pub origin: bool,
    pub chosen: bool,
}

/// A result to record for a row: `Ok` with the engines that made it, or
/// the failure of the run.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    Done {
        redo: Redo,
        asr_label: String,
        llm_label: Option<String>,
    },
    Failed {
        profile_id: ProfileId,
        code: ErrorCode,
        text: String,
    },
}

/// A dictation that failed after it was recorded.
#[derive(Debug, Clone, PartialEq)]
pub struct FailedUtterance {
    pub ts: i64,
    pub profile_id: ProfileId,
    pub app_name: Option<String>,
    pub audio_path: PathBuf,
    pub audio_ms: Option<u64>,
    pub code: ErrorCode,
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

/// What moving the recordings to another folder did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MoveReport {
    pub moved: usize,
    /// Files that could not be moved; they and their rows are unchanged.
    pub left: usize,
}

/// What bringing a device's rows of a backup in did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Merged {
    pub added: usize,
    pub updated: usize,
    pub skipped: usize,
    pub expiring: usize,
}

/// A device whose rows are in this store.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct DeviceRow {
    pub id: String,
    pub name: String,
    pub platform: String,
    pub rows: u64,
    /// When its newest row was spoken.
    pub last_ts: Option<i64>,
    /// The device this store belongs to.
    pub this: bool,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct PruneReport {
    pub rows_deleted: usize,
    pub recordings_deleted: usize,
}

/// Contract for `audio_path` on every row: the column holds only the file
/// name of the recording; the file lives directly inside the recordings
/// folder of the moment. Rows read back carry the absolute path under the
/// store's current folder (`set_recordings_dir`), so a folder that is
/// renamed or moved by hand still finds its files. Callers pass any path;
/// only its file name is kept.
pub struct HistoryStore {
    conn: Mutex<Connection>,
    device_id: String,
    recordings_dir: Mutex<PathBuf>,
}

/// The file name a row stores for a recording.
fn name_of(path: &Path) -> Option<String> {
    path.file_name().map(|n| n.to_string_lossy().to_string())
}

/// Where a stored name points under `dir`. A stored value that is still a
/// path (written before names were kept) is reduced to its name first.
fn resolve(name: &str, dir: &Path) -> PathBuf {
    let name = Path::new(name)
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_default();
    dir.join(name)
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
CREATE TABLE IF NOT EXISTS attempts (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    utterance_id INTEGER NOT NULL REFERENCES utterances(id) ON DELETE CASCADE,
    ts INTEGER NOT NULL,
    profile_id TEXT NOT NULL,
    asr_label TEXT,
    llm_label TEXT,
    raw_text TEXT NOT NULL DEFAULT '',
    final_text TEXT NOT NULL DEFAULT '',
    degraded INTEGER NOT NULL DEFAULT 0,
    asr_ms INTEGER NOT NULL DEFAULT 0,
    llm_ms INTEGER,
    dict_fixes INTEGER NOT NULL DEFAULT 0,
    snippets INTEGER NOT NULL DEFAULT 0,
    llm_changed INTEGER NOT NULL DEFAULT 0,
    error TEXT,
    error_code TEXT,
    origin INTEGER NOT NULL DEFAULT 0,
    chosen INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS attempts_utterance ON attempts(utterance_id);
";

const STATS_SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS stats_hourly (
    device_id TEXT NOT NULL, updated INTEGER NOT NULL DEFAULT 0,
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
    PRIMARY KEY (device_id, hour, app_name, profile_id)
);
CREATE TABLE IF NOT EXISTS stats_meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
";

const DEVICES_SCHEMA: &str = "
CREATE UNIQUE INDEX IF NOT EXISTS utterances_origin ON utterances(device_id, origin_id);
CREATE TABLE IF NOT EXISTS devices (id TEXT PRIMARY KEY, name TEXT NOT NULL, platform TEXT NOT NULL DEFAULT '');
";

/// The device id of a store opened in memory.
pub const TEST_DEVICE: &str = "test-device";

/// The installation a store belongs to: its rows and statistics carry `id`.
#[derive(Debug, Clone, Copy)]
pub struct DeviceRef<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub platform: &'a str,
}

fn has_column(conn: &Connection, table: &str, column: &str) -> rusqlite::Result<bool> {
    let mut stmt = conn.prepare("SELECT 1 FROM pragma_table_info(?1) WHERE name = ?2")?;
    stmt.exists(params![table, column])
}

/// Statistics of a build that kept one set of buckets: they become this
/// device's.
fn give_stats_to_device(conn: &Connection, device: &str) -> rusqlite::Result<()> {
    let has_table = conn
        .prepare("SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'stats_hourly'")?
        .exists([])?;
    if !has_table || has_column(conn, "stats_hourly", "device_id")? {
        return Ok(());
    }
    let columns = stats::COUNTERS.join(", ");
    let tx = conn.unchecked_transaction()?;
    tx.execute_batch("ALTER TABLE stats_hourly RENAME TO stats_hourly_old")?;
    tx.execute_batch(STATS_SCHEMA)?;
    tx.execute(
        &format!(
            "INSERT INTO stats_hourly (device_id, updated, hour, app_name, profile_id, {columns})
             SELECT ?1, hour * 1000, hour, app_name, profile_id, {columns} FROM stats_hourly_old"
        ),
        params![device],
    )?;
    tx.execute_batch("DROP TABLE stats_hourly_old")?;
    tx.commit()
}

/// Brings a database from an earlier build up to date; safe to run at every open.
pub(crate) fn upgrade(
    conn: &Connection,
    recordings_dir: &Path,
    device: &DeviceRef<'_>,
) -> Result<(), HistoryError> {
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
        (
            "error_code",
            "ALTER TABLE utterances ADD COLUMN error_code TEXT",
        ),
        (
            "device_id",
            "ALTER TABLE utterances ADD COLUMN device_id TEXT",
        ),
        (
            "origin_id",
            "ALTER TABLE utterances ADD COLUMN origin_id INTEGER",
        ),
        (
            "updated",
            "ALTER TABLE utterances ADD COLUMN updated INTEGER NOT NULL DEFAULT 0",
        ),
    ] {
        if !has_column(conn, "utterances", name)? {
            conn.execute_batch(ddl)?;
        }
    }
    // Rows written before rows carried a device belong to this one.
    conn.execute(
        "UPDATE utterances SET device_id = ?1, origin_id = id, updated = ts * 1000 WHERE device_id IS NULL",
        params![device.id],
    )?;
    conn.execute_batch(DEVICES_SCHEMA)?;
    conn.execute(
        "INSERT INTO devices (id, name, platform) VALUES (?1, ?2, ?3)
         ON CONFLICT (id) DO UPDATE SET name = excluded.name, platform = excluded.platform",
        params![device.id, device.name, device.platform],
    )?;
    give_stats_to_device(conn, device.id)?;
    // Earlier builds stored the recording's absolute path; only the name is
    // kept now, so a folder renamed or moved by hand keeps its recordings.
    keep_names_only(conn)?;
    conn.execute_batch(STATS_SCHEMA)?;
    // A failed backfill leaves the history usable; it runs again at the next open.
    if let Err(e) = stats::backfill(conn, recordings_dir, device.id) {
        tracing::warn!(error = %e, "statistics backfill failed");
    }
    Ok(())
}

/// Reduces every stored recording path to its file name.
fn keep_names_only(conn: &Connection) -> Result<(), HistoryError> {
    let mut stmt = conn.prepare(
        "SELECT id, audio_path FROM utterances WHERE audio_path LIKE '%/%' OR audio_path LIKE '%\\%'",
    )?;
    let rows: Vec<(i64, String)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<_, _>>()?;
    drop(stmt);
    for (id, path) in rows {
        let name = Path::new(&path)
            .file_name()
            .map(|n| n.to_string_lossy().to_string());
        conn.execute(
            "UPDATE utterances SET audio_path = ?1 WHERE id = ?2",
            params![name, id],
        )?;
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

/// Marks the row just inserted as this device's own: its id here is its id
/// everywhere.
fn own_last_row(conn: &Connection) -> rusqlite::Result<i64> {
    let id = conn.last_insert_rowid();
    conn.execute(
        "UPDATE utterances SET origin_id = id WHERE id = ?1",
        params![id],
    )?;
    Ok(id)
}

fn insert_row(conn: &Connection, u: &NewUtterance, device: &str) -> rusqlite::Result<i64> {
    conn.execute(
        "INSERT INTO utterances (ts, profile_id, app_name, raw_text, refined_text, final_text, degraded, audio_path, asr_ms, llm_ms, audio_ms, dict_fixes, snippets, device_id, updated)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
        params![
            u.ts,
            u.profile_id.0,
            u.app_name,
            u.raw_text,
            u.refined_text,
            u.final_text,
            u.degraded as i64,
            u.audio_path.as_deref().and_then(name_of),
            u.asr_ms as i64,
            u.llm_ms.map(|v| v as i64),
            u.audio_ms.map(|v| v as i64),
            u.dict_fixes as i64,
            u.snippets as i64,
            device,
            now_ms(),
        ],
    )?;
    own_last_row(conn)
}

pub fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Milliseconds since the epoch: when a row or a statistics bucket last changed.
pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

impl HistoryStore {
    /// `recordings_dir` is where the engine keeps recordings; the first open
    /// after an upgrade reads the audio length of the files there.
    pub fn open(
        path: &Path,
        recordings_dir: &Path,
        device: &DeviceRef<'_>,
    ) -> Result<Self, HistoryError> {
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        conn.execute_batch(SCHEMA)?;
        Self::upgraded(conn, recordings_dir, device)
    }

    /// The folder the rows' recordings are reported under from now on.
    pub fn set_recordings_dir(&self, dir: &Path) {
        *self.recordings_dir.lock().unwrap() = dir.to_path_buf();
    }

    fn dir(&self) -> PathBuf {
        self.recordings_dir.lock().unwrap().clone()
    }

    /// Gives a row the absolute path of its recording.
    fn locate(&self, mut u: Utterance) -> Utterance {
        if let Some(p) = u.audio_path.take() {
            u.audio_path = Some(resolve(&p.to_string_lossy(), &self.dir()));
        }
        u
    }

    /// Opens an existing store for reading only: no schema upgrade, no
    /// device row, every write refused by the database. For tools that look
    /// at an installation's history while the application may be running.
    pub fn open_read_only(path: &Path, recordings_dir: &Path) -> Result<Self, HistoryError> {
        let conn = Connection::open_with_flags(
            path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        let device: String = conn
            .query_row("SELECT id FROM devices ORDER BY rowid LIMIT 1", [], |r| {
                r.get(0)
            })
            .unwrap_or_default();
        let store = Self::from_connection(conn, &device);
        store.set_recordings_dir(recordings_dir);
        Ok(store)
    }

    pub fn open_in_memory() -> Result<Self, HistoryError> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch("PRAGMA foreign_keys=ON;")?;
        conn.execute_batch(SCHEMA)?;
        let device = DeviceRef {
            id: TEST_DEVICE,
            name: "Test",
            platform: "",
        };
        Self::upgraded(conn, Path::new(""), &device)
    }

    fn upgraded(
        conn: Connection,
        recordings_dir: &Path,
        device: &DeviceRef<'_>,
    ) -> Result<Self, HistoryError> {
        upgrade(&conn, recordings_dir, device)?;
        let store = Self::from_connection(conn, device.id);
        store.set_recordings_dir(recordings_dir);
        Ok(store)
    }

    pub(crate) fn from_connection(conn: Connection, device_id: &str) -> Self {
        Self {
            conn: Mutex::new(conn),
            device_id: device_id.to_string(),
            recordings_dir: Mutex::new(PathBuf::new()),
        }
    }

    /// The device whose rows this store writes.
    pub fn device_id(&self) -> &str {
        &self.device_id
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
        self.insert_replacing(u, None, keep)
    }

    /// `insert_if`, and when the row is kept, the row `replaces` goes in the
    /// same step: the one that held the recording while the dictation was
    /// processed.
    pub fn insert_replacing(
        &self,
        u: NewUtterance,
        replaces: Option<HistoryId>,
        keep: impl FnOnce() -> bool,
    ) -> Result<Option<HistoryId>, HistoryError> {
        let conn = self.conn.lock().unwrap();
        let tx = conn.unchecked_transaction()?;
        if let Some(old) = replaces {
            tx.execute("DELETE FROM utterances WHERE id = ?1", params![old.0])?;
        }
        let id = insert_row(&tx, &u, &self.device_id)?;
        let added = stats::add(
            &tx,
            &stats::Sample {
                device_id: &self.device_id,
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
                if let Some(old) = replaces {
                    conn.execute("DELETE FROM utterances WHERE id = ?1", params![old.0])?;
                }
                Ok(Some(HistoryId(insert_row(&conn, &u, &self.device_id)?)))
            }
        }
    }

    /// Stores a failed dictation so its recording can be processed again.
    /// It has no text and is not counted in the statistics.
    pub fn insert_failed(&self, u: FailedUtterance) -> Result<HistoryId, HistoryError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO utterances (ts, profile_id, app_name, raw_text, refined_text, final_text, audio_path, audio_ms, error, error_code, device_id, updated)
             VALUES (?1, ?2, ?3, '', '', '', ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                u.ts,
                u.profile_id.0,
                u.app_name,
                name_of(&u.audio_path),
                u.audio_ms.map(|v| v as i64),
                u.error,
                u.code.name(),
                self.device_id,
                now_ms(),
            ],
        )?;
        Ok(HistoryId(own_last_row(&conn)?))
    }

    /// Replaces why a failed row failed, after another attempt failed too.
    /// A row that has text is left alone.
    pub fn set_error(
        &self,
        id: HistoryId,
        code: ErrorCode,
        error: &str,
    ) -> Result<(), HistoryError> {
        self.conn.lock().unwrap().execute(
            "UPDATE utterances SET error = ?1, error_code = ?2, updated = ?4 WHERE id = ?3 AND error IS NOT NULL",
            params![error, code.name(), id.0, now_ms()],
        )?;
        Ok(())
    }

    /// The length of a failed row's recording, after the file was replaced.
    pub fn set_audio_ms(&self, id: HistoryId, audio_ms: Option<u64>) -> Result<(), HistoryError> {
        self.conn.lock().unwrap().execute(
            "UPDATE utterances SET audio_ms = ?1, updated = ?3 WHERE id = ?2",
            params![audio_ms.map(|v| v as i64), id.0, now_ms()],
        )?;
        Ok(())
    }

    /// Removes the row that holds the recording at `path` for a dictation
    /// still being processed. True when there was one; a row that reached a
    /// result, or failed for a reason of its own, is left alone.
    pub fn discard_interrupted(&self, path: &Path) -> Result<bool, HistoryError> {
        let removed = self.conn.lock().unwrap().execute(
            "DELETE FROM utterances WHERE audio_path = ?1 AND error_code = ?2",
            params![name_of(path), ErrorCode::Interrupted.name()],
        )?;
        Ok(removed > 0)
    }

    /// Moves every recording directly inside `from` into `to`, which is the
    /// rows' folder from then on; rows hold names only and need no change.
    /// A file that cannot be moved, or whose name is taken in `to`, stays
    /// where it is and is counted in `left`; its row then names the file of
    /// that name in `to`, or nothing when there is none.
    pub fn move_recordings(&self, from: &Path, to: &Path) -> Result<MoveReport, HistoryError> {
        let mut report = MoveReport::default();
        if from == to {
            return Ok(report);
        }
        let Ok(entries) = std::fs::read_dir(from) else {
            self.set_recordings_dir(to);
            return Ok(report);
        };
        std::fs::create_dir_all(to)?;
        for entry in entries.flatten() {
            let old = entry.path();
            let is_wav = old
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("wav"));
            let Some(name) = old.file_name().filter(|_| is_wav && old.is_file()) else {
                continue;
            };
            let new = to.join(name);
            // A rename works within one volume; across volumes the file is copied.
            let moved = !new.exists()
                && (std::fs::rename(&old, &new).is_ok()
                    || (std::fs::copy(&old, &new).is_ok() && std::fs::remove_file(&old).is_ok()));
            if !moved {
                report.left += 1;
                continue;
            }
            report.moved += 1;
        }
        self.set_recordings_dir(to);
        Ok(report)
    }

    /// Writes the result of processing a row's recording again. A row that
    /// had failed becomes an ordinary one and is counted in the statistics
    /// now, at the time it was spoken; a row that already had text is not
    /// counted a second time. False when the row no longer exists.
    /// Makes a result the row's text, profile and timings. True when the
    /// row exists. A row that had failed is counted in the statistics now.
    pub fn apply_redo(&self, id: HistoryId, r: &Redo) -> Result<bool, HistoryError> {
        let conn = self.conn.lock().unwrap();
        let tx = conn.unchecked_transaction()?;
        let applied = apply_in(&tx, &self.device_id, id, r)?;
        tx.commit()?;
        Ok(applied)
    }

    /// Records a result of processing the row's recording again. Before the
    /// first one, the row's own result or failure is kept as the origin
    /// attempt. With `choose`, or for a row that had failed, the result
    /// becomes the row's. None when the row is gone.
    pub fn record_attempt(
        &self,
        id: HistoryId,
        outcome: &Outcome,
        choose: bool,
    ) -> Result<Option<Attempt>, HistoryError> {
        let conn = self.conn.lock().unwrap();
        let tx = conn.unchecked_transaction()?;
        let Some(row) = tx
            .query_row(&format!("{SELECT} WHERE id = ?1"), params![id.0], map_row)
            .optional()?
        else {
            return Ok(None);
        };
        let has_attempts: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM attempts WHERE utterance_id = ?1)",
            params![id.0],
            |r| r.get(0),
        )?;
        let was_failed = row.error.is_some();
        if !has_attempts {
            insert_attempt(
                &tx,
                id.0,
                &AttemptLine {
                    ts: row.ts,
                    profile_id: row.profile_id.0.clone(),
                    asr_label: None,
                    llm_label: None,
                    raw_text: row.raw_text.clone(),
                    final_text: row.final_text.clone(),
                    degraded: row.degraded,
                    asr_ms: row.asr_ms,
                    llm_ms: row.llm_ms,
                    dict_fixes: 0,
                    snippets: 0,
                    llm_changed: false,
                    error: row.error.clone(),
                    error_code: row.error_code.clone(),
                    origin: true,
                    chosen: !was_failed,
                },
            )?;
        }
        let ts = now_unix();
        let line = match outcome {
            Outcome::Done {
                redo,
                asr_label,
                llm_label,
            } => AttemptLine {
                ts,
                profile_id: redo.profile_id.0.clone(),
                asr_label: Some(asr_label.clone()),
                llm_label: llm_label.clone(),
                raw_text: redo.raw_text.clone(),
                final_text: redo.final_text.clone(),
                degraded: redo.degraded,
                asr_ms: redo.asr_ms,
                llm_ms: redo.llm_ms,
                dict_fixes: redo.dict_fixes,
                snippets: redo.snippets,
                llm_changed: redo.llm_changed,
                error: None,
                error_code: None,
                origin: false,
                chosen: false,
            },
            Outcome::Failed {
                profile_id,
                code,
                text,
            } => AttemptLine {
                ts,
                profile_id: profile_id.0.clone(),
                asr_label: None,
                llm_label: None,
                raw_text: String::new(),
                final_text: String::new(),
                degraded: false,
                asr_ms: 0,
                llm_ms: None,
                dict_fixes: 0,
                snippets: 0,
                llm_changed: false,
                error: Some(text.clone()),
                error_code: Some(code.name().to_string()),
                origin: false,
                chosen: false,
            },
        };
        let attempt_id = insert_attempt(&tx, id.0, &line)?;
        if let (Outcome::Done { redo, .. }, true) = (outcome, choose || was_failed) {
            apply_in(&tx, &self.device_id, id, redo)?;
            choose_in(&tx, id, attempt_id)?;
        } else {
            touch_in(&tx, id)?;
        }
        let attempt = tx.query_row(
            &format!("{SELECT_ATTEMPT} WHERE id = ?1"),
            params![attempt_id],
            map_attempt,
        )?;
        tx.commit()?;
        Ok(Some(attempt))
    }

    /// Makes a recorded result the row's. False when the row or the
    /// attempt is gone, or the attempt is a failure.
    pub fn choose_attempt(&self, id: HistoryId, attempt: AttemptId) -> Result<bool, HistoryError> {
        let conn = self.conn.lock().unwrap();
        let tx = conn.unchecked_transaction()?;
        let Some(a) = tx
            .query_row(
                &format!("{SELECT_ATTEMPT} WHERE id = ?1 AND utterance_id = ?2"),
                params![attempt.0, id.0],
                map_attempt,
            )
            .optional()?
        else {
            return Ok(false);
        };
        if a.error.is_some() {
            return Ok(false);
        }
        let redo = Redo {
            profile_id: a.profile_id,
            raw_text: a.raw_text,
            final_text: a.final_text,
            degraded: a.degraded,
            asr_ms: a.asr_ms,
            llm_ms: a.llm_ms,
            dict_fixes: a.dict_fixes,
            snippets: a.snippets,
            llm_changed: a.llm_changed,
        };
        if !apply_in(&tx, &self.device_id, id, &redo)? {
            return Ok(false);
        }
        choose_in(&tx, id, attempt.0)?;
        tx.commit()?;
        Ok(true)
    }

    /// Removes a recorded result that is neither the row's nor the
    /// original one (the dictation as it was made, which a rerun without a
    /// chosen profile follows). False when it is one of those, or gone.
    pub fn delete_attempt(&self, attempt: AttemptId) -> Result<bool, HistoryError> {
        let conn = self.conn.lock().unwrap();
        let tx = conn.unchecked_transaction()?;
        let Some((utterance, kept)) = tx
            .query_row(
                "SELECT utterance_id, chosen OR origin FROM attempts WHERE id = ?1",
                params![attempt.0],
                |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)? != 0)),
            )
            .optional()?
        else {
            return Ok(false);
        };
        if kept {
            return Ok(false);
        }
        tx.execute("DELETE FROM attempts WHERE id = ?1", params![attempt.0])?;
        touch_in(&tx, HistoryId(utterance))?;
        tx.commit()?;
        Ok(true)
    }

    /// Every result a row had, newest first.
    pub fn attempts(&self, id: HistoryId) -> Result<Vec<Attempt>, HistoryError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(&format!(
            "{SELECT_ATTEMPT} WHERE utterance_id = ?1 ORDER BY ts DESC, id DESC"
        ))?;
        let rows = stmt.query_map(params![id.0], map_attempt)?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn set_final(&self, id: HistoryId, text: &str) -> Result<(), HistoryError> {
        self.conn.lock().unwrap().execute(
            "UPDATE utterances SET final_text = ?1, updated = ?3 WHERE id = ?2",
            params![text, id.0, now_ms()],
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

    /// A device's rows for a backup, each with the place of its recording
    /// here. A row that only holds the recording of a dictation still being
    /// processed is left out.
    pub(crate) fn export_rows(
        &self,
        device: &str,
        recordings_dir: &Path,
    ) -> Result<Vec<(HistoryLine, Option<PathBuf>)>, HistoryError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT origin_id, ts, updated, profile_id, app_name, raw_text, refined_text, final_text, degraded,
                    asr_ms, llm_ms, audio_ms, dict_fixes, snippets, error, error_code, audio_path, id
             FROM utterances
             WHERE device_id = ?1 AND origin_id IS NOT NULL AND (error_code IS NULL OR error_code != ?2)
             ORDER BY origin_id",
        )?;
        let rows = stmt.query_map(params![device, ErrorCode::Interrupted.name()], |r| {
            let line = HistoryLine {
                origin_id: r.get(0)?,
                ts: r.get(1)?,
                updated: r.get(2)?,
                profile_id: r.get(3)?,
                app_name: r.get(4)?,
                raw_text: r.get(5)?,
                refined_text: r.get(6)?,
                final_text: r.get(7)?,
                degraded: r.get::<_, i64>(8)? != 0,
                asr_ms: r.get::<_, i64>(9)? as u64,
                llm_ms: r.get::<_, Option<i64>>(10)?.map(|v| v as u64),
                audio_ms: r.get::<_, Option<i64>>(11)?.map(|v| v as u64),
                dict_fixes: r.get::<_, i64>(12)? as u32,
                snippets: r.get::<_, i64>(13)? as u32,
                error: r.get(14)?,
                error_code: r.get(15)?,
                audio: None,
                attempts: Vec::new(),
            };
            Ok((line, r.get::<_, Option<String>>(16)?, r.get::<_, i64>(17)?))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (mut line, audio, id) = row?;
            let audio = audio.map(|name| resolve(&name, recordings_dir));
            line.attempts = attempt_lines(&conn, id)?;
            out.push((line, audio));
        }
        Ok(out)
    }

    /// A device's statistics buckets for a backup.
    pub(crate) fn export_cells(&self, device: &str) -> Result<Vec<StatsLine>, HistoryError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(&format!(
            "SELECT hour, app_name, profile_id, updated, {} FROM stats_hourly
             WHERE device_id = ?1 ORDER BY hour, app_name, profile_id",
            stats::COUNTERS.join(", ")
        ))?;
        let rows = stmt.query_map(params![device], |r| {
            let mut counters = std::collections::BTreeMap::new();
            for (i, name) in stats::COUNTERS.iter().enumerate() {
                counters.insert((*name).to_string(), r.get::<_, i64>(4 + i)?.into());
            }
            Ok(StatsLine {
                hour: r.get(0)?,
                app_name: r.get(1)?,
                profile_id: r.get(2)?,
                updated: r.get(3)?,
                counters,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// The rows of a device that have a recording, by the id they have on
    /// that device.
    pub(crate) fn origins_with_audio(
        &self,
        device: &str,
    ) -> Result<std::collections::HashSet<i64>, HistoryError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT origin_id FROM utterances WHERE device_id = ?1 AND audio_path IS NOT NULL AND origin_id IS NOT NULL",
        )?;
        let ids = stmt.query_map(params![device], |r| r.get(0))?;
        Ok(ids.collect::<Result<_, _>>()?)
    }

    /// Makes a device of a backup known here. A device already known keeps
    /// the name it has.
    pub(crate) fn register_device(&self, meta: &DeviceMeta) -> Result<(), HistoryError> {
        self.conn.lock().unwrap().execute(
            "INSERT INTO devices (id, name, platform) VALUES (?1, ?2, ?3) ON CONFLICT (id) DO NOTHING",
            params![meta.id, meta.name, meta.platform],
        )?;
        Ok(())
    }

    /// Brings a device's rows of a backup in, in one step: a row that is not
    /// here is added, one that is older here is replaced, the rest stays. A
    /// row gets the recording given with its line when it has none. Rows
    /// added with a time before `cutoff` are counted as expiring.
    pub(crate) fn merge_rows(
        &self,
        device: &str,
        lines: &[(HistoryLine, Option<PathBuf>)],
        cutoff: i64,
    ) -> Result<Merged, HistoryError> {
        let conn = self.conn.lock().unwrap();
        let tx = conn.unchecked_transaction()?;
        let mut merged = Merged::default();
        for (l, audio) in lines {
            let audio = audio.as_deref().and_then(name_of);
            let existing: Option<(i64, i64, bool)> = tx
                .query_row(
                    "SELECT id, updated, audio_path IS NOT NULL FROM utterances WHERE device_id = ?1 AND origin_id = ?2",
                    params![device, l.origin_id],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .optional()?;
            match existing {
                None => {
                    tx.execute(
                        "INSERT INTO utterances (ts, profile_id, app_name, raw_text, refined_text, final_text, degraded, audio_path,
                            asr_ms, llm_ms, audio_ms, dict_fixes, snippets, error, error_code, device_id, origin_id, updated)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)",
                        params![
                            l.ts,
                            l.profile_id,
                            l.app_name,
                            l.raw_text,
                            l.refined_text,
                            l.final_text,
                            l.degraded as i64,
                            audio,
                            l.asr_ms as i64,
                            l.llm_ms.map(|v| v as i64),
                            l.audio_ms.map(|v| v as i64),
                            l.dict_fixes as i64,
                            l.snippets as i64,
                            l.error,
                            l.error_code,
                            device,
                            l.origin_id,
                            l.updated,
                        ],
                    )?;
                    let new_id = tx.last_insert_rowid();
                    for a in &l.attempts {
                        insert_attempt(&tx, new_id, a)?;
                    }
                    merged.added += 1;
                    if l.ts < cutoff {
                        merged.expiring += 1;
                    }
                }
                Some((id, updated, has_audio)) => {
                    if updated < l.updated {
                        tx.execute(
                            "UPDATE utterances SET ts = ?1, profile_id = ?2, app_name = ?3, raw_text = ?4, refined_text = ?5,
                                final_text = ?6, degraded = ?7, asr_ms = ?8, llm_ms = ?9, audio_ms = ?10, dict_fixes = ?11,
                                snippets = ?12, error = ?13, error_code = ?14, updated = ?15 WHERE id = ?16",
                            params![
                                l.ts,
                                l.profile_id,
                                l.app_name,
                                l.raw_text,
                                l.refined_text,
                                l.final_text,
                                l.degraded as i64,
                                l.asr_ms as i64,
                                l.llm_ms.map(|v| v as i64),
                                l.audio_ms.map(|v| v as i64),
                                l.dict_fixes as i64,
                                l.snippets as i64,
                                l.error,
                                l.error_code,
                                l.updated,
                                id,
                            ],
                        )?;
                        tx.execute("DELETE FROM attempts WHERE utterance_id = ?1", params![id])?;
                        for a in &l.attempts {
                            insert_attempt(&tx, id, a)?;
                        }
                        merged.updated += 1;
                    } else {
                        merged.skipped += 1;
                    }
                    if let (false, Some(path)) = (has_audio, &audio) {
                        tx.execute(
                            "UPDATE utterances SET audio_path = ?1 WHERE id = ?2",
                            params![path, id],
                        )?;
                    }
                }
            }
        }
        tx.commit()?;
        Ok(merged)
    }

    /// Brings a device's statistics buckets of a backup in: a bucket that is
    /// not here is added, one that is older here is replaced. Returns how
    /// many changed.
    pub(crate) fn merge_cells(
        &self,
        device: &str,
        cells: &[StatsLine],
    ) -> Result<usize, HistoryError> {
        let columns = stats::COUNTERS.join(", ");
        let slots: Vec<String> = (0..stats::COUNTERS.len())
            .map(|i| format!("?{}", i + 6))
            .collect();
        let sets: Vec<String> = stats::COUNTERS
            .iter()
            .map(|c| format!("{c} = excluded.{c}"))
            .collect();
        let sql = format!(
            "INSERT INTO stats_hourly (device_id, hour, app_name, profile_id, updated, {columns})
             VALUES (?1, ?2, ?3, ?4, ?5, {})
             ON CONFLICT (device_id, hour, app_name, profile_id) DO UPDATE SET updated = excluded.updated, {}
             WHERE excluded.updated > stats_hourly.updated",
            slots.join(", "),
            sets.join(", ")
        );
        let conn = self.conn.lock().unwrap();
        let tx = conn.unchecked_transaction()?;
        let mut changed = 0;
        {
            let mut stmt = tx.prepare(&sql)?;
            for c in cells {
                let mut values: Vec<rusqlite::types::Value> = vec![
                    device.to_string().into(),
                    c.hour.into(),
                    c.app_name.clone().into(),
                    c.profile_id.clone().into(),
                    c.updated.into(),
                ];
                values.extend(stats::COUNTERS.iter().map(|name| c.counter(name).into()));
                changed += stmt.execute(rusqlite::params_from_iter(values))?;
            }
        }
        tx.commit()?;
        Ok(changed)
    }

    /// The devices with rows here: this one first, the others by name.
    pub fn devices(&self) -> Result<Vec<DeviceRow>, HistoryError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT d.id, d.name, d.platform, COUNT(u.id), MAX(u.ts)
             FROM devices d LEFT JOIN utterances u ON u.device_id = d.id
             GROUP BY d.id ORDER BY d.id != ?1, d.name COLLATE NOCASE, d.id",
        )?;
        let rows = stmt.query_map(params![self.device_id], |r| {
            let id: String = r.get(0)?;
            Ok(DeviceRow {
                this: id == self.device_id,
                id,
                name: r.get(1)?,
                platform: r.get(2)?,
                rows: r.get::<_, i64>(3)? as u64,
                last_ts: r.get(4)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn set_device_name(&self, id: &str, name: &str) -> Result<(), HistoryError> {
        self.conn.lock().unwrap().execute(
            "UPDATE devices SET name = ?1 WHERE id = ?2",
            params![name, id],
        )?;
        Ok(())
    }

    /// Removes another device's rows, their recordings inside
    /// `recordings_dir`, and its statistics. Returns how many rows went.
    pub fn remove_device(&self, id: &str, recordings_dir: &Path) -> Result<usize, HistoryError> {
        if id == self.device_id {
            return Err(HistoryError::OwnDevice);
        }
        let conn = self.conn.lock().unwrap();
        let tx = conn.unchecked_transaction()?;
        let paths: Vec<String> = {
            let mut stmt = tx.prepare(
                "SELECT audio_path FROM utterances WHERE device_id = ?1 AND audio_path IS NOT NULL",
            )?;
            let found = stmt.query_map(params![id], |r| r.get(0))?;
            found.collect::<Result<_, _>>()?
        };
        let rows = tx.execute("DELETE FROM utterances WHERE device_id = ?1", params![id])?;
        tx.execute("DELETE FROM stats_hourly WHERE device_id = ?1", params![id])?;
        tx.execute("DELETE FROM devices WHERE id = ?1", params![id])?;
        tx.commit()?;
        for name in &paths {
            let _ = std::fs::remove_file(resolve(name, recordings_dir));
        }
        Ok(rows)
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
        Ok(row.map(|u| self.locate(u)))
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
        let rows = rows.collect::<Result<Vec<_>, _>>()?;
        Ok(rows.into_iter().map(|u| self.locate(u)).collect())
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
        for name in paths {
            if let Ok(meta) = std::fs::metadata(resolve(&name, recordings_dir)) {
                out.recordings += 1;
                out.recording_bytes += meta.len();
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
        for (id, name) in old {
            let p = resolve(&name, recordings_dir);
            if p.exists() && std::fs::remove_file(&p).is_ok() {
                report.recordings_deleted += 1;
            }
            // Clear the reference only once the file is confirmed gone. A
            // failed removal leaves the row pointing at the still-present file.
            if !p.exists() {
                conn.execute(
                    "UPDATE utterances SET audio_path = NULL, updated = ?2 WHERE id = ?1",
                    params![id, now_ms()],
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
        for name in doomed {
            let p = resolve(&name, recordings_dir);
            if p.exists() && std::fs::remove_file(&p).is_ok() {
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
        .map(|name| resolve(&name, recordings_dir))
        .filter(|p| p.is_file()))
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
        .map(|p| p.map(|name| resolve(&name, recordings_dir)))
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
    let Some(path) = path.map(|name| resolve(&name, recordings_dir)) else {
        return Ok(false);
    };
    let removed = path.is_file() && std::fs::remove_file(&path).is_ok();
    if !path.exists() {
        conn.execute(
            "UPDATE utterances SET audio_path = NULL, updated = ?2 WHERE id = ?1",
            params![id, now_ms()],
        )?;
    }
    Ok(removed)
}

/// Makes a result the row's text, profile and timings, on an open
/// transaction. True when the row exists. A row that had failed is counted
/// in the statistics now; one that had text is not counted again.
fn apply_in(
    tx: &Connection,
    device_id: &str,
    id: HistoryId,
    r: &Redo,
) -> Result<bool, HistoryError> {
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
            asr_ms = ?5, llm_ms = ?6, dict_fixes = ?7, snippets = ?8, error = NULL, error_code = NULL, updated = ?10 WHERE id = ?9",
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
            now_ms(),
        ],
    )?;
    if was_failed {
        let counted = stats::add(
            tx,
            &stats::Sample {
                device_id,
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
    Ok(true)
}

/// Marks one attempt as the row's and the others as not.
fn choose_in(tx: &Connection, id: HistoryId, attempt: i64) -> rusqlite::Result<()> {
    tx.execute(
        "UPDATE attempts SET chosen = (id = ?1) WHERE utterance_id = ?2",
        params![attempt, id.0],
    )?;
    Ok(())
}

/// Says the row changed, so a backup takes it again.
fn touch_in(tx: &Connection, id: HistoryId) -> rusqlite::Result<()> {
    tx.execute(
        "UPDATE utterances SET updated = ?1 WHERE id = ?2",
        params![now_ms(), id.0],
    )?;
    Ok(())
}

const SELECT: &str = "SELECT id, ts, profile_id, app_name, raw_text, refined_text, final_text, degraded, audio_path, asr_ms, llm_ms, audio_ms, error, error_code,
    (SELECT COUNT(*) FROM attempts a WHERE a.utterance_id = utterances.id AND a.origin = 0) FROM utterances";

const SELECT_ATTEMPT: &str = "SELECT id, utterance_id, ts, profile_id, asr_label, llm_label, raw_text, final_text, degraded, asr_ms, llm_ms,
    dict_fixes, snippets, llm_changed, error, error_code, origin, chosen FROM attempts";

fn map_attempt(r: &rusqlite::Row<'_>) -> rusqlite::Result<Attempt> {
    Ok(Attempt {
        id: AttemptId(r.get(0)?),
        utterance_id: HistoryId(r.get(1)?),
        ts: r.get(2)?,
        profile_id: ProfileId(r.get(3)?),
        asr_label: r.get(4)?,
        llm_label: r.get(5)?,
        raw_text: r.get(6)?,
        final_text: r.get(7)?,
        degraded: r.get::<_, i64>(8)? != 0,
        asr_ms: r.get::<_, i64>(9)? as u64,
        llm_ms: r.get::<_, Option<i64>>(10)?.map(|v| v as u64),
        dict_fixes: r.get::<_, i64>(11)? as u32,
        snippets: r.get::<_, i64>(12)? as u32,
        llm_changed: r.get::<_, i64>(13)? != 0,
        error: r.get(14)?,
        error_code: r.get(15)?,
        origin: r.get::<_, i64>(16)? != 0,
        chosen: r.get::<_, i64>(17)? != 0,
    })
}

/// Writes one attempt line; `chosen` and `origin` as given.
fn insert_attempt(tx: &Connection, utterance: i64, a: &AttemptLine) -> rusqlite::Result<i64> {
    tx.execute(
        "INSERT INTO attempts (utterance_id, ts, profile_id, asr_label, llm_label, raw_text, final_text, degraded, asr_ms, llm_ms,
            dict_fixes, snippets, llm_changed, error, error_code, origin, chosen)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
        params![
            utterance,
            a.ts,
            a.profile_id,
            a.asr_label,
            a.llm_label,
            a.raw_text,
            a.final_text,
            a.degraded as i64,
            a.asr_ms as i64,
            a.llm_ms.map(|v| v as i64),
            a.dict_fixes as i64,
            a.snippets as i64,
            a.llm_changed as i64,
            a.error,
            a.error_code,
            a.origin as i64,
            a.chosen as i64,
        ],
    )?;
    Ok(tx.last_insert_rowid())
}

/// The attempts of a row for a records file, oldest first.
fn attempt_lines(conn: &Connection, utterance: i64) -> rusqlite::Result<Vec<AttemptLine>> {
    let mut stmt = conn.prepare(&format!(
        "{SELECT_ATTEMPT} WHERE utterance_id = ?1 ORDER BY ts, id"
    ))?;
    let rows = stmt.query_map(params![utterance], |r| {
        let a = map_attempt(r)?;
        Ok(AttemptLine {
            ts: a.ts,
            profile_id: a.profile_id.0,
            asr_label: a.asr_label,
            llm_label: a.llm_label,
            raw_text: a.raw_text,
            final_text: a.final_text,
            degraded: a.degraded,
            asr_ms: a.asr_ms,
            llm_ms: a.llm_ms,
            dict_fixes: a.dict_fixes,
            snippets: a.snippets,
            llm_changed: a.llm_changed,
            error: a.error,
            error_code: a.error_code,
            origin: a.origin,
            chosen: a.chosen,
        })
    })?;
    rows.collect()
}

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
        error_code: r.get(13)?,
        redone: r.get::<_, i64>(14)? as u32,
    })
}

/// A plain row for tests in other modules.
#[cfg(test)]
pub(crate) mod tests_support {
    use super::*;

    pub(crate) fn utterance() -> NewUtterance {
        NewUtterance {
            ts: now_unix(),
            profile_id: ProfileId("default".into()),
            app_name: None,
            raw_text: "raw".into(),
            refined_text: "refined".into(),
            final_text: "refined".into(),
            degraded: false,
            audio_path: None,
            asr_ms: 0,
            llm_ms: None,
            audio_ms: None,
            dict_fixes: 0,
            snippets: 0,
            llm_changed: false,
        }
    }
}

#[cfg(test)]
pub(crate) const TEST_REF: DeviceRef<'static> = DeviceRef {
    id: TEST_DEVICE,
    name: "Test",
    platform: "",
};

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
    fn a_recording_is_stored_by_name_and_reported_in_the_current_folder() {
        let tmp = tempfile::tempdir().unwrap();
        let rec = tmp.path().join("recordings");
        std::fs::create_dir_all(&rec).unwrap();
        std::fs::write(rec.join("a.wav"), b"x").unwrap();
        let s = HistoryStore::open_in_memory().unwrap();
        s.set_recordings_dir(&rec);
        let id = s.insert(new(100, Some(rec.join("a.wav")))).unwrap();

        let stored: String = s
            .connection()
            .query_row(
                "SELECT audio_path FROM utterances WHERE id = ?1",
                params![id.0],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(stored, "a.wav", "only the name is kept");
        assert_eq!(
            s.get(id).unwrap().unwrap().audio_path,
            Some(rec.join("a.wav"))
        );
        assert_eq!(s.recording_path(id, &rec).unwrap(), Some(rec.join("a.wav")));

        // The folder moves: the row follows without being rewritten.
        let other = tmp.path().join("elsewhere");
        std::fs::create_dir_all(&other).unwrap();
        std::fs::rename(rec.join("a.wav"), other.join("a.wav")).unwrap();
        s.set_recordings_dir(&other);
        assert_eq!(
            s.get(id).unwrap().unwrap().audio_path,
            Some(other.join("a.wav"))
        );
        assert_eq!(
            s.recording_path(id, &other).unwrap(),
            Some(other.join("a.wav"))
        );
    }

    #[test]
    fn absolute_paths_from_earlier_builds_become_names_at_open() {
        let tmp = tempfile::tempdir().unwrap();
        let rec = tmp.path().join("recordings");
        std::fs::create_dir_all(&rec).unwrap();
        std::fs::write(rec.join("x.wav"), b"x").unwrap();
        let path = tmp.path().join("history.sqlite");
        let device = DeviceRef {
            id: TEST_DEVICE,
            name: "Test",
            platform: "",
        };
        let id = {
            let s = HistoryStore::open(&path, &rec, &device).unwrap();
            let id = s.insert(new(100, None)).unwrap();
            // An earlier build wrote the absolute path of a folder since renamed.
            s.connection()
                .execute(
                    "UPDATE utterances SET audio_path = ?1 WHERE id = ?2",
                    params!["/Volumes/old name/recordings/x.wav", id.0],
                )
                .unwrap();
            id
        };
        let s = HistoryStore::open(&path, &rec, &device).unwrap();
        assert_eq!(
            s.get(id).unwrap().unwrap().audio_path,
            Some(rec.join("x.wav"))
        );
        assert_eq!(s.recording_path(id, &rec).unwrap(), Some(rec.join("x.wav")));
        let r = HistoryStore::open_read_only(&path, &rec).unwrap();
        assert_eq!(
            r.get(id).unwrap().unwrap().audio_path,
            Some(rec.join("x.wav"))
        );
    }

    #[test]
    fn read_only_store_reads_rows_and_refuses_writes() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("history.sqlite");
        let device = DeviceRef {
            id: TEST_DEVICE,
            name: "Test",
            platform: "",
        };
        let writer = HistoryStore::open(&path, tmp.path(), &device).unwrap();
        let id = writer.insert(tests_support::utterance()).unwrap();

        let reader = HistoryStore::open_read_only(&path, tmp.path()).unwrap();
        let row = reader.get(id).unwrap().unwrap();
        assert_eq!(row.raw_text, tests_support::utterance().raw_text);
        assert!(
            reader.insert(tests_support::utterance()).is_err(),
            "a read-only store must not write"
        );
        // The writer is untouched by the reader's refusal.
        assert_eq!(writer.list_recent(10).unwrap().len(), 1);
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
        s.set_recordings_dir(&rec);
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
                code: ErrorCode::ProviderTimeout,
                error: "timed out".into(),
            })
            .unwrap();
        let row = s.get(id).unwrap().unwrap();
        assert_eq!(
            (row.final_text.as_str(), row.error.as_deref()),
            ("", Some("timed out"))
        );
        assert_eq!(s.stats(0, 0).unwrap().totals.count, 0);

        assert_eq!(row.error_code.as_deref(), Some("provider_timeout"));
        s.set_error(id, ErrorCode::ProviderUnreachable, "still down")
            .unwrap();
        assert_eq!(
            s.get(id).unwrap().unwrap().error_code.as_deref(),
            Some("provider_unreachable")
        );
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
        s.set_error(id, ErrorCode::Internal, "ignored").unwrap();
        assert_eq!(s.get(id).unwrap().unwrap().error, None);
        assert!(!s.apply_redo(HistoryId(999), &redo).unwrap());
    }

    fn done(profile: &str, text: &str) -> Outcome {
        Outcome::Done {
            redo: Redo {
                profile_id: ProfileId(profile.into()),
                raw_text: "raw".into(),
                final_text: text.into(),
                degraded: false,
                asr_ms: 10,
                llm_ms: Some(20),
                dict_fixes: 0,
                snippets: 0,
                llm_changed: true,
            },
            asr_label: "Breeze".into(),
            llm_label: Some("Gemma".into()),
        }
    }

    #[test]
    fn a_result_is_recorded_and_the_row_keeps_its_own_until_one_is_chosen() {
        let s = HistoryStore::open_in_memory().unwrap();
        let id = s.insert(new(100, None)).unwrap();
        assert_eq!(s.get(id).unwrap().unwrap().redone, 0);
        let a1 = s
            .record_attempt(id, &done("mail", "first"), false)
            .unwrap()
            .unwrap();
        assert!(!a1.chosen && !a1.origin);
        assert_eq!(a1.asr_label.as_deref(), Some("Breeze"));
        let row = s.get(id).unwrap().unwrap();
        assert_eq!((row.final_text.as_str(), row.redone), ("refined", 1));
        let list = s.attempts(id).unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].id, a1.id);
        assert!(list[1].origin && list[1].chosen);
        assert_eq!(list[1].final_text, "refined");

        assert!(s.choose_attempt(id, a1.id).unwrap());
        let row = s.get(id).unwrap().unwrap();
        assert_eq!(
            (row.final_text.as_str(), row.profile_id.0.as_str()),
            ("first", "mail")
        );
        let list = s.attempts(id).unwrap();
        assert!(list[0].chosen && !list[1].chosen);
        // Switching back is the origin attempt again, not a run.
        assert!(s.choose_attempt(id, list[1].id).unwrap());
        assert_eq!(s.get(id).unwrap().unwrap().final_text, "refined");
        assert_eq!(s.stats(0, 0).unwrap().totals.count, 1);

        // The chosen attempt stays; another goes.
        assert!(!s.delete_attempt(list[1].id).unwrap());
        assert!(s.delete_attempt(a1.id).unwrap());
        // The original stays even when another result is in use.
        let a2 = s
            .record_attempt(id, &done("mail", "second"), true)
            .unwrap()
            .unwrap();
        assert!(a2.chosen);
        let origin = s
            .attempts(id)
            .unwrap()
            .into_iter()
            .find(|a| a.origin)
            .unwrap();
        assert!(!s.delete_attempt(origin.id).unwrap());
        assert!(!s.delete_attempt(a2.id).unwrap());
        assert_eq!(s.attempts(id).unwrap().len(), 2);
        assert_eq!(s.get(id).unwrap().unwrap().redone, 1);
        assert!(!s.choose_attempt(id, a1.id).unwrap());
        assert!(s
            .record_attempt(HistoryId(999), &done("x", "y"), false)
            .unwrap()
            .is_none());
    }

    #[test]
    fn a_failed_row_takes_its_first_result_and_keeps_the_failure_as_its_origin() {
        let s = HistoryStore::open_in_memory().unwrap();
        let id = s
            .insert_failed(FailedUtterance {
                ts: 100,
                profile_id: ProfileId("default".into()),
                app_name: None,
                audio_path: PathBuf::from("/tmp/x.wav"),
                audio_ms: Some(1000),
                code: ErrorCode::ProviderError,
                error: "down".into(),
            })
            .unwrap();
        let again = s
            .record_attempt(
                id,
                &Outcome::Failed {
                    profile_id: ProfileId("default".into()),
                    code: ErrorCode::ProviderTimeout,
                    text: "late".into(),
                },
                false,
            )
            .unwrap()
            .unwrap();
        assert_eq!(again.error.as_deref(), Some("late"));
        assert!(s.get(id).unwrap().unwrap().error.is_some());
        assert!(!s.choose_attempt(id, again.id).unwrap());

        let ok = s
            .record_attempt(id, &done("default", "done"), false)
            .unwrap()
            .unwrap();
        assert!(ok.chosen);
        let row = s.get(id).unwrap().unwrap();
        assert_eq!(
            (row.final_text.as_str(), row.error, row.redone),
            ("done", None, 2)
        );
        assert_eq!(s.stats(0, 0).unwrap().totals.count, 1);
        let list = s.attempts(id).unwrap();
        assert_eq!(list.len(), 3);
        let origin = list.last().unwrap();
        assert!(origin.origin && !origin.chosen);
        assert_eq!(origin.error_code.as_deref(), Some("provider_error"));
    }

    #[test]
    fn deleting_the_row_takes_its_attempts() {
        let s = HistoryStore::open_in_memory().unwrap();
        let id = s.insert(new(100, None)).unwrap();
        s.record_attempt(id, &done("mail", "first"), false).unwrap();
        s.delete(id).unwrap();
        let n: i64 = s
            .connection()
            .query_row("SELECT COUNT(*) FROM attempts", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0);
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
        let s = HistoryStore::open(&path, tmp.path(), &TEST_REF).unwrap();
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
        upgrade(&conn, Path::new("/nonexistent"), &TEST_REF).unwrap();
        upgrade(&conn, Path::new("/nonexistent"), &TEST_REF).unwrap();
        let s = HistoryStore::from_connection(conn, TEST_DEVICE);
        let got = s.list(10, None, false).unwrap();
        assert_eq!(got[0].audio_ms, None);
        assert_eq!(s.stats(0, 0).unwrap().totals.count, 1);
    }

    const OLD_STATS: &str = "
CREATE TABLE stats_hourly (
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
CREATE TABLE stats_meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
INSERT INTO stats_meta (key, value) VALUES ('backfilled', '1');
";

    #[test]
    fn rows_of_an_earlier_build_go_to_this_device() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA).unwrap();
        for col in [
            "audio_ms INTEGER",
            "dict_fixes INTEGER NOT NULL DEFAULT 0",
            "snippets INTEGER NOT NULL DEFAULT 0",
            "error TEXT",
            "error_code TEXT",
        ] {
            conn.execute_batch(&format!("ALTER TABLE utterances ADD COLUMN {col}"))
                .unwrap();
        }
        conn.execute_batch(OLD_STATS).unwrap();
        conn.execute_batch(
            "INSERT INTO utterances (ts, profile_id, raw_text, refined_text, final_text) VALUES (7200, 'default', 'a', 'a', 'a');
             INSERT INTO utterances (ts, profile_id, raw_text, refined_text, final_text) VALUES (9000, 'mail', 'b', 'b', 'b');
             INSERT INTO stats_hourly (hour, app_name, profile_id, count, chars, max_cpm) VALUES (7200, 'Notes', 'default', 3, 30, 99);
             INSERT INTO stats_hourly (hour, app_name, profile_id, count, chars) VALUES (10800, '', 'mail', 2, 8);",
        )
        .unwrap();
        let device = DeviceRef {
            id: "dev-a",
            name: "A",
            platform: "macos",
        };
        upgrade(&conn, Path::new(""), &device).unwrap();
        upgrade(&conn, Path::new(""), &device).unwrap();

        let rows: Vec<(i64, String, i64, i64, i64)> = conn
            .prepare("SELECT id, device_id, origin_id, updated, ts FROM utterances ORDER BY id")
            .unwrap()
            .query_map([], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
            })
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(rows.len(), 2);
        for (id, dev, origin, updated, ts) in rows {
            assert_eq!(dev, "dev-a");
            assert_eq!(origin, id);
            assert_eq!(updated, ts * 1000);
        }
        let cells: Vec<(String, i64, i64, i64, i64)> = conn
            .prepare(
                "SELECT device_id, hour, count, chars, max_cpm FROM stats_hourly ORDER BY hour",
            )
            .unwrap()
            .query_map([], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
            })
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(
            cells,
            vec![
                ("dev-a".to_string(), 7200, 3, 30, 99),
                ("dev-a".to_string(), 10800, 2, 8, 0)
            ]
        );
        let named: (String, String) = conn
            .query_row(
                "SELECT name, platform FROM devices WHERE id = 'dev-a'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(named, ("A".to_string(), "macos".to_string()));
        let s = HistoryStore::from_connection(conn, "dev-a");
        assert_eq!(s.stats(0, 0).unwrap().totals.count, 5);
        assert_eq!(s.device_id(), "dev-a");
    }

    fn meta(s: &HistoryStore, id: HistoryId) -> (String, i64, i64) {
        s.connection()
            .query_row(
                "SELECT device_id, origin_id, updated FROM utterances WHERE id = ?1",
                params![id.0],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap()
    }

    #[test]
    fn a_new_row_carries_the_device_and_its_own_id() {
        let s = HistoryStore::open_in_memory().unwrap();
        let id = s.insert(new(100, None)).unwrap();
        let (device, origin, updated) = meta(&s, id);
        assert_eq!(device, TEST_DEVICE);
        assert_eq!(origin, id.0);
        assert!(updated > 0);
        let failed = s
            .insert_failed(FailedUtterance {
                ts: 5,
                profile_id: ProfileId("default".into()),
                app_name: None,
                audio_path: PathBuf::from("/r/a.wav"),
                audio_ms: None,
                code: ErrorCode::ProviderError,
                error: "x".into(),
            })
            .unwrap();
        let (device, origin, updated) = meta(&s, failed);
        assert_eq!((device.as_str(), origin), (TEST_DEVICE, failed.0));
        assert!(updated > 0);
    }

    #[test]
    fn changing_a_row_moves_its_updated_time() {
        let s = HistoryStore::open_in_memory().unwrap();
        let id = s.insert(new(100, None)).unwrap();
        let failed = s
            .insert_failed(FailedUtterance {
                ts: 5,
                profile_id: ProfileId("default".into()),
                app_name: None,
                audio_path: PathBuf::from("/r/a.wav"),
                audio_ms: None,
                code: ErrorCode::ProviderError,
                error: "x".into(),
            })
            .unwrap();
        let reset = |id: HistoryId| {
            s.connection()
                .execute(
                    "UPDATE utterances SET updated = 1 WHERE id = ?1",
                    params![id.0],
                )
                .unwrap();
        };
        reset(id);
        s.set_final(id, "edited").unwrap();
        assert!(meta(&s, id).2 > 1);

        reset(failed);
        s.set_error(failed, ErrorCode::ProviderTimeout, "again")
            .unwrap();
        assert!(meta(&s, failed).2 > 1);

        reset(failed);
        s.set_audio_ms(failed, Some(900)).unwrap();
        assert!(meta(&s, failed).2 > 1);

        reset(failed);
        let redo = Redo {
            profile_id: ProfileId("default".into()),
            raw_text: "r".into(),
            final_text: "f".into(),
            degraded: false,
            asr_ms: 1,
            llm_ms: None,
            dict_fixes: 0,
            snippets: 0,
            llm_changed: false,
        };
        assert!(s.apply_redo(failed, &redo).unwrap());
        assert!(meta(&s, failed).2 > 1);
    }

    fn foreign_row(s: &HistoryStore, device: &str, origin: i64, ts: i64, audio: Option<&Path>) {
        s.connection()
            .execute(
                "INSERT INTO utterances (ts, profile_id, raw_text, refined_text, final_text, audio_path, device_id, origin_id, updated)
                 VALUES (?1, 'default', 'r', 'f', 'f', ?2, ?3, ?4, ?5)",
                params![ts, audio.map(|p| p.to_string_lossy().to_string()), device, origin, ts * 1000],
            )
            .unwrap();
    }

    fn foreign_device(s: &HistoryStore, id: &str, name: &str) {
        s.connection()
            .execute(
                "INSERT INTO devices (id, name, platform) VALUES (?1, ?2, 'macos')",
                params![id, name],
            )
            .unwrap();
    }

    #[test]
    fn devices_lists_this_one_first_with_counts() {
        let s = HistoryStore::open_in_memory().unwrap();
        s.insert(new(100, None)).unwrap();
        s.insert(new(300, None)).unwrap();
        foreign_device(&s, "aaa", "Anna");
        foreign_row(&s, "aaa", 1, 900, None);
        foreign_device(&s, "bbb", "Empty");
        let got = s.devices().unwrap();
        let brief: Vec<_> = got
            .iter()
            .map(|d| (d.id.as_str(), d.name.as_str(), d.rows, d.last_ts, d.this))
            .collect();
        assert_eq!(
            brief,
            vec![
                (TEST_DEVICE, "Test", 2, Some(300), true),
                ("aaa", "Anna", 1, Some(900), false),
                ("bbb", "Empty", 0, None, false),
            ]
        );
    }

    #[test]
    fn a_device_gets_another_name() {
        let s = HistoryStore::open_in_memory().unwrap();
        foreign_device(&s, "aaa", "Anna");
        s.set_device_name("aaa", "Office").unwrap();
        assert_eq!(s.devices().unwrap()[1].name, "Office");
    }

    #[test]
    fn removing_a_device_takes_its_rows_cells_and_recordings() {
        let tmp = tempfile::tempdir().unwrap();
        let wav = tmp.path().join("aaa-1.wav");
        std::fs::write(&wav, b"x").unwrap();
        let s = HistoryStore::open_in_memory().unwrap();
        s.insert(new(100, None)).unwrap();
        foreign_device(&s, "aaa", "Anna");
        foreign_row(&s, "aaa", 1, 900, Some(&wav));
        foreign_row(&s, "aaa", 2, 950, None);
        s.connection()
            .execute(
                "INSERT INTO stats_hourly (device_id, hour, app_name, profile_id, count) VALUES ('aaa', 0, '', 'default', 4)",
                [],
            )
            .unwrap();
        assert_eq!(s.stats(0, 0).unwrap().totals.count, 5);

        assert_eq!(s.remove_device("aaa", tmp.path()).unwrap(), 2);
        assert!(!wav.exists());
        assert_eq!(s.devices().unwrap().len(), 1);
        assert_eq!(s.list(10, None, false).unwrap().len(), 1);
        assert_eq!(s.stats(0, 0).unwrap().totals.count, 1);
    }

    #[test]
    fn this_device_cannot_be_removed() {
        let s = HistoryStore::open_in_memory().unwrap();
        s.insert(new(100, None)).unwrap();
        assert!(s.remove_device(TEST_DEVICE, Path::new("")).is_err());
        assert_eq!(s.list(10, None, false).unwrap().len(), 1);
        assert_eq!(s.devices().unwrap().len(), 1);
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

    fn held(path: &Path) -> FailedUtterance {
        FailedUtterance {
            ts: 100,
            profile_id: ProfileId("default".into()),
            app_name: None,
            audio_path: path.to_path_buf(),
            audio_ms: Some(2000),
            code: ErrorCode::Interrupted,
            error: "the dictation did not finish".into(),
        }
    }

    #[test]
    fn the_result_replaces_the_row_that_held_the_recording() {
        let s = HistoryStore::open_in_memory().unwrap();
        s.set_recordings_dir(Path::new("/rec"));
        let path = PathBuf::from("/rec/a.wav");
        let holding = s.insert_failed(held(&path)).unwrap();
        let id = s
            .insert_replacing(new(100, Some(path.clone())), Some(holding), || true)
            .unwrap()
            .unwrap();
        let rows = s.list_recent(10).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, id);
        assert_eq!(rows[0].error, None);
        assert_eq!(rows[0].audio_path, Some(path));
    }

    #[test]
    fn a_withdrawn_result_leaves_the_holding_row() {
        let s = HistoryStore::open_in_memory().unwrap();
        let path = PathBuf::from("/rec/a.wav");
        let holding = s.insert_failed(held(&path)).unwrap();
        let kept = s
            .insert_replacing(new(100, Some(path)), Some(holding), || false)
            .unwrap();
        assert_eq!(kept, None);
        let rows = s.list_recent(10).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, holding);
    }

    #[test]
    fn only_a_row_still_being_processed_is_discarded() {
        let s = HistoryStore::open_in_memory().unwrap();
        let a = PathBuf::from("/rec/a.wav");
        let b = PathBuf::from("/rec/b.wav");
        s.insert_failed(held(&a)).unwrap();
        let failed = s.insert_failed(held(&b)).unwrap();
        s.set_error(failed, ErrorCode::ProviderTimeout, "timed out")
            .unwrap();
        s.insert(new(100, Some(PathBuf::from("/rec/c.wav"))))
            .unwrap();

        assert!(s.discard_interrupted(&a).unwrap());
        assert!(!s.discard_interrupted(&a).unwrap());
        assert!(!s.discard_interrupted(&b).unwrap());
        assert!(!s.discard_interrupted(Path::new("/rec/c.wav")).unwrap());
        assert_eq!(s.list_recent(10).unwrap().len(), 2);
    }

    #[test]
    fn the_length_of_a_held_recording_can_be_corrected() {
        let s = HistoryStore::open_in_memory().unwrap();
        let id = s.insert_failed(held(Path::new("/rec/a.wav"))).unwrap();
        s.set_audio_ms(id, Some(1200)).unwrap();
        assert_eq!(s.get(id).unwrap().unwrap().audio_ms, Some(1200));
    }

    #[test]
    fn recordings_move_to_another_folder_with_their_rows() {
        let tmp = tempfile::tempdir().unwrap();
        let from = tmp.path().join("old");
        let to = tmp.path().join("new");
        std::fs::create_dir_all(&from).unwrap();
        std::fs::create_dir_all(&to).unwrap();
        let moved = from.join("a.wav");
        let taken = from.join("b.wav");
        let stray = from.join("c.wav");
        for p in [&moved, &taken, &stray] {
            std::fs::write(p, b"x").unwrap();
        }
        std::fs::write(from.join("notes.txt"), b"x").unwrap();
        // The name is already used in the new folder.
        std::fs::write(to.join("b.wav"), b"other").unwrap();
        let s = HistoryStore::open_in_memory().unwrap();
        let a = s.insert(new(100, Some(moved.clone()))).unwrap();
        let b = s.insert(new(100, Some(taken.clone()))).unwrap();

        let report = s.move_recordings(&from, &to).unwrap();

        assert_eq!(report, MoveReport { moved: 2, left: 1 });
        assert_eq!(
            s.get(a).unwrap().unwrap().audio_path,
            Some(to.join("a.wav"))
        );
        assert!(to.join("a.wav").is_file() && !moved.exists());
        // The row names the file of that name in the new folder; the file
        // that could not be moved stays behind and is counted in `left`.
        assert_eq!(
            s.get(b).unwrap().unwrap().audio_path,
            Some(to.join("b.wav"))
        );
        assert!(taken.is_file());
        assert!(to.join("c.wav").is_file() && !stray.exists());
        assert!(from.join("notes.txt").is_file());
        // The same folder, or one that does not exist: nothing to do.
        assert_eq!(s.move_recordings(&to, &to).unwrap(), MoveReport::default());
        assert_eq!(
            s.move_recordings(&tmp.path().join("none"), &to).unwrap(),
            MoveReport::default()
        );
    }
}
