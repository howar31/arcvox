//! Learning from the difference between what was heard and what was kept.
//! A replacement that recurs across dictations (the same text heard, the
//! same text kept) is a dictionary candidate: the kept text as the term,
//! the heard text as its alias. Nothing here changes the dictionary; the
//! suggestions are shown and the user decides.

use std::collections::{BTreeMap, BTreeSet};

use crate::history::Utterance;

pub mod zh;

/// Where a pair of texts comes from. The user's own corrections weigh
/// more than what the pipeline changed by itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Source {
    /// The pipeline's own change: the raw transcript against the final text.
    Pipeline,
    /// The user corrected what was heard: the raw transcript against the
    /// user's version of the spoken words.
    Heard,
    /// The user corrected the result: the final text against the user's
    /// version of it.
    Final,
}

/// One before/after pair of texts, with the row it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pair {
    pub id: String,
    pub from: String,
    pub to: String,
    pub source: Source,
    /// When the row was dictated, unix seconds; 0 when unknown.
    pub at: i64,
}

/// How many rows of each source hold a replacement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Counts {
    pub pipeline: u32,
    pub heard: u32,
    pub finals: u32,
    /// Distinct rows the user corrected, of either kind: a row corrected
    /// both ways counts once.
    pub users: u32,
}

impl Counts {
    /// Rows corrected by the user, of either kind.
    pub fn user(&self) -> u32 {
        self.users
    }
}

/// The rows per source behind one alias or one term.
#[derive(Debug, Clone, Default)]
struct Rows {
    pipeline: BTreeSet<String>,
    heard: BTreeSet<String>,
    finals: BTreeSet<String>,
}

impl Rows {
    fn add(&mut self, source: Source, id: &str) {
        match source {
            Source::Pipeline => self.pipeline.insert(id.to_string()),
            Source::Heard => self.heard.insert(id.to_string()),
            Source::Final => self.finals.insert(id.to_string()),
        };
    }

    fn merge(&mut self, other: &Rows) {
        self.pipeline.extend(other.pipeline.iter().cloned());
        self.heard.extend(other.heard.iter().cloned());
        self.finals.extend(other.finals.iter().cloned());
    }

    fn counts(&self) -> Counts {
        Counts {
            pipeline: self.pipeline.len() as u32,
            heard: self.heard.len() as u32,
            finals: self.finals.len() as u32,
            users: self.heard.union(&self.finals).count() as u32,
        }
    }
}

/// What a heard text is, next to the term it stood for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AliasKind {
    /// Either side has Latin letters or digits: judged by spelling.
    Latin,
    /// Chinese, and no word the dictionaries know: misheard, and safe to
    /// replace wherever it appears.
    NonWord,
    /// A Chinese word that sounds like the term, or that a corrected
    /// transcript shows: misheard, but a word of its own too.
    Misheard,
    /// A Chinese word that sounds unlike the term, seen only in a
    /// corrected result: the user changed the wording.
    Rephrase,
}

/// A text that was heard in place of the term.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Alias {
    pub text: String,
    pub counts: Counts,
    pub kind: AliasKind,
}

impl Alias {
    fn classify(text: &str, term: &str, counts: &Counts, region: zh::Region) -> AliasKind {
        if !zh::is_chinese(text) || !zh::is_chinese(term) {
            // A Latin pair from one corrected result that is not spelled
            // alike (quick for fast) is a change of wording too.
            let result_once = counts.heard == 0 && counts.pipeline == 0 && counts.finals == 1;
            if result_once && !spelled_alike(text, term) {
                AliasKind::Rephrase
            } else {
                AliasKind::Latin
            }
        } else if !zh::is_word(text) {
            AliasKind::NonWord
        } else if counts.heard > 0 || zh::sound_alike(text, term, region) {
            AliasKind::Misheard
        } else {
            AliasKind::Rephrase
        }
    }
}

/// A dictionary candidate: the text kept, with the texts heard for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Suggestion {
    pub term: String,
    pub aliases: Vec<Alias>,
    pub counts: Counts,
    /// A few rows around the term, the term marked.
    pub excerpts: Vec<String>,
    /// The term is in the dictionary already; the aliases are new.
    pub known_term: bool,
    /// A dictionary term this one is a letter or two away from (zephir
    /// kit beside Zephyr Kit): accepting folds this spelling and its
    /// aliases into that entry rather than making a second one.
    pub near_term: Option<String>,
    /// Every alias is the term in another casing or spacing: a form to
    /// fix, not a word heard wrong.
    pub form_only: bool,
    /// When the newest dictation behind it was made, unix seconds.
    pub latest: i64,
}

/// What counts as a candidate.
#[derive(Debug, Clone, PartialEq)]
pub struct Options {
    /// Rows a source must show a term in; one source meeting its minimum
    /// is enough. A correction of what was heard is the surest signal, a
    /// corrected result also carries wording changes, and the pipeline's
    /// own changes carry the rewrite's habits.
    pub min_heard: u32,
    pub min_final: u32,
    pub min_pipeline: u32,
    /// The longest replacement considered, in tokens, on either side; a
    /// longer one is a rewrite, not a term.
    pub max_tokens: usize,
    /// Excerpts kept per suggestion.
    pub excerpts: usize,
    /// The share of a row's tokens that may change before the row counts
    /// as a rewrite, whose fragments are not terms. A row shorter than
    /// `rewrite_len` tokens is never a rewrite: it may be the term alone.
    pub max_changed: f64,
    pub rewrite_len: usize,
    /// Dictionary terms with their aliases: a pair already covered is
    /// skipped.
    pub known: Vec<(String, Vec<String>)>,
    /// Whose readings of a character apply.
    pub region: zh::Region,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            min_heard: 1,
            min_final: 1,
            min_pipeline: 3,
            max_tokens: 8,
            excerpts: 3,
            max_changed: 0.5,
            rewrite_len: 8,
            known: Vec::new(),
            region: zh::Region::Taiwan,
        }
    }
}

/// One unit of comparison: a CJK (or any other non-ASCII) character on its
/// own, or a run of ASCII letters and digits. Whitespace and punctuation
/// are left out.
pub fn tokens(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut word = String::new();
    for c in text.chars() {
        if c.is_ascii_alphanumeric() {
            word.push(c);
            continue;
        }
        if !word.is_empty() {
            out.push(std::mem::take(&mut word));
        }
        if c.is_whitespace() || c.is_ascii_punctuation() || is_cjk_punctuation(c) {
            continue;
        }
        out.push(c.to_string());
    }
    if !word.is_empty() {
        out.push(word);
    }
    out
}

fn is_cjk_punctuation(c: char) -> bool {
    "，。、？！：；「」『』（）《》〈〉【】…—～·‧”“’‘".contains(c)
}

/// Tokens back into text: a space between two ASCII words, nothing
/// elsewhere.
pub fn join(tokens: &[String]) -> String {
    let mut s = String::new();
    let mut prev_ascii = false;
    for t in tokens {
        let ascii = t.chars().all(|c| c.is_ascii_alphanumeric());
        if ascii && prev_ascii {
            s.push(' ');
        }
        s.push_str(t);
        prev_ascii = ascii;
    }
    s
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    Equal,
    Delete,
    Insert,
}

/// Token-level edit script, matches preferred, then deletions, then
/// insertions.
fn diff_tokens(a: &[String], b: &[String]) -> Vec<(Op, usize)> {
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
    let (mut i, mut j) = (a.len(), b.len());
    // (op, index into a for Equal and Delete, into b for Insert)
    let mut steps: Vec<(Op, usize)> = Vec::new();
    while i > 0 || j > 0 {
        if i > 0 && j > 0 && a[i - 1] == b[j - 1] && d[i][j] == d[i - 1][j - 1] {
            steps.push((Op::Equal, i - 1));
            i -= 1;
            j -= 1;
        } else if i > 0 && d[i][j] == d[i - 1][j] + 1 {
            steps.push((Op::Delete, i - 1));
            i -= 1;
        } else if j > 0 && d[i][j] == d[i][j - 1] + 1 {
            steps.push((Op::Insert, j - 1));
            j -= 1;
        } else {
            steps.push((Op::Insert, j - 1));
            steps.push((Op::Delete, i - 1));
            i -= 1;
            j -= 1;
        }
    }
    steps.reverse();
    steps
}

/// A run of tokens in `to` that stands where other tokens stood in `from`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Replacement {
    pub from: Vec<String>,
    pub to: Vec<String>,
    /// Where the kept run starts and ends in the tokens of `to`.
    pub at: (usize, usize),
}

/// What differs between two texts.
#[derive(Debug, Clone, PartialEq)]
pub struct Diffed {
    /// Every maximal run of changes that deletes something and inserts
    /// something. Pure deletions and pure insertions are not replacements.
    pub replacements: Vec<Replacement>,
    /// Changed tokens over the longer text's length, 0 to 1.
    pub changed: f64,
    /// The longer text's length in tokens.
    pub len: usize,
}

pub fn replacements(from: &str, to: &str) -> Diffed {
    let a = tokens(from);
    let b = tokens(to);
    let steps = diff_tokens(&a, &b);
    let longest = a.len().max(b.len());
    let changed = if longest == 0 {
        0.0
    } else {
        steps.iter().filter(|(op, _)| *op != Op::Equal).count() as f64 / longest as f64
    };
    let mut out = Vec::new();
    let mut deleted: Vec<String> = Vec::new();
    let mut inserted: Vec<String> = Vec::new();
    let mut start: Option<usize> = None;
    let mut end = 0usize;
    let flush = |deleted: &mut Vec<String>,
                 inserted: &mut Vec<String>,
                 start: &mut Option<usize>,
                 end: usize,
                 out: &mut Vec<Replacement>| {
        if !deleted.is_empty() && !inserted.is_empty() {
            out.push(Replacement {
                from: std::mem::take(deleted),
                to: std::mem::take(inserted),
                at: (start.unwrap_or(end), end),
            });
        }
        deleted.clear();
        inserted.clear();
        *start = None;
    };
    for (op, idx) in steps {
        match op {
            Op::Equal => flush(&mut deleted, &mut inserted, &mut start, end, &mut out),
            Op::Delete => deleted.push(a[idx].clone()),
            Op::Insert => {
                if start.is_none() {
                    start = Some(idx);
                }
                end = idx + 1;
                inserted.push(b[idx].clone());
            }
        }
    }
    flush(&mut deleted, &mut inserted, &mut start, end, &mut out);
    Diffed {
        replacements: out,
        changed,
        len: longest,
    }
}

/// A replacement worth suggesting: short on both sides, and the kept text
/// is more than one character, so a single swapped character (a slip the
/// rewrite handles) is not a term.
fn term_like(r: &Replacement, max_tokens: usize) -> bool {
    r.from.len() <= max_tokens
        && r.to.len() <= max_tokens
        && join(&r.to).chars().count() >= 2
        && join(&r.from).chars().count() >= 2
}

fn excerpt(to_tokens: &[String], at: (usize, usize), span: usize) -> String {
    let (s, e) = at;
    let lo = s.saturating_sub(span);
    let hi = (e + span).min(to_tokens.len());
    let mut text = String::new();
    if lo > 0 {
        text.push('…');
    }
    text.push_str(&join(&to_tokens[lo..s]));
    text.push('【');
    text.push_str(&join(&to_tokens[s..e]));
    text.push('】');
    text.push_str(&join(&to_tokens[e..hi]));
    if hi < to_tokens.len() {
        text.push('…');
    }
    text
}

#[derive(Default, Clone)]
struct Tally {
    rows: Rows,
    excerpts: Vec<String>,
    /// The newest row behind the alias.
    latest: i64,
}

/// Dictionary candidates from the pairs, newest first: by the latest
/// dictation behind each, then by name.
pub fn suggest(pairs: &[Pair], opts: &Options) -> Vec<Suggestion> {
    // term -> alias -> rows per source
    let mut table: BTreeMap<String, BTreeMap<String, Tally>> = BTreeMap::new();
    for pair in pairs {
        let to_tokens = tokens(&pair.to);
        let diffed = replacements(&pair.from, &pair.to);
        if diffed.len >= opts.rewrite_len && diffed.changed > opts.max_changed {
            continue;
        }
        for r in diffed.replacements {
            if !term_like(&r, opts.max_tokens) {
                continue;
            }
            let term = join(&r.to);
            let alias = join(&r.from);
            if covered(&opts.known, &term, &alias) {
                continue;
            }
            let tally = table.entry(term).or_default().entry(alias).or_default();
            tally.rows.add(pair.source, &pair.id);
            tally.latest = tally.latest.max(pair.at);
            if tally.excerpts.len() < opts.excerpts {
                tally.excerpts.push(excerpt(&to_tokens, r.at, 6));
            }
        }
    }
    let table: Table = merge_forms(table);
    let mut out: Vec<Suggestion> = table
        .into_iter()
        .filter_map(|(term, aliases)| {
            let mut all = Rows::default();
            let mut latest = 0i64;
            let mut aliases: Vec<(Alias, Vec<String>)> = aliases
                .into_iter()
                .filter_map(|(text, tally)| {
                    let counts = tally.rows.counts();
                    let kind = Alias::classify(&text, &term, &counts, opts.region);
                    // A changed wording is no dictation error: not a suggestion.
                    if kind == AliasKind::Rephrase {
                        return None;
                    }
                    all.merge(&tally.rows);
                    latest = latest.max(tally.latest);
                    Some((Alias { text, counts, kind }, tally.excerpts))
                })
                .collect();
            if aliases.is_empty() {
                return None;
            }
            let counts = all.counts();
            // Strongest alias first; its excerpts lead too.
            aliases.sort_by(|(x, _), (y, _)| {
                (y.counts.user(), y.counts.pipeline, &x.text).cmp(&(
                    x.counts.user(),
                    x.counts.pipeline,
                    &y.text,
                ))
            });
            let mut excerpts: Vec<String> = Vec::new();
            for (_, found) in &aliases {
                for e in found {
                    if excerpts.len() < opts.excerpts && !excerpts.contains(e) {
                        excerpts.push(e.clone());
                    }
                }
            }
            let aliases: Vec<Alias> = aliases.into_iter().map(|(a, _)| a).collect();
            // The same word in another casing or spacing is the same term.
            let known_term = opts.known.iter().any(|(t, _)| same_form(t, &term));
            let near_term = (!known_term)
                .then(|| {
                    opts.known
                        .iter()
                        .map(|(t, _)| t)
                        .find(|t| spelled_near(t, &term))
                        .cloned()
                })
                .flatten();
            let form_only = aliases.iter().all(|a| same_form(&a.text, &term));
            Some(Suggestion {
                term,
                aliases,
                counts,
                excerpts,
                known_term,
                near_term,
                form_only,
                latest,
            })
        })
        .filter(|s| {
            s.counts.heard >= opts.min_heard
                || s.counts.finals >= opts.min_final
                || s.counts.pipeline >= opts.min_pipeline
        })
        .collect();
    out.sort_by(|x, y| (y.latest, &x.term).cmp(&(x.latest, &y.term)));
    out
}

/// The letters of a text apart from ASCII case and spaces.
fn form_of(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_whitespace())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// The same letters, apart from ASCII case and spaces.
fn same_form(a: &str, b: &str) -> bool {
    form_of(a) == form_of(b)
}

/// term -> alias -> the rows and excerpts behind the pair
type Table = BTreeMap<String, BTreeMap<String, Tally>>;
/// The writings of one word, each with its aliases.
type Variants = Vec<(String, BTreeMap<String, Tally>)>;

/// Terms that are one word in different casings or spacings become one
/// suggestion: the writing most rows kept is the term, the other writings
/// its aliases, and their aliases join it. The user wants one way of
/// writing a word, and the replacement brings the others to it.
fn merge_forms(table: Table) -> Table {
    let mut groups: BTreeMap<String, Variants> = BTreeMap::new();
    for (term, aliases) in table {
        groups
            .entry(form_of(&term))
            .or_default()
            .push((term, aliases));
    }
    let mut out = BTreeMap::new();
    for (_, mut variants) in groups {
        if variants.len() == 1 {
            let (term, aliases) = variants.pop().expect("one variant");
            out.insert(term, aliases);
            continue;
        }
        let total = |aliases: &BTreeMap<String, Tally>| {
            let mut rows = Rows::default();
            for t in aliases.values() {
                rows.merge(&t.rows);
            }
            rows.counts()
        };
        variants.sort_by(|(x, xa), (y, ya)| {
            let (cx, cy) = (total(xa), total(ya));
            (cy.user(), cy.pipeline, x).cmp(&(cx.user(), cx.pipeline, y))
        });
        let mut variants = variants.into_iter();
        let (term, mut aliases) = variants.next().expect("two or more variants");
        for (other, other_aliases) in variants {
            let mut as_alias = Tally::default();
            for t in other_aliases.values() {
                as_alias.rows.merge(&t.rows);
                as_alias.latest = as_alias.latest.max(t.latest);
                for e in &t.excerpts {
                    if !as_alias.excerpts.contains(e) {
                        as_alias.excerpts.push(e.clone());
                    }
                }
            }
            aliases.insert(other, as_alias);
            for (alias, t) in other_aliases {
                let into = aliases.entry(alias).or_default();
                into.rows.merge(&t.rows);
                into.latest = into.latest.max(t.latest);
                for e in t.excerpts {
                    if !into.excerpts.contains(&e) {
                        into.excerpts.push(e);
                    }
                }
            }
        }
        out.insert(term, aliases);
    }
    out
}

/// The pairs a history row holds: the user's correction of what was heard
/// (the raw transcript against it) and of the result (the pipeline's text
/// against the final text, which differ only once the user changed it).
/// The pipeline's own changes are not a source here: they do not recur.
pub fn pairs_of(rows: &[Utterance]) -> Vec<Pair> {
    let mut out = Vec::new();
    for u in rows {
        if u.error.is_some() {
            continue;
        }
        let id = u.id.0.to_string();
        if let Some(heard) = &u.heard_text {
            if heard != &u.raw_text {
                out.push(Pair {
                    at: u.ts,
                    id: id.clone(),
                    from: u.raw_text.clone(),
                    to: heard.clone(),
                    source: Source::Heard,
                });
            }
        }
        // A translated result is in another language or style: its
        // correction says nothing about what was heard.
        if u.translated {
            continue;
        }
        // The correction of the text the dictation put in, which a rerun
        // chosen since keeps aside.
        let (from, to) = match &u.origin_correction {
            Some((from, to)) => (from, to),
            None => (&u.refined_text, &u.final_text),
        };
        if from != to {
            out.push(Pair {
                at: u.ts,
                id,
                from: from.clone(),
                to: to.clone(),
                source: Source::Final,
            });
        }
    }
    out
}

/// The aliases an accepted suggestion brings along. A replacement never
/// looks at its context, so a heard text that is a word of its own
/// (統一 for 同意) stays a hint only until two or more rows agree on it; a
/// heard text that is no word (車師 for 測試) is safe to replace from one
/// row. With Latin letters or digits, an alias comes along when a
/// corrected transcript shows it, two or more rows agree on it, or it is
/// spelled close to the term; a corrected result alone may be a change of
/// mind (quick to fast), not a slip of the ear.
pub fn aliases_to_add(s: &Suggestion) -> Vec<String> {
    s.aliases
        .iter()
        .filter(|a| {
            let agreed = a.counts.user() + a.counts.pipeline >= 2;
            match a.kind {
                AliasKind::Latin => agreed || a.counts.heard > 0 || spelled_alike(&a.text, &s.term),
                AliasKind::NonWord => true,
                AliasKind::Misheard => agreed,
                AliasKind::Rephrase => false,
            }
        })
        .map(|a| a.text.clone())
        .collect()
}

/// Whether two Latin texts are one letter apart, or two when longer than
/// eight letters: the same word, spelled a little differently.
fn spelled_near(a: &str, b: &str) -> bool {
    if !a.chars().any(|c| c.is_ascii_alphabetic()) || !b.chars().any(|c| c.is_ascii_alphabetic()) {
        return false;
    }
    let fold = |s: &str| -> Vec<char> {
        s.chars()
            .filter(|c| !c.is_whitespace())
            .map(|c| c.to_ascii_lowercase())
            .collect()
    };
    let (x, y) = (fold(a), fold(b));
    let longest = x.len().max(y.len());
    let allowed = if longest > 8 { 2 } else { 1 };
    longest >= 3 && crate::eval::edit_distance(&x, &y) <= allowed
}

/// Whether two texts share most of their letters: the edit distance over
/// their lower-cased characters, spaces left out, is at most half the
/// longer one.
fn spelled_alike(a: &str, b: &str) -> bool {
    let fold = |s: &str| -> Vec<char> {
        s.chars()
            .filter(|c| !c.is_whitespace())
            .map(|c| c.to_ascii_lowercase())
            .collect()
    };
    let (x, y) = (fold(a), fold(b));
    let longest = x.len().max(y.len());
    longest > 0 && crate::eval::edit_distance(&x, &y) * 2 <= longest
}

/// What a field held where a text was put in, read back later.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadBack {
    /// The text is there as it was put in.
    Unchanged,
    /// The text where it was put in, as the user changed it.
    Corrected(String),
    /// Too little of the text is left to tell what became of it.
    TooDifferent,
}

/// The piece of a field that lines up with a text put in earlier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Alignment {
    /// The field's text where the inserted text was, trimmed.
    pub span: String,
    /// Characters changed, added or dropped between the two.
    pub changes: usize,
    /// How many changes still count as a correction of the text.
    pub allowed: usize,
}

impl Alignment {
    /// A span changed more than a correction would be.
    pub fn too_different(&self) -> bool {
        self.changes > self.allowed
    }
}

/// Finds `inserted` in `field` around `at` (the caret before the
/// insertion, in characters) and reports how it reads now. Only the span
/// that lines up with the inserted text is returned, never the field.
/// A span that differs by more than the allowed changes is not a
/// correction; see `align`.
pub fn read_back(inserted: &str, field: &str, at: usize, max_changed: f64) -> ReadBack {
    match align(inserted, field, at, max_changed) {
        None => ReadBack::TooDifferent,
        Some(a) if a.too_different() => ReadBack::TooDifferent,
        Some(a) if a.span == inserted.trim() => ReadBack::Unchanged,
        Some(a) => ReadBack::Corrected(a.span),
    }
}

/// The substring of `field` closest to `inserted`, with how far apart
/// they are: of the substrings starting within a few characters of the
/// caret `at`, the one closest by edit distance is taken. The changes
/// allowed are `max_changed` of the longer length, at least three so a
/// short text may have a word of two characters and its punctuation
/// corrected, and at most half the length so a text replaced outright
/// does not pass. None when the inserted text is blank or the field
/// holds nothing near the caret.
pub fn align(inserted: &str, field: &str, at: usize, max_changed: f64) -> Option<Alignment> {
    let inserted = inserted.trim();
    if inserted.is_empty() {
        return None;
    }
    let field: Vec<char> = field.chars().collect();
    let wanted: Vec<char> = inserted.chars().collect();
    let n = wanted.len();
    let lo = at.saturating_sub(4).min(field.len());
    let hi = (at + n + n / 2 + 20).min(field.len());
    let allowed_for = |longest: usize| -> usize {
        let share = (longest as f64 * max_changed).floor() as usize;
        share.max(3).min(longest / 2)
    };
    if field[lo..hi].iter().collect::<String>().contains(inserted) {
        return Some(Alignment {
            span: inserted.to_string(),
            changes: 0,
            allowed: allowed_for(n),
        });
    }
    let (start, end, changes) = best_span(&wanted, &field, lo, hi)?;
    let span: String = field[start..end].iter().collect();
    let span = span.trim().to_string();
    let longest = n.max(span.chars().count());
    Some(Alignment {
        span,
        changes,
        allowed: allowed_for(longest),
    })
}

/// Finds `inserted` anywhere in `field`, for a field whose caret says
/// nothing about where the text went (a terminal gives its whole screen
/// and a caret that does not map to it). Runs of white space are folded
/// first, since a terminal wraps lines and indents them; the span comes
/// back folded the same way. Of the spans within the allowed changes the
/// one nearest the end wins, since the newest text on a screen is at the
/// bottom and an older copy of the text may stand above it; when none is
/// within them, the closest span is reported. The allowed changes follow
/// `align`.
pub fn align_anywhere(inserted: &str, field: &str, max_changed: f64) -> Option<Alignment> {
    let wanted = fold_space(inserted);
    let field = fold_space(field);
    let n = wanted.len();
    if n == 0 || field.is_empty() {
        return None;
    }
    let allowed_for = |longest: usize| -> usize {
        let share = (longest as f64 * max_changed).floor() as usize;
        share.max(3).min(longest / 2)
    };
    let (start, end, changes) = nearest_span(&wanted, &field, |len| allowed_for(n.max(len)))?;
    let span: String = field[start..end].iter().collect();
    let span = span.trim().to_string();
    let longest = n.max(span.chars().count());
    Some(Alignment {
        span,
        changes,
        allowed: allowed_for(longest),
    })
}

/// Collapses every run of white space to one space; a run with a line
/// break in it (a terminal wrapping and indenting a line) collapses to
/// nothing between two characters that are not written with spaces (CJK).
fn fold_space(s: &str) -> Vec<char> {
    let spaced = |c: char| c.is_ascii() || (c.is_alphanumeric() && !is_cjk(c));
    let mut out: Vec<char> = Vec::new();
    // Some(true) while a run with a line break is pending.
    let mut pending: Option<bool> = None;
    for c in s.chars() {
        if c.is_whitespace() {
            pending = Some(pending.unwrap_or(false) || c == '\n' || c == '\r');
            continue;
        }
        if let Some(wrapped) = pending.take() {
            if let Some(&last) = out.last() {
                if !wrapped || spaced(last) || spaced(c) {
                    out.push(' ');
                }
            }
        }
        out.push(c);
    }
    out
}

fn is_cjk(c: char) -> bool {
    matches!(c as u32, 0x3000..=0x30FF | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF | 0xFF00..=0xFFEF)
}

/// The substring of `field` that lines up with `wanted`, any start, any
/// end: one edit-distance pass with a free start, each cell carrying the
/// start it came from, gives the distance to every end. Each occurrence
/// of the text is a dip in that distance (the ends just past it cost one
/// more per character); of the dips within `allowed(span length)` the
/// last one is taken, else the lowest dip overall.
fn nearest_span(
    wanted: &[char],
    field: &[char],
    allowed: impl Fn(usize) -> usize,
) -> Option<(usize, usize, usize)> {
    let n = wanted.len();
    let m = field.len();
    if n == 0 || m == 0 {
        return None;
    }
    let mut prev: Vec<(usize, usize)> = (0..=m).map(|j| (0, j)).collect();
    let mut cur = vec![(0usize, 0usize); m + 1];
    for i in 1..=n {
        cur[0] = (i, 0);
        for j in 1..=m {
            let cost = usize::from(wanted[i - 1] != field[j - 1]);
            let sub = (prev[j - 1].0 + cost, prev[j - 1].1);
            let skip_field = (cur[j - 1].0 + 1, cur[j - 1].1);
            let skip_wanted = (prev[j].0 + 1, prev[j].1);
            cur[j] = if sub.0 <= skip_field.0 && sub.0 <= skip_wanted.0 {
                sub
            } else if skip_field.0 <= skip_wanted.0 {
                skip_field
            } else {
                skip_wanted
            };
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    let dips = (1..=m).filter(|&j| {
        let d = prev[j].0;
        d <= prev[j - 1].0 && (j == m || d < prev[j + 1].0)
    });
    let mut closest: Option<(usize, usize, usize)> = None;
    let mut last_fit: Option<(usize, usize, usize)> = None;
    for j in dips {
        let (distance, start) = prev[j];
        if closest.is_none_or(|(_, _, d)| distance < d) {
            closest = Some((start, j, distance));
        }
        if distance <= allowed(j - start) {
            last_fit = Some((start, j, distance));
        }
    }
    last_fit.or(closest)
}

/// The substring of `field[lo..hi]` closest to `wanted`: for each start
/// within `lo..lo + 9`, one edit-distance pass against the rest of the
/// window gives the distance to every end at once; the smallest wins,
/// ties going to the length nearest the wanted one.
fn best_span(
    wanted: &[char],
    field: &[char],
    lo: usize,
    hi: usize,
) -> Option<(usize, usize, usize)> {
    let n = wanted.len();
    let mut best: Option<(usize, usize, usize)> = None;
    for start in lo..(lo + 9).min(hi) {
        let window = &field[start..hi];
        let m = window.len();
        // prev[j] is the distance between all of `wanted` and window[..j].
        let mut prev: Vec<usize> = (0..=m).collect();
        let mut cur = vec![0usize; m + 1];
        for i in 1..=n {
            cur[0] = i;
            for j in 1..=m {
                let cost = usize::from(wanted[i - 1] != window[j - 1]);
                cur[j] = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
            }
            std::mem::swap(&mut prev, &mut cur);
        }
        let gap = |a: usize, b: usize| (a as i64 - b as i64).unsigned_abs();
        for (j, &distance) in prev.iter().enumerate().skip(1) {
            let better = match best {
                None => true,
                Some((bs, be, bd)) => {
                    distance < bd || (distance == bd && gap(j, n) < gap(be - bs, n))
                }
            };
            if better {
                best = Some((start, start + j, distance));
            }
        }
    }
    best
}

/// The dictionary already turns `alias` into `term`, or `alias` is a
/// known term itself (the kept text was the slip, not the heard one).
fn covered(known: &[(String, Vec<String>)], term: &str, alias: &str) -> bool {
    known.iter().any(|(t, aliases)| {
        (t.eq_ignore_ascii_case(term) && aliases.iter().any(|a| a.eq_ignore_ascii_case(alias)))
            || t.eq_ignore_ascii_case(alias)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pair(id: &str, from: &str, to: &str, source: Source) -> Pair {
        Pair {
            at: 0,
            id: id.into(),
            from: from.into(),
            to: to.into(),
            source,
        }
    }

    #[test]
    fn tokens_split_cjk_by_character_and_ascii_by_word() {
        assert_eq!(
            tokens("用 Zephyr Kit 寫程式，v2.1！"),
            ["用", "Zephyr", "Kit", "寫", "程", "式", "v2", "1"]
        );
        assert_eq!(join(&tokens("Zephyr Kit 寫")), "Zephyr Kit寫");
    }

    #[test]
    fn replacements_keep_only_runs_that_delete_and_insert() {
        let r = replacements("我用賽佛寫程式然後測試", "我用 Zephyr 寫程式測試").replacements;
        assert_eq!(r.len(), 1);
        assert_eq!(join(&r[0].from), "賽佛");
        assert_eq!(join(&r[0].to), "Zephyr");
        assert_eq!(r[0].at, (2, 3));
        assert!(replacements("好的", "好的").replacements.is_empty());
        assert!(replacements("然後好的", "好的").replacements.is_empty());
    }

    #[test]
    fn a_single_character_slip_is_not_a_term() {
        let r = replacements("我在試一次", "我再試一次").replacements;
        assert_eq!(r.len(), 1);
        assert!(!term_like(&r[0], 8));
    }

    #[test]
    fn suggestions_group_aliases_under_the_term_and_count_rows() {
        let pairs = vec![
            pair("1", "用賽佛寫", "用 Zephyr 寫", Source::Pipeline),
            pair("2", "賽佛很快", "Zephyr 很快", Source::Pipeline),
            pair("3", "問謝佛", "問 Zephyr", Source::Pipeline),
            pair("4", "問謝佛", "問 Zephyr", Source::Heard),
            pair("5", "這是阿斯特", "這是 Astro", Source::Pipeline),
        ];
        let out = suggest(&pairs, &Options::default());
        assert_eq!(out.len(), 1, "{out:?}");
        let s = &out[0];
        assert_eq!(s.term, "Zephyr");
        assert_eq!(s.counts.pipeline, 3);
        assert_eq!(s.counts.heard, 1);
        assert_eq!(s.aliases.len(), 2);
        assert_eq!(s.aliases[0].text, "謝佛");
        assert_eq!(s.aliases[0].counts.heard, 1);
        assert_eq!(s.aliases[1].text, "賽佛");
        assert_eq!(s.aliases[1].counts.pipeline, 2);
        assert_eq!(s.excerpts[0], "問【Zephyr】", "the strongest alias leads");
        assert!(s.excerpts.contains(&"用【Zephyr】寫".to_string()));
        assert!(!s.known_term);
        assert!(!s.form_only);
    }

    #[test]
    fn one_heard_correction_is_enough_and_the_threshold_is_per_term() {
        let pairs = vec![
            pair("1", "這是阿斯特", "這是 Astro", Source::Heard),
            pair("2", "用賽佛寫", "用 Zephyr 寫", Source::Pipeline),
            pair("3", "問謝佛", "問 Zephyr", Source::Pipeline),
            pair("4", "找賽法", "找 Zephyr", Source::Pipeline),
        ];
        let out = suggest(&pairs, &Options::default());
        let terms: Vec<&str> = out.iter().map(|s| s.term.as_str()).collect();
        assert_eq!(terms, ["Astro", "Zephyr"]);
    }

    #[test]
    fn covered_pairs_are_skipped_and_a_known_term_is_marked() {
        let pairs = vec![
            pair("1", "用賽佛寫", "用 Zephyr 寫", Source::Heard),
            pair("2", "問謝佛", "問 Zephyr", Source::Heard),
            pair("3", "Zephyr 很快", "賽佛很快", Source::Heard),
        ];
        let opts = Options {
            known: vec![("Zephyr".into(), vec!["賽佛".into()])],
            ..Options::default()
        };
        let out = suggest(&pairs, &opts);
        assert_eq!(out.len(), 1, "{out:?}");
        assert_eq!(out[0].term, "Zephyr");
        assert!(out[0].known_term);
        assert_eq!(out[0].aliases.len(), 1);
        assert_eq!(out[0].aliases[0].text, "謝佛");
    }

    #[test]
    fn a_rewritten_row_yields_no_terms() {
        let d = replacements("這個東西我覺得應該要重新做一次比較好", "我建議整個重做");
        assert!(d.changed > 0.5, "{}", d.changed);
        let pairs = vec![pair(
            "1",
            "這個東西我覺得應該要重新做一次比較好",
            "我建議整個重做",
            Source::Final,
        )];
        assert!(suggest(&pairs, &Options::default()).is_empty());
    }

    #[test]
    fn excerpts_mark_the_term_and_trim_the_row() {
        let t = tokens("一二三四五六七八九十 Zephyr 甲乙丙丁戊己庚辛");
        assert_eq!(
            excerpt(&t, (10, 11), 6),
            "…五六七八九十【Zephyr】甲乙丙丁戊己…"
        );
    }
}

#[cfg(test)]
mod thresholds {
    use super::*;

    fn pair(id: &str, from: &str, to: &str, source: Source) -> Pair {
        Pair {
            at: 0,
            id: id.into(),
            from: from.into(),
            to: to.into(),
            source,
        }
    }

    #[test]
    fn a_changed_wording_is_left_out_and_the_newest_term_sorts_first() {
        let at = |mut p: Pair, at: i64| {
            p.at = at;
            p
        };
        let pairs = vec![
            at(pair("1", "用別的方式", "用其他方式", Source::Final), 50),
            at(pair("2", "清 cdn 快取", "清 CDN 快取", Source::Final), 10),
            at(pair("3", "清 cdn 吧", "清 CDN 吧", Source::Final), 30),
            at(pair("4", "問謝佛", "問 Zephyr", Source::Final), 20),
            at(pair("5", "問謝佛", "問 Zephyr", Source::Final), 5),
        ];
        let out = suggest(&pairs, &Options::default());
        let terms: Vec<(&str, bool, i64)> = out
            .iter()
            .map(|s| (s.term.as_str(), s.form_only, s.latest))
            .collect();
        assert_eq!(terms, [("CDN", true, 30), ("Zephyr", false, 20)]);
    }
}

#[cfg(test)]
mod from_rows {
    use super::*;
    use crate::history::{HistoryStore, NewUtterance};
    use crate::types::ProfileId;

    fn row(raw: &str, text: &str) -> NewUtterance {
        NewUtterance {
            ts: 100,
            profile_id: ProfileId("default".into()),
            app_name: None,
            raw_text: raw.into(),
            refined_text: text.into(),
            final_text: text.into(),
            degraded: false,
            audio_path: None,
            asr_ms: 1,
            llm_ms: None,
            audio_ms: None,
            dict_fixes: 0,
            snippets: 0,
            llm_changed: false,
            dict_hits: Vec::new(),
            stages: Vec::new(),
        }
    }

    #[test]
    fn a_translated_row_gives_no_final_pair() {
        use crate::history::{StageKind, StageRecord};
        let s = HistoryStore::open_in_memory().unwrap();
        let mut translated = row("測試一下", "A test.");
        translated.stages = vec![StageRecord {
            position: 1,
            kind: StageKind::Translate,
            text: Some("A test.".into()),
            ms: None,
            detail: serde_json::json!({ "target": "英文" }),
        }];
        let id = s.insert(translated).unwrap();
        s.set_final(id, "A quick test.").unwrap();
        s.set_heard(id, Some("測試一下下")).unwrap();
        let rows = s.list_recent(10).unwrap();
        assert!(rows[0].translated);
        let pairs = pairs_of(&rows);
        assert_eq!(pairs.len(), 1, "{pairs:?}");
        assert_eq!(pairs[0].source, Source::Heard);
    }

    #[test]
    fn rows_give_a_heard_pair_and_a_final_pair_only_when_corrected() {
        let s = HistoryStore::open_in_memory().unwrap();
        let a = s.insert(row("用賽佛寫", "用賽佛寫")).unwrap();
        let b = s.insert(row("問一下", "問一下")).unwrap();
        s.set_heard(a, Some("用 Zephyr 寫")).unwrap();
        s.set_final(b, "問一下 Zephyr").unwrap();
        let rows = s.list_recent(10).unwrap();
        let pairs = pairs_of(&rows);
        assert_eq!(pairs.len(), 2);
        let heard = pairs.iter().find(|p| p.source == Source::Heard).unwrap();
        assert_eq!(
            (heard.from.as_str(), heard.to.as_str()),
            ("用賽佛寫", "用 Zephyr 寫")
        );
        let fin = pairs.iter().find(|p| p.source == Source::Final).unwrap();
        assert_eq!(
            (fin.from.as_str(), fin.to.as_str()),
            ("問一下", "問一下 Zephyr")
        );
    }

    #[test]
    fn the_final_pair_stays_while_a_rerun_is_chosen() {
        use crate::history::{Outcome, Redo};
        let s = HistoryStore::open_in_memory().unwrap();
        let b = s.insert(row("問一下", "問一下")).unwrap();
        s.set_final(b, "問一下 Zephyr").unwrap();
        let rerun = Outcome::Done {
            redo: Redo {
                profile_id: ProfileId("default".into()),
                raw_text: "問一下".into(),
                final_text: "問一下。".into(),
                degraded: false,
                asr_ms: 1,
                llm_ms: None,
                dict_fixes: 0,
                snippets: 0,
                llm_changed: false,
                dict_hits: Vec::new(),
                stages: Vec::new(),
            },
            asr_label: "asr".into(),
            llm_label: None,
        };
        s.record_attempt(b, &rerun, true).unwrap();
        let pairs = pairs_of(&s.list_recent(10).unwrap());
        assert_eq!(pairs.len(), 1);
        assert_eq!(
            (pairs[0].from.as_str(), pairs[0].to.as_str()),
            ("問一下", "問一下 Zephyr")
        );
    }

    #[test]
    fn a_chinese_term_heard_once_brings_no_alias() {
        let alias = |text: &str, user: u32| {
            let counts = Counts {
                heard: user,
                users: user,
                ..Counts::default()
            };
            Alias {
                text: text.into(),
                kind: Alias::classify(text, "權限", &counts, zh::Region::Taiwan),
                counts,
            }
        };
        let chinese = Suggestion {
            term: "權限".into(),
            aliases: vec![alias("全線", 3), alias("程序", 1)],
            counts: Counts::default(),
            excerpts: Vec::new(),
            known_term: false,
            near_term: None,
            form_only: false,
            latest: 0,
        };
        assert_eq!(aliases_to_add(&chinese), ["全線"]);
        let latin = Suggestion {
            term: "phase".into(),
            aliases: vec![Alias {
                kind: AliasKind::Latin,
                ..alias("face", 1)
            }],
            ..chinese.clone()
        };
        assert_eq!(aliases_to_add(&latin), ["face"]);
        // A corrected result alone: only an alias spelled like the term.
        let from_result = |text: &str| Alias {
            text: text.into(),
            counts: Counts {
                finals: 1,
                users: 1,
                ..Counts::default()
            },
            kind: AliasKind::Latin,
        };
        let result_only = Suggestion {
            term: "fast".into(),
            aliases: vec![from_result("quick"), from_result("fasst")],
            ..chinese.clone()
        };
        assert_eq!(aliases_to_add(&result_only), ["fasst"]);
        let once = Suggestion {
            aliases: vec![alias("風暴", 1)],
            ..chinese
        };
        assert!(aliases_to_add(&once).is_empty(), "a hint only");
    }
}

#[cfg(test)]
mod distinct_rows {
    use super::*;

    #[test]
    fn a_row_corrected_both_ways_counts_once() {
        let pairs = vec![
            Pair {
                at: 0,
                id: "1".into(),
                from: "問謝佛".into(),
                to: "問 Zephyr".into(),
                source: Source::Heard,
            },
            Pair {
                at: 0,
                id: "1".into(),
                from: "問謝佛".into(),
                to: "問 Zephyr".into(),
                source: Source::Final,
            },
            Pair {
                at: 0,
                id: "2".into(),
                from: "問謝佛".into(),
                to: "問 Zephyr".into(),
                source: Source::Final,
            },
        ];
        let out = suggest(&pairs, &Options::default());
        assert_eq!(out.len(), 1);
        let c = out[0].counts;
        assert_eq!((c.heard, c.finals, c.user()), (1, 2, 2));
        assert_eq!(out[0].aliases[0].counts.user(), 2);
    }
}

#[cfg(test)]
mod forms {
    use super::*;

    fn pair(id: &str, from: &str, to: &str) -> Pair {
        Pair {
            at: 0,
            id: id.into(),
            from: from.into(),
            to: to.into(),
            source: Source::Heard,
        }
    }

    #[test]
    fn casing_variants_of_a_term_become_one_suggestion() {
        let pairs = vec![
            pair("1", "用 sefirkit 吧", "用 Zephyr Kit 吧"),
            pair("2", "用 sefirkit 吧", "用 Zephyr Kit 吧"),
            pair("3", "看 sefirkit 的", "看 zephyr kit 的"),
        ];
        let out = suggest(&pairs, &Options::default());
        assert_eq!(out.len(), 1, "{out:?}");
        let s = &out[0];
        assert_eq!(s.term, "Zephyr Kit");
        assert_eq!(s.counts.user(), 3);
        let aliases: Vec<(&str, u32)> = s
            .aliases
            .iter()
            .map(|a| (a.text.as_str(), a.counts.user()))
            .collect();
        assert_eq!(aliases, [("sefirkit", 3), ("zephyr kit", 1)]);
        assert!(!s.form_only);
    }
}

#[cfg(test)]
mod read_back_tests {
    use super::*;

    #[test]
    fn the_text_as_put_in_is_unchanged() {
        let field = "前面的字。我明天會晚一點到。後面的字";
        assert_eq!(
            read_back("我明天會晚一點到。", field, 5, 0.3),
            ReadBack::Unchanged
        );
    }

    #[test]
    fn a_changed_word_comes_back_as_the_span_alone() {
        let field = "前面的字。我後天會晚一點到。後面的字";
        assert_eq!(
            read_back("我明天會晚一點到。", field, 5, 0.3),
            ReadBack::Corrected("我後天會晚一點到。".into())
        );
        let field = "hello, see you at the Zephyr Kit meeting! bye";
        assert_eq!(
            read_back("see you at the zefir kit meeting!", field, 7, 0.3),
            ReadBack::Corrected("see you at the Zephyr Kit meeting!".into())
        );
    }

    #[test]
    fn one_swapped_character_in_a_short_text_is_a_correction() {
        assert_eq!(
            read_back("漢字 整理後", "前面的字。漢字 整理好。後面的字", 5, 0.3),
            ReadBack::Corrected("漢字 整理好".into())
        );
    }

    #[test]
    fn a_replaced_text_is_too_different_and_an_empty_field_too() {
        let field = "前面的字。完全不同的一句話在這裡。後面的字";
        assert_eq!(
            read_back("我明天會晚一點到。", field, 5, 0.3),
            ReadBack::TooDifferent
        );
        assert_eq!(
            read_back("我明天會晚一點到。", "", 0, 0.3),
            ReadBack::TooDifferent
        );
    }

    #[test]
    fn a_short_text_may_have_three_changes_but_not_half_of_it() {
        // A word of two characters and its punctuation, in seven characters.
        assert_eq!(
            read_back("測試一下，停歇", "測試一下聽寫\n", 0, 0.3),
            ReadBack::Corrected("測試一下聽寫".into())
        );
        let a = align("測試一下，停歇", "測試一下聽寫\n", 0, 0.3).unwrap();
        assert_eq!((a.changes, a.allowed), (3, 3));
        // Four changes in seven are too many.
        assert_eq!(
            read_back("測試一下，停歇", "測試看看聽寫\n", 0, 0.3),
            ReadBack::TooDifferent
        );
        // Three changes in four is more than half.
        assert_eq!(
            read_back("收到謝謝", "好的再見", 0, 0.3),
            ReadBack::TooDifferent
        );
        // A long text keeps the share.
        let a = align("a".repeat(40).as_str(), &"a".repeat(40), 0, 0.3).unwrap();
        assert_eq!(a.allowed, 12);
    }

    #[test]
    fn the_caret_may_be_off_by_a_little() {
        let field = "一二三四五六我後天會晚一點到七八九";
        assert_eq!(
            read_back("我明天會晚一點到", field, 9, 0.3),
            ReadBack::Corrected("我後天會晚一點到".into())
        );
    }

    #[test]
    fn a_screen_is_searched_whole_and_its_line_wraps_folded() {
        // A terminal screen: a prompt, output, then the dictated text on
        // the input line, wrapped and indented by the program, with one
        // word changed.
        let screen = "$ run\nSearched for 1 pattern\n\n> 我後天會晚一點到，請先\n    開始。\n\n  ? for shortcuts";
        let a = align_anywhere("我明天會晚一點到，請先開始。", screen, 0.3).unwrap();
        assert_eq!(a.span, "我後天會晚一點到，請先開始。");
        assert_eq!(a.changes, 1);
        // Latin words keep one space between them across a wrap.
        let screen = "> see you at the Zephyr\n    Kit meeting!";
        let a = align_anywhere("see you at the zefir kit meeting!", screen, 0.3).unwrap();
        assert_eq!(a.span, "see you at the Zephyr Kit meeting!");
        assert!(!a.too_different());
    }

    #[test]
    fn a_space_the_text_had_between_chinese_words_stays() {
        let a = align_anywhere("漢字 整理後", "> 漢字 整理好\n", 0.3).unwrap();
        assert_eq!(a.span, "漢字 整理好");
        assert_eq!(a.changes, 1);
    }

    #[test]
    fn of_two_copies_on_a_screen_the_lower_one_is_taken() {
        let screen = "我明天會晚一點到。\n一些輸出\n我後天會晚一點到。";
        let a = align_anywhere("我明天會晚一點到。", screen, 0.3).unwrap();
        assert_eq!(a.span, "我後天會晚一點到。");
        let screen = "我後天會晚一點到。\n一些輸出\n我明天會晚一點到。";
        let a = align_anywhere("我明天會晚一點到。", screen, 0.3).unwrap();
        assert_eq!(a.span, "我明天會晚一點到。");
        assert_eq!(a.changes, 0);
    }

    #[test]
    fn a_screen_without_the_text_is_too_different() {
        let screen = "$ ls\nsrc  docs  target\n$ cargo test\nrunning 12 tests";
        let a = align_anywhere("我明天會晚一點到。", screen, 0.3).unwrap();
        assert!(a.too_different());
        assert_eq!(align_anywhere("  ", screen, 0.3), None);
        assert_eq!(align_anywhere("我明天", "", 0.3), None);
    }
}

#[cfg(test)]
mod chinese_kinds {
    use super::*;

    fn pair(id: &str, from: &str, to: &str, source: Source) -> Pair {
        Pair {
            at: 0,
            id: id.into(),
            from: from.into(),
            to: to.into(),
            source,
        }
    }

    #[test]
    fn a_non_word_heard_once_brings_its_alias_and_a_word_does_not() {
        let pairs = vec![
            pair("1", "這是寒碾的", "這是慘烈的", Source::Final),
            pair("2", "看風暴層", "看封包層", Source::Final),
        ];
        let out = suggest(&pairs, &Options::default());
        let by_term = |t: &str| out.iter().find(|s| s.term == t).expect(t).clone();
        let non_word = by_term("慘烈");
        assert_eq!(non_word.aliases[0].kind, AliasKind::NonWord);
        assert_eq!(aliases_to_add(&non_word), ["寒碾"]);
        let word = by_term("封包");
        assert_eq!(word.aliases[0].kind, AliasKind::Misheard);
        assert!(aliases_to_add(&word).is_empty(), "a hint only");
    }

    #[test]
    fn a_changed_wording_is_no_suggestion_unless_the_transcript_was_corrected() {
        let pairs = vec![pair("1", "用別的方式", "用其他方式", Source::Final)];
        assert!(suggest(&pairs, &Options::default()).is_empty());
        let pairs = vec![pair("1", "用別的方式", "用其他方式", Source::Heard)];
        let out = suggest(&pairs, &Options::default());
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].aliases[0].kind, AliasKind::Misheard);
    }
}

#[cfg(test)]
mod near_terms {
    use super::*;

    #[test]
    fn a_spelling_a_letter_away_from_a_dictionary_term_names_it() {
        let pairs = vec![Pair {
            at: 0,
            id: "1".into(),
            from: "用 zefirkit 寫".into(),
            to: "用 zephyrkit 寫".into(),
            source: Source::Heard,
        }];
        let opts = Options {
            known: vec![("Zephyr Kit".into(), vec![])],
            ..Options::default()
        };
        let out = suggest(&pairs, &opts);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].term, "zephyrkit");
        assert!(out[0].known_term, "only case and spacing differ");
        assert_eq!(out[0].near_term, None);

        let pairs = vec![Pair {
            at: 0,
            id: "1".into(),
            from: "用 zefirkit 寫".into(),
            to: "用 zephir kit 寫".into(),
            source: Source::Heard,
        }];
        let out = suggest(&pairs, &opts);
        assert_eq!(out[0].term, "zephir kit");
        assert!(!out[0].known_term);
        assert_eq!(out[0].near_term.as_deref(), Some("Zephyr Kit"));
        assert!(spelled_near("zephir kit", "Zephyr Kit"));
        assert!(spelled_near("Slak", "Slack"));
        assert!(
            !spelled_near("form", "from"),
            "a transposition is two edits"
        );
        assert!(!spelled_near("權限", "全線"));
    }
}
