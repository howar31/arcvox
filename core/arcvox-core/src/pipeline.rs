//! Runs a profile's step chain, emits events, and applies degradation:
//! a fatal step (ASR) aborts; a recoverable step is skipped and the result
//! is marked degraded.

use std::sync::Arc;
use std::time::Instant;

use tokio::sync::mpsc;

use crate::config::{Config, Profile, ResolvedEngines, StepSpec};
use crate::history::{StageKind, StageRecord};
use crate::protocol::Event;
use crate::providers::registry::ProviderRegistry;
use crate::steps::asr::AsrStep;
use crate::steps::llm::{LlmStep, UnavailableStep};
use crate::steps::opencc::OpenCcStep;
use crate::steps::rules::RulesStep;
use crate::steps::translate::TranslateStep;
use crate::steps::{PipelineContext, Step, StepError, StepInput};
use crate::tidy::TidyRules;
use crate::types::{AudioClip, ErrorCode, StepName};

pub struct Pipeline {
    steps: Vec<Box<dyn Step>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Outcome {
    pub raw_text: String,
    pub final_text: String,
    pub degraded: bool,
    pub language: Option<String>,
    pub asr_ms: u64,
    pub llm_ms: Option<u64>,
    /// Dictionary term corrections and snippet insertions made by the run.
    pub dict_fixes: u32,
    /// The dictionary term of each replacement made, in order.
    pub dict_hits: Vec<String>,
    pub snippets: u32,
    /// The rewrite step ran, did not fail, and returned different text.
    pub llm_changed: bool,
    /// What each text step did, in order; a step that failed and was
    /// skipped has no text and names the error in its detail.
    pub stages: Vec<StageRecord>,
}

impl Pipeline {
    /// Builds the profile's steps on `engines`, the engines resolved for
    /// it (`Config::resolve_engines`), so a profile whose own engine is
    /// gone runs on the default.
    pub fn build(
        config: &Config,
        profile: &Profile,
        engines: &ResolvedEngines,
        registry: &ProviderRegistry,
    ) -> Result<Pipeline, StepError> {
        let mut steps: Vec<Box<dyn Step>> = Vec::with_capacity(profile.steps.len());
        for spec in &profile.steps {
            // A module that is off keeps its settings and does not run.
            if spec.is_off() {
                continue;
            }
            let step: Box<dyn Step> = match spec {
                StepSpec::Asr => {
                    let cfg = config.provider(&engines.asr.id).ok_or_else(|| {
                        StepError::fatal(
                            ErrorCode::InvalidConfig,
                            format!("asr provider {} missing", engines.asr.id),
                        )
                    })?;
                    let provider = registry.asr(cfg, &config.general).map_err(|e| StepError {
                        recoverable: false,
                        ..StepError::from(e)
                    })?;
                    Box::new(AsrStep::new(provider))
                }
                StepSpec::OpenCc {
                    config,
                    punctuation,
                } => Box::new(OpenCcStep::new(config, *punctuation)?),
                StepSpec::Llm { .. } => {
                    let id = engines.llm.as_ref().map(|c| &c.id).ok_or_else(|| {
                        StepError::fatal(ErrorCode::InvalidConfig, "llm step without an engine")
                    })?;
                    let cfg = config.provider(id).ok_or_else(|| {
                        StepError::fatal(
                            ErrorCode::InvalidConfig,
                            format!("llm provider {id} missing"),
                        )
                    })?;
                    // Unlike the ASR provider, an LLM provider that fails to build
                    // (for example a missing API key) does not abort the whole
                    // pipeline build: the step chain still needs an LLM slot so
                    // `run` can degrade to the ASR text at run time instead of the
                    // session never starting.
                    let llm_step: Box<dyn Step> = match registry.llm(cfg, &config.general) {
                        Ok(provider) => Box::new(LlmStep::new(provider)),
                        Err(e) => Box::new(UnavailableStep {
                            name: StepName::Llm,
                            error: StepError::from(e),
                        }),
                    };
                    llm_step
                }
                StepSpec::Rules {
                    dictionary,
                    trailing_stop,
                    off: _,
                    question_mark,
                    question_mark_inside,
                    spacing,
                    punctuation_width,
                } => Box::new(RulesStep {
                    dictionary: *dictionary,
                    rules: TidyRules {
                        trailing_stop: *trailing_stop,
                        question_mark: *question_mark,
                        question_mark_inside: *question_mark_inside,
                        spacing: *spacing,
                        punctuation_width: *punctuation_width,
                    },
                }),
                StepSpec::Translate {
                    target, addition, ..
                } => {
                    let id = engines.translate.as_ref().map(|c| &c.id).ok_or_else(|| {
                        StepError::fatal(
                            ErrorCode::InvalidConfig,
                            "translate step without an engine",
                        )
                    })?;
                    let cfg = config.provider(id).ok_or_else(|| {
                        StepError::fatal(
                            ErrorCode::InvalidConfig,
                            format!("translate provider {id} missing"),
                        )
                    })?;
                    // Like the rewrite: an engine that cannot be built now
                    // fails at run time, and the text goes on untranslated.
                    let step: Box<dyn Step> = match registry.llm(cfg, &config.general) {
                        Ok(provider) => Box::new(TranslateStep::new(provider, target, addition)),
                        Err(e) => Box::new(UnavailableStep {
                            name: StepName::Translate,
                            error: StepError::from(e),
                        }),
                    };
                    step
                }
            };
            steps.push(step);
        }
        Ok(Pipeline { steps })
    }

    pub async fn run(
        &self,
        audio: Arc<AudioClip>,
        ctx: &PipelineContext,
        events: &mpsc::Sender<Event>,
    ) -> Result<Outcome, StepError> {
        self.run_inner(Some(audio), String::new(), ctx, events)
            .await
    }

    /// Runs the steps after ASR on `transcript`, as if ASR had produced it.
    /// For measuring the rewrite on its own.
    pub async fn run_from_text(
        &self,
        transcript: &str,
        ctx: &PipelineContext,
        events: &mpsc::Sender<Event>,
    ) -> Result<Outcome, StepError> {
        self.run_inner(None, transcript.to_string(), ctx, events)
            .await
    }

    async fn run_inner(
        &self,
        audio: Option<Arc<AudioClip>>,
        start: String,
        ctx: &PipelineContext,
        events: &mpsc::Sender<Event>,
    ) -> Result<Outcome, StepError> {
        let mut raw_text = start.clone();
        let mut text = start;
        let mut language = None;
        let mut degraded = false;
        let mut asr_ms = 0;
        let mut llm_ms = None;
        let mut dict_fixes = 0u32;
        let mut dict_hits: Vec<String> = Vec::new();
        let mut snippets = 0u32;
        let mut llm_changed = false;
        let mut stages: Vec<StageRecord> = Vec::with_capacity(self.steps.len());
        // Whether the rewrite has run: a script conversion before it and one
        // after it are two different places.
        let mut after_rewrite = false;

        for step in &self.steps {
            let name = step.name();
            if name == StepName::Asr && audio.is_none() {
                continue;
            }
            let kind = match name {
                StepName::Asr => StageKind::Asr,
                StepName::OpenCc => StageKind::OpenCc,
                StepName::Llm => StageKind::Llm,
                StepName::Rules => StageKind::Rules,
                StepName::Translate => StageKind::Translate,
            };
            // One step per place: the pages show one step for each. A second
            // conversion before the rewrite (kept in a profile whose rewrite
            // is turned off, so that it comes back with it) or a repeated
            // step does not run.
            let taken = stages.iter().any(|s| {
                s.kind == kind
                    && (kind != StageKind::OpenCc || s.detail["after_rewrite"] == after_rewrite)
            });
            if taken {
                continue;
            }
            if kind == StageKind::Llm {
                after_rewrite = true;
            }
            // Numbered in the order they ran; the engine renumbers once the
            // audio stages and the record stage are around them.
            let position = stages.len() as u32 + 1;
            let _ = events.send(Event::StepStarted { step: name }).await;
            let started = Instant::now();
            let input = StepInput {
                text: text.clone(),
                audio: (name == StepName::Asr).then(|| audio.clone()).flatten(),
            };
            match step.run(input, ctx).await {
                Ok(out) => {
                    let elapsed = started.elapsed().as_millis() as u64;
                    let mut detail = out.detail.clone();
                    if kind == StageKind::OpenCc {
                        detail["after_rewrite"] = serde_json::Value::Bool(after_rewrite);
                    }
                    stages.push(StageRecord {
                        position,
                        kind,
                        text: Some(out.text.clone()),
                        ms: Some(elapsed),
                        detail,
                    });
                    match name {
                        StepName::Asr => {
                            asr_ms = elapsed;
                            raw_text = out.text.clone();
                            language = out.language.clone();
                            *ctx.recognised.lock().unwrap() = Some(out.text.clone());
                            let _ = events
                                .send(Event::Transcript {
                                    text: out.text.clone(),
                                    is_final: true,
                                    language: out.language.clone(),
                                })
                                .await;
                        }
                        StepName::Llm => {
                            llm_ms = Some(elapsed);
                            llm_changed = out.text != text;
                        }
                        _ => {}
                    }
                    dict_fixes += out.aliases.fixes;
                    dict_hits.extend(out.aliases.terms.iter().cloned());
                    snippets += out.aliases.snippets;
                    text = out.text;
                    let _ = events
                        .send(Event::StepFinished {
                            step: name,
                            text: text.clone(),
                            elapsed_ms: elapsed,
                        })
                        .await;
                    // An ASR step that succeeds with only whitespace means no
                    // speech was recognized. Sending that to the LLM would ask
                    // it to refine nothing, so the session ends here instead.
                    if name == StepName::Asr && text.trim().is_empty() {
                        let err = StepError::fatal(ErrorCode::NoSpeech, "no speech recognized");
                        let _ = events
                            .send(Event::Error {
                                step: Some(StepName::Asr),
                                code: err.code,
                                message: err.message.clone(),
                                recoverable: err.recoverable,
                                hint: err.hint.clone(),
                            })
                            .await;
                        return Err(err);
                    }
                }
                Err(err) => {
                    let _ = events
                        .send(Event::Error {
                            step: Some(name),
                            code: err.code,
                            message: err.message.clone(),
                            recoverable: err.recoverable,
                            hint: err.hint.clone(),
                        })
                        .await;
                    if !err.recoverable || name == StepName::Asr {
                        return Err(err);
                    }
                    let mut detail = serde_json::json!({
                        "error": err.code.name(),
                        "message": err.message,
                    });
                    if kind == StageKind::OpenCc {
                        detail["after_rewrite"] = serde_json::Value::Bool(after_rewrite);
                    }
                    stages.push(StageRecord {
                        position,
                        kind,
                        text: None,
                        ms: Some(started.elapsed().as_millis() as u64),
                        detail,
                    });
                    degraded = true;
                }
            }
        }
        let _ = events
            .send(Event::Refined {
                text: text.clone(),
                degraded,
                translated: StageRecord::translated(&stages),
            })
            .await;
        Ok(Outcome {
            raw_text,
            final_text: text,
            degraded,
            language,
            asr_ms,
            llm_ms,
            dict_fixes,
            dict_hits,
            snippets,
            llm_changed,
            stages,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Profile;
    use crate::steps::asr::fakes::FakeAsr;
    use crate::steps::llm::fakes::FakeLlm;
    use crate::steps::test_context;

    fn pipeline(asr: Arc<FakeAsr>, llm: Arc<FakeLlm>) -> Pipeline {
        Pipeline {
            steps: vec![
                Box::new(AsrStep::new(asr)),
                Box::new(OpenCcStep::new("s2twp", false).unwrap()),
                Box::new(LlmStep::new(llm)),
                Box::new(OpenCcStep::new("s2twp", false).unwrap()),
                Box::new(RulesStep {
                    dictionary: true,
                    rules: TidyRules::NONE,
                }),
            ],
        }
    }

    fn audio() -> Arc<AudioClip> {
        Arc::new(AudioClip {
            samples: vec![0.0; 16_000],
            sample_rate: 16_000,
        })
    }

    async fn collect(rx: &mut mpsc::Receiver<Event>) -> Vec<Event> {
        let mut out = Vec::new();
        while let Ok(e) = rx.try_recv() {
            out.push(e);
        }
        out
    }

    #[tokio::test]
    async fn stages_are_numbered_in_the_order_they_ran() {
        let (tx, _rx) = mpsc::channel(64);
        let p = pipeline(FakeAsr::ok("软件 原文"), FakeLlm::ok("软件 整理後"));
        let out = p.run(audio(), &test_context(), &tx).await.unwrap();
        let positions: Vec<(u32, StageKind)> =
            out.stages.iter().map(|s| (s.position, s.kind)).collect();
        assert_eq!(
            positions,
            [
                (1, StageKind::Asr),
                (2, StageKind::OpenCc),
                (3, StageKind::Llm),
                (4, StageKind::OpenCc),
                (5, StageKind::Rules)
            ]
        );
        assert_eq!(out.stages[1].detail["after_rewrite"], false);
        assert_eq!(out.stages[3].detail["after_rewrite"], true);
        assert_eq!(out.stages[0].text.as_deref(), Some("软件 原文"));
        assert_eq!(out.stages[1].text.as_deref(), Some("軟體 原文"));
        assert_eq!(out.stages[1].detail["config"], "s2twp");
        assert_eq!(out.stages[1].detail["changed_chars"], 2);
        assert_eq!(out.stages[2].detail["in_chars"], 5);
        assert_eq!(out.stages[2].detail["out_chars"], 6);
        assert!(
            out.stages[2].detail["prompt"]["base_chars"]
                .as_u64()
                .unwrap()
                > 0
        );
        assert_eq!(out.stages[4].detail["fixes"], 0);
        assert!(out.stages.iter().all(|s| s.ms.is_some()));
    }

    #[tokio::test]
    async fn a_translation_runs_last_and_its_failure_keeps_the_tidied_text() {
        use crate::steps::translate::TranslateStep;
        let (tx, _rx) = mpsc::channel(64);
        let mut p = pipeline(FakeAsr::ok("原文"), FakeLlm::ok("原文。"));
        p.steps.push(Box::new(TranslateStep::new(
            FakeLlm::ok("The original."),
            "英文",
            "",
        )));
        let out = p.run(audio(), &test_context(), &tx).await.unwrap();
        let kinds: Vec<(u32, StageKind)> =
            out.stages.iter().map(|s| (s.position, s.kind)).collect();
        assert_eq!(kinds[5], (6, StageKind::Translate));
        assert_eq!(out.final_text, "The original.");
        assert_eq!(out.stages[5].detail["target"], "英文");
        assert!(!out.degraded);
        assert!(StageRecord::translated(&out.stages));

        let mut p = pipeline(FakeAsr::ok("原文"), FakeLlm::ok("原文。"));
        p.steps.push(Box::new(TranslateStep::new(
            FakeLlm::failing(ErrorCode::ProviderTimeout),
            "英文",
            "",
        )));
        let out = p.run(audio(), &test_context(), &tx).await.unwrap();
        assert_eq!(out.final_text, "原文。");
        assert!(out.degraded);
        assert_eq!(out.stages[5].text, None);
        assert!(!StageRecord::translated(&out.stages));
    }

    #[tokio::test]
    async fn a_second_step_at_a_taken_place_does_not_run() {
        let (tx, mut rx) = mpsc::channel(64);
        // A profile whose rewrite is turned off keeps both conversions.
        let p = Pipeline {
            steps: vec![
                Box::new(AsrStep::new(FakeAsr::ok("软件"))),
                Box::new(OpenCcStep::new("s2tw", false).unwrap()),
                Box::new(OpenCcStep::new("t2s", false).unwrap()),
                Box::new(RulesStep {
                    dictionary: true,
                    rules: TidyRules::NONE,
                }),
            ],
        };
        let out = p.run(audio(), &test_context(), &tx).await.unwrap();
        let positions: Vec<u32> = out.stages.iter().map(|s| s.position).collect();
        assert_eq!(positions, [1, 2, 3]);
        assert_eq!(out.final_text, "軟件");
        let started = collect(&mut rx)
            .await
            .into_iter()
            .filter(|e| {
                matches!(
                    e,
                    Event::StepStarted {
                        step: StepName::OpenCc
                    }
                )
            })
            .count();
        assert_eq!(started, 1);
    }

    #[tokio::test]
    async fn a_conversion_after_the_rewrite_alone_is_told_so() {
        let (tx, _rx) = mpsc::channel(64);
        let p = Pipeline {
            steps: vec![
                Box::new(AsrStep::new(FakeAsr::ok("软件"))),
                Box::new(LlmStep::new(FakeLlm::ok("软件"))),
                Box::new(OpenCcStep::new("s2tw", false).unwrap()),
            ],
        };
        let out = p.run(audio(), &test_context(), &tx).await.unwrap();
        let positions: Vec<u32> = out.stages.iter().map(|s| s.position).collect();
        assert_eq!(positions, [1, 2, 3]);
        assert_eq!(out.stages[2].kind, StageKind::OpenCc);
        assert_eq!(out.stages[2].detail["after_rewrite"], true);
    }

    #[tokio::test]
    async fn a_failed_step_is_recorded_without_text() {
        let (tx, _rx) = mpsc::channel(64);
        let p = pipeline(
            FakeAsr::ok("原文"),
            FakeLlm::failing(ErrorCode::ProviderTimeout),
        );
        let out = p.run(audio(), &test_context(), &tx).await.unwrap();
        assert!(out.degraded);
        let llm = out
            .stages
            .iter()
            .find(|s| s.kind == StageKind::Llm)
            .unwrap();
        assert_eq!(llm.position, 3);
        assert_eq!(llm.text, None);
        assert_eq!(llm.detail["error"], "provider_timeout");
        assert!(llm.detail["message"].is_string());
        assert_eq!(out.stages.len(), 5);
    }

    #[tokio::test]
    async fn run_from_text_has_no_asr_stage() {
        let (tx, _rx) = mpsc::channel(64);
        let p = pipeline(FakeAsr::ok("x"), FakeLlm::ok("整理後"));
        let out = p.run_from_text("原文", &test_context(), &tx).await.unwrap();
        assert_eq!(out.stages[0].kind, StageKind::OpenCc);
        assert_eq!(out.stages[0].position, 1);
    }

    #[tokio::test]
    async fn run_from_text_skips_asr_and_treats_the_text_as_the_transcript() {
        let (tx, mut rx) = mpsc::channel(64);
        let asr = FakeAsr::failing(ErrorCode::ProviderTimeout);
        let p = pipeline(asr, FakeLlm::ok("软件 整理後"));
        let out = p
            .run_from_text("软件 原文", &test_context(), &tx)
            .await
            .unwrap();
        assert_eq!(out.raw_text, "软件 原文");
        assert_eq!(out.final_text, "軟體 整理後");
        assert_eq!(out.asr_ms, 0);
        assert!(out.llm_ms.is_some());
        let events = collect(&mut rx).await;
        assert!(
            !events.iter().any(|e| matches!(
                e,
                Event::StepStarted {
                    step: StepName::Asr
                }
            )),
            "{events:?}"
        );
        assert!(events.iter().any(|e| matches!(e, Event::Refined { .. })));
    }

    #[tokio::test]
    async fn happy_path_runs_all_steps_and_emits_events() {
        let (tx, mut rx) = mpsc::channel(64);
        let p = pipeline(FakeAsr::ok("软件 原文"), FakeLlm::ok("软件 整理後"));
        let out = p.run(audio(), &test_context(), &tx).await.unwrap();
        assert_eq!(out.raw_text, "软件 原文");
        assert_eq!(out.final_text, "軟體 整理後");
        assert!(!out.degraded);
        assert!(out.llm_ms.is_some());
        let events = collect(&mut rx).await;
        let kinds: Vec<&str> = events
            .iter()
            .map(|e| match e {
                Event::StepStarted { .. } => "started",
                Event::StepFinished { .. } => "finished",
                Event::Transcript { .. } => "transcript",
                Event::Refined { .. } => "refined",
                _ => "other",
            })
            .collect();
        assert_eq!(
            kinds,
            [
                "started",
                "transcript",
                "finished",
                "started",
                "finished",
                "started",
                "finished",
                "started",
                "finished",
                "started",
                "finished",
                "refined"
            ]
        );
        assert!(matches!(
            events.last(),
            Some(Event::Refined {
                degraded: false,
                ..
            })
        ));
    }

    #[tokio::test]
    async fn outcome_counts_dictionary_replacements_and_a_changed_rewrite() {
        let (tx, _rx) = mpsc::channel(64);
        let mut ctx = test_context();
        ctx.dictionary = Arc::new(vec![crate::config::DictionaryEntry {
            term: "Arcvox".into(),
            aliases: vec!["雅客".into()],
            asr_hint: false,
            ..Default::default()
        }]);
        let p = pipeline(FakeAsr::ok("原文"), FakeLlm::ok("雅客 整理後"));
        let out = p.run(audio(), &ctx, &tx).await.unwrap();
        assert_eq!(out.final_text, "Arcvox 整理後");
        assert_eq!((out.dict_fixes, out.snippets), (1, 0));
        assert!(out.llm_changed);
    }

    #[tokio::test]
    async fn an_unchanged_rewrite_is_not_counted_as_changed() {
        let (tx, _rx) = mpsc::channel(64);
        let p = pipeline(FakeAsr::ok("原文"), FakeLlm::ok("原文"));
        let out = p.run(audio(), &test_context(), &tx).await.unwrap();
        assert!(out.llm_ms.is_some());
        assert!(!out.llm_changed);
        assert_eq!((out.dict_fixes, out.snippets), (0, 0));
    }

    #[tokio::test]
    async fn llm_failure_degrades_to_converted_asr_text() {
        let (tx, mut rx) = mpsc::channel(64);
        let p = pipeline(
            FakeAsr::ok("软件"),
            FakeLlm::failing(ErrorCode::ProviderTimeout),
        );
        let out = p.run(audio(), &test_context(), &tx).await.unwrap();
        assert_eq!(out.final_text, "軟體");
        assert!(out.degraded);
        assert!(out.llm_ms.is_none());
        let events = collect(&mut rx).await;
        assert!(events.iter().any(|e| matches!(
            e,
            Event::Error {
                step: Some(StepName::Llm),
                recoverable: true,
                ..
            }
        )));
        assert!(matches!(
            events.last(),
            Some(Event::Refined { degraded: true, .. })
        ));
    }

    #[tokio::test]
    async fn empty_llm_output_degrades() {
        let (tx, _rx) = mpsc::channel(64);
        let p = pipeline(FakeAsr::ok("原文"), FakeLlm::ok("  \n"));
        let out = p.run(audio(), &test_context(), &tx).await.unwrap();
        assert_eq!(out.final_text, "原文");
        assert!(out.degraded);
    }

    #[tokio::test]
    async fn asr_failure_aborts_without_refined() {
        let (tx, mut rx) = mpsc::channel(64);
        let p = pipeline(
            FakeAsr::failing(ErrorCode::ProviderUnreachable),
            FakeLlm::ok("x"),
        );
        let err = p.run(audio(), &test_context(), &tx).await.unwrap_err();
        assert_eq!(err.code, ErrorCode::ProviderUnreachable);
        let events = collect(&mut rx).await;
        assert!(events.iter().any(|e| matches!(
            e,
            Event::Error {
                step: Some(StepName::Asr),
                recoverable: false,
                ..
            }
        )));
        assert!(!events.iter().any(|e| matches!(e, Event::Refined { .. })));
    }

    #[tokio::test]
    async fn empty_transcript_aborts_without_llm() {
        let (tx, mut rx) = mpsc::channel(64);
        let llm = FakeLlm::ok("x");
        let p = pipeline(FakeAsr::ok("   "), llm.clone());
        let err = p.run(audio(), &test_context(), &tx).await.unwrap_err();
        assert_eq!(err.code, ErrorCode::NoSpeech);
        let events = collect(&mut rx).await;
        assert!(events.iter().any(|e| matches!(
            e,
            Event::Error {
                step: Some(StepName::Asr),
                code: ErrorCode::NoSpeech,
                ..
            }
        )));
        assert!(!events.iter().any(|e| matches!(e, Event::Refined { .. })));
        assert!(llm.calls.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn build_with_missing_llm_key_degrades_at_run() {
        // The ASR provider in `first_run` is a real HTTP provider, so a
        // `Pipeline::build`-then-`run` test would need a live server. The
        // build-level guarantee (that a missing LLM key does not fail the
        // build) is covered separately by `build_succeeds_when_llm_key_is_missing`;
        // here the pipeline is constructed directly with fakes to exercise
        // what `Pipeline::build` would have produced: an `UnavailableStep`
        // in the LLM slot.
        let p = Pipeline {
            steps: vec![
                Box::new(AsrStep::new(FakeAsr::ok("原文"))),
                Box::new(crate::steps::llm::UnavailableStep {
                    name: StepName::Llm,
                    error: StepError::recoverable(ErrorCode::ProviderUnauthorized, "missing key"),
                }),
            ],
        };
        let (tx, mut rx) = mpsc::channel(64);
        let out = p.run(audio(), &test_context(), &tx).await.unwrap();
        assert!(out.degraded);
        assert_eq!(out.final_text, "原文");
        let events = collect(&mut rx).await;
        assert!(events.iter().any(|e| matches!(
            e,
            Event::Error {
                step: Some(StepName::Llm),
                recoverable: true,
                code: ErrorCode::ProviderUnauthorized,
                ..
            }
        )));
    }

    #[test]
    fn build_succeeds_when_llm_key_is_missing() {
        let mut config = Config::http_test_config();
        config.providers[1].api_key_ref = Some("absent".into());
        let registry = ProviderRegistry::new(
            reqwest::Client::new(),
            Arc::new(crate::secrets::MemorySecretStore::default()),
        );
        let engines = config.resolve_engines(config.default_profile(), |_| true);
        assert!(Pipeline::build(&config, config.default_profile(), &engines, &registry).is_ok());
    }

    #[test]
    fn a_module_that_is_off_is_not_built() {
        let config = Config::http_test_config();
        let registry = ProviderRegistry::new(
            reqwest::Client::new(),
            Arc::new(crate::secrets::MemorySecretStore::default()),
        );
        let mut profile = config.default_profile().clone();
        for s in &mut profile.steps {
            if let StepSpec::Llm { off } | StepSpec::Rules { off, .. } = s {
                *off = true;
            }
        }
        let engines = config.resolve_engines(&profile, |_| true);
        let p = Pipeline::build(&config, &profile, &engines, &registry).unwrap();
        let names: Vec<StepName> = p.steps.iter().map(|s| s.name()).collect();
        assert!(!names.contains(&StepName::Llm));
        assert!(!names.contains(&StepName::Rules));
        assert_eq!(names.len(), profile.steps.len() - 2);
    }

    #[tokio::test]
    async fn build_from_first_run_config_succeeds_and_rejects_bad_opencc() {
        let config = Config::http_test_config();
        let registry = ProviderRegistry::new(
            reqwest::Client::new(),
            Arc::new(crate::secrets::MemorySecretStore::default()),
        );
        let engines = config.resolve_engines(config.default_profile(), |_| true);
        assert!(Pipeline::build(&config, config.default_profile(), &engines, &registry).is_ok());
        let mut profile: Profile = config.default_profile().clone();
        profile.steps = vec![
            StepSpec::Asr,
            StepSpec::OpenCc {
                config: "bogus".into(),
                punctuation: false,
            },
        ];
        let err = Pipeline::build(&config, &profile, &engines, &registry)
            .err()
            .unwrap();
        assert_eq!(err.code, ErrorCode::InvalidConfig);
    }
}
