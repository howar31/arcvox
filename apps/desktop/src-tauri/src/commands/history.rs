use std::path::Path;

use arcvox_core::history::{HistoryStore, Redo, Utterance};
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
}

/// The result of processing a row's recording again.
#[derive(Debug, Clone, Serialize, serde::Deserialize)]
pub struct RedoDto {
    /// True when the result was stored right away (the row had failed).
    pub applied: bool,
    pub profile_id: String,
    pub raw_text: String,
    pub final_text: String,
    pub degraded: bool,
    pub asr_ms: u64,
    pub llm_ms: Option<u64>,
    pub dict_fixes: u32,
    pub snippets: u32,
    pub llm_changed: bool,
    pub asr_label: String,
    pub llm_label: Option<String>,
}

impl RedoDto {
    fn redo(&self) -> Redo {
        Redo {
            profile_id: ProfileId(self.profile_id.clone()),
            raw_text: self.raw_text.clone(),
            final_text: self.final_text.clone(),
            degraded: self.degraded,
            asr_ms: self.asr_ms,
            llm_ms: self.llm_ms,
            dict_fixes: self.dict_fixes,
            snippets: self.snippets,
            llm_changed: self.llm_changed,
        }
    }
}

/// Processes a row's recording again with the named profile, or the row's
/// own. A row that had failed takes the result at once, or the new reason
/// when this attempt fails too; a row that has text is left as it is and
/// the result is returned for the user to accept.
pub async fn reprocess(engine: &Engine, id: i64, profile_id: Option<String>) -> CmdResult<RedoDto> {
    let id = HistoryId(id);
    let history = engine.history();
    let was_failed = history.get(id)?.is_some_and(|u| u.error.is_some());
    let done = match engine.reprocess(id, profile_id.map(ProfileId)).await {
        Ok(done) => done,
        Err(e) => {
            if let ReprocessError::Failed(why) = &e {
                history.set_error(id, why)?;
            }
            return Err(e.into());
        }
    };
    let applied = was_failed && history.apply_redo(id, &done.redo)?;
    let r = done.redo;
    Ok(RedoDto {
        applied,
        profile_id: r.profile_id.0,
        raw_text: r.raw_text,
        final_text: r.final_text,
        degraded: r.degraded,
        asr_ms: r.asr_ms,
        llm_ms: r.llm_ms,
        dict_fixes: r.dict_fixes,
        snippets: r.snippets,
        llm_changed: r.llm_changed,
        asr_label: done.asr_label,
        llm_label: done.llm_label,
    })
}

#[tauri::command]
pub async fn history_reprocess(
    state: State<'_, AppState>,
    id: i64,
    profile_id: Option<String>,
) -> CmdResult<RedoDto> {
    let e = engine(&state)?;
    reprocess(&e, id, profile_id).await
}

/// Replaces a row's text with a result `history_reprocess` returned.
#[tauri::command]
pub fn history_apply_redo(state: State<'_, AppState>, id: i64, redo: RedoDto) -> CmdResult<()> {
    let e = engine(&state)?;
    if !e.history().apply_redo(HistoryId(id), &redo.redo())? {
        return Err(CmdError {
            message: "the history row no longer exists".into(),
        });
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
        }
    }
}

pub fn list(
    history: &HistoryStore,
    recordings_dir: &Path,
    limit: usize,
    query: Option<&str>,
    with_audio: bool,
) -> CmdResult<Vec<UtteranceDto>> {
    Ok(history
        .list(limit, query, with_audio)?
        .into_iter()
        .map(|u| UtteranceDto::new(u, recordings_dir))
        // A row whose file is gone has no recording to show.
        .filter(|u| !with_audio || u.has_audio)
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
        .ok_or_else(|| CmdError {
            message: "recording not found".into(),
        })
}

#[tauri::command]
pub fn history_list(
    state: State<'_, AppState>,
    limit: Option<usize>,
    query: Option<String>,
    with_audio: Option<bool>,
) -> CmdResult<Vec<UtteranceDto>> {
    let e = engine(&state)?;
    list(
        &e.history(),
        &e.dirs().recordings_dir(),
        limit.unwrap_or(200),
        query.as_deref(),
        with_audio.unwrap_or(false),
    )
}

#[tauri::command]
pub fn history_audio(state: State<'_, AppState>, id: i64) -> CmdResult<tauri::ipc::Response> {
    let e = engine(&state)?;
    Ok(tauri::ipc::Response::new(audio(
        &e.history(),
        &e.dirs().recordings_dir(),
        id,
    )?))
}

#[tauri::command]
pub fn history_delete_recording(state: State<'_, AppState>, id: i64) -> CmdResult<()> {
    let e = engine(&state)?;
    e.history()
        .delete_recording(HistoryId(id), &e.dirs().recordings_dir())?;
    Ok(())
}

/// Deletes every recording; returns how many files were removed.
#[tauri::command]
pub fn history_clear_recordings(state: State<'_, AppState>) -> CmdResult<usize> {
    let e = engine(&state)?;
    Ok(e.history().clear_recordings(&e.dirs().recordings_dir())?)
}

#[tauri::command]
pub fn history_reveal_recording(state: State<'_, AppState>, id: i64) -> CmdResult<()> {
    let e = engine(&state)?;
    let path = recording(&e.history(), &e.dirs().recordings_dir(), id)?;
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
}

#[tauri::command]
pub fn history_storage(state: State<'_, AppState>) -> CmdResult<StorageDto> {
    let e = engine(&state)?;
    let s = e.history().storage(&e.dirs().recordings_dir())?;
    Ok(StorageDto {
        rows: s.rows,
        recordings: s.recordings,
        recording_bytes: s.recording_bytes,
    })
}

#[tauri::command]
pub fn history_delete(state: State<'_, AppState>, id: i64) -> CmdResult<()> {
    let e = engine(&state)?;
    e.history()
        .delete_row(HistoryId(id), &e.dirs().recordings_dir())?;
    Ok(())
}

#[tauri::command]
pub fn history_clear(state: State<'_, AppState>) -> CmdResult<()> {
    let e = engine(&state)?;
    e.history().clear_all(&e.dirs().recordings_dir())?;
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
        }
    }

    #[test]
    fn a_row_reports_its_recording_only_when_the_file_is_in_the_recordings_directory() {
        let dir = tempfile::tempdir().unwrap();
        let elsewhere = tempfile::tempdir().unwrap();
        let history = HistoryStore::open_in_memory().unwrap();
        let kept = dir.path().join("a.wav");
        let stray = elsewhere.path().join("b.wav");
        std::fs::write(&kept, b"0123456789").unwrap();
        std::fs::write(&stray, b"01234").unwrap();
        history.insert(row(1, "kept", Some(kept))).unwrap();
        let stray_id = history.insert(row(2, "stray", Some(stray))).unwrap();
        history
            .insert(row(3, "gone", Some(dir.path().join("missing.wav"))))
            .unwrap();

        let rows = list(&history, dir.path(), 10, None, false).unwrap();
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
        let path = dir.path().join("a.wav");
        std::fs::write(&path, b"RIFFdata").unwrap();
        let id = history.insert(row(1, "with sound", Some(path))).unwrap();
        history.insert(row(2, "text only", None)).unwrap();

        assert_eq!(audio(&history, dir.path(), id.0).unwrap(), b"RIFFdata");
        let only = |q: Option<&str>, with_audio: bool| {
            list(&history, dir.path(), 10, q, with_audio)
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
