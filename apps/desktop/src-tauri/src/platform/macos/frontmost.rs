//! Frontmost application via NSWorkspace, plus its focused window's title
//! and (browsers) document URL via AX.

use objc2_app_kit::NSWorkspace;

use super::ax::AxElement;
use crate::platform::FrontmostApp;

const AX_FOCUSED_WINDOW: &str = "AXFocusedWindow";
const AX_TITLE: &str = "AXTitle";
const AX_DOCUMENT: &str = "AXDocument";
const AX_URL: &str = "AXURL";

pub fn frontmost_app(ax_trusted: bool) -> Option<FrontmostApp> {
    let ws = NSWorkspace::sharedWorkspace();
    let app = ws.frontmostApplication()?;
    let pid = app.processIdentifier();
    let bundle_id = app.bundleIdentifier().map(|s| s.to_string());
    let name = app.localizedName().map(|s| s.to_string());
    let (window_title, url) = if ax_trusted {
        window_title_and_url(pid, bundle_id.as_deref())
    } else {
        (None, None)
    };
    Some(FrontmostApp {
        pid,
        bundle_id,
        name,
        window_title,
        url,
    })
}

fn is_browser(bundle_id: Option<&str>) -> bool {
    matches!(
        bundle_id,
        Some(
            "com.apple.Safari"
                | "com.google.Chrome"
                | "com.google.Chrome.canary"
                | "com.microsoft.edgemac"
                | "com.brave.Browser"
                | "org.mozilla.firefox"
                | "company.thebrowser.Browser"
                | "com.vivaldi.Vivaldi"
        )
    )
}

fn window_title_and_url(pid: i32, bundle_id: Option<&str>) -> (Option<String>, Option<String>) {
    let app = AxElement::application(pid);
    let Some(window) = app.attribute_element(AX_FOCUSED_WINDOW) else {
        return (None, None);
    };
    let title = window.attribute_string(AX_TITLE).filter(|s| !s.is_empty());
    let url = if is_browser(bundle_id) {
        window
            .attribute_string(AX_DOCUMENT)
            .or_else(|| window.attribute_string(AX_URL))
            .filter(|u| u.starts_with("http"))
    } else {
        None
    };
    (title, url)
}
