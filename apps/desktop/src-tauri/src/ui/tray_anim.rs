//! The motion of the menu-bar image: frames cut from sheets, and the phases
//! the image passes through between the states of a dictation.
//!
//! The image is one colour, 18 pt, and at the edge of the screen; it moves
//! only where the widget's motion reads at that size: the gap of the mark
//! closes and takes the dot in when a dictation begins, a gap goes round
//! while the text is processed, the gap opens and the dot comes out when
//! the dictation is over. The level of the voice, the outcome's pulse and
//! the shake of a failure stay on the widget.

use std::sync::OnceLock;
use std::time::Duration;

use tauri::image::Image;

use crate::dictation::ui::UiState;

/// A frame's side in pixels; the sheets are this wide and a multiple of it high.
const SIDE: u32 = 36;

const SHEET_IDLE: Image<'static> = tauri::include_image!("./icons/tray-idle.png");
const SHEET_PAUSED: Image<'static> = tauri::include_image!("./icons/tray-paused.png");
const SHEET_PROBLEM: Image<'static> = tauri::include_image!("./icons/tray-problem.png");
const SHEET_UPDATE: Image<'static> = tauri::include_image!("./icons/tray-update.png");
const SHEET_START: Image<'static> = tauri::include_image!("./icons/tray-start.png");
const SHEET_STOP: Image<'static> = tauri::include_image!("./icons/tray-stop.png");
const SHEET_RECORDING: Image<'static> = tauri::include_image!("./icons/tray-recording.png");
const SHEET_PROCESSING: Image<'static> = tauri::include_image!("./icons/tray-processing.png");
const SHEET_DONE: Image<'static> = tauri::include_image!("./icons/tray-done.png");
const SHEET_ATTENTION: Image<'static> = tauri::include_image!("./icons/tray-attention.png");
const SHEET_FAILED: Image<'static> = tauri::include_image!("./icons/tray-failed.png");

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Sheet {
    Idle,
    Paused,
    Problem,
    Update,
    Start,
    Stop,
    Recording,
    Processing,
    Done,
    Attention,
    Failed,
}

const SHEETS: [(Sheet, Image<'static>); 11] = [
    (Sheet::Idle, SHEET_IDLE),
    (Sheet::Paused, SHEET_PAUSED),
    (Sheet::Problem, SHEET_PROBLEM),
    (Sheet::Update, SHEET_UPDATE),
    (Sheet::Start, SHEET_START),
    (Sheet::Stop, SHEET_STOP),
    (Sheet::Recording, SHEET_RECORDING),
    (Sheet::Processing, SHEET_PROCESSING),
    (Sheet::Done, SHEET_DONE),
    (Sheet::Attention, SHEET_ATTENTION),
    (Sheet::Failed, SHEET_FAILED),
];

/// The frames of a sheet, top to bottom.
fn cut(sheet: &Image<'static>) -> Vec<Image<'static>> {
    let bytes = (SIDE * SIDE * 4) as usize;
    sheet
        .rgba()
        .chunks_exact(bytes)
        .map(|frame| Image::new_owned(frame.to_vec(), SIDE, SIDE))
        .collect()
}

fn frames(sheet: Sheet) -> &'static [Image<'static>] {
    static FRAMES: OnceLock<Vec<Vec<Image<'static>>>> = OnceLock::new();
    let all = FRAMES.get_or_init(|| SHEETS.iter().map(|(_, image)| cut(image)).collect());
    let at = SHEETS.iter().position(|(s, _)| *s == sheet).unwrap_or(0);
    &all[at]
}

/// A frame of a sheet; the index wraps, so a loop never runs out.
fn frame(sheet: Sheet, i: usize) -> Image<'static> {
    let frames = frames(sheet);
    frames[i % frames.len()].clone()
}

const START_FRAMES: usize = 8;
const STOP_FRAMES: usize = 8;
const RECORDING_FRAMES: usize = 17;
const PROCESSING_FRAMES: usize = 27;
/// One frame of the closing, opening and going round: a 30th of a second.
const TICK: Duration = Duration::from_micros(33_333);
/// One frame of the breath while recording.
const BREATH: Duration = Duration::from_millis(100);
/// How long an outcome is shown before the gap opens again.
const OUTCOME_FOR: Duration = Duration::from_millis(1000);

/// What the image settles on, from the app's state.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Goal {
    Rest,
    /// The shortcuts are paused: the mark, faint.
    Paused,
    Problem,
    /// A newer version can be installed.
    Update,
    Recording,
    Processing,
}

impl Goal {
    /// `noticed`: something needs attention although dictation works;
    /// `update`: a newer version was found. A problem outranks an update.
    pub fn of(state: UiState, noticed: bool, update: bool) -> Self {
        match state {
            UiState::Idle if noticed => Goal::Problem,
            UiState::Idle if update => Goal::Update,
            UiState::Idle => Goal::Rest,
            UiState::Recording | UiState::Locked => Goal::Recording,
            UiState::Processing | UiState::Inserting => Goal::Processing,
            UiState::NoAccessibility | UiState::EngineError => Goal::Problem,
            UiState::Paused => Goal::Paused,
        }
    }

    fn is_session(self) -> bool {
        matches!(self, Goal::Recording | Goal::Processing)
    }
}

/// How a dictation ended, shown for a moment before the image rests.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Outcome {
    /// The text went where it should.
    Done,
    /// There is text, but it needs the user's attention.
    Attention,
    Failed,
}

/// What the image shows right now.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    Rest,
    Paused,
    Problem,
    Update,
    /// The gap closing, frame by frame.
    Start(usize),
    /// The dot breathing.
    Recording(usize),
    /// The gap going round.
    Processing(usize),
    Outcome(Outcome),
    /// The gap opening again.
    Stop(usize),
}

impl Phase {
    /// The phase a new goal puts the image in. `still`: the system asks
    /// for reduced motion, so the image jumps between the resting pictures.
    pub fn toward(self, goal: Goal, still: bool) -> Phase {
        match goal {
            Goal::Recording => match self {
                Phase::Recording(_) | Phase::Start(_) => self,
                Phase::Rest if !still => Phase::Start(0),
                _ => Phase::Recording(0),
            },
            Goal::Processing => match self {
                Phase::Processing(_) | Phase::Start(_) => self,
                Phase::Rest if !still => Phase::Start(0),
                _ => Phase::Processing(0),
            },
            Goal::Problem => match self {
                // The outcome has its moment; the problem shows after it.
                Phase::Outcome(_) => self,
                _ => Phase::Problem,
            },
            Goal::Update => match self {
                Phase::Outcome(_) => self,
                _ => Phase::Update,
            },
            Goal::Paused => match self {
                Phase::Outcome(_) => self,
                _ => Phase::Paused,
            },
            Goal::Rest => match self {
                Phase::Rest | Phase::Outcome(_) | Phase::Stop(_) => self,
                Phase::Problem | Phase::Update | Phase::Paused => Phase::Rest,
                _ if still => Phase::Rest,
                _ => Phase::Stop(0),
            },
        }
    }

    /// The phase an outcome puts the image in: shown unless a new dictation
    /// is already under way.
    pub fn with_outcome(self, outcome: Outcome, goal: Goal) -> Phase {
        if goal.is_session() {
            self
        } else {
            Phase::Outcome(outcome)
        }
    }

    /// How long this frame stays, or `None` for a picture that stays as it is.
    pub fn wait(self, still: bool) -> Option<Duration> {
        match self {
            Phase::Rest | Phase::Paused | Phase::Problem | Phase::Update => None,
            Phase::Start(_) | Phase::Stop(_) => Some(TICK),
            Phase::Recording(_) => (!still).then_some(BREATH),
            Phase::Processing(_) => (!still).then_some(TICK),
            Phase::Outcome(_) => Some(OUTCOME_FOR),
        }
    }

    /// The phase after this frame's time, with the goal as it is then.
    pub fn next(self, goal: Goal, still: bool) -> Phase {
        match self {
            Phase::Start(i) if i + 1 < START_FRAMES => Phase::Start(i + 1),
            Phase::Start(_) => match goal {
                Goal::Recording => Phase::Recording(0),
                Goal::Processing => Phase::Processing(0),
                Goal::Rest => Phase::Stop(0),
                Goal::Problem => Phase::Problem,
                Goal::Update => Phase::Update,
                Goal::Paused => Phase::Paused,
            },
            Phase::Recording(i) => Phase::Recording((i + 1) % RECORDING_FRAMES),
            Phase::Processing(i) => Phase::Processing((i + 1) % PROCESSING_FRAMES),
            Phase::Outcome(_) => match goal {
                Goal::Problem => Phase::Problem,
                Goal::Update => Phase::Update,
                Goal::Paused => Phase::Paused,
                _ if still => Phase::Rest,
                _ => Phase::Stop(0),
            },
            Phase::Stop(i) if i + 1 < STOP_FRAMES => Phase::Stop(i + 1),
            Phase::Stop(_) => match goal {
                Goal::Problem => Phase::Problem,
                Goal::Update => Phase::Update,
                Goal::Paused => Phase::Paused,
                _ => Phase::Rest,
            },
            Phase::Rest | Phase::Paused | Phase::Problem | Phase::Update => self,
        }
    }

    pub fn image(self) -> Image<'static> {
        match self {
            Phase::Rest => frame(Sheet::Idle, 0),
            Phase::Paused => frame(Sheet::Paused, 0),
            Phase::Problem => frame(Sheet::Problem, 0),
            Phase::Update => frame(Sheet::Update, 0),
            Phase::Start(i) => frame(Sheet::Start, i),
            Phase::Recording(i) => frame(Sheet::Recording, i),
            Phase::Processing(i) => frame(Sheet::Processing, i),
            Phase::Outcome(Outcome::Done) => frame(Sheet::Done, 0),
            Phase::Outcome(Outcome::Attention) => frame(Sheet::Attention, 0),
            Phase::Outcome(Outcome::Failed) => frame(Sheet::Failed, 0),
            Phase::Stop(i) => frame(Sheet::Stop, i),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Runs the clock until the image stays; returns the phases shown.
    fn run(mut phase: Phase, goal: Goal, still: bool) -> Vec<Phase> {
        let mut seen = vec![phase];
        while phase.wait(still).is_some() && seen.len() < 200 {
            phase = phase.next(goal, still);
            seen.push(phase);
            if matches!(phase, Phase::Recording(_) | Phase::Processing(_)) {
                break;
            }
        }
        seen
    }

    #[test]
    fn every_sheet_cuts_into_whole_frames() {
        for (sheet, image) in SHEETS.iter() {
            assert_eq!(image.width(), SIDE, "{sheet:?}");
            assert_eq!(image.height() % SIDE, 0, "{sheet:?}");
        }
        assert_eq!(frames(Sheet::Start).len(), START_FRAMES);
        assert_eq!(frames(Sheet::Stop).len(), STOP_FRAMES);
        assert_eq!(frames(Sheet::Recording).len(), RECORDING_FRAMES);
        assert_eq!(frames(Sheet::Processing).len(), PROCESSING_FRAMES);
        assert_eq!(frames(Sheet::Idle).len(), 1);
    }

    #[test]
    fn a_dictation_closes_the_gap_then_breathes() {
        let shown = run(
            Phase::Rest.toward(Goal::Recording, false),
            Goal::Recording,
            false,
        );
        assert_eq!(shown[0], Phase::Start(0));
        assert_eq!(shown[START_FRAMES - 1], Phase::Start(START_FRAMES - 1));
        assert_eq!(shown[START_FRAMES], Phase::Recording(0));
    }

    #[test]
    fn processing_goes_round_and_an_outcome_opens_the_gap() {
        let spinning = Phase::Recording(3).toward(Goal::Processing, false);
        assert_eq!(spinning, Phase::Processing(0));
        assert_eq!(spinning.next(Goal::Processing, false), Phase::Processing(1));
        let shown = Phase::Processing(7).with_outcome(Outcome::Done, Goal::Rest);
        assert_eq!(shown, Phase::Outcome(Outcome::Done));
        // The state goes idle right after the outcome; the outcome keeps its moment.
        assert_eq!(shown.toward(Goal::Rest, false), shown);
        let rest = run(shown, Goal::Rest, false);
        assert_eq!(rest[1], Phase::Stop(0));
        assert_eq!(*rest.last().unwrap(), Phase::Rest);
    }

    #[test]
    fn a_cancelled_dictation_opens_the_gap_without_an_outcome() {
        let rest = run(
            Phase::Recording(5).toward(Goal::Rest, false),
            Goal::Rest,
            false,
        );
        assert_eq!(rest[0], Phase::Stop(0));
        assert_eq!(*rest.last().unwrap(), Phase::Rest);
    }

    #[test]
    fn an_outcome_is_not_shown_once_the_next_dictation_began() {
        assert_eq!(
            Phase::Recording(2).with_outcome(Outcome::Failed, Goal::Recording),
            Phase::Recording(2)
        );
    }

    #[test]
    fn a_pause_shows_at_once_and_rest_follows_it_without_motion() {
        assert_eq!(Phase::Rest.toward(Goal::Paused, false), Phase::Paused);
        assert_eq!(
            Phase::Recording(3).toward(Goal::Paused, false),
            Phase::Paused
        );
        assert_eq!(Phase::Paused.wait(false), None);
        assert_eq!(Phase::Paused.toward(Goal::Rest, false), Phase::Rest);
        let shown = Phase::Outcome(Outcome::Done);
        assert_eq!(shown.toward(Goal::Paused, false), shown);
        assert_eq!(shown.next(Goal::Paused, false), Phase::Paused);
    }

    #[test]
    fn a_problem_waits_for_the_outcome_and_then_shows() {
        let shown = Phase::Outcome(Outcome::Attention);
        assert_eq!(shown.toward(Goal::Problem, false), shown);
        assert_eq!(shown.next(Goal::Problem, false), Phase::Problem);
        assert_eq!(Phase::Rest.toward(Goal::Problem, false), Phase::Problem);
        assert_eq!(Phase::Problem.toward(Goal::Rest, false), Phase::Rest);
    }

    #[test]
    fn reduced_motion_jumps_between_still_pictures() {
        let recording = Phase::Rest.toward(Goal::Recording, true);
        assert_eq!(recording, Phase::Recording(0));
        assert_eq!(recording.wait(true), None);
        assert_eq!(
            recording.toward(Goal::Processing, true),
            Phase::Processing(0)
        );
        assert_eq!(Phase::Processing(0).wait(true), None);
        assert_eq!(Phase::Processing(0).toward(Goal::Rest, true), Phase::Rest);
        assert_eq!(
            Phase::Outcome(Outcome::Done).next(Goal::Rest, true),
            Phase::Rest
        );
    }

    #[test]
    fn a_new_dictation_during_the_opening_closes_again_at_once() {
        assert_eq!(
            Phase::Stop(2).toward(Goal::Recording, false),
            Phase::Recording(0)
        );
        assert_eq!(
            Phase::Outcome(Outcome::Done).toward(Goal::Processing, false),
            Phase::Processing(0)
        );
    }
}
