//! Tells when the pointer comes over a window and when it leaves it,
//! whether or not the app is in front. A window that never becomes key
//! does not get this from its web content.

use std::ffi::c_void;

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{class, define_class, msg_send, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_foundation::{NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize};

/// Called with true when the pointer comes over the window, false when it leaves.
pub type Report = Box<dyn Fn(bool)>;

// NSTrackingMouseEnteredAndExited, NSTrackingActiveAlways, NSTrackingInVisibleRect.
const OPTIONS: usize = 0x01 | 0x80 | 0x200;

struct Ivars {
    report: Report,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "KongwePointerWatcher"]
    #[thread_kind = MainThreadOnly]
    #[ivars = Ivars]
    struct Watcher;

    unsafe impl NSObjectProtocol for Watcher {}

    impl Watcher {
        #[unsafe(method(mouseEntered:))]
        fn entered(&self, _event: &AnyObject) {
            (self.ivars().report)(true);
        }

        #[unsafe(method(mouseExited:))]
        fn exited(&self, _event: &AnyObject) {
            (self.ivars().report)(false);
        }
    }
);

/// Watches the whole content of the window `ns_window` points to, for as
/// long as the process lives. False when it could not be set up; this must
/// run on the main thread.
pub fn watch(ns_window: *mut c_void, report: Report) -> bool {
    let Some(mtm) = MainThreadMarker::new() else {
        return false;
    };
    let window = ns_window.cast::<AnyObject>();
    if window.is_null() {
        return false;
    }
    let this = Watcher::alloc(mtm).set_ivars(Ivars { report });
    // SAFETY: a plain NSObject initialiser on a fresh allocation.
    let watcher: Retained<Watcher> = unsafe { msg_send![super(this), init] };
    // SAFETY: `window` is a live NSWindow and this is the main thread; the
    // area is retained by the view, and its owner is kept alive below.
    unsafe {
        let view: *mut AnyObject = msg_send![window, contentView];
        if view.is_null() {
            return false;
        }
        // The rectangle is not used: the area follows the view's visible part.
        let rect = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(0.0, 0.0));
        let area: *mut AnyObject = msg_send![class!(NSTrackingArea), alloc];
        let area: *mut AnyObject = msg_send![
            area,
            initWithRect: rect,
            options: OPTIONS,
            owner: &*watcher,
            userInfo: std::ptr::null::<AnyObject>(),
        ];
        let _: () = msg_send![view, addTrackingArea: area];
        let _: () = msg_send![area, release];
    }
    // The area does not keep its owner; the widget's window is never closed.
    std::mem::forget(watcher);
    true
}
