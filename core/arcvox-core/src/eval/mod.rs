//! Quality measurement: a seed set of recordings with the text that was
//! spoken and the text that should come out, scored by character error rate
//! against what a provider or a profile produces. Two scores per item: the
//! words (whitespace and punctuation ignored, ASCII case folded) and the
//! punctuation on its own, so a wrong comma does not hide a wrong word.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

pub mod run;
pub use run::{list_reports, rows_from_history, Examples, Progress, ReportHead, RunRequest};

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
    /// A reference translation of `final_text`, when the set has the
    /// sixth column; the translate stage is scored against it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub translated: Option<String>,
}

pub const SEED_HEADER: &str = "id\taudio\ttags\tspoken\tfinal";
/// The header of a set with reference translations.
pub const SEED_HEADER_TRANSLATED: &str = "id\taudio\ttags\tspoken\tfinal\ttranslated";

/// Parses the tab-separated seed set. Tags are comma-separated; empty lines
/// and lines starting with `#` are skipped. Newlines inside a text are
/// written as `\n`.
pub fn parse_seed_tsv(text: &str) -> Result<Vec<SeedRow>, String> {
    let mut rows = Vec::new();
    for (n, line) in text.lines().enumerate() {
        if n == 0 && matches!(line.trim_end(), SEED_HEADER | SEED_HEADER_TRANSLATED) {
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
            translated: cols
                .get(5)
                .map(|c| unescape(c))
                .filter(|t| !t.trim().is_empty()),
        });
    }
    Ok(rows)
}

/// Writes the set; the sixth column is there when any row has a reference
/// translation.
pub fn seed_tsv(rows: &[SeedRow]) -> String {
    let translated = rows.iter().any(|r| r.translated.is_some());
    let mut out = String::from(if translated {
        SEED_HEADER_TRANSLATED
    } else {
        SEED_HEADER
    });
    out.push('\n');
    for r in rows {
        out.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}",
            r.id,
            r.audio.display(),
            r.tags.join(","),
            escape(&r.spoken),
            escape(&r.final_text)
        ));
        if translated {
            out.push('\t');
            out.push_str(&escape(r.translated.as_deref().unwrap_or("")));
        }
        out.push('\n');
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

/// How often a kind of mark was written where none was expected (`extra`)
/// or left out where one was (`missing`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct MarkCount {
    pub extra: usize,
    pub missing: usize,
}

/// Punctuation and spacing differences by kind, read off the character
/// diff: `stop` is a full stop at the end of the text, `inner` a comma,
/// enumeration comma, semicolon, colon or a full stop inside the text,
/// `question` a question mark, `quote` corner quotes, `other` every other
/// mark, `space` a space between a CJK character and a Latin letter or
/// digit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Marks {
    pub stop: MarkCount,
    pub question: MarkCount,
    pub inner: MarkCount,
    pub quote: MarkCount,
    pub other: MarkCount,
    pub space: MarkCount,
}

impl Marks {
    pub fn add(&mut self, other: Marks) {
        for (a, b) in self.each_mut().into_iter().zip(other.each()) {
            a.extra += b.extra;
            a.missing += b.missing;
        }
    }

    /// The kinds in display order, with their names.
    pub const KINDS: [&'static str; 6] = ["stop", "question", "inner", "quote", "other", "space"];

    pub fn each(&self) -> [MarkCount; 6] {
        [
            self.stop,
            self.question,
            self.inner,
            self.quote,
            self.other,
            self.space,
        ]
    }

    fn each_mut(&mut self) -> [&mut MarkCount; 6] {
        [
            &mut self.stop,
            &mut self.question,
            &mut self.inner,
            &mut self.quote,
            &mut self.other,
            &mut self.space,
        ]
    }
}

fn is_cjk(c: char) -> bool {
    matches!(c,
        '\u{4E00}'..='\u{9FFF}'
        | '\u{3400}'..='\u{4DBF}'
        | '\u{F900}'..='\u{FAFF}'
        | '\u{3040}'..='\u{30FF}'
        | '\u{AC00}'..='\u{D7AF}')
}

/// A CJK character next to a Latin letter or digit, in either order.
fn is_script_boundary(left: Option<char>, right: Option<char>) -> bool {
    match (left, right) {
        (Some(l), Some(r)) => {
            (is_cjk(l) && r.is_ascii_alphanumeric()) || (l.is_ascii_alphanumeric() && is_cjk(r))
        }
        _ => false,
    }
}

/// Which counter a mark belongs to; `trailing` says whether the mark is
/// the last non-whitespace character of its text.
fn mark_kind(c: char, trailing: bool) -> Option<usize> {
    if !is_punctuation(c) {
        return None;
    }
    Some(match c {
        '。' if trailing => 0,
        '？' | '?' => 1,
        '，' | '、' | '；' | '：' | '。' | ',' | ';' | ':' => 2,
        '「' | '」' | '『' | '』' => 3,
        _ => 4,
    })
}

/// Punctuation and spacing differences between the expected and the
/// actual text, by kind.
pub fn marks(expected: &str, actual: &str) -> Marks {
    let a: Vec<char> = expected.chars().collect();
    let b: Vec<char> = actual.chars().collect();
    let last_mark = |t: &[char]| t.iter().rposition(|c| !c.is_whitespace());
    let (last_a, last_b) = (last_mark(&a), last_mark(&b));
    let mut m = Marks::default();
    let (mut i, mut j) = (0usize, 0usize);
    for op in diff(expected, actual) {
        match op {
            DiffOp::Equal(s) => {
                let n = s.chars().count();
                i += n;
                j += n;
            }
            DiffOp::Delete(s) => {
                let n = s.chars().count();
                if s.chars().all(char::is_whitespace) {
                    if is_script_boundary(i.checked_sub(1).map(|k| a[k]), a.get(i + n).copied()) {
                        m.space.missing += 1;
                    }
                } else {
                    for (k, c) in s.chars().enumerate() {
                        if let Some(kind) = mark_kind(c, last_a == Some(i + k)) {
                            m.each_mut()[kind].missing += 1;
                        }
                    }
                }
                i += n;
            }
            DiffOp::Insert(s) => {
                let n = s.chars().count();
                if s.chars().all(char::is_whitespace) {
                    if is_script_boundary(j.checked_sub(1).map(|k| b[k]), b.get(j + n).copied()) {
                        m.space.extra += 1;
                    }
                } else {
                    for (k, c) in s.chars().enumerate() {
                        if let Some(kind) = mark_kind(c, last_b == Some(j + k)) {
                            m.each_mut()[kind].extra += 1;
                        }
                    }
                }
                j += n;
            }
        }
    }
    m
}

/// The categories an item is reported under: `question`, `list`, `digits`
/// and `english` read off the expected text, `correction` from a spoken
/// marker in it or the stored tag, `plain` when none of those applies;
/// then the stored tags (`short`, `long`, and anything written by hand)
/// except `mixed`, which `english` replaces.
pub fn categories(expected: &str, tags: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    if expected.contains(['？', '?']) {
        out.push("question".into());
    }
    if expected.contains('\n') {
        out.push("list".into());
    }
    if expected.chars().any(|c| c.is_ascii_digit()) {
        out.push("digits".into());
    }
    if expected.chars().any(|c| c.is_ascii_alphabetic()) {
        out.push("english".into());
    }
    if CORRECTION_MARKERS.iter().any(|m| expected.contains(m))
        || tags.iter().any(|t| t == "correction")
    {
        out.push("correction".into());
    }
    if out.is_empty() {
        out.push("plain".into());
    }
    for t in tags {
        if t != "mixed" && !out.contains(t) {
            out.push(t.clone());
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
    /// `final_text` through the profile's translation module alone, scored
    /// against `translated` where the set has it.
    Translate,
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
            Stage::Asr | Stage::Translate => None,
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

/// Who made a report: the command line or the application's quality page.
impl ItemResult {
    /// An item with nothing to score against: the output and the time are
    /// kept, both scores stay empty (length 0).
    pub fn unscored(
        id: &str,
        stage: Stage,
        provider: &str,
        tags: &[String],
        output: Result<String, String>,
        ms: u64,
    ) -> ItemResult {
        let (text_out, error) = match output {
            Ok(t) => (t, None),
            Err(e) => (String::new(), Some(e)),
        };
        ItemResult {
            id: id.to_string(),
            stage,
            provider: provider.to_string(),
            tags: tags.to_vec(),
            expected: String::new(),
            output: text_out,
            error,
            ms,
            text: Cer::default(),
            punctuation: Cer::default(),
            checks: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    #[default]
    Command,
    App,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Report {
    pub name: String,
    /// Unix seconds, as text.
    pub created: String,
    /// What the run was made with, for the record: profile, prompt source, data folder.
    pub notes: Vec<String>,
    pub items: Vec<ItemResult>,
    #[serde(default)]
    pub origin: Origin,
    /// The run was stopped before every row was done.
    #[serde(default)]
    pub stopped: bool,
}

/// Scores added up over a set of items.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Totals {
    pub items: usize,
    pub text: Cer,
    pub punctuation: Cer,
    pub marks: Marks,
}

impl Totals {
    fn add_item(&mut self, it: &ItemResult) {
        self.items += 1;
        // A row with nothing to score against counts, and adds nothing else.
        if it.expected.is_empty() && it.text == Cer::default() {
            return;
        }
        add(&mut self.text, it.text);
        add(&mut self.punctuation, it.punctuation);
        self.marks.add(marks(&it.expected, &it.output));
    }
}

/// Totals for one stage and provider, overall and per category (in order
/// of first appearance).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Summary {
    pub stage: Stage,
    pub provider: String,
    pub items: usize,
    pub failed: usize,
    pub ms_total: u64,
    pub total: Totals,
    pub by_category: Vec<(String, Totals)>,
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
                    ms_total: 0,
                    total: Totals::default(),
                    by_category: Vec::new(),
                });
                out.last_mut().unwrap()
            }
        };
        s.items += 1;
        s.failed += usize::from(it.error.is_some());
        s.ms_total += it.ms;
        s.total.add_item(it);
        for cat in categories(&it.expected, &it.tags) {
            match s.by_category.iter_mut().find(|(c, _)| *c == cat) {
                Some((_, t)) => t.add_item(it),
                None => {
                    let mut t = Totals::default();
                    t.add_item(it);
                    s.by_category.push((cat, t));
                }
            }
        }
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
    pub categories: Vec<String>,
    pub before: Cer,
    pub after: Cer,
    pub before_punctuation: Cer,
    pub after_punctuation: Cer,
    pub before_output: String,
    pub after_output: String,
}

impl Change {
    /// Errors gained (positive) or lost in the word score.
    pub fn words_delta(&self) -> i64 {
        self.after.errors as i64 - self.before.errors as i64
    }

    pub fn punctuation_delta(&self) -> i64 {
        self.after_punctuation.errors as i64 - self.before_punctuation.errors as i64
    }

    /// Worse in words first, then worse in punctuation, then improved in
    /// words, then improved in punctuation only; larger changes first.
    fn rank(&self) -> (u8, i64) {
        let (w, p) = (self.words_delta(), self.punctuation_delta());
        if w > 0 {
            (0, -w)
        } else if p > 0 {
            (1, -p)
        } else if w < 0 {
            (2, w)
        } else {
            (3, p)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Tally {
    pub improved: usize,
    pub worse: usize,
    pub same: usize,
}

impl Tally {
    fn count(&mut self, delta: i64) {
        match delta.cmp(&0) {
            std::cmp::Ordering::Less => self.improved += 1,
            std::cmp::Ordering::Greater => self.worse += 1,
            std::cmp::Ordering::Equal => self.same += 1,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Comparison {
    /// Items whose word or punctuation score changed, worst first.
    pub changed: Vec<Change>,
    /// Items present in only one of the reports.
    pub unmatched: usize,
    pub words: Tally,
    pub punctuation: Tally,
    /// Per category: the word tally and the punctuation tally.
    pub by_category: Vec<(String, Tally, Tally)>,
    pub before: Vec<Summary>,
    pub after: Vec<Summary>,
}

/// Matches items by id and stage, and by provider only where a report ran
/// several providers in that stage (a provider label that changed between
/// runs, such as a model renamed, must not hide the comparison); tallies
/// the word and the punctuation score apart, overall and per category.
pub fn compare(before: &Report, after: &Report) -> Comparison {
    let mut c = Comparison {
        before: summarize(before),
        after: summarize(after),
        ..Comparison::default()
    };
    let providers_in = |stage: Stage| -> usize {
        c.before
            .iter()
            .chain(c.after.iter())
            .filter(|s| s.stage == stage)
            .map(|s| s.provider.as_str())
            .collect::<std::collections::BTreeSet<_>>()
            .len()
    };
    let mut matched_after = vec![false; after.items.len()];
    for b in &before.items {
        let by_provider = providers_in(b.stage) > 2
            || c.before.iter().filter(|s| s.stage == b.stage).count() > 1
            || c.after.iter().filter(|s| s.stage == b.stage).count() > 1;
        let found = after.items.iter().enumerate().find(|(_, a)| {
            a.id == b.id && a.stage == b.stage && (!by_provider || a.provider == b.provider)
        });
        let Some((idx, a)) = found else {
            c.unmatched += 1;
            continue;
        };
        matched_after[idx] = true;
        let change = Change {
            id: b.id.clone(),
            stage: b.stage,
            provider: b.provider.clone(),
            categories: categories(&b.expected, &b.tags),
            before: b.text,
            after: a.text,
            before_punctuation: b.punctuation,
            after_punctuation: a.punctuation,
            before_output: b.output.clone(),
            after_output: a.output.clone(),
        };
        let (w, p) = (change.words_delta(), change.punctuation_delta());
        c.words.count(w);
        c.punctuation.count(p);
        for cat in &change.categories {
            let entry = match c.by_category.iter_mut().find(|(n, _, _)| n == cat) {
                Some(e) => e,
                None => {
                    c.by_category
                        .push((cat.clone(), Tally::default(), Tally::default()));
                    c.by_category.last_mut().unwrap()
                }
            };
            entry.1.count(w);
            entry.2.count(p);
        }
        if w != 0 || p != 0 {
            c.changed.push(change);
        }
    }
    c.changed.sort_by_key(Change::rank);
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

/// A fixed subset of a seed set for quick runs: every category is covered
/// in turn until `limit` rows are taken, so the rare categories are all in
/// and the common ones share the rest. The order the rows are tried in is
/// a hash of the id, so the same set gives the same slice whatever its
/// order on disk.
pub fn pick_slice(rows: &[SeedRow], limit: usize) -> Vec<SeedRow> {
    let mut ordered: Vec<&SeedRow> = rows.iter().collect();
    ordered.sort_by_key(|r| (fnv1a(&r.id), r.id.clone()));
    let keyed: Vec<(String, Vec<String>)> = ordered
        .iter()
        .map(|r| (r.id.clone(), categories(&r.final_text, &r.tags)))
        .collect();
    pick_spread(&keyed, limit)
        .into_iter()
        .filter_map(|id| ordered.iter().find(|r| r.id == id).map(|r| (*r).clone()))
        .collect()
}

fn fnv1a(s: &str) -> u64 {
    s.bytes().fold(0xcbf29ce484222325u64, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x100000001b3)
    })
}

/// How the examples shown to the rewrite model are chosen from a pool.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExamplePick {
    /// The rows whose spoken text shares the most character pairs with the
    /// one under test.
    Similar,
    /// A fixed arbitrary choice per row, the control for `Similar`.
    Random,
}

impl ExamplePick {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "similar" => Some(Self::Similar),
            "random" => Some(Self::Random),
            _ => None,
        }
    }
}

/// Character pairs of the word characters, the unit `similarity` counts.
fn bigrams(s: &str) -> Vec<(char, char)> {
    let chars = normalize_text(s);
    let mut pairs: Vec<(char, char)> = chars.windows(2).map(|w| (w[0], w[1])).collect();
    pairs.sort_unstable();
    pairs
}

/// Dice coefficient over character pairs: 1 for the same words, 0 for
/// nothing in common. Both inputs are sorted pair lists from `bigrams`.
fn similarity(a: &[(char, char)], b: &[(char, char)]) -> f64 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let (mut i, mut j, mut shared) = (0, 0, 0usize);
    while i < a.len() && j < b.len() {
        match a[i].cmp(&b[j]) {
            std::cmp::Ordering::Less => i += 1,
            std::cmp::Ordering::Greater => j += 1,
            std::cmp::Ordering::Equal => {
                shared += 1;
                i += 1;
                j += 1;
            }
        }
    }
    2.0 * shared as f64 / (a.len() + b.len()) as f64
}

/// Up to `k` rows of `pool` as examples for the row `id` with text `spoken`.
/// The row itself is never an example, so a pool that holds the set under
/// test is held out row by row. The best example comes last, nearest the
/// utterance. `Random` orders the pool by a hash of both ids, so every row
/// gets its own fixed choice.
pub fn pick_examples(
    pool: &[SeedRow],
    id: &str,
    spoken: &str,
    k: usize,
    pick: ExamplePick,
) -> Vec<crate::config::Example> {
    let others = pool.iter().filter(|r| r.id != id);
    let mut ranked: Vec<(f64, u64, &SeedRow)> = match pick {
        ExamplePick::Similar => {
            let own = bigrams(spoken);
            others
                .map(|r| (similarity(&own, &bigrams(&r.spoken)), 0, r))
                .collect()
        }
        ExamplePick::Random => others
            .map(|r| (0.0, fnv1a(&format!("{id}:{}", r.id)), r))
            .collect(),
    };
    // Best first for the cut; ties by id so the result does not depend on
    // the pool's order on disk.
    ranked.sort_by(|a, b| {
        b.0.total_cmp(&a.0)
            .then(a.1.cmp(&b.1))
            .then(a.2.id.cmp(&b.2.id))
    });
    ranked
        .into_iter()
        .take(k)
        .rev()
        .map(|(_, _, r)| crate::config::Example {
            raw: r.spoken.clone(),
            refined: r.final_text.clone(),
        })
        .collect()
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
                translated: None,
            },
            SeedRow {
                id: "51".into(),
                audio: PathBuf::from("rec/b.wav"),
                spoken: "好".into(),
                final_text: "好。".into(),
                tags: vec![],
                translated: None,
            },
        ];
        let text = seed_tsv(&rows);
        assert!(text.starts_with(SEED_HEADER), "{text}");
        assert!(!text.contains("translated"), "{text}");
        assert!(text.contains("第一行\\n第二行"), "{text}");
        assert_eq!(parse_seed_tsv(&text).unwrap(), rows);
        // A reference translation adds the sixth column, empty where a row has none.
        let mut with = rows.clone();
        with[0].translated = Some("Line one, line two.".into());
        let text = seed_tsv(&with);
        assert!(text.starts_with(SEED_HEADER_TRANSLATED), "{text}");
        assert!(text.contains("\t好。\t\n"), "{text}");
        assert_eq!(parse_seed_tsv(&text).unwrap(), with);
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
            origin: Origin::Command,
            stopped: false,
        }
    }

    #[test]
    fn summary_totals_per_stage_provider_and_category() {
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
            breeze.total.text,
            Cer {
                errors: 1,
                length: 8
            }
        );
        assert_eq!(breeze.ms_total, 20);
        let names: Vec<&str> = breeze.by_category.iter().map(|(c, _)| c.as_str()).collect();
        assert_eq!(names, vec!["plain", "short", "long"]);
        let long = &breeze.by_category[2].1;
        assert_eq!(long.items, 1);
        assert_eq!(
            long.text,
            Cer {
                errors: 1,
                length: 4
            }
        );
        let turbo = s.iter().find(|s| s.provider == "turbo").unwrap();
        assert_eq!(turbo.failed, 1);
        assert_eq!(
            turbo.total.text,
            Cer {
                errors: 4,
                length: 6
            }
        );
    }

    #[test]
    fn an_unscored_item_counts_without_adding_scores_or_marks() {
        let r = report(vec![
            ItemResult::unscored("1", Stage::Translate, "g", &[], Ok("Why?".into()), 5),
            item("2", Stage::Translate, "g", &[], "好。", "好？"),
        ]);
        let s = summarize(&r);
        assert_eq!(s[0].items, 2);
        assert_eq!(s[0].total.text.length, 1);
        assert_eq!(s[0].total.marks.question.extra, 1);
        assert_eq!(s[0].total.marks.stop.missing, 1);
    }

    #[test]
    fn summary_adds_up_marks_per_kind() {
        let r = report(vec![
            item("1", Stage::Llm, "gemma", &[], "你好嗎？", "你好嗎。"),
            item("2", Stage::Llm, "gemma", &[], "用 git 提交", "用git提交"),
        ]);
        let s = summarize(&r);
        let m = s[0].total.marks;
        assert_eq!(m.question.missing, 1);
        assert_eq!(m.stop.extra, 1);
        assert_eq!(m.space.missing, 2);
        assert_eq!(s[0].by_category[0].0, "question");
        assert_eq!(s[0].by_category[0].1.marks.question.missing, 1);
        assert_eq!(s[0].by_category[0].1.marks.space.missing, 0);
    }

    #[test]
    fn compare_tallies_words_and_punctuation_apart_and_per_category() {
        let before = report(vec![
            item("1", Stage::Llm, "gemma", &[], "一二三四", "一二三五"),
            item("2", Stage::Llm, "gemma", &[], "一二三四", "一二三四"),
            item("3", Stage::Llm, "gemma", &[], "好嗎？", "好嗎。"),
            item("9", Stage::Llm, "gemma", &[], "一二", "一二"),
        ]);
        let after = report(vec![
            item("1", Stage::Llm, "gemma", &[], "一二三四", "一二三四"),
            item("2", Stage::Llm, "gemma", &[], "一二三四", "一二五五"),
            item("3", Stage::Llm, "gemma", &[], "好嗎？", "好嗎？"),
            item("3", Stage::Asr, "breeze", &[], "一二三四", "一二三四"),
        ]);
        let c = compare(&before, &after);
        assert_eq!(c.unmatched, 2);
        assert_eq!(
            c.words,
            Tally {
                improved: 1,
                worse: 1,
                same: 1
            }
        );
        assert_eq!(
            c.punctuation,
            Tally {
                improved: 1,
                worse: 0,
                same: 2
            }
        );
        // Only the items that changed in either score, worst first.
        let ids: Vec<&str> = c.changed.iter().map(|ch| ch.id.as_str()).collect();
        assert_eq!(ids, vec!["2", "1", "3"]);
        assert_eq!(c.changed[2].after_output, "好嗎？");
        let q = c
            .by_category
            .iter()
            .find(|(name, _, _)| name == "question")
            .unwrap();
        assert_eq!(
            q.1,
            Tally {
                improved: 0,
                worse: 0,
                same: 1
            }
        );
        assert_eq!(
            q.2,
            Tally {
                improved: 1,
                worse: 0,
                same: 0
            }
        );
        assert_eq!(c.before[0].provider, "gemma");
        assert_eq!(c.before[0].total.marks.question.missing, 1);
        assert_eq!(c.after[0].total.marks.question.missing, 0);
    }

    #[test]
    fn compare_ignores_the_provider_label_when_each_stage_has_one_provider() {
        let before = report(vec![item(
            "1",
            Stage::Llm,
            "gemma",
            &[],
            "一二三四",
            "一二三五",
        )]);
        let after = report(vec![item(
            "1",
            Stage::Llm,
            "gemma-3-4b-it-q4_k_m",
            &[],
            "一二三四",
            "一二三四",
        )]);
        let c = compare(&before, &after);
        assert_eq!(c.unmatched, 0);
        assert_eq!(c.words.improved, 1);

        let before = report(vec![
            item("1", Stage::Llm, "a", &[], "一二三四", "一二三五"),
            item("1", Stage::Llm, "b", &[], "一二三四", "一二三四"),
        ]);
        let after = report(vec![item(
            "1",
            Stage::Llm,
            "b",
            &[],
            "一二三四",
            "一二三四",
        )]);
        let c = compare(&before, &after);
        assert_eq!(c.unmatched, 1, "with several providers the label matters");
        assert_eq!(c.words.same, 1);
    }

    #[test]
    fn marks_sort_punctuation_by_kind_and_place() {
        let m = marks("你好嗎？", "你好嗎。");
        assert_eq!(m.question.missing, 1);
        assert_eq!(m.stop.extra, 1);
        assert_eq!(m.inner.extra + m.inner.missing, 0);

        let m = marks("今天，很好", "今天很好。");
        assert_eq!(m.inner.missing, 1);
        assert_eq!(m.stop.extra, 1);

        let m = marks("一。二", "一二");
        assert_eq!(m.inner.missing, 1, "a full stop inside the text is inner");
        assert_eq!(m.stop.missing, 0);

        let m = marks("「好」！", "好");
        assert_eq!(m.quote.missing, 2);
        assert_eq!(m.other.missing, 1);

        let m = marks("好。", "好。 ");
        assert_eq!(
            m.stop.extra + m.stop.missing,
            0,
            "trailing whitespace is not a mark"
        );
    }

    #[test]
    fn marks_count_spaces_between_cjk_and_latin_only() {
        let m = marks("用 git 提交", "用git提交");
        assert_eq!(m.space.missing, 2);
        assert_eq!(m.space.extra, 0);

        let m = marks("第2次", "第 2 次");
        assert_eq!(m.space.extra, 2, "digits count like letters");

        let m = marks("a b", "ab");
        assert_eq!(m.space.missing, 0, "two Latin words are not a CJK boundary");

        let m = marks("打開 git 吧", "打開吧");
        assert_eq!(
            m.space.missing, 0,
            "a dropped word is a word error, not spacing"
        );
    }

    #[test]
    fn categories_come_from_the_expected_text_plus_stored_length_tags() {
        let t = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(
            categories("要不要？", &t(&["short", "mixed"])),
            vec!["question", "short"]
        );
        assert_eq!(
            categories("第 2 點\n第 3 點 OK", &t(&["long"])),
            vec!["list", "digits", "english", "long"]
        );
        assert_eq!(categories("沒事", &[]), vec!["plain"]);
        assert_eq!(
            categories("十點 不對 十一點", &t(&["correction"])),
            vec!["correction"]
        );
        assert_eq!(categories("好", &t(&["correction"])), vec!["correction"]);
    }

    #[test]
    fn pick_slice_covers_every_category_and_is_deterministic() {
        let row = |id: &str, final_text: &str, tags: &[&str]| SeedRow {
            id: id.into(),
            audio: PathBuf::from(format!("{id}.wav")),
            tags: tags.iter().map(|t| t.to_string()).collect(),
            spoken: String::new(),
            final_text: final_text.into(),
            translated: None,
        };
        let rows: Vec<SeedRow> = (1..=30)
            .map(|n| row(&n.to_string(), "平常的句子", &[]))
            .chain([
                row("q1", "好嗎？", &[]),
                row("q2", "行嗎？", &[]),
                row("l1", "一\n二", &[]),
                row("c1", "好", &["correction"]),
            ])
            .collect();
        let a = pick_slice(&rows, 8);
        let ids: Vec<&str> = a.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids.len(), 8);
        assert!(ids.contains(&"l1") && ids.contains(&"c1"), "{ids:?}");
        assert!(ids.contains(&"q1") || ids.contains(&"q2"), "{ids:?}");
        assert!(ids.iter().any(|i| i.parse::<u32>().is_ok()), "{ids:?}");
        assert_eq!(a, pick_slice(&rows, 8));
        let mut shuffled = rows.clone();
        shuffled.reverse();
        let mut b: Vec<String> = pick_slice(&shuffled, 8).into_iter().map(|r| r.id).collect();
        let mut a: Vec<String> = a.into_iter().map(|r| r.id).collect();
        a.sort();
        b.sort();
        assert_eq!(a, b, "the pick does not depend on the input order");
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

    fn pool() -> Vec<SeedRow> {
        let row = |id: &str, spoken: &str, final_text: &str| SeedRow {
            id: id.into(),
            audio: PathBuf::new(),
            spoken: spoken.into(),
            final_text: final_text.into(),
            tags: vec![],
            translated: None,
        };
        vec![
            row("1", "今天下午三點開會", "今天下午三點開會"),
            row("2", "明天早上開會嗎", "明天早上開會嗎？"),
            row("3", "這個檔案要刪掉", "這個檔案要刪掉"),
            row("4", "今天下午要開會對吧", "今天下午要開會對吧"),
            row("5", "完全不同的句子", "完全不同的句子"),
        ]
    }

    #[test]
    fn similarity_is_one_for_the_same_words_and_zero_for_none_shared() {
        let a = bigrams("今天，開會。");
        assert_eq!(similarity(&a, &bigrams("今天開會")), 1.0);
        assert_eq!(similarity(&a, &bigrams("完全不同")), 0.0);
        assert_eq!(similarity(&a, &bigrams("")), 0.0);
        let half = similarity(&bigrams("今天開會"), &bigrams("今天休息"));
        assert!(half > 0.0 && half < 1.0, "{half}");
    }

    #[test]
    fn similar_examples_leave_the_row_itself_out_and_end_with_the_best() {
        let pool = pool();
        let picked = pick_examples(&pool, "1", "今天下午三點開會", 2, ExamplePick::Similar);
        let raws: Vec<&str> = picked.iter().map(|e| e.raw.as_str()).collect();
        // Row 4 shares the most pairs; row 2 shares 開會. Row 1 is held out.
        assert_eq!(raws, vec!["明天早上開會嗎", "今天下午要開會對吧"]);
        assert_eq!(picked[0].refined, "明天早上開會嗎？");
    }

    #[test]
    fn random_examples_are_fixed_per_row_and_hold_the_row_out() {
        let pool = pool();
        let a = pick_examples(&pool, "3", "這個檔案要刪掉", 3, ExamplePick::Random);
        let b = pick_examples(&pool, "3", "這個檔案要刪掉", 3, ExamplePick::Random);
        assert_eq!(a, b);
        assert_eq!(a.len(), 3);
        assert!(a.iter().all(|e| e.raw != "這個檔案要刪掉"));
        let mut reversed = pool.clone();
        reversed.reverse();
        assert_eq!(
            pick_examples(&reversed, "3", "這個檔案要刪掉", 3, ExamplePick::Random),
            a
        );
    }

    #[test]
    fn examples_stop_at_the_pool_and_at_zero() {
        let pool = pool();
        assert_eq!(
            pick_examples(&pool, "1", "x", 9, ExamplePick::Similar).len(),
            4
        );
        assert!(pick_examples(&pool, "1", "x", 0, ExamplePick::Random).is_empty());
        assert_eq!(ExamplePick::parse("similar"), Some(ExamplePick::Similar));
        assert_eq!(ExamplePick::parse("nearest"), None);
    }
}
