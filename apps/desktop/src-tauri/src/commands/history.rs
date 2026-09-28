use kongwe_core::history::Utterance;
use kongwe_core::types::HistoryId;
use kongwe_core::Engine;
use serde::Serialize;
use tauri::State;

use super::{engine, CmdResult};
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
    pub has_audio: bool,
    pub asr_ms: u64,
    pub llm_ms: Option<u64>,
}

impl From<Utterance> for UtteranceDto {
    fn from(u: Utterance) -> Self {
        Self {
            id: u.id.0,
            ts: u.ts,
            profile_id: u.profile_id.0,
            app_name: u.app_name,
            raw_text: u.raw_text,
            refined_text: u.refined_text,
            final_text: u.final_text,
            degraded: u.degraded,
            has_audio: u.audio_path.is_some(),
            asr_ms: u.asr_ms,
            llm_ms: u.llm_ms,
        }
    }
}

pub fn list(engine: &Engine, limit: usize) -> CmdResult<Vec<UtteranceDto>> {
    Ok(engine
        .history()
        .list_recent(limit)?
        .into_iter()
        .map(Into::into)
        .collect())
}

#[tauri::command]
pub fn history_list(
    state: State<'_, AppState>,
    limit: Option<usize>,
) -> CmdResult<Vec<UtteranceDto>> {
    list(&engine(&state)?, limit.unwrap_or(200))
}

#[tauri::command]
pub fn history_delete(state: State<'_, AppState>, id: i64) -> CmdResult<()> {
    engine(&state)?.history().delete(HistoryId(id))?;
    Ok(())
}

#[tauri::command]
pub fn history_clear(state: State<'_, AppState>) -> CmdResult<()> {
    let e = engine(&state)?;
    e.history().clear_all(&e.dirs().recordings_dir())?;
    Ok(())
}
