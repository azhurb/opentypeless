use std::time::Duration;

use anyhow::Result;
use tauri::AppHandle;

use super::chunker::{plan_chunks, ChunkPlan};
use crate::app_detector::cli_detect;
use crate::app_detector::AppContext;

#[cfg(target_os = "macos")]
use std::sync::atomic::{AtomicBool, Ordering};

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

/// How long the delayed-clipboard path waits for the receiving app to read
/// (consume) the pasted text before concluding the paste had nowhere to land.
/// A genuine paste reads almost immediately; this is generous enough to absorb
/// a slow consumer without making the no-target tip feel laggy. Tune against
/// real apps.
#[cfg(target_os = "macos")]
const LANDED_TIMEOUT_MS: u64 = 400;

/// Sticky for the process: set once any paste sees the private sentinel type
/// read, which means a clipboard manager (or similar) mirrors the pasteboard.
/// When set, we stop restoring the user's previous clipboard after a landed
/// paste — the consume signal can no longer be trusted to mean "the target read
/// it", so leaving the dictation on the clipboard guarantees it is never lost
/// (it also stays recoverable from the manager's own history).
#[cfg(target_os = "macos")]
static SENTINEL_SEEN: AtomicBool = AtomicBool::new(false);

/// Result of a paste attempt. `landed` is true when the receiving app consumed
/// the pasted text — or when we don't attempt detection at all (terminals,
/// multi-chunk pastes, non-macOS), which are treated as reliable targets. When
/// false, the dictation had nowhere to land and was left on the clipboard for a
/// manual paste.
#[derive(Debug, Clone, Copy)]
pub struct PasteOutcome {
    pub landed: bool,
}

/// `editable` is whether Accessibility sees a focused editable text element —
/// used only to gate clipboard restore on the single, non-terminal detection
/// path (see [`paste_single_detect`]). Ignored for terminals / chunked pastes /
/// non-macOS.
pub async fn paste(
    app_handle: &AppHandle,
    text: &str,
    app: &AppContext,
    editable: bool,
) -> Result<PasteOutcome> {
    let text = text.to_string();
    let app = app.clone();
    let app_handle = app_handle.clone();
    tokio::task::spawn_blocking(move || paste_blocking(app_handle, text, &app, editable)).await?
}

/// Eager paste: write each chunk to the clipboard, synthesize Cmd+V, and restore
/// the user's previous clipboard afterwards. Used for reliable targets where we
/// don't attempt landing detection (terminals, multi-chunk pastes, non-macOS).
/// arboard exposes plain text only; an image/file clipboard isn't preserved.
fn paste_eager(app_handle: &AppHandle, chunks: &[String]) -> Result<()> {
    let mut clipboard = arboard::Clipboard::new()
        .map_err(|e| anyhow::anyhow!("Failed to access clipboard: {}", e))?;
    let previous = clipboard.get_text().ok();

    let last = chunks.len().saturating_sub(1);
    for (i, chunk) in chunks.iter().enumerate() {
        write_and_paste(&mut clipboard, chunk, app_handle)?;
        if i != last {
            std::thread::sleep(Duration::from_millis(INTER_CHUNK_DELAY_MS));
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

#[cfg(target_os = "macos")]
fn paste_blocking(
    app_handle: AppHandle,
    text: String,
    app: &AppContext,
    editable: bool,
) -> Result<PasteOutcome> {
    // Detect a coding CLI running inside the focused app (by process tree),
    // which drives chunking even when the window title doesn't name the CLI.
    let detected = app.pid.and_then(cli_detect::detect_foreground_cli);
    let is_terminal = super::target_is_terminal(app);

    // Terminals and any chunked paste are treated as reliable targets: paste
    // eagerly with clipboard restore and report landed (no tip). Detection runs
    // only for a single, non-terminal paste — the case where the dictation can
    // genuinely have nowhere to land (a browser tab/title bar, the desktop, a
    // non-editable element).
    match plan_chunks(text, app, detected) {
        ChunkPlan::Multi(chunks) => {
            paste_eager(&app_handle, &chunks)?;
            Ok(PasteOutcome { landed: true })
        }
        ChunkPlan::Single(t) if is_terminal => {
            paste_eager(&app_handle, std::slice::from_ref(&t))?;
            Ok(PasteOutcome { landed: true })
        }
        ChunkPlan::Single(t) => paste_single_detect(&app_handle, &t, editable),
    }
}

#[cfg(not(target_os = "macos"))]
fn paste_blocking(
    app_handle: AppHandle,
    text: String,
    app: &AppContext,
    _editable: bool,
) -> Result<PasteOutcome> {
    // No paste-landing detection off macOS yet: paste eagerly and always report
    // landed so the no-target tip never fires (behavior unchanged from before).
    let detected = app.pid.and_then(cli_detect::detect_foreground_cli);
    let chunks = match plan_chunks(text, app, detected) {
        ChunkPlan::Single(t) => vec![t],
        ChunkPlan::Multi(chunks) => chunks,
    };
    paste_eager(&app_handle, &chunks)?;
    Ok(PasteOutcome { landed: true })
}

/// Single, non-terminal paste with landing detection via delayed-clipboard
/// rendering. Writes the text to the pasteboard lazily, synthesizes Cmd+V, and
/// watches whether a consumer reads it within [`LANDED_TIMEOUT_MS`].
///
/// The "was it read?" signal tells a *native* no-target (menu bar, desktop, a
/// non-editable control — nothing reads the clipboard) from a real paste, so it
/// drives the tip via [`PasteOutcome::landed`]. But it is NOT sufficient to
/// decide whether to *restore* the user's previous clipboard: browsers (and
/// Electron) read the clipboard on Cmd+V even when the paste lands nowhere, so a
/// read there is ambiguous. Restoring on an ambiguous read would overwrite — and
/// lose — the dictation. So restore is gated additionally on `editable`
/// (Accessibility confirming a focused text field) and on no clipboard manager
/// having been seen ([`SENTINEL_SEEN`]):
///   * not read → nowhere to land → materialize the text concretely and leave it
///     on the clipboard; report `landed = false` so the caller shows the tip.
///   * read + editable + no manager → confident landing → restore previous.
///   * read + (not editable / browser / manager) → leave the dictation on the
///     clipboard (no restore, no tip); it's never lost and is recoverable with a
///     manual Cmd+V. Matches the reference product's silent browser behavior.
#[cfg(target_os = "macos")]
fn paste_single_detect(app_handle: &AppHandle, text: &str, editable: bool) -> Result<PasteOutcome> {
    use std::ffi::CString;

    // Snapshot the user's current clipboard so we can restore it on a landed,
    // manager-free paste. Plain text only (arboard limitation).
    let previous = arboard::Clipboard::new()
        .ok()
        .and_then(|mut c| c.get_text().ok());

    let write_text = CString::new(text)
        .map_err(|_| anyhow::anyhow!("dictation text contains an interior NUL byte"))?;

    // Write the lazy pasteboard item on the main thread — the provider's data
    // callback is delivered on the main runloop. The returned handle holds an
    // extra retain so the provider stays alive while we poll it.
    let handle = run_on_main(app_handle, move || {
        SendPtr(unsafe { otl_pasteboard_write_lazy(write_text.as_ptr()) })
    })?;
    if handle.0.is_null() {
        anyhow::bail!("failed to write lazy clipboard item");
    }

    // Settle so the pasteboard change propagates, then synthesize Cmd+V (reuses
    // the main-thread CGEvent path).
    std::thread::sleep(Duration::from_millis(CLIPBOARD_SETTLE_MS));
    invoke_paste(app_handle)?;

    // Wait the landing window, then read the (sticky) consume flags. A genuine
    // paste reads the plain-text type within a few ms; nothing reads it when the
    // paste had nowhere to land (a native no-target — menu bar, desktop). A read
    // of the private sentinel type flags a clipboard manager mirroring the
    // pasteboard.
    std::thread::sleep(Duration::from_millis(LANDED_TIMEOUT_MS));
    let mut sentinel_flag: std::os::raw::c_int = 0;
    let landed = unsafe { otl_pasteboard_consumed(handle.0, &mut sentinel_flag) } != 0;
    if sentinel_flag != 0 {
        SENTINEL_SEEN.store(true, Ordering::Release);
    }
    let background_reader = SENTINEL_SEEN.load(Ordering::Acquire);
    tracing::debug!(
        "paste detect: landed={landed} editable={editable} sentinel={} background_reader={background_reader}",
        sentinel_flag != 0
    );

    if !landed {
        // Nowhere to land: replace the promised item with a concrete one so a
        // later manual Cmd+V still finds the dictation.
        if let Ok(materialize_text) = CString::new(text) {
            unsafe { otl_pasteboard_materialize(materialize_text.as_ptr()) };
        }
    } else if editable && !background_reader {
        // Confident landing in a real text field, no clipboard manager mirroring
        // the pasteboard → safe to restore the user's previous clipboard.
        if let Some(prev) = previous {
            if let Ok(mut cb) = arboard::Clipboard::new() {
                let _ = cb.set_text(prev);
            }
        }
    }
    // Otherwise (read but Accessibility can't confirm a field, e.g. a browser /
    // contenteditable, or a clipboard manager is present): leave the dictation
    // on the clipboard rather than restore over a paste we can't verify. It is
    // never lost — a manual Cmd+V recovers it — and no tip is shown.

    unsafe { otl_pasteboard_provider_release(handle.0) };
    Ok(PasteOutcome { landed })
}

/// Run `f` on the Tauri main thread and return its result, blocking the calling
/// (spawn_blocking) worker until it completes. Mirrors the dispatch pattern in
/// [`invoke_paste`]; used so pasteboard writes happen where the data-provider
/// callback is delivered.
#[cfg(target_os = "macos")]
fn run_on_main<T, F>(app_handle: &AppHandle, f: F) -> Result<T>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    let (tx, rx) = std::sync::mpsc::sync_channel::<T>(1);
    app_handle
        .run_on_main_thread(move || {
            let _ = tx.send(f());
        })
        .map_err(|e| anyhow::anyhow!("Failed to dispatch to main thread: {e}"))?;
    rx.recv()
        .map_err(|_| anyhow::anyhow!("Main-thread closure dropped before sending"))
}

/// Raw pointer wrapper so an opaque ObjC handle can cross the main-thread
/// dispatch boundary. The handle is only ever passed back to the pasteboard FFI,
/// whose operations are thread-safe.
#[cfg(target_os = "macos")]
struct SendPtr(*mut std::os::raw::c_void);
#[cfg(target_os = "macos")]
unsafe impl Send for SendPtr {}

// Delayed-clipboard provider, implemented in src/output/pasteboard_provider.m.
#[cfg(target_os = "macos")]
extern "C" {
    fn otl_pasteboard_write_lazy(utf8: *const std::os::raw::c_char) -> *mut std::os::raw::c_void;
    fn otl_pasteboard_consumed(
        handle: *mut std::os::raw::c_void,
        out_sentinel: *mut std::os::raw::c_int,
    ) -> std::os::raw::c_int;
    fn otl_pasteboard_materialize(utf8: *const std::os::raw::c_char);
    fn otl_pasteboard_provider_release(handle: *mut std::os::raw::c_void);
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
