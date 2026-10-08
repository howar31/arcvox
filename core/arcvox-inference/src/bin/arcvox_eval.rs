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

use arcvox_core::config::{ConfigStore, Profile};
use arcvox_core::dictionary::asr_hint;
use arcvox_core::eval::{
    compare, diff, guess_tags, parse_seed_tsv, pick_spread, seed_tsv, summarize, Cer, DiffOp,
    ItemResult, Report, SeedRow, Stage, Summary,
};
use arcvox_core::history::HistoryStore;
use arcvox_core::models::ModelStore;
use arcvox_core::pipeline::Pipeline;
use arcvox_core::providers::registry::ProviderRegistry;
use arcvox_core::providers::AsrHint;
use arcvox_core::steps::asr::ASR_HINT_BUDGET_BYTES;
use arcvox_core::steps::PipelineContext;
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
           [--asr id,id] [--llm id,id] [--profile id] [--base-prompt file] [--name text]
           asr: every recording through each ASR provider, scored against `spoken`.
           llm: `spoken` through the profile's steps after ASR with each LLM
                provider, scored against `final`.
           full: every recording through the whole profile, scored against `final`.
  summary  <run.json> [--worst 10]
  compare  <before.json> <after.json> [--limit 20]

  --data names the application whose data to read (default \"Arcvox\") or a
  folder laid out like a custom data directory. Nothing in it is written.
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
        "summary" => summary(&args),
        "compare" => compare_runs(&args),
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

fn read_config(dirs: &DataDir) -> Result<(ConfigStore, PathBuf), String> {
    if !dirs.config_path().is_file() {
        return Err(format!(
            "no configuration at {}; is --data right?",
            dirs.config_path().display()
        ));
    }
    let store = ConfigStore::open(dirs).map_err(|e| e.to_string())?;
    let recordings = store
        .current()
        .general
        .recordings_dir
        .clone()
        .unwrap_or_else(|| dirs.recordings_dir());
    Ok((store, recordings))
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
    let (store, _) = read_config(&dirs)?;
    let config = store.current();
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
        None => store.base_prompt().map_err(|e| e.to_string())?,
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
        format!("set: {} ({} rows)", args.required("set")?, rows.len()),
    ];
    let mut items: Vec<ItemResult> = Vec::new();
    let started = Instant::now();

    match stage {
        Stage::Asr => {
            let ids = {
                let l = args.list("asr");
                if l.is_empty() {
                    vec![profile.asr_provider.0.clone()]
                } else {
                    l
                }
            };
            notes.push(format!("asr providers: {}", ids.join(", ")));
            let hint = AsrHint {
                language: profile.language_hint.clone(),
                prompt: asr_hint(&config.dictionary, ASR_HINT_BUDGET_BYTES),
            };
            for id in &ids {
                let cfg = config
                    .provider(&ProviderId(id.clone()))
                    .ok_or_else(|| format!("no provider `{id}`"))?;
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
                    vec![profile
                        .llm_provider
                        .as_ref()
                        .map(|p| p.0.clone())
                        .ok_or("the profile has no LLM provider; pass --llm")?]
                } else {
                    l
                }
            };
            notes.push(format!("llm providers: {}", ids.join(", ")));
            for id in &ids {
                let mut prof = profile.clone();
                prof.llm_provider = Some(ProviderId(id.clone()));
                let (pipeline, ctx) = build(&config, prof, &registry, &base_prompt)?;
                for (n, row) in rows.iter().enumerate() {
                    progress(n, rows.len(), id, &row.id);
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
                prof.asr_provider = ProviderId(a.clone());
            }
            if let Some(l) = llm.first() {
                prof.llm_provider = Some(ProviderId(l.clone()));
            }
            let label = format!(
                "{}+{}",
                prof.asr_provider.0,
                prof.llm_provider
                    .as_ref()
                    .map(|p| p.0.as_str())
                    .unwrap_or("none")
            );
            notes.push(format!("pipeline: {label}"));
            let (pipeline, ctx) = build(&config, prof, &registry, &base_prompt)?;
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

fn build(
    config: &arcvox_core::config::Config,
    profile: Profile,
    registry: &ProviderRegistry,
    base_prompt: &str,
) -> Result<(Pipeline, PipelineContext), String> {
    let pipeline = Pipeline::build(config, &profile, registry).map_err(|e| e.message)?;
    let ctx = PipelineContext {
        profile: Arc::new(profile),
        context: Context::default(),
        dictionary: Arc::new(config.dictionary.clone()),
        base_prompt: base_prompt.to_string(),
        general: config.general.clone(),
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
        pct(s.text),
        pct(s.punctuation),
        avg
    );
    for (tag, c) in &s.by_tag {
        println!(
            "       {:<28} {:>5} {:>6} {:>8}",
            format!("  {tag}"),
            "",
            "",
            pct(*c)
        );
    }
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
    println!(
        "{:<6} {:<28} {:>8} {:>8} {:>8} {:>8}",
        "stage", "provider", "words A", "words B", "punct A", "punct B"
    );
    let sa = summarize(&before);
    let sb = summarize(&after);
    for x in &sa {
        if let Some(y) = sb
            .iter()
            .find(|y| y.stage == x.stage && y.provider == x.provider)
        {
            println!(
                "{:<6} {:<28} {:>8} {:>8} {:>8} {:>8}",
                stage_name(x.stage),
                x.provider,
                pct(x.text),
                pct(y.text),
                pct(x.punctuation),
                pct(y.punctuation)
            );
        }
    }
    let c = compare(&before, &after);
    println!(
        "\nimproved {}, worse {}, same {}, unmatched {}",
        c.improved.len(),
        c.worse.len(),
        c.same,
        c.unmatched
    );
    for (title, list) in [("worse", &c.worse), ("improved", &c.improved)] {
        if list.is_empty() {
            continue;
        }
        println!("\n{title}:");
        for ch in list.iter().take(limit) {
            println!(
                "\n[{} {} {}] {} -> {} errors",
                ch.id,
                stage_name(ch.stage),
                ch.provider,
                ch.before.errors,
                ch.after.errors
            );
            println!("  A: {}", ch.before_output.replace('\n', "\\n"));
            println!("  B: {}", ch.after_output.replace('\n', "\\n"));
        }
        if list.len() > limit {
            println!("  ... and {} more", list.len() - limit);
        }
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
}
