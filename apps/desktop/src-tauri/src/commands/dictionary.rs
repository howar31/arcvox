use arcvox_core::config::{Config, DictionaryEntry};
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
    dismissed: Vec<String>,
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
    dismissed: Vec<String>,
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
    /// The aliases accepting brings along; empty for a term that is a
    /// recognition hint only.
    pub add_aliases: Vec<String>,
    pub form_only: bool,
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
        ..learn::Options::default()
    };
    let pairs = learn::pairs_of(&rows);
    Ok(learn::suggest(&pairs, &opts)
        .into_iter()
        .filter(|s| s.counts.user() > 0)
        .filter(|s| !config.dictionary_dismissed.iter().any(|d| d == &s.term))
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
            term: s.term,
            excerpts: s.excerpts,
            known_term: s.known_term,
            form_only: s.form_only,
        })
        .collect())
}

#[tauri::command]
pub fn dictionary_suggestions(state: State<'_, AppState>) -> CmdResult<Vec<SuggestionDto>> {
    suggestions(&engine(&state)?)
}
