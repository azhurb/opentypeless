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

#[cfg(not(test))]
const POLL_INTERVAL_MS: u64 = 1000;
#[cfg(test)]
const POLL_INTERVAL_MS: u64 = 50;

#[cfg(not(test))]
const WATCH_DURATION_MS: u64 = 15_000;
#[cfg(test)]
const WATCH_DURATION_MS: u64 = 2_000;

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
    let baseline = match field.snapshot(&typed_text) {
        Some(s) if !s.is_secure => s,
        _ => return,
    };

    let started = std::time::Instant::now();
    // Relaxed: cancellation is eventual; the next poll cycle picks it up.
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
            continue;
        }
        tracing::debug!(
            "correction: change detected (baseline_len={}, current_len={})",
            baseline.value.len(),
            cur.value.len()
        );
        let sub = match diff::find_single_word_substitution(
            &baseline.value,
            &cur.value,
            &typed_text,
        ) {
            Some(s) => s,
            None => {
                tracing::debug!("correction: diff found no single-word substitution");
                continue;
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
            continue;
        }
        let row_id = match dictionary.add(&sub.new, None).await {
            Ok(id) => id,
            Err(e) => {
                tracing::warn!("dictionary insert failed during correction: {}", e);
                return;
            }
        };
        tracing::debug!("correction: emitting suggestion row_id={}", row_id);
        on_suggest(CorrectionSuggestion {
            row_id,
            old: sub.old,
            new: sub.new,
            auto_confirm_ms: AUTO_CONFIRM_MS,
        });
        return;
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
            self.snaps.lock().unwrap().first().cloned()
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
        let typed_start = value.find(typed).unwrap_or(0);
        let typed_end = typed_start + typed.len();
        FieldSnapshot {
            value: value.to_string(),
            typed_start,
            typed_end,
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
        assert!(listed.iter().any(|e| e.id == got.row_id && e.word == "Tim"));
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
