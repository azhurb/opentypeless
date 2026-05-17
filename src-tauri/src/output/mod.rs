pub mod clipboard;
mod chunker;

use anyhow::Result;
use tauri::AppHandle;

use crate::app_detector::AppContext;

/// Paste `text` into whichever app is currently focused.
///
/// Backs up the user's existing clipboard, writes `text` to the system
/// clipboard, synthesizes Cmd+V, then restores the prior clipboard
/// contents. For terminal-hosted CLIs that struggle with bulk pastes
/// (Claude CLI, Codex CLI, …) the paste is split into multiple chunks
/// with brief inter-chunk delays.
///
/// `app_handle` is used to marshal the macOS Cmd+V synthesis onto the
/// main thread; modern macOS panics the process when HIToolbox is
/// touched from a worker thread.
pub async fn paste_text(app_handle: &AppHandle, text: &str, app: &AppContext) -> Result<()> {
    clipboard::paste(app_handle, text, app).await
}
