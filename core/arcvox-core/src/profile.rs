//! Picks the profile for a session: explicit override, then first matcher
//! hit, then the default profile.

use crate::config::{
    Config, Matcher, Profile, FACTORY_PRESET, MODEL_ID_DEFAULT_ASR, MODEL_ID_DEFAULT_LLM,
};
use crate::types::{AppInfo, ProfileId, ProviderId};

pub fn resolve<'a>(
    profiles: &'a [Profile],
    app: &AppInfo,
    override_id: Option<&ProfileId>,
) -> &'a Profile {
    if let Some(id) = override_id {
        if let Some(p) = profiles.iter().find(|p| &p.id == id) {
            return p;
        }
    }
    if let Some(p) = profiles.iter().find(|p| matches(p, app)) {
        return p;
    }
    profiles
        .iter()
        .find(|p| p.is_default)
        .expect("validated config has exactly one default profile")
}

fn matches(profile: &Profile, app: &AppInfo) -> bool {
    let m = &profile.matcher;
    if let Some(id) = &app.app_id {
        if m.app_ids.iter().any(|a| a == id) {
            return true;
        }
    }
    if let Some(url) = &app.url {
        let url = url.to_ascii_lowercase();
        if m.url_patterns
            .iter()
            .any(|p| !p.is_empty() && url.contains(&p.to_ascii_lowercase()))
        {
            return true;
        }
    }
    false
}

/// A built-in profile template: a whole profile as shipped, from the
/// engines to the applications it is used in. A new profile is a copy of
/// one, and a reset makes a profile a copy of its own again. A template
/// follows the default engines unless it names a model on purpose; a
/// named engine is one the app makes (a catalogue model's, a platform
/// service's), created when missing, and the catalogue must carry it
/// (`templates_name_only_bundled_models` in the desktop crate).
#[derive(Debug, Clone, Copy)]
pub struct ProfilePreset {
    pub id: &'static str,
    /// The speech engine, None to follow the default.
    pub asr: Option<&'static str>,
    /// The rewrite engine, None to follow the default.
    pub llm: Option<&'static str>,
    /// Whether the writing style was tuned against engines: the named ones,
    /// else the factory pair. The blank template is untuned.
    pub tuned: bool,
    pub language_hint: Option<&'static str>,
    pub app_ids: &'static [&'static str],
    pub prompt_addition: &'static str,
}

/// The order the templates are offered in: the tuned ones, then the
/// untuned one for those who start from nothing.
static PRESETS: [ProfilePreset; 4] = [
    ProfilePreset {
        id: "casual",
        asr: None,
        llm: None,
        tuned: true,
        language_hint: Some("zh-TW"),
        app_ids: &[],
        prompt_addition: "",
    },
    ProfilePreset {
        id: "formal",
        asr: None,
        llm: None,
        tuned: true,
        language_hint: Some("zh-TW"),
        app_ids: &["com.apple.mail", "com.microsoft.Outlook"],
        prompt_addition: "這是正式的書面文字。使用完整句子與書面用語，句尾加句點，依語意分段；稱呼與結尾問候語依原稿保留，不自行增加。",
    },
    ProfilePreset {
        id: "verbatim",
        asr: None,
        llm: None,
        tuned: true,
        language_hint: Some("zh-TW"),
        app_ids: &[],
        prompt_addition: "逐字記錄：每個字都照原稿保留，不吸收自我修正、不刪語助詞與重複、不改用詞、不翻譯；只加入標點與分段。",
    },
    // Every option at its untuned default: where the other templates start
    // from, for a user who wants to tune everything alone.
    ProfilePreset {
        id: "blank",
        asr: None,
        llm: None,
        tuned: false,
        language_hint: Some("zh-TW"),
        app_ids: &[],
        prompt_addition: "",
    },
];

pub fn presets() -> &'static [ProfilePreset] {
    &PRESETS
}

impl ProfilePreset {
    /// The engines the template's writing style was tuned with, as
    /// (speech, rewrite): the ones it names, else the factory pair; None
    /// for an untuned template. They do not move with the user's default.
    pub fn tuned_with(&self) -> Option<(&'static str, &'static str)> {
        self.tuned.then(|| {
            (
                self.asr.unwrap_or(MODEL_ID_DEFAULT_ASR),
                self.llm.unwrap_or(MODEL_ID_DEFAULT_LLM),
            )
        })
    }
}

pub fn preset(id: &str) -> Option<&'static ProfilePreset> {
    PRESETS.iter().find(|p| p.id == id)
}

/// The preset a reset of `profile` returns to: the one it was made from,
/// else the factory one. None when the profile names a preset that no
/// longer exists.
pub fn reset_target(profile: &Profile) -> Option<&'static ProfilePreset> {
    preset(profile.preset.as_deref().unwrap_or(FACTORY_PRESET))
}

impl Profile {
    /// A copy of the template under a new id (the preset's, with a numeric
    /// suffix while taken) and `name`.
    pub fn from_preset(preset: &ProfilePreset, name: String, taken: &[ProfileId]) -> Profile {
        let mut id = ProfileId(preset.id.to_string());
        let mut n = 2;
        while taken.contains(&id) {
            id = ProfileId(format!("{}-{n}", preset.id));
            n += 1;
        }
        let mut p = Profile {
            id,
            name,
            is_default: false,
            matcher: Matcher::default(),
            asr_provider: None,
            llm_provider: None,
            language_hint: None,
            prompt_addition: String::new(),
            few_shot: vec![],
            steps: vec![],
            preset: None,
            asr_dictionary_hint: true,
            llm_dictionary_terms: true,
            llm_window_title: true,
            llm_short_fallback: true,
        };
        p.reset_to_preset(preset);
        p
    }

    /// Makes the profile a copy of the template again: the engines, the
    /// applications it is used in, the language, the steps, the switches,
    /// the writing style and the examples. Only the id, the name and
    /// whether it is the default stay; the profile records the preset.
    pub fn reset_to_preset(&mut self, preset: &ProfilePreset) {
        self.matcher = Matcher {
            app_ids: preset.app_ids.iter().map(|a| a.to_string()).collect(),
            url_patterns: vec![],
        };
        self.asr_provider = preset.asr.map(|id| ProviderId(id.to_string()));
        self.llm_provider = preset.llm.map(|id| ProviderId(id.to_string()));
        self.language_hint = preset.language_hint.map(str::to_string);
        self.prompt_addition = preset.prompt_addition.to_string();
        self.few_shot = vec![];
        self.steps = Config::default_steps();
        self.asr_dictionary_hint = true;
        self.llm_dictionary_terms = true;
        self.llm_window_title = true;
        self.llm_short_fallback = true;
        self.preset = Some(preset.id.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    fn profiles() -> Vec<Profile> {
        let base = Config::first_run().profiles.remove(0);
        let mut slack = base.clone();
        slack.id = ProfileId("slack".into());
        slack.is_default = false;
        slack.matcher = Matcher {
            app_ids: vec!["com.tinyspeck.slackmacgap".into()],
            url_patterns: vec![],
        };
        let mut gh = base.clone();
        gh.id = ProfileId("github".into());
        gh.is_default = false;
        gh.matcher = Matcher {
            app_ids: vec![],
            url_patterns: vec!["GitHub.com".into()],
        };
        vec![base, slack, gh]
    }

    #[test]
    fn override_wins_then_app_id_then_url_then_default() {
        let ps = profiles();
        let app = AppInfo {
            app_id: Some("com.tinyspeck.slackmacgap".into()),
            ..Default::default()
        };
        assert_eq!(
            resolve(&ps, &app, Some(&ProfileId("github".into()))).id.0,
            "github"
        );
        assert_eq!(
            resolve(&ps, &app, Some(&ProfileId("missing".into()))).id.0,
            "slack"
        );
        assert_eq!(resolve(&ps, &app, None).id.0, "slack");
        let web = AppInfo {
            app_id: Some("com.apple.Safari".into()),
            url: Some("https://github.com/x".into()),
            ..Default::default()
        };
        assert_eq!(resolve(&ps, &web, None).id.0, "github");
        assert_eq!(resolve(&ps, &AppInfo::default(), None).id.0, "default");
    }

    #[test]
    fn empty_matcher_never_matches() {
        let ps = profiles();
        let app = AppInfo {
            app_id: Some("".into()),
            url: Some("".into()),
            ..Default::default()
        };
        assert_eq!(resolve(&ps, &app, None).id.0, "default");
    }

    #[test]
    fn presets_have_unique_ids_and_apps() {
        let ids: Vec<&str> = presets().iter().map(|p| p.id).collect();
        assert_eq!(ids, ["casual", "formal", "verbatim", "blank"]);
        for p in presets() {
            if p.id == "casual" || p.id == "blank" {
                assert!(p.app_ids.is_empty());
                assert!(p.prompt_addition.is_empty());
            } else {
                assert!(!p.prompt_addition.trim().is_empty());
            }
            assert_eq!(p.asr, None, "every template follows the default");
            assert_eq!(p.llm, None);
        }
        assert!(!preset("formal").app_ids.is_empty());
        assert!(preset("verbatim").app_ids.is_empty());
        assert_eq!(
            preset("casual").tuned_with(),
            Some((MODEL_ID_DEFAULT_ASR, MODEL_ID_DEFAULT_LLM))
        );
        assert_eq!(preset("blank").tuned_with(), None);
    }

    fn preset(id: &str) -> &'static ProfilePreset {
        super::preset(id).unwrap()
    }

    #[test]
    fn the_factory_profile_is_the_casual_template() {
        let d = Config::factory_profile();
        let c = Profile::from_preset(preset("casual"), "x".into(), &[]);
        assert_eq!(d.asr_provider, c.asr_provider);
        assert_eq!(d.llm_provider, c.llm_provider);
        assert_eq!(d.steps, c.steps);
        assert_eq!(d.prompt_addition, c.prompt_addition);
        assert_eq!(d.preset.as_deref(), Some("casual"));
        assert!(d.is_default);
        // The untuned template is offered last.
        assert_eq!(presets().last().map(|p| p.id), Some("blank"));
    }

    #[test]
    fn reset_to_preset_keeps_only_the_identity() {
        let c = Config::first_run();
        let mut p = c.profiles[0].clone();
        p.id = ProfileId("mail".into());
        p.name = "郵件".into();
        p.is_default = false;
        p.matcher.app_ids = vec!["com.tinyspeck.slackmacgap".into()];
        p.matcher.url_patterns = vec!["example.com".into()];
        p.prompt_addition = "tuned".into();
        p.language_hint = None;
        p.llm_short_fallback = false;
        p.steps = vec![crate::config::StepSpec::Asr];
        p.asr_provider = Some(ProviderId("other".into()));
        p.llm_provider = None;
        p.few_shot = vec![crate::config::Example {
            raw: "a".into(),
            refined: "b".into(),
        }];
        p.preset = None;
        p.reset_to_preset(preset("formal"));
        assert_eq!(p.id, ProfileId("mail".into()));
        assert_eq!(p.name, "郵件");
        assert!(!p.is_default);
        assert_eq!(
            p.matcher.app_ids,
            ["com.apple.mail", "com.microsoft.Outlook"]
        );
        assert!(p.matcher.url_patterns.is_empty());
        assert_eq!(p.prompt_addition, preset("formal").prompt_addition);
        assert_eq!(p.preset.as_deref(), Some("formal"));
        assert_eq!(p.language_hint.as_deref(), Some("zh-TW"));
        assert!(p.llm_short_fallback);
        assert_eq!(p.steps, Config::default_steps());
        assert_eq!(p.asr_provider, None, "follows the default");
        assert_eq!(p.llm_provider, None);
        assert!(p.few_shot.is_empty());
    }

    #[test]
    fn a_reset_returns_to_the_recorded_preset_else_the_factory_one() {
        let mut p = Config::first_run().profiles.remove(0);
        assert_eq!(reset_target(&p).map(|x| x.id), Some("casual"));
        p.preset = None;
        assert_eq!(reset_target(&p).map(|x| x.id), Some("casual"));
        p.preset = Some("verbatim".into());
        assert_eq!(reset_target(&p).map(|x| x.id), Some("verbatim"));
        p.preset = Some("retired".into());
        assert!(reset_target(&p).is_none());
    }

    #[test]
    fn from_preset_is_a_whole_copy_and_suffixes_taken_ids() {
        let taken = vec![ProfileId("mail".into()), ProfileId("mail-2".into())];
        let p = Profile::from_preset(preset("formal"), "郵件".into(), &taken);
        assert_eq!(p.id, ProfileId("formal".into()));
        assert_eq!(p.name, "郵件");
        assert!(!p.is_default);
        assert_eq!(p.asr_provider, None);
        assert_eq!(p.language_hint.as_deref(), Some("zh-TW"));
        assert_eq!(p.steps, Config::default_steps());
        assert_eq!(
            p.matcher.app_ids,
            ["com.apple.mail", "com.microsoft.Outlook"]
        );
        assert!(p.matcher.url_patterns.is_empty());
        assert!(p.few_shot.is_empty());
        assert_eq!(p.preset.as_deref(), Some("formal"));
        let taken = vec![ProfileId("formal".into()), ProfileId("formal-2".into())];
        let third = Profile::from_preset(preset("formal"), "x".into(), &taken);
        assert_eq!(third.id, ProfileId("formal-3".into()));
        let blank = Profile::from_preset(preset("blank"), "空白".into(), &[]);
        assert_eq!(blank.prompt_addition, "");
        assert!(blank.matcher.app_ids.is_empty());
    }
}
