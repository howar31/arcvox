//! Maps BCP 47 language hints to whisper.cpp language codes.

/// Returns the whisper.cpp code for a language hint, or `None` for automatic detection.
pub fn whisper_code(hint: Option<&str>) -> Option<&'static str> {
    let primary = hint?.split(['-', '_']).next()?.to_ascii_lowercase();
    match primary.as_str() {
        "zh" => Some("zh"),
        "en" => Some("en"),
        "ja" => Some("ja"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_hints_to_whisper_codes() {
        assert_eq!(whisper_code(Some("zh-TW")), Some("zh"));
        assert_eq!(whisper_code(Some("zh")), Some("zh"));
        assert_eq!(whisper_code(Some("en-US")), Some("en"));
        assert_eq!(whisper_code(Some("ja-JP")), Some("ja"));
        assert_eq!(whisper_code(Some("auto")), None);
        assert_eq!(whisper_code(None), None);
        assert_eq!(whisper_code(Some("xx")), None);
    }
}
