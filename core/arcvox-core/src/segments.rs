//! What the speech engine knows about a transcript beyond its text: the
//! segments it decoded, with their times, their no-speech probability and
//! each token's probability. Kept in the stage record of every dictation for
//! later analysis; nothing reads it yet.

use crate::types::TranscriptSegment;

/// Joins the segments' texts into one trimmed transcript and returns, for
/// each segment, the character offset where it starts inside that text.
pub fn join_segments(segments: &[TranscriptSegment]) -> (String, Vec<usize>) {
    let mut joined = String::new();
    let mut starts = Vec::with_capacity(segments.len());
    for seg in segments {
        starts.push(joined.chars().count());
        joined.push_str(&seg.text);
    }
    let leading = joined.chars().take_while(|c| c.is_whitespace()).count();
    let text = joined.trim().to_string();
    let starts = starts
        .into_iter()
        .map(|s| s.saturating_sub(leading))
        .collect();
    (text, starts)
}

/// The segments as the stage record stores them: compact, one object per
/// segment, tokens as `[text, probability]` pairs.
pub fn segment_records(segments: &[TranscriptSegment]) -> serde_json::Value {
    let (_, starts) = join_segments(segments);
    let rows: Vec<serde_json::Value> = segments
        .iter()
        .zip(starts)
        .map(|(s, at)| {
            serde_json::json!({
                "at": at,
                "start_ms": s.start_ms,
                "end_ms": s.end_ms,
                "no_speech": round3(s.no_speech),
                "tokens": s.tokens.iter().map(|t| serde_json::json!([t.text, round3(t.p)])).collect::<Vec<_>>(),
            })
        })
        .collect();
    serde_json::Value::Array(rows)
}

fn round3(x: f32) -> f64 {
    (f64::from(x) * 1000.0).round() / 1000.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::TranscriptToken;

    fn seg(text: &str, start_ms: u32, end_ms: u32) -> TranscriptSegment {
        TranscriptSegment {
            text: text.into(),
            start_ms,
            end_ms,
            no_speech: 0.01234,
            tokens: vec![TranscriptToken {
                text: text.trim().into(),
                p: 0.98765,
            }],
        }
    }

    #[test]
    fn segment_starts_are_offsets_in_the_trimmed_text() {
        let segs = [
            seg(" 你好", 0, 500),
            seg("世界", 500, 900),
            seg(" 再見 ", 900, 1300),
        ];
        let (text, starts) = join_segments(&segs);
        assert_eq!(text, "你好世界 再見");
        assert_eq!(starts, vec![0, 2, 4]);
    }

    #[test]
    fn records_are_compact_and_rounded() {
        let v = segment_records(&[seg("你好", 0, 500)]);
        assert_eq!(
            v,
            serde_json::json!([{
                "at": 0, "start_ms": 0, "end_ms": 500, "no_speech": 0.012,
                "tokens": [["你好", 0.988]]
            }])
        );
    }
}
