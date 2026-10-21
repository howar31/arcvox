//! Quality test: scores history rows or a seed set file through engines or
//! a whole profile, keeps the run in progress and the finished report, and
//! saves named reports beside the command line's.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use arcvox_core::eval::run::report_file_name;
use arcvox_core::eval::{
    categories, compare, list_reports, parse_seed_tsv, pick_slice, rows_from_history, summarize,
    Comparison, ItemResult, Origin, Report, ReportHead, RunRequest, SeedRow, Stage, Summary,
};
use arcvox_core::history::{HistoryStore, ListFilter};
use arcvox_core::types::ProviderId;
use arcvox_core::Engine;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};

use super::{engine, CmdError, CmdResult};
use crate::app::AppState;

const SETTINGS_WINDOW: &str = "settings";
/// Every history row with a recording is a candidate; the limit cuts the set.
const ALL_ROWS: usize = 100_000;

/// Where the rows come from.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Source {
    /// History rows with a recording in the window, at most `limit` of
    /// them spread over the categories.
    History {
        since: Option<i64>,
        until: Option<i64>,
        limit: usize,
    },
    /// A seed set file as the command line reads it.
    File { path: String },
}

/// What a source holds.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SourceInfo {
    /// Rows the run would take.
    pub rows: usize,
    /// Rows the source has before the limit.
    pub available: usize,
    /// Rows whose transcript was corrected by hand (history only).
    pub heard: usize,
    /// Rows per category, in first-appearance order.
    pub categories: Vec<(String, usize)>,
}

/// The run in progress or the last one finished.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RunState {
    pub running: bool,
    pub stage: Stage,
    pub profile: String,
    /// Rows in the set; `total` counts them once per engine.
    pub rows: usize,
    pub done: usize,
    pub total: usize,
    /// The engine the last row ran on.
    pub provider: Option<String>,
    /// Unix seconds when the run began.
    pub started: i64,
    pub elapsed_ms: u64,
    pub stopped: bool,
    /// Why the run ended early, when it failed.
    pub error: Option<String>,
    pub items: Vec<ItemResult>,
    pub summaries: Vec<Summary>,
    /// The file the report was saved to, once it was.
    pub saved: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct ProgressEvent {
    done: usize,
    total: usize,
    provider: String,
    item: ItemResult,
    summaries: Vec<Summary>,
}

#[derive(Debug, Clone, Serialize)]
struct DoneEvent {
    stopped: bool,
    error: Option<String>,
}

#[derive(Default)]
pub struct QualityState {
    task: Mutex<Option<tauri::async_runtime::JoinHandle<()>>>,
    cancel: Arc<AtomicBool>,
    run: Arc<Mutex<Option<Current>>>,
}

/// The run's record, filled as rows come in.
#[derive(Debug, Clone)]
struct Current {
    state: RunState,
    report: Option<Report>,
}

impl QualityState {
    /// Asks the run in progress to stop after the row it is on.
    pub fn stop(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// The rows a source gives, in the order they run.
fn rows_of(
    history: &HistoryStore,
    recordings_dir: &Path,
    source: &Source,
) -> CmdResult<(Vec<SeedRow>, SourceInfo)> {
    match source {
        Source::History {
            since,
            until,
            limit,
        } => {
            let filter = ListFilter {
                query: None,
                with_audio: true,
                since: *since,
                until: *until,
                before: None,
            };
            let rows = history.list_page(ALL_ROWS, &filter)?;
            let heard = rows.iter().filter(|u| u.heard_text.is_some()).count();
            let mut set: Vec<SeedRow> = rows_from_history(&rows)
                .into_iter()
                .filter(|r| r.audio.is_file())
                .collect();
            // The recordings folder is already resolved into the paths;
            // `recordings_dir` only documents where they are.
            let _ = recordings_dir;
            let available = set.len();
            if *limit > 0 && set.len() > *limit {
                set = pick_slice(&set, *limit);
            }
            let cats = category_counts(&set);
            Ok((
                set,
                SourceInfo {
                    rows: available.min(if *limit > 0 { *limit } else { available }),
                    available,
                    heard,
                    categories: cats,
                },
            ))
        }
        Source::File { path } => {
            let text = std::fs::read_to_string(path)
                .map_err(|e| CmdError::with("file_unreadable", e.to_string(), &[]))?;
            let set = parse_seed_tsv(&text).map_err(|e| CmdError::with("set_invalid", e, &[]))?;
            let available = set.len();
            let cats = category_counts(&set);
            Ok((
                set,
                SourceInfo {
                    rows: available,
                    available,
                    heard: 0,
                    categories: cats,
                },
            ))
        }
    }
}

fn category_counts(set: &[SeedRow]) -> Vec<(String, usize)> {
    let mut out: Vec<(String, usize)> = Vec::new();
    for r in set {
        for c in categories(&r.final_text, &r.tags) {
            match out.iter_mut().find(|(n, _)| *n == c) {
                Some((_, n)) => *n += 1,
                None => out.push((c, 1)),
            }
        }
    }
    out
}

/// What a source holds, for the page's count line.
pub fn source_info(e: &Engine, source: &Source) -> CmdResult<SourceInfo> {
    let (_, info) = rows_of(&e.history(), &e.recordings_dir(), source)?;
    Ok(info)
}

/// What the page needs to start a run.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct StartArgs {
    pub source: Source,
    pub stage: Stage,
    pub profile: String,
    pub asr: Vec<String>,
    pub llm: Vec<String>,
}

#[tauri::command]
pub fn quality_source_info(source: Source, state: State<'_, AppState>) -> CmdResult<SourceInfo> {
    source_info(&engine(&state)?, &source)
}

/// Starts a run in the background, replacing a finished one; a run in
/// progress is refused. Rows arrive as `quality-progress` events and the
/// end as `quality-done`.
#[tauri::command]
pub fn quality_start(args: StartArgs, app: AppHandle, state: State<'_, AppState>) -> CmdResult<()> {
    let e = engine(&state)?;
    {
        let run = state.quality.run.lock().unwrap();
        if run.as_ref().is_some_and(|c| c.state.running) {
            return Err(CmdError::new(
                "quality_running",
                "a quality test is running",
            ));
        }
    }
    let config = e.config().current();
    let profile = config
        .profiles
        .iter()
        .find(|p| p.id.0 == args.profile)
        .cloned()
        .ok_or_else(|| CmdError::new("profile_gone", "profile not found"))?;
    let (rows, info) = rows_of(&e.history(), &e.recordings_dir(), &args.source)?;
    if rows.is_empty() {
        return Err(CmdError::new("quality_no_rows", "no rows to test"));
    }
    let source_note = match &args.source {
        Source::History { limit, .. } => format!(
            "set: history ({} of {} rows{})",
            rows.len(),
            info.available,
            if *limit > 0 {
                format!(", limit {limit}")
            } else {
                String::new()
            }
        ),
        Source::File { path } => format!("set: {path} ({} rows)", rows.len()),
    };
    let row_count = rows.len();
    let total = match args.stage {
        Stage::Asr => rows.len() * args.asr.len().max(1),
        Stage::Llm => rows.len() * args.llm.len().max(1),
        Stage::Full => rows.len(),
    };
    let request = RunRequest {
        name: String::new(),
        stage: args.stage,
        rows,
        profile: profile.clone(),
        asr: args.asr.into_iter().map(ProviderId).collect(),
        llm: args.llm.into_iter().map(ProviderId).collect(),
        base_prompt: String::new(),
        translate_prompt: String::new(),
        asr_sample: None,
        examples: None,
        notes: vec![
            format!("profile: {}", profile.id.0),
            "base prompt: the installation's".to_string(),
            source_note,
        ],
        origin: Origin::App,
    };
    let current = Current {
        state: RunState {
            running: true,
            stage: args.stage,
            profile: profile.id.0.clone(),
            rows: row_count,
            done: 0,
            total,
            provider: None,
            started: now(),
            elapsed_ms: 0,
            stopped: false,
            error: None,
            items: Vec::new(),
            summaries: Vec::new(),
            saved: None,
        },
        report: None,
    };
    *state.quality.run.lock().unwrap() = Some(current);
    state.quality.cancel.store(false, Ordering::SeqCst);
    let cancel = state.quality.cancel.clone();
    let run = state.quality.run.clone();
    let mut task = state.quality.task.lock().unwrap();
    if let Some(old) = task.take() {
        old.abort();
    }
    *task = Some(tauri::async_runtime::spawn(async move {
        let started = std::time::Instant::now();
        let progress_app = app.clone();
        let progress_run = run.clone();
        let result = e
            .quality_run(request, &|| cancel.load(Ordering::SeqCst), &mut |p| {
                let items = {
                    let mut guard = progress_run.lock().unwrap();
                    let Some(c) = guard.as_mut() else { return };
                    c.state.done = p.done;
                    c.state.total = p.total;
                    c.state.provider = Some(p.provider.to_string());
                    c.state.elapsed_ms = started.elapsed().as_millis() as u64;
                    c.state.items.push(p.item.clone());
                    c.state.items.clone()
                };
                let summaries = summarize(&Report {
                    name: String::new(),
                    created: String::new(),
                    notes: Vec::new(),
                    items,
                    origin: Origin::App,
                    stopped: false,
                });
                if let Some(c) = progress_run.lock().unwrap().as_mut() {
                    c.state.summaries = summaries.clone();
                }
                let _ = progress_app.emit_to(
                    SETTINGS_WINDOW,
                    "quality-progress",
                    ProgressEvent {
                        done: p.done,
                        total: p.total,
                        provider: p.provider.to_string(),
                        item: p.item.clone(),
                        summaries,
                    },
                );
            })
            .await;
        let (stopped, error) = {
            let mut guard = run.lock().unwrap();
            let Some(c) = guard.as_mut() else { return };
            c.state.running = false;
            c.state.elapsed_ms = started.elapsed().as_millis() as u64;
            match result {
                Ok(report) => {
                    c.state.stopped = report.stopped;
                    c.state.summaries = summarize(&report);
                    c.report = Some(report);
                }
                Err(e) => c.state.error = Some(e),
            }
            (c.state.stopped, c.state.error.clone())
        };
        let _ = app.emit_to(
            SETTINGS_WINDOW,
            "quality-done",
            DoneEvent { stopped, error },
        );
    }));
    Ok(())
}

/// Stops the run after the row it is on.
#[tauri::command]
pub fn quality_cancel(state: State<'_, AppState>) -> CmdResult<()> {
    state.quality.stop();
    Ok(())
}

/// The run in progress or the last one, with every row so far.
#[tauri::command]
pub fn quality_current(state: State<'_, AppState>) -> CmdResult<Option<RunState>> {
    Ok(state
        .quality
        .run
        .lock()
        .unwrap()
        .as_ref()
        .map(|c| c.state.clone()))
}

/// Forgets the last run, so the page starts from its settings again.
#[tauri::command]
pub fn quality_clear(state: State<'_, AppState>) -> CmdResult<()> {
    let mut run = state.quality.run.lock().unwrap();
    if run.as_ref().is_some_and(|c| c.state.running) {
        return Err(CmdError::new(
            "quality_running",
            "a quality test is running",
        ));
    }
    *run = None;
    Ok(())
}

/// Saves the finished run under `name` into the reports folder.
#[tauri::command]
pub fn quality_save(name: String, state: State<'_, AppState>) -> CmdResult<ReportHead> {
    let dir = state.dirs.quality_dir();
    let mut run = state.quality.run.lock().unwrap();
    let current = run
        .as_mut()
        .ok_or_else(|| CmdError::new("quality_no_run", "no run to save"))?;
    if current.state.running {
        return Err(CmdError::new(
            "quality_running",
            "a quality test is running",
        ));
    }
    let report = current
        .report
        .as_mut()
        .ok_or_else(|| CmdError::new("quality_no_run", "no run to save"))?;
    let name = name.trim();
    report.name = if name.is_empty() {
        "run".to_string()
    } else {
        name.to_string()
    };
    let head = save_report(&dir, report)?;
    current.state.saved = Some(head.file.clone());
    Ok(head)
}

pub fn save_report(dir: &Path, report: &Report) -> CmdResult<ReportHead> {
    std::fs::create_dir_all(dir)?;
    let path = report_file_name(dir, &report.name);
    let text = serde_json::to_string_pretty(report).map_err(|e| CmdError::plain(e.to_string()))?;
    std::fs::write(&path, text)?;
    let file = path.file_name().unwrap().to_string_lossy().into_owned();
    ReportHead::of(&file, report)
        .ok_or_else(|| CmdError::new("quality_empty", "the run has no rows"))
}

/// The saved reports, newest first.
#[tauri::command]
pub fn quality_list(state: State<'_, AppState>) -> CmdResult<Vec<ReportHead>> {
    Ok(list_reports(&state.dirs.quality_dir()))
}

/// A saved report with its rows and totals.
#[derive(Debug, Clone, Serialize)]
pub struct OpenedReport {
    pub report: Report,
    pub summaries: Vec<Summary>,
}

fn report_path(dir: &Path, file: &str) -> CmdResult<PathBuf> {
    // The file name comes from a listing of the folder; a path is refused.
    if file.is_empty() || file.contains('/') || file.contains('\\') || file.starts_with('.') {
        return Err(CmdError::new("quality_no_report", "report not found"));
    }
    let path = dir.join(file);
    if !path.is_file() {
        return Err(CmdError::new("quality_no_report", "report not found"));
    }
    Ok(path)
}

pub fn load_report(dir: &Path, file: &str) -> CmdResult<Report> {
    let path = report_path(dir, file)?;
    let text = std::fs::read_to_string(&path)?;
    serde_json::from_str(&text)
        .map_err(|e| CmdError::with("quality_bad_report", e.to_string(), &[]))
}

#[tauri::command]
pub fn quality_open(file: String, state: State<'_, AppState>) -> CmdResult<OpenedReport> {
    let report = load_report(&state.dirs.quality_dir(), &file)?;
    let summaries = summarize(&report);
    Ok(OpenedReport { report, summaries })
}

#[tauri::command]
pub fn quality_delete(file: String, state: State<'_, AppState>) -> CmdResult<()> {
    let path = report_path(&state.dirs.quality_dir(), &file)?;
    std::fs::remove_file(path)?;
    Ok(())
}

/// Two saved reports side by side.
#[tauri::command]
pub fn quality_compare(
    before: String,
    after: String,
    state: State<'_, AppState>,
) -> CmdResult<Comparison> {
    let dir = state.dirs.quality_dir();
    let a = load_report(&dir, &before)?;
    let b = load_report(&dir, &after)?;
    Ok(compare(&a, &b))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: &str, spoken: &str, final_text: &str, tags: &[&str]) -> SeedRow {
        SeedRow {
            id: id.into(),
            audio: PathBuf::new(),
            spoken: spoken.into(),
            final_text: final_text.into(),
            tags: tags.iter().map(|t| t.to_string()).collect(),
        }
    }

    #[test]
    fn categories_are_counted_in_first_appearance_order() {
        let set = vec![
            row("1", "a", "你好嗎？", &[]),
            row("2", "b", "再見。", &["short"]),
            row("3", "c", "可以嗎？", &[]),
        ];
        let counts = category_counts(&set);
        let q = counts.iter().find(|(n, _)| n == "question").unwrap();
        assert_eq!(q.1, 2);
        assert!(counts.iter().any(|(n, c)| n == "short" && *c == 1));
    }

    #[test]
    fn a_file_source_reads_the_set_and_counts_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("set.tsv");
        std::fs::write(
            &path,
            "id\taudio\ttags\tspoken\tfinal\n1\t/x.wav\tshort\t你好\t你好。\n2\t/y.wav\t\t可以嗎\t可以嗎？\n",
        )
        .unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        let set = parse_seed_tsv(&text).unwrap();
        assert_eq!(set.len(), 2);
        let counts = category_counts(&set);
        assert!(counts.iter().any(|(n, c)| n == "question" && *c == 1));
    }

    #[test]
    fn saved_reports_are_listed_and_opened_by_file_name_only() {
        let dir = tempfile::tempdir().unwrap();
        let report = Report {
            name: "試 一".into(),
            created: "5".into(),
            notes: vec![],
            items: vec![ItemResult::scored(
                "1",
                Stage::Llm,
                "g",
                &[],
                "你好。",
                Ok("你好".into()),
                3,
            )],
            origin: Origin::App,
            stopped: false,
        };
        let head = save_report(dir.path(), &report).unwrap();
        assert_eq!(head.file, "試 一.json");
        assert_eq!(head.origin, Origin::App);
        let opened = load_report(dir.path(), &head.file).unwrap();
        assert_eq!(opened, report);
        assert!(load_report(dir.path(), "../x.json").is_err());
        assert!(load_report(dir.path(), "nope.json").is_err());
        let heads = list_reports(dir.path());
        assert_eq!(heads.len(), 1);
        assert_eq!(heads[0].punctuation.errors, 1);
    }

    #[test]
    fn a_report_file_name_is_never_a_path() {
        let dir = tempfile::tempdir().unwrap();
        assert!(report_path(dir.path(), "").is_err());
        assert!(report_path(dir.path(), "a/b.json").is_err());
        assert!(report_path(dir.path(), ".hidden").is_err());
    }
}
