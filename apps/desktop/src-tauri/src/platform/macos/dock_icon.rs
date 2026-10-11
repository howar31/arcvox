//! The Dock icon of the running app follows the system's light or dark
//! appearance. The icon in the bundle follows the system's icon style
//! instead, which does not change with the appearance.

use std::ptr::NonNull;

use block2::RcBlock;
use objc2::{AnyThread, MainThreadMarker};
use objc2_app_kit::{NSApplication, NSImage};
use objc2_foundation::{
    NSData, NSDistributedNotificationCenter, NSNotification, NSOperationQueue, NSString,
    NSUserDefaults,
};

const LIGHT: &[u8] = include_bytes!("../../../icons/dock-light.png");
const DARK: &[u8] = include_bytes!("../../../icons/dock-dark.png");

/// The system writes this setting only while the appearance is dark.
fn dark() -> bool {
    let key = NSString::from_str("AppleInterfaceStyle");
    NSUserDefaults::standardUserDefaults()
        .stringForKey(&key)
        .is_some_and(|style| style.to_string() == "Dark")
}

/// Sets the icon for the current appearance. The Dock takes the image only
/// while the app has a Dock tile, so the caller that switches the app to the
/// regular activation policy sets it again.
pub fn apply() {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let data = NSData::with_bytes(if dark() { DARK } else { LIGHT });
    let Some(image) = NSImage::initWithData(NSImage::alloc(), &data) else {
        return;
    };
    // SAFETY: called on the main thread with a live image, which the
    // application retains.
    unsafe { NSApplication::sharedApplication(mtm).setApplicationIconImage(Some(&image)) };
}

/// Sets the icon now and again on every change of the appearance. Call on
/// the main thread; the observer lives as long as the process.
pub fn follow_appearance() {
    apply();
    let name = NSString::from_str("AppleInterfaceThemeChangedNotification");
    let block = RcBlock::new(move |_: NonNull<NSNotification>| apply());
    let center = NSDistributedNotificationCenter::defaultCenter();
    let queue = NSOperationQueue::mainQueue();
    // SAFETY: the name is a live string for the call, the block is owned by
    // the observer the centre keeps, and both are kept for the life of the
    // process because the observer is never removed.
    let observer = unsafe {
        center.addObserverForName_object_queue_usingBlock(Some(&name), None, Some(&queue), &block)
    };
    std::mem::forget(observer);
}
