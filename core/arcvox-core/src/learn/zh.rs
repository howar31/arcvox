//! Chinese readings and words, for telling a slip of the ear from a change
//! of mind: a heard text that is no word at all was misheard; one that is
//! a word and sounds like the kept text was misheard too, but replacing it
//! everywhere would hit the places it was meant; one that is a word and
//! sounds unlike the kept text was a change of wording.
//!
//! The readings come from the Unihan database (kMandarin for the customary
//! reading of each region, kHanyuPinyin and kTGHZ2013 for the rest), the
//! words from the jieba and jieba-tw dictionaries; both tables are built
//! by `scripts/zh-tables.py`.

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

const READINGS: &str = include_str!("../../data/zh-readings.tsv");
const WORDS: &str = include_str!("../../data/zh-words.txt");

/// Whose customary readings apply: a character read one way in Taiwan
/// and another on the mainland (垃圾, 企) takes the region's reading
/// first; its other dictionary readings count too.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Region {
    #[default]
    Taiwan,
    Mainland,
}

impl Region {
    /// The region of a profile's main language: zh-CN, zh-Hans and zh-SG
    /// are the mainland's, everything else (zh-TW, zh-Hant, none) Taiwan's.
    pub fn of_language(hint: Option<&str>) -> Region {
        let hint = hint.unwrap_or("").to_ascii_lowercase();
        if hint == "zh-cn" || hint == "zh-sg" || hint.starts_with("zh-hans") {
            Region::Mainland
        } else {
            Region::Taiwan
        }
    }
}

struct Readings {
    taiwan: &'static str,
    mainland: &'static str,
    others: Vec<&'static str>,
    /// Total strokes, as counted in Taiwan and on the mainland.
    strokes: (u16, u16),
}

fn table() -> &'static HashMap<char, Readings> {
    static TABLE: OnceLock<HashMap<char, Readings>> = OnceLock::new();
    TABLE.get_or_init(|| {
        READINGS
            .lines()
            .filter_map(|line| {
                let mut f = line.split('\t');
                let c = f.next()?.chars().next()?;
                let taiwan = f.next()?;
                let mainland = f.next()?;
                let others = f
                    .next()
                    .map(|s| s.split(',').filter(|s| !s.is_empty()).collect())
                    .unwrap_or_default();
                let mut count = || f.next().and_then(|v| v.parse().ok()).unwrap_or(0u16);
                let strokes = (count(), count());
                Some((
                    c,
                    Readings {
                        taiwan,
                        mainland,
                        others,
                        strokes,
                    },
                ))
            })
            .collect()
    })
}

fn words() -> &'static HashSet<&'static str> {
    static WORDS_SET: OnceLock<HashSet<&'static str>> = OnceLock::new();
    WORDS_SET.get_or_init(|| WORDS.lines().filter(|w| !w.is_empty()).collect())
}

/// The customary reading of a character in the region, toneless pinyin;
/// None for a character outside the table.
pub fn reading(c: char, region: Region) -> Option<&'static str> {
    let r = table().get(&c)?;
    Some(match region {
        Region::Taiwan => r.taiwan,
        Region::Mainland => r.mainland,
    })
}

/// How many strokes a character takes in the region; None when unknown.
pub fn strokes(c: char, region: Region) -> Option<u16> {
    let r = table().get(&c)?;
    let n = match region {
        Region::Taiwan => r.strokes.0,
        Region::Mainland => r.strokes.1,
    };
    (n > 0).then_some(n)
}

/// The first letter of a character's reading, for an A to Z index.
pub fn pinyin_initial(c: char, region: Region) -> Option<char> {
    reading(c, region)?
        .chars()
        .next()
        .map(|l| l.to_ascii_uppercase())
}

/// The first Bopomofo symbol of a character's reading: its initial, or
/// the medial or final of a syllable without one, for a ㄅ to ㄦ index.
pub fn zhuyin_initial(c: char, region: Region) -> Option<&'static str> {
    let r = reading(c, region)?;
    const INITIALS: [(&str, &str); 21] = [
        ("zh", "ㄓ"),
        ("ch", "ㄔ"),
        ("sh", "ㄕ"),
        ("b", "ㄅ"),
        ("p", "ㄆ"),
        ("m", "ㄇ"),
        ("f", "ㄈ"),
        ("d", "ㄉ"),
        ("t", "ㄊ"),
        ("n", "ㄋ"),
        ("l", "ㄌ"),
        ("g", "ㄍ"),
        ("k", "ㄎ"),
        ("h", "ㄏ"),
        ("j", "ㄐ"),
        ("q", "ㄑ"),
        ("x", "ㄒ"),
        ("r", "ㄖ"),
        ("z", "ㄗ"),
        ("c", "ㄘ"),
        ("s", "ㄙ"),
    ];
    if let Some((_, z)) = INITIALS.iter().find(|(p, _)| r.starts_with(p)) {
        return Some(z);
    }
    // Syllables without an initial: a medial (yi, wu, yu) or a final.
    const BARE: [(&str, &str); 15] = [
        ("yu", "ㄩ"),
        ("y", "ㄧ"),
        ("w", "ㄨ"),
        ("ai", "ㄞ"),
        ("ei", "ㄟ"),
        ("ao", "ㄠ"),
        ("ou", "ㄡ"),
        ("ang", "ㄤ"),
        ("eng", "ㄥ"),
        ("an", "ㄢ"),
        ("en", "ㄣ"),
        ("er", "ㄦ"),
        ("a", "ㄚ"),
        ("o", "ㄛ"),
        ("e", "ㄜ"),
    ];
    BARE.iter().find(|(p, _)| r.starts_with(p)).map(|(_, z)| *z)
}

/// Whether every character is a CJK ideograph of the basic block.
pub fn is_chinese(text: &str) -> bool {
    !text.is_empty() && text.chars().all(|c| ('\u{4E00}'..='\u{9FFF}').contains(&c))
}

/// Whether the dictionaries list `text` as a word.
pub fn is_word(text: &str) -> bool {
    words().contains(text)
}

/// The readings of `c` in `region`: the customary one first, then the
/// dictionary's others. Empty for a character the table does not know.
fn readings(c: char, region: Region) -> Vec<&'static str> {
    let Some(r) = table().get(&c) else {
        return Vec::new();
    };
    let first = match region {
        Region::Taiwan => r.taiwan,
        Region::Mainland => r.mainland,
    };
    let mut out = vec![first];
    for o in &r.others {
        if !out.contains(o) {
            out.push(o);
        }
    }
    out
}

/// A toneless pinyin syllable as its initial and final.
fn parts(syllable: &str) -> (&str, &str) {
    const INITIALS: [&str; 23] = [
        "zh", "ch", "sh", "b", "p", "m", "f", "d", "t", "n", "l", "g", "k", "h", "j", "q", "x",
        "r", "z", "c", "s", "y", "w",
    ];
    for i in INITIALS {
        if let Some(rest) = syllable.strip_prefix(i) {
            return (i, rest);
        }
    }
    ("", syllable)
}

/// Whether two syllables sound alike to a Taiwan Mandarin ear: equal, or
/// apart only by a retroflex (ㄓ/ㄗ, ㄔ/ㄘ, ㄕ/ㄙ), ㄋ/ㄌ, ㄈ/ㄏ, or a nasal
/// ending (ㄣ/ㄥ, ㄢ/ㄤ, ㄧㄣ/ㄧㄥ, ㄧㄢ/ㄧㄤ, ㄨㄢ/ㄨㄤ).
fn syllables_alike(a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    let (ia, fa) = parts(a);
    let (ib, fb) = parts(b);
    const INITIAL_PAIRS: [(&str, &str); 5] = [
        ("zh", "z"),
        ("ch", "c"),
        ("sh", "s"),
        ("n", "l"),
        ("f", "h"),
    ];
    const FINAL_PAIRS: [(&str, &str); 5] = [
        ("en", "eng"),
        ("an", "ang"),
        ("in", "ing"),
        ("ian", "iang"),
        ("uan", "uang"),
    ];
    let paired = |x: &str, y: &str, pairs: &[(&str, &str)]| {
        x == y
            || pairs
                .iter()
                .any(|(p, q)| (x == *p && y == *q) || (x == *q && y == *p))
    };
    paired(ia, ib, &INITIAL_PAIRS) && paired(fa, fb, &FINAL_PAIRS)
}

/// Whether some reading of `a` sounds like some reading of `b`.
fn chars_alike(a: char, b: char, region: Region) -> bool {
    if a == b {
        return true;
    }
    let ra = readings(a, region);
    let rb = readings(b, region);
    ra.iter().any(|x| rb.iter().any(|y| syllables_alike(x, y)))
}

/// Whether two Chinese texts sound alike: character by character, every
/// pair alike, with at most one character dropped or added on one side.
pub fn sound_alike(a: &str, b: &str, region: Region) -> bool {
    let a: Vec<char> = a.chars().filter(|c| !c.is_whitespace()).collect();
    let b: Vec<char> = b.chars().filter(|c| !c.is_whitespace()).collect();
    if a.is_empty() || b.is_empty() {
        return false;
    }
    let all_alike = |x: &[char], y: &[char]| {
        x.len() == y.len() && x.iter().zip(y).all(|(p, q)| chars_alike(*p, *q, region))
    };
    if a.len() == b.len() {
        return all_alike(&a, &b);
    }
    let (longer, shorter) = if a.len() > b.len() {
        (&a, &b)
    } else {
        (&b, &a)
    };
    if longer.len() != shorter.len() + 1 {
        return false;
    }
    (0..longer.len()).any(|skip| {
        let rest: Vec<char> = longer
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != skip)
            .map(|(_, c)| *c)
            .collect();
        all_alike(&rest, shorter)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_region_follows_the_language_hint() {
        assert_eq!(Region::of_language(Some("zh-TW")), Region::Taiwan);
        assert_eq!(Region::of_language(Some("zh-CN")), Region::Mainland);
        assert_eq!(Region::of_language(Some("zh-Hans-CN")), Region::Mainland);
        assert_eq!(Region::of_language(None), Region::Taiwan);
        assert_eq!(Region::of_language(Some("en")), Region::Taiwan);
    }

    #[test]
    fn words_and_non_words() {
        assert!(is_word("風暴"));
        assert!(is_word("統一"));
        assert!(is_word("其他"));
        assert!(!is_word("寒碾"));
        assert!(!is_word("帶出裡"));
        assert!(is_chinese("權限"));
        assert!(!is_chinese("Zephyr"));
        assert!(!is_chinese("v2 版"));
    }

    #[test]
    fn syllables_with_the_usual_confusions_sound_alike() {
        assert!(syllables_alike("quan", "quan"));
        assert!(syllables_alike("zhi", "zi"));
        assert!(syllables_alike("fen", "feng"));
        assert!(syllables_alike("nan", "lan"));
        assert!(syllables_alike("fu", "hu"));
        assert!(!syllables_alike("bao", "feng"));
        assert!(!syllables_alike("xian", "xie"));
    }

    #[test]
    fn misheard_words_sound_alike_and_rewordings_do_not() {
        assert!(sound_alike("全線", "權限", Region::Taiwan));
        assert!(sound_alike("風暴", "封包", Region::Taiwan));
        assert!(sound_alike("姿勢", "直式", Region::Taiwan));
        assert!(sound_alike("統一", "同意", Region::Taiwan));
        assert!(sound_alike("幾何", "稽核", Region::Taiwan));
        assert!(!sound_alike("別的", "其他", Region::Taiwan));
        assert!(!sound_alike("專案", "項目", Region::Taiwan));
        // One character dropped: 帶出裡 against 待處理 is three to three,
        // 實作 against 石頭人在 is too far apart.
        assert!(sound_alike("帶出裡", "待處理", Region::Taiwan));
        assert!(!sound_alike("實作", "石頭人在", Region::Taiwan));
    }

    #[test]
    fn a_heteronym_matches_through_any_of_its_readings() {
        // 行 is xing customarily and hang in 銀行.
        assert!(sound_alike("銀行", "銀航", Region::Taiwan));
        assert!(sound_alike("銀行", "銀行", Region::Mainland));
    }
}

#[cfg(test)]
mod index_tests {
    use super::*;

    #[test]
    fn strokes_and_initials_come_from_the_table() {
        assert_eq!(strokes('封', Region::Taiwan), Some(9));
        assert_eq!(strokes('權', Region::Taiwan), Some(21));
        assert_eq!(strokes('a', Region::Taiwan), None);
        assert_eq!(pinyin_initial('權', Region::Taiwan), Some('Q'));
        assert_eq!(zhuyin_initial('權', Region::Taiwan), Some("ㄑ"));
        assert_eq!(zhuyin_initial('知', Region::Taiwan), Some("ㄓ"));
        assert_eq!(zhuyin_initial('一', Region::Taiwan), Some("ㄧ"));
        assert_eq!(zhuyin_initial('安', Region::Taiwan), Some("ㄢ"));
        assert_eq!(zhuyin_initial('二', Region::Taiwan), Some("ㄦ"));
    }
}
