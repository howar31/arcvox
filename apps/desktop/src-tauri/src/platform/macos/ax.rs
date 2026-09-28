//! Thin RAII layer over the Accessibility (AX) C API.

use std::ptr;

use accessibility_sys::{
    kAXErrorSuccess, kAXFocusedUIElementAttribute, kAXSelectedTextAttribute,
    kAXTrustedCheckOptionPrompt, AXIsProcessTrustedWithOptions, AXUIElementCopyAttributeValue,
    AXUIElementCreateApplication, AXUIElementRef, AXUIElementSetAttributeValue,
    AXUIElementSetMessagingTimeout,
};
use core_foundation::base::{CFRelease, CFType, CFTypeRef, TCFType};
use core_foundation::boolean::CFBoolean;
use core_foundation::dictionary::CFDictionary;
use core_foundation::string::CFString;

use crate::platform::PlatformError;

/// A hung target app must not block dictation; AX calls give up after this.
const MESSAGING_TIMEOUT_SECS: f32 = 0.25;

pub struct AxElement(AXUIElementRef);

impl Drop for AxElement {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { CFRelease(self.0 as CFTypeRef) };
        }
    }
}

impl AxElement {
    pub fn application(pid: i32) -> AxElement {
        let el = unsafe { AXUIElementCreateApplication(pid) };
        unsafe { AXUIElementSetMessagingTimeout(el, MESSAGING_TIMEOUT_SECS) };
        AxElement(el)
    }

    fn copy_attribute(&self, name: &str) -> Option<CFType> {
        let key = CFString::new(name);
        let mut out: CFTypeRef = ptr::null();
        let err =
            unsafe { AXUIElementCopyAttributeValue(self.0, key.as_concrete_TypeRef(), &mut out) };
        if err != kAXErrorSuccess || out.is_null() {
            return None;
        }
        Some(unsafe { CFType::wrap_under_create_rule(out) })
    }

    pub fn attribute_string(&self, name: &str) -> Option<String> {
        let value = self.copy_attribute(name)?;
        value.downcast::<CFString>().map(|s| s.to_string())
    }

    /// Returns the child element stored under `name` (for example the
    /// focused element or the focused window).
    pub fn attribute_element(&self, name: &str) -> Option<AxElement> {
        let value = self.copy_attribute(name)?;
        // The value is an AXUIElement; keep it alive by taking over the
        // create-rule reference that `copy_attribute` wrapped.
        let raw = value.as_CFTypeRef() as AXUIElementRef;
        std::mem::forget(value);
        Some(AxElement(raw))
    }

    pub fn focused_element(&self) -> Option<AxElement> {
        self.attribute_element(kAXFocusedUIElementAttribute)
    }

    pub fn set_attribute_string(&self, name: &str, value: &str) -> Result<(), PlatformError> {
        let key = CFString::new(name);
        let val = CFString::new(value);
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

pub fn is_trusted(prompt: bool) -> bool {
    let key = unsafe { CFString::wrap_under_get_rule(kAXTrustedCheckOptionPrompt) };
    let value = if prompt {
        CFBoolean::true_value()
    } else {
        CFBoolean::false_value()
    };
    let dict = CFDictionary::from_CFType_pairs(&[(key.as_CFType(), value.as_CFType())]);
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
    let focused = app.focused_element().ok_or(PlatformError::Ax(-1))?;
    focused.set_attribute_string(kAXSelectedTextAttribute, text)
}
