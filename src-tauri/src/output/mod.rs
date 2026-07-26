mod chunker;
pub mod clipboard;

use anyhow::Result;
use tauri::AppHandle;

pub use clipboard::PasteOutcome;

use crate::app_detector::AppContext;

/// Paste `text` into whichever app is currently focused.
///
/// Writes `text` to the system clipboard, synthesizes Cmd+V, and — on macOS for
/// a single, non-terminal paste — detects whether the receiving app actually
/// consumed it. When it did (or for reliable targets we don't probe: terminals,
/// chunked pastes, non-macOS), the user's prior clipboard is restored and
/// [`PasteOutcome::landed`] is true. When nothing consumed the paste, the
/// dictation is left on the clipboard for a manual paste and `landed` is false.
/// For terminal-hosted CLIs that struggle with bulk pastes (Claude CLI, Codex
/// CLI, …) the paste is split into multiple chunks with brief inter-chunk delays.
///
/// `app_handle` is used to marshal the macOS Cmd+V synthesis onto the
/// main thread; modern macOS panics the process when HIToolbox is
/// touched from a worker thread.
///
/// `editable` (whether Accessibility sees a focused text field) gates the
/// clipboard restore on the single-paste detection path so an unverifiable
/// browser paste never restores over — and loses — the dictation.
pub async fn paste_text(
    app_handle: &AppHandle,
    text: &str,
    app: &AppContext,
    editable: bool,
) -> Result<PasteOutcome> {
    clipboard::paste(app_handle, text, app, editable).await
}

/// True when the foreground app is a terminal emulator or an editor/IDE that
/// hosts an integrated terminal — contexts whose keyboard focus macOS
/// Accessibility reports unreliably. The no-target paste tip is suppressed for
/// these so daily CLI use never gets a spurious "press ⌘V" hint.
pub fn target_is_terminal(app: &AppContext) -> bool {
    app.bundle_id
        .as_deref()
        .map(chunker::is_terminal_like)
        .unwrap_or(false)
}
