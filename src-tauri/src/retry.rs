//! Retry with exponential backoff for the provider calls where a second
//! attempt is safe.
//!
//! Safety is not uniform across the provider traits. `SttProvider` is a
//! stateful streaming session, so only `connect` may be retried — resending
//! audio or re-reading transcripts mid-stream would reorder or duplicate the
//! utterance. The one-shot HTTP calls (the Whisper-compatible file upload,
//! `LlmProvider::polish`) are idempotent and retry freely, but only up to the
//! point where output becomes visible: `polish` retries the request head and
//! stops once chunks start reaching the frontend callback.
//!
//! User-initiated connection tests deliberately do not retry — a failure is
//! the answer the user asked for.
//!
//! See [`docs/architecture/providers.md`](../../../docs/architecture/providers.md).

use std::future::Future;
use std::time::{Duration, Instant};

use anyhow::Result;
use reqwest::StatusCode;

/// Total attempts, including the first one.
const MAX_ATTEMPTS: u32 = 3;

/// Stop retrying once this much time has been spent on failed attempts.
///
/// Attempt count alone is not a safe bound: provider requests carry a 60s
/// timeout, and a timeout is a retryable error, so three attempts could sit for
/// three minutes — past `pipeline::STT_FINALIZE_TIMEOUT_SECS` (120s), and
/// `polish` has no outer deadline at all. Retry is worth it while failures are
/// cheap (a 429 or 502 comes back in milliseconds, a refused connection
/// immediately); an attempt that already burned real time is a provider in
/// trouble, and the user is better served by the error than by another wait.
const TIME_BUDGET: Duration = Duration::from_secs(10);

/// Delay before the first retry; doubles for each subsequent one. With
/// `MAX_ATTEMPTS = 3` that adds at most 1.2s to a failing call. Kept well
/// under a second-scale wait on purpose: retries are silent, so the budget is
/// bounded by how long the capsule can sit in one state before the user reads
/// it as a hang rather than as work in progress.
const BASE_BACKOFF: Duration = Duration::from_millis(400);

/// Byte budget for a provider error body echoed into an error message.
const ERROR_BODY_LIMIT: usize = 200;

/// A response the provider rejected, carrying the status so [`is_retryable`]
/// can classify it after the error has been erased into `anyhow::Error`.
///
/// `Display` is the caller's own message verbatim, so wrapping an existing
/// `bail!` in this type does not change what the user sees.
#[derive(Debug)]
pub struct HttpStatusError {
    pub status: StatusCode,
    message: String,
}

impl HttpStatusError {
    pub fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for HttpStatusError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for HttpStatusError {}

/// Trim a provider error body to [`ERROR_BODY_LIMIT`], cutting on a UTF-8 char
/// boundary — provider errors are often not ASCII, and slicing mid-codepoint
/// panics.
pub fn truncate_error_body(body: &str) -> &str {
    let end = body
        .char_indices()
        .take_while(|&(i, _)| i < ERROR_BODY_LIMIT)
        .last()
        .map(|(i, c)| i + c.len_utf8())
        .unwrap_or(body.len());
    &body[..end]
}

/// 429 and 5xx can succeed on a second attempt. Every other 4xx — bad key,
/// malformed request, exhausted quota — returns the same answer forever, and
/// retrying it only delays the error the user needs to see.
pub fn is_retryable_status(status: StatusCode) -> bool {
    status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error()
}

/// Whether a failure is transient enough to be worth another attempt.
///
/// Walks the `anyhow` cause chain so callers keep using `.context(...)` freely;
/// the first recognized cause decides. An unrecognized error is treated as
/// fatal — retrying something we cannot classify risks repeating a side effect.
pub fn is_retryable(err: &anyhow::Error) -> bool {
    for cause in err.chain() {
        if let Some(e) = cause.downcast_ref::<HttpStatusError>() {
            return is_retryable_status(e.status);
        }
        if let Some(e) = cause.downcast_ref::<reqwest::Error>() {
            return e.is_timeout() || e.is_connect() || e.status().is_some_and(is_retryable_status);
        }
        if let Some(e) = cause.downcast_ref::<tokio_tungstenite::tungstenite::Error>() {
            return is_retryable_websocket(e);
        }
    }
    false
}

fn is_retryable_websocket(err: &tokio_tungstenite::tungstenite::Error) -> bool {
    use tokio_tungstenite::tungstenite::Error as WsError;
    match err {
        // Refused / reset / DNS failure at the socket layer — the transient case.
        WsError::Io(_) => true,
        // The handshake got an HTTP response; same status rule as REST.
        WsError::Http(response) => is_retryable_status(response.status()),
        // Protocol, TLS and capacity failures are deterministic for the same
        // request, and `AlreadyClosed` / `ConnectionClosed` mean the caller is
        // using a dead session rather than failing to open one.
        _ => false,
    }
}

/// Run `op`, retrying transient failures with exponential backoff.
///
/// Retries are silent to the user: the capsule already shows a progress state
/// for the step being retried, and a "retrying 2/3" surface would turn a
/// recovery nobody was meant to notice into an apparent fault. Each retry is
/// logged at warn level so a support log still shows what happened.
pub async fn with_retry<T, F, Fut>(label: &str, op: F) -> Result<T>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T>>,
{
    with_backoff(label, MAX_ATTEMPTS, BASE_BACKOFF, TIME_BUDGET, op).await
}

/// [`with_retry`] with the policy spelled out, so tests can drop the sleeps.
async fn with_backoff<T, F, Fut>(
    label: &str,
    max_attempts: u32,
    base_backoff: Duration,
    time_budget: Duration,
    mut op: F,
) -> Result<T>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T>>,
{
    let started = Instant::now();
    let mut backoff = base_backoff;
    for attempt in 1..max_attempts {
        match op().await {
            Ok(value) => return Ok(value),
            Err(e) if is_retryable(&e) => {
                let spent = started.elapsed();
                if spent >= time_budget {
                    tracing::warn!(
                        "{} failed (attempt {}/{}) after {}ms, over the {}ms retry budget — giving up: {:#}",
                        label,
                        attempt,
                        max_attempts,
                        spent.as_millis(),
                        time_budget.as_millis(),
                        e
                    );
                    return Err(e);
                }
                tracing::warn!(
                    "{} failed (attempt {}/{}), retrying in {}ms: {:#}",
                    label,
                    attempt,
                    max_attempts,
                    backoff.as_millis(),
                    e
                );
                tokio::time::sleep(backoff).await;
                backoff *= 2;
            }
            Err(e) => return Err(e),
        }
    }
    // Last attempt: whatever it returns is the answer, so the caller sees the
    // provider's own error rather than a retry-flavored wrapper.
    op().await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn status_error(code: u16) -> anyhow::Error {
        HttpStatusError::new(StatusCode::from_u16(code).unwrap(), format!("HTTP {code}")).into()
    }

    #[test]
    fn retries_rate_limit_and_server_errors() {
        for code in [429, 500, 502, 503, 504] {
            assert!(
                is_retryable_status(StatusCode::from_u16(code).unwrap()),
                "{code} is transient and should be retried"
            );
        }
    }

    #[test]
    fn does_not_retry_client_errors_other_than_rate_limit() {
        for code in [400, 401, 402, 403, 404, 413, 422] {
            assert!(
                !is_retryable_status(StatusCode::from_u16(code).unwrap()),
                "{code} will not improve on a second attempt"
            );
        }
    }

    #[test]
    fn does_not_retry_success_statuses() {
        assert!(!is_retryable_status(StatusCode::OK));
        assert!(!is_retryable_status(StatusCode::NO_CONTENT));
    }

    #[test]
    fn classifies_http_status_error_through_anyhow() {
        assert!(is_retryable(&status_error(503)));
        assert!(!is_retryable(&status_error(401)));
    }

    #[test]
    fn classifies_http_status_error_through_added_context() {
        let err = status_error(429).context("polishing transcript");
        assert!(
            is_retryable(&err),
            "the classifier must see through .context() wrappers"
        );
    }

    #[test]
    fn http_status_error_displays_the_callers_message() {
        let err = HttpStatusError::new(StatusCode::BAD_GATEWAY, "LLM API error 502: upstream down");
        assert_eq!(err.to_string(), "LLM API error 502: upstream down");
    }

    #[test]
    fn does_not_retry_unrecognized_errors() {
        let err = anyhow::anyhow!("audio exceeds maximum length");
        assert!(
            !is_retryable(&err),
            "an unclassifiable failure must not be retried"
        );
    }

    #[test]
    fn classifies_websocket_handshake_status() {
        use tokio_tungstenite::tungstenite::Error as WsError;

        let refused: anyhow::Error = WsError::Io(std::io::Error::new(
            std::io::ErrorKind::ConnectionRefused,
            "refused",
        ))
        .into();
        assert!(is_retryable(&refused), "a refused socket is transient");

        let overloaded: anyhow::Error = WsError::Http(
            http::Response::builder()
                .status(503)
                .body(None)
                .expect("valid response"),
        )
        .into();
        assert!(is_retryable(&overloaded));

        let unauthorized: anyhow::Error = WsError::Http(
            http::Response::builder()
                .status(401)
                .body(None)
                .expect("valid response"),
        )
        .into();
        assert!(
            !is_retryable(&unauthorized),
            "a rejected key must surface immediately"
        );
    }

    #[test]
    fn truncate_error_body_keeps_short_bodies_whole() {
        assert_eq!(truncate_error_body(""), "");
        assert_eq!(truncate_error_body("nope"), "nope");
    }

    #[test]
    fn truncate_error_body_cuts_on_a_char_boundary() {
        // 3-byte chars: 200 bytes lands mid-codepoint, which would panic a
        // naive slice.
        let body = "é".repeat(200);
        let truncated = truncate_error_body(&body);
        assert!(truncated.len() <= ERROR_BODY_LIMIT + 1);
        assert!(body.starts_with(truncated));
    }

    #[tokio::test]
    async fn succeeds_after_a_transient_failure() {
        let attempts = Cell::new(0);
        let result = with_backoff("test", 3, Duration::ZERO, TIME_BUDGET, || async {
            attempts.set(attempts.get() + 1);
            if attempts.get() < 3 {
                Err(status_error(503))
            } else {
                Ok("transcript")
            }
        })
        .await;

        assert_eq!(result.unwrap(), "transcript");
        assert_eq!(attempts.get(), 3);
    }

    #[tokio::test]
    async fn gives_up_after_the_attempt_budget() {
        let attempts = Cell::new(0);
        let result: Result<()> = with_backoff("test", 3, Duration::ZERO, TIME_BUDGET, || async {
            attempts.set(attempts.get() + 1);
            Err(status_error(500))
        })
        .await;

        assert!(result.is_err());
        assert_eq!(attempts.get(), 3, "must not exceed the attempt budget");
    }

    #[tokio::test]
    async fn does_not_stack_slow_failures() {
        // A zero budget stands in for "the first attempt already burned more
        // time than we are willing to spend" — the 60s-request-timeout case,
        // which must not be retried into a multi-minute wait.
        let attempts = Cell::new(0);
        let result: Result<()> =
            with_backoff("test", 3, Duration::ZERO, Duration::ZERO, || async {
                attempts.set(attempts.get() + 1);
                Err(status_error(503))
            })
            .await;

        assert!(result.is_err());
        assert_eq!(
            attempts.get(),
            1,
            "a retryable but expensive failure must surface instead of being retried"
        );
    }

    #[tokio::test]
    async fn fails_fast_on_a_fatal_error() {
        let attempts = Cell::new(0);
        let result: Result<()> = with_backoff("test", 3, Duration::ZERO, TIME_BUDGET, || async {
            attempts.set(attempts.get() + 1);
            Err(status_error(401))
        })
        .await;

        assert_eq!(
            result.unwrap_err().to_string(),
            "HTTP 401",
            "the provider's own error must reach the caller unwrapped"
        );
        assert_eq!(attempts.get(), 1, "a bad key must not be retried");
    }
}
