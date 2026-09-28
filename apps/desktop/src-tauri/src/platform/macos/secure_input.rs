//! Secure Input (password fields, Terminal `sudo`) blocks synthetic input
//! and reading of keystrokes; the client must not try to insert while it is on.

#[link(name = "Carbon", kind = "framework")]
extern "C" {
    fn IsSecureEventInputEnabled() -> u8;
}

pub fn is_active() -> bool {
    unsafe { IsSecureEventInputEnabled() != 0 }
}
