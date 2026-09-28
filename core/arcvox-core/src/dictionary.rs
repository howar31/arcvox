//! One dictionary, three consumers: ASR hotword prompt, LLM term list,
//! and post-processing alias replacement.

use crate::config::DictionaryEntry;

/// Terms with `asr_hint`, comma-joined, cut at whole terms to fit `budget_bytes`.
pub fn asr_hint(entries: &[DictionaryEntry], budget_bytes: usize) -> Option<String> {
    let mut out = String::new();
    for e in entries.iter().filter(|e| e.asr_hint) {
        let candidate = if out.is_empty() {
            e.term.clone()
        } else {
            format!("{out}, {}", e.term)
        };
        if candidate.len() > budget_bytes {
            break;
        }
        out = candidate;
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

pub fn llm_terms(entries: &[DictionaryEntry]) -> Vec<String> {
    entries.iter().map(|e| e.term.clone()).collect()
}

/// Replaces every alias with its term, matching against the original text
/// in a single pass so replacement output is never re-scanned. At each
/// position the longest matching alias wins, so `台北市` wins over `台北`
/// when both are aliases.
pub fn apply_aliases(entries: &[DictionaryEntry], text: &str) -> String {
    let mut pairs: Vec<(&str, &str)> = entries
        .iter()
        .flat_map(|e| e.aliases.iter().map(move |a| (a.as_str(), e.term.as_str())))
        .filter(|(alias, _)| !alias.is_empty())
        .collect();
    pairs.sort_by_key(|(alias, _)| std::cmp::Reverse(alias.len()));

    let mut out = String::new();
    let mut i = 0;
    while i < text.len() {
        let rest = &text[i..];
        if let Some(&(alias, term)) = pairs.iter().find(|(alias, _)| rest.starts_with(alias)) {
            out.push_str(term);
            i += alias.len();
        } else {
            let ch = rest
                .chars()
                .next()
                .expect("i < text.len() implies a char remains");
            out.push(ch);
            i += ch.len_utf8();
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(term: &str, aliases: &[&str], hint: bool) -> DictionaryEntry {
        DictionaryEntry {
            term: term.into(),
            aliases: aliases.iter().map(|s| s.to_string()).collect(),
            asr_hint: hint,
        }
    }

    #[test]
    fn asr_hint_joins_only_hinted_terms() {
        let d = vec![
            entry("Arcvox", &[], true),
            entry("語音輸入法", &[], true),
            entry("hidden", &[], false),
        ];
        assert_eq!(asr_hint(&d, 1000).as_deref(), Some("Arcvox, 語音輸入法"));
    }

    #[test]
    fn asr_hint_respects_byte_budget_on_whole_terms() {
        let d = vec![entry("Arcvox", &[], true), entry("語音輸入法", &[], true)];
        // "Arcvox" is 6 bytes; adding ", 語音輸入法" (2 + 15 bytes) exceeds 20.
        assert_eq!(asr_hint(&d, 20).as_deref(), Some("Arcvox"));
        assert_eq!(asr_hint(&d, 3), None);
    }

    #[test]
    fn llm_terms_lists_everything() {
        let d = vec![entry("A", &[], true), entry("B", &[], false)];
        assert_eq!(llm_terms(&d), vec!["A", "B"]);
    }

    #[test]
    fn aliases_replace_longest_first_and_keep_multibyte_text_intact() {
        let d = vec![
            entry("Arcvox", &["雅客", "亞克"], true),
            entry("臺北市", &["台北市"], true),
            entry("臺北", &["台北"], true),
        ];
        assert_eq!(
            apply_aliases(&d, "我用雅客在台北市寫字"),
            "我用Arcvox在臺北市寫字"
        );
        assert_eq!(apply_aliases(&d, "亞克、台北"), "Arcvox、臺北");
    }

    #[test]
    fn empty_alias_is_ignored() {
        let d = vec![entry("X", &[""], true)];
        assert_eq!(apply_aliases(&d, "abc"), "abc");
    }

    #[test]
    fn replacement_output_is_not_rescanned() {
        let d = vec![entry("XY", &["ABC"], true), entry("Z", &["XY"], true)];
        assert_eq!(apply_aliases(&d, "ABC"), "XY");
        assert_eq!(apply_aliases(&d, "XY ABC"), "Z XY");
    }
}
