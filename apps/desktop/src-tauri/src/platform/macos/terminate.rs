//! Lets the app decide whether a system quit request (the Dock menu, a quit
//! Apple event) ends it. The end of the login session is always allowed.

use std::ffi::c_char;
use std::sync::OnceLock;

use objc2::runtime::{AnyClass, AnyObject, Imp, Sel};
use objc2::{class, msg_send, sel};

const TERMINATE_CANCEL: usize = 0;
const TERMINATE_NOW: usize = 1;

/// Apple event attribute that carries the reason of a quit request.
const QUIT_REASON: u32 = u32::from_be_bytes(*b"why?");

type Decision = Box<dyn Fn() -> bool + Send + Sync>;
static DECISION: OnceLock<Decision> = OnceLock::new();

/// Routes quit requests to `allow`: `true` lets the app terminate. Must run
/// on the main thread after the application delegate is set.
pub fn install(allow: impl Fn() -> bool + Send + Sync + 'static) {
    if DECISION.set(Box::new(allow)).is_err() {
        return;
    }
    // SAFETY: the shared application and its delegate are valid objects on
    // the main thread; the method is added with the signature AppKit expects
    // for `applicationShouldTerminate:` (NSUInteger return, one object).
    unsafe {
        let app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
        let delegate: *mut AnyObject = msg_send![app, delegate];
        if delegate.is_null() {
            return;
        }
        let class = (*delegate).class() as *const AnyClass as *mut AnyClass;
        let imp: Imp = std::mem::transmute(
            should_terminate
                as unsafe extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject) -> usize,
        );
        let _ = objc2::ffi::class_addMethod(
            class,
            sel!(applicationShouldTerminate:),
            imp,
            c"Q@:@".as_ptr() as *const c_char,
        );
    }
}

unsafe extern "C-unwind" fn should_terminate(
    _this: *mut AnyObject,
    _cmd: Sel,
    _sender: *mut AnyObject,
) -> usize {
    if ends_session(quit_reason()) || DECISION.get().is_none_or(|allow| allow()) {
        TERMINATE_NOW
    } else {
        TERMINATE_CANCEL
    }
}

/// The reason code of the quit request being handled; 0 when it has none.
fn quit_reason() -> u32 {
    // SAFETY: plain messages to the shared Apple event manager; a nil event
    // or attribute is checked before it is used.
    unsafe {
        let manager: *mut AnyObject =
            msg_send![class!(NSAppleEventManager), sharedAppleEventManager];
        let event: *mut AnyObject = msg_send![manager, currentAppleEvent];
        if event.is_null() {
            return 0;
        }
        let reason: *mut AnyObject = msg_send![event, attributeDescriptorForKeyword: QUIT_REASON];
        if reason.is_null() {
            return 0;
        }
        msg_send![reason, enumCodeValue]
    }
}

/// Logging out, restarting and shutting down give a reason; a quit asked for
/// by the user gives none.
fn ends_session(reason: u32) -> bool {
    reason != 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_request_with_a_reason_ends_the_session() {
        assert!(!ends_session(0));
        assert!(ends_session(u32::from_be_bytes(*b"logo")));
        assert!(ends_session(u32::from_be_bytes(*b"shut")));
    }
}
