//! Learning from the difference between what was heard and what was kept.
//! A replacement that recurs across dictations (the same text heard, the
//! same text kept) is a dictionary candidate: the kept text as the term,
//! the heard text as its alias. Nothing here changes the dictionary; the
//! suggestions are shown and the user decides.

use std::collections::{BTreeMap, BTreeSet};

use crate::history::Utterance;

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

/// A text that was heard in place of the term.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Alias {
    pub text: String,
    pub counts: Counts,
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
    /// Every alias is the term in another casing or spacing: a form to
    /// fix, not a word heard wrong.
    pub form_only: bool,
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
}

impl Default for Options {
    fn default() -> Self {
        Options {
            min_heard: 1,
            min_final: 2,
            min_pipeline: 3,
            max_tokens: 8,
            excerpts: 3,
            max_changed: 0.5,
            rewrite_len: 8,
            known: Vec::new(),
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
}

/// Dictionary candidates from the pairs, strongest first: by the rows the
/// user corrected, then by the rows the pipeline changed, then by name.
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
            if tally.excerpts.len() < opts.excerpts {
                tally.excerpts.push(excerpt(&to_tokens, r.at, 6));
            }
        }
    }
    let table: Table = merge_forms(table);
    let mut out: Vec<Suggestion> = table
        .into_iter()
        .map(|(term, aliases)| {
            let mut all = Rows::default();
            let mut aliases: Vec<(Alias, Vec<String>)> = aliases
                .into_iter()
                .map(|(text, tally)| {
                    all.merge(&tally.rows);
                    let counts = tally.rows.counts();
                    (Alias { text, counts }, tally.excerpts)
                })
                .collect();
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
            let known_term = opts
                .known
                .iter()
                .any(|(t, _)| t.eq_ignore_ascii_case(&term));
            let form_only = aliases.iter().all(|a| same_form(&a.text, &term));
            Suggestion {
                term,
                aliases,
                counts,
                excerpts,
                known_term,
                form_only,
            }
        })
        .filter(|s| {
            s.counts.heard >= opts.min_heard
                || s.counts.finals >= opts.min_final
                || s.counts.pipeline >= opts.min_pipeline
        })
        .collect();
    out.sort_by(|x, y| {
        (x.form_only, y.counts.user(), y.counts.pipeline, &x.term).cmp(&(
            y.form_only,
            x.counts.user(),
            x.counts.pipeline,
            &y.term,
        ))
    });
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
                    id: id.clone(),
                    from: u.raw_text.clone(),
                    to: heard.clone(),
                    source: Source::Heard,
                });
            }
        }
        if u.final_text != u.refined_text {
            out.push(Pair {
                id,
                from: u.refined_text.clone(),
                to: u.final_text.clone(),
                source: Source::Final,
            });
        }
    }
    out
}

/// The aliases an accepted suggestion brings along. A replacement never
/// looks at its context, so a Chinese word heard once as another Chinese
/// word stays a hint only: the heard word may be a word of its own. A
/// term with Latin letters or digits takes every alias; a Chinese term
/// takes the aliases two or more rows agree on.
pub fn aliases_to_add(s: &Suggestion) -> Vec<String> {
    let latin = s.term.chars().any(|c| c.is_ascii_alphanumeric());
    s.aliases
        .iter()
        .filter(|a| latin || a.counts.user() + a.counts.pipeline >= 2)
        .map(|a| a.text.clone())
        .collect()
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
            id: id.into(),
            from: from.into(),
            to: to.into(),
            source,
        }
    }

    #[test]
    fn a_corrected_result_needs_two_rows_and_casing_pairs_sort_last() {
        let pairs = vec![
            pair("1", "用別的方式", "用其他方式", Source::Final),
            pair("2", "清 cdn 快取", "清 CDN 快取", Source::Final),
            pair("3", "清 cdn 吧", "清 CDN 吧", Source::Final),
            pair("4", "問謝佛", "問 Zephyr", Source::Final),
            pair("5", "問謝佛", "問 Zephyr", Source::Final),
        ];
        let out = suggest(&pairs, &Options::default());
        let terms: Vec<(&str, bool)> = out.iter().map(|s| (s.term.as_str(), s.form_only)).collect();
        assert_eq!(terms, [("Zephyr", false), ("CDN", true)]);
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
            stages: Vec::new(),
        }
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
    fn a_chinese_term_heard_once_brings_no_alias() {
        let alias = |text: &str, user: u32| Alias {
            text: text.into(),
            counts: Counts {
                heard: user,
                users: user,
                ..Counts::default()
            },
        };
        let chinese = Suggestion {
            term: "權限".into(),
            aliases: vec![alias("全線", 3), alias("程序", 1)],
            counts: Counts::default(),
            excerpts: Vec::new(),
            known_term: false,
            form_only: false,
        };
        assert_eq!(aliases_to_add(&chinese), ["全線"]);
        let latin = Suggestion {
            term: "phase".into(),
            aliases: vec![alias("face", 1)],
            ..chinese.clone()
        };
        assert_eq!(aliases_to_add(&latin), ["face"]);
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
                id: "1".into(),
                from: "問謝佛".into(),
                to: "問 Zephyr".into(),
                source: Source::Heard,
            },
            Pair {
                id: "1".into(),
                from: "問謝佛".into(),
                to: "問 Zephyr".into(),
                source: Source::Final,
            },
            Pair {
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
