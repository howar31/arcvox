//! The fixed rules at the end of the text stages: the dictionary's alias
//! replacement, then the text rules that need no judgement.

use async_trait::async_trait;

use crate::dictionary::{apply_aliases_counted, AliasCounts};
use crate::steps::{PipelineContext, Step, StepError, StepInput, StepOutput};
use crate::tidy::{self, TidyRules};
use crate::types::StepName;

pub struct RulesStep {
    /// Whether the dictionary's aliases are replaced.
    pub dictionary: bool,
    pub rules: TidyRules,
}

#[async_trait]
impl Step for RulesStep {
    fn name(&self) -> StepName {
        StepName::Rules
    }

    async fn run(&self, input: StepInput, ctx: &PipelineContext) -> Result<StepOutput, StepError> {
        let (text, aliases) = if self.dictionary {
            apply_aliases_counted(&ctx.dictionary, &input.text)
        } else {
            (input.text, AliasCounts::default())
        };
        let (text, tidy) = tidy::apply(&text, self.rules);
        Ok(StepOutput {
            text,
            language: None,
            detail: serde_json::json!({
                "fixes": aliases.fixes,
                "snippets": aliases.snippets,
                "widened": tidy.widened,
                "spaces": tidy.spaces,
                "stop_removed": tidy.stop_removed,
                "question_added": tidy.question_added,
                "questions_inside": tidy.questions_inside,
            }),
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
            term: "Arcvox".into(),
            aliases: vec!["雅客".into()],
            asr_hint: true,
            ..Default::default()
        }]);
        let out = RulesStep {
            dictionary: true,
            rules: TidyRules::NONE,
        }
        .run(
            StepInput {
                text: "雅客很好".into(),
                audio: None,
            },
            &ctx,
        )
        .await
        .unwrap();
        assert_eq!(out.text, "Arcvox很好");
    }

    #[tokio::test]
    async fn the_text_rules_run_after_the_aliases_and_are_counted() {
        let mut ctx = test_context();
        ctx.dictionary = Arc::new(vec![DictionaryEntry {
            term: "Arcvox".into(),
            aliases: vec!["雅客".into()],
            asr_hint: true,
            ..Default::default()
        }]);
        let out = RulesStep {
            dictionary: true,
            rules: TidyRules {
                trailing_stop: true,
                question_mark: true,
                question_mark_inside: true,
                spacing: true,
                punctuation_width: true,
            },
        }
        .run(
            StepInput {
                text: "雅客好用嗎。".into(),
                audio: None,
            },
            &ctx,
        )
        .await
        .unwrap();
        assert_eq!(out.text, "Arcvox 好用嗎？");
        assert_eq!(out.detail["fixes"], 1);
        assert_eq!(out.detail["spaces"], 1);
        assert_eq!(out.detail["stop_removed"], true);
        assert_eq!(out.detail["question_added"], true);
    }

    #[tokio::test]
    async fn the_dictionary_can_be_left_out() {
        let mut ctx = test_context();
        ctx.dictionary = Arc::new(vec![DictionaryEntry {
            term: "Arcvox".into(),
            aliases: vec!["雅客".into()],
            asr_hint: true,
            ..Default::default()
        }]);
        let out = RulesStep {
            dictionary: false,
            rules: TidyRules::NONE,
        }
        .run(
            StepInput {
                text: "雅客很好".into(),
                audio: None,
            },
            &ctx,
        )
        .await
        .unwrap();
        assert_eq!(out.text, "雅客很好");
        assert_eq!(out.detail["fixes"], 0);
    }
}
