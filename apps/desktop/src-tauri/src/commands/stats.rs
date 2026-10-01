use arcvox_core::stats::Stats;
use tauri::State;

use super::{engine, CmdResult};
use crate::app::AppState;

/// Statistics from `since` (seconds since the epoch) on, with days and hours
/// taken at `tz_offset_secs` east of UTC.
#[tauri::command]
pub fn stats_get(state: State<'_, AppState>, since: i64, tz_offset_secs: i32) -> CmdResult<Stats> {
    Ok(engine(&state)?.history().stats(since, tz_offset_secs)?)
}

#[tauri::command]
pub fn stats_reset(state: State<'_, AppState>) -> CmdResult<()> {
    engine(&state)?.history().reset_stats()?;
    Ok(())
}
