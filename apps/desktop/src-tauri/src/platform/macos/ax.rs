//! Thin RAII layer over the Accessibility (AX) C API.

use std::ptr;

use accessibility_sys::{
    kAXErrorAttributeUnsupported, kAXErrorIllegalArgument, kAXErrorNoValue, kAXErrorSuccess,
    kAXFocusedUIElementAttribute, kAXSelectedTextAttribute, kAXTrustedCheckOptionPrompt, AXError,
    AXIsProcessTrustedWithOptions, AXUIElementCopyAttributeValue, AXUIElementCreateApplication,
    AXUIElementGetTypeID, AXUIElementIsAttributeSettable, AXUIElementRef,
    AXUIElementSetAttributeValue, AXUIElementSetMessagingTimeout,
};
use core_foundation::base::{CFGetTypeID, CFRelease, CFType, CFTypeRef, TCFType};
use core_foundation::boolean::CFBoolean;
use core_foundation::dictionary::CFDictionary;
use core_foundation::string::CFString;
use core_foundation::url::CFURL;

use crate::platform::PlatformError;

/// A hung target app must not block dictation; AX calls give up after this.
const MESSAGING_TIMEOUT_SECS: f32 = 0.25;

pub struct AxElement(AXUIElementRef);

impl Drop for AxElement {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: `self.0` was obtained under the create rule (either
            // `AXUIElementCreateApplication` or the `mem::forget`-transferred
            // reference in `attribute_element_result`), and `AxElement` is
            // its sole owner, so releasing it here is correct exactly once.
            unsafe { CFRelease(self.0 as CFTypeRef) };
        }
    }
}

impl AxElement {
    pub fn application(pid: i32) -> AxElement {
        // SAFETY: `AXUIElementCreateApplication` accepts any pid (an
        // invalid one just yields an element every call on which fails);
        // the returned reference follows the create rule and is released
        // in `Drop`.
        let el = unsafe { AXUIElementCreateApplication(pid) };
        // SAFETY: `el` was just created above and is a valid
        // `AXUIElementRef` for this call.
        unsafe { AXUIElementSetMessagingTimeout(el, MESSAGING_TIMEOUT_SECS) };
        AxElement(el)
    }

    fn copy_attribute(&self, name: &str) -> Result<CFType, AXError> {
        let key = CFString::new(name);
        let mut out: CFTypeRef = ptr::null();
        // SAFETY: `self.0` is a valid `AXUIElementRef` for the lifetime of
        // `&self`; `key` is a live `CFString`; `out` is a valid out-pointer.
        let err =
            unsafe { AXUIElementCopyAttributeValue(self.0, key.as_concrete_TypeRef(), &mut out) };
        if err != kAXErrorSuccess {
            return Err(err);
        }
        if out.is_null() {
            return Err(kAXErrorNoValue);
        }
        // SAFETY: `err == kAXErrorSuccess` and `out` is non-null, so this
        // follows the create rule documented for
        // `AXUIElementCopyAttributeValue`: the caller now owns one reference.
        Ok(unsafe { CFType::wrap_under_create_rule(out) })
    }

    pub fn attribute_string(&self, name: &str) -> Option<String> {
        let value = self.copy_attribute(name).ok()?;
        value.downcast::<CFString>().map(|s| s.to_string())
    }

    /// Returns an attribute that AX may report as either a `CFString` or a
    /// `CFURL`. Chromium reports `AXURL`/`AXDocument` as a `CFURL`, not a
    /// string, so `attribute_string` returns nothing for it there.
    pub fn attribute_url(&self, name: &str) -> Option<String> {
        let value = self.copy_attribute(name).ok()?;
        if let Some(s) = value.downcast::<CFString>() {
            return Some(s.to_string());
        }
        value
            .downcast::<CFURL>()
            .map(|u| u.get_string().to_string())
    }

    /// Returns the child element stored under `name` (for example the
    /// focused element or the focused window). The element is checked to
    /// really be an AXUIElement before the cast, and carries the same
    /// messaging timeout as `application` so a hung target cannot stall
    /// every subsequent call made through it.
    fn attribute_element_result(&self, name: &str) -> Result<AxElement, AXError> {
        let value = self.copy_attribute(name)?;
        // SAFETY: `value` is a live `CFType` still holding its create-rule
        // reference; `CFGetTypeID` reads its header and `AXUIElementGetTypeID`
        // takes no arguments, so neither call has any other precondition.
        if unsafe { CFGetTypeID(value.as_CFTypeRef()) } != unsafe { AXUIElementGetTypeID() } {
            return Err(kAXErrorIllegalArgument);
        }
        let raw = value.as_CFTypeRef() as AXUIElementRef;
        // The value is an AXUIElement; keep it alive by taking over the
        // create-rule reference that `copy_attribute` wrapped.
        std::mem::forget(value);
        // SAFETY: `raw` was just verified to be an `AXUIElement` and its
        // create-rule reference was transferred from `value` above, so it
        // is a valid, owned `AXUIElementRef`.
        unsafe { AXUIElementSetMessagingTimeout(raw, MESSAGING_TIMEOUT_SECS) };
        Ok(AxElement(raw))
    }

    pub fn attribute_element(&self, name: &str) -> Option<AxElement> {
        self.attribute_element_result(name).ok()
    }

    fn focused_element_result(&self) -> Result<AxElement, AXError> {
        self.attribute_element_result(kAXFocusedUIElementAttribute)
    }

    pub fn focused_element(&self) -> Option<AxElement> {
        self.focused_element_result().ok()
    }

    /// Whether the element says that `name` can be written.
    pub fn is_settable(&self, name: &str) -> bool {
        let key = CFString::new(name);
        let mut settable: u8 = 0;
        // SAFETY: `self.0` is a valid `AXUIElementRef` for the lifetime of
        // `&self`; `key` is a live `CFString`; `settable` is a valid
        // out-pointer.
        let err = unsafe {
            AXUIElementIsAttributeSettable(self.0, key.as_concrete_TypeRef(), &mut settable)
        };
        err == kAXErrorSuccess && settable != 0
    }

    pub fn set_attribute_string(&self, name: &str, value: &str) -> Result<(), PlatformError> {
        let key = CFString::new(name);
        let val = CFString::new(value);
        // SAFETY: `self.0` is a valid `AXUIElementRef` for the lifetime of
        // `&self`; `key` and `val` are live `CFString`s kept alive for the
        // duration of this call.
        let err = unsafe {
            AXUIElementSetAttributeValue(self.0, key.as_concrete_TypeRef(), val.as_CFTypeRef())
        };
        if err == kAXErrorSuccess {
            Ok(())
        } else {
            Err(PlatformError::Ax(err))
        }
    }
}

/// Calls `notify` on every change of the Accessibility trust the system
/// announces (it posts a distributed notification when the list in System
/// Settings changes). The observer lives as long as the process.
pub fn watch_trust(notify: Box<dyn Fn() + Send + Sync>) {
    use std::ptr::NonNull;

    use block2::RcBlock;
    use objc2_foundation::{NSDistributedNotificationCenter, NSNotification, NSString};

    let name = NSString::from_str("com.apple.accessibility.api");
    let block = RcBlock::new(move |_: NonNull<NSNotification>| notify());
    let center = NSDistributedNotificationCenter::defaultCenter();
    // SAFETY: the name is a live string for the call, the block is owned by
    // the observer the centre keeps, and both are kept for the life of the
    // process because the observer is never removed.
    let observer = unsafe {
        center.addObserverForName_object_queue_usingBlock(Some(&name), None, None, &block)
    };
    std::mem::forget(observer);
}

pub fn is_trusted(prompt: bool) -> bool {
    // SAFETY: `kAXTrustedCheckOptionPrompt` is a process-lifetime CFString
    // constant exported by the framework; the get rule borrows it without
    // taking ownership.
    let key = unsafe { CFString::wrap_under_get_rule(kAXTrustedCheckOptionPrompt) };
    let value = if prompt {
        CFBoolean::true_value()
    } else {
        CFBoolean::false_value()
    };
    let dict = CFDictionary::from_CFType_pairs(&[(key.as_CFType(), value.as_CFType())]);
    // SAFETY: `dict` is a valid, live `CFDictionary` kept alive for this
    // call; the function has no other precondition.
    unsafe { AXIsProcessTrustedWithOptions(dict.as_concrete_TypeRef()) }
}

/// Text currently selected in the focused element of `pid`, if any.
pub fn selected_text(pid: i32) -> Option<String> {
    let app = AxElement::application(pid);
    let focused = app.focused_element()?;
    focused
        .attribute_string(kAXSelectedTextAttribute)
        .filter(|s| !s.is_empty())
}

/// Replaces the selection (or inserts at the caret) in the focused element.
pub fn insert_at_selection(pid: i32, text: &str) -> Result<(), PlatformError> {
    let app = AxElement::application(pid);
    let focused = app.focused_element_result().map_err(PlatformError::Ax)?;
    // Some applications (terminals among them) accept the write and report
    // success without taking the text; they do say that it cannot be set.
    if !focused.is_settable(kAXSelectedTextAttribute) {
        return Err(PlatformError::Ax(kAXErrorAttributeUnsupported));
    }
    focused.set_attribute_string(kAXSelectedTextAttribute, text)
}
