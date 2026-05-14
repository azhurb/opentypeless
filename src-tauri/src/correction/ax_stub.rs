#![cfg(not(target_os = "macos"))]

use super::{FieldSnapshot, FocusedField};

pub struct StubFocusedField;

impl FocusedField for StubFocusedField {
    fn snapshot(&self, _typed_text: &str) -> Option<FieldSnapshot> {
        None
    }
    fn current(&self, _baseline: &FieldSnapshot) -> Option<FieldSnapshot> {
        None
    }
}
