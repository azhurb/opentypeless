pub mod clipboard;
mod chunker;

use anyhow::Result;

use crate::app_detector::AppContext;

/// Paste `text` into whichever app is currently focused.
///
/// Backs up the user's existing clipboard, writes `text` to the system
/// clipboard, synthesizes Cmd+V, then restores the prior clipboard
/// contents. For terminal-hosted CLIs that struggle with bulk pastes
/// (Claude CLI, Codex CLI, …) the paste is split into multiple chunks
/// with brief inter-chunk delays.
pub async fn paste_text(text: &str, app: &AppContext) -> Result<()> {
    clipboard::paste(text, app).await
}
