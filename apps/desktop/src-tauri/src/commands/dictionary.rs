use arcvox_core::config::{Config, DictionaryEntry, Dismissed};
use arcvox_core::learn;
use arcvox_core::Engine;
use serde::Serialize;
use tauri::State;

use super::{engine, CmdResult};
use crate::app::AppState;

/// Replaces the dictionary and the list of turned-down suggestions.
pub fn set(
    engine: &Engine,
    entries: Vec<DictionaryEntry>,
    dismissed: Vec<Dismissed>,
) -> CmdResult<Config> {
    let next = engine.config().update(|c| {
        c.dictionary = entries.clone();
        c.dictionary_dismissed = dismissed.clone();
    })?;
    Ok((*next).clone())
}

#[tauri::command]
pub fn dictionary_set(
    state: State<'_, AppState>,
    entries: Vec<DictionaryEntry>,
    dismissed: Vec<Dismissed>,
) -> CmdResult<Config> {
    set(&engine(&state)?, entries, dismissed)
}

/// A text heard in place of the term.
#[derive(Debug, Clone, Serialize)]
pub struct AliasDto {
    pub text: String,
    /// Rows the user corrected that show it.
    pub rows: u32,
}

/// A dictionary candidate from the dictations the user corrected.
#[derive(Debug, Clone, Serialize)]
pub struct SuggestionDto {
    pub term: String,
    pub aliases: Vec<AliasDto>,
    /// Distinct rows the user corrected that show the term.
    pub rows: u32,
    pub excerpts: Vec<String>,
    /// The term is in the dictionary; accepting adds the aliases to it.
    pub known_term: bool,
    /// A dictionary term a letter or two away: accepting folds this
    /// spelling and its aliases into that entry.
    pub near_term: Option<String>,
    /// The aliases accepting brings along; empty for a term that is a
    /// recognition hint only.
    pub add_aliases: Vec<String>,
    pub form_only: bool,
    /// Every heard text is a Chinese word that sounds like the term: a
    /// hint only, since replacing a word hits the places it was meant.
    pub sound_alike: bool,
}

/// The candidates from the history, the turned-down ones left out.
pub fn suggestions(engine: &Engine) -> CmdResult<Vec<SuggestionDto>> {
    let config = engine.config().current();
    let rows = engine
        .history()
        .list(usize::MAX, None, false)
        .map_err(|e| e.to_string())?;
    let opts = learn::Options {
        known: config
            .dictionary
            .iter()
            .map(|e| (e.term.clone(), e.aliases.clone()))
            .collect(),
        region: learn::zh::Region::of_language(
            config
                .profiles
                .iter()
                .find(|p| p.is_default)
                .and_then(|p| p.language_hint.as_deref()),
        ),
        ..learn::Options::default()
    };
    let pairs = learn::pairs_of(&rows);
    Ok(learn::suggest(&pairs, &opts)
        .into_iter()
        .filter(|s| s.counts.user() > 0)
        // A term already in the dictionary is only worth showing for the
        // aliases it would bring; without any there is nothing to add.
        .filter(|s| !s.known_term || !learn::aliases_to_add(s).is_empty())
        .map(|s| SuggestionDto {
            add_aliases: learn::aliases_to_add(&s),
            aliases: s
                .aliases
                .iter()
                .map(|a| AliasDto {
                    text: a.text.clone(),
                    rows: a.counts.user(),
                })
                .collect(),
            rows: s.counts.user(),
            sound_alike: s
                .aliases
                .iter()
                .all(|a| a.kind == learn::AliasKind::Misheard),
            term: s.term,
            excerpts: s.excerpts,
            known_term: s.known_term,
            near_term: s.near_term,
            form_only: s.form_only,
        })
        .collect())
}

#[tauri::command]
pub fn dictionary_suggestions(state: State<'_, AppState>) -> CmdResult<Vec<SuggestionDto>> {
    suggestions(&engine(&state)?)
}

/// The keys the Dictionary page sorts and groups a term by: the stroke
/// count, the Bopomofo initial and the pinyin initial of its first Han
/// character, in the default profile's regional reading. All None for a
/// term that does not start with a Han character.
#[derive(Debug, Clone, Serialize)]
pub struct IndexDto {
    pub strokes: Option<u16>,
    pub zhuyin: Option<String>,
    pub pinyin: Option<String>,
}

fn region_of(config: &Config) -> learn::zh::Region {
    learn::zh::Region::of_language(
        config
            .profiles
            .iter()
            .find(|p| p.is_default)
            .and_then(|p| p.language_hint.as_deref()),
    )
}

/// How often a term's aliases were replaced, and when last (unix seconds).
#[derive(Debug, Clone, Serialize)]
pub struct HitsDto {
    pub term: String,
    pub count: u64,
    pub last: i64,
}

#[tauri::command]
pub fn dictionary_hits(state: State<'_, AppState>) -> CmdResult<Vec<HitsDto>> {
    Ok(engine(&state)?
        .history()
        .dictionary_hits()?
        .into_iter()
        .map(|h| HitsDto {
            term: h.term,
            count: h.count,
            last: h.last_ts,
        })
        .collect())
}

#[tauri::command]
pub fn dictionary_index(
    state: State<'_, AppState>,
    terms: Vec<String>,
) -> CmdResult<Vec<IndexDto>> {
    let config = engine(&state)?.config().current();
    let region = region_of(&config);
    Ok(terms
        .iter()
        .map(|t| match t.trim().chars().next() {
            Some(c) if learn::zh::is_chinese(&c.to_string()) => IndexDto {
                strokes: learn::zh::strokes(c, region),
                zhuyin: learn::zh::zhuyin_initial(c, region).map(str::to_string),
                pinyin: learn::zh::pinyin_initial(c, region).map(|l| l.to_string()),
            },
            _ => IndexDto {
                strokes: None,
                zhuyin: None,
                pinyin: None,
            },
        })
        .collect())
}

/// Up to `limit` dictations whose final text holds the term, as excerpts
/// with the term between 【 and 】, newest first.
#[tauri::command]
pub fn dictionary_examples(
    state: State<'_, AppState>,
    term: String,
    limit: usize,
) -> CmdResult<Vec<String>> {
    let term = term.trim().to_string();
    if term.is_empty() {
        return Ok(Vec::new());
    }
    let rows = engine(&state)?
        .history()
        .list(limit.min(10), Some(&term), false)?;
    Ok(rows
        .iter()
        .filter_map(|r| {
            let text = r.final_text.replace(['\n', '\r'], " ");
            let at = text.find(&term)?;
            let chars: Vec<char> = text.chars().collect();
            let start_c = text[..at].chars().count();
            let end_c = start_c + term.chars().count();
            let lo = start_c.saturating_sub(18);
            let hi = (end_c + 18).min(chars.len());
            let mut out = String::new();
            if lo > 0 {
                out.push('…');
            }
            out.extend(&chars[lo..start_c]);
            out.push('【');
            out.extend(&chars[start_c..end_c]);
            out.push('】');
            out.extend(&chars[end_c..hi]);
            if hi < chars.len() {
                out.push('…');
            }
            Some(out)
        })
        .collect())
}
