use anyhow::Result;
use async_trait::async_trait;
use enigo::{Direction, Enigo, Key, Keyboard, Settings};

use super::{OutputMode, TextOutput};

/// Maximum characters per enigo.text() call to avoid input buffer overflow.
const TYPE_CHUNK_SIZE: usize = 200;
/// Delay between typing chunks.
const TYPE_CHUNK_DELAY_MS: u64 = 5;

/// Collapse CR, CRLF, and Unicode line/paragraph separators to '\n' so they
/// can't leak through enigo.text() and be interpreted as a Return keypress
/// (which would auto-submit web forms / chat composers mid-dictation).
fn normalize_for_typing(text: &str) -> String {
    text.replace("\r\n", "\n")
        .replace('\r',    "\n")
        .replace('\u{2028}', "\n")
        .replace('\u{2029}', "\n")
}

/// Type a single string into the foreground app via the given Enigo handle.
/// Splits on '\n' and inserts Shift+Return between line segments to produce a
/// soft newline (most editors accept this, while bare Return tends to submit).
fn type_string(enigo: &mut Enigo, text: &str) -> Result<()> {
    let normalized = normalize_for_typing(text);
    let lines: Vec<&str> = normalized.split('\n').collect();
    for (i, line) in lines.iter().enumerate() {
        if !line.is_empty() {
            for chunk in line.chars().collect::<Vec<_>>().chunks(TYPE_CHUNK_SIZE) {
                let s: String = chunk.iter().collect();
                enigo
                    .text(&s)
                    .map_err(|e| anyhow::anyhow!("Failed to type text: {:?}", e))?;
                std::thread::sleep(std::time::Duration::from_millis(TYPE_CHUNK_DELAY_MS));
            }
        }
        if i < lines.len() - 1 {
            enigo
                .key(Key::Shift, Direction::Press)
                .map_err(|e| anyhow::anyhow!("Key error: {:?}", e))?;
            enigo
                .key(Key::Return, Direction::Click)
                .map_err(|e| anyhow::anyhow!("Key error: {:?}", e))?;
            enigo
                .key(Key::Shift, Direction::Release)
                .map_err(|e| anyhow::anyhow!("Key error: {:?}", e))?;
        }
    }
    Ok(())
}

/// Drive the keyboard from a stream of chunks. Creates one Enigo handle and types every
/// chunk that arrives on `rx` in order. Returns when the sender side is dropped.
/// Run on a blocking thread (Enigo is sync).
pub fn type_stream(rx: std::sync::mpsc::Receiver<String>) -> Result<()> {
    let mut enigo = Enigo::new(&Settings::default())
        .map_err(|e| anyhow::anyhow!("Failed to create Enigo: {:?}", e))?;
    while let Ok(chunk) = rx.recv() {
        if chunk.is_empty() {
            continue;
        }
        type_string(&mut enigo, &chunk)?;
    }
    Ok(())
}

pub struct KeyboardOutput;

impl Default for KeyboardOutput {
    fn default() -> Self {
        Self::new()
    }
}

impl KeyboardOutput {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl TextOutput for KeyboardOutput {
    async fn type_text(&self, text: &str) -> Result<()> {
        let text = text.to_string();
        tokio::task::spawn_blocking(move || {
            let mut enigo = Enigo::new(&Settings::default())
                .map_err(|e| anyhow::anyhow!("Failed to create Enigo: {:?}", e))?;
            type_string(&mut enigo, &text)
        })
        .await?
    }

    fn mode(&self) -> OutputMode {
        OutputMode::Keyboard
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test] fn normalize_strips_crlf()                      { assert_eq!(normalize_for_typing("a\r\nb"), "a\nb"); }
    #[test] fn normalize_strips_bare_cr()                    { assert_eq!(normalize_for_typing("a\rb"),   "a\nb"); }
    #[test] fn normalize_strips_unicode_line_separators()    { assert_eq!(normalize_for_typing("a\u{2028}b\u{2029}c"), "a\nb\nc"); }
}
