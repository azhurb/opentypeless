use std::time::Duration;

use anyhow::Result;

use crate::app_detector::AppContext;
use super::chunker::{plan_chunks, ChunkPlan};

/// Delay between writing the clipboard and issuing Cmd+V. Small enough
/// to feel instantaneous; large enough that the receiving app sees the
/// pasteboard update before its paste handler reads.
const CLIPBOARD_SETTLE_MS: u64 = 30;

/// Delay between successive Cmd+V's when chunking a long paste. Gives
/// CLIs with line-buffered input parsers time to consume one chunk
/// before the next arrives.
const INTER_CHUNK_DELAY_MS: u64 = 50;

/// How long to wait after the final paste before restoring the user's
/// previous clipboard. The receiving app must finish consuming our
/// pasted text within this window or restoration will overwrite it.
const RESTORE_DELAY_MS: u64 = 500;

pub async fn paste(text: &str, app: &AppContext) -> Result<()> {
    let text = text.to_string();
    let app = app.clone();
    tokio::task::spawn_blocking(move || paste_blocking(text, &app)).await?
}

fn paste_blocking(text: String, app: &AppContext) -> Result<()> {
    let mut clipboard = arboard::Clipboard::new()
        .map_err(|e| anyhow::anyhow!("Failed to access clipboard: {}", e))?;

    // Snapshot the user's existing plain-text clipboard so we can restore
    // it after the paste lands. arboard exposes plain text only; if the
    // user had an image or files on the clipboard those won't be
    // preserved. Acceptable for v1.
    let previous = clipboard.get_text().ok();

    match plan_chunks(text, app) {
        ChunkPlan::Single(t) => {
            write_and_paste(&mut clipboard, &t)?;
        }
        ChunkPlan::Multi(chunks) => {
            let last = chunks.len().saturating_sub(1);
            for (i, chunk) in chunks.iter().enumerate() {
                write_and_paste(&mut clipboard, chunk)?;
                if i != last {
                    std::thread::sleep(Duration::from_millis(INTER_CHUNK_DELAY_MS));
                }
            }
        }
    }

    if let Some(prev) = previous {
        std::thread::sleep(Duration::from_millis(RESTORE_DELAY_MS));
        if let Ok(mut cb) = arboard::Clipboard::new() {
            let _ = cb.set_text(prev);
        }
    }

    Ok(())
}

fn write_and_paste(clipboard: &mut arboard::Clipboard, text: &str) -> Result<()> {
    clipboard
        .set_text(text)
        .map_err(|e| anyhow::anyhow!("Failed to set clipboard: {}", e))?;
    std::thread::sleep(Duration::from_millis(CLIPBOARD_SETTLE_MS));
    invoke_paste()
}

#[cfg(target_os = "macos")]
fn invoke_paste() -> Result<()> {
    // Apple Events (already in entitlements) is enough for Cmd+V; this
    // avoids requiring the Accessibility TCC grant that CGEventPost
    // would need.
    let status = std::process::Command::new("osascript")
        .args([
            "-e",
            r#"tell application "System Events" to keystroke "v" using command down"#,
        ])
        .status()?;
    if !status.success() {
        anyhow::bail!("osascript paste failed with exit code: {:?}", status.code());
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn invoke_paste() -> Result<()> {
    use enigo::{Direction, Enigo, Key, Keyboard, Settings};
    let mut enigo = Enigo::new(&Settings::default())
        .map_err(|e| anyhow::anyhow!("Failed to create Enigo: {:?}", e))?;
    enigo
        .key(Key::Control, Direction::Press)
        .map_err(|e| anyhow::anyhow!("Key press error: {:?}", e))?;
    enigo
        .key(Key::Unicode('v'), Direction::Click)
        .map_err(|e| anyhow::anyhow!("Key click error: {:?}", e))?;
    enigo
        .key(Key::Control, Direction::Release)
        .map_err(|e| anyhow::anyhow!("Key release error: {:?}", e))?;
    Ok(())
}
