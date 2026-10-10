//! Simplified to Traditional conversion: characters only (`s2tw`, the
//! default) or characters and Taiwan phrasing (`s2twp`).
//! Pure Rust via opencc-fmmseg; no C dependency.

use async_trait::async_trait;

use crate::steps::{PipelineContext, Step, StepError, StepInput, StepOutput};
use crate::types::{ErrorCode, StepName};

const SUPPORTED: &[&str] = &[
    "s2t", "s2tw", "s2twp", "t2s", "tw2s", "tw2sp", "s2hk", "hk2s", "t2tw", "tw2t",
];

pub struct OpenCcStep {
    config: String,
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
    pub fn new(config: &str) -> Result<Self, StepError> {
        if !SUPPORTED.contains(&config) {
            return Err(StepError::fatal(
                ErrorCode::InvalidConfig,
                format!("unknown OpenCC config `{config}`"),
            ));
        }
        Ok(Self {
            config: config.to_string(),
            engine: opencc_fmmseg::OpenCC::new(),
        })
    }

    pub fn convert(&self, text: &str) -> String {
        self.engine.convert(text, &self.config, false)
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
        let step = OpenCcStep::new("s2twp").unwrap();
        assert_eq!(step.convert("汉字"), "漢字");
        // Taiwan phrasing: 软件 -> 軟體, 信息 -> 資訊.
        assert_eq!(step.convert("软件信息"), "軟體資訊");
    }

    #[test]
    fn traditional_input_is_unchanged() {
        let step = OpenCcStep::new("s2twp").unwrap();
        assert_eq!(
            step.convert("臺灣正體中文，English and 日本語。"),
            "臺灣正體中文，English and 日本語。"
        );
    }

    #[test]
    fn unknown_config_is_rejected() {
        let err = OpenCcStep::new("nope").unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidConfig);
        assert!(!err.recoverable);
    }

    #[tokio::test]
    async fn runs_as_step() {
        let step = OpenCcStep::new("s2twp").unwrap();
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
