//! Quality measurement from the command line.
//!
//! `export` proposes rows of an installation's history as a seed set,
//! `run` scores the seed set through providers or a whole profile,
//! `summary` prints a saved run, `compare` sets two runs side by side.
//! The installation's data is only read; results go where `--out` says.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use arcvox_core::config::base_prompt::DEFAULT_BASE_PROMPT;
use arcvox_core::config::ProviderConfig;
use arcvox_core::config::{Config, ConfigStore, Profile};
use arcvox_core::device::Device;
use arcvox_core::dictionary::asr_hint;
use arcvox_core::eval::{
    categories, compare, diff, guess_tags, parse_seed_tsv, pick_examples, pick_slice, pick_spread,
    seed_tsv, summarize, Cer, Change, DiffOp, ExamplePick, ItemResult, Marks, Report, SeedRow,
    Stage, Summary, Tally, Totals,
};
use arcvox_core::history::{DeviceRef, HistoryStore};
use arcvox_core::learn::{self, Pair, Source};
use arcvox_core::models::{Manifest, ModelRole, ModelStore};
use arcvox_core::pipeline::Pipeline;
use arcvox_core::providers::registry::ProviderRegistry;
use arcvox_core::providers::AsrHint;
use arcvox_core::steps::asr::{asr_prompt, ASR_HINT_BUDGET_BYTES};
use arcvox_core::steps::PipelineContext;
use arcvox_core::types::HistoryId;
use arcvox_core::types::{Context, ProviderId};
use arcvox_core::{DataDir, MemorySecretStore};
use arcvox_inference::factory::EmbeddedFactory;

const BUNDLED_MANIFEST: &str =
    include_str!("../../../../apps/desktop/src-tauri/resources/models-manifest.json");

const USAGE: &str = "\
arcvox-eval <command> [options]

  export   --data <name|dir> --out <set.tsv> [--limit 50] [--ids 1,2,3]
           Proposes history rows with a recording as a seed set: the raw
           transcript as `spoken`, the row's final text as `final`, tags
           guessed (short, long, mixed, correction). Edit both texts.
  run      --data <name|dir> --set <set.tsv> --out <run.json> --stage asr|llm|full
           [--asr id,id] [--llm id,id] [--profile id] [--base-prompt file]
           [--style file] [--asr-sample file] [--name text]
           [--examples pool.tsv] [--examples-k 3] [--examples-pick similar|random]
           asr: every recording through each ASR provider, scored against `spoken`;
                --asr also takes the id of an installed catalogue model that has
                no engine yet (an engine is made for the run only).
           llm: `spoken` through the profile's steps after ASR with each LLM
                provider, scored against `final`.
           full: every recording through the whole profile, scored against `final`.
  fetch    --data <name|dir> --model <catalogue id>
           Downloads a catalogue model into the installation's models folder,
           the one place the app reads them from (the only thing written).
  slice    --set <set.tsv> --out <slice.tsv> [--limit 120]
           A fixed subset for quick runs: every category covered in turn
           (question, list, digits, english, correction, plain, short, long),
           the same rows every time for the same set.
  summary  <run.json> [--worst 10]
           Scores overall and per category; punctuation and spacing by kind
           as written-too-many / left-out counts.
  compare  <before.json> <after.json> [--limit 20]
           Words and punctuation tallied apart, overall and per category;
           the changed items worst first.
  import   --data <name|dir> --set <set.tsv> [--heard-only]
           Writes the set's corrections into the history rows with the same
           ids: `spoken` as the corrected transcript and `final` as the
           corrected result (left out with --heard-only), where they differ
           from the row. A copy of the database is made beside it first.
  suggest  --data <name|dir> [--set <set.tsv>] [--days N] [--limit 50]
           [--min-heard 1] [--min-final 1] [--min 3] [--max-tokens 8]
           [--region tw|cn]
           Dictionary candidates: replacements that recur between what was
           heard and what was kept. Sources: the history's raw transcript
           against its final text (pipeline), a final text the user edited
           (final), and with --set the set's `spoken` against the history
           row's raw transcript (heard) and the set's `final` against its
           `spoken` (final). A term needs --min-heard heard rows, or
           --min-final final rows, or --min pipeline rows; --days keeps the
           history rows of the last N days. Pairs the dictionary covers are
           skipped; candidates differing only in casing or spacing come last.
           Each alias is marked by kind: latin, non-word, misheard (a
           Chinese word that sounds like the term); a changed wording is
           left out. --region picks whose readings of a character apply
           (default: the default profile's main language).

  --data names the application whose data to read (default \"Arcvox\") or a
  folder laid out like a custom data directory. Nothing in it is written.
  --base-prompt and --style try a base prompt and a profile writing style
  from files in place of the installed ones; --asr-sample gives the speech
  engine a sample of writing, placed before the dictionary terms.
  --examples (llm stage) shows the rewrite model, before each row, up to
  --examples-k spoken/final pairs taken from the pool: the rows whose spoken
  text shares the most character pairs with the row (similar, default) or a
  fixed arbitrary choice per row (random). A pool row with the row's own id
  is never shown, so the set under test may be its own pool.
";

#[derive(Debug, PartialEq, Eq)]
struct Args {
    command: String,
    positional: Vec<String>,
    flags: BTreeMap<String, String>,
}

impl Args {
    fn flag(&self, name: &str) -> Option<&str> {
        self.flags.get(name).map(String::as_str)
    }

    fn required(&self, name: &str) -> Result<&str, String> {
        self.flag(name)
            .ok_or_else(|| format!("--{name} is required\n\n{USAGE}"))
    }

    fn list(&self, name: &str) -> Vec<String> {
        self.flag(name)
            .map(|v| {
                v.split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default()
    }
}

/// The first word is the command; `--name value` and `--name=value` are
/// flags, a `--name` without a value is `true`; everything else is positional.
fn parse_args(argv: &[String]) -> Result<Args, String> {
    let Some(command) = argv.first() else {
        return Err(USAGE.to_string());
    };
    let mut flags = BTreeMap::new();
    let mut positional = Vec::new();
    let mut i = 1;
    while i < argv.len() {
        let a = &argv[i];
        if let Some(rest) = a.strip_prefix("--") {
            if let Some((k, v)) = rest.split_once('=') {
                flags.insert(k.to_string(), v.to_string());
            } else if i + 1 < argv.len() && !argv[i + 1].starts_with("--") {
                flags.insert(rest.to_string(), argv[i + 1].clone());
                i += 1;
            } else {
                flags.insert(rest.to_string(), "true".to_string());
            }
        } else {
            positional.push(a.clone());
        }
        i += 1;
    }
    Ok(Args {
        command: command.clone(),
        positional,
        flags,
    })
}

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let code = match parse_args(&argv) {
        Ok(args) => {
            let rt = tokio::runtime::Runtime::new().expect("runtime");
            match rt.block_on(dispatch(args)) {
                Ok(()) => 0,
                Err(e) => {
                    eprintln!("{e}");
                    1
                }
            }
        }
        Err(e) => {
            eprintln!("{e}");
            2
        }
    };
    std::process::exit(code);
}

async fn dispatch(args: Args) -> Result<(), String> {
    match args.command.as_str() {
        "export" => export(&args),
        "run" => run(&args).await,
        "fetch" => fetch(&args).await,
        "slice" => slice(&args),
        "summary" => summary(&args),
        "compare" => compare_runs(&args),
        "suggest" => suggest(&args),
        "import" => import_set(&args),
        "help" | "--help" | "-h" => {
            print!("{USAGE}");
            Ok(())
        }
        other => Err(format!("unknown command `{other}`\n\n{USAGE}")),
    }
}

fn data_dir(args: &Args) -> Result<DataDir, String> {
    let name = args.flag("data").unwrap_or("Arcvox");
    let as_path = Path::new(name);
    if as_path.is_dir() {
        return Ok(DataDir::custom(as_path));
    }
    DataDir::resolve(name).ok_or_else(|| "no home directory".to_string())
}

/// The installation's config and recordings folder, read without touching
/// anything: this command never creates, moves or rewrites a file of the
/// application's.
fn read_config(dirs: &DataDir) -> Result<(Config, PathBuf), String> {
    if !dirs.config_path().is_file() {
        return Err(format!(
            "no configuration at {}; is --data right?",
            dirs.config_path().display()
        ));
    }
    let config = ConfigStore::read_only(dirs).map_err(|e| e.to_string())?;
    let recordings = config
        .general
        .recordings_dir
        .clone()
        .unwrap_or_else(|| dirs.recordings_dir());
    Ok((config, recordings))
}

/// The base prompt as the installation holds it, the shipped text when it
/// was never written.
fn read_base_prompt(dirs: &DataDir) -> Result<String, String> {
    match std::fs::read_to_string(dirs.base_prompt_path()) {
        Ok(s) => Ok(s),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(DEFAULT_BASE_PROMPT.to_string()),
        Err(e) => Err(format!("{}: {e}", dirs.base_prompt_path().display())),
    }
}

fn export(args: &Args) -> Result<(), String> {
    let dirs = data_dir(args)?;
    let (_, recordings) = read_config(&dirs)?;
    let history = HistoryStore::open_read_only(&dirs.history_path(), &recordings)
        .map_err(|e| e.to_string())?;
    let out = PathBuf::from(args.required("out")?);
    let limit: usize = args
        .flag("limit")
        .map(|v| {
            v.parse()
                .map_err(|_| "--limit must be a number".to_string())
        })
        .transpose()?
        .unwrap_or(50);
    let wanted = args.list("ids");

    let rows = history
        .list(usize::MAX, None, true)
        .map_err(|e| e.to_string())?;
    let mut candidates: Vec<(String, Vec<String>, SeedRow)> = Vec::new();
    for u in rows.into_iter().rev() {
        if u.error.is_some() || u.raw_text.trim().is_empty() {
            continue;
        }
        let id = u.id.0.to_string();
        if !wanted.is_empty() && !wanted.contains(&id) {
            continue;
        }
        let Some(path) = history
            .recording_path(u.id, &recordings)
            .map_err(|e| e.to_string())?
        else {
            continue;
        };
        let tags = guess_tags(&u.raw_text, u.audio_ms);
        let row = SeedRow {
            id: id.clone(),
            audio: path,
            spoken: u.raw_text.clone(),
            final_text: u.final_text.clone(),
            tags: tags.clone(),
        };
        candidates.push((id, tags, row));
    }
    let picked = if wanted.is_empty() {
        let keyed: Vec<(String, Vec<String>)> = candidates
            .iter()
            .map(|(id, tags, _)| (id.clone(), tags.clone()))
            .collect();
        pick_spread(&keyed, limit)
    } else {
        wanted.clone()
    };
    let mut rows: Vec<SeedRow> = Vec::new();
    for id in &picked {
        if let Some((_, _, row)) = candidates.iter().find(|(c, _, _)| c == id) {
            rows.push(row.clone());
        } else {
            eprintln!("row {id}: not found or without a usable recording");
        }
    }
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(&out, seed_tsv(&rows)).map_err(|e| e.to_string())?;
    println!(
        "{} rows written to {} (from {} candidates)",
        rows.len(),
        out.display(),
        candidates.len()
    );
    Ok(())
}

fn load_set(args: &Args) -> Result<Vec<SeedRow>, String> {
    let path = args.required("set")?;
    let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    let rows = parse_seed_tsv(&text)?;
    if rows.is_empty() {
        return Err(format!("{path}: the seed set is empty"));
    }
    Ok(rows)
}

fn load_clip(path: &Path) -> Result<Arc<arcvox_core::types::AudioClip>, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    arcvox_core::audio::decode_wav_pcm16(&bytes)
        .map(Arc::new)
        .map_err(|e| format!("{}: {e}", path.display()))
}

fn now_iso() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{secs}")
}

async fn run(args: &Args) -> Result<(), String> {
    let dirs = data_dir(args)?;
    let (config, _) = read_config(&dirs)?;
    let rows = load_set(args)?;
    let out = PathBuf::from(args.required("out")?);
    let stage = match args.required("stage")? {
        "asr" => Stage::Asr,
        "llm" => Stage::Llm,
        "full" => Stage::Full,
        other => return Err(format!("--stage must be asr, llm or full, not `{other}`")),
    };
    let base_prompt = match args.flag("base-prompt") {
        Some(p) => std::fs::read_to_string(p).map_err(|e| format!("{p}: {e}"))?,
        None => read_base_prompt(&dirs)?,
    };
    let profile: Profile = match args.flag("profile") {
        Some(id) => config
            .profiles
            .iter()
            .find(|p| p.id.0 == id)
            .cloned()
            .ok_or_else(|| format!("no profile `{id}`"))?,
        None => config.default_profile().clone(),
    };
    let style = args
        .flag("style")
        .map(|p| std::fs::read_to_string(p).map_err(|e| format!("{p}: {e}")))
        .transpose()?;
    let profile = with_style(profile, style.as_deref());
    let asr_sample = args
        .flag("asr-sample")
        .map(|p| std::fs::read_to_string(p).map_err(|e| format!("{p}: {e}")))
        .transpose()?;
    let examples = load_examples(args)?;

    let client = reqwest::Client::new();
    let models = Arc::new(ModelStore::new(
        &dirs,
        client.clone(),
        BUNDLED_MANIFEST,
        None,
    ));
    let registry = ProviderRegistry::with_factories(
        client,
        Arc::new(MemorySecretStore::default()),
        vec![Arc::new(EmbeddedFactory::new(models, true))],
    );

    let mut notes = vec![
        format!("data: {}", dirs.data_dir.display()),
        format!("profile: {}", profile.id.0),
        format!(
            "base prompt: {}",
            args.flag("base-prompt").unwrap_or("the installation's")
        ),
        format!(
            "writing style: {}",
            args.flag("style").unwrap_or("the profile's")
        ),
        format!(
            "speech sample: {}",
            args.flag("asr-sample").unwrap_or("none")
        ),
        format!(
            "examples: {}",
            examples
                .as_ref()
                .map(|e| e.describe(args.flag("examples").unwrap_or("")))
                .unwrap_or_else(|| "none".to_string())
        ),
        format!("set: {} ({} rows)", args.required("set")?, rows.len()),
    ];
    let mut items: Vec<ItemResult> = Vec::new();
    let started = Instant::now();

    match stage {
        Stage::Asr => {
            let ids = {
                let l = args.list("asr");
                if l.is_empty() {
                    vec![config
                        .resolve_engines(&profile, |cfg| registry.ready(cfg))
                        .asr
                        .id
                        .0
                        .clone()]
                } else {
                    l
                }
            };
            notes.push(format!("asr providers: {}", ids.join(", ")));
            let hint = AsrHint {
                language: profile.language_hint.clone(),
                prompt: asr_prompt(
                    asr_sample.as_deref(),
                    asr_hint(&config.dictionary, ASR_HINT_BUDGET_BYTES),
                ),
            };
            let manifest = Manifest::parse(BUNDLED_MANIFEST).map_err(|e| e.to_string())?;
            for id in &ids {
                // A catalogue model without an engine gets one for this run only.
                let made = config.provider(&ProviderId(id.clone())).is_none().then(|| {
                    manifest
                        .get(id)
                        .filter(|e| e.role == ModelRole::Asr)
                        .map(|e| {
                            let mut made = ProviderConfig::embedded(id, &e.label, e.runtime, id);
                            made.capabilities = e.capabilities();
                            made
                        })
                });
                let cfg = match made {
                    Some(Some(ref cfg)) => cfg,
                    Some(None) => return Err(format!("no provider or catalogue model `{id}`")),
                    None => config.provider(&ProviderId(id.clone())).unwrap(),
                };
                let provider = registry
                    .asr(cfg, &config.general)
                    .map_err(|e| format!("{id}: {}", e.message))?;
                for (n, row) in rows.iter().enumerate() {
                    progress(n, rows.len(), id, &row.id);
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
                    items.push(ItemResult::scored(
                        &row.id,
                        stage,
                        id,
                        &row.tags,
                        &row.spoken,
                        result.0,
                        result.1,
                    ));
                }
            }
        }
        Stage::Llm => {
            let ids = {
                let l = args.list("llm");
                if l.is_empty() {
                    vec![config
                        .resolve_engines(&profile, |cfg| registry.ready(cfg))
                        .llm
                        .map(|c| c.id.0)
                        .ok_or("the profile has no rewrite step; pass --llm")?]
                } else {
                    l
                }
            };
            notes.push(format!("llm providers: {}", ids.join(", ")));
            for id in &ids {
                let mut prof = profile.clone();
                prof.llm_provider = Some(ProviderId(id.clone()));
                let (pipeline, ctx) = build(
                    &config,
                    prof,
                    &registry,
                    &base_prompt,
                    asr_sample.as_deref(),
                )?;
                for (n, row) in rows.iter().enumerate() {
                    progress(n, rows.len(), id, &row.id);
                    let ctx = match &examples {
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
                    items.push(ItemResult::scored(
                        &row.id,
                        stage,
                        id,
                        &row.tags,
                        &row.final_text,
                        r,
                        ms,
                    ));
                }
            }
        }
        Stage::Full => {
            let mut prof = profile.clone();
            let asr = args.list("asr");
            let llm = args.list("llm");
            if asr.len() > 1 || llm.len() > 1 {
                return Err("--stage full takes at most one --asr and one --llm".into());
            }
            if let Some(a) = asr.first() {
                prof.asr_provider = Some(ProviderId(a.clone()));
            }
            if let Some(l) = llm.first() {
                prof.llm_provider = Some(ProviderId(l.clone()));
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
            let (pipeline, ctx) = build(
                &config,
                prof,
                &registry,
                &base_prompt,
                asr_sample.as_deref(),
            )?;
            for (n, row) in rows.iter().enumerate() {
                progress(n, rows.len(), &label, &row.id);
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
                items.push(ItemResult::scored(
                    &row.id,
                    stage,
                    &label,
                    &row.tags,
                    &row.final_text,
                    result.0,
                    result.1,
                ));
            }
        }
    }
    eprintln!();
    notes.push(format!("wall time: {} s", started.elapsed().as_secs()));

    let report = Report {
        name: args.flag("name").map(str::to_string).unwrap_or_else(|| {
            out.file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default()
        }),
        created: now_iso(),
        notes,
        items,
    };
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(
        &out,
        serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?,
    )
    .map_err(|e| format!("{}: {e}", out.display()))?;
    print_summary(&report);
    println!("\nsaved to {}", out.display());
    Ok(())
}

/// The pool the rewrite model's examples are picked from, per row.
struct Examples {
    pool: Vec<SeedRow>,
    k: usize,
    pick: ExamplePick,
}

impl Examples {
    fn describe(&self, path: &str) -> String {
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

fn load_examples(args: &Args) -> Result<Option<Examples>, String> {
    let Some(path) = args.flag("examples") else {
        if args.flag("examples-k").is_some() || args.flag("examples-pick").is_some() {
            return Err("--examples-k and --examples-pick need --examples".into());
        }
        return Ok(None);
    };
    let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    let pool = parse_seed_tsv(&text)?;
    if pool.is_empty() {
        return Err(format!("{path}: the example pool is empty"));
    }
    let k = match args.flag("examples-k") {
        Some(v) => v
            .parse::<usize>()
            .ok()
            .filter(|k| *k > 0)
            .ok_or_else(|| format!("--examples-k must be a number above 0, not `{v}`"))?,
        None => 3,
    };
    let pick = match args.flag("examples-pick") {
        Some(v) => ExamplePick::parse(v)
            .ok_or_else(|| format!("--examples-pick must be similar or random, not `{v}`"))?,
        None => ExamplePick::Similar,
    };
    Ok(Some(Examples { pool, k, pick }))
}

/// The profile with its writing style replaced by `style`, when given.
fn with_style(mut profile: Profile, style: Option<&str>) -> Profile {
    if let Some(text) = style {
        profile.prompt_addition = text.trim().to_string();
    }
    profile
}

fn build(
    config: &arcvox_core::config::Config,
    profile: Profile,
    registry: &ProviderRegistry,
    base_prompt: &str,
    asr_sample: Option<&str>,
) -> Result<(Pipeline, PipelineContext), String> {
    let engines = config.resolve_engines(&profile, |cfg| registry.ready(cfg));
    let pipeline = Pipeline::build(config, &profile, &engines, registry).map_err(|e| e.message)?;
    let ctx = PipelineContext {
        profile: Arc::new(profile),
        context: Context::default(),
        dictionary: Arc::new(config.dictionary.clone()),
        base_prompt: base_prompt.to_string(),
        general: config.general.clone(),
        recognised: Default::default(),
        asr_sample: asr_sample.map(str::to_string),
    };
    Ok((pipeline, ctx))
}

/// An event sender nobody listens to: the pipeline's progress events are
/// not needed here, and a sender with a live but unread receiver would block
/// once the channel is full.
fn closed_events() -> tokio::sync::mpsc::Sender<arcvox_core::Event> {
    let (tx, rx) = tokio::sync::mpsc::channel(1);
    drop(rx);
    tx
}

fn progress(n: usize, total: usize, provider: &str, id: &str) {
    eprint!("\r{provider}: {}/{total} (row {id})      ", n + 1);
}

fn load_report(path: &str) -> Result<Report, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    serde_json::from_str(&text).map_err(|e| format!("{path}: {e}"))
}

fn pct(c: Cer) -> String {
    format!("{:5.1}%", c.rate() * 100.0)
}

fn print_summary(report: &Report) {
    println!("{}  ({} items)", report.name, report.items.len());
    for n in &report.notes {
        println!("  {n}");
    }
    println!();
    println!(
        "{:<6} {:<28} {:>5} {:>6} {:>8} {:>8} {:>9}",
        "stage", "provider", "items", "failed", "words", "punct", "avg ms"
    );
    for s in summarize(report) {
        print_summary_row(&s);
    }
}

fn print_summary_row(s: &Summary) {
    let avg = if s.items > 0 {
        s.ms_total / s.items as u64
    } else {
        0
    };
    println!(
        "{:<6} {:<28} {:>5} {:>6} {:>8} {:>8} {:>9}",
        stage_name(s.stage),
        s.provider,
        s.items,
        s.failed,
        pct(s.total.text),
        pct(s.total.punctuation),
        avg
    );
    println!("  marks (extra/missing): {}", marks_line(&s.total.marks));
    println!(
        "  {:<12} {:>5} {:>8} {:>8}  {}",
        "category",
        "items",
        "words",
        "punct",
        Marks::KINDS
            .iter()
            .map(|k| format!("{k:>9}"))
            .collect::<String>()
    );
    for (cat, t) in &s.by_category {
        println!("  {}", category_line(cat, t));
    }
}

/// `stop 460/0  question 3/135 ...`: written too many / left out per kind.
fn marks_line(m: &Marks) -> String {
    Marks::KINDS
        .iter()
        .zip(m.each())
        .map(|(k, c)| format!("{k} {}/{}", c.extra, c.missing))
        .collect::<Vec<_>>()
        .join("  ")
}

fn category_line(cat: &str, t: &Totals) -> String {
    format!(
        "{:<12} {:>5} {:>8} {:>8}  {}",
        cat,
        t.items,
        pct(t.text),
        pct(t.punctuation),
        t.marks
            .each()
            .iter()
            .map(|c| format!("{:>9}", format!("{}/{}", c.extra, c.missing)))
            .collect::<String>()
    )
}

fn tally(t: Tally) -> String {
    format!("+{} -{} ={}", t.improved, t.worse, t.same)
}

fn stage_name(s: Stage) -> &'static str {
    match s {
        Stage::Asr => "asr",
        Stage::Llm => "llm",
        Stage::Full => "full",
    }
}

/// `[-missing-]` is expected text the output lacks, `{+extra+}` is output
/// text that was not expected.
fn render_diff(expected: &str, actual: &str) -> String {
    diff(expected, actual)
        .into_iter()
        .map(|op| match op {
            DiffOp::Equal(s) => s,
            DiffOp::Delete(s) => format!("[-{s}-]"),
            DiffOp::Insert(s) => format!("{{+{s}+}}"),
        })
        .collect()
}

fn summary(args: &Args) -> Result<(), String> {
    let path = args
        .positional
        .first()
        .ok_or_else(|| format!("summary needs a run file\n\n{USAGE}"))?;
    let report = load_report(path)?;
    let worst: usize = args
        .flag("worst")
        .map(|v| {
            v.parse()
                .map_err(|_| "--worst must be a number".to_string())
        })
        .transpose()?
        .unwrap_or(10);
    print_summary(&report);
    let mut items: Vec<&ItemResult> = report.items.iter().collect();
    items.sort_by(|a, b| {
        b.text
            .rate()
            .partial_cmp(&a.text.rate())
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(b.text.errors.cmp(&a.text.errors))
    });
    println!("\nworst {} by word error rate:", worst.min(items.len()));
    for it in items.iter().take(worst) {
        println!(
            "\n[{} {} {}] words {} punct {}{}",
            it.id,
            stage_name(it.stage),
            it.provider,
            pct(it.text),
            pct(it.punctuation),
            it.error
                .as_deref()
                .map(|e| format!("  FAILED: {e}"))
                .unwrap_or_default()
        );
        println!("  {}", render_diff(&it.expected, &it.output));
    }
    Ok(())
}

fn compare_runs(args: &Args) -> Result<(), String> {
    let [a, b] = args.positional.as_slice() else {
        return Err(format!("compare needs two run files\n\n{USAGE}"));
    };
    let before = load_report(a)?;
    let after = load_report(b)?;
    let limit: usize = args
        .flag("limit")
        .map(|v| {
            v.parse()
                .map_err(|_| "--limit must be a number".to_string())
        })
        .transpose()?
        .unwrap_or(20);
    let c = compare(&before, &after);
    println!("A: {}\nB: {}\n", before.name, after.name);
    println!(
        "{:<6} {:<28} {:>8} {:>8} {:>8} {:>8}",
        "stage", "provider", "words A", "words B", "punct A", "punct B"
    );
    for x in &c.before {
        // The same pairing as the item matching: by provider, or by stage
        // alone when each report ran one provider in it.
        let same_stage: Vec<&Summary> = c.after.iter().filter(|y| y.stage == x.stage).collect();
        let y = same_stage
            .iter()
            .find(|y| y.provider == x.provider)
            .copied()
            .or_else(|| {
                let before_count = c.before.iter().filter(|b| b.stage == x.stage).count();
                (before_count == 1 && same_stage.len() == 1).then(|| same_stage[0])
            });
        let Some(y) = y else {
            continue;
        };
        println!(
            "{:<6} {:<28} {:>8} {:>8} {:>8} {:>8}",
            stage_name(x.stage),
            x.provider,
            pct(x.total.text),
            pct(y.total.text),
            pct(x.total.punctuation),
            pct(y.total.punctuation)
        );
        println!("  marks (extra/missing)");
        for ((kind, ma), mb) in Marks::KINDS
            .iter()
            .zip(x.total.marks.each())
            .zip(y.total.marks.each())
        {
            println!(
                "    {:<9} {:>9} {:>9}",
                kind,
                format!("{}/{}", ma.extra, ma.missing),
                format!("{}/{}", mb.extra, mb.missing)
            );
        }
        println!(
            "  {:<12} {:>5} {:>8} {:>8} {:>14} {:>8} {:>8} {:>14}",
            "category", "items", "words A", "words B", "w +/-/=", "punct A", "punct B", "p +/-/="
        );
        for (cat, ta) in &x.by_category {
            let tb = y
                .by_category
                .iter()
                .find(|(n, _)| n == cat)
                .map(|(_, t)| *t)
                .unwrap_or_default();
            let (tw, tp) = c
                .by_category
                .iter()
                .find(|(n, _, _)| n == cat)
                .map(|(_, w, p)| (*w, *p))
                .unwrap_or_default();
            println!(
                "  {:<12} {:>5} {:>8} {:>8} {:>14} {:>8} {:>8} {:>14}",
                cat,
                ta.items,
                pct(ta.text),
                pct(tb.text),
                tally(tw),
                pct(ta.punctuation),
                pct(tb.punctuation),
                tally(tp)
            );
        }
    }
    println!(
        "\nwords {}   punctuation {}   unmatched {}",
        tally(c.words),
        tally(c.punctuation),
        c.unmatched
    );
    if !c.changed.is_empty() {
        println!("\nchanged, worst first:");
        for ch in c.changed.iter().take(limit) {
            println!("\n{}", change_head(ch));
            println!("  A: {}", ch.before_output.replace('\n', "\\n"));
            println!("  B: {}", ch.after_output.replace('\n', "\\n"));
        }
        if c.changed.len() > limit {
            println!("  ... and {} more", c.changed.len() - limit);
        }
    }
    Ok(())
}

/// `[12 llm gemma] words +2 punct -1 (question, english)`
fn change_head(ch: &Change) -> String {
    format!(
        "[{} {} {}] words {:+} punct {:+} ({})",
        ch.id,
        stage_name(ch.stage),
        ch.provider,
        ch.words_delta(),
        ch.punctuation_delta(),
        ch.categories.join(", ")
    )
}

async fn fetch(args: &Args) -> Result<(), String> {
    let dirs = data_dir(args)?;
    let id = args.required("model")?;
    let models = ModelStore::new(&dirs, reqwest::Client::new(), BUNDLED_MANIFEST, None);
    let m = models.download(id).await.map_err(|e| e.to_string())?;
    println!("{} is at {}", m.id, m.path.display());
    Ok(())
}

fn slice(args: &Args) -> Result<(), String> {
    let rows = load_set(args)?;
    let out = PathBuf::from(args.required("out")?);
    let limit: usize = args
        .flag("limit")
        .map(|v| {
            v.parse()
                .map_err(|_| "--limit must be a number".to_string())
        })
        .transpose()?
        .unwrap_or(120);
    let picked = pick_slice(&rows, limit);
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(&out, seed_tsv(&picked)).map_err(|e| e.to_string())?;
    println!(
        "{} of {} rows written to {}",
        picked.len(),
        rows.len(),
        out.display()
    );
    let mut counts: Vec<(String, usize, usize)> = Vec::new();
    for (set, idx) in [(&rows, 1usize), (&picked, 2usize)] {
        for r in set {
            for cat in categories(&r.final_text, &r.tags) {
                let e = match counts.iter_mut().find(|(n, _, _)| *n == cat) {
                    Some(e) => e,
                    None => {
                        counts.push((cat, 0, 0));
                        counts.last_mut().unwrap()
                    }
                };
                if idx == 1 {
                    e.1 += 1;
                } else {
                    e.2 += 1;
                }
            }
        }
    }
    println!("{:<12} {:>5} {:>5}", "category", "set", "slice");
    for (cat, all, part) in counts {
        println!("{cat:<12} {all:>5} {part:>5}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(s: &str) -> Vec<String> {
        s.split_whitespace().map(str::to_string).collect()
    }

    #[test]
    fn parse_args_reads_command_flags_and_positionals() {
        let a = parse_args(&argv(
            "run x.json --data Arcvox --stage=asr --asr a,b --dry",
        ))
        .unwrap();
        assert_eq!(a.command, "run");
        assert_eq!(a.flag("data"), Some("Arcvox"));
        assert_eq!(a.flag("stage"), Some("asr"));
        assert_eq!(a.list("asr"), vec!["a", "b"]);
        assert_eq!(
            a.flag("dry"),
            Some("true"),
            "a flag without a value is true"
        );
        assert_eq!(a.positional, vec!["x.json"]);
        assert!(parse_args(&[]).is_err());
    }

    #[test]
    fn render_diff_marks_missing_and_extra_text() {
        assert_eq!(
            render_diff("明天十點開會", "明天十一點開會"),
            "明天十{+一+}點開會"
        );
        assert_eq!(render_diff("不對是十一點", "是十一點"), "[-不對-]是十一點");
    }
    #[test]
    fn marks_line_and_category_line_show_extra_over_missing_per_kind() {
        let mut t = Totals {
            items: 3,
            ..Totals::default()
        };
        t.marks.stop.extra = 2;
        t.marks.space.missing = 5;
        assert_eq!(
            marks_line(&t.marks),
            "stop 2/0  question 0/0  inner 0/0  quote 0/0  other 0/0  space 0/5"
        );
        let line = category_line("question", &t);
        assert!(line.starts_with("question         3"), "{line}");
        assert!(line.ends_with("      0/5"), "{line}");
    }

    #[test]
    fn change_head_names_both_deltas_and_the_categories() {
        let before = ItemResult::scored(
            "7",
            Stage::Llm,
            "gemma",
            &["short".into()],
            "好嗎？",
            Ok("好嗎。".into()),
            1,
        );
        let after = ItemResult::scored(
            "7",
            Stage::Llm,
            "gemma",
            &["short".into()],
            "好嗎？",
            Ok("好嗎？".into()),
            1,
        );
        let ch = Change {
            id: "7".into(),
            stage: Stage::Llm,
            provider: "gemma".into(),
            categories: categories("好嗎？", &["short".into()]),
            before: before.text,
            after: after.text,
            before_punctuation: before.punctuation,
            after_punctuation: after.punctuation,
            before_output: before.output,
            after_output: after.output,
        };
        assert_eq!(
            change_head(&ch),
            "[7 llm gemma] words +0 punct -1 (question, short)"
        );
    }
    #[test]
    fn a_style_file_replaces_the_profile_writing_style_only() {
        let mut p = arcvox_core::config::Config::factory_profile();
        p.prompt_addition = "installed".into();
        let kept = with_style(p.clone(), None);
        assert_eq!(kept.prompt_addition, "installed");
        let swapped = with_style(p.clone(), Some("聊天的寫法。\n"));
        assert_eq!(swapped.prompt_addition, "聊天的寫法。");
        assert_eq!(swapped.id, p.id);
        assert_eq!(swapped.steps, p.steps);
    }
}

fn suggest(args: &Args) -> Result<(), String> {
    let dirs = data_dir(args)?;
    let (config, recordings) = read_config(&dirs)?;
    let history = HistoryStore::open_read_only(&dirs.history_path(), &recordings)
        .map_err(|e| e.to_string())?;
    let number = |name: &str, default: usize| -> Result<usize, String> {
        args.flag(name)
            .map(|v| v.parse().map_err(|_| format!("--{name} must be a number")))
            .transpose()
            .map(|v| v.unwrap_or(default))
    };
    let limit = number("limit", 50)?;
    let since = match number("days", 0)? {
        0 => 0,
        days => {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            now - days as i64 * 86_400
        }
    };
    let region = match args.flag("region") {
        Some("cn") => learn::zh::Region::Mainland,
        Some("tw") => learn::zh::Region::Taiwan,
        Some(other) => return Err(format!("--region must be tw or cn, not `{other}`")),
        None => learn::zh::Region::of_language(
            config
                .profiles
                .iter()
                .find(|p| p.is_default)
                .and_then(|p| p.language_hint.as_deref()),
        ),
    };
    let opts = learn::Options {
        min_heard: number("min-heard", 1)? as u32,
        min_final: number("min-final", 1)? as u32,
        min_pipeline: number("min", 3)? as u32,
        max_tokens: number("max-tokens", 8)?,
        region,
        known: config
            .dictionary
            .iter()
            .map(|e| (e.term.clone(), e.aliases.clone()))
            .collect(),
        ..learn::Options::default()
    };

    let rows = history
        .list(usize::MAX, None, false)
        .map_err(|e| e.to_string())?;
    let rows: Vec<_> = rows
        .into_iter()
        .filter(|u| u.error.is_none() && !u.raw_text.trim().is_empty() && u.ts >= since)
        .collect();
    let mut pairs: Vec<Pair> = learn::pairs_of(&rows);
    let edited = pairs.iter().filter(|p| p.source == Source::Final).count();
    let heard = pairs.iter().filter(|p| p.source == Source::Heard).count();
    let mut raw_by_id: BTreeMap<String, String> = BTreeMap::new();
    for u in &rows {
        let id = u.id.0.to_string();
        raw_by_id.insert(id.clone(), u.raw_text.clone());
        pairs.push(Pair {
            id,
            from: u.raw_text.clone(),
            to: u.final_text.clone(),
            source: Source::Pipeline,
        });
    }
    let mut set_rows = 0usize;
    if let Some(path) = args.flag("set") {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
        for row in parse_seed_tsv(&text)? {
            set_rows += 1;
            if let Some(raw) = raw_by_id.get(&row.id) {
                pairs.push(Pair {
                    id: format!("set:{}", row.id),
                    from: raw.clone(),
                    to: row.spoken.clone(),
                    source: Source::Heard,
                });
            }
            pairs.push(Pair {
                id: format!("set:{}", row.id),
                from: row.spoken,
                to: row.final_text,
                source: Source::Final,
            });
        }
    }

    let found = learn::suggest(&pairs, &opts);
    let forms = found.iter().filter(|s| s.form_only).count();
    println!(
        "{} history rows ({} with a corrected transcript, {} with a corrected result), {} set rows; {} candidates ({} casing or spacing only), {} shown; a term needs {} heard, {} final or {} pipeline rows",
        raw_by_id.len(),
        heard,
        edited,
        set_rows,
        found.len(),
        forms,
        found.len().min(limit),
        opts.min_heard,
        opts.min_final,
        opts.min_pipeline
    );
    for (n, s) in found.iter().take(limit).enumerate() {
        println!();
        println!(
            "{:>3}. {}{}{}   heard {} | final {} | pipeline {}",
            n + 1,
            s.term,
            if s.known_term {
                " (in the dictionary)"
            } else {
                ""
            },
            if s.form_only {
                " (casing or spacing)"
            } else {
                ""
            },
            s.counts.heard,
            s.counts.finals,
            s.counts.pipeline
        );
        let aliases: Vec<String> = s
            .aliases
            .iter()
            .map(|a| {
                let mut parts = Vec::new();
                if a.counts.heard > 0 {
                    parts.push(format!("heard {}", a.counts.heard));
                }
                if a.counts.finals > 0 {
                    parts.push(format!("final {}", a.counts.finals));
                }
                if a.counts.pipeline > 0 {
                    parts.push(format!("pipeline {}", a.counts.pipeline));
                }
                let kind = match a.kind {
                    learn::AliasKind::Latin => "latin",
                    learn::AliasKind::NonWord => "non-word",
                    learn::AliasKind::Misheard => "misheard",
                    learn::AliasKind::Rephrase => "rephrase",
                };
                format!("{} [{kind}] ({})", a.text, parts.join(", "))
            })
            .collect();
        println!("     <- {}", aliases.join("; "));
        for e in &s.excerpts {
            println!("        {e}");
        }
    }
    Ok(())
}

fn import_set(args: &Args) -> Result<(), String> {
    let dirs = data_dir(args)?;
    let (_, recordings) = read_config(&dirs)?;
    let path = args.required("set")?;
    let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    let set = parse_seed_tsv(&text)?;
    let heard_only = args.flag("heard-only").is_some();

    let dir = dirs
        .device_path()
        .parent()
        .ok_or("the data directory has no parent")?
        .to_path_buf();
    let device = Device::load_or_create(&dir, None, "Device").map_err(|e| e.to_string())?;
    let platform = std::env::consts::OS;
    let device_ref = DeviceRef {
        id: &device.id,
        name: &device.name,
        platform,
    };
    let history = HistoryStore::open(&dirs.history_path(), &recordings, &device_ref)
        .map_err(|e| e.to_string())?;

    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let copy = dirs
        .history_path()
        .with_file_name(format!("history-before-import-{stamp}.sqlite"));
    history.copy_to(&copy).map_err(|e| e.to_string())?;
    println!("copy of the database: {}", copy.display());

    let (mut heard, mut finals, mut missing, mut same) = (0usize, 0usize, 0usize, 0usize);
    for row in &set {
        let Ok(id) = row.id.parse::<i64>() else {
            missing += 1;
            continue;
        };
        let Some(u) = history.get(HistoryId(id)).map_err(|e| e.to_string())? else {
            missing += 1;
            continue;
        };
        let mut touched = false;
        if row.spoken != u.raw_text && u.heard_text.as_deref() != Some(row.spoken.as_str()) {
            history
                .set_heard(u.id, Some(&row.spoken))
                .map_err(|e| e.to_string())?;
            heard += 1;
            touched = true;
        }
        if !heard_only && row.final_text != u.final_text {
            history
                .set_final(u.id, &row.final_text)
                .map_err(|e| e.to_string())?;
            finals += 1;
            touched = true;
        }
        if !touched {
            same += 1;
        }
    }
    println!(
        "{} set rows: {} corrected transcripts written, {} corrected results written, {} rows already so, {} rows not in the history",
        set.len(),
        heard,
        finals,
        same,
        missing
    );
    Ok(())
}
