use std::path::Path;

use arcvox_core::history::{
    Attempt, AttemptId, HistoryStore, ListFilter, StageKind, StageRecord, Utterance,
};
use arcvox_core::types::{HistoryId, ProfileId};
use arcvox_core::{Engine, ReprocessError};
use serde::Serialize;
use tauri::State;

use super::{engine, CmdError, CmdResult};
use crate::app::AppState;

#[derive(Debug, Clone, Serialize)]
pub struct UtteranceDto {
    pub id: i64,
    pub ts: i64,
    pub profile_id: String,
    pub app_name: Option<String>,
    pub raw_text: String,
    pub refined_text: String,
    pub final_text: String,
    pub degraded: bool,
    /// True when the recording file is present.
    pub has_audio: bool,
    pub asr_ms: u64,
    pub llm_ms: Option<u64>,
    pub audio_ms: Option<u64>,
    pub audio_bytes: Option<u64>,
    /// Why the dictation failed; such a row has no text yet.
    pub error: Option<String>,
    /// The kind of failure, when it was kept.
    pub error_code: Option<String>,
    /// How many times the recording was processed again.
    pub redone: u32,
}

/// What `history_reprocess` returns: the result recorded with the row, or
/// why the engines failed (recorded too).
#[derive(Debug, Clone, Serialize)]
pub struct RedoOutcome {
    pub attempt: Option<AttemptDto>,
    pub failure: Option<FailureDto>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FailureDto {
    pub code: &'static str,
    pub detail: String,
}

/// One result a row had: what processing its recording gave, or how it
/// failed. The chosen one is what the row shows.
#[derive(Debug, Clone, Serialize)]
pub struct AttemptDto {
    pub id: i64,
    pub ts: i64,
    pub profile_id: String,
    pub asr_label: Option<String>,
    pub llm_label: Option<String>,
    pub raw_text: String,
    pub final_text: String,
    pub degraded: bool,
    pub asr_ms: u64,
    pub llm_ms: Option<u64>,
    pub error: Option<String>,
    pub error_code: Option<String>,
    pub origin: bool,
    pub chosen: bool,
}

impl From<Attempt> for AttemptDto {
    fn from(a: Attempt) -> Self {
        AttemptDto {
            id: a.id.0,
            ts: a.ts,
            profile_id: a.profile_id.0,
            asr_label: a.asr_label,
            llm_label: a.llm_label,
            raw_text: a.raw_text,
            final_text: a.final_text,
            degraded: a.degraded,
            asr_ms: a.asr_ms,
            llm_ms: a.llm_ms,
            error: a.error,
            error_code: a.error_code,
            origin: a.origin,
            chosen: a.chosen,
        }
    }
}

/// Processes a row's recording again with the named profile, or the row's
/// own. The result is recorded with the row; a row that had failed takes it
/// at once, one that has text keeps its own until the user chooses.
pub async fn reprocess(
    engine: &Engine,
    id: i64,
    profile_id: Option<String>,
) -> CmdResult<RedoOutcome> {
    let id = HistoryId(id);
    match engine.reprocess(id, profile_id.map(ProfileId)).await {
        Ok(done) => Ok(RedoOutcome {
            attempt: Some(done.attempt.into()),
            failure: None,
        }),
        Err(ReprocessError::Failed { code, text }) => Ok(RedoOutcome {
            attempt: None,
            failure: Some(FailureDto {
                code: code.name(),
                detail: text,
            }),
        }),
        Err(e) => Err(e.into()),
    }
}

#[tauri::command]
pub async fn history_reprocess(
    state: State<'_, AppState>,
    id: i64,
    profile_id: Option<String>,
) -> CmdResult<RedoOutcome> {
    let e = engine(&state)?;
    state.backup.touch();
    reprocess(&e, id, profile_id).await
}

/// Every result a row had, newest first.
#[tauri::command]
pub fn history_attempts(state: State<'_, AppState>, id: i64) -> CmdResult<Vec<AttemptDto>> {
    let e = engine(&state)?;
    Ok(e.history()
        .attempts(HistoryId(id))?
        .into_iter()
        .map(AttemptDto::from)
        .collect())
}

/// What one stage of a run did; `detail` holds the stage's own facts.
#[derive(Debug, Clone, Serialize)]
pub struct StageDto {
    pub position: u32,
    pub kind: StageKind,
    pub text: Option<String>,
    pub ms: Option<u64>,
    pub detail: serde_json::Value,
}

impl From<StageRecord> for StageDto {
    fn from(s: StageRecord) -> Self {
        StageDto {
            position: s.position,
            kind: s.kind,
            text: s.text,
            ms: s.ms,
            detail: s.detail,
        }
    }
}

/// The newest dictation made with a profile and what its stages did.
#[derive(Debug, Clone, Serialize)]
pub struct LastRunDto {
    pub row: UtteranceDto,
    pub stages: Vec<StageDto>,
}

/// The stages of a row's current result, in order.
#[tauri::command]
pub fn history_stages(state: State<'_, AppState>, id: i64) -> CmdResult<Vec<StageDto>> {
    let e = engine(&state)?;
    Ok(e.history()
        .stages(HistoryId(id))?
        .into_iter()
        .map(StageDto::from)
        .collect())
}

/// The newest dictation made with a profile, with its stages; None when the
/// profile has not been used yet.
#[tauri::command]
pub async fn history_last(
    state: State<'_, AppState>,
    profile_id: String,
) -> CmdResult<Option<LastRunDto>> {
    let e = engine(&state)?;
    let recordings_dir = e.recordings_dir();
    Ok(e.last_run(&ProfileId(profile_id))
        .await?
        .map(|(row, stages)| LastRunDto {
            row: UtteranceDto::new(row, &recordings_dir),
            stages: stages.into_iter().map(StageDto::from).collect(),
        }))
}

/// Makes a recorded result the row's text.
#[tauri::command]
pub fn history_choose_attempt(state: State<'_, AppState>, id: i64, attempt: i64) -> CmdResult<()> {
    let e = engine(&state)?;
    state.backup.touch();
    if !e
        .history()
        .choose_attempt(HistoryId(id), AttemptId(attempt))?
    {
        return Err(CmdError::new("attempt_gone", "the result no longer exists"));
    }
    Ok(())
}

/// Removes a recorded result that the row does not show.
#[tauri::command]
pub fn history_delete_attempt(state: State<'_, AppState>, attempt: i64) -> CmdResult<()> {
    let e = engine(&state)?;
    state.backup.touch();
    if !e.history().delete_attempt(AttemptId(attempt))? {
        return Err(CmdError::new(
            "attempt_in_use",
            "the result the row shows, and the original one, cannot be removed",
        ));
    }
    Ok(())
}

impl UtteranceDto {
    /// A recording counts only when its file is inside `recordings_dir` and can be read.
    fn new(u: Utterance, recordings_dir: &Path) -> Self {
        let audio_bytes = u
            .audio_path
            .as_deref()
            .filter(|p| p.starts_with(recordings_dir))
            .and_then(|p| std::fs::metadata(p).ok())
            .map(|m| m.len());
        Self {
            id: u.id.0,
            ts: u.ts,
            profile_id: u.profile_id.0,
            app_name: u.app_name,
            raw_text: u.raw_text,
            refined_text: u.refined_text,
            final_text: u.final_text,
            degraded: u.degraded,
            has_audio: audio_bytes.is_some(),
            asr_ms: u.asr_ms,
            llm_ms: u.llm_ms,
            audio_ms: u.audio_ms.filter(|_| audio_bytes.is_some()),
            audio_bytes,
            error: u.error,
            error_code: u.error_code,
            redone: u.redone,
        }
    }
}

/// One page of rows, newest first; see `ListFilter` for the window and the
/// last row shown. `in_flight` says whether a recording belongs to a dictation still
/// being processed; its row is left out until the result is known.
pub fn list(
    history: &HistoryStore,
    recordings_dir: &Path,
    limit: usize,
    filter: &ListFilter,
    in_flight: impl Fn(&Path) -> bool,
) -> CmdResult<Vec<UtteranceDto>> {
    Ok(history
        .list_page(limit, filter)?
        .into_iter()
        .filter(|u| !u.audio_path.as_deref().is_some_and(&in_flight))
        .map(|u| UtteranceDto::new(u, recordings_dir))
        // A row whose file is gone has no recording to show.
        .filter(|u| !filter.with_audio || u.has_audio)
        .collect())
}

/// The bytes of a row's recording.
pub fn audio(history: &HistoryStore, recordings_dir: &Path, id: i64) -> CmdResult<Vec<u8>> {
    let path = recording(history, recordings_dir, id)?;
    Ok(std::fs::read(path)?)
}

fn recording(
    history: &HistoryStore,
    recordings_dir: &Path,
    id: i64,
) -> CmdResult<std::path::PathBuf> {
    history
        .recording_path(HistoryId(id), recordings_dir)?
        .ok_or_else(|| CmdError::new("recording_not_kept", "recording not found"))
}

/// `since` and `until` are unix seconds; `before_ts` and `before_id` name
/// the last row of the page already shown.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn history_list(
    state: State<'_, AppState>,
    limit: Option<usize>,
    query: Option<String>,
    with_audio: Option<bool>,
    since: Option<i64>,
    until: Option<i64>,
    before_ts: Option<i64>,
    before_id: Option<i64>,
) -> CmdResult<Vec<UtteranceDto>> {
    let e = engine(&state)?;
    let filter = ListFilter {
        query,
        with_audio: with_audio.unwrap_or(false),
        since,
        until,
        before: before_ts.zip(before_id),
    };
    list(
        &e.history(),
        &e.recordings_dir(),
        limit.unwrap_or(100),
        &filter,
        |p| e.in_flight(p),
    )
}

#[tauri::command]
pub fn history_audio(state: State<'_, AppState>, id: i64) -> CmdResult<tauri::ipc::Response> {
    let e = engine(&state)?;
    Ok(tauri::ipc::Response::new(audio(
        &e.history(),
        &e.recordings_dir(),
        id,
    )?))
}

#[tauri::command]
pub fn history_delete_recording(state: State<'_, AppState>, id: i64) -> CmdResult<()> {
    let e = engine(&state)?;
    e.history()
        .delete_recording(HistoryId(id), &e.recordings_dir())?;
    Ok(())
}

/// Deletes every recording; returns how many files were removed.
#[tauri::command]
pub fn history_clear_recordings(state: State<'_, AppState>) -> CmdResult<usize> {
    let e = engine(&state)?;
    Ok(e.history().clear_recordings(&e.recordings_dir())?)
}

#[tauri::command]
pub fn history_reveal_recording(state: State<'_, AppState>, id: i64) -> CmdResult<()> {
    let e = engine(&state)?;
    let path = recording(&e.history(), &e.recordings_dir(), id)?;
    std::process::Command::new("open")
        .arg("-R")
        .arg(path)
        .spawn()?;
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
pub struct StorageDto {
    pub rows: u64,
    pub recordings: u64,
    pub recording_bytes: u64,
    pub text_bytes: u64,
    pub stage_bytes: u64,
    pub stages: u64,
    pub attempts: u64,
    pub stats_hours: u64,
    pub db_bytes: u64,
}

/// Rows per local day, for the period picker's calendar.
#[tauri::command]
pub fn history_days(
    state: State<'_, AppState>,
    tz_offset_secs: i32,
    with_audio: Option<bool>,
) -> CmdResult<Vec<arcvox_core::history::DayCount>> {
    let e = engine(&state)?;
    Ok(e.history()
        .day_counts(tz_offset_secs, with_audio.unwrap_or(false))?)
}

/// Rows per local hour between `since` and `until`, for the picker's hour row.
#[tauri::command]
pub fn history_hours(
    state: State<'_, AppState>,
    since: i64,
    until: i64,
    tz_offset_secs: i32,
    with_audio: Option<bool>,
) -> CmdResult<Vec<u32>> {
    let e = engine(&state)?;
    Ok(e.history()
        .hour_counts(since, until, tz_offset_secs, with_audio.unwrap_or(false))?)
}

#[tauri::command]
pub fn history_storage(
    state: State<'_, AppState>,
    since: Option<i64>,
    until: Option<i64>,
) -> CmdResult<StorageDto> {
    let e = engine(&state)?;
    let s = e.history().storage_in(since, until, &e.recordings_dir())?;
    Ok(StorageDto {
        rows: s.rows,
        recordings: s.recordings,
        recording_bytes: s.recording_bytes,
        text_bytes: s.text_bytes,
        stage_bytes: s.stage_bytes,
        stages: s.stages,
        attempts: s.attempts,
        stats_hours: s.stats_hours,
        db_bytes: s.db_bytes,
    })
}

/// Drops every stage record; the rows keep their texts. Returns how many went.
#[tauri::command]
pub fn history_clear_stages(state: State<'_, AppState>) -> CmdResult<usize> {
    Ok(engine(&state)?.history().clear_stages()?)
}

/// Drops every rerun result; the result each row uses stays. Returns how many went.
#[tauri::command]
pub fn history_clear_attempts(state: State<'_, AppState>) -> CmdResult<usize> {
    Ok(engine(&state)?.history().clear_attempts()?)
}

/// Removes the rows between `since` and `until` with their recordings.
#[tauri::command]
pub fn history_clear_range(
    state: State<'_, AppState>,
    since: Option<i64>,
    until: Option<i64>,
) -> CmdResult<usize> {
    let e = engine(&state)?;
    Ok(e.history().clear_range(since, until, &e.recordings_dir())?)
}

/// Removes the recordings of the rows between `since` and `until`.
#[tauri::command]
pub fn history_clear_recordings_range(
    state: State<'_, AppState>,
    since: Option<i64>,
    until: Option<i64>,
) -> CmdResult<usize> {
    let e = engine(&state)?;
    Ok(e.history()
        .clear_recordings_in(since, until, &e.recordings_dir())?)
}

/// The folder recordings are kept in.
#[derive(Debug, Clone, Serialize)]
pub struct RecordingsDirDto {
    pub path: String,
    /// False when the user chose it.
    pub is_default: bool,
    /// Recordings moved by this change, and those that could not be moved.
    pub moved: usize,
    pub left: usize,
}

fn recordings_dir_dto(e: &arcvox_core::Engine, moved: usize, left: usize) -> RecordingsDirDto {
    RecordingsDirDto {
        path: e.recordings_dir().display().to_string(),
        is_default: e.config().current().general.recordings_dir.is_none(),
        moved,
        left,
    }
}

#[tauri::command]
pub fn recordings_dir_get(state: State<'_, AppState>) -> CmdResult<RecordingsDirDto> {
    Ok(recordings_dir_dto(&engine(&state)?, 0, 0))
}

/// Keeps recordings in `path` from now on, or in the default folder without
/// one, and moves the existing recordings there.
#[tauri::command]
pub async fn recordings_dir_set(
    state: State<'_, AppState>,
    path: Option<String>,
) -> CmdResult<RecordingsDirDto> {
    let e = engine(&state)?;
    let report = e
        .set_recordings_dir(path.map(std::path::PathBuf::from))
        .await?;
    Ok(recordings_dir_dto(&e, report.moved, report.left))
}

/// Opens the recordings folder in the file manager.
#[tauri::command]
pub fn recordings_dir_open(state: State<'_, AppState>) -> CmdResult<()> {
    let dir = engine(&state)?.recordings_dir();
    // It may not exist yet, or was removed by hand.
    std::fs::create_dir_all(&dir)?;
    std::process::Command::new("open").arg(dir).spawn()?;
    Ok(())
}

#[tauri::command]
pub fn history_delete(state: State<'_, AppState>, id: i64) -> CmdResult<()> {
    let e = engine(&state)?;
    e.history().delete_row(HistoryId(id), &e.recordings_dir())?;
    Ok(())
}

#[tauri::command]
pub fn history_clear(state: State<'_, AppState>) -> CmdResult<()> {
    let e = engine(&state)?;
    e.history().clear_all(&e.recordings_dir())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use arcvox_core::history::NewUtterance;
    use arcvox_core::types::ProfileId;

    use super::*;

    fn row(ts: i64, text: &str, audio: Option<PathBuf>) -> NewUtterance {
        NewUtterance {
            ts,
            profile_id: ProfileId("default".into()),
            app_name: None,
            raw_text: text.into(),
            refined_text: text.into(),
            final_text: text.into(),
            degraded: false,
            audio_path: audio,
            asr_ms: 10,
            llm_ms: None,
            audio_ms: Some(1500),
            dict_fixes: 0,
            snippets: 0,
            llm_changed: false,
            stages: Vec::new(),
        }
    }

    #[test]
    fn a_row_reports_its_recording_only_when_the_file_is_in_the_recordings_directory() {
        let dir = tempfile::tempdir().unwrap();
        let elsewhere = tempfile::tempdir().unwrap();
        let history = HistoryStore::open_in_memory().unwrap();
        history.set_recordings_dir(dir.path());
        let kept = dir.path().join("a.wav");
        let stray = elsewhere.path().join("b.wav");
        std::fs::write(&kept, b"0123456789").unwrap();
        std::fs::write(&stray, b"01234").unwrap();
        history.insert(row(1, "kept", Some(kept))).unwrap();
        let stray_id = history.insert(row(2, "stray", Some(stray))).unwrap();
        history
            .insert(row(3, "gone", Some(dir.path().join("missing.wav"))))
            .unwrap();

        let rows = list(&history, dir.path(), 10, &ListFilter::default(), |_| false).unwrap();
        let by_text = |t: &str| rows.iter().find(|r| r.final_text == t).unwrap();
        assert!(by_text("kept").has_audio);
        assert_eq!(by_text("kept").audio_bytes, Some(10));
        assert_eq!(by_text("kept").audio_ms, Some(1500));
        for text in ["stray", "gone"] {
            assert!(!by_text(text).has_audio, "{text}");
            assert_eq!(by_text(text).audio_bytes, None, "{text}");
            assert_eq!(by_text(text).audio_ms, None, "{text}");
        }
        assert!(audio(&history, dir.path(), stray_id.0).is_err());
    }

    #[test]
    fn audio_returns_the_file_and_the_list_filters() {
        let dir = tempfile::tempdir().unwrap();
        let history = HistoryStore::open_in_memory().unwrap();
        history.set_recordings_dir(dir.path());
        let path = dir.path().join("a.wav");
        std::fs::write(&path, b"RIFFdata").unwrap();
        let id = history.insert(row(1, "with sound", Some(path))).unwrap();
        history.insert(row(2, "text only", None)).unwrap();

        assert_eq!(audio(&history, dir.path(), id.0).unwrap(), b"RIFFdata");
        let only = |q: Option<&str>, with_audio: bool| {
            list(
                &history,
                dir.path(),
                10,
                &ListFilter {
                    query: q.map(str::to_string),
                    with_audio,
                    ..ListFilter::default()
                },
                |_| false,
            )
            .unwrap()
            .into_iter()
            .map(|r| r.final_text)
            .collect::<Vec<_>>()
        };
        assert_eq!(only(None, true), vec!["with sound"]);
        history
            .insert(row(3, "file gone", Some(dir.path().join("missing.wav"))))
            .unwrap();
        assert_eq!(only(None, true), vec!["with sound"]);
        assert_eq!(only(Some("only"), false), vec!["text only"]);
        assert!(audio(&history, dir.path(), 999).is_err());
    }
}
