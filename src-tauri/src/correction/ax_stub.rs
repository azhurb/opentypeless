#![cfg(not(target_os = "macos"))]

use super::{FieldSnapshot, FocusedField};

/// No Accessibility equivalent outside macOS, so the pipeline always falls back
/// to the clipboard capture. See the macOS twin for what `None` does and does
/// not mean.
pub fn focused_selected_text() -> Option<String> {
    None
}

pub struct StubFocusedField;

impl FocusedField for StubFocusedField {
    fn snapshot(&self, _typed_text: &str) -> Option<FieldSnapshot> {
        None
    }
    fn current(&self, _baseline: &FieldSnapshot) -> Option<FieldSnapshot> {
        None
    }
}
