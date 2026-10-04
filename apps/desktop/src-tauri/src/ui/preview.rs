//! What the widget shows while it is tried out from the settings window:
//! one state of a dictation at a time, with settings that are not saved yet.

use serde::Deserialize;

use crate::settings::{WidgetFade, WidgetIdle};

/// A state of the widget the user can look at without dictating.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Scene {
    /// Shown to be dragged into place.
    #[default]
    Place,
    Idle,
    Listening,
    Processing,
    Done,
    Failed,
}

impl Scene {
    /// The widget state that shows this scene.
    pub fn state(self) -> &'static str {
        match self {
            Scene::Place => super::PLACING,
            Scene::Listening => "recording",
            Scene::Processing => "processing",
            Scene::Idle | Scene::Done | Scene::Failed => "idle",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Scene::Place => "place",
            Scene::Idle => "idle",
            Scene::Listening => "listening",
            Scene::Processing => "processing",
            Scene::Done => "done",
            Scene::Failed => "failed",
        }
    }
}

/// The settings that decide how the widget looks.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Look {
    pub scale: f64,
    pub idle: WidgetIdle,
    pub fade: WidgetFade,
}

/// Texts a scene shows in place of a real dictation's.
#[derive(Debug, Clone, Default)]
pub struct Sample {
    pub raw: Option<String>,
    pub refined: Option<String>,
    pub detail: Option<String>,
}

/// The saved look, kept while another one is tried out.
#[derive(Default)]
pub struct Saved(Option<Look>);

impl Saved {
    /// Keeps the look in use before the first one is tried out; later
    /// calls change nothing.
    pub fn keep(&mut self, current: Look) {
        self.0.get_or_insert(current);
    }

    /// A setting was saved while another look is tried out: the look to
    /// return to follows it.
    pub fn edit(&mut self, change: impl FnOnce(&mut Look)) {
        if let Some(look) = self.0.as_mut() {
            change(look);
        }
    }

    /// The look to return to, when one was tried out.
    pub fn take(&mut self) -> Option<Look> {
        self.0.take()
    }
}

/// The input level of the listening scene after `secs`: rises and falls
/// like speech, between a quiet room and a raised voice.
pub fn level(secs: f32) -> f32 {
    let phrase = 0.5 + 0.5 * (secs * 2.3).sin();
    let syllable = 0.5 + 0.5 * (secs * 9.1 + 1.0).sin();
    0.01 + 0.19 * phrase * syllable
}

#[cfg(test)]
mod tests {
    use super::*;

    fn look(scale: f64) -> Look {
        Look {
            scale,
            idle: WidgetIdle::Hidden,
            fade: WidgetFade::default(),
        }
    }

    #[test]
    fn scenes_are_read_by_their_names() {
        for scene in [
            Scene::Place,
            Scene::Idle,
            Scene::Listening,
            Scene::Processing,
            Scene::Done,
            Scene::Failed,
        ] {
            let read: Scene = serde_json::from_str(&format!("\"{}\"", scene.name())).unwrap();
            assert_eq!(read, scene);
        }
    }

    #[test]
    fn only_listening_and_processing_look_like_a_dictation_in_progress() {
        assert_eq!(Scene::Place.state(), "placing");
        assert_eq!(Scene::Listening.state(), "recording");
        assert_eq!(Scene::Processing.state(), "processing");
        for scene in [Scene::Idle, Scene::Done, Scene::Failed] {
            assert_eq!(scene.state(), "idle", "{scene:?}");
        }
    }

    #[test]
    fn the_first_look_kept_is_the_one_returned_to() {
        let mut saved = Saved::default();
        assert_eq!(saved.take(), None);
        saved.keep(look(1.0));
        saved.keep(look(1.5));
        assert_eq!(saved.take(), Some(look(1.0)));
        assert_eq!(saved.take(), None);
    }

    #[test]
    fn a_setting_saved_meanwhile_changes_the_look_returned_to() {
        let mut saved = Saved::default();
        // Nothing is tried out: a saved setting is not kept here.
        saved.edit(|l| l.scale = 2.0);
        assert_eq!(saved.take(), None);
        saved.keep(look(1.0));
        saved.edit(|l| l.idle = WidgetIdle::Ring);
        let back = saved.take().unwrap();
        assert_eq!((back.scale, back.idle), (1.0, WidgetIdle::Ring));
    }

    #[test]
    fn the_sample_level_stays_within_what_a_microphone_gives() {
        let levels: Vec<f32> = (0..400).map(|i| level(i as f32 * 0.05)).collect();
        assert!(levels.iter().all(|l| (0.01..=0.2).contains(l)));
        let (min, max) = levels
            .iter()
            .fold((1.0f32, 0.0f32), |(a, b), l| (a.min(*l), b.max(*l)));
        assert!(max - min > 0.1, "the level moves: {min} to {max}");
    }
}
