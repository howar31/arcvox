//! Fixed text rules applied after the rewrite: the parts of the writing
//! that need no judgement and that a model gets wrong often enough to be
//! worth settling by hand. Each rule only adds or removes what it names.

use serde::{Deserialize, Serialize};

/// Which rules run; every field is a switch of the profile's rules step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TidyRules {
    /// A full stop that ends the whole text is removed.
    pub trailing_stop: bool,
    /// A text ending in 嗎 or 呢 with no mark after it gets a question mark.
    pub question_mark: bool,
    /// A full stop right after 嗎 or 呢 anywhere in the text becomes a
    /// question mark.
    pub question_mark_inside: bool,
    /// A space goes between a CJK character and an ASCII letter or digit.
    pub spacing: bool,
    /// An ASCII , . ? ! : ; right after a CJK character becomes full-width.
    pub punctuation_width: bool,
}

impl TidyRules {
    pub const NONE: TidyRules = TidyRules {
        trailing_stop: false,
        question_mark: false,
        question_mark_inside: false,
        spacing: false,
        punctuation_width: false,
    };
}

/// What a pass changed, for the stage record.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TidyCounts {
    pub widened: u32,
    pub spaces: u32,
    pub stop_removed: bool,
    pub question_added: bool,
    /// Full stops after 嗎 or 呢 inside the text turned into question marks.
    pub questions_inside: u32,
}

/// Runs the enabled rules in a fixed order: width, spacing, the question
/// marks inside, the trailing stop, then the question mark at the end (so
/// 嗎。 ends as 嗎？).
pub fn apply(text: &str, rules: TidyRules) -> (String, TidyCounts) {
    let mut counts = TidyCounts::default();
    let mut text = text.to_string();
    if rules.punctuation_width {
        let (t, n) = widen_marks(&text);
        text = t;
        counts.widened = n;
    }
    if rules.spacing {
        let (t, n) = space_cjk_ascii(&text);
        text = t;
        counts.spaces = n;
    }
    if rules.question_mark_inside {
        let (t, n) = question_marks_inside(&text);
        text = t;
        counts.questions_inside = n;
    }
    if rules.trailing_stop {
        let (t, removed) = remove_trailing_stop(&text);
        text = t;
        counts.stop_removed = removed;
    }
    if rules.question_mark {
        let (t, added) = add_question_mark(&text);
        text = t;
        counts.question_added = added;
    }
    (text, counts)
}

/// Han, kana and Hangul: the scripts written without spaces.
fn is_cjk(c: char) -> bool {
    matches!(c,
        '\u{3400}'..='\u{4DBF}' | '\u{4E00}'..='\u{9FFF}' | '\u{F900}'..='\u{FAFF}'
        | '\u{3040}'..='\u{30FF}' | '\u{AC00}'..='\u{D7AF}')
}

fn widen_marks(text: &str) -> (String, u32) {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut n = 0;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let prev_cjk = i > 0 && is_cjk(chars[i - 1]);
        let next = chars.get(i + 1).copied();
        let wide = match c {
            ',' => Some('，'),
            '?' => Some('？'),
            '!' => Some('！'),
            ':' => Some('：'),
            ';' => Some('；'),
            // A dot between digits or letters is a decimal or a domain.
            '.' if !next.is_some_and(|n| n.is_ascii_alphanumeric()) => Some('。'),
            _ => None,
        };
        match wide {
            Some(w) if prev_cjk => {
                out.push(w);
                n += 1;
                // The space a half-width mark was followed by goes with it.
                if next == Some(' ') && chars.get(i + 2).copied().is_some_and(is_cjk) {
                    i += 1;
                }
            }
            _ => out.push(c),
        }
        i += 1;
    }
    (out, n)
}

fn space_cjk_ascii(text: &str) -> (String, u32) {
    let mut out = String::with_capacity(text.len() + 8);
    let mut n = 0;
    let mut prev: Option<char> = None;
    for c in text.chars() {
        if let Some(p) = prev {
            let boundary = (is_cjk(p) && c.is_ascii_alphanumeric())
                || (p.is_ascii_alphanumeric() && is_cjk(c));
            if boundary {
                out.push(' ');
                n += 1;
            }
        }
        out.push(c);
        prev = Some(c);
    }
    (out, n)
}

fn question_marks_inside(text: &str) -> (String, u32) {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut n = 0;
    for (i, &c) in chars.iter().enumerate() {
        if c == '。' && i > 0 && matches!(chars[i - 1], '嗎' | '呢') && i + 1 < chars.len() {
            out.push('？');
            n += 1;
        } else {
            out.push(c);
        }
    }
    (out, n)
}

fn remove_trailing_stop(text: &str) -> (String, bool) {
    let trimmed = text.trim_end();
    match trimmed.strip_suffix('。') {
        Some(rest) => (rest.to_string(), true),
        None => (text.to_string(), false),
    }
}

fn add_question_mark(text: &str) -> (String, bool) {
    let trimmed = text.trim_end();
    if trimmed.ends_with('嗎') || trimmed.ends_with('呢') {
        (format!("{trimmed}？"), true)
    } else {
        (text.to_string(), false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: TidyRules = TidyRules {
        trailing_stop: true,
        question_mark: true,
        question_mark_inside: true,
        spacing: true,
        punctuation_width: true,
    };

    #[test]
    fn half_width_marks_after_cjk_become_full_width() {
        let (t, n) = widen_marks("你好, 世界? 版本1.5和a.b的網址.");
        assert_eq!(t, "你好，世界？版本1.5和a.b的網址。");
        assert_eq!(n, 3);
    }

    #[test]
    fn spaces_go_between_cjk_and_ascii_and_are_never_removed() {
        let (t, n) = space_cjk_ascii("第2次用iPhone15和 Mac 測試，呢");
        assert_eq!(t, "第 2 次用 iPhone15 和 Mac 測試，呢");
        assert_eq!(n, 4);
        assert_eq!(space_cjk_ascii("已有 空白 a").0, "已有 空白 a");
    }

    #[test]
    fn the_trailing_stop_goes_and_a_question_ends_with_its_mark() {
        assert_eq!(apply("好的。", ALL).0, "好的");
        assert_eq!(apply("中間的。句號留著。", ALL).0, "中間的。句號留著");
        assert_eq!(apply("可以嗎。", ALL).0, "可以嗎？");
        assert_eq!(apply("可以嗎？", ALL).0, "可以嗎？");
        assert_eq!(apply("好吧", ALL).0, "好吧");
        let (_, counts) = apply("可以嗎。", ALL);
        assert!(counts.stop_removed && counts.question_added);
    }

    #[test]
    fn a_full_stop_after_a_question_particle_inside_the_text_becomes_a_question_mark() {
        let (t, counts) = apply("可以嗎。我想知道呢。好的。", ALL);
        assert_eq!(t, "可以嗎？我想知道呢？好的");
        assert_eq!(counts.questions_inside, 2);
        // A comma after the particle is left alone: 呢 also marks a topic.
        assert_eq!(apply("之後呢，再說", ALL).0, "之後呢，再說");
    }

    #[test]
    fn switched_off_rules_leave_the_text_alone() {
        let text = "你好, 第2次。可以嗎";
        assert_eq!(
            apply(text, TidyRules::NONE),
            (text.to_string(), TidyCounts::default())
        );
    }
}
