//! Picks the profile for a session: explicit override, then first matcher
//! hit, then the default profile.

use crate::config::{
    Config, Matcher, Profile, StepSpec, FACTORY_PRESET, MODEL_ID_DEFAULT_ASR, MODEL_ID_DEFAULT_LLM,
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
/// one, and a reset makes a profile a copy of its own again. A tuned
/// template binds the engines its writing style was tuned with (the ones
/// it names, else the factory pair), since the style is written for them;
/// the untuned template follows the default engines. A named engine is
/// one the app makes (a catalogue model's, a platform service's), created
/// when missing, and the catalogue must carry it
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
    /// Every writing style the template has shipped with, oldest first; the
    /// last one is the current. A profile remembers the index it last took
    /// (`Profile::preset_revision`), so a later build knows whether the
    /// profile still carries a shipped text or one the user wrote. Texts
    /// are only ever appended here: an installation may carry any of them.
    pub additions: &'static [&'static str],
    /// Whether the rules step removes a full stop that ends the text.
    pub trailing_stop: bool,
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
        // Shipped without a style first; the chat-message style came with
        // the tidy rules.
        additions: &[
            "",
            "這是聊天訊息的寫法：輸出的最後一個字之後不加句號，整段以最後一個字或問號結束；中間句子之間的句號照常。",
        ],
        trailing_stop: true,
    },
    ProfilePreset {
        id: "formal",
        asr: None,
        llm: None,
        tuned: true,
        language_hint: Some("zh-TW"),
        app_ids: &["com.apple.mail", "com.microsoft.Outlook"],
        additions: &[
            "這是正式的書面文字。使用完整句子與書面用語，句尾加句點，依語意分段；稱呼與結尾問候語依原稿保留，不自行增加。",
        ],
        trailing_stop: false,
    },
    ProfilePreset {
        id: "verbatim",
        asr: None,
        llm: None,
        tuned: true,
        language_hint: Some("zh-TW"),
        app_ids: &[],
        additions: &[
            "逐字記錄：每個字都照原稿保留，不吸收自我修正、不刪語助詞與重複、不改用詞、不翻譯；只加入標點與分段。",
        ],
        trailing_stop: false,
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
        additions: &[""],
        trailing_stop: false,
    },
];

pub fn presets() -> &'static [ProfilePreset] {
    &PRESETS
}

impl ProfilePreset {
    /// The writing style the template ships with now.
    pub fn prompt_addition(&self) -> &'static str {
        self.additions.last().copied().unwrap_or("")
    }

    /// The revision of the current writing style: how many times it has
    /// changed since the template first shipped.
    pub fn revision(&self) -> u32 {
        self.additions.len().saturating_sub(1) as u32
    }

    /// Whether `text` is a writing style the template shipped with from
    /// revision `since` up to, but not including, the current one: the
    /// text a profile at `since` took and never edited.
    pub fn shipped_since(&self, since: u32, text: &str) -> bool {
        let current = self.revision() as usize;
        self.additions
            .iter()
            .enumerate()
            .any(|(i, t)| i >= since as usize && i < current && *t == text)
    }

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
            preset_revision: 0,
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
        // The writing style was tuned with these engines, so the profile
        // names them; the user may switch it to the default afterwards.
        let (asr, llm) = match preset.tuned_with() {
            Some((asr, llm)) => (Some(asr), Some(llm)),
            None => (preset.asr, preset.llm),
        };
        self.asr_provider = asr.map(|id| ProviderId(id.to_string()));
        self.llm_provider = llm.map(|id| ProviderId(id.to_string()));
        self.language_hint = preset.language_hint.map(str::to_string);
        self.prompt_addition = preset.prompt_addition().to_string();
        self.preset_revision = preset.revision();
        self.few_shot = vec![];
        self.steps = Config::default_steps();
        for step in &mut self.steps {
            if let StepSpec::Rules { trailing_stop, .. } = step {
                *trailing_stop = preset.trailing_stop;
            }
        }
        self.asr_dictionary_hint = true;
        self.llm_dictionary_terms = true;
        self.llm_window_title = true;
        self.llm_short_fallback = true;
        self.preset = Some(preset.id.to_string());
    }

    /// The template whose writing style has changed since the profile last
    /// took it; None when the profile is current or follows no known
    /// template.
    pub fn moved_template(&self) -> Option<&'static ProfilePreset> {
        reset_target(self).filter(|t| self.preset_revision < t.revision())
    }

    /// Takes the template's current writing style, as a profile made from
    /// it now would have.
    pub fn take_template_style(&mut self, preset: &ProfilePreset) {
        self.prompt_addition = preset.prompt_addition().to_string();
        self.preset_revision = preset.revision();
    }

    /// Keeps the profile's own writing style and stops asking about the
    /// template's current one.
    pub fn keep_own_style(&mut self, preset: &ProfilePreset) {
        self.preset_revision = preset.revision();
    }
}

impl Config {
    /// Brings every profile that still carries a shipped writing style up
    /// to its template's current one. A profile whose style is the text its
    /// template shipped at the revision the profile took, or any shipped
    /// since, was never edited and takes the current text; one already on
    /// the current text only records so; one with a text of the user's own
    /// is left alone, with its revision behind, for the user to decide
    /// (`Profile::moved_template`). Returns the profiles whose style changed.
    pub fn refresh_template_styles(&mut self) -> Vec<ProfileId> {
        let mut changed = Vec::new();
        for profile in &mut self.profiles {
            let Some(preset) = profile.moved_template() else {
                continue;
            };
            if profile.prompt_addition == preset.prompt_addition() {
                profile.keep_own_style(preset);
            } else if preset.shipped_since(profile.preset_revision, &profile.prompt_addition) {
                profile.take_template_style(preset);
                changed.push(profile.id.clone());
            }
        }
        changed
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
            if p.id == "blank" {
                assert!(
                    p.prompt_addition().is_empty(),
                    "the untuned template has no style"
                );
            } else {
                assert!(!p.prompt_addition().trim().is_empty());
            }
            assert!(
                !p.additions.is_empty(),
                "every template has a current style"
            );
            if p.id == "casual" || p.id == "blank" {
                assert!(p.app_ids.is_empty());
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
    fn the_casual_style_drops_the_last_full_stop_and_keeps_question_marks() {
        let style = preset("casual").prompt_addition();
        assert!(style.contains("最後一個字之後不加句號"), "{style}");
        assert!(style.contains("問號"), "{style}");
        assert!(style.contains("中間句子之間的句號照常"), "{style}");
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
        assert_eq!(p.prompt_addition, preset("formal").prompt_addition());
        assert_eq!(p.preset_revision, preset("formal").revision());
        assert_eq!(p.preset.as_deref(), Some("formal"));
        assert_eq!(p.language_hint.as_deref(), Some("zh-TW"));
        assert!(p.llm_short_fallback);
        assert_eq!(p.steps.len(), Config::default_steps().len());
        assert!(matches!(
            p.steps.last(),
            Some(StepSpec::Rules {
                trailing_stop: false,
                ..
            })
        ));
        assert_eq!(
            p.asr_provider.as_ref().map(|p| p.0.as_str()),
            Some(MODEL_ID_DEFAULT_ASR),
            "a tuned template binds the engines it was tuned with"
        );
        assert_eq!(
            p.llm_provider.as_ref().map(|p| p.0.as_str()),
            Some(MODEL_ID_DEFAULT_LLM)
        );
        assert!(p.few_shot.is_empty());
    }

    #[test]
    fn the_blank_template_follows_the_default_engines() {
        let p = Profile::from_preset(preset("blank"), "x".into(), &[]);
        assert_eq!(p.asr_provider, None);
        assert_eq!(p.llm_provider, None);
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
        assert_eq!(
            p.asr_provider.as_ref().map(|p| p.0.as_str()),
            Some(MODEL_ID_DEFAULT_ASR)
        );
        assert_eq!(p.language_hint.as_deref(), Some("zh-TW"));
        assert_eq!(p.steps.len(), Config::default_steps().len());
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

    #[test]
    fn the_casual_template_remembers_its_first_empty_style() {
        let casual = preset("casual");
        assert_eq!(casual.revision(), 1);
        assert!(casual.shipped_since(0, ""), "the first shipped style");
        assert!(
            !casual.shipped_since(0, casual.prompt_addition()),
            "the current text is not an earlier one"
        );
        assert!(
            !casual.shipped_since(1, ""),
            "a profile at revision 1 never took the empty style"
        );
        assert!(!casual.shipped_since(0, "mine"));
        assert_eq!(preset("formal").revision(), 0);
        assert_eq!(preset("blank").revision(), 0);
    }

    /// A profile written before revisions existed reads as revision 0.
    fn old_profile(preset: Option<&str>, addition: &str) -> Profile {
        let mut p = Config::first_run().profiles.remove(0);
        p.preset = preset.map(str::to_string);
        p.preset_revision = 0;
        p.prompt_addition = addition.to_string();
        p
    }

    #[test]
    fn an_unedited_profile_takes_the_template_s_new_style() {
        let casual = preset("casual");
        let mut c = Config::first_run();
        c.profiles = vec![old_profile(Some("casual"), "")];
        assert!(c.profiles[0].moved_template().is_some());
        let changed = c.refresh_template_styles();
        assert_eq!(changed, [c.profiles[0].id.clone()]);
        assert_eq!(c.profiles[0].prompt_addition, casual.prompt_addition());
        assert_eq!(c.profiles[0].preset_revision, casual.revision());
        assert!(c.profiles[0].moved_template().is_none());
        assert!(
            c.refresh_template_styles().is_empty(),
            "nothing moves twice"
        );
    }

    #[test]
    fn a_profile_without_a_recorded_preset_follows_the_factory_template() {
        let mut c = Config::first_run();
        c.profiles = vec![old_profile(None, "")];
        c.refresh_template_styles();
        assert_eq!(
            c.profiles[0].prompt_addition,
            preset("casual").prompt_addition()
        );
    }

    #[test]
    fn an_edited_profile_keeps_its_style_and_stays_marked_until_the_user_decides() {
        let casual = preset("casual");
        let mut c = Config::first_run();
        c.profiles = vec![old_profile(Some("casual"), "我的寫法")];
        assert!(c.refresh_template_styles().is_empty());
        assert_eq!(c.profiles[0].prompt_addition, "我的寫法");
        assert_eq!(c.profiles[0].moved_template().map(|p| p.id), Some("casual"));
        let mut kept = c.profiles[0].clone();
        kept.keep_own_style(casual);
        assert_eq!(kept.prompt_addition, "我的寫法");
        assert!(kept.moved_template().is_none());
        let mut taken = c.profiles[0].clone();
        taken.take_template_style(casual);
        assert_eq!(taken.prompt_addition, casual.prompt_addition());
        assert!(taken.moved_template().is_none());
    }

    #[test]
    fn a_profile_already_on_the_current_text_only_records_the_revision() {
        let casual = preset("casual");
        let mut c = Config::first_run();
        c.profiles = vec![old_profile(Some("casual"), casual.prompt_addition())];
        assert!(c.refresh_template_styles().is_empty());
        assert_eq!(c.profiles[0].preset_revision, casual.revision());
    }

    #[test]
    fn templates_that_never_changed_and_unknown_ones_move_nothing() {
        let mut c = Config::first_run();
        c.profiles = vec![
            old_profile(Some("formal"), preset("formal").prompt_addition()),
            old_profile(Some("blank"), ""),
            old_profile(Some("mail"), "這是一封電子郵件的內容。"),
        ];
        for p in &c.profiles {
            assert!(
                p.moved_template().is_none(),
                "{}",
                p.preset.as_deref().unwrap()
            );
        }
        assert!(c.refresh_template_styles().is_empty());
    }
}
