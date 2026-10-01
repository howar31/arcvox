//! Dictionary alias replacement as a pipeline step.

use async_trait::async_trait;

use crate::dictionary::apply_aliases_counted;
use crate::steps::{PipelineContext, Step, StepError, StepInput, StepOutput};
use crate::types::StepName;

pub struct RulesStep;

#[async_trait]
impl Step for RulesStep {
    fn name(&self) -> StepName {
        StepName::Rules
    }

    async fn run(&self, input: StepInput, ctx: &PipelineContext) -> Result<StepOutput, StepError> {
        let (text, aliases) = apply_aliases_counted(&ctx.dictionary, &input.text);
        Ok(StepOutput {
            text,
            language: None,
            aliases,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::config::DictionaryEntry;
    use crate::steps::test_context;

    #[tokio::test]
    async fn replaces_using_context_dictionary() {
        let mut ctx = test_context();
        ctx.dictionary = Arc::new(vec![DictionaryEntry {
            term: "Kongwe".into(),
            aliases: vec!["空威".into()],
            asr_hint: true,
        }]);
        let out = RulesStep
            .run(
                StepInput {
                    text: "空威很好".into(),
                    audio: None,
                },
                &ctx,
            )
            .await
            .unwrap();
        assert_eq!(out.text, "Kongwe很好");
    }
}
