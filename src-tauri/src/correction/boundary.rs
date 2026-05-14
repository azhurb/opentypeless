//! Boundary anchors for the correction watcher.
//!
//! Detects when the user's edit of a dictated region has settled by checking
//! whether the surrounding field context is intact in the current field value.
//!
//! Anchors come from the field state captured immediately after dictation
//! (a `FieldSnapshot`): the chars right BEFORE typed_text and right AFTER it
//! in the field's value. Single-word dictations into empty fields produce
//! empty anchors; the caller handles that degenerate case.

use std::ops::Range;

use super::FieldSnapshot;

/// Maximum bytes used for each side of the anchor.
/// Long enough to be distinctive; short enough that small unrelated edits
/// outside the dictation don't invalidate the anchors.
const ANCHOR_MAX_BYTES: usize = 24;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Anchors {
    /// Bytes immediately before typed_text in the baseline field value.
    /// Empty if dictation was at position 0 or typed_span_found=false.
    pub prefix: String,
    /// Bytes immediately after typed_text in the baseline field value.
    /// Empty if dictation was at end of field or typed_span_found=false.
    pub suffix: String,
}

impl Anchors {
    pub fn is_degenerate(&self) -> bool {
        self.prefix.is_empty() && self.suffix.is_empty()
    }
}

/// Extract surrounding-context anchors from the baseline snapshot.
///
/// If `typed_span_found` is false (typed_start == typed_end with non-empty
/// typed_text), returns degenerate anchors — caller must use a fallback.
pub fn extract_anchors(snapshot: &FieldSnapshot, typed_text: &str) -> Anchors {
    if snapshot.value.is_empty() || typed_text.is_empty() {
        return Anchors {
            prefix: String::new(),
            suffix: String::new(),
        };
    }
    if snapshot.typed_end <= snapshot.typed_start {
        return Anchors {
            prefix: String::new(),
            suffix: String::new(),
        };
    }

    let prefix = take_trailing(&snapshot.value[..snapshot.typed_start], ANCHOR_MAX_BYTES);
    let suffix_src = &snapshot.value[snapshot.typed_end..];
    let suffix = take_leading(suffix_src, ANCHOR_MAX_BYTES);
    Anchors {
        prefix: prefix.to_string(),
        suffix: suffix.to_string(),
    }
}

/// Find the dictated region's current span inside `current` using the anchors.
/// Returns Some(prefix_end..suffix_start) when both anchors are findable in order.
/// Empty prefix means span starts at 0; empty suffix means span ends at current.len().
pub fn find_span(current: &str, anchors: &Anchors) -> Option<Range<usize>> {
    let prefix_end = if anchors.prefix.is_empty() {
        0
    } else {
        // Use the LAST occurrence of the prefix — bias toward the most recent
        // copy of the surrounding context if it happens to repeat.
        current.rfind(&anchors.prefix)? + anchors.prefix.len()
    };
    let suffix_start = if anchors.suffix.is_empty() {
        current.len()
    } else {
        current[prefix_end..]
            .find(&anchors.suffix)
            .map(|off| prefix_end + off)?
    };
    if suffix_start < prefix_end {
        return None;
    }
    Some(prefix_end..suffix_start)
}

fn take_trailing(s: &str, n_bytes: usize) -> &str {
    if s.len() <= n_bytes {
        return s;
    }
    let mut start = s.len() - n_bytes;
    while start < s.len() && !s.is_char_boundary(start) {
        start += 1;
    }
    &s[start..]
}

fn take_leading(s: &str, n_bytes: usize) -> &str {
    if s.len() <= n_bytes {
        return s;
    }
    let mut end = n_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mk(value: &str, typed: &str) -> FieldSnapshot {
        let (s, e) = match value.find(typed) {
            Some(i) => (i, i + typed.len()),
            None => (0, 0),
        };
        FieldSnapshot {
            value: value.into(),
            typed_start: s,
            typed_end: e,
            is_secure: false,
        }
    }

    #[test]
    fn extracts_prefix_and_suffix_around_dictation() {
        let snap = mk("Hi there Philip and friends", "Philip");
        let a = extract_anchors(&snap, "Philip");
        assert_eq!(a.prefix, "Hi there ");
        assert_eq!(a.suffix, " and friends");
    }

    #[test]
    fn empty_prefix_when_dictation_at_start() {
        let snap = mk("Philip and friends", "Philip");
        let a = extract_anchors(&snap, "Philip");
        assert_eq!(a.prefix, "");
        assert_eq!(a.suffix, " and friends");
    }

    #[test]
    fn empty_suffix_when_dictation_at_end() {
        let snap = mk("Hi Philip", "Philip");
        let a = extract_anchors(&snap, "Philip");
        assert_eq!(a.prefix, "Hi ");
        assert_eq!(a.suffix, "");
    }

    #[test]
    fn degenerate_when_typed_span_not_found() {
        let snap = mk("Some prior text", "Philip"); // typed_start=typed_end=0
        let a = extract_anchors(&snap, "Philip");
        assert!(a.is_degenerate());
    }

    #[test]
    fn trims_long_prefix_to_anchor_max() {
        let long_prefix: String = "a".repeat(100);
        let value = format!("{}Philip", long_prefix);
        let snap = mk(&value, "Philip");
        let a = extract_anchors(&snap, "Philip");
        assert!(a.prefix.len() <= ANCHOR_MAX_BYTES);
        assert_eq!(a.prefix, "a".repeat(ANCHOR_MAX_BYTES));
    }

    #[test]
    fn finds_span_with_both_anchors_intact() {
        let anchors = Anchors {
            prefix: "Hi ".into(),
            suffix: " bye".into(),
        };
        let span = find_span("Hi Philipp bye", &anchors).unwrap();
        assert_eq!(&"Hi Philipp bye"[span], "Philipp");
    }

    #[test]
    fn returns_none_when_suffix_missing() {
        let anchors = Anchors {
            prefix: "Hi ".into(),
            suffix: " bye".into(),
        };
        assert!(find_span("Hi Philipp", &anchors).is_none());
    }

    #[test]
    fn returns_none_when_prefix_missing() {
        let anchors = Anchors {
            prefix: "Hi ".into(),
            suffix: " bye".into(),
        };
        assert!(find_span("Yo Philipp bye", &anchors).is_none());
    }

    #[test]
    fn empty_prefix_starts_at_zero() {
        let anchors = Anchors {
            prefix: String::new(),
            suffix: " end".into(),
        };
        let span = find_span("Philipp end", &anchors).unwrap();
        assert_eq!(&"Philipp end"[span], "Philipp");
    }

    #[test]
    fn empty_suffix_ends_at_len() {
        let anchors = Anchors {
            prefix: "Hi ".into(),
            suffix: String::new(),
        };
        let span = find_span("Hi Philipp", &anchors).unwrap();
        assert_eq!(&"Hi Philipp"[span], "Philipp");
    }

    #[test]
    fn snapshot_retry_scenario_recovers_anchors() {
        let snap1 = mk("Hi ", "Philip"); // typed_span_found=false
        let snap2 = mk("Hi Philip", "Philip"); // typed_span_found=true
        let a1 = extract_anchors(&snap1, "Philip");
        let a2 = extract_anchors(&snap2, "Philip");
        assert!(a1.is_degenerate());
        assert!(!a2.is_degenerate());
        assert_eq!(a2.prefix, "Hi ");
    }
}
