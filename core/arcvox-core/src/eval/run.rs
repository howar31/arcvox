//! Runs a seed set through providers or a whole profile and scores every
//! row; the command line and the application's quality page share it.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use serde::{Deserialize, Serialize};

use super::{
    guess_tags, pick_examples, summarize, Cer, ExamplePick, ItemResult, Origin, Report, SeedRow,
    Stage,
};
use crate::config::{Config, Profile, ProviderConfig};
use crate::dictionary::asr_hint;
use crate::engine::Engine;
use crate::history::Utterance;
use crate::pipeline::Pipeline;
use crate::providers::registry::ProviderRegistry;
use crate::providers::AsrHint;
use crate::steps::asr::{asr_prompt, ASR_HINT_BUDGET_BYTES};
use crate::steps::{translate, PipelineContext, Step, StepInput};
use crate::types::{AudioClip, Context, ProviderId};

/// The pool the rewrite model's examples are picked from, per row.
pub struct Examples {
    pub pool: Vec<SeedRow>,
    pub k: usize,
    pub pick: ExamplePick,
}

impl Examples {
    pub fn describe(&self, path: &str) -> String {
        format!(
            "{path} ({} rows), {} per row, {}",
            self.pool.len(),
            self.k,
            match self.pick {
                ExamplePick::Similar => "the most similar spoken text",
                ExamplePick::Random => "a fixed arbitrary choice",
            }
        )
    }

    /// The context with the row's own examples in the profile; the steps
    /// stay the pipeline's, only the prompt reads the profile's examples.
    fn for_row(&self, ctx: &PipelineContext, row: &SeedRow) -> PipelineContext {
        let mut profile = (*ctx.profile).clone();
        profile.few_shot = pick_examples(&self.pool, &row.id, &row.spoken, self.k, self.pick);
        PipelineContext {
            profile: Arc::new(profile),
            ..ctx.clone()
        }
    }
}

/// What to run and with what.
pub struct RunRequest {
    pub name: String,
    pub stage: Stage,
    pub rows: Vec<SeedRow>,
    pub profile: Profile,
    /// The speech engines for the `asr` stage, or at most one for `full`;
    /// empty means the profile's own.
    pub asr: Vec<ProviderId>,
    /// The rewrite engines for the `llm` stage, or at most one for `full`;
    /// empty means the profile's own.
    pub llm: Vec<ProviderId>,
    pub base_prompt: String,
    pub translate_prompt: String,
    pub asr_sample: Option<String>,
    pub examples: Option<Examples>,
    /// Lines for the report's record, before the ones the run adds.
    pub notes: Vec<String>,
    pub origin: Origin,
}

/// One row done: how far the run is and what the row gave.
pub struct Progress<'a> {
    pub provider: &'a str,
    pub done: usize,
    pub total: usize,
    pub item: &'a ItemResult,
}

/// Unix seconds now, as the report writes them.
pub fn now_secs() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{secs}")
}

fn load_clip(path: &Path) -> Result<Arc<AudioClip>, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    crate::audio::decode_wav_pcm16(&bytes)
        .map(Arc::new)
        .map_err(|e| format!("{}: {e}", path.display()))
}

/// An event sender nobody listens to: the pipeline's progress events are
/// not needed here, and a sender with a live but unread receiver would block
/// once the channel is full.
fn closed_events() -> tokio::sync::mpsc::Sender<crate::Event> {
    let (tx, rx) = tokio::sync::mpsc::channel(1);
    drop(rx);
    tx
}

fn context(config: &Config, profile: Profile, request: &RunRequest) -> PipelineContext {
    PipelineContext {
        profile: Arc::new(profile),
        context: Context::default(),
        dictionary: Arc::new(config.dictionary.clone()),
        base_prompt: request.base_prompt.clone(),
        translate_prompt: request.translate_prompt.clone(),
        general: config.general.clone(),
        recognised: Default::default(),
        asr_sample: request.asr_sample.clone(),
    }
}

fn build(
    config: &Config,
    profile: Profile,
    registry: &ProviderRegistry,
    request: &RunRequest,
) -> Result<(Pipeline, PipelineContext), String> {
    let engines = config.resolve_engines(&profile, |cfg| registry.ready(cfg));
    let pipeline = Pipeline::build(config, &profile, &engines, registry).map_err(|e| e.message)?;
    Ok((pipeline, context(config, profile, request)))
}

/// Scores every row of `request` and reports each as it is done. `engine_for`
/// supplies a configuration for an engine id the configuration lacks (the
/// command line makes one for an installed catalogue model); `cancelled`
/// is read before every row, and a run stopped that way returns what it
/// has, marked `stopped`.
pub async fn run(
    request: RunRequest,
    config: &Config,
    registry: &ProviderRegistry,
    engine_for: &(dyn Fn(&str) -> Option<ProviderConfig> + Sync),
    cancelled: &(dyn Fn() -> bool + Sync),
    on_item: &mut (dyn FnMut(Progress<'_>) + Send),
) -> Result<Report, String> {
    let mut notes = request.notes.clone();
    let mut items: Vec<ItemResult> = Vec::new();
    let started = Instant::now();
    let rows = &request.rows;
    let mut stopped = false;
    let mut done = 0usize;
    // Pushes a scored row and tells the caller.
    macro_rules! record {
        ($total:expr, $provider:expr, $item:expr) => {{
            let item = $item;
            done += 1;
            on_item(Progress {
                provider: $provider,
                done,
                total: $total,
                item: &item,
            });
            items.push(item);
        }};
    }

    match request.stage {
        Stage::Asr => {
            let ids: Vec<String> = if request.asr.is_empty() {
                vec![config
                    .resolve_engines(&request.profile, |cfg| registry.ready(cfg))
                    .asr
                    .id
                    .0
                    .clone()]
            } else {
                request.asr.iter().map(|id| id.0.clone()).collect()
            };
            notes.push(format!("asr providers: {}", ids.join(", ")));
            let hint = AsrHint {
                language: request.profile.language_hint.clone(),
                prompt: asr_prompt(
                    request.asr_sample.as_deref(),
                    asr_hint(&config.dictionary, ASR_HINT_BUDGET_BYTES),
                ),
            };
            let total = ids.len() * rows.len();
            'engines: for id in &ids {
                let made;
                let cfg = match config.provider(&ProviderId(id.clone())) {
                    Some(cfg) => cfg,
                    None => {
                        made = engine_for(id)
                            .ok_or_else(|| format!("no provider or catalogue model `{id}`"))?;
                        &made
                    }
                };
                let provider = registry
                    .asr(cfg, &config.general)
                    .map_err(|e| format!("{id}: {}", e.message))?;
                for row in rows {
                    if cancelled() {
                        stopped = true;
                        break 'engines;
                    }
                    let result = match load_clip(&row.audio) {
                        Ok(clip) => {
                            let t0 = Instant::now();
                            let r = provider
                                .transcribe(&clip, &hint)
                                .await
                                .map(|t| t.text)
                                .map_err(|e| e.message);
                            (r, t0.elapsed().as_millis() as u64)
                        }
                        Err(e) => (Err(e), 0),
                    };
                    record!(
                        total,
                        id,
                        ItemResult::scored(
                            &row.id,
                            request.stage,
                            id,
                            &row.tags,
                            &row.spoken,
                            result.0,
                            result.1,
                        )
                    );
                }
            }
        }
        Stage::Llm => {
            let ids: Vec<String> = if request.llm.is_empty() {
                vec![config
                    .resolve_engines(&request.profile, |cfg| registry.ready(cfg))
                    .llm
                    .map(|c| c.id.0)
                    .ok_or("the profile has no rewrite step; pass --llm")?]
            } else {
                request.llm.iter().map(|id| id.0.clone()).collect()
            };
            notes.push(format!("llm providers: {}", ids.join(", ")));
            let total = ids.len() * rows.len();
            'engines: for id in &ids {
                let mut prof = request.profile.clone();
                prof.llm_provider = Some(ProviderId(id.clone()));
                let (pipeline, ctx) = build(config, prof, registry, &request)?;
                for row in rows {
                    if cancelled() {
                        stopped = true;
                        break 'engines;
                    }
                    let ctx = match &request.examples {
                        Some(e) => e.for_row(&ctx, row),
                        None => ctx.clone(),
                    };
                    let tx = closed_events();
                    let t0 = Instant::now();
                    let r = pipeline
                        .run_from_text(&row.spoken, &ctx, &tx)
                        .await
                        .map(|o| o.final_text)
                        .map_err(|e| e.message);
                    let ms = t0.elapsed().as_millis() as u64;
                    record!(
                        total,
                        id,
                        ItemResult::scored(
                            &row.id,
                            request.stage,
                            id,
                            &row.tags,
                            &row.final_text,
                            r,
                            ms,
                        )
                    );
                }
            }
        }
        Stage::Full => {
            let mut prof = request.profile.clone();
            if request.asr.len() > 1 || request.llm.len() > 1 {
                return Err(
                    "the full stage takes at most one speech and one rewrite engine".into(),
                );
            }
            if let Some(a) = request.asr.first() {
                prof.asr_provider = Some(a.clone());
            }
            if let Some(l) = request.llm.first() {
                prof.llm_provider = Some(l.clone());
            }
            let engines = config.resolve_engines(&prof, |cfg| registry.ready(cfg));
            let label = format!(
                "{}+{}",
                engines.asr.id.0,
                engines
                    .llm
                    .as_ref()
                    .map(|c| c.id.0.as_str())
                    .unwrap_or("none")
            );
            notes.push(format!("pipeline: {label}"));
            let (pipeline, ctx) = build(config, prof, registry, &request)?;
            let total = rows.len();
            for row in rows {
                if cancelled() {
                    stopped = true;
                    break;
                }
                let result = match load_clip(&row.audio) {
                    Ok(clip) => {
                        let tx = closed_events();
                        let t0 = Instant::now();
                        let r = pipeline
                            .run(clip, &ctx, &tx)
                            .await
                            .map(|o| o.final_text)
                            .map_err(|e| e.message);
                        (r, t0.elapsed().as_millis() as u64)
                    }
                    Err(e) => (Err(e), 0),
                };
                record!(
                    total,
                    &label,
                    ItemResult::scored(
                        &row.id,
                        request.stage,
                        &label,
                        &row.tags,
                        &row.final_text,
                        result.0,
                        result.1,
                    )
                );
            }
        }
        Stage::Translate => {
            let (target, addition, _) = request
                .profile
                .translate_step()
                .map(|(t, a, p)| (t.to_string(), a.to_string(), p.cloned()))
                .ok_or("the profile has no translation module")?;
            let ids: Vec<String> = if request.llm.is_empty() {
                vec![config
                    .resolve_engines(&request.profile, |cfg| registry.ready(cfg))
                    .translate
                    .map(|c| c.id.0)
                    .ok_or("the profile has no translation engine; pass --llm")?]
            } else {
                request.llm.iter().map(|id| id.0.clone()).collect()
            };
            notes.push(format!("translate providers: {}", ids.join(", ")));
            notes.push(format!("target: {}", translate::target_name(&target)));
            let references = rows.iter().filter(|r| r.translated.is_some()).count();
            notes.push(format!(
                "reference translations: {references} of {} rows",
                rows.len()
            ));
            let total = ids.len() * rows.len();
            let ctx = context(config, request.profile.clone(), &request);
            'engines: for id in &ids {
                let cfg = config
                    .provider(&ProviderId(id.clone()))
                    .ok_or_else(|| format!("no provider `{id}`"))?;
                let provider = registry
                    .llm(cfg, &config.general)
                    .map_err(|e| format!("{id}: {}", e.message))?;
                let step = translate::TranslateStep::new(provider, &target, &addition);
                for row in rows {
                    if cancelled() {
                        stopped = true;
                        break 'engines;
                    }
                    let t0 = Instant::now();
                    let input = StepInput {
                        text: row.final_text.clone(),
                        audio: None,
                    };
                    let r = step
                        .run(input, &ctx)
                        .await
                        .map(|o| o.text)
                        .map_err(|e| e.message);
                    let ms = t0.elapsed().as_millis() as u64;
                    let item = match &row.translated {
                        Some(expected) => ItemResult::scored(
                            &row.id,
                            request.stage,
                            id,
                            &row.tags,
                            expected,
                            r,
                            ms,
                        ),
                        None => ItemResult::unscored(&row.id, request.stage, id, &row.tags, r, ms),
                    };
                    record!(total, id, item);
                }
            }
        }
    }
    notes.push(format!("wall time: {} s", started.elapsed().as_secs()));
    if stopped {
        notes.push(format!("stopped after {} of {} rows", done, rows.len()));
    }
    Ok(Report {
        name: request.name,
        created: now_secs(),
        notes,
        items,
        origin: request.origin,
        stopped,
    })
}

/// History rows as a seed set: the corrected transcript, or the raw one
/// where nobody corrected it, as `spoken`; the final text as `final`;
/// tags guessed from the raw transcript. Rows without a recording or with
/// a failed dictation are left out.
pub fn rows_from_history(rows: &[Utterance]) -> Vec<SeedRow> {
    rows.iter()
        .filter(|u| u.error.is_none())
        .filter_map(|u| {
            let audio = u.audio_path.clone()?;
            Some(SeedRow {
                id: u.id.0.to_string(),
                audio,
                tags: guess_tags(&u.raw_text, u.audio_ms),
                spoken: u.heard_text.clone().unwrap_or_else(|| u.raw_text.clone()),
                final_text: u.final_text.clone(),
                translated: None,
            })
        })
        .collect()
}

/// A saved report in brief: the file it is in and the scores of its one
/// stage (every provider of the run added up).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReportHead {
    /// The file name inside the reports folder.
    pub file: String,
    pub name: String,
    pub created: String,
    pub origin: Origin,
    pub stopped: bool,
    pub stage: Stage,
    pub providers: Vec<String>,
    pub items: usize,
    pub failed: usize,
    pub text: Cer,
    pub punctuation: Cer,
}

impl ReportHead {
    pub fn of(file: &str, report: &Report) -> Option<ReportHead> {
        let summaries = summarize(report);
        let stage = summaries.first()?.stage;
        let mut text = Cer::default();
        let mut punctuation = Cer::default();
        let mut failed = 0;
        for s in &summaries {
            text.errors += s.total.text.errors;
            text.length += s.total.text.length;
            punctuation.errors += s.total.punctuation.errors;
            punctuation.length += s.total.punctuation.length;
            failed += s.failed;
        }
        Some(ReportHead {
            file: file.to_string(),
            name: report.name.clone(),
            created: report.created.clone(),
            origin: report.origin,
            stopped: report.stopped,
            stage,
            providers: summaries.iter().map(|s| s.provider.clone()).collect(),
            items: report.items.len(),
            failed,
            text,
            punctuation,
        })
    }
}

/// The reports saved in `dir`, newest first; a file that is not a report
/// is skipped.
pub fn list_reports(dir: &Path) -> Vec<ReportHead> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut heads: Vec<ReportHead> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .filter_map(|e| {
            let text = std::fs::read_to_string(e.path()).ok()?;
            let report: Report = serde_json::from_str(&text).ok()?;
            ReportHead::of(&e.file_name().to_string_lossy(), &report)
        })
        .collect();
    heads.sort_by(|a, b| {
        let at = a.created.parse::<i64>().unwrap_or(0);
        let bt = b.created.parse::<i64>().unwrap_or(0);
        bt.cmp(&at).then_with(|| a.file.cmp(&b.file))
    });
    heads
}

/// A file name for a report: the name with what a file name cannot hold
/// replaced, and a number when the name is taken.
pub fn report_file_name(dir: &Path, name: &str) -> PathBuf {
    let stem: String = name
        .trim()
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
                '_'
            } else {
                c
            }
        })
        .collect();
    let stem = if stem.is_empty() {
        "run".to_string()
    } else {
        stem
    };
    let mut path = dir.join(format!("{stem}.json"));
    let mut n = 2;
    while path.exists() {
        path = dir.join(format!("{stem}-{n}.json"));
        n += 1;
    }
    path
}

/// Rearms the idle unload when a run ends, also when its future is dropped.
struct IdleOnDrop<'a>(&'a Engine);

impl Drop for IdleOnDrop<'_> {
    fn drop(&mut self) {
        self.0.touch_idle();
    }
}

impl Engine {
    /// Runs `request` on this engine's configuration and providers, the base
    /// and translation prompts taken from the installation; the speech
    /// engines loaded for the run alone are freed afterwards.
    pub async fn quality_run(
        &self,
        mut request: RunRequest,
        cancelled: &(dyn Fn() -> bool + Sync),
        on_item: &mut (dyn FnMut(Progress<'_>) + Send),
    ) -> Result<Report, String> {
        self.mark_busy();
        let _idle = IdleOnDrop(self);
        let config = self.config().current();
        request.base_prompt = self
            .config()
            .base_prompt()
            .unwrap_or_else(|_| crate::config::base_prompt::DEFAULT_BASE_PROMPT.to_string());
        request.translate_prompt = self
            .config()
            .translate_prompt()
            .unwrap_or_else(|_| crate::config::base_prompt::DEFAULT_TRANSLATE_PROMPT.to_string());
        let result = run(
            request,
            &config,
            self.registry(),
            &|_| None,
            cancelled,
            on_item,
        )
        .await;
        self.unload_unreferenced_asr().await;
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::HistoryId;

    fn row(id: i64, raw: &str, heard: Option<&str>, final_text: &str, audio: bool) -> Utterance {
        Utterance {
            id: HistoryId(id),
            ts: 0,
            profile_id: crate::types::ProfileId("default".into()),
            app_name: None,
            raw_text: raw.into(),
            refined_text: final_text.into(),
            final_text: final_text.into(),
            heard_text: heard.map(str::to_string),
            degraded: false,
            audio_path: audio.then(|| PathBuf::from("/tmp/x.wav")),
            asr_ms: 0,
            llm_ms: None,
            audio_ms: Some(3000),
            error: None,
            error_code: None,
            redone: 0,
            correction_source: None,
            origin_correction: None,
            translated: false,
        }
    }

    #[tokio::test]
    async fn the_translate_stage_runs_the_module_alone_and_scores_only_rows_with_a_reference() {
        use crate::config::StepSpec;
        use crate::providers::registry::tests::{embedded_cfg, FakeFactory};
        use crate::secrets::MemorySecretStore;
        let mut config = Config::http_test_config();
        let mut emb = embedded_cfg();
        emb.capabilities.chat = true;
        config.providers.push(emb.clone());
        let registry = ProviderRegistry::with_factories(
            reqwest::Client::new(),
            Arc::new(MemorySecretStore::default()),
            vec![Arc::new(FakeFactory)],
        );
        let mut profile = config.default_profile().clone();
        profile.steps.push(StepSpec::Translate {
            off: false,
            target: "en".into(),
            addition: String::new(),
            provider: Some(emb.id.clone()),
        });
        let rows = vec![
            SeedRow {
                id: "1".into(),
                audio: PathBuf::new(),
                spoken: "嗨".into(),
                final_text: "嗨。".into(),
                tags: vec![],
                translated: Some("hi".into()),
            },
            SeedRow {
                id: "2".into(),
                audio: PathBuf::new(),
                spoken: "好".into(),
                final_text: "好。".into(),
                tags: vec![],
                translated: None,
            },
        ];
        let request = RunRequest {
            name: "t".into(),
            stage: Stage::Translate,
            rows,
            profile,
            asr: vec![],
            llm: vec![],
            base_prompt: String::new(),
            translate_prompt: String::new(),
            asr_sample: None,
            examples: None,
            notes: vec![],
            origin: Origin::Command,
        };
        let report = run(
            request,
            &config,
            &registry,
            &|_| None,
            &|| false,
            &mut |_| {},
        )
        .await
        .unwrap();
        assert_eq!(report.items.len(), 2);
        assert_eq!(report.items[0].output, "hi");
        assert_eq!(report.items[0].expected, "hi");
        assert_eq!(report.items[0].text.length, 2);
        assert_eq!(report.items[0].text.errors, 0);
        assert_eq!(report.items[1].expected, "");
        assert_eq!(report.items[1].text, Cer::default());
        assert!(report.items[1].checks.is_none());
        assert!(report.notes.iter().any(|n| n == "target: 英文"));
        assert!(report
            .notes
            .iter()
            .any(|n| n == "reference translations: 1 of 2 rows"));

        // Without a translation module the stage refuses.
        let request = RunRequest {
            name: "t".into(),
            stage: Stage::Translate,
            rows: vec![],
            profile: config.default_profile().clone(),
            asr: vec![],
            llm: vec![],
            base_prompt: String::new(),
            translate_prompt: String::new(),
            asr_sample: None,
            examples: None,
            notes: vec![],
            origin: Origin::Command,
        };
        let err = run(
            request,
            &config,
            &registry,
            &|_| None,
            &|| false,
            &mut |_| {},
        )
        .await
        .unwrap_err();
        assert!(err.contains("translation module"), "{err}");
    }

    #[test]
    fn history_rows_take_the_corrected_transcript_or_the_raw_one() {
        let rows = vec![
            row(1, "原文一", Some("校對一"), "最終一。", true),
            row(2, "原文二", None, "最終二。", true),
            row(3, "沒錄音", None, "x", false),
        ];
        let set = rows_from_history(&rows);
        assert_eq!(set.len(), 2);
        assert_eq!(set[0].id, "1");
        assert_eq!(set[0].spoken, "校對一");
        assert_eq!(set[0].final_text, "最終一。");
        assert_eq!(set[1].spoken, "原文二");
        assert!(set[0].tags.contains(&"short".to_string()));
    }

    #[test]
    fn a_failed_row_is_left_out() {
        let mut r = row(1, "", None, "", true);
        r.error = Some("no speech".into());
        assert!(rows_from_history(&[r]).is_empty());
    }

    fn report(name: &str, created: &str, stage: Stage, provider: &str) -> Report {
        Report {
            name: name.into(),
            created: created.into(),
            notes: vec![],
            items: vec![
                ItemResult::scored("1", stage, provider, &[], "你好。", Ok("你好。".into()), 10),
                ItemResult::scored("2", stage, provider, &[], "再見。", Err("boom".into()), 0),
            ],
            origin: Origin::App,
            stopped: false,
        }
    }

    #[test]
    fn reports_are_listed_newest_first_with_their_scores() {
        let dir = tempfile::tempdir().unwrap();
        let a = report("older", "100", Stage::Llm, "g");
        let b = report("newer", "200", Stage::Asr, "b");
        std::fs::write(
            dir.path().join("a.json"),
            serde_json::to_string(&a).unwrap(),
        )
        .unwrap();
        std::fs::write(
            dir.path().join("b.json"),
            serde_json::to_string(&b).unwrap(),
        )
        .unwrap();
        std::fs::write(dir.path().join("junk.json"), "{}").unwrap();
        std::fs::write(dir.path().join("note.txt"), "x").unwrap();
        let heads = list_reports(dir.path());
        assert_eq!(heads.len(), 2);
        assert_eq!(heads[0].file, "b.json");
        assert_eq!(heads[0].stage, Stage::Asr);
        assert_eq!(heads[0].items, 2);
        assert_eq!(heads[0].failed, 1);
        assert_eq!(heads[0].providers, vec!["b".to_string()]);
        assert_eq!(heads[0].text.errors, 2);
        assert_eq!(heads[0].origin, Origin::App);
        assert_eq!(heads[1].file, "a.json");
    }

    #[test]
    fn old_report_files_without_the_new_fields_still_load() {
        let text = r#"{"name":"x","created":"1","notes":[],"items":[]}"#;
        let r: Report = serde_json::from_str(text).unwrap();
        assert_eq!(r.origin, Origin::Command);
        assert!(!r.stopped);
    }

    #[test]
    fn report_file_names_avoid_taken_names_and_separators() {
        let dir = tempfile::tempdir().unwrap();
        let p = report_file_name(dir.path(), "基準 a/b");
        assert_eq!(p.file_name().unwrap().to_string_lossy(), "基準 a_b.json");
        std::fs::write(&p, "x").unwrap();
        let q = report_file_name(dir.path(), "基準 a/b");
        assert_eq!(q.file_name().unwrap().to_string_lossy(), "基準 a_b-2.json");
        assert_eq!(
            report_file_name(dir.path(), "  ")
                .file_name()
                .unwrap()
                .to_string_lossy(),
            "run.json"
        );
    }

    #[tokio::test]
    async fn the_llm_stage_runs_every_row_through_each_engine_and_stops_on_request() {
        use crate::providers::registry::tests::{embedded_cfg, FakeFactory};
        use crate::secrets::MemorySecretStore;
        let mut config = Config::http_test_config();
        let mut emb = embedded_cfg();
        emb.capabilities.chat = true;
        emb.capabilities.transcribe = true;
        config.providers.push(emb.clone());
        let registry = ProviderRegistry::with_factories(
            reqwest::Client::new(),
            Arc::new(MemorySecretStore::default()),
            vec![Arc::new(FakeFactory)],
        );
        let rows = vec![
            SeedRow {
                id: "1".into(),
                audio: PathBuf::new(),
                spoken: "hi".into(),
                final_text: "hi".into(),
                tags: vec![],
                translated: None,
            },
            SeedRow {
                id: "2".into(),
                audio: PathBuf::new(),
                spoken: "x".into(),
                final_text: "hi".into(),
                tags: vec!["long".into()],
                translated: None,
            },
        ];
        let request = RunRequest {
            name: "t".into(),
            stage: Stage::Llm,
            rows: rows.clone(),
            profile: config.default_profile().clone(),
            asr: vec![],
            llm: vec![emb.id.clone()],
            base_prompt: String::new(),
            translate_prompt: String::new(),
            asr_sample: None,
            examples: None,
            notes: vec!["made in a test".into()],
            origin: Origin::App,
        };
        let mut seen = Vec::new();
        let report = run(
            request,
            &config,
            &registry,
            &|_| None,
            &|| false,
            &mut |p| seen.push((p.done, p.total, p.item.id.clone())),
        )
        .await
        .unwrap();
        assert_eq!(seen, vec![(1, 2, "1".to_string()), (2, 2, "2".to_string())]);
        assert_eq!(report.items.len(), 2);
        assert_eq!(report.items[0].provider, "emb");
        assert!(!report.stopped);
        assert_eq!(report.origin, Origin::App);
        assert_eq!(report.notes[0], "made in a test");
        assert!(report.notes.iter().any(|n| n == "llm providers: emb"));

        // Cancelled after the first row: the report keeps that row and says so.
        let request = RunRequest {
            name: "t".into(),
            stage: Stage::Llm,
            rows,
            profile: config.default_profile().clone(),
            asr: vec![],
            llm: vec![emb.id.clone()],
            base_prompt: String::new(),
            translate_prompt: String::new(),
            asr_sample: None,
            examples: None,
            notes: vec![],
            origin: Origin::Command,
        };
        let count = std::sync::atomic::AtomicUsize::new(0);
        let report = run(
            request,
            &config,
            &registry,
            &|_| None,
            &|| count.load(std::sync::atomic::Ordering::SeqCst) >= 1,
            &mut |_| {
                count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            },
        )
        .await
        .unwrap();
        assert_eq!(report.items.len(), 1);
        assert!(report.stopped);
        assert!(report
            .notes
            .iter()
            .any(|n| n == "stopped after 1 of 2 rows"));
    }
}
