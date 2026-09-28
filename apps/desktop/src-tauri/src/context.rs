//! Captures what the frontmost application is and, per the user's opt-ins,
//! what it shows, at the moment recording starts.

use kongwe_core::types::{AppInfo, Context};

use crate::platform::Platform;
use crate::settings::ContextOptIns;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InsertTarget {
    pub pid: i32,
    pub bundle_id: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Captured {
    pub app: AppInfo,
    pub context: Context,
    pub target: InsertTarget,
}

pub fn capture(platform: &dyn Platform, opt: &ContextOptIns) -> Captured {
    let Some(front) = platform.frontmost_app() else {
        return Captured::default();
    };
    let app = AppInfo {
        app_id: front.bundle_id.clone(),
        app_name: front.name.clone(),
        window_title: front.window_title.clone(),
        url: front.url.clone(),
    };
    let context = Context {
        app_name: opt.app_name.then(|| front.name.clone()).flatten(),
        window_title: opt
            .window_title
            .then(|| front.window_title.clone())
            .flatten(),
        selected_text: if opt.selected_text {
            platform.selected_text(front.pid)
        } else {
            None
        },
        clipboard: if opt.clipboard {
            platform.clipboard_text()
        } else {
            None
        },
    };
    let mut app = app;
    if !opt.url {
        app.url = None;
    }
    Captured {
        app,
        context,
        target: InsertTarget {
            pid: front.pid,
            bundle_id: front.bundle_id,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::fake::FakePlatform;
    use crate::platform::FrontmostApp;

    fn front() -> FrontmostApp {
        FrontmostApp {
            pid: 42,
            bundle_id: Some("com.apple.Safari".into()),
            name: Some("Safari".into()),
            window_title: Some("Kongwe".into()),
            url: Some("https://example.com/".into()),
        }
    }

    #[test]
    fn defaults_keep_app_fields_and_drop_text_fields() {
        let p = FakePlatform::new();
        *p.frontmost.lock().unwrap() = Some(front());
        *p.selected.lock().unwrap() = Some("sel".into());
        *p.clipboard.lock().unwrap() = Some("clip".into());
        let c = capture(&p, &ContextOptIns::default());
        assert_eq!(c.app.app_id.as_deref(), Some("com.apple.Safari"));
        assert_eq!(c.app.url.as_deref(), Some("https://example.com/"));
        assert_eq!(c.context.app_name.as_deref(), Some("Safari"));
        assert_eq!(c.context.window_title.as_deref(), Some("Kongwe"));
        assert_eq!(c.context.selected_text, None);
        assert_eq!(c.context.clipboard, None);
        assert_eq!(
            c.target,
            InsertTarget {
                pid: 42,
                bundle_id: Some("com.apple.Safari".into())
            }
        );
    }

    #[test]
    fn opt_ins_control_every_field() {
        let p = FakePlatform::new();
        *p.frontmost.lock().unwrap() = Some(front());
        *p.selected.lock().unwrap() = Some("sel".into());
        *p.clipboard.lock().unwrap() = Some("clip".into());
        let all_on = ContextOptIns {
            app_name: true,
            window_title: true,
            url: true,
            selected_text: true,
            clipboard: true,
        };
        let c = capture(&p, &all_on);
        assert_eq!(c.context.selected_text.as_deref(), Some("sel"));
        assert_eq!(c.context.clipboard.as_deref(), Some("clip"));
        let all_off = ContextOptIns {
            app_name: false,
            window_title: false,
            url: false,
            selected_text: false,
            clipboard: false,
        };
        let c = capture(&p, &all_off);
        assert_eq!(c.context, Context::default());
        assert_eq!(c.app.url, None);
        assert_eq!(
            c.app.app_id.as_deref(),
            Some("com.apple.Safari"),
            "app id always kept for profile matching"
        );
    }

    #[test]
    fn no_frontmost_app_yields_defaults() {
        let p = FakePlatform::new();
        assert_eq!(capture(&p, &ContextOptIns::default()), Captured::default());
    }
}
