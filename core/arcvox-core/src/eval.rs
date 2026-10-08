//! Quality measurement: a seed set of recordings with the text that was
//! spoken and the text that should come out, scored by character error rate
//! against what a provider or a profile produces. Two scores per item: the
//! words (whitespace and punctuation ignored, ASCII case folded) and the
//! punctuation on its own, so a wrong comma does not hide a wrong word.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Errors over length; `rate()` is the character error rate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Cer {
    pub errors: usize,
    pub length: usize,
}

impl Cer {
    pub fn rate(&self) -> f64 {
        if self.length == 0 {
            if self.errors == 0 {
                0.0
            } else {
                1.0
            }
        } else {
            self.errors as f64 / self.length as f64
        }
    }
}

const CJK_PUNCTUATION: &str = "，。、？！：；「」『』（）《》〈〉【】…—～·‧”“’‘";

fn is_punctuation(c: char) -> bool {
    c.is_ascii_punctuation() || CJK_PUNCTUATION.contains(c)
}

/// The characters that count as words: no whitespace, no punctuation,
/// ASCII letters lower-cased.
pub fn normalize_text(s: &str) -> Vec<char> {
    s.chars()
        .filter(|c| !c.is_whitespace() && !is_punctuation(*c))
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// The punctuation marks of `s`, in order.
pub fn punctuation_of(s: &str) -> Vec<char> {
    s.chars().filter(|c| is_punctuation(*c)).collect()
}

/// Levenshtein distance over characters.
pub fn edit_distance(a: &[char], b: &[char]) -> usize {
    edit_matrix(a, b)[a.len()][b.len()]
}

fn edit_matrix(a: &[char], b: &[char]) -> Vec<Vec<usize>> {
    let mut d = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for (i, row) in d.iter_mut().enumerate() {
        row[0] = i;
    }
    for (j, cell) in d[0].iter_mut().enumerate() {
        *cell = j;
    }
    for i in 1..=a.len() {
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            d[i][j] = (d[i - 1][j] + 1)
                .min(d[i][j - 1] + 1)
                .min(d[i - 1][j - 1] + cost);
        }
    }
    d
}

fn cer_of(expected: &[char], actual: &[char]) -> Cer {
    Cer {
        errors: edit_distance(expected, actual),
        length: expected.len(),
    }
}

/// Word score: errors in the normalized text over the normalized expected length.
pub fn text_cer(expected: &str, actual: &str) -> Cer {
    cer_of(&normalize_text(expected), &normalize_text(actual))
}

/// Punctuation score: errors in the punctuation sequence over the expected count.
pub fn punctuation_cer(expected: &str, actual: &str) -> Cer {
    cer_of(&punctuation_of(expected), &punctuation_of(actual))
}

/// One run of a character-level diff. `Delete` is expected text that is
/// missing from the output, `Insert` is output text that was not expected.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", content = "text", rename_all = "snake_case")]
pub enum DiffOp {
    Equal(String),
    Delete(String),
    Insert(String),
}

/// Character-level diff of the raw texts, adjacent runs merged.
pub fn diff(expected: &str, actual: &str) -> Vec<DiffOp> {
    let a: Vec<char> = expected.chars().collect();
    let b: Vec<char> = actual.chars().collect();
    let d = edit_matrix(&a, &b);
    // Walk back from the corner, preferring matches, then deletions, then
    // insertions; the steps come out reversed.
    let (mut i, mut j) = (a.len(), b.len());
    let mut steps: Vec<(u8, char)> = Vec::new();
    while i > 0 || j > 0 {
        if i > 0 && j > 0 && a[i - 1] == b[j - 1] && d[i][j] == d[i - 1][j - 1] {
            steps.push((0, a[i - 1]));
            i -= 1;
            j -= 1;
        } else if i > 0 && d[i][j] == d[i - 1][j] + 1 {
            steps.push((1, a[i - 1]));
            i -= 1;
        } else if j > 0 && d[i][j] == d[i][j - 1] + 1 {
            steps.push((2, b[j - 1]));
            j -= 1;
        } else {
            // A substitution: the expected character is missing and the
            // output character is extra.
            steps.push((2, b[j - 1]));
            steps.push((1, a[i - 1]));
            i -= 1;
            j -= 1;
        }
    }
    steps.reverse();
    let mut ops: Vec<DiffOp> = Vec::new();
    for (kind, c) in steps {
        let same_kind = matches!(
            (ops.last(), kind),
            (Some(DiffOp::Equal(_)), 0)
                | (Some(DiffOp::Delete(_)), 1)
                | (Some(DiffOp::Insert(_)), 2)
        );
        if same_kind {
            match ops.last_mut() {
                Some(DiffOp::Equal(s)) | Some(DiffOp::Delete(s)) | Some(DiffOp::Insert(s)) => {
                    s.push(c)
                }
                None => unreachable!(),
            }
        } else {
            ops.push(match kind {
                0 => DiffOp::Equal(c.to_string()),
                1 => DiffOp::Delete(c.to_string()),
                _ => DiffOp::Insert(c.to_string()),
            });
        }
    }
    ops
}

/// Shape checks on a rewrite, independent of the expected text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Checks {
    pub ends_with_punctuation: bool,
    pub trailing_whitespace: bool,
    /// A spoken self-correction marker is still in the text.
    pub correction_marker_left: bool,
}

pub const CORRECTION_MARKERS: &[&str] = &["不對", "不是，", "我是說", "更正", "說錯了"];

pub fn check_rewrite(text: &str) -> Checks {
    let trimmed = text.trim_end();
    Checks {
        ends_with_punctuation: trimmed.chars().last().is_some_and(is_punctuation),
        trailing_whitespace: trimmed.len() != text.len(),
        correction_marker_left: CORRECTION_MARKERS.iter().any(|m| text.contains(m)),
    }
}

/// One item of the seed set: a recording, what was spoken (verbatim, with
/// the slips) and what should come out of the rewrite.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeedRow {
    pub id: String,
    pub audio: PathBuf,
    pub spoken: String,
    pub final_text: String,
    pub tags: Vec<String>,
}

pub const SEED_HEADER: &str = "id\taudio\ttags\tspoken\tfinal";

/// Parses the tab-separated seed set. Tags are comma-separated; empty lines
/// and lines starting with `#` are skipped. Newlines inside a text are
/// written as `\n`.
pub fn parse_seed_tsv(text: &str) -> Result<Vec<SeedRow>, String> {
    let mut rows = Vec::new();
    for (n, line) in text.lines().enumerate() {
        if n == 0 && line.trim_end() == SEED_HEADER {
            continue;
        }
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        let cols: Vec<&str> = line.split('\t').collect();
        if cols.len() < 5 {
            return Err(format!(
                "line {}: expected 5 tab-separated columns ({SEED_HEADER}), got {}",
                n + 1,
                cols.len()
            ));
        }
        rows.push(SeedRow {
            id: cols[0].trim().to_string(),
            audio: PathBuf::from(cols[1].trim()),
            tags: cols[2]
                .split(',')
                .map(str::trim)
                .filter(|t| !t.is_empty())
                .map(str::to_string)
                .collect(),
            spoken: unescape(cols[3]),
            final_text: unescape(cols[4]),
        });
    }
    Ok(rows)
}

pub fn seed_tsv(rows: &[SeedRow]) -> String {
    let mut out = String::from(SEED_HEADER);
    out.push('\n');
    for r in rows {
        out.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\n",
            r.id,
            r.audio.display(),
            r.tags.join(","),
            escape(&r.spoken),
            escape(&r.final_text)
        ));
    }
    out
}

fn escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('\t', "\\t")
        .replace('\n', "\\n")
}

fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('\\') => out.push('\\'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    /// Audio to text with one ASR provider, scored against `spoken`.
    Asr,
    /// `spoken` through the profile's steps after ASR with one LLM provider,
    /// scored against `final_text`.
    Llm,
    /// Audio through the whole profile, scored against `final_text`.
    Full,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ItemResult {
    pub id: String,
    pub stage: Stage,
    pub provider: String,
    pub tags: Vec<String>,
    pub expected: String,
    pub output: String,
    pub error: Option<String>,
    pub ms: u64,
    pub text: Cer,
    pub punctuation: Cer,
    pub checks: Option<Checks>,
}

impl ItemResult {
    /// Scores `output` against `expected`; a failed item counts every
    /// expected character as an error.
    pub fn scored(
        id: &str,
        stage: Stage,
        provider: &str,
        tags: &[String],
        expected: &str,
        output: Result<String, String>,
        ms: u64,
    ) -> ItemResult {
        let (text_out, error) = match output {
            Ok(t) => (t, None),
            Err(e) => (String::new(), Some(e)),
        };
        let checks = match stage {
            Stage::Asr => None,
            Stage::Llm | Stage::Full => Some(check_rewrite(&text_out)),
        };
        ItemResult {
            id: id.to_string(),
            stage,
            provider: provider.to_string(),
            tags: tags.to_vec(),
            expected: expected.to_string(),
            output: text_out.clone(),
            error,
            ms,
            text: text_cer(expected, &text_out),
            punctuation: punctuation_cer(expected, &text_out),
            checks,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Report {
    pub name: String,
    pub created: String,
    /// What the run was made with, for the record: profile, prompt source, data folder.
    pub notes: Vec<String>,
    pub items: Vec<ItemResult>,
}

/// Totals for one stage and provider, overall and per tag.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Summary {
    pub stage: Stage,
    pub provider: String,
    pub items: usize,
    pub failed: usize,
    pub text: Cer,
    pub punctuation: Cer,
    pub ms_total: u64,
    pub by_tag: Vec<(String, Cer)>,
}

pub fn summarize(report: &Report) -> Vec<Summary> {
    let mut out: Vec<Summary> = Vec::new();
    for it in &report.items {
        let s = match out
            .iter_mut()
            .find(|s| s.stage == it.stage && s.provider == it.provider)
        {
            Some(s) => s,
            None => {
                out.push(Summary {
                    stage: it.stage,
                    provider: it.provider.clone(),
                    items: 0,
                    failed: 0,
                    text: Cer::default(),
                    punctuation: Cer::default(),
                    ms_total: 0,
                    by_tag: Vec::new(),
                });
                out.last_mut().unwrap()
            }
        };
        s.items += 1;
        s.failed += usize::from(it.error.is_some());
        add(&mut s.text, it.text);
        add(&mut s.punctuation, it.punctuation);
        s.ms_total += it.ms;
        for tag in &it.tags {
            match s.by_tag.iter_mut().find(|(t, _)| t == tag) {
                Some((_, c)) => add(c, it.text),
                None => s.by_tag.push((tag.clone(), it.text)),
            }
        }
        s.by_tag.sort_by(|a, b| a.0.cmp(&b.0));
    }
    out
}

fn add(total: &mut Cer, part: Cer) {
    total.errors += part.errors;
    total.length += part.length;
}

/// One item scored in two reports.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Change {
    pub id: String,
    pub stage: Stage,
    pub provider: String,
    pub before: Cer,
    pub after: Cer,
    pub before_output: String,
    pub after_output: String,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Comparison {
    pub improved: Vec<Change>,
    pub worse: Vec<Change>,
    pub same: usize,
    /// Items present in only one of the reports.
    pub unmatched: usize,
}

/// Matches items by id, stage and provider and sorts them by the word score.
pub fn compare(before: &Report, after: &Report) -> Comparison {
    let mut c = Comparison::default();
    let mut matched_after = vec![false; after.items.len()];
    for b in &before.items {
        let found = after
            .items
            .iter()
            .enumerate()
            .find(|(_, a)| a.id == b.id && a.stage == b.stage && a.provider == b.provider);
        let Some((idx, a)) = found else {
            c.unmatched += 1;
            continue;
        };
        matched_after[idx] = true;
        let change = Change {
            id: b.id.clone(),
            stage: b.stage,
            provider: b.provider.clone(),
            before: b.text,
            after: a.text,
            before_output: b.output.clone(),
            after_output: a.output.clone(),
        };
        match a.text.errors.cmp(&b.text.errors) {
            std::cmp::Ordering::Less => c.improved.push(change),
            std::cmp::Ordering::Greater => c.worse.push(change),
            std::cmp::Ordering::Equal => c.same += 1,
        }
    }
    c.unmatched += matched_after.iter().filter(|m| !**m).count();
    c
}

/// Tags a history row gets when it is proposed for the seed set: `short`
/// (under five seconds), `long` (over thirty), `mixed` (ASCII letters in the
/// transcript), `correction` (a spoken self-correction marker).
pub fn guess_tags(raw_text: &str, audio_ms: Option<u64>) -> Vec<String> {
    let mut tags = Vec::new();
    match audio_ms {
        Some(ms) if ms < 5_000 => tags.push("short".to_string()),
        Some(ms) if ms > 30_000 => tags.push("long".to_string()),
        _ => {}
    }
    if raw_text.chars().any(|c| c.is_ascii_alphabetic()) {
        tags.push("mixed".to_string());
    }
    if CORRECTION_MARKERS.iter().any(|m| raw_text.contains(m)) {
        tags.push("correction".to_string());
    }
    tags
}

/// Picks up to `limit` ids so that every tag is covered before any tag is
/// repeated: one item per tag in turn, then the rest in the given order.
pub fn pick_spread(candidates: &[(String, Vec<String>)], limit: usize) -> Vec<String> {
    let mut picked: Vec<String> = Vec::new();
    let mut taken = vec![false; candidates.len()];
    // Every distinct tag, in order of first appearance.
    let mut tags: Vec<&str> = Vec::new();
    for (_, t) in candidates {
        for tag in t {
            if !tags.contains(&tag.as_str()) {
                tags.push(tag);
            }
        }
    }
    let mut progress = true;
    while picked.len() < limit && progress {
        progress = false;
        for tag in &tags {
            if picked.len() >= limit {
                break;
            }
            let next = candidates
                .iter()
                .enumerate()
                .find(|(i, (_, t))| !taken[*i] && t.iter().any(|x| x == tag));
            if let Some((i, (id, _))) = next {
                taken[i] = true;
                picked.push(id.clone());
                progress = true;
            }
        }
    }
    for (i, (id, _)) in candidates.iter().enumerate() {
        if picked.len() >= limit {
            break;
        }
        if !taken[i] {
            taken[i] = true;
            picked.push(id.clone());
        }
    }
    picked
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edit_distance_counts_substitutions_insertions_and_deletions() {
        let a: Vec<char> = "今天天氣好".chars().collect();
        assert_eq!(edit_distance(&a, &a), 0);
        assert_eq!(
            edit_distance(&a, &"今天天器好".chars().collect::<Vec<_>>()),
            1
        );
        assert_eq!(
            edit_distance(&a, &"今天氣好".chars().collect::<Vec<_>>()),
            1
        );
        assert_eq!(
            edit_distance(&a, &"今天天氣很好".chars().collect::<Vec<_>>()),
            1
        );
        assert_eq!(edit_distance(&a, &[]), 5);
    }

    #[test]
    fn text_cer_ignores_whitespace_punctuation_and_ascii_case() {
        let c = text_cer("請確認我的 Profile 設定。", "請確認我的profile設定");
        assert_eq!(
            c,
            Cer {
                errors: 0,
                length: 14
            }
        );
        let c = text_cer("明天十點開會。", "明天十一點開會");
        assert_eq!(c.errors, 1);
        assert_eq!(c.length, 6);
    }

    #[test]
    fn cer_rate_of_an_empty_expectation() {
        assert_eq!(
            Cer {
                errors: 0,
                length: 0
            }
            .rate(),
            0.0
        );
        assert_eq!(
            Cer {
                errors: 3,
                length: 0
            }
            .rate(),
            1.0
        );
        assert_eq!(
            Cer {
                errors: 1,
                length: 4
            }
            .rate(),
            0.25
        );
    }

    #[test]
    fn punctuation_cer_compares_only_the_marks() {
        let c = punctuation_cer("好，那我們明天開會。", "好那我們明天開會。");
        assert_eq!(
            c,
            Cer {
                errors: 1,
                length: 2
            }
        );
        let c = punctuation_cer("好，那我們明天開會。", "好,那我們明天開會.");
        assert_eq!(
            c,
            Cer {
                errors: 2,
                length: 2
            }
        );
        let c = punctuation_cer("沒有標點", "也沒有");
        assert_eq!(
            c,
            Cer {
                errors: 0,
                length: 0
            }
        );
        let c = punctuation_cer("沒有標點", "多了。");
        assert_eq!(
            c,
            Cer {
                errors: 1,
                length: 0
            }
        );
    }

    #[test]
    fn diff_merges_runs_and_marks_missing_and_extra_text() {
        let d = diff("明天十點開會", "明天十一點開會了");
        assert_eq!(
            d,
            vec![
                DiffOp::Equal("明天十".into()),
                DiffOp::Insert("一".into()),
                DiffOp::Equal("點開會".into()),
                DiffOp::Insert("了".into()),
            ]
        );
        let d = diff("不對是十一點", "是十一點");
        assert_eq!(
            d,
            vec![
                DiffOp::Delete("不對".into()),
                DiffOp::Equal("是十一點".into())
            ]
        );
        assert_eq!(diff("", ""), Vec::<DiffOp>::new());
    }

    #[test]
    fn check_rewrite_reports_punctuation_whitespace_and_leftover_markers() {
        let c = check_rewrite("明天十一點開會。");
        assert_eq!(
            c,
            Checks {
                ends_with_punctuation: true,
                trailing_whitespace: false,
                correction_marker_left: false
            }
        );
        let c = check_rewrite("明天十點開會，不對，是十一點\n");
        assert_eq!(
            c,
            Checks {
                ends_with_punctuation: false,
                trailing_whitespace: true,
                correction_marker_left: true
            }
        );
        assert!(!check_rewrite("").ends_with_punctuation);
    }

    #[test]
    fn seed_tsv_round_trips_tags_and_multiline_text() {
        let rows = vec![
            SeedRow {
                id: "50".into(),
                audio: PathBuf::from("rec/a.wav"),
                spoken: "第一行\n第二行".into(),
                final_text: "第一行，第二行。".into(),
                tags: vec!["short".into(), "mixed".into()],
            },
            SeedRow {
                id: "51".into(),
                audio: PathBuf::from("rec/b.wav"),
                spoken: "好".into(),
                final_text: "好。".into(),
                tags: vec![],
            },
        ];
        let text = seed_tsv(&rows);
        assert!(text.starts_with(SEED_HEADER), "{text}");
        assert!(text.contains("第一行\\n第二行"), "{text}");
        assert_eq!(parse_seed_tsv(&text).unwrap(), rows);
    }

    #[test]
    fn seed_tsv_skips_blank_and_comment_lines_and_rejects_short_rows() {
        let text = format!("{SEED_HEADER}\n\n# note\n1\ta.wav\t\t說了\t說了。\n");
        let rows = parse_seed_tsv(&text).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].tags, Vec::<String>::new());
        let bad = format!("{SEED_HEADER}\n1\ta.wav\tshort\n");
        let err = parse_seed_tsv(&bad).unwrap_err();
        assert!(err.contains("line 2"), "{err}");
    }

    #[test]
    fn scored_item_measures_output_and_charges_a_failure_in_full() {
        let ok = ItemResult::scored(
            "1",
            Stage::Llm,
            "gemma",
            &["short".to_string()],
            "明天十一點開會。",
            Ok("明天十一點開會".into()),
            120,
        );
        assert_eq!(
            ok.text,
            Cer {
                errors: 0,
                length: 7
            }
        );
        assert_eq!(
            ok.punctuation,
            Cer {
                errors: 1,
                length: 1
            }
        );
        assert!(!ok.checks.unwrap().ends_with_punctuation);
        assert_eq!(ok.error, None);

        let failed = ItemResult::scored(
            "1",
            Stage::Asr,
            "breeze",
            &[],
            "明天十一點開會",
            Err("timeout".into()),
            0,
        );
        assert_eq!(
            failed.text,
            Cer {
                errors: 7,
                length: 7
            }
        );
        assert_eq!(failed.error.as_deref(), Some("timeout"));
        assert_eq!(failed.checks, None, "ASR output has no shape checks");
    }

    fn item(
        id: &str,
        stage: Stage,
        provider: &str,
        tags: &[&str],
        exp: &str,
        out: &str,
    ) -> ItemResult {
        ItemResult::scored(
            id,
            stage,
            provider,
            &tags.iter().map(|t| t.to_string()).collect::<Vec<_>>(),
            exp,
            Ok(out.to_string()),
            10,
        )
    }

    fn report(items: Vec<ItemResult>) -> Report {
        Report {
            name: "r".into(),
            created: "2026-10-08T00:00:00+08:00".into(),
            notes: vec![],
            items,
        }
    }

    #[test]
    fn summary_totals_per_stage_provider_and_tag() {
        let r = report(vec![
            item(
                "1",
                Stage::Asr,
                "breeze",
                &["short"],
                "一二三四",
                "一二三四",
            ),
            item("2", Stage::Asr, "breeze", &["long"], "一二三四", "一二三五"),
            item("3", Stage::Asr, "turbo", &["short"], "一二三四", "一二五五"),
            ItemResult::scored("4", Stage::Asr, "turbo", &[], "一二", Err("x".into()), 0),
        ]);
        let s = summarize(&r);
        assert_eq!(s.len(), 2);
        let breeze = s.iter().find(|s| s.provider == "breeze").unwrap();
        assert_eq!(breeze.items, 2);
        assert_eq!(breeze.failed, 0);
        assert_eq!(
            breeze.text,
            Cer {
                errors: 1,
                length: 8
            }
        );
        assert_eq!(breeze.ms_total, 20);
        assert_eq!(
            breeze.by_tag,
            vec![
                (
                    "long".to_string(),
                    Cer {
                        errors: 1,
                        length: 4
                    }
                ),
                (
                    "short".to_string(),
                    Cer {
                        errors: 0,
                        length: 4
                    }
                ),
            ]
        );
        let turbo = s.iter().find(|s| s.provider == "turbo").unwrap();
        assert_eq!(turbo.failed, 1);
        assert_eq!(
            turbo.text,
            Cer {
                errors: 4,
                length: 6
            }
        );
    }

    #[test]
    fn compare_matches_items_and_sorts_them_into_improved_worse_same() {
        let before = report(vec![
            item("1", Stage::Llm, "gemma", &[], "一二三四", "一二三五"),
            item("2", Stage::Llm, "gemma", &[], "一二三四", "一二三四"),
            item("3", Stage::Llm, "gemma", &[], "一二三四", "一二三四"),
            item("9", Stage::Llm, "gemma", &[], "一二", "一二"),
        ]);
        let after = report(vec![
            item("1", Stage::Llm, "gemma", &[], "一二三四", "一二三四"),
            item("2", Stage::Llm, "gemma", &[], "一二三四", "一二五五"),
            item("3", Stage::Llm, "gemma", &[], "一二三四", "一二三四"),
            item("3", Stage::Asr, "breeze", &[], "一二三四", "一二三四"),
        ]);
        let c = compare(&before, &after);
        assert_eq!(c.improved.len(), 1);
        assert_eq!(c.improved[0].id, "1");
        assert_eq!(c.improved[0].after_output, "一二三四");
        assert_eq!(c.worse.len(), 1);
        assert_eq!(c.worse[0].id, "2");
        assert_eq!(c.same, 1);
        assert_eq!(c.unmatched, 2);
    }

    #[test]
    fn guess_tags_from_length_script_and_markers() {
        assert_eq!(guess_tags("好的", Some(2_000)), vec!["short"]);
        assert_eq!(
            guess_tags("請確認 Profile 設定", Some(10_000)),
            vec!["mixed"]
        );
        assert_eq!(
            guess_tags("十點開會 不對 是十一點", Some(40_000)),
            vec!["long", "correction"]
        );
        assert_eq!(guess_tags("沒有特徵", None), Vec::<String>::new());
    }

    #[test]
    fn pick_spread_covers_every_tag_before_repeating_and_fills_in_order() {
        let c = |id: &str, tags: &[&str]| {
            (
                id.to_string(),
                tags.iter().map(|t| t.to_string()).collect::<Vec<_>>(),
            )
        };
        let cands = vec![
            c("1", &["short"]),
            c("2", &["short"]),
            c("3", &["long"]),
            c("4", &[]),
            c("5", &["mixed"]),
            c("6", &["correction"]),
        ];
        assert_eq!(pick_spread(&cands, 4), vec!["1", "3", "5", "6"]);
        assert_eq!(pick_spread(&cands, 10), vec!["1", "3", "5", "6", "2", "4"]);
        assert_eq!(pick_spread(&cands, 0), Vec::<String>::new());
    }
}
