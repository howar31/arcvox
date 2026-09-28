//! Global and local NSEvent monitors for `flagsChanged` (the fn key) and
//! `keyDown`. Global monitors need the process to be trusted for
//! Accessibility. Must be installed on the main thread; the handler runs
//! on the main thread and only forwards to a channel.

use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::time::Instant;

use block2::RcBlock;
use objc2_app_kit::{NSEvent, NSEventMask, NSEventModifierFlags, NSEventType};

use crate::platform::ModifierEvent;

/// kVK_Function from Carbon's `HIToolbox/Events.h`. macOS reports this same
/// code for the Globe key on keyboards that relabel fn to it, so this is
/// the one code that actually means "the fn/Globe key changed", as opposed
/// to some other modifier also flipping the `Function` flag bit.
const KVK_FUNCTION: u16 = 63;

struct Forwarder {
    tx: Sender<ModifierEvent>,
    fn_down: AtomicBool,
}

impl Forwarder {
    fn handle(&self, event: &NSEvent) {
        match event.r#type() {
            NSEventType::FlagsChanged => {
                if event.keyCode() != KVK_FUNCTION {
                    return;
                }
                let down = event
                    .modifierFlags()
                    .contains(NSEventModifierFlags::Function);
                if down != self.fn_down.swap(down, Ordering::SeqCst) {
                    // Stamped here, at the moment AppKit delivered the
                    // event, rather than later when the controller thread
                    // gets around to handling it.
                    let at = Instant::now();
                    let _ = self.tx.send(if down {
                        ModifierEvent::FnDown { at }
                    } else {
                        ModifierEvent::FnUp { at }
                    });
                }
            }
            NSEventType::KeyDown => {
                let key_code = event.keyCode();
                let _ = self.tx.send(ModifierEvent::KeyDown {
                    key_code,
                    at: Instant::now(),
                });
            }
            _ => {}
        }
    }
}

/// Installs both monitors and leaks their handles: they live as long as
/// the process. Returns `false` when the global monitor could not be
/// installed (the process is not trusted).
pub fn install(tx: Sender<ModifierEvent>) -> bool {
    let fwd = Arc::new(Forwarder {
        tx,
        fn_down: AtomicBool::new(false),
    });
    let mask = NSEventMask::FlagsChanged | NSEventMask::KeyDown;

    let global = {
        let fwd = fwd.clone();
        RcBlock::new(move |event: NonNull<NSEvent>| {
            // SAFETY: AppKit guarantees the event pointer handed to a
            // monitor block is non-null and valid for the duration of
            // this call.
            fwd.handle(unsafe { event.as_ref() });
        })
    };
    let handle = NSEvent::addGlobalMonitorForEventsMatchingMask_handler(mask, &global);
    let Some(handle) = handle else {
        return false;
    };
    std::mem::forget(handle);
    std::mem::forget(global);

    let local = {
        let fwd = fwd.clone();
        RcBlock::new(move |event: NonNull<NSEvent>| -> *mut NSEvent {
            // SAFETY: same guarantee as the global monitor block above.
            fwd.handle(unsafe { event.as_ref() });
            event.as_ptr()
        })
    };
    // SAFETY: `install` is documented to run on the main thread, which
    // AppKit requires for registering an event monitor; the handler block
    // above returns promptly and never unwinds across the ObjC boundary.
    if let Some(handle) =
        unsafe { NSEvent::addLocalMonitorForEventsMatchingMask_handler(mask, &local) }
    {
        std::mem::forget(handle);
    }
    std::mem::forget(local);
    true
}
