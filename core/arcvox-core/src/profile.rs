//! Picks the profile for a session: explicit override, then first matcher
//! hit, then the default profile.

use crate::config::{
    Config, Matcher, Profile, ProviderConfig, FACTORY_ASR, FACTORY_LLM, FACTORY_PRESET,
};
use crate::types::{AppInfo, ProfileId};

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

/// A built-in starting point for a per-app profile.
#[derive(Debug, Clone, Copy)]
pub struct ProfilePreset {
    pub id: &'static str,
    pub app_ids: &'static [&'static str],
    pub prompt_addition: &'static str,
    /// Takes the factory profile's settings instead of the default
    /// profile's: a clean copy of how a new installation starts.
    pub factory: bool,
}

static PRESETS: [ProfilePreset; 3] = [
    ProfilePreset {
        id: "casual",
        app_ids: &[],
        prompt_addition: "",
        factory: true,
    },
    ProfilePreset {
        id: "formal",
        app_ids: &["com.apple.mail", "com.microsoft.Outlook"],
        prompt_addition: "這是正式的書面文字。使用完整句子與書面用語，句尾加句點，依語意分段；稱呼與結尾問候語依原稿保留，不自行增加。",
        factory: false,
    },
    ProfilePreset {
        id: "verbatim",
        app_ids: &[],
        prompt_addition: "逐字記錄：每個字都照原稿保留，不吸收自我修正、不刪語助詞與重複、不改用詞、不翻譯；只加入標點與分段。",
        factory: false,
    },
];

pub fn presets() -> &'static [ProfilePreset] {
    &PRESETS
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
    /// Builds a profile from a preset, reusing the provider pair, language
    /// hint, steps and switches of `base` (the factory profile's for a
    /// factory preset, with `base`'s providers when the factory ones are
    /// gone). The id gets a numeric suffix while taken.
    pub fn from_preset(
        preset: &ProfilePreset,
        name: String,
        base: &Profile,
        providers: &[ProviderConfig],
        taken: &[ProfileId],
    ) -> Profile {
        let factory = Config::factory_profile();
        let mut source = if preset.factory { &factory } else { base }.clone();
        if preset.factory && !factory_providers_present(providers) {
            source.asr_provider = base.asr_provider.clone();
            source.llm_provider = base.llm_provider.clone();
        }
        let base = &source;
        let mut id = ProfileId(preset.id.to_string());
        let mut n = 2;
        while taken.contains(&id) {
            id = ProfileId(format!("{}-{n}", preset.id));
            n += 1;
        }
        Profile {
            id,
            name,
            is_default: false,
            matcher: Matcher {
                app_ids: preset.app_ids.iter().map(|a| a.to_string()).collect(),
                url_patterns: vec![],
            },
            asr_provider: base.asr_provider.clone(),
            llm_provider: base.llm_provider.clone(),
            language_hint: base.language_hint.clone(),
            prompt_addition: preset.prompt_addition.to_string(),
            few_shot: vec![],
            steps: base.steps.clone(),
            preset: Some(preset.id.to_string()),
            asr_dictionary_hint: base.asr_dictionary_hint,
            llm_dictionary_terms: base.llm_dictionary_terms,
            llm_window_title: base.llm_window_title,
            llm_short_fallback: base.llm_short_fallback,
        }
    }

    /// Puts the profile's settings back to those of `preset`: the factory
    /// profile's steps, switches, language hint and examples, and the
    /// preset's prompt addition. The id, the name, whether it is the default
    /// and where it applies stay; the profile records the preset. The
    /// providers go back to the factory pair only while both are still
    /// configured; returns whether they did.
    pub fn reset_to_preset(
        &mut self,
        preset: &ProfilePreset,
        providers: &[ProviderConfig],
    ) -> bool {
        let factory = Config::factory_profile();
        self.steps = factory.steps;
        self.language_hint = factory.language_hint;
        self.prompt_addition = preset.prompt_addition.to_string();
        self.few_shot = factory.few_shot;
        self.asr_dictionary_hint = factory.asr_dictionary_hint;
        self.llm_dictionary_terms = factory.llm_dictionary_terms;
        self.llm_window_title = factory.llm_window_title;
        self.llm_short_fallback = factory.llm_short_fallback;
        self.preset = Some(preset.id.to_string());
        let engines = factory_providers_present(providers);
        if engines {
            self.asr_provider = factory.asr_provider;
            self.llm_provider = factory.llm_provider;
        }
        engines
    }
}

/// Whether the two providers the factory profile names are configured.
pub fn factory_providers_present(providers: &[ProviderConfig]) -> bool {
    let has = |id: &str| providers.iter().any(|p| p.id.0 == id);
    has(FACTORY_ASR) && has(FACTORY_LLM)
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
        assert_eq!(ids, ["casual", "formal", "verbatim"]);
        for p in presets() {
            if p.factory {
                assert!(p.app_ids.is_empty());
                assert!(p.prompt_addition.is_empty());
            } else {
                assert!(!p.prompt_addition.trim().is_empty());
            }
        }
        assert!(!preset("formal").app_ids.is_empty());
        assert!(preset("verbatim").app_ids.is_empty());
    }

    fn preset(id: &str) -> &'static ProfilePreset {
        super::preset(id).unwrap()
    }

    #[test]
    fn the_casual_preset_is_a_copy_of_the_factory_profile() {
        let c = Config::first_run();
        let mut base = c.profiles[0].clone();
        base.prompt_addition = "tuned".into();
        base.llm_window_title = false;
        base.steps = vec![crate::config::StepSpec::Asr];
        let p = Profile::from_preset(preset("casual"), "休閒".into(), &base, &c.providers, &[]);
        let factory = Config::factory_profile();
        assert_eq!(p.id, ProfileId("casual".into()));
        assert!(!p.is_default);
        assert!(p.matcher.app_ids.is_empty());
        assert_eq!(p.preset.as_deref(), Some("casual"));
        assert_eq!(p.prompt_addition, "");
        assert_eq!(p.steps, factory.steps);
        assert!(p.llm_window_title);
        assert_eq!(p.asr_provider, factory.asr_provider);
        // Without the factory providers the copy keeps the default profile's.
        let mut base2 = base.clone();
        base2.asr_provider = crate::types::ProviderId("other".into());
        let few: Vec<_> = c
            .providers
            .iter()
            .filter(|x| x.id.0 != FACTORY_ASR)
            .cloned()
            .collect();
        let q = Profile::from_preset(preset("casual"), "休閒".into(), &base2, &few, &[]);
        assert_eq!(q.asr_provider, base2.asr_provider);
    }

    #[test]
    fn reset_to_preset_keeps_identity_and_restores_engines_only_when_present() {
        let c = Config::first_run();
        let mut p = c.profiles[0].clone();
        p.id = ProfileId("mail".into());
        p.name = "郵件".into();
        p.is_default = false;
        p.matcher.app_ids = vec!["com.apple.mail".into()];
        p.prompt_addition = "tuned".into();
        p.language_hint = None;
        p.llm_short_fallback = false;
        p.steps = vec![crate::config::StepSpec::Asr];
        p.asr_provider = crate::types::ProviderId("other".into());
        p.preset = None;
        assert!(p.reset_to_preset(preset("formal"), &c.providers));
        let factory = Config::factory_profile();
        assert_eq!(p.id, ProfileId("mail".into()));
        assert_eq!(p.name, "郵件");
        assert!(!p.is_default);
        assert_eq!(p.matcher.app_ids, ["com.apple.mail"]);
        assert_eq!(p.prompt_addition, preset("formal").prompt_addition);
        assert_eq!(p.preset.as_deref(), Some("formal"));
        assert_eq!(p.language_hint, factory.language_hint);
        assert!(p.llm_short_fallback);
        assert_eq!(p.steps, factory.steps);
        assert_eq!(p.asr_provider, factory.asr_provider);
        let mut q = p.clone();
        q.asr_provider = crate::types::ProviderId("other".into());
        let few: Vec<_> = c
            .providers
            .iter()
            .filter(|x| x.id.0 != FACTORY_LLM)
            .cloned()
            .collect();
        assert!(!q.reset_to_preset(preset("casual"), &few));
        assert_eq!(q.asr_provider, crate::types::ProviderId("other".into()));
        assert_eq!(q.prompt_addition, "");
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
    fn from_preset_copies_the_base_pair_and_suffixes_taken_ids() {
        let base = Config::first_run().profiles.remove(0);
        let taken = vec![ProfileId("mail".into()), ProfileId("mail-2".into())];
        let p = Profile::from_preset(preset("formal"), "郵件".into(), &base, &[], &taken);
        assert_eq!(p.id, ProfileId("formal".into()));
        assert_eq!(p.name, "郵件");
        assert!(!p.is_default);
        assert_eq!(p.asr_provider, base.asr_provider);
        assert_eq!(p.llm_provider, base.llm_provider);
        assert_eq!(p.language_hint, base.language_hint);
        assert_eq!(p.steps, base.steps);
        assert_eq!(
            p.matcher.app_ids,
            ["com.apple.mail", "com.microsoft.Outlook"]
        );
        assert!(p.matcher.url_patterns.is_empty());
        assert!(p.few_shot.is_empty());
        assert_eq!(p.preset.as_deref(), Some("formal"));
        let taken = vec![ProfileId("formal".into()), ProfileId("formal-2".into())];
        let third = Profile::from_preset(preset("formal"), "x".into(), &base, &[], &taken);
        assert_eq!(third.id, ProfileId("formal-3".into()));
    }
}
