//! Runs a profile's step chain, emits events, and applies degradation:
//! a fatal step (ASR) aborts; a recoverable step is skipped and the result
//! is marked degraded.

use std::sync::Arc;
use std::time::Instant;

use tokio::sync::mpsc;

use crate::config::{Config, Profile, StepSpec};
use crate::protocol::Event;
use crate::providers::registry::ProviderRegistry;
use crate::steps::asr::AsrStep;
use crate::steps::llm::LlmStep;
use crate::steps::opencc::OpenCcStep;
use crate::steps::rules::RulesStep;
use crate::steps::{PipelineContext, Step, StepError, StepInput};
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
}

impl Pipeline {
    pub fn build(
        config: &Config,
        profile: &Profile,
        registry: &ProviderRegistry,
    ) -> Result<Pipeline, StepError> {
        let mut steps: Vec<Box<dyn Step>> = Vec::with_capacity(profile.steps.len());
        for spec in &profile.steps {
            let step: Box<dyn Step> = match spec {
                StepSpec::Asr => {
                    let cfg = config.provider(&profile.asr_provider).ok_or_else(|| {
                        StepError::fatal(
                            ErrorCode::InvalidConfig,
                            format!("asr provider {} missing", profile.asr_provider),
                        )
                    })?;
                    let provider = registry.asr(cfg, &config.general).map_err(|e| StepError {
                        recoverable: false,
                        ..StepError::from(e)
                    })?;
                    Box::new(AsrStep::new(provider))
                }
                StepSpec::OpenCc { config: name } => Box::new(OpenCcStep::new(name)?),
                StepSpec::Llm => {
                    let id = profile.llm_provider.as_ref().ok_or_else(|| {
                        StepError::fatal(ErrorCode::InvalidConfig, "llm step without llm_provider")
                    })?;
                    let cfg = config.provider(id).ok_or_else(|| {
                        StepError::fatal(
                            ErrorCode::InvalidConfig,
                            format!("llm provider {id} missing"),
                        )
                    })?;
                    let provider = registry.llm(cfg).map_err(|e| StepError {
                        recoverable: false,
                        ..StepError::from(e)
                    })?;
                    Box::new(LlmStep::new(provider))
                }
                StepSpec::Rules => Box::new(RulesStep),
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
        let mut text = String::new();
        let mut raw_text = String::new();
        let mut language = None;
        let mut degraded = false;
        let mut asr_ms = 0;
        let mut llm_ms = None;

        for step in &self.steps {
            let name = step.name();
            let _ = events.send(Event::StepStarted { step: name }).await;
            let started = Instant::now();
            let input = StepInput {
                text: text.clone(),
                audio: (name == StepName::Asr).then(|| audio.clone()),
            };
            match step.run(input, ctx).await {
                Ok(out) => {
                    let elapsed = started.elapsed().as_millis() as u64;
                    match name {
                        StepName::Asr => {
                            asr_ms = elapsed;
                            raw_text = out.text.clone();
                            language = out.language.clone();
                            let _ = events
                                .send(Event::Transcript {
                                    text: out.text.clone(),
                                    is_final: true,
                                    language: out.language.clone(),
                                })
                                .await;
                        }
                        StepName::Llm => llm_ms = Some(elapsed),
                        _ => {}
                    }
                    text = out.text;
                    let _ = events
                        .send(Event::StepFinished {
                            step: name,
                            text: text.clone(),
                            elapsed_ms: elapsed,
                        })
                        .await;
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
                    degraded = true;
                }
            }
        }
        let _ = events
            .send(Event::Refined {
                text: text.clone(),
                degraded,
            })
            .await;
        Ok(Outcome {
            raw_text,
            final_text: text,
            degraded,
            language,
            asr_ms,
            llm_ms,
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
                Box::new(OpenCcStep::new("s2twp").unwrap()),
                Box::new(LlmStep::new(llm)),
                Box::new(OpenCcStep::new("s2twp").unwrap()),
                Box::new(RulesStep),
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
    async fn build_from_first_run_config_succeeds_and_rejects_bad_opencc() {
        let config = Config::first_run();
        let registry = ProviderRegistry::new(
            reqwest::Client::new(),
            Arc::new(crate::secrets::MemorySecretStore::default()),
        );
        assert!(Pipeline::build(&config, config.default_profile(), &registry).is_ok());
        let mut profile: Profile = config.default_profile().clone();
        profile.steps = vec![
            StepSpec::Asr,
            StepSpec::OpenCc {
                config: "bogus".into(),
            },
        ];
        let err = Pipeline::build(&config, &profile, &registry).err().unwrap();
        assert_eq!(err.code, ErrorCode::InvalidConfig);
    }
}
