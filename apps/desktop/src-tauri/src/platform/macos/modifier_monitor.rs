//! Global and local NSEvent monitors for `flagsChanged` (the fn key) and
//! `keyDown`. Global monitors need the process to be trusted for
//! Accessibility. Must be installed on the main thread; the handler runs
//! on the main thread and only forwards to a channel.

use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;

use block2::RcBlock;
use objc2_app_kit::{NSEvent, NSEventMask, NSEventModifierFlags, NSEventType};

use crate::platform::ModifierEvent;

struct Forwarder {
    tx: Sender<ModifierEvent>,
    fn_down: AtomicBool,
}

impl Forwarder {
    fn handle(&self, event: &NSEvent) {
        match event.r#type() {
            NSEventType::FlagsChanged => {
                let down = event
                    .modifierFlags()
                    .contains(NSEventModifierFlags::Function);
                if down != self.fn_down.swap(down, Ordering::SeqCst) {
                    let _ = self.tx.send(if down {
                        ModifierEvent::FnDown
                    } else {
                        ModifierEvent::FnUp
                    });
                }
            }
            NSEventType::KeyDown => {
                let key_code = event.keyCode();
                let _ = self.tx.send(ModifierEvent::KeyDown { key_code });
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
            fwd.handle(unsafe { event.as_ref() });
            event.as_ptr()
        })
    };
    if let Some(handle) =
        unsafe { NSEvent::addLocalMonitorForEventsMatchingMask_handler(mask, &local) }
    {
        std::mem::forget(handle);
    }
    std::mem::forget(local);
    true
}
