pub mod boundary;
pub mod classify;
pub mod diff;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use crate::storage::DictionaryStore;

/// Snapshot of a focused text field at one point in time.
#[derive(Debug, Clone)]
pub struct FieldSnapshot {
    pub value: String,
    pub typed_start: usize,
    pub typed_end: usize,
    pub is_secure: bool,
}

pub trait FocusedField: Send + Sync {
    fn snapshot(&self, typed_text: &str) -> Option<FieldSnapshot>;
    fn current(&self, baseline: &FieldSnapshot) -> Option<FieldSnapshot>;
}

#[derive(Debug, Clone)]
pub struct CorrectionSuggestion {
    pub row_id: i64,
    pub old: String,
    pub new: String,
    pub auto_confirm_ms: u32,
}

#[derive(Clone)]
pub struct CorrectionHandle {
    cancelled: Arc<AtomicBool>,
}

impl CorrectionHandle {
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }
}

// Wispr Flow uses exactly 1.0 s (`DispatchTimeInterval.seconds(1)`, verified
// via static disassembly of their Swift helper — see
// .scratch-wispr/findings/02-swift-helper.md). 1 s is also frugal on CPU and
// AX traffic; the toast is a 5 s auto-confirm so reaction time is fine.
#[cfg(not(test))]
const POLL_INTERVAL_MS: u64 = 1000;
#[cfg(test)]
const POLL_INTERVAL_MS: u64 = 25;

#[cfg(not(test))]
const WATCH_DURATION_MS: u64 = 60_000;
#[cfg(test)]
const WATCH_DURATION_MS: u64 = 2_000;

/// How long to keep retrying the initial snapshot when AX hasn't caught up to
/// enigo's keystrokes yet (typed_span_found=false). After this, we accept the
/// degenerate baseline and rely on the watcher's later polls.
#[cfg(not(test))]
const SNAPSHOT_RETRY_BUDGET_MS: u64 = 1_500;
#[cfg(test)]
const SNAPSHOT_RETRY_BUDGET_MS: u64 = 200;

const AUTO_CONFIRM_MS: u32 = 5_000;

pub fn spawn<F>(
    field: Arc<dyn FocusedField>,
    dictionary: Arc<DictionaryStore>,
    typed_text: String,
    on_suggest: F,
) -> CorrectionHandle
where
    F: FnOnce(CorrectionSuggestion) + Send + 'static,
{
    let cancelled = Arc::new(AtomicBool::new(false));
    let handle = CorrectionHandle {
        cancelled: cancelled.clone(),
    };
    tokio::spawn(async move {
        run(field, dictionary, typed_text, cancelled, on_suggest).await;
    });
    handle
}

async fn run<F>(
    field: Arc<dyn FocusedField>,
    dictionary: Arc<DictionaryStore>,
    typed_text: String,
    cancelled: Arc<AtomicBool>,
    on_suggest: F,
) where
    F: FnOnce(CorrectionSuggestion) + Send + 'static,
{
    // Snapshot retry: AX reads can lag behind enigo's keystroke propagation.
    // Retry briefly until typed_text is visible in the field value; otherwise
    // baseline anchors degenerate to an empty prefix/suffix and we fall back
    // to time-debounce + full-string compare (which still handles dictation
    // into an empty field correctly).
    let baseline = match snapshot_with_retry(&*field, &typed_text, &cancelled).await {
        Some(s) if !s.is_secure => s,
        _ => return,
    };
    let anchors = boundary::extract_anchors(&baseline, &typed_text);
    tracing::debug!(
        "correction: anchors prefix_len={} suffix_len={} degenerate={}",
        anchors.prefix.len(),
        anchors.suffix.len(),
        anchors.is_degenerate(),
    );

    let started = std::time::Instant::now();
    // Debounce: only act on a candidate substitution once the field has been
    // stable (same value) for at least one full poll cycle. Without this, the
    // watcher would catch intermediate states while the user is mid-edit —
    // e.g., backspacing "Vladislav" → "Vlad" we'd otherwise commit "Vladi".
    // The boundary check (when anchors are non-degenerate) is a stronger
    // structural signal but doesn't help when dictation is at the field's
    // end (empty suffix anchor matches anything trailing), so we keep this
    // debounce as defense-in-depth.
    let mut pending_value: Option<String> = None;
    // Latest cur.value that differed from baseline. If WATCH_DURATION elapses
    // without an emit, the timeout-emit path tries the degenerate (full-string
    // compare) diff one last time on this value. Mirrors Wispr's verified
    // timeout behavior — their 60 s watchdog calls the same emit helper as the
    // success path, see .scratch-wispr/findings/02-swift-helper.md.
    let mut last_changed: Option<String> = None;
    while !cancelled.load(Ordering::Relaxed)
        && started.elapsed() < Duration::from_millis(WATCH_DURATION_MS)
    {
        tokio::time::sleep(Duration::from_millis(POLL_INTERVAL_MS)).await;
        if cancelled.load(Ordering::Relaxed) {
            return;
        }
        let cur = match field.current(&baseline) {
            Some(s) if !s.is_secure => s,
            _ => return,
        };
        if cur.value == baseline.value {
            pending_value = None;
            last_changed = None;
            continue;
        }
        last_changed = Some(cur.value.clone());
        let stable = matches!(&pending_value, Some(prev) if prev == &cur.value);
        if !stable {
            tracing::debug!(
                "correction: change detected but waiting for stability (current_len={})",
                cur.value.len()
            );
            pending_value = Some(cur.value.clone());
            continue;
        }
        // Stable. Try to build a suggestion via the boundary-aware path.
        if let Some(sugg) = try_build_suggestion(
            &baseline.value,
            &cur.value,
            &typed_text,
            &dictionary,
            Some(&anchors),
        )
        .await
        {
            tracing::debug!(
                "correction: emitting suggestion row_id={} (boundary path)",
                sugg.row_id
            );
            on_suggest(sugg);
            return;
        }
    }
    // Watch window elapsed. One last attempt on the latest-seen edited value
    // using the more permissive degenerate diff path. This is Wispr's safety
    // net for cases where the boundary anchors never realign (e.g., user edits
    // the suffix context, or single-word dictation with no surrounding text).
    if cancelled.load(Ordering::Relaxed) {
        return;
    }
    if let Some(cur_value) = last_changed {
        tracing::debug!("correction: watch timeout reached, attempting emit on last-seen value");
        if let Some(sugg) = try_build_suggestion(
            &baseline.value,
            &cur_value,
            &typed_text,
            &dictionary,
            None,
        )
        .await
        {
            tracing::debug!(
                "correction: emitting suggestion row_id={} (timeout path)",
                sugg.row_id
            );
            on_suggest(sugg);
        }
    }
}

/// Run the diff → classify → insert pipeline. Returns the built suggestion
/// (with row_id) only when all stages accept; otherwise None and no DB write
/// happens. Safe to call repeatedly; each Some result writes exactly one row.
async fn try_build_suggestion(
    baseline_value: &str,
    cur_value: &str,
    typed_text: &str,
    dictionary: &DictionaryStore,
    anchors: Option<&boundary::Anchors>,
) -> Option<CorrectionSuggestion> {
    let sub = match anchors {
        Some(a) if !a.is_degenerate() => {
            let span = match boundary::find_span(cur_value, a) {
                Some(r) => r,
                None => {
                    tracing::debug!(
                        "correction: boundaries not aligned (current_len={})",
                        cur_value.len()
                    );
                    return None;
                }
            };
            tracing::debug!("correction: boundaries aligned, span_len={}", span.len());
            let current_span = &cur_value[span];
            match diff::find_word_substitution_in_spans(typed_text, current_span) {
                Some(s) => s,
                None => {
                    tracing::debug!("correction: diff found no single-word substitution in span");
                    return None;
                }
            }
        }
        _ => {
            tracing::debug!(
                "correction: degenerate-path diff (baseline_len={}, current_len={})",
                baseline_value.len(),
                cur_value.len()
            );
            match diff::find_single_word_substitution(baseline_value, cur_value, typed_text) {
                Some(s) => s,
                None => {
                    tracing::debug!("correction: diff found no single-word substitution");
                    return None;
                }
            }
        }
    };
    tracing::debug!(
        "correction: candidate substitution old_len={} new_len={}",
        sub.old.len(),
        sub.new.len()
    );
    let existing_lower: Vec<String> = dictionary
        .words()
        .await
        .into_iter()
        .map(|w| w.to_lowercase())
        .collect();
    if !classify::is_dictionary_candidate(&sub.old, &sub.new, &existing_lower) {
        tracing::debug!("correction: classifier rejected candidate");
        return None;
    }
    let row_id = match dictionary.add_learned(&sub.new, &sub.old).await {
        Ok(id) => id,
        Err(e) => {
            tracing::warn!("dictionary insert failed during correction: {}", e);
            return None;
        }
    };
    Some(CorrectionSuggestion {
        row_id,
        old: sub.old,
        new: sub.new,
        auto_confirm_ms: AUTO_CONFIRM_MS,
    })
}

async fn snapshot_with_retry(
    field: &dyn FocusedField,
    typed_text: &str,
    cancelled: &Arc<AtomicBool>,
) -> Option<FieldSnapshot> {
    let started = std::time::Instant::now();
    let mut last: Option<FieldSnapshot> = None;
    loop {
        if cancelled.load(Ordering::Relaxed) {
            return None;
        }
        let snap = field.snapshot(typed_text)?;
        if snap.is_secure || snap.typed_end > snap.typed_start {
            return Some(snap);
        }
        last = Some(snap);
        if started.elapsed() >= Duration::from_millis(SNAPSHOT_RETRY_BUDGET_MS) {
            tracing::debug!(
                "correction: snapshot retry budget exhausted, proceeding with degenerate baseline"
            );
            return last;
        }
        tokio::time::sleep(Duration::from_millis(POLL_INTERVAL_MS / 2 + 1)).await;
    }
}

#[cfg(target_os = "macos")]
pub mod ax_macos;
#[cfg(not(target_os = "macos"))]
pub mod ax_stub;

#[cfg(target_os = "macos")]
pub fn current_platform_field() -> Option<Arc<dyn FocusedField>> {
    Some(Arc::new(ax_macos::MacOsFocusedField::new()))
}

#[cfg(not(target_os = "macos"))]
pub fn current_platform_field() -> Option<Arc<dyn FocusedField>> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    struct FakeField {
        snaps: Mutex<Vec<FieldSnapshot>>,
    }

    impl FakeField {
        fn new(snaps: Vec<FieldSnapshot>) -> Arc<Self> {
            Arc::new(Self {
                snaps: Mutex::new(snaps),
            })
        }
    }

    impl FocusedField for FakeField {
        fn snapshot(&self, _typed: &str) -> Option<FieldSnapshot> {
            let mut s = self.snaps.lock().unwrap();
            // Allow retry tests to provide a sequence where the head represents
            // a "typed_span_found=false" snapshot that should be skipped.
            while s.len() > 1 {
                let head_bad = s
                    .first()
                    .map(|f| !f.is_secure && f.typed_end <= f.typed_start)
                    .unwrap_or(false);
                if head_bad {
                    let _ = s.remove(0);
                } else {
                    break;
                }
            }
            s.first().cloned()
        }
        fn current(&self, _baseline: &FieldSnapshot) -> Option<FieldSnapshot> {
            let mut s = self.snaps.lock().unwrap();
            if s.len() > 1 {
                let _ = s.remove(0);
            }
            s.first().cloned()
        }
    }

    fn temp_store() -> Arc<DictionaryStore> {
        use std::sync::atomic::AtomicU64;
        static N: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join("otl-correction-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!(
            "watch-{}-{}.db",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        Arc::new(DictionaryStore::new(path).unwrap())
    }

    fn mk(value: &str, typed: &str) -> FieldSnapshot {
        // Mirrors ax_macos::snapshot's behavior: typed_span_found=false maps
        // to typed_start=typed_end=0 so the caller can detect the race.
        let (typed_start, typed_end) = match value.find(typed) {
            Some(i) => (i, i + typed.len()),
            None => (0, 0),
        };
        FieldSnapshot {
            value: value.to_string(),
            typed_start,
            typed_end,
            is_secure: false,
        }
    }

    fn stale_snap(value: &str) -> FieldSnapshot {
        // Represents an AX read where typed_text wasn't yet visible.
        FieldSnapshot {
            value: value.to_string(),
            typed_start: 0,
            typed_end: 0,
            is_secure: false,
        }
    }

    #[tokio::test]
    async fn emits_suggestion_when_user_corrects_one_word() {
        let field = FakeField::new(vec![
            mk("Hello Timmy ", "Hello Timmy "),
            mk("Hello Tim ", "Hello Timmy "),
        ]);
        let dict = temp_store();
        let (tx, rx) = std::sync::mpsc::channel();
        let _h = spawn(
            field,
            dict.clone(),
            "Hello Timmy ".to_string(),
            move |s| {
                let _ = tx.send(s);
            },
        );
        let got = tokio::task::spawn_blocking(move || rx.recv_timeout(Duration::from_millis(500)))
            .await
            .unwrap()
            .expect("watcher must emit a suggestion");
        assert_eq!(got.old, "Timmy");
        assert_eq!(got.new, "Tim");
        assert_eq!(got.auto_confirm_ms, 5_000);
        let listed = dict.list().await.unwrap();
        let learned = listed
            .iter()
            .find(|e| e.id == got.row_id && e.word == "Tim")
            .expect("learned row missing");
        assert_eq!(learned.source, "user_edits");
        assert_eq!(learned.observed_source.as_deref(), Some("Timmy"));
        assert_eq!(learned.frequency_used, 1);
        assert!(learned.last_used.is_some(), "watcher must stamp last_used");
    }

    #[tokio::test]
    async fn skips_secure_field() {
        let mut secret = mk("Tim ", "Tim ");
        secret.is_secure = true;
        let field = FakeField::new(vec![secret]);
        let dict = temp_store();
        let (tx, rx) = std::sync::mpsc::channel::<CorrectionSuggestion>();
        let _h = spawn(field, dict, "Tim ".to_string(), move |s| {
            let _ = tx.send(s);
        });
        let got = tokio::task::spawn_blocking(move || rx.recv_timeout(Duration::from_millis(300)))
            .await
            .unwrap();
        assert!(got.is_err(), "secure field must not trigger suggestion");
    }

    #[tokio::test]
    async fn no_emit_when_no_change() {
        let field = FakeField::new(vec![mk("Tim ", "Tim "), mk("Tim ", "Tim ")]);
        let dict = temp_store();
        let (tx, rx) = std::sync::mpsc::channel::<CorrectionSuggestion>();
        let _h = spawn(field, dict, "Tim ".to_string(), move |s| {
            let _ = tx.send(s);
        });
        let got = tokio::task::spawn_blocking(move || rx.recv_timeout(Duration::from_millis(2500)))
            .await
            .unwrap();
        assert!(got.is_err(), "no change must not trigger suggestion");
    }

    #[tokio::test]
    async fn waits_for_stable_value_before_emitting() {
        // Backspace-by-character simulation: field passes through several
        // intermediate values before settling on the final one. Without
        // debouncing the watcher would fire on the first intermediate; with
        // debouncing it waits until the final value has been seen twice.
        let field = FakeField::new(vec![
            mk("Hello Vladislav ", "Hello Vladislav "),
            mk("Hello Vladisl ", "Hello Vladislav "),
            mk("Hello Vladi ", "Hello Vladislav "),
            mk("Hello Vlad ", "Hello Vladislav "),
            mk("Hello Vlad ", "Hello Vladislav "),
        ]);
        let dict = temp_store();
        let (tx, rx) = std::sync::mpsc::channel();
        let _h = spawn(
            field,
            dict.clone(),
            "Hello Vladislav ".to_string(),
            move |s| {
                let _ = tx.send(s);
            },
        );
        let got = tokio::task::spawn_blocking(move || rx.recv_timeout(Duration::from_millis(800)))
            .await
            .unwrap()
            .expect("watcher must emit a suggestion after the value stabilises");
        assert_eq!(got.old, "Vladislav");
        assert_eq!(
            got.new, "Vlad",
            "must wait for the stable final value, not an intermediate"
        );
    }

    #[tokio::test]
    async fn fires_on_single_word_suffix_edit_with_surrounding_context() {
        // Reproduces the Philip → Philipp smoke-test scenario: the user
        // dictated "Philip" into a field that already contained "Hi I am ",
        // then edited the trailing letter.
        let field = FakeField::new(vec![
            mk("Hi I am Philip", "Philip"),  // baseline (typed_span_found=true)
            mk("Hi I am Philipp", "Philip"), // user edited
            mk("Hi I am Philipp", "Philip"), // stable
        ]);
        let dict = temp_store();
        let (tx, rx) = std::sync::mpsc::channel();
        let _h = spawn(field, dict.clone(), "Philip".to_string(), move |s| {
            let _ = tx.send(s);
        });
        let got = tokio::task::spawn_blocking(move || rx.recv_timeout(Duration::from_millis(800)))
            .await
            .unwrap()
            .expect("watcher must emit Philip → Philipp");
        assert_eq!(got.old, "Philip");
        assert_eq!(got.new, "Philipp");
    }

    #[tokio::test]
    async fn snapshot_retries_until_typed_span_found() {
        // First AX read missed the typed text (race with enigo). Subsequent
        // reads see the dictation present.
        let field = FakeField::new(vec![
            stale_snap("Hi I am "),                     // bad: typed_span_found=false
            mk("Hi I am Philip", "Philip"),             // good: snapshot retry succeeds
            mk("Hi I am Philipp", "Philip"),            // user edited
            mk("Hi I am Philipp", "Philip"),            // stable
        ]);
        let dict = temp_store();
        let (tx, rx) = std::sync::mpsc::channel();
        let _h = spawn(field, dict.clone(), "Philip".to_string(), move |s| {
            let _ = tx.send(s);
        });
        let got = tokio::task::spawn_blocking(move || rx.recv_timeout(Duration::from_millis(800)))
            .await
            .unwrap()
            .expect("watcher must emit after snapshot retry");
        assert_eq!(got.old, "Philip");
        assert_eq!(got.new, "Philipp");
    }

    #[tokio::test]
    async fn does_not_fire_when_anchors_not_aligned() {
        // baseline anchors: prefix="Hi ", suffix=" bye"
        // user edits the SUFFIX context (not the dictation) → anchors never realign
        let field = FakeField::new(vec![
            mk("Hi Philip bye", "Philip"),         // baseline
            mk("Hi Philip later", "Philip"),       // user mangled the suffix
            mk("Hi Philip later", "Philip"),       // stable
        ]);
        let dict = temp_store();
        let (tx, rx) = std::sync::mpsc::channel::<CorrectionSuggestion>();
        let _h = spawn(field, dict, "Philip".to_string(), move |s| {
            let _ = tx.send(s);
        });
        let got = tokio::task::spawn_blocking(move || rx.recv_timeout(Duration::from_millis(2500)))
            .await
            .unwrap();
        assert!(
            got.is_err(),
            "boundaries never realign — must not fire spurious suggestion"
        );
    }

    #[tokio::test]
    async fn emits_on_timeout_when_boundaries_never_align() {
        // User dictated "Bob" into a field with surrounding context. Then they
        // modified the SUFFIX context (not the dictation) — anchors never
        // realign. The watcher should still fire at watchdog timeout using the
        // degenerate diff path on the latest-seen value.
        //
        // baseline = "Hi Bob bye" (anchors prefix="Hi ", suffix=" bye")
        // current  = "Hi Bob Vladimir" — user replaced " bye" with " Vladimir".
        //   - Boundary check fails (suffix " bye" not in current).
        //   - Degenerate diff: ["Hi","Bob","bye"] vs ["Hi","Bob","Vladimir"]
        //     → single substitution bye → Vladimir.
        //   - Classifier accepts (capital V, len 8).
        let field = FakeField::new(vec![
            mk("Hi Bob bye", "Bob"),         // baseline
            mk("Hi Bob Vladimir", "Bob"),    // user edited the suffix
            mk("Hi Bob Vladimir", "Bob"),    // stable; loop runs until WATCH_DURATION
        ]);
        let dict = temp_store();
        let (tx, rx) = std::sync::mpsc::channel();
        let _h = spawn(field, dict.clone(), "Bob".to_string(), move |s| {
            let _ = tx.send(s);
        });
        // Wait past the test-mode WATCH_DURATION_MS (2000ms).
        let got = tokio::task::spawn_blocking(move || rx.recv_timeout(Duration::from_millis(2_500)))
            .await
            .unwrap()
            .expect("timeout-emit must fire on degenerate path");
        assert_eq!(got.old, "bye");
        assert_eq!(got.new, "Vladimir");
    }

    #[tokio::test]
    async fn cancel_stops_watcher() {
        let field = FakeField::new(vec![
            mk("Hello Timmy ", "Hello Timmy "),
            mk("Hello Tim ", "Hello Timmy "),
        ]);
        let dict = temp_store();
        let (tx, rx) = std::sync::mpsc::channel::<CorrectionSuggestion>();
        let h = spawn(field, dict, "Hello Timmy ".to_string(), move |s| {
            let _ = tx.send(s);
        });
        h.cancel();
        let got = tokio::task::spawn_blocking(move || rx.recv_timeout(Duration::from_millis(300)))
            .await
            .unwrap();
        assert!(got.is_err(), "cancelled watcher must not emit");
    }
}
