//! Whether the system lets this process record, and asking for it.

use block2::RcBlock;
use objc2::runtime::Bool;
use objc2::{class, msg_send};
use objc2_foundation::NSString;

use crate::platform::MicAccess;

#[link(name = "AVFoundation", kind = "framework")]
extern "C" {
    static AVMediaTypeAudio: &'static NSString;
}

/// The system's answer as it is now; asking it shows nothing to the user.
pub fn access() -> MicAccess {
    // SAFETY: a class method that takes the framework's media type constant
    // and returns an `AVAuthorizationStatus` (an `NSInteger`).
    let status: isize = unsafe {
        msg_send![
            class!(AVCaptureDevice),
            authorizationStatusForMediaType: AVMediaTypeAudio
        ]
    };
    match status {
        0 => MicAccess::NotAsked,
        1 => MicAccess::Restricted,
        2 => MicAccess::Denied,
        _ => MicAccess::Granted,
    }
}

/// Shows the system's request when it was never answered; `done` runs on a
/// thread of the system's once there is an answer.
pub fn request(done: Box<dyn Fn() + Send + Sync>) {
    let block = RcBlock::new(move |_granted: Bool| done());
    // SAFETY: the handler takes one `BOOL` and returns nothing; the system
    // copies the block and keeps it until it has called it.
    unsafe {
        let _: () = msg_send![
            class!(AVCaptureDevice),
            requestAccessForMediaType: AVMediaTypeAudio,
            completionHandler: &*block
        ];
    }
}
