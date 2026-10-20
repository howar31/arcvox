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
/// when both are aliases. An alias edge that is an ASCII letter or digit
/// only matches at a word boundary, so `js` leaves `json` alone. ASCII
/// letters match in either case, so `slake` catches `Slake` too; the
/// term is written as the user wrote it.
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
        let before = text[..i].chars().next_back();
        if let Some(&(alias, term, snippet)) = pairs
            .iter()
            .find(|(alias, _, _)| starts_with_fold(rest, alias) && at_boundary(alias, before, rest))
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

/// Whether `text` begins with `prefix`, ASCII letters in either case.
/// Folding ASCII case keeps every byte length, so the match covers
/// exactly `prefix.len()` bytes of `text`.
fn starts_with_fold(text: &str, prefix: &str) -> bool {
    text.len() >= prefix.len()
        && text.is_char_boundary(prefix.len())
        && text.as_bytes()[..prefix.len()].eq_ignore_ascii_case(prefix.as_bytes())
}

/// False when the alias starts or ends with an ASCII letter or digit and
/// the neighbouring character in the text continues that word.
fn at_boundary(alias: &str, before: Option<char>, rest: &str) -> bool {
    let word = |c: char| c.is_ascii_alphanumeric();
    let starts_word = alias.chars().next().is_some_and(word);
    let ends_word = alias.chars().next_back().is_some_and(word);
    let after = rest[alias.len()..].chars().next();
    !(starts_word && before.is_some_and(word)) && !(ends_word && after.is_some_and(word))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_ascii_alias_matches_in_either_case_and_keeps_the_terms_writing() {
        let d = vec![
            entry("Slack", &["slake"], true),
            entry("權限", &["全線"], true),
        ];
        assert_eq!(
            apply_aliases(&d, "你看 Slake 和 SLAKE 的全線"),
            "你看 Slack 和 Slack 的權限"
        );
    }

    #[test]
    fn ascii_alias_needs_a_word_boundary() {
        let d = vec![entry("JavaScript", &["js"], false)];
        assert_eq!(apply_aliases(&d, "json and js"), "json and JavaScript");
        assert_eq!(apply_aliases(&d, "nodejs"), "nodejs");
        assert_eq!(apply_aliases(&d, "用js寫"), "用JavaScript寫");
        assert_eq!(apply_aliases(&d, "js, js."), "JavaScript, JavaScript.");
    }

    #[test]
    fn cjk_alias_matches_inside_running_text() {
        let d = vec![entry("語音", &["弧克"], false)];
        assert_eq!(apply_aliases(&d, "用弧克輸入法abc"), "用語音輸入法abc");
    }

    #[test]
    fn mixed_alias_checks_only_its_ascii_edge() {
        let d = vec![entry("C++ 語言", &["c加加"], false)];
        assert_eq!(apply_aliases(&d, "abc加加"), "abc加加");
        assert_eq!(apply_aliases(&d, "學c加加很久"), "學C++ 語言很久");
    }

    fn entry(term: &str, aliases: &[&str], hint: bool) -> DictionaryEntry {
        DictionaryEntry {
            term: term.into(),
            aliases: aliases.iter().map(|s| s.to_string()).collect(),
            asr_hint: hint,
            ..Default::default()
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
