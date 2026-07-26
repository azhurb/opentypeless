#![cfg(target_os = "macos")]

use std::ffi::c_void;
use std::ptr;

use super::{FieldSnapshot, FocusedField};

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXUIElementCreateSystemWide() -> *mut c_void;
    fn AXUIElementCopyAttributeValue(
        element: *mut c_void,
        attribute: *mut c_void,
        out_value: *mut *mut c_void,
    ) -> i32;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFRelease(cf: *mut c_void);
    fn CFGetTypeID(cf: *mut c_void) -> usize;
    fn CFStringGetTypeID() -> usize;
    fn CFStringGetLength(s: *mut c_void) -> isize;
    fn CFStringGetCString(s: *mut c_void, buffer: *mut u8, buffer_size: isize, encoding: u32)
        -> u8;
    fn CFStringCreateWithCString(alloc: *mut c_void, cstr: *const u8, encoding: u32)
        -> *mut c_void;
}

const K_CF_STRING_ENCODING_UTF8: u32 = 0x08000100;
const AX_SECURE_TEXT_FIELD: &str = "AXSecureTextField";

// The AX attribute extern statics (kAXFocusedUIElementAttribute, kAXValueAttribute,
// kAXRoleAttribute) live in the HIServices subframework and aren't visible to the
// release linker through the ApplicationServices umbrella in all toolchains. Build the
// CFStringRefs at runtime from their well-known names instead — same value, no linkage
// dependency.
unsafe fn cfstr(name: &[u8]) -> *mut c_void {
    // `name` must be NUL-terminated.
    CFStringCreateWithCString(ptr::null_mut(), name.as_ptr(), K_CF_STRING_ENCODING_UTF8)
}

pub struct MacOsFocusedField {
    _private: (),
}

impl MacOsFocusedField {
    pub fn new() -> Self {
        Self { _private: () }
    }
}

impl Default for MacOsFocusedField {
    fn default() -> Self {
        Self::new()
    }
}

unsafe fn cf_string_to_rust(cf: *mut c_void) -> Option<String> {
    if cf.is_null() {
        return None;
    }
    if CFGetTypeID(cf) != CFStringGetTypeID() {
        return None;
    }
    let len_chars = CFStringGetLength(cf);
    // Worst-case UTF-8 bytes ≈ 4 * UTF-16 code units + NUL terminator.
    let buf_size = (len_chars * 4 + 1).max(16);
    let mut buf: Vec<u8> = vec![0u8; buf_size as usize];
    if CFStringGetCString(cf, buf.as_mut_ptr(), buf_size, K_CF_STRING_ENCODING_UTF8) == 0 {
        return None;
    }
    let nul = buf.iter().position(|b| *b == 0).unwrap_or(buf.len());
    buf.truncate(nul);
    String::from_utf8(buf).ok()
}

/// Read the focused element's role (skip if secure) and value. Returns Some((value, is_secure))
/// on success — when `is_secure`, value is empty.
fn read_focused_value() -> Option<(String, bool)> {
    unsafe {
        if !crate::pipeline::is_accessibility_trusted() {
            return None;
        }
        let attr_focused = cfstr(b"AXFocusedUIElement\0");
        let attr_role = cfstr(b"AXRole\0");
        let attr_value = cfstr(b"AXValue\0");
        if attr_focused.is_null() || attr_role.is_null() || attr_value.is_null() {
            if !attr_focused.is_null() {
                CFRelease(attr_focused);
            }
            if !attr_role.is_null() {
                CFRelease(attr_role);
            }
            if !attr_value.is_null() {
                CFRelease(attr_value);
            }
            return None;
        }

        let system_wide = AXUIElementCreateSystemWide();
        if system_wide.is_null() {
            CFRelease(attr_focused);
            CFRelease(attr_role);
            CFRelease(attr_value);
            return None;
        }

        let mut focused: *mut c_void = ptr::null_mut();
        let err = AXUIElementCopyAttributeValue(system_wide, attr_focused, &mut focused);
        if err != 0 || focused.is_null() {
            CFRelease(attr_focused);
            CFRelease(attr_role);
            CFRelease(attr_value);
            CFRelease(system_wide);
            return None;
        }

        let mut role_ref: *mut c_void = ptr::null_mut();
        let mut is_secure = false;
        if AXUIElementCopyAttributeValue(focused, attr_role, &mut role_ref) == 0
            && !role_ref.is_null()
        {
            if let Some(role) = cf_string_to_rust(role_ref) {
                if role == AX_SECURE_TEXT_FIELD {
                    is_secure = true;
                }
            }
            CFRelease(role_ref);
        }

        if is_secure {
            CFRelease(focused);
            CFRelease(system_wide);
            CFRelease(attr_focused);
            CFRelease(attr_role);
            CFRelease(attr_value);
            return Some((String::new(), true));
        }

        let mut value_ref: *mut c_void = ptr::null_mut();
        let err2 = AXUIElementCopyAttributeValue(focused, attr_value, &mut value_ref);
        let value = if err2 == 0 && !value_ref.is_null() {
            cf_string_to_rust(value_ref)
        } else {
            None
        };
        if !value_ref.is_null() {
            CFRelease(value_ref);
        }
        CFRelease(focused);
        CFRelease(system_wide);
        CFRelease(attr_focused);
        CFRelease(attr_role);
        CFRelease(attr_value);
        value.map(|v| (v, false))
    }
}

/// AX roles that can receive pasted text. A focused element with one of these
/// roles is a paste target we can be confident about.
fn is_editable_role(role: &str) -> bool {
    matches!(
        role,
        "AXTextField" | "AXTextArea" | "AXComboBox" | "AXSearchField" | AX_SECURE_TEXT_FIELD
    )
}

/// True only when the system-wide focused UI element is an editable text element
/// — a paste target we can be confident about. Returns false when nothing is
/// focused, the focused element is non-editable (a button, a window, the menu
/// bar, a browser tab strip), or its role can't be read (a browser
/// contenteditable, which is invisible to Accessibility).
///
/// This is deliberately a *positive-only* signal used to gate clipboard
/// *restore*: we restore the user's previous clipboard only when we're confident
/// the paste landed in a field. An unrecognized target (browser web content)
/// returns false, so the dictation is left on the clipboard rather than risking
/// restoring over a paste we couldn't verify. When Accessibility isn't granted
/// we also return false — the caller already gates the paste on AX, so this is a
/// defensive fallback that errs toward keeping the dictation.
pub fn focused_editable_present() -> bool {
    unsafe {
        if !crate::pipeline::is_accessibility_trusted() {
            return false;
        }
        let attr_focused = cfstr(b"AXFocusedUIElement\0");
        let attr_role = cfstr(b"AXRole\0");
        if attr_focused.is_null() || attr_role.is_null() {
            if !attr_focused.is_null() {
                CFRelease(attr_focused);
            }
            if !attr_role.is_null() {
                CFRelease(attr_role);
            }
            return false;
        }
        let system_wide = AXUIElementCreateSystemWide();
        if system_wide.is_null() {
            CFRelease(attr_focused);
            CFRelease(attr_role);
            return false;
        }
        let mut focused: *mut c_void = ptr::null_mut();
        let err = AXUIElementCopyAttributeValue(system_wide, attr_focused, &mut focused);
        if err != 0 || focused.is_null() {
            if !focused.is_null() {
                CFRelease(focused);
            }
            CFRelease(system_wide);
            CFRelease(attr_focused);
            CFRelease(attr_role);
            return false;
        }

        let mut role_ref: *mut c_void = ptr::null_mut();
        let role_err = AXUIElementCopyAttributeValue(focused, attr_role, &mut role_ref);
        let role = if role_err == 0 && !role_ref.is_null() {
            let r = cf_string_to_rust(role_ref);
            CFRelease(role_ref);
            r
        } else {
            None
        };

        CFRelease(focused);
        CFRelease(system_wide);
        CFRelease(attr_focused);
        CFRelease(attr_role);

        match role {
            Some(r) => is_editable_role(&r),
            None => false,
        }
    }
}

impl FocusedField for MacOsFocusedField {
    fn snapshot(&self, typed_text: &str) -> Option<FieldSnapshot> {
        let (value, is_secure) = read_focused_value()?;
        if is_secure {
            return Some(FieldSnapshot {
                value: String::new(),
                typed_start: 0,
                typed_end: 0,
                is_secure: true,
            });
        }
        let (typed_start, typed_end) = match value.find(typed_text) {
            Some(start) => (start, start + typed_text.len()),
            None => (0, 0),
        };
        tracing::debug!(
            "AX snapshot: len={}, typed_span_found={}",
            value.len(),
            typed_start != typed_end
        );
        Some(FieldSnapshot {
            value,
            typed_start,
            typed_end,
            is_secure: false,
        })
    }

    fn current(&self, _baseline: &FieldSnapshot) -> Option<FieldSnapshot> {
        let (value, is_secure) = read_focused_value()?;
        if is_secure {
            return None;
        }
        Some(FieldSnapshot {
            value,
            typed_start: 0,
            typed_end: 0,
            is_secure: false,
        })
    }
}
