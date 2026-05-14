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
use crate::output::{self, OutputMode};
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

/// Wind down a streaming-keyboard typing task. Optionally emits a trailing space so successive
/// dictations don't glue together (matches `with_trailing_space` behavior for the batch path),
/// then closes the channel and waits for the typing task to drain. No-op if streaming wasn't in
/// use for this session.
async fn finish_streaming_typing(
    tx: Option<std::sync::mpsc::Sender<String>>,
    handle: Option<tokio::task::JoinHandle<Result<()>>>,
    trailing_space: bool,
) {
    if let Some(t) = tx {
        if trailing_space {
            let _ = t.send(" ".to_string());
        }
        drop(t);
    }
    if let Some(h) = handle {
        match h.await {
            Ok(Err(e)) => tracing::error!("Streaming keyboard typing failed: {}", e),
            Err(e) => tracing::error!("Streaming keyboard task join error: {}", e),
            Ok(Ok(())) => {}
        }
    }
}

/// On macOS, verify whether the process has been granted Accessibility (Assistive Access)
/// permission. enigo uses CGEventPost under the hood, which requires this permission;
/// without it all synthesised key events are silently dropped by the OS.
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

        // Clear accumulated text
        self.accumulated_text.lock().unwrap_or_else(|e| e.into_inner()).clear();

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

    pub async fn start(&self) -> Result<()> {
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

        // Clear accumulated text
        self.accumulated_text
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();

        // P0-2: Load config BEFORE starting audio capture — fail fast on missing API key
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
            "Pipeline using config: stt_provider={}, stt_key_len={}, stt_lang={}",
            config_data.stt_provider,
            config_data.stt_api_key.len(),
            config_data.stt_language
        );

        // Guard: empty API key — bail before starting audio
        if config_data.stt_api_key.is_empty() {
            let _ = self.app_handle.emit(
                "pipeline:error",
                "STT API key is not configured. Please set it in Settings → Speech Recognition.",
            );
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
            return Ok(());
        }

        // P0-3: Pre-connect STT provider before spawning task
        let stt_config = SttConfig {
            api_key: config_data.stt_api_key.clone(),
            language: if config_data.stt_language == "multi" {
                None
            } else {
                Some(config_data.stt_language.clone())
            },
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
            return Ok(());
        }

        // Start audio capture on dedicated thread
        let config = AudioConfig::default();
        let (handle, mut audio_rx) = match AudioCaptureHandle::start(config) {
            Ok(result) => result,
            Err(e) => {
                tracing::error!("Audio capture failed: {}", e);
                let _ = self.app_handle.emit(
                    "pipeline:error",
                    format!("Audio capture failed: {e}"),
                );
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
                return Ok(());
            }
        };

        // Store the audio handle's volume reference.
        // Check abort_flag first — if abort() was called while we were connecting
        // to STT, don't store the handle (it would be orphaned with nobody to stop it).
        if self.abort_flag.load(Ordering::SeqCst) {
            tracing::info!("Pipeline aborted during setup, discarding audio capture");
            // handle drops here, stopping the capture thread
            self.set_state(PipelineState::Idle);
            return Ok(());
        }
        let audio_vol = handle.get_volume();
        *self.audio_volume.lock().unwrap_or_else(|e| e.into_inner()) = audio_vol;
        *self.audio_handle.lock().unwrap_or_else(|e| e.into_inner()) = Some(handle);

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

        // Selected text will be captured in stop() after hotkey is released,
        // so Ctrl+C simulation won't conflict with held keys.
        *self
            .preloaded_selected_text
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = None;

        // STT streaming task — provider is already connected
        let app_handle = self.app_handle.clone();
        let accumulated = self.accumulated_text.clone();
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
                                    Ok(Some(text)) => {
                                        let mut acc = accumulated.lock().unwrap_or_else(|e| e.into_inner());
                                        acc.push_str(&text);
                                        let current = acc.clone();
                                        drop(acc);
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
                            Ok(Some(TranscriptEvent::Final { text, .. })) => {
                                let mut acc = accumulated.lock().unwrap_or_else(|e| e.into_inner());
                                acc.push_str(&text);
                                acc.push(' ');
                                let current = acc.clone();
                                drop(acc);
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

        // Always use batch output: keyboard mode uses output_text() after full LLM
        // response arrives. Streaming chunk-by-chunk clipboard paste was unreliable
        // on Windows — each Ctrl+V is async and the next set_text() could overwrite
        // the clipboard before the target app processed the previous paste, producing
        // garbled output that differed from what History recorded.

        // Pre-build LLM provider and Enigo while STT is still processing
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

            // Stream tokens directly into the foreground app's keyboard as they arrive
            // from the LLM, so the user sees text appear within ~200ms of stop instead of
            // waiting for the full response. Clipboard mode stays batched (chunk-by-chunk
            // clipboard paste was unreliable; see comment further up). Accessibility on
            // macOS is required for streaming-keyboard typing — fall back to batch output
            // when missing so the existing ACCESSIBILITY_REQUIRED error path still fires.
            let stream_keyboard = config.output_mode == "keyboard"
                && (cfg!(not(target_os = "macos")) || is_accessibility_trusted());

            let (typing_tx, typing_handle) = if stream_keyboard {
                let (tx, rx) = std::sync::mpsc::channel::<String>();
                let handle = tokio::task::spawn_blocking(move || output::keyboard::type_stream(rx));
                (Some(tx), Some(handle))
            } else {
                (None, None)
            };

            // on_chunk drives the UI capsule and (when streaming) the keyboard. Honors
            // the abort flag so a cancelled session doesn't keep typing. Counts sent
            // chunks so we can decide whether a polish failure is recoverable via raw
            // fallback (zero chunks typed) or has already produced partial output.
            let app_handle = self.app_handle.clone();
            let abort = self.abort_flag.clone();
            let typing_tx_chunk = typing_tx.clone();
            let sent_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
            let sent_count_chunk = sent_count.clone();
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
                if let Some(t) = &typing_tx_chunk {
                    if t.send(chunk.to_string()).is_ok() {
                        sent_count_chunk.fetch_add(1, Ordering::SeqCst);
                    }
                }
            });

            let req = PolishRequest {
                raw_text: raw_text.clone(),
                app_type: app_ctx.app_type,
                dictionary: dictionary_words,
                translate_enabled: config.translate_enabled,
                target_lang: config.target_lang.clone(),
                selected_text,
            };

            let polish_result = provider.polish(&llm_config, &req, Some(&on_chunk)).await;
            llm_elapsed = llm_start.elapsed();

            // Drop the chunk callback explicitly. The closure captures a clone of
            // typing_tx; if we leave it alive in scope, finish_streaming_typing will
            // drop the original sender but the closure clone keeps the channel open,
            // so type_stream's rx.recv() blocks forever and h.await never returns —
            // leaving state stuck and start() silently failing the Idle→Recording CAS.
            drop(on_chunk);

            match polish_result {
                Ok(response) => {
                    if self.abort_flag.load(Ordering::SeqCst) {
                        tracing::info!("Pipeline aborted after LLM polish, skipping output");
                        finish_streaming_typing(typing_tx, typing_handle, false).await;
                        return Ok(());
                    }
                    final_text = response.polished_text;

                    if stream_keyboard {
                        // The capsule shows the "Done" check mark on Outputting — only
                        // transition now, when the LLM has actually finished and we're
                        // draining the last chunks + trailing space, not earlier.
                        self.set_state(PipelineState::Outputting);
                        finish_streaming_typing(typing_tx, typing_handle, true).await;
                        let _ = self.app_handle.emit("pipeline:target_app", &app_ctx.app_name);
                    } else if let Err(e) = self
                        .output_text(&final_text, &app_ctx.app_name, &config)
                        .await
                    {
                        tracing::error!("Output failed: {}", e);
                        let _ = self
                            .app_handle
                            .emit("pipeline:error", format!("Output failed: {e}"));
                    }
                }
                Err(e) => {
                    if self.abort_flag.load(Ordering::SeqCst) {
                        tracing::info!("Pipeline aborted after LLM error, skipping output");
                        finish_streaming_typing(typing_tx, typing_handle, false).await;
                        return Ok(());
                    }
                    tracing::error!("LLM polish failed: {}, outputting raw text", e);
                    final_text = raw_text.clone();
                    let _ = self
                        .app_handle
                        .emit("pipeline:error", format!("LLM polishing failed: {e}"));

                    if stream_keyboard {
                        finish_streaming_typing(typing_tx, typing_handle, false).await;
                        // If nothing was typed, fall back to batch raw output. If some
                        // chunks were already typed, don't double-output — surface the
                        // partial result and the error and let the user decide.
                        if sent_count.load(Ordering::SeqCst) == 0 {
                            if let Err(e) = self
                                .output_text(&final_text, &app_ctx.app_name, &config)
                                .await
                            {
                                tracing::error!("Output failed: {}", e);
                                let _ = self.app_handle.emit(
                                    "pipeline:error",
                                    format!("Output failed: {e}"),
                                );
                            }
                        }
                    } else if let Err(e) = self
                        .output_text(&final_text, &app_ctx.app_name, &config)
                        .await
                    {
                        tracing::error!("Output failed: {}", e);
                        let _ = self
                            .app_handle
                            .emit("pipeline:error", format!("Output failed: {e}"));
                    }
                }
            }

            let ttft_ms = first_chunk_at
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .map(|d| d.as_millis() as i64)
                .unwrap_or(-1);
            tracing::info!(
                "[Pipeline Timing] LLM polish: {}ms (TTFT: {}ms, {} chunks{})",
                llm_elapsed.as_millis(),
                ttft_ms,
                sent_count.load(Ordering::SeqCst),
                if stream_keyboard { ", streaming-keyboard" } else { ", batch" }
            );
        } else {
            llm_elapsed = std::time::Duration::ZERO;
            final_text = raw_text.clone();
            if let Err(e) = self
                .output_text(&final_text, &app_ctx.app_name, &config)
                .await
            {
                tracing::error!("Output failed: {}", e);
                let _ = self
                    .app_handle
                    .emit("pipeline:error", format!("Output failed: {e}"));
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
            language: None,
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
        app_name: &str,
        config: &storage::AppConfig,
    ) -> Result<()> {
        self.set_state(PipelineState::Outputting);

        let mode = if config.output_mode == "keyboard" {
            OutputMode::Keyboard
        } else {
            OutputMode::Clipboard
        };

        // On macOS, keyboard output uses CGEventPost via enigo which requires
        // Accessibility permission. Clipboard mode uses osascript which does not.
        if mode == OutputMode::Keyboard && !is_accessibility_trusted() {
            anyhow::bail!("ACCESSIBILITY_REQUIRED");
        }

        // Trailing single space so successive dictations don't glue together
        // ("hello world" + "goodbye" → "hello world. goodbye." instead of
        // "hello world.goodbye."). History stores the un-normalized text.
        let typed = with_trailing_space(text);

        let output = output::create_output(mode);
        output.type_text(&typed).await?;

        let _ = self.app_handle.emit("pipeline:target_app", app_name);

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
    use super::with_trailing_space;

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
}
