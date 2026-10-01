//! One dictionary, three consumers: ASR hotword prompt, LLM term list,
//! and post-processing alias replacement.

use crate::config::DictionaryEntry;

/// A term spanning several lines is a snippet: its aliases are the spoken
/// keywords and the whole term is what gets inserted.
pub fn is_snippet(entry: &DictionaryEntry) -> bool {
    entry.term.trim().contains('\n')
}

/// Hinted items, comma-joined, cut at whole items to fit `budget_bytes`.
/// A snippet contributes its non-empty aliases; any other entry its term.
pub fn asr_hint(entries: &[DictionaryEntry], budget_bytes: usize) -> Option<String> {
    let items = entries
        .iter()
        .filter(|e| e.asr_hint)
        .flat_map(|e| -> Vec<&str> {
            if is_snippet(e) {
                e.aliases
                    .iter()
                    .map(String::as_str)
                    .filter(|a| !a.is_empty())
                    .collect()
            } else {
                vec![e.term.as_str()]
            }
        });
    let mut out = String::new();
    for item in items {
        let candidate = if out.is_empty() {
            item.to_string()
        } else {
            format!("{out}, {item}")
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
    entries
        .iter()
        .filter(|e| !is_snippet(e))
        .map(|e| e.term.clone())
        .collect()
}

/// How many replacements a pass made: term corrections and snippet insertions.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AliasCounts {
    pub fixes: u32,
    pub snippets: u32,
}

/// Replaces every alias with its term, matching against the original text
/// in a single pass so replacement output is never re-scanned. At each
/// position the longest matching alias wins, so `台北市` wins over `台北`
/// when both are aliases.
pub fn apply_aliases(entries: &[DictionaryEntry], text: &str) -> String {
    apply_aliases_counted(entries, text).0
}

/// `apply_aliases` plus the number of replacements, split by entry kind.
pub fn apply_aliases_counted(entries: &[DictionaryEntry], text: &str) -> (String, AliasCounts) {
    let mut pairs: Vec<(&str, &str, bool)> = entries
        .iter()
        .flat_map(|e| {
            let snippet = is_snippet(e);
            e.aliases
                .iter()
                .map(move |a| (a.as_str(), e.term.as_str(), snippet))
        })
        .filter(|(alias, _, _)| !alias.is_empty())
        .collect();
    pairs.sort_by_key(|(alias, _, _)| std::cmp::Reverse(alias.len()));

    let mut out = String::new();
    let mut counts = AliasCounts::default();
    let mut i = 0;
    while i < text.len() {
        let rest = &text[i..];
        if let Some(&(alias, term, snippet)) =
            pairs.iter().find(|(alias, _, _)| rest.starts_with(alias))
        {
            out.push_str(term);
            if snippet {
                counts.snippets += 1;
            } else {
                counts.fixes += 1;
            }
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
    (out, counts)
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
    fn snippet_hint_uses_aliases_and_llm_terms_skip_it() {
        let d = vec![
            entry("Arcvox", &[], true),
            entry("Best regards,\nThe team", &["簽名檔", "signature"], true),
            entry("line one\nline two", &["片段"], false),
        ];
        assert!(is_snippet(&d[1]));
        assert!(!is_snippet(&d[0]));
        assert_eq!(
            asr_hint(&d, 1000).as_deref(),
            Some("Arcvox, 簽名檔, signature")
        );
        assert_eq!(llm_terms(&d), vec!["Arcvox"]);
        assert_eq!(
            apply_aliases(&d, "附上簽名檔"),
            "附上Best regards,\nThe team"
        );
    }

    #[test]
    fn surrounding_newlines_do_not_make_a_snippet() {
        assert!(!is_snippet(&entry("Arcvox\n", &[], true)));
        assert!(!is_snippet(&entry("\n Arcvox \n", &[], true)));
        assert!(is_snippet(&entry("\nA\nB", &["k"], true)));
    }

    #[test]
    fn replacement_output_is_not_rescanned() {
        let d = vec![entry("XY", &["ABC"], true), entry("Z", &["XY"], true)];
        assert_eq!(apply_aliases(&d, "ABC"), "XY");
        assert_eq!(apply_aliases(&d, "XY ABC"), "Z XY");
    }

    #[test]
    fn counted_replacement_separates_terms_from_snippets() {
        let d = vec![
            entry("Arcvox", &["雅客", "語音"], false),
            entry("Best regards,\nThe team", &["結尾敬語"], false),
        ];
        let (text, counts) = apply_aliases_counted(&d, "雅客 和 語音 結尾敬語");
        assert_eq!(text, "Arcvox 和 Arcvox Best regards,\nThe team");
        assert_eq!(
            counts,
            AliasCounts {
                fixes: 2,
                snippets: 1
            }
        );
    }

    #[test]
    fn counted_replacement_without_a_match_counts_nothing() {
        let d = vec![entry("Arcvox", &["雅客"], false)];
        assert_eq!(
            apply_aliases_counted(&d, "hello"),
            ("hello".to_string(), AliasCounts::default())
        );
    }
}
