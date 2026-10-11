//! Chinese script conversion with the OpenCC dictionaries: every
//! configuration the engine offers (Simplified, Traditional, Taiwan, Hong
//! Kong and Japanese forms, with or without phrasing), `s2tw` by default.
//! Pure Rust via opencc-fmmseg; no C dependency.

use async_trait::async_trait;

use crate::steps::{PipelineContext, Step, StepError, StepInput, StepOutput};
use crate::types::{ErrorCode, StepName};

/// Every configuration the engine offers, in the order the client lists them.
pub const SUPPORTED: &[&str] = &[
    "s2tw", "s2twp", "t2tw", "t2twp", "s2hk", "t2hk", "s2t", "tw2t", "tw2tp", "hk2t", "jp2t",
    "t2s", "tw2s", "tw2sp", "hk2s", "t2jp",
];

/// The configurations whose punctuation parameter the engine reads: the
/// regional ones ignore it.
pub const WITH_PUNCTUATION: &[&str] = &[
    "s2t", "s2tw", "s2twp", "s2hk", "t2s", "tw2s", "tw2sp", "hk2s",
];

pub struct OpenCcStep {
    config: String,
    punctuation: bool,
    engine: opencc_fmmseg::OpenCC,
}

// `opencc_fmmseg::OpenCC` does not implement `Debug`, so this is implemented
// manually (only the config name is printed) instead of deriving it. Needed
// because `Result::unwrap_err` requires the `Ok` type to be `Debug`.
impl std::fmt::Debug for OpenCcStep {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenCcStep")
            .field("config", &self.config)
            .finish()
    }
}

impl OpenCcStep {
    pub fn new(config: &str, punctuation: bool) -> Result<Self, StepError> {
        if !SUPPORTED.contains(&config) {
            return Err(StepError::fatal(
                ErrorCode::InvalidConfig,
                format!("unknown OpenCC config `{config}`"),
            ));
        }
        Ok(Self {
            config: config.to_string(),
            punctuation: punctuation && WITH_PUNCTUATION.contains(&config),
            engine: opencc_fmmseg::OpenCC::new(),
        })
    }

    pub fn convert(&self, text: &str) -> String {
        self.engine.convert(text, &self.config, self.punctuation)
    }
}

#[async_trait]
impl Step for OpenCcStep {
    fn name(&self) -> StepName {
        StepName::OpenCc
    }

    async fn run(&self, input: StepInput, _ctx: &PipelineContext) -> Result<StepOutput, StepError> {
        let text = self.convert(&input.text);
        Ok(StepOutput {
            detail: serde_json::json!({
                "config": self.config,
                "punctuation": self.punctuation,
                "changed_chars": changed_chars(&input.text, &text),
            }),
            text,
            language: None,
            ..Default::default()
        })
    }
}

/// How many characters the conversion changed: the differing positions when
/// the length is kept, else every character of the result that the input
/// does not contain.
fn changed_chars(from: &str, to: &str) -> usize {
    let a: Vec<char> = from.chars().collect();
    let b: Vec<char> = to.chars().collect();
    if a.len() == b.len() {
        a.iter().zip(&b).filter(|(x, y)| x != y).count()
    } else {
        b.iter().filter(|c| !a.contains(c)).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::steps::test_context;

    #[test]
    fn s2twp_converts_characters_and_taiwan_phrasing() {
        let step = OpenCcStep::new("s2twp", false).unwrap();
        assert_eq!(step.convert("汉字"), "漢字");
        // Taiwan phrasing: 软件 -> 軟體, 信息 -> 資訊.
        assert_eq!(step.convert("软件信息"), "軟體資訊");
    }

    #[test]
    fn traditional_input_is_unchanged() {
        let step = OpenCcStep::new("s2twp", false).unwrap();
        assert_eq!(
            step.convert("臺灣正體中文，English and 日本語。"),
            "臺灣正體中文，English and 日本語。"
        );
    }

    #[test]
    fn every_engine_configuration_is_accepted() {
        for c in SUPPORTED {
            OpenCcStep::new(c, false).unwrap();
        }
        assert_eq!(
            OpenCcStep::new("t2jp", false).unwrap().convert("臺灣"),
            "台湾"
        );
        assert_eq!(
            OpenCcStep::new("t2hk", false).unwrap().convert("衛生"),
            "衞生"
        );
    }

    #[test]
    fn punctuation_follows_the_script_only_where_the_engine_reads_it() {
        assert_eq!(
            OpenCcStep::new("s2tw", true).unwrap().convert("“你好”"),
            "「你好」"
        );
        assert_eq!(
            OpenCcStep::new("s2tw", false).unwrap().convert("“你好”"),
            "“你好”"
        );
        assert!(!OpenCcStep::new("t2hk", true).unwrap().punctuation);
    }

    #[test]
    fn unknown_config_is_rejected() {
        let err = OpenCcStep::new("nope", false).unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidConfig);
        assert!(!err.recoverable);
    }

    #[tokio::test]
    async fn runs_as_step() {
        let step = OpenCcStep::new("s2twp", false).unwrap();
        let out = step
            .run(
                StepInput {
                    text: "网络".into(),
                    audio: None,
                },
                &test_context(),
            )
            .await
            .unwrap();
        assert_eq!(out.text, "網路");
        assert_eq!(step.name(), StepName::OpenCc);
    }
}
