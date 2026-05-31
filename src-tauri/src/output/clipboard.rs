use std::time::Duration;

use anyhow::Result;
use tauri::AppHandle;

use crate::app_detector::cli_detect;
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

pub async fn paste(app_handle: &AppHandle, text: &str, app: &AppContext) -> Result<()> {
    let text = text.to_string();
    let app = app.clone();
    let app_handle = app_handle.clone();
    tokio::task::spawn_blocking(move || paste_blocking(app_handle, text, &app)).await?
}

fn paste_blocking(app_handle: AppHandle, text: String, app: &AppContext) -> Result<()> {
    let mut clipboard = arboard::Clipboard::new()
        .map_err(|e| anyhow::anyhow!("Failed to access clipboard: {}", e))?;

    // Snapshot the user's existing plain-text clipboard so we can restore
    // it after the paste lands. arboard exposes plain text only; if the
    // user had an image or files on the clipboard those won't be
    // preserved. Acceptable for v1.
    let previous = clipboard.get_text().ok();

    // Detect a coding CLI running inside the focused app (by process tree),
    // which drives chunking even when the window title doesn't name the CLI.
    let detected = app.pid.and_then(cli_detect::detect_foreground_cli);

    match plan_chunks(text, app, detected) {
        ChunkPlan::Single(t) => {
            write_and_paste(&mut clipboard, &t, &app_handle)?;
        }
        ChunkPlan::Multi(chunks) => {
            let last = chunks.len().saturating_sub(1);
            for (i, chunk) in chunks.iter().enumerate() {
                write_and_paste(&mut clipboard, chunk, &app_handle)?;
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

fn write_and_paste(
    clipboard: &mut arboard::Clipboard,
    text: &str,
    app_handle: &AppHandle,
) -> Result<()> {
    clipboard
        .set_text(text)
        .map_err(|e| anyhow::anyhow!("Failed to set clipboard: {}", e))?;
    std::thread::sleep(Duration::from_millis(CLIPBOARD_SETTLE_MS));
    invoke_paste(app_handle)
}

#[cfg(not(target_os = "macos"))]
fn invoke_paste(_app_handle: &AppHandle) -> Result<()> {
    use enigo::{Direction, Enigo, Key, Keyboard, Settings};
    let mut enigo = Enigo::new(&Settings::default())
        .map_err(|e| anyhow::anyhow!("Failed to create Enigo: {:?}", e))?;
    enigo
        .key(Key::Control, Direction::Press)
        .map_err(|e| anyhow::anyhow!("Ctrl press error: {:?}", e))?;
    enigo
        .key(Key::Unicode('v'), Direction::Click)
        .map_err(|e| anyhow::anyhow!("V click error: {:?}", e))?;
    enigo
        .key(Key::Control, Direction::Release)
        .map_err(|e| anyhow::anyhow!("Ctrl release error: {:?}", e))?;
    Ok(())
}

#[cfg(target_os = "macos")]
fn invoke_paste(app_handle: &AppHandle) -> Result<()> {
    // CGEvent APIs are documented thread-safe, but post-paste we sometimes
    // run alongside other main-thread-touching paths (tray, capsule
    // updates). Marshalling onto Tauri's main thread serialises with those
    // and matches the reference helper's per-paste dispatch_sync pattern.
    let (tx, rx) = std::sync::mpsc::sync_channel::<Result<()>>(1);
    app_handle
        .run_on_main_thread(move || {
            let _ = tx.send(post_paste_keystroke());
        })
        .map_err(|e| anyhow::anyhow!("Failed to dispatch paste to main thread: {e}"))?;
    rx.recv()
        .map_err(|_| anyhow::anyhow!("Main-thread paste closure dropped before sending"))?
}

/// macOS paste keystroke synthesis.
///
/// Builds two CGEvents (V key-down, V key-up) directly via core-graphics,
/// sets `kCGEventFlagMaskCommand` on each event, and posts them to the
/// HID event tap with a 5 ms gap. This mirrors the canonical CGEvent
/// pattern used by macOS keystroke-synthesising helpers and avoids the
/// race that `enigo` 0.2.x exhibits: enigo posts a separate Cmd
/// `flagsChanged` event and relies on the OS to inherit the modifier
/// state from `CombinedSessionState` by the time the V event is created
/// — under load this inheritance is intermittent and the V event ships
/// without the Cmd flag, so the receiving app types a literal "v"
/// instead of pasting. Setting the flag directly on the V event makes
/// the keystroke modifier-deterministic.
///
/// Uses `HIDSystemState` for the event source so flags derive from
/// hardware state alone, isolating synthesis from any in-flight
/// synthesised modifier state on `CombinedSessionState`.
#[cfg(target_os = "macos")]
fn post_paste_keystroke() -> Result<()> {
    use core_graphics::event::CGEventTapLocation;

    let (down, up) = build_paste_events()?;
    down.post(CGEventTapLocation::HID);
    // 5 ms between the down and up posts: the receiving app's key-down
    // handler must run before the up arrives, or some apps (terminals
    // especially) drop the shortcut. The reference helper uses the same
    // 5 ms gap (`usleep(0x1388)`).
    std::thread::sleep(Duration::from_millis(5));
    up.post(CGEventTapLocation::HID);
    Ok(())
}

/// Build the V key-down and V key-up CGEvents for a Cmd+V paste.
///
/// Factored out of [`post_paste_keystroke`] so unit tests can inspect
/// the resulting events' keycode and flags without posting them.
#[cfg(target_os = "macos")]
fn build_paste_events() -> Result<(core_graphics::event::CGEvent, core_graphics::event::CGEvent)> {
    use core_graphics::event::{CGEvent, CGEventFlags};
    use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};

    // kVK_ANSI_V — the layout-independent keycode for the "V" key.
    const KVK_ANSI_V: core_graphics::event::CGKeyCode = 0x09;

    let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState)
        .map_err(|_| anyhow::anyhow!("Failed to create CGEventSource"))?;

    let down = CGEvent::new_keyboard_event(source.clone(), KVK_ANSI_V, true)
        .map_err(|_| anyhow::anyhow!("Failed to create V key-down event"))?;
    down.set_flags(CGEventFlags::CGEventFlagCommand);

    let up = CGEvent::new_keyboard_event(source, KVK_ANSI_V, false)
        .map_err(|_| anyhow::anyhow!("Failed to create V key-up event"))?;
    up.set_flags(CGEventFlags::CGEventFlagCommand);

    Ok((down, up))
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;
    use core_graphics::event::{CGEventFlags, EventField};

    /// kVK_ANSI_V — the layout-independent keycode for the V key on macOS.
    const KVK_ANSI_V: i64 = 0x09;

    #[test]
    fn paste_events_use_v_keycode() {
        let (down, up) = build_paste_events().expect("CGEvents construct");
        assert_eq!(
            down.get_integer_value_field(EventField::KEYBOARD_EVENT_KEYCODE),
            KVK_ANSI_V,
            "key-down event must carry kVK_ANSI_V"
        );
        assert_eq!(
            up.get_integer_value_field(EventField::KEYBOARD_EVENT_KEYCODE),
            KVK_ANSI_V,
            "key-up event must carry kVK_ANSI_V"
        );
    }

    #[test]
    fn paste_events_set_command_flag_on_both_down_and_up() {
        // The whole point of this rewrite: the Cmd flag must be stamped
        // directly on both the down and the up event. If either event
        // ships without it, modern macOS racily routes the keystroke as a
        // literal "v" character instead of paste.
        let (down, up) = build_paste_events().expect("CGEvents construct");
        assert!(
            down.get_flags().contains(CGEventFlags::CGEventFlagCommand),
            "key-down must have Cmd flag set"
        );
        assert!(
            up.get_flags().contains(CGEventFlags::CGEventFlagCommand),
            "key-up must have Cmd flag set"
        );
    }

    #[test]
    fn paste_events_do_not_leak_other_modifiers() {
        // Defence against future edits accidentally OR-ing in Shift/Ctrl
        // /Alt — that would turn paste into a different shortcut.
        let (down, up) = build_paste_events().expect("CGEvents construct");
        let other_modifiers = CGEventFlags::CGEventFlagShift
            | CGEventFlags::CGEventFlagControl
            | CGEventFlags::CGEventFlagAlternate
            | CGEventFlags::CGEventFlagSecondaryFn;
        assert!(
            !down.get_flags().intersects(other_modifiers),
            "key-down has stray modifier: flags={:?}",
            down.get_flags()
        );
        assert!(
            !up.get_flags().intersects(other_modifiers),
            "key-up has stray modifier: flags={:?}",
            up.get_flags()
        );
    }
}
