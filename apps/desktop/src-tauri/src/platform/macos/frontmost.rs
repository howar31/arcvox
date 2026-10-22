//! Frontmost application via NSWorkspace, plus its focused window's title
//! and web address via AX.

use objc2_app_kit::NSWorkspace;

use super::ax::AxElement;
use crate::platform::FrontmostApp;

const AX_FOCUSED_WINDOW: &str = "AXFocusedWindow";
const AX_TITLE: &str = "AXTitle";
const AX_DOCUMENT: &str = "AXDocument";
const AX_URL: &str = "AXURL";
const AX_PARENT: &str = "AXParent";
const AX_ROLE: &str = "AXRole";
const AX_WEB_AREA: &str = "AXWebArea";
/// Ancestors looked at above the focused element before giving up.
const WEB_AREA_SEARCH_DEPTH: usize = 40;

pub fn frontmost_app(ax_trusted: bool) -> Option<FrontmostApp> {
    let ws = NSWorkspace::sharedWorkspace();
    let app = ws.frontmostApplication()?;
    let pid = app.processIdentifier();
    let bundle_id = app.bundleIdentifier().map(|s| s.to_string());
    let name = app.localizedName().map(|s| s.to_string());
    let (window_title, url) = if ax_trusted {
        window_title_and_url(pid)
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

/// The address of the page the focus is in. Chromium browsers keep it on
/// the web area that holds the focused element, not on the window; Safari
/// and others name it as the window's document. A file address (an
/// editor's document) is left out.
fn window_title_and_url(pid: i32) -> (Option<String>, Option<String>) {
    let app = AxElement::application(pid);
    let Some(window) = app.attribute_element(AX_FOCUSED_WINDOW) else {
        return (None, None);
    };
    let title = window.attribute_string(AX_TITLE).filter(|s| !s.is_empty());
    let url = web_area_url(&app)
        .or_else(|| window.attribute_url(AX_DOCUMENT))
        .or_else(|| window.attribute_url(AX_URL))
        .filter(|u| u.starts_with("http://") || u.starts_with("https://"));
    (title, url)
}

/// `AXURL` of the nearest web area above the focused element.
fn web_area_url(app: &AxElement) -> Option<String> {
    let mut element = app.focused_element()?;
    for _ in 0..WEB_AREA_SEARCH_DEPTH {
        if element.attribute_string(AX_ROLE).as_deref() == Some(AX_WEB_AREA) {
            return element.attribute_url(AX_URL);
        }
        element = element.attribute_element(AX_PARENT)?;
    }
    None
}
