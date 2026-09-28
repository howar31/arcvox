//! Picks the profile for a session: explicit override, then first matcher
//! hit, then the default profile.

use crate::config::Profile;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, Matcher};

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
}
