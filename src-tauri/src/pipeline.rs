use anyhow::Result;
use enigo::{Direction, Enigo, Key, Keyboard, Settings as EnigoSettings};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use tauri::Emitter;
use tauri::Manager;
use tokio::sync::Notify;

use crate::app_detector;
use crate::audio::{AudioCaptureHandle, AudioConfig};
use crate::llm::{self, LlmConfig, PolishRequest};
use crate::output;
use crate::storage;
use crate::stt::{self, SttConfig, TranscriptEvent};

// ─── Timing constants ───

/// Normalize text for typing into the foreground app. Trims trailing whitespace and appends a
/// single space so that successive dictations don't glue together. The polish prompt asks the
/// LLM to end with terminal punctuation, so the typical typed output is e.g. `"Hello world. "`.
/// Returns an empty string for empty input.
fn with_trailing_space(text: &str) -> String {
    let trimmed = text.trim_end();
    if trimmed.is_empty() {
        return String::new();
    }
    let mut out = String::with_capacity(trimmed.len() + 1);
    out.push_str(trimmed);
    out.push(' ');
    out
}

/// Keep the dictation text on the clipboard and show the manual-paste tip when
/// the paste did not land (no app consumed it) — but never in a terminal, which
/// is a reliable paste target whose daily CLI flow shouldn't be interrupted.
/// The paste path already reports `landed = true` for terminals; the explicit
/// `is_terminal` guard is defense-in-depth so a tip can never fire there.
fn should_retain_on_clipboard(landed: bool, is_terminal: bool) -> bool {
    !landed && !is_terminal
}

/// On macOS, verify whether the process has been granted Accessibility (Assistive Access)
/// permission. The paste path posts CGEvents directly and the selected-text capture goes
/// through enigo's CGEventPost; both require this permission, and without it the OS silently
/// drops every synthesised key event.
/// Returns true on all non-macOS platforms (no permission needed).
pub fn is_accessibility_trusted() -> bool {
    #[cfg(target_os = "macos")]
    {
        #[link(name = "ApplicationServices", kind = "framework")]
        extern "C" {
            fn AXIsProcessTrusted() -> u8;
        }
        unsafe { AXIsProcessTrusted() != 0 }
    }
    #[cfg(not(target_os = "macos"))]
    {
        true
    }
}

/// On macOS, request Accessibility permission by showing the system authorization dialog.
/// Uses AXIsProcessTrustedWithOptions with kAXTrustedCheckOptionPrompt = true.
/// Returns true if permission is already granted or on non-macOS platforms.
pub fn request_accessibility_permission() -> bool {
    #[cfg(target_os = "macos")]
    {
        // The dictionary key MUST be the real extern CFStringRef constant exported by
        // HIServices, not a synthesized "kAXTrustedCheckOptionPrompt" string. The
        // backing string of the constant is "AXTrustedCheckOptionPrompt" (no k);
        // using a synthesized key makes the framework's lookup return NULL, which
        // it then dereferences (crash at CFGetTypeID + 152, FAR=0x8).
        #[link(name = "ApplicationServices", kind = "framework")]
        extern "C" {
            fn AXIsProcessTrustedWithOptions(options: *mut std::ffi::c_void) -> u8;
            static kAXTrustedCheckOptionPrompt: *mut std::ffi::c_void;
        }
        #[link(name = "CoreFoundation", kind = "framework")]
        extern "C" {
            fn CFDictionaryCreate(
                allocator: *mut std::ffi::c_void,
                keys: *const *mut std::ffi::c_void,
                values: *const *mut std::ffi::c_void,
                num_values: isize,
                key_callbacks: *const std::ffi::c_void,
                value_callbacks: *const std::ffi::c_void,
            ) -> *mut std::ffi::c_void;
            fn CFRelease(cf: *mut std::ffi::c_void);
            static kCFTypeDictionaryKeyCallBacks: std::ffi::c_void;
            static kCFTypeDictionaryValueCallBacks: std::ffi::c_void;
            static kCFBooleanTrue: *mut std::ffi::c_void;
        }

        unsafe {
            let keys: [*mut std::ffi::c_void; 1] = [kAXTrustedCheckOptionPrompt];
            let values: [*mut std::ffi::c_void; 1] = [kCFBooleanTrue];

            let options = CFDictionaryCreate(
                std::ptr::null_mut(),
                keys.as_ptr(),
                values.as_ptr(),
                1,
                &kCFTypeDictionaryKeyCallBacks as *const std::ffi::c_void,
                &kCFTypeDictionaryValueCallBacks as *const std::ffi::c_void,
            );

            let trusted = AXIsProcessTrustedWithOptions(options) != 0;
            if !options.is_null() {
                CFRelease(options);
            }
            trusted
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        true
    }
}

/// Frontend-recognised error code that means "macOS Accessibility permission
/// was not granted, so paste was skipped." The frontend uses it to flip a
/// store flag and surface a banner; kept bare (not wrapped in "Output failed:
/// …") so the comparison stays exact.
const ACCESSIBILITY_REQUIRED_CODE: &str = "ACCESSIBILITY_REQUIRED";

fn output_error_message(e: &anyhow::Error) -> String {
    if e.to_string() == ACCESSIBILITY_REQUIRED_CODE {
        return ACCESSIBILITY_REQUIRED_CODE.to_string();
    }
    format!("Output failed: {e}")
}

/// Delay before capturing selected text to ensure hotkey modifiers are released.
const SELECTED_TEXT_CAPTURE_DELAY_MS: u64 = 60;
/// Delay after simulating Ctrl+C to let the clipboard update.
const CLIPBOARD_COPY_SETTLE_MS: u64 = 100;
/// Interval for polling audio volume during recording.
const VOLUME_POLL_INTERVAL_MS: u64 = 50;
/// Timeout for STT finalization after recording stops.
const STT_FINALIZE_TIMEOUT_SECS: u64 = 120;

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PipelineState {
    Idle,
    Recording,
    Transcribing,
    Polishing,
    Outputting,
}

impl PipelineState {
    fn as_u8(self) -> u8 {
        match self {
            Self::Idle => 0,
            Self::Recording => 1,
            Self::Transcribing => 2,
            Self::Polishing => 3,
            Self::Outputting => 4,
        }
    }

    fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::Recording,
            2 => Self::Transcribing,
            3 => Self::Polishing,
            4 => Self::Outputting,
            _ => Self::Idle,
        }
    }
}

pub struct PipelineHandle {
    app_handle: tauri::AppHandle,
    state: Arc<AtomicU8>,
    audio_handle: Arc<Mutex<Option<AudioCaptureHandle>>>,
    audio_volume: Arc<Mutex<f32>>,
    accumulated_text: Arc<Mutex<String>>,
    /// Last language code reported by the STT for this utterance, if any.
    /// Populated from `TranscriptEvent::Final.language` (streaming) or from
    /// the second element of the `disconnect()` tuple (file-based).
    detected_language: Arc<Mutex<Option<String>>>,
    stt_done: Arc<Notify>,
    abort_flag: Arc<AtomicBool>,
    preloaded_config: Arc<Mutex<Option<storage::AppConfig>>>,
    preloaded_app_ctx: Arc<Mutex<Option<app_detector::AppContext>>>,
    preloaded_dictionary: Arc<Mutex<Option<Vec<String>>>>,
    preloaded_selected_text: Arc<Mutex<Option<String>>>,
    recording_start: Arc<Mutex<Option<std::time::Instant>>>,
    pub(crate) current_correction: Arc<Mutex<Option<crate::correction::CorrectionHandle>>>,
    shared_client: reqwest::Client,
    /// Serializes start()/stop() so that stop() waits for start() to finish
    /// its setup before reading shared state (preloaded_config, audio_handle, etc.).
    /// Without this, a quick press-release in hold mode causes stop() to run
    /// while start() is still connecting to STT, finding empty fields.
    pipeline_lock: tokio::sync::Mutex<()>,
}

impl PipelineHandle {
    pub fn new(app_handle: tauri::AppHandle) -> Self {
        Self {
            app_handle,
            state: Arc::new(AtomicU8::new(PipelineState::Idle.as_u8())),
            audio_handle: Arc::new(Mutex::new(None)),
            audio_volume: Arc::new(Mutex::new(0.0)),
            accumulated_text: Arc::new(Mutex::new(String::new())),
            detected_language: Arc::new(Mutex::new(None)),
            stt_done: Arc::new(Notify::new()),
            abort_flag: Arc::new(AtomicBool::new(false)),
            preloaded_config: Arc::new(Mutex::new(None)),
            preloaded_app_ctx: Arc::new(Mutex::new(None)),
            preloaded_dictionary: Arc::new(Mutex::new(None)),
            preloaded_selected_text: Arc::new(Mutex::new(None)),
            recording_start: Arc::new(Mutex::new(None)),
            current_correction: Arc::new(Mutex::new(None)),
            shared_client: reqwest::Client::new(),
            pipeline_lock: tokio::sync::Mutex::new(()),
        }
    }

    fn set_state(&self, new_state: PipelineState) {
        self.state.store(new_state.as_u8(), Ordering::SeqCst);
        let _ = self.app_handle.emit("pipeline:state", new_state);

        // Update tray tooltip + menu to reflect pipeline state
        if let Some(tray_handle) = self.app_handle.try_state::<crate::TrayHandle>() {
            let tooltip = match new_state {
                PipelineState::Recording => "OpenTypeless - Recording...",
                PipelineState::Transcribing => "OpenTypeless - Transcribing...",
                PipelineState::Polishing => "OpenTypeless - Polishing...",
                PipelineState::Outputting => "OpenTypeless - Outputting...",
                PipelineState::Idle => "OpenTypeless",
            };
            if let Ok(t) = tray_handle.tray.lock() {
                let _ = t.set_tooltip(Some(tooltip));
            }
        }
        crate::refresh_tray(&self.app_handle);
    }

    pub fn current_state(&self) -> PipelineState {
        PipelineState::from_u8(self.state.load(Ordering::SeqCst))
    }

    /// Immediately abort the pipeline regardless of current state.
    /// Stops audio capture, forces state to Idle, and signals any
    /// ongoing stop() to exit early via abort_flag.
    pub fn abort(&self) {
        tracing::info!("Pipeline abort requested (current state: {:?})", self.current_state());

        // Set abort flag so any running stop() exits early
        self.abort_flag.store(true, Ordering::SeqCst);

        // Stop audio capture (closes channel → STT task terminates naturally)
        {
            let mut handle = self.audio_handle.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(ref mut h) = *handle {
                h.stop();
            }
            *handle = None;
        }

        // Unblock stop() if it's waiting on stt_done.notified()
        self.stt_done.notify_one();

        // Clear accumulated text + detected language
        self.accumulated_text.lock().unwrap_or_else(|e| e.into_inner()).clear();
        *self.detected_language.lock().unwrap_or_else(|e| e.into_inner()) = None;

        // Force state to Idle — emits pipeline:state event to sync frontend
        self.set_state(PipelineState::Idle);
    }

    /// Capture selected text from the foreground app by simulating Ctrl+C / Cmd+C.
    /// Must be called when no hotkey modifier keys are physically held down.
    /// Called from async context via block_in_place, so std::thread::sleep is acceptable.
    fn capture_selected_text(&self) -> Option<String> {
        let mut clipboard = arboard::Clipboard::new().ok()?;
        let backup = clipboard.get_text().ok();

        if let Ok(mut enigo) = Enigo::new(&EnigoSettings::default()) {
            #[cfg(target_os = "macos")]
            let modifier = Key::Meta;
            #[cfg(not(target_os = "macos"))]
            let modifier = Key::Control;

            let pressed = enigo.key(modifier, Direction::Press).is_ok();
            if pressed {
                let _ = enigo.key(Key::Unicode('c'), Direction::Click);
                let _ = enigo.key(modifier, Direction::Release);
            }
        }

        std::thread::sleep(std::time::Duration::from_millis(CLIPBOARD_COPY_SETTLE_MS));

        let selected = clipboard.get_text().ok();

        // Always restore clipboard
        if let Some(ref b) = backup {
            let _ = clipboard.set_text(b);
        }

        tracing::info!(
            "Selected text capture: backup_len={}, selected_len={}",
            backup.as_deref().map(|s| s.len()).unwrap_or(0),
            selected.as_deref().map(|s| s.len()).unwrap_or(0)
        );

        // On macOS, if Cmd+C had no effect (e.g., no Accessibility permission),
        // the clipboard is unchanged, so selected == backup — return None to avoid
        // passing stale clipboard content to the LLM as if it were selected text.
        match &selected {
            Some(s) if !s.trim().is_empty() => {
                if backup.as_deref() == Some(s.as_str()) {
                    tracing::debug!(
                        "Selected text equals clipboard backup — Cmd+C had no effect, ignoring"
                    );
                    None
                } else {
                    Some(s.clone())
                }
            }
            _ => None,
        }
    }

    async fn load_config(&self) -> storage::AppConfig {
        self.app_handle
            .state::<storage::ConfigManager>()
            .load()
            .await
            .unwrap_or_default()
    }

    /// Tear down a partially-initialised recording: stop any running audio
    /// capture, clear preloaded slots and timing, transition back to Idle.
    /// Safe to call regardless of how far `start()` got — the audio-handle
    /// slot is `None` until capture succeeds, and the preloaded slots are
    /// `None` until they're populated, so each clear is a no-op when nothing
    /// was set.
    fn cleanup_failed_start(&self) {
        {
            let mut handle = self.audio_handle.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(ref mut h) = *handle {
                h.stop();
            }
            *handle = None;
        }
        *self.audio_volume.lock().unwrap_or_else(|e| e.into_inner()) = 0.0;
        *self
            .recording_start
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = None;
        *self
            .preloaded_config
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = None;
        *self
            .preloaded_app_ctx
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = None;
        *self
            .preloaded_dictionary
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = None;
        self.set_state(PipelineState::Idle);
    }

    pub async fn start(&self) -> Result<()> {
        // Hard-gate: macOS Microphone permission must be Authorized before
        // we touch cpal. If we let cpal try, the OS device-open fails with a
        // generic error and the user has no path back to the prompt (it's
        // one-shot per install). Surface MICROPHONE_DENIED so the frontend
        // can show the banner pointing to System Settings.
        #[cfg(target_os = "macos")]
        {
            let status = crate::audio::check_microphone_permission();
            if matches!(
                status,
                crate::audio::MicAuthStatus::Denied | crate::audio::MicAuthStatus::Restricted
            ) {
                let _ = self.app_handle.emit("pipeline:error", "MICROPHONE_DENIED");
                let _ = self
                    .app_handle
                    .emit("permissions:mic_status", &status);
                anyhow::bail!("MICROPHONE_DENIED");
            }
        }

        // Hold pipeline_lock for the entire setup so stop() cannot read
        // partially-initialised state (preloaded_config, audio_handle, etc.).
        let _guard = self.pipeline_lock.lock().await;

        // Reset abort flag for new recording
        self.abort_flag.store(false, Ordering::SeqCst);

        // Cancel any in-flight correction watcher from the previous dictation
        if let Some(h) = self
            .current_correction
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
        {
            h.cancel();
        }

        // Atomic CAS: only one caller can transition Idle → Recording
        if self
            .state
            .compare_exchange(
                PipelineState::Idle.as_u8(),
                PipelineState::Recording.as_u8(),
                Ordering::SeqCst,
                Ordering::SeqCst,
            )
            .is_err()
        {
            return Ok(());
        }
        let _ = self
            .app_handle
            .emit("pipeline:state", PipelineState::Recording);
        // Update tray for recording state
        if let Some(tray_handle) = self.app_handle.try_state::<crate::TrayHandle>() {
            if let Ok(t) = tray_handle.tray.lock() {
                let _ = t.set_tooltip(Some("OpenTypeless - Recording..."));
            }
        }
        crate::refresh_tray(&self.app_handle);

        // Clear accumulated text + detected language
        self.accumulated_text
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
        *self
            .detected_language
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = None;
        // Selected text is captured in stop() after the hotkey is released
        // (so Ctrl+C simulation won't conflict with held keys). Clear the
        // slot here so a fresh recording can't observe a leftover value.
        *self
            .preloaded_selected_text
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = None;

        // Open the audio capture FIRST, before the slow async setup (config
        // load, foreground-app detection, dictionary fetch, STT connect).
        // The audio mpsc channel in audio/capture.rs is bounded at 200 chunks
        // of ~20 ms each (~4 s of headroom), so the cpal callback can buffer
        // samples while the rest of setup runs — closing the dead-window
        // gap that previously dropped the first few hundred ms of speech.
        let config = AudioConfig::default();
        let (handle, mut audio_rx) = match AudioCaptureHandle::start(config) {
            Ok(result) => result,
            Err(e) => {
                tracing::error!("Audio capture failed: {}", e);
                let _ = self
                    .app_handle
                    .emit("pipeline:error", format!("Audio capture failed: {e}"));
                self.cleanup_failed_start();
                return Ok(());
            }
        };
        let audio_vol = handle.get_volume();
        *self.audio_volume.lock().unwrap_or_else(|e| e.into_inner()) = audio_vol;
        *self.audio_handle.lock().unwrap_or_else(|e| e.into_inner()) = Some(handle);
        // Stamp recording_start now so the `recording_ms` metric measures
        // real capture duration rather than post-connect duration.
        *self
            .recording_start
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = Some(std::time::Instant::now());

        // Volume monitoring task
        let app_handle = self.app_handle.clone();
        let audio_handle_ref = self.audio_handle.clone();
        let state_ref = self.state.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_millis(VOLUME_POLL_INTERVAL_MS)).await;
                let current = PipelineState::from_u8(state_ref.load(Ordering::SeqCst));
                if current != PipelineState::Recording {
                    break;
                }
                let vol = audio_handle_ref
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .as_ref()
                    .map(|h| h.get_volume())
                    .unwrap_or(0.0);
                let _ = app_handle.emit("audio:volume", vol);
            }
        });

        // Now do the slow setup. Audio is already buffering into audio_rx.
        let config_data = self.load_config().await;
        *self
            .preloaded_config
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = Some(config_data.clone());
        *self
            .preloaded_app_ctx
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = Some(app_detector::detect_current_app());
        let dict_words = self
            .app_handle
            .state::<std::sync::Arc<storage::DictionaryStore>>()
            .words()
            .await;
        *self
            .preloaded_dictionary
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = Some(dict_words);

        tracing::debug!(
            "Pipeline using config: stt_provider={}, stt_key_len={}, stt_langs={:?}",
            config_data.stt_provider,
            config_data.stt_api_key.len(),
            config_data.stt_languages
        );

        // Guard: empty API key — bail and tear down the running capture
        if config_data.stt_api_key.is_empty() {
            let _ = self.app_handle.emit(
                "pipeline:error",
                "STT API key is not configured. Please set it in Settings → Speech Recognition.",
            );
            self.cleanup_failed_start();
            return Ok(());
        }

        // Pre-connect STT provider. For streaming providers (Deepgram /
        // AssemblyAI) this is a full WebSocket handshake; audio is buffering
        // into audio_rx the whole time, so the dead window the user used to
        // see has now been folded into a pre-buffer.
        let stt_config = SttConfig {
            api_key: config_data.stt_api_key.clone(),
            languages: config_data.stt_languages.clone(),
            smart_format: true,
            sample_rate: 16000,
        };

        let mut provider =
            stt::create_provider(&config_data.stt_provider, Some(self.shared_client.clone()));
        if let Err(e) = provider.connect(&stt_config).await {
            tracing::error!("STT connect failed: {}", e);
            let _ = self
                .app_handle
                .emit("pipeline:error", format!("STT connection failed: {e}"));
            self.cleanup_failed_start();
            return Ok(());
        }

        // Check abort_flag — if abort() was called during the connect (or
        // any earlier async setup step), drop the connected provider and
        // tear down the running capture.
        if self.abort_flag.load(Ordering::SeqCst) {
            tracing::info!("Pipeline aborted during setup, discarding audio capture and STT");
            drop(provider);
            self.cleanup_failed_start();
            return Ok(());
        }

        // STT streaming task — provider is already connected, audio_rx may
        // already hold a few hundred ms of pre-buffered samples that the
        // forwarder will flush immediately.
        let app_handle = self.app_handle.clone();
        let accumulated = self.accumulated_text.clone();
        let detected_lang = self.detected_language.clone();
        let stt_done = self.stt_done.clone();

        tokio::spawn(async move {
            // Forward audio to STT and receive transcripts
            loop {
                tokio::select! {
                    chunk = audio_rx.recv() => {
                        match chunk {
                            Some(data) => {
                                let _ = provider.send_audio(&data).await;
                            }
                            None => {
                                // Audio channel closed — disconnect and capture final transcript
                                match provider.disconnect().await {
                                    Ok(Some((text, lang))) => {
                                        let mut acc = accumulated.lock().unwrap_or_else(|e| e.into_inner());
                                        acc.push_str(&text);
                                        let current = acc.clone();
                                        drop(acc);
                                        if let Some(code) = lang {
                                            *detected_lang
                                                .lock()
                                                .unwrap_or_else(|e| e.into_inner()) = Some(code);
                                        }
                                        let _ = app_handle.emit("stt:final", &current);
                                    }
                                    Ok(None) => {}
                                    Err(e) => {
                                        tracing::error!("STT disconnect error: {}", e);
                                        let _ = app_handle.emit("pipeline:error", format!("STT error: {e}"));
                                    }
                                }
                                break;
                            }
                        }
                    }
                    transcript = provider.recv_transcript() => {
                        match transcript {
                            Ok(Some(TranscriptEvent::Partial { text })) => {
                                let _ = app_handle.emit("stt:partial", &text);
                            }
                            Ok(Some(TranscriptEvent::Final { text, language, .. })) => {
                                let mut acc = accumulated.lock().unwrap_or_else(|e| e.into_inner());
                                acc.push_str(&text);
                                acc.push(' ');
                                let current = acc.clone();
                                drop(acc);
                                if let Some(code) = language {
                                    *detected_lang
                                        .lock()
                                        .unwrap_or_else(|e| e.into_inner()) = Some(code);
                                }
                                let _ = app_handle.emit("stt:final", &current);
                            }
                            Ok(Some(TranscriptEvent::Error { message })) => {
                                tracing::error!("STT error: {}", message);
                                let _ = app_handle.emit("pipeline:error", format!("STT error: {message}"));
                                // Break out of the loop — STT has failed, no point
                                // continuing. Without break, the loop keeps running
                                // and the pipeline stays stuck in Recording forever.
                                break;
                            }
                            Err(e) => {
                                tracing::error!("STT recv error: {}", e);
                                break;
                            }
                            _ => {}
                        }
                    }
                }
            }

            // Signal that STT processing is complete
            stt_done.notify_one();
        });

        Ok(())
    }

    pub async fn stop(&self) -> Result<()> {
        // Acquire pipeline_lock so we wait for start() to finish its setup
        // (load_config, connect STT, start audio) before reading shared state.
        // Released before the long stt_done wait so start() isn't blocked 120s.
        let guard = self.pipeline_lock.lock().await;

        // Atomic CAS: only one caller can transition Recording → Transcribing
        if self
            .state
            .compare_exchange(
                PipelineState::Recording.as_u8(),
                PipelineState::Transcribing.as_u8(),
                Ordering::SeqCst,
                Ordering::SeqCst,
            )
            .is_err()
        {
            return Ok(());
        }
        let _ = self
            .app_handle
            .emit("pipeline:state", PipelineState::Transcribing);
        // Update tray for transcribing state
        if let Some(tray_handle) = self.app_handle.try_state::<crate::TrayHandle>() {
            if let Ok(t) = tray_handle.tray.lock() {
                let _ = t.set_tooltip(Some("OpenTypeless - Transcribing..."));
            }
        }
        crate::refresh_tray(&self.app_handle);

        let stop_start = std::time::Instant::now();

        // Capture selected text now — hotkey is released so Ctrl+C won't conflict.
        // Small delay to ensure hotkey modifiers are fully released (especially in toggle mode).
        let config_data = self
            .preloaded_config
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
            .unwrap_or_default();
        let selected_text = if config_data.selected_text_enabled {
            tokio::time::sleep(std::time::Duration::from_millis(
                SELECTED_TEXT_CAPTURE_DELAY_MS,
            ))
            .await;
            tokio::task::block_in_place(|| self.capture_selected_text())
        } else {
            None
        };
        tracing::info!(
            "Selected text result: len={}",
            selected_text.as_deref().map(|s| s.len()).unwrap_or(0)
        );
        *self
            .preloaded_selected_text
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = selected_text;

        // Stop audio capture (this drops the channel, signaling STT task to stop)
        {
            let mut handle = self.audio_handle.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(ref mut h) = *handle {
                h.stop();
            }
            *handle = None;
        }

        // P2-1: Pre-build LLM resources while waiting for STT
        let preloaded_config = self
            .preloaded_config
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        let config = match preloaded_config {
            Some(c) => c,
            None => self.load_config().await,
        };
        let app_ctx = self
            .preloaded_app_ctx
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
            .unwrap_or_else(app_detector::detect_current_app);
        let dictionary_words = self
            .preloaded_dictionary
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
            .unwrap_or_default();
        let selected_text = self
            .preloaded_selected_text
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();

        // All shared state has been taken — release the lock so a new start()
        // isn't blocked by the long stt_done wait that follows.
        drop(guard);

        // Pre-build LLM provider while STT is still processing
        let pre_llm = if config.polish_enabled && !config.llm_api_key.is_empty() {
            let llm_config = LlmConfig {
                api_key: config.llm_api_key.clone(),
                model: config.llm_model.clone(),
                base_url: config.llm_base_url.clone(),
                max_tokens: 4096,
                temperature: 0.3,
            };
            let provider =
                llm::create_provider(&config.llm_provider, Some(self.shared_client.clone()));
            Some((llm_config, provider))
        } else {
            None
        };

        // Wait for STT task to finish (handles both streaming and file-based providers)
        // Timeout after 120s to support long recordings
        let stt_done = self.stt_done.clone();
        tokio::select! {
            _ = stt_done.notified() => {
                tracing::debug!("STT task completed");
            }
            _ = tokio::time::sleep(std::time::Duration::from_secs(STT_FINALIZE_TIMEOUT_SECS)) => {
                tracing::warn!("STT task timed out after {}s, using accumulated text so far", STT_FINALIZE_TIMEOUT_SECS);
            }
        }

        let stt_elapsed = stop_start.elapsed();
        tracing::info!(
            "[Pipeline Timing] STT finalize: {}ms",
            stt_elapsed.as_millis()
        );

        // Check if pipeline was aborted while waiting for STT
        if self.abort_flag.load(Ordering::SeqCst) {
            tracing::info!("Pipeline aborted after STT wait, skipping LLM and output");
            return Ok(());
        }

        let raw_text = self
            .accumulated_text
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .trim()
            .to_string();
        let detected_language = self
            .detected_language
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();

        if raw_text.is_empty() {
            let _ = self
                .app_handle
                .emit("pipeline:error", "No speech detected. Please try again.");
            self.set_state(PipelineState::Idle);
            return Ok(());
        }

        let final_text;
        let llm_elapsed;

        // Polish with LLM (resources already pre-built)
        // Check abort before entering LLM polish and output
        if self.abort_flag.load(Ordering::SeqCst) {
            tracing::info!("Pipeline aborted before LLM/output");
            return Ok(());
        }

        if let Some((llm_config, provider)) = pre_llm {
            self.set_state(PipelineState::Polishing);
            let llm_start = std::time::Instant::now();

            // The capsule UI listens for `llm:chunk` to render a live polish
            // indicator. The chunks are not fanned out to the foreground app —
            // output happens once polish finishes, via a single chunked paste.
            let app_handle = self.app_handle.clone();
            let abort = self.abort_flag.clone();
            let chunk_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
            let chunk_count_inner = chunk_count.clone();
            let first_chunk_at = Arc::new(Mutex::new(None::<std::time::Duration>));
            let first_chunk_at_clone = first_chunk_at.clone();
            let on_chunk: llm::ChunkCallback = Box::new(move |chunk: &str| {
                if abort.load(Ordering::SeqCst) {
                    return;
                }
                {
                    let mut slot = first_chunk_at_clone
                        .lock()
                        .unwrap_or_else(|e| e.into_inner());
                    if slot.is_none() {
                        *slot = Some(llm_start.elapsed());
                    }
                }
                let _ = app_handle.emit("llm:chunk", chunk);
                chunk_count_inner.fetch_add(1, Ordering::SeqCst);
            });

            let req = PolishRequest {
                raw_text: raw_text.clone(),
                app_type: app_ctx.app_type,
                dictionary: dictionary_words,
                translate_enabled: config.translate_enabled,
                target_lang: config.target_lang.clone(),
                selected_text,
                detected_language: detected_language.clone(),
                user_languages: config.stt_languages.clone(),
            };

            let polish_result = provider.polish(&llm_config, &req, Some(&on_chunk)).await;
            llm_elapsed = llm_start.elapsed();
            drop(on_chunk);

            match polish_result {
                Ok(response) => {
                    if self.abort_flag.load(Ordering::SeqCst) {
                        tracing::info!("Pipeline aborted after LLM polish, skipping output");
                        return Ok(());
                    }
                    final_text = response.polished_text;
                    if let Err(e) = self
                        .output_text(&final_text, &app_ctx)
                        .await
                    {
                        tracing::error!("Output failed: {}", e);
                        let _ = self
                            .app_handle
                            .emit("pipeline:error", output_error_message(&e));
                    }
                }
                Err(e) => {
                    if self.abort_flag.load(Ordering::SeqCst) {
                        tracing::info!("Pipeline aborted after LLM error, skipping output");
                        return Ok(());
                    }
                    tracing::error!("LLM polish failed: {}, outputting raw text", e);
                    final_text = raw_text.clone();
                    let _ = self
                        .app_handle
                        .emit("pipeline:error", format!("LLM polishing failed: {e}"));
                    if let Err(e) = self
                        .output_text(&final_text, &app_ctx)
                        .await
                    {
                        tracing::error!("Output failed: {}", e);
                        let _ = self
                            .app_handle
                            .emit("pipeline:error", output_error_message(&e));
                    }
                }
            }

            let ttft_ms = first_chunk_at
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .map(|d| d.as_millis() as i64)
                .unwrap_or(-1);
            tracing::info!(
                "[Pipeline Timing] LLM polish: {}ms (TTFT: {}ms, {} chunks)",
                llm_elapsed.as_millis(),
                ttft_ms,
                chunk_count.load(Ordering::SeqCst),
            );
        } else {
            llm_elapsed = std::time::Duration::ZERO;
            final_text = raw_text.clone();
            if let Err(e) = self
                .output_text(&final_text, &app_ctx)
                .await
            {
                tracing::error!("Output failed: {}", e);
                let _ = self
                    .app_handle
                    .emit("pipeline:error", output_error_message(&e));
            }
        }

        let total_elapsed = stop_start.elapsed();

        // Compute recording duration
        let duration_ms = self
            .recording_start
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
            .map(|start| start.elapsed().as_millis() as i64);

        tracing::info!(
            "[Pipeline Timing] Total stop(): {}ms (STT: {}ms, LLM: {}ms, Output+Save: {}ms)",
            total_elapsed.as_millis(),
            stt_elapsed.as_millis(),
            llm_elapsed.as_millis(),
            total_elapsed.as_millis() - stt_elapsed.as_millis() - llm_elapsed.as_millis(),
        );

        // Emit timing to frontend
        let _ = self.app_handle.emit(
            "pipeline:timing",
            serde_json::json!({
                "stt_ms": stt_elapsed.as_millis() as u64,
                "llm_ms": llm_elapsed.as_millis() as u64,
                "total_ms": total_elapsed.as_millis() as u64,
                "recording_ms": duration_ms,
                "detected_language": detected_language,
            }),
        );

        // Save to history
        let now = chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string();
        let entry = storage::HistoryEntry {
            id: 0, // auto-increment
            created_at: now,
            app_name: app_ctx.app_name,
            app_type: format!("{:?}", app_ctx.app_type),
            raw_text,
            polished_text: final_text.clone(),
            language: detected_language.clone(),
            duration_ms,
        };
        if let Err(e) = self
            .app_handle
            .state::<storage::HistoryStore>()
            .add(entry)
            .await
        {
            tracing::error!("Failed to save history: {}", e);
        }

        // Learn-from-corrections: watch for the user fixing one word in our typed output.
        if config.learn_from_corrections_enabled {
            let typed = with_trailing_space(&final_text);
            if !typed.trim().is_empty() {
                if let Some(field) = crate::correction::current_platform_field() {
                    let dictionary = self
                        .app_handle
                        .state::<std::sync::Arc<storage::DictionaryStore>>()
                        .inner()
                        .clone();
                    let app_handle = self.app_handle.clone();
                    let handle = crate::correction::spawn(
                        field,
                        dictionary,
                        typed,
                        move |sugg| {
                            let payload = serde_json::json!({
                                "rowId": sugg.row_id,
                                "old": sugg.old,
                                "new": sugg.new,
                                "autoConfirmMs": sugg.auto_confirm_ms,
                            });
                            if let Err(e) =
                                app_handle.emit_to("capsule", "correction:suggest", payload)
                            {
                                tracing::warn!("failed to emit correction:suggest: {}", e);
                            }
                            // Tell every window the dictionary just changed so the
                            // Settings → Dictionary list re-fetches without a restart.
                            if let Err(e) = app_handle.emit("dictionary:changed", ()) {
                                tracing::warn!("failed to emit dictionary:changed: {}", e);
                            }
                        },
                    );
                    *self
                        .current_correction
                        .lock()
                        .unwrap_or_else(|e| e.into_inner()) = Some(handle);
                }
            }
        }

        self.set_state(PipelineState::Idle);
        Ok(())
    }

    async fn output_text(
        &self,
        text: &str,
        app_ctx: &app_detector::AppContext,
    ) -> Result<()> {
        self.set_state(PipelineState::Outputting);

        // Paste relies on CGEventPost; without Accessibility the OS silently
        // drops every synthesised key and CGEventPost returns void, so we must
        // gate up front rather than detect after the fact.
        #[cfg(target_os = "macos")]
        if !is_accessibility_trusted() {
            let _ = request_accessibility_permission();
            anyhow::bail!("ACCESSIBILITY_REQUIRED");
        }

        // Trailing single space so successive dictations don't glue together
        // ("hello world" + "goodbye" → "hello world. goodbye." instead of
        // "hello world.goodbye."). History stores the un-normalized text.
        let typed = with_trailing_space(text);

        // Paste, then decide based on whether the receiving app actually
        // consumed it. For a single, non-terminal paste the output path uses
        // delayed-clipboard rendering to observe consumption; terminals and
        // chunked pastes are treated as reliable targets (always landed). When
        // nothing consumed the paste, the dictation was left on the clipboard —
        // surface a "press ⌘V to paste" tip so it isn't silently lost.
        //
        // `editable` (Accessibility seeing a focused text field) is passed so the
        // output path only restores the user's previous clipboard when it's
        // confident the paste landed in a field — a browser paste we can't verify
        // leaves the dictation on the clipboard instead of restoring over it.
        let is_terminal = output::target_is_terminal(app_ctx);
        let editable = crate::correction::focused_editable_present();
        let outcome = output::paste_text(&self.app_handle, &typed, app_ctx, editable).await?;
        let retain = should_retain_on_clipboard(outcome.landed, is_terminal);

        if retain {
            tracing::info!("Output paste did not land; left text on clipboard for manual paste");
            let _ = self.app_handle.emit_to("capsule", "output:no_target", ());
        }

        let _ = self.app_handle.emit("pipeline:target_app", &app_ctx.app_name);

        Ok(())
    }

    /// P1-2: Pre-warm HTTP connection pool by issuing a HEAD request to the STT endpoint.
    /// Call once after app startup to avoid cold-start TLS handshake on first recording.
    pub async fn pre_warm(&self) {
        let config = self.load_config().await;

        // Pre-warm STT endpoint
        let stt_endpoint = match config.stt_provider.as_str() {
            "glm-asr" => "https://open.bigmodel.cn/api/paas/v4/audio/transcriptions".to_string(),
            "openai-whisper" => "https://api.openai.com/v1/audio/transcriptions".to_string(),
            "groq-whisper" => "https://api.groq.com/openai/v1/audio/transcriptions".to_string(),
            "siliconflow" => "https://api.siliconflow.cn/v1/audio/transcriptions".to_string(),
            "deepgram" => "https://api.deepgram.com/v1/listen".to_string(),
            "assemblyai" => "https://api.assemblyai.com/v2/transcript".to_string(),
            _ => {
                tracing::debug!(
                    "Unknown STT provider '{}', skipping pre-warm",
                    config.stt_provider
                );
                return;
            }
        };
        tracing::debug!("Pre-warming HTTP connection to {}", stt_endpoint);
        let _ = self
            .shared_client
            .head(&stt_endpoint)
            .timeout(std::time::Duration::from_secs(5))
            .send()
            .await;
        tracing::debug!("STT connection pre-warm complete");

        // Pre-warm LLM endpoint if polish is enabled
        if config.polish_enabled {
            let llm_url = config.llm_base_url.clone();
            tracing::debug!("Pre-warming LLM connection to {}", llm_url);
            let _ = self
                .shared_client
                .head(&llm_url)
                .timeout(std::time::Duration::from_secs(5))
                .send()
                .await;
            tracing::debug!("LLM connection pre-warm complete");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        output_error_message, should_retain_on_clipboard, with_trailing_space,
        ACCESSIBILITY_REQUIRED_CODE,
    };

    #[test]
    fn retain_when_paste_did_not_land_and_not_terminal() {
        // The case the feature exists for: nothing consumed the paste in an
        // ordinary app → keep it on the clipboard and show the tip.
        assert!(should_retain_on_clipboard(false, false));
    }

    #[test]
    fn no_retain_when_paste_landed() {
        // Normal successful paste — restore the clipboard, no tip.
        assert!(!should_retain_on_clipboard(true, false));
    }

    #[test]
    fn no_retain_in_terminal_even_when_not_landed() {
        // Terminal guard: terminals are reliable paste targets, so never tip.
        assert!(!should_retain_on_clipboard(false, true));
    }

    #[test]
    fn no_retain_when_landed_in_terminal() {
        assert!(!should_retain_on_clipboard(true, true));
    }

    #[test]
    fn appends_single_space_to_normal_text() {
        assert_eq!(with_trailing_space("Hello world."), "Hello world. ");
    }

    #[test]
    fn collapses_existing_trailing_whitespace_to_one_space() {
        assert_eq!(with_trailing_space("Hello world.  \n\t"), "Hello world. ");
    }

    #[test]
    fn empty_input_yields_empty_output() {
        assert_eq!(with_trailing_space(""), "");
        assert_eq!(with_trailing_space("   \n"), "");
    }

    #[test]
    fn preserves_internal_newlines_in_lists() {
        let input = "1. Buy milk\n2. Do laundry\n3. Write the code";
        assert_eq!(
            with_trailing_space(input),
            "1. Buy milk\n2. Do laundry\n3. Write the code "
        );
    }

    #[test]
    fn handles_multibyte_terminal_punctuation() {
        // Japanese full-width period — must not panic and must append a single ASCII space.
        assert_eq!(with_trailing_space("こんにちは。"), "こんにちは。 ");
    }

    #[test]
    fn output_error_passes_accessibility_code_bare() {
        // Frontend matches on the exact string ACCESSIBILITY_REQUIRED. If we
        // wrapped it ("Output failed: ACCESSIBILITY_REQUIRED") the capsule's
        // permission-error branch would never fire and users would see the
        // raw token instead of the localized message.
        let err = anyhow::anyhow!(ACCESSIBILITY_REQUIRED_CODE);
        assert_eq!(output_error_message(&err), ACCESSIBILITY_REQUIRED_CODE);
    }

    #[test]
    fn output_error_wraps_other_errors() {
        let err = anyhow::anyhow!("Connection refused");
        assert_eq!(output_error_message(&err), "Output failed: Connection refused");
    }

    #[test]
    fn output_error_wraps_substring_matches() {
        // Only the exact code is special-cased — a message that merely
        // contains the token shouldn't be treated as a permission error.
        let err = anyhow::anyhow!("ACCESSIBILITY_REQUIRED somewhere in the middle");
        assert!(output_error_message(&err).starts_with("Output failed:"));
    }
}
