pub mod app_detector;
pub mod audio;
pub mod correction;
pub mod llm;
pub mod output;
pub mod pipeline;
pub mod storage;
pub mod stt;

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::Emitter;
use tauri::Manager;
use tauri_plugin_autostart::MacosLauncher;
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};
use tauri_plugin_store::StoreExt;
use tracing_subscriber::EnvFilter;

use std::sync::{Arc, Mutex};

/// Cached hotkey mode to avoid loading config from disk on every keypress.
/// Updated whenever config is saved.
struct HotkeyModeCache(Arc<Mutex<String>>);

/// Cached close_to_tray setting to avoid blocking I/O in the window close handler.
struct CloseToTrayCache(Arc<Mutex<bool>>);

/// Managed tray icon handle for dynamic menu/tooltip updates.
pub struct TrayHandle {
    pub tray: Mutex<tauri::tray::TrayIcon>,
}

/// Persisted window position and size.
#[derive(serde::Serialize, serde::Deserialize, Clone)]
struct WindowState {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

/// Build (or rebuild) the system tray menu based on current state.
fn build_tray_menu(
    app: &tauri::AppHandle,
    is_recording: bool,
    window_visible: bool,
) -> Result<Menu<tauri::Wry>, Box<dyn std::error::Error>> {
    let show_hide = MenuItem::with_id(
        app,
        "show_hide",
        if window_visible {
            "Hide Window"
        } else {
            "Show Window"
        },
        true,
        None::<&str>,
    )?;
    let sep1 = PredefinedMenuItem::separator(app)?;
    let record = MenuItem::with_id(
        app,
        "record",
        if is_recording {
            "Stop Recording"
        } else {
            "Start Recording"
        },
        true,
        None::<&str>,
    )?;
    let sep2 = PredefinedMenuItem::separator(app)?;
    let settings = MenuItem::with_id(app, "settings", "Settings", true, None::<&str>)?;
    let history = MenuItem::with_id(app, "history", "History", true, None::<&str>)?;
    let sep3 = PredefinedMenuItem::separator(app)?;
    let about = MenuItem::with_id(app, "about", "About OpenTypeless", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;

    let menu = Menu::with_items(
        app,
        &[
            &show_hide, &sep1, &record, &sep2, &settings, &history, &sep3, &about, &quit,
        ],
    )?;
    Ok(menu)
}

/// Rebuild the tray menu and update tooltip based on pipeline state.
pub fn refresh_tray(app: &tauri::AppHandle) {
    let is_recording = app
        .try_state::<pipeline::PipelineHandle>()
        .map(|p| p.current_state() == pipeline::PipelineState::Recording)
        .unwrap_or(false);
    let window_visible = app
        .get_webview_window("main")
        .and_then(|w| w.is_visible().ok())
        .unwrap_or(false);

    if let Some(tray_handle) = app.try_state::<TrayHandle>() {
        if let Ok(tray) = tray_handle.tray.lock() {
            if let Ok(menu) = build_tray_menu(app, is_recording, window_visible) {
                let _ = tray.set_menu(Some(menu));
            }
        }
    }
}

#[tauri::command]
async fn start_recording(state: tauri::State<'_, pipeline::PipelineHandle>) -> Result<(), String> {
    state.start().await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn stop_recording(state: tauri::State<'_, pipeline::PipelineHandle>) -> Result<(), String> {
    state.stop().await.map_err(|e| e.to_string())
}

#[tauri::command]
fn abort_recording(state: tauri::State<'_, pipeline::PipelineHandle>) -> Result<(), String> {
    state.abort();
    Ok(())
}

#[tauri::command]
fn check_accessibility_permission() -> bool {
    pipeline::is_accessibility_trusted()
}

#[tauri::command]
fn request_accessibility_permission() -> bool {
    pipeline::request_accessibility_permission()
}

#[tauri::command]
fn check_microphone_permission() -> audio::MicAuthStatus {
    audio::check_microphone_permission()
}

#[tauri::command]
async fn request_microphone_permission() -> bool {
    audio::request_microphone_permission().await
}

#[tauri::command]
async fn get_config(
    state: tauri::State<'_, storage::ConfigManager>,
) -> Result<storage::AppConfig, String> {
    state.load().await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn update_config(
    app: tauri::AppHandle,
    state: tauri::State<'_, storage::ConfigManager>,
    cache: tauri::State<'_, HotkeyModeCache>,
    close_tray_cache: tauri::State<'_, CloseToTrayCache>,
    history: tauri::State<'_, storage::HistoryStore>,
    config: storage::AppConfig,
) -> Result<(), String> {
    *cache.0.lock().unwrap_or_else(|e| e.into_inner()) = config.hotkey_mode.clone();
    *close_tray_cache.0.lock().unwrap_or_else(|e| e.into_inner()) = config.close_to_tray;
    state.save(&config).await.map_err(|e| e.to_string())?;
    // Apply the (possibly lowered) history retention immediately, so the user
    // sees the setting take effect on Save instead of at next launch. A failed
    // DELETE must not fail the save — the config itself is already persisted.
    let pruned = history
        .prune_older_than(config.history_retention_days)
        .await;
    match pruned {
        Ok(0) => {}
        Ok(removed) => {
            tracing::info!("history retention pruned {} entries", removed);
            // `config:changed` only replaces each webview's config copy, so without
            // this the History pane keeps listing the rows we just deleted and the
            // user concludes the setting did nothing.
            if let Err(e) = app.emit("history:changed", ()) {
                tracing::warn!("failed to emit history:changed: {}", e);
            }
        }
        Err(e) => tracing::warn!("history retention prune failed: {}", e),
    }
    // Broadcast to every webview (main + capsule) so each window's Zustand
    // can replace its local config copy without an app restart. The capsule
    // is the load-bearing case — its show/hide is derived from
    // `capsule_auto_hide`, and without this event the toggle in Settings
    // wouldn't take effect until next launch.
    if let Err(e) = app.emit("config:changed", &config) {
        tracing::warn!("failed to emit config:changed: {}", e);
    }
    Ok(())
}

#[tauri::command]
async fn test_stt_connection(api_key: String, provider: String) -> Result<bool, String> {
    if provider.is_empty() {
        return Ok(false);
    }

    if api_key.is_empty() {
        return Ok(false);
    }

    match provider.as_str() {
        "deepgram" => {
            let client = reqwest::Client::new();
            let resp = client
                .get("https://api.deepgram.com/v1/projects")
                .header("Authorization", format!("Token {}", api_key))
                .timeout(std::time::Duration::from_secs(10))
                .send()
                .await
                .map_err(|e| e.to_string())?;
            Ok(resp.status().is_success())
        }
        "assemblyai" => {
            let client = reqwest::Client::new();
            let resp = client
                .get("https://api.assemblyai.com/v2/transcript?limit=1")
                .header("Authorization", api_key)
                .timeout(std::time::Duration::from_secs(10))
                .send()
                .await
                .map_err(|e| e.to_string())?;
            Ok(resp.status().is_success())
        }
        "glm-asr" | "openai-whisper" | "groq-whisper" | "siliconflow" => {
            // All four use Whisper-compatible file upload API
            let (endpoint, model, extra_fields): (&str, &str, &[(&str, &str)]) =
                match provider.as_str() {
                    "glm-asr" => (
                        "https://open.bigmodel.cn/api/paas/v4/audio/transcriptions",
                        "glm-asr-2512",
                        &[("stream", "false")][..],
                    ),
                    "openai-whisper" => (
                        "https://api.openai.com/v1/audio/transcriptions",
                        "whisper-1",
                        &[][..],
                    ),
                    "groq-whisper" => (
                        "https://api.groq.com/openai/v1/audio/transcriptions",
                        "whisper-large-v3-turbo",
                        &[][..],
                    ),
                    _ => (
                        "https://api.siliconflow.cn/v1/audio/transcriptions",
                        "FunAudioLLM/SenseVoiceSmall",
                        &[][..],
                    ),
                };

            let silent_pcm = vec![0u8; 3200]; // 0.1s at 16kHz 16-bit mono
            let wav = stt::whisper_compat::WhisperCompatProvider::build_wav(&silent_pcm, 16000);

            let file_part = reqwest::multipart::Part::bytes(wav)
                .file_name("test.wav")
                .mime_str("audio/wav")
                .map_err(|e| e.to_string())?;
            let mut form = reqwest::multipart::Form::new()
                .text("model", model.to_string())
                .part("file", file_part);
            for &(key, value) in extra_fields {
                form = form.text(key.to_string(), value.to_string());
            }

            let client = reqwest::Client::new();
            let resp = client
                .post(endpoint)
                .header("Authorization", format!("Bearer {}", api_key))
                .multipart(form)
                .timeout(std::time::Duration::from_secs(15))
                .send()
                .await
                .map_err(|e| e.to_string())?;
            Ok(resp.status().is_success())
        }
        _ => Err(format!("Unknown STT provider: {}", provider)),
    }
}

#[tauri::command]
async fn test_llm_connection(
    api_key: String,
    provider: String,
    base_url: String,
    model: String,
) -> Result<bool, String> {
    if provider.is_empty() {
        return Ok(false);
    }

    if api_key.is_empty() || base_url.is_empty() {
        return Ok(false);
    }

    // Validate base_url is a proper HTTP(S) URL
    let parsed = url::Url::parse(&base_url).map_err(|e| format!("Invalid base URL: {e}"))?;
    if parsed.scheme() != "https" && parsed.scheme() != "http" {
        return Err("Base URL must use http or https scheme".to_string());
    }

    let client = reqwest::Client::new();
    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
    let body = serde_json::json!({
        "model": model,
        "messages": [{"role": "user", "content": "hi"}],
        "max_tokens": 1
    });

    let resp = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&body)
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await
        .map_err(|e| e.to_string())?;

    Ok(resp.status().is_success())
}

#[tauri::command]
async fn fetch_llm_models(api_key: String, base_url: String) -> Result<Vec<String>, String> {
    if base_url.is_empty() {
        return Ok(vec![]);
    }

    // Validate base_url is a proper HTTP(S) URL
    let parsed = url::Url::parse(&base_url).map_err(|e| format!("Invalid base URL: {e}"))?;
    if parsed.scheme() != "https" && parsed.scheme() != "http" {
        return Err("Base URL must use http or https scheme".to_string());
    }

    let client = reqwest::Client::new();
    let url = format!("{}/models", base_url.trim_end_matches('/'));

    let resp = client
        .get(&url)
        .header("Authorization", format!("Bearer {}", api_key))
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !resp.status().is_success() {
        return Ok(vec![]);
    }

    let body: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;

    // OpenAI-compatible: { data: [{ id: "model-name" }] }
    // Ollama-compatible: { models: [{ name: "model-name" }] }
    let mut models: Vec<String> = Vec::new();

    if let Some(data) = body.get("data").and_then(|d| d.as_array()) {
        for item in data {
            if let Some(id) = item.get("id").and_then(|v| v.as_str()) {
                models.push(id.to_string());
            }
        }
    } else if let Some(data) = body.get("models").and_then(|d| d.as_array()) {
        for item in data {
            if let Some(name) = item.get("name").and_then(|v| v.as_str()) {
                models.push(name.to_string());
            }
        }
    }

    models.sort();
    Ok(models)
}

#[tauri::command]
async fn bench_stt_connection(api_key: String, provider: String) -> Result<u32, String> {
    if provider.is_empty() {
        return Err("No provider specified".to_string());
    }

    if api_key.is_empty() {
        return Err("API key is empty".to_string());
    }

    match provider.as_str() {
        "deepgram" => {
            let client = reqwest::Client::new();
            let t0 = std::time::Instant::now();
            let resp = client
                .get("https://api.deepgram.com/v1/projects")
                .header("Authorization", format!("Token {}", api_key))
                .timeout(std::time::Duration::from_secs(10))
                .send()
                .await
                .map_err(|e| e.to_string())?;
            let elapsed = t0.elapsed().as_millis() as u32;
            if !resp.status().is_success() {
                return Err(format!("HTTP {}", resp.status()));
            }
            Ok(elapsed)
        }
        "assemblyai" => {
            let client = reqwest::Client::new();
            let t0 = std::time::Instant::now();
            let resp = client
                .get("https://api.assemblyai.com/v2/transcript?limit=1")
                .header("Authorization", api_key)
                .timeout(std::time::Duration::from_secs(10))
                .send()
                .await
                .map_err(|e| e.to_string())?;
            let elapsed = t0.elapsed().as_millis() as u32;
            if !resp.status().is_success() {
                return Err(format!("HTTP {}", resp.status()));
            }
            Ok(elapsed)
        }
        "glm-asr" | "openai-whisper" | "groq-whisper" | "siliconflow" => {
            let (endpoint, model, extra_fields): (&str, &str, &[(&str, &str)]) =
                match provider.as_str() {
                    "glm-asr" => (
                        "https://open.bigmodel.cn/api/paas/v4/audio/transcriptions",
                        "glm-asr-2512",
                        &[("stream", "false")][..],
                    ),
                    "openai-whisper" => (
                        "https://api.openai.com/v1/audio/transcriptions",
                        "whisper-1",
                        &[][..],
                    ),
                    "groq-whisper" => (
                        "https://api.groq.com/openai/v1/audio/transcriptions",
                        "whisper-large-v3-turbo",
                        &[][..],
                    ),
                    _ => (
                        "https://api.siliconflow.cn/v1/audio/transcriptions",
                        "FunAudioLLM/SenseVoiceSmall",
                        &[][..],
                    ),
                };

            let silent_pcm = vec![0u8; 3200]; // 0.1s at 16kHz 16-bit mono
            let wav = stt::whisper_compat::WhisperCompatProvider::build_wav(&silent_pcm, 16000);

            let file_part = reqwest::multipart::Part::bytes(wav)
                .file_name("test.wav")
                .mime_str("audio/wav")
                .map_err(|e| e.to_string())?;
            let mut form = reqwest::multipart::Form::new()
                .text("model", model.to_string())
                .part("file", file_part);
            for &(key, value) in extra_fields {
                form = form.text(key.to_string(), value.to_string());
            }

            let client = reqwest::Client::new();
            let t0 = std::time::Instant::now();
            let resp = client
                .post(endpoint)
                .header("Authorization", format!("Bearer {}", api_key))
                .multipart(form)
                .timeout(std::time::Duration::from_secs(15))
                .send()
                .await
                .map_err(|e| e.to_string())?;
            let elapsed = t0.elapsed().as_millis() as u32;
            if !resp.status().is_success() {
                return Err(format!("HTTP {}", resp.status()));
            }
            Ok(elapsed)
        }
        _ => Err(format!("Unknown STT provider: {}", provider)),
    }
}

#[tauri::command]
async fn bench_llm_connection(
    api_key: String,
    provider: String,
    base_url: String,
    model: String,
) -> Result<u32, String> {
    if provider.is_empty() {
        return Err("No provider specified".to_string());
    }

    if api_key.is_empty() || base_url.is_empty() {
        return Err("API key or base URL is empty".to_string());
    }

    let parsed = url::Url::parse(&base_url).map_err(|e| format!("Invalid base URL: {e}"))?;
    if parsed.scheme() != "https" && parsed.scheme() != "http" {
        return Err("Base URL must use http or https scheme".to_string());
    }

    let client = reqwest::Client::new();
    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
    let body = serde_json::json!({
        "model": model,
        "messages": [{"role": "user", "content": "hi"}],
        "max_tokens": 1
    });

    let t0 = std::time::Instant::now();
    let resp = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&body)
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let elapsed = t0.elapsed().as_millis() as u32;

    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()));
    }

    Ok(elapsed)
}

#[tauri::command]
async fn get_history(
    state: tauri::State<'_, storage::HistoryStore>,
    limit: u32,
    offset: u32,
) -> Result<Vec<storage::HistoryEntry>, String> {
    state.list(limit, offset).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn clear_history(state: tauri::State<'_, storage::HistoryStore>) -> Result<(), String> {
    state.clear().await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn get_dictionary(
    state: tauri::State<'_, std::sync::Arc<storage::DictionaryStore>>,
) -> Result<Vec<storage::DictionaryEntry>, String> {
    state.list().await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn add_dictionary_entry(
    state: tauri::State<'_, std::sync::Arc<storage::DictionaryStore>>,
    word: String,
    pronunciation: Option<String>,
) -> Result<(), String> {
    let word = word.trim().to_string();
    if word.is_empty() {
        return Err("Word cannot be empty".to_string());
    }
    if word.len() > 100 {
        return Err("Word is too long (max 100 characters)".to_string());
    }
    if let Some(ref p) = pronunciation {
        if p.len() > 100 {
            return Err("Pronunciation is too long (max 100 characters)".to_string());
        }
    }
    state
        .add_manual(&word, pronunciation.as_deref())
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn remove_dictionary_entry(
    state: tauri::State<'_, std::sync::Arc<storage::DictionaryStore>>,
    id: i64,
) -> Result<(), String> {
    state.remove(id).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn correction_undo(
    app: tauri::AppHandle,
    state: tauri::State<'_, std::sync::Arc<storage::DictionaryStore>>,
    row_id: i64,
) -> Result<(), String> {
    state.remove(row_id).await.map_err(|e| e.to_string())?;
    if let Err(e) = app.emit("dictionary:changed", ()) {
        tracing::warn!("failed to emit dictionary:changed: {}", e);
    }
    Ok(())
}

#[tauri::command]
async fn set_auto_start(
    app: tauri::AppHandle,
    config_state: tauri::State<'_, storage::ConfigManager>,
    enabled: bool,
) -> Result<(), String> {
    use tauri_plugin_autostart::ManagerExt;
    let autolaunch = app.autolaunch();
    if enabled {
        autolaunch.enable().map_err(|e| e.to_string())?;
    } else {
        autolaunch.disable().map_err(|e| e.to_string())?;
    }
    let mut config = config_state.load().await.map_err(|e| e.to_string())?;
    config.auto_start = enabled;
    config_state
        .save(&config)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
async fn update_hotkey(
    app: tauri::AppHandle,
    config_state: tauri::State<'_, storage::ConfigManager>,
    hotkey: String,
) -> Result<(), String> {
    let new_shortcut =
        parse_hotkey(&hotkey).ok_or_else(|| format!("Invalid hotkey: {}", hotkey))?;

    // Unregister all existing shortcuts, then register the new one
    // (the global handler from with_handler is still active)
    app.global_shortcut()
        .unregister_all()
        .map_err(|e| e.to_string())?;
    app.global_shortcut()
        .register(new_shortcut)
        .map_err(|e| e.to_string())?;

    // Save updated hotkey to config
    let mut config = config_state.load().await.map_err(|e| e.to_string())?;
    config.hotkey = hotkey;
    config_state
        .save(&config)
        .await
        .map_err(|e| e.to_string())?;

    Ok(())
}

/// Temporarily unregister all global shortcuts so the webview can capture key events.
#[tauri::command]
fn pause_hotkey(app: tauri::AppHandle) -> Result<(), String> {
    app.global_shortcut()
        .unregister_all()
        .map_err(|e| e.to_string())
}

/// Re-register the current hotkey from config after recording is done.
#[tauri::command]
async fn resume_hotkey(
    app: tauri::AppHandle,
    config_state: tauri::State<'_, storage::ConfigManager>,
) -> Result<(), String> {
    let config = config_state.load().await.map_err(|e| e.to_string())?;
    let shortcut = parse_hotkey(&config.hotkey).unwrap_or_else(default_shortcut);
    // Ensure clean state, then register
    let _ = app.global_shortcut().unregister_all();
    app.global_shortcut()
        .register(shortcut)
        .map_err(|e| e.to_string())
}

/// Configure the capsule's NSWindow so it overlays other apps' fullscreen
/// Spaces. Three changes, all needed:
///   1. Swap the object's class from `NSWindow` to `NSPanel` and OR
///      `NSWindowStyleMaskNonactivatingPanel` into the style mask. macOS
///      excludes regular `NSWindow`s from foreign fullscreen Spaces; only
///      `NSPanel` with the nonactivating style is allowed.
///   2. OR `NSWindowCollectionBehaviorFullScreenAuxiliary` into the collection
///      behavior so the panel is allowed to render in fullscreen Spaces.
///      `CanJoinAllSpaces` alone is not sufficient on macOS Tahoe.
///   3. Raise the window level to `NSPopUpMenuWindowLevel` (101). Tauri's
///      `alwaysOnTop` only sets `NSFloatingWindowLevel` (3), which sits below
///      a fullscreen app's content, so the capsule would be hidden behind it.
///
/// All read-modify-write so we don't clobber flags Tauri/tao already set.
/// Safe because `NSPanel` inherits from `NSWindow` and adds no extra ivars,
/// so the existing `NSWindow` allocation has the right layout.
#[cfg(target_os = "macos")]
fn configure_capsule_collection_behavior(ns_window: *mut std::ffi::c_void) {
    use std::ffi::c_void;
    use std::os::raw::c_char;

    #[link(name = "objc", kind = "dylib")]
    extern "C" {
        fn sel_registerName(name: *const c_char) -> *mut c_void;
        fn objc_getClass(name: *const c_char) -> *mut c_void;
        fn object_setClass(obj: *mut c_void, class: *mut c_void) -> *mut c_void;
    }
    extern "C" {
        fn objc_msgSend();
    }

    if ns_window.is_null() {
        return;
    }

    // NSWindowStyleMaskNonactivatingPanel = 1 << 7
    const NONACTIVATING_PANEL: u64 = 1 << 7;
    // NSWindowCollectionBehaviorFullScreenAuxiliary = 1 << 8
    const FULL_SCREEN_AUXILIARY: u64 = 1 << 8;
    // NSPopUpMenuWindowLevel — above status bar, below cursor; survives other apps' fullscreen.
    const POPUP_MENU_LEVEL: i64 = 101;

    unsafe {
        // Swap the class to NSPanel so the nonactivating-panel style takes effect.
        let panel_class = objc_getClass(c"NSPanel".as_ptr());
        if !panel_class.is_null() {
            object_setClass(ns_window, panel_class);
        }

        let get_style = sel_registerName(c"styleMask".as_ptr());
        let set_style = sel_registerName(c"setStyleMask:".as_ptr());
        let get_collection = sel_registerName(c"collectionBehavior".as_ptr());
        let set_collection = sel_registerName(c"setCollectionBehavior:".as_ptr());
        let set_level = sel_registerName(c"setLevel:".as_ptr());
        if get_style.is_null()
            || set_style.is_null()
            || get_collection.is_null()
            || set_collection.is_null()
            || set_level.is_null()
        {
            return;
        }

        let get_u64: unsafe extern "C" fn(*mut c_void, *mut c_void) -> u64 =
            std::mem::transmute(objc_msgSend as *const c_void);
        let set_u64: unsafe extern "C" fn(*mut c_void, *mut c_void, u64) =
            std::mem::transmute(objc_msgSend as *const c_void);
        let set_i64: unsafe extern "C" fn(*mut c_void, *mut c_void, i64) =
            std::mem::transmute(objc_msgSend as *const c_void);

        let style = get_u64(ns_window, get_style);
        set_u64(ns_window, set_style, style | NONACTIVATING_PANEL);

        let collection = get_u64(ns_window, get_collection);
        set_u64(
            ns_window,
            set_collection,
            collection | FULL_SCREEN_AUXILIARY,
        );

        set_i64(ns_window, set_level, POPUP_MENU_LEVEL);
    }
}

// ─── Hotkey parsing ───

fn default_shortcut() -> Shortcut {
    let default_hotkey = storage::AppConfig::default().hotkey;
    let fallback = {
        #[cfg(target_os = "macos")]
        {
            Shortcut::new(Some(Modifiers::ALT), Code::Slash)
        }
        #[cfg(not(target_os = "macos"))]
        {
            Shortcut::new(Some(Modifiers::CONTROL), Code::Slash)
        }
    };
    parse_hotkey(&default_hotkey).unwrap_or(fallback)
}

fn build_shortcut_handler(
    app_handle: tauri::AppHandle,
) -> impl Fn(&tauri::AppHandle, &Shortcut, tauri_plugin_global_shortcut::ShortcutEvent)
       + Send
       + Sync
       + 'static {
    move |_app, _shortcut, event| {
        let handle = app_handle.clone();
        match event.state {
            ShortcutState::Pressed => {
                let hotkey_mode = handle
                    .state::<HotkeyModeCache>()
                    .0
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .clone();
                tauri::async_runtime::spawn(async move {
                    let pipeline = handle.state::<pipeline::PipelineHandle>();

                    if hotkey_mode == "toggle" {
                        if pipeline.current_state() == pipeline::PipelineState::Idle {
                            if let Err(e) = pipeline.start().await {
                                tracing::error!("Failed to start recording: {}", e);
                                let _ = handle.emit("pipeline:error", e.to_string());
                            }
                        } else if let Err(e) = pipeline.stop().await {
                            tracing::error!("Failed to stop recording: {}", e);
                            let _ = handle.emit("pipeline:error", e.to_string());
                        }
                    } else if let Err(e) = pipeline.start().await {
                        tracing::error!("Failed to start recording: {}", e);
                        let _ = handle.emit("pipeline:error", e.to_string());
                    }
                });
            }
            ShortcutState::Released => {
                let hotkey_mode = handle
                    .state::<HotkeyModeCache>()
                    .0
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .clone();
                if hotkey_mode != "toggle" {
                    tauri::async_runtime::spawn(async move {
                        let pipeline = handle.state::<pipeline::PipelineHandle>();
                        if let Err(e) = pipeline.stop().await {
                            tracing::error!("Failed to stop recording: {}", e);
                            let _ = handle.emit("pipeline:error", e.to_string());
                        }
                    });
                }
            }
        }
    }
}

fn parse_hotkey(s: &str) -> Option<Shortcut> {
    let parts: Vec<&str> = s.split('+').map(|p| p.trim()).collect();
    if parts.is_empty() {
        return None;
    }

    let mut modifiers = Modifiers::empty();
    let key_str = parts.last()?;

    for &part in &parts[..parts.len() - 1] {
        match part.to_lowercase().as_str() {
            "alt" => modifiers |= Modifiers::ALT,
            "ctrl" | "control" => modifiers |= Modifiers::CONTROL,
            "shift" => modifiers |= Modifiers::SHIFT,
            "meta" | "super" | "win" | "cmd" => modifiers |= Modifiers::META,
            _ => return None,
        }
    }

    let code = match key_str.to_lowercase().as_str() {
        "space" => Code::Space,
        "tab" => Code::Tab,
        "enter" | "return" => Code::Enter,
        "backspace" => Code::Backspace,
        "escape" | "esc" => Code::Escape,
        "delete" => Code::Delete,
        "insert" => Code::Insert,
        "home" => Code::Home,
        "end" => Code::End,
        "pageup" => Code::PageUp,
        "pagedown" => Code::PageDown,
        "arrowup" | "up" => Code::ArrowUp,
        "arrowdown" | "down" => Code::ArrowDown,
        "arrowleft" | "left" => Code::ArrowLeft,
        "arrowright" | "right" => Code::ArrowRight,
        "f1" => Code::F1,
        "f2" => Code::F2,
        "f3" => Code::F3,
        "f4" => Code::F4,
        "f5" => Code::F5,
        "f6" => Code::F6,
        "f7" => Code::F7,
        "f8" => Code::F8,
        "f9" => Code::F9,
        "f10" => Code::F10,
        "f11" => Code::F11,
        "f12" => Code::F12,
        "a" => Code::KeyA,
        "b" => Code::KeyB,
        "c" => Code::KeyC,
        "d" => Code::KeyD,
        "e" => Code::KeyE,
        "f" => Code::KeyF,
        "g" => Code::KeyG,
        "h" => Code::KeyH,
        "i" => Code::KeyI,
        "j" => Code::KeyJ,
        "k" => Code::KeyK,
        "l" => Code::KeyL,
        "m" => Code::KeyM,
        "n" => Code::KeyN,
        "o" => Code::KeyO,
        "p" => Code::KeyP,
        "q" => Code::KeyQ,
        "r" => Code::KeyR,
        "s" => Code::KeyS,
        "t" => Code::KeyT,
        "u" => Code::KeyU,
        "v" => Code::KeyV,
        "w" => Code::KeyW,
        "x" => Code::KeyX,
        "y" => Code::KeyY,
        "z" => Code::KeyZ,
        "0" => Code::Digit0,
        "1" => Code::Digit1,
        "2" => Code::Digit2,
        "3" => Code::Digit3,
        "4" => Code::Digit4,
        "5" => Code::Digit5,
        "6" => Code::Digit6,
        "7" => Code::Digit7,
        "8" => Code::Digit8,
        "9" => Code::Digit9,
        "/" | "slash" => Code::Slash,
        "\\" | "backslash" => Code::Backslash,
        "." | "period" => Code::Period,
        "," | "comma" => Code::Comma,
        ";" | "semicolon" => Code::Semicolon,
        "'" | "quote" => Code::Quote,
        "`" | "backquote" => Code::Backquote,
        "-" | "minus" => Code::Minus,
        "=" | "equal" => Code::Equal,
        "[" | "bracketleft" => Code::BracketLeft,
        "]" | "bracketright" => Code::BracketRight,
        _ => return None,
    };

    let mods = if modifiers.is_empty() {
        None
    } else {
        Some(modifiers)
    };
    Some(Shortcut::new(mods, code))
}

/// Decide whether to surface the main window on app startup.
///
/// OpenTypeless lives in the system tray, so the main window stays hidden
/// most of the time. The exception is when the user is still in onboarding —
/// either because the STT API key is unset (a fresh install) or because the
/// `onboarding_completed` flag is missing / false (a re-test or a flag drop).
/// Without surfacing the window in that case, onboarding renders into an
/// invisible web view and the user has no path to it.
fn should_show_window_on_launch(stt_key_empty: bool, onboarding_completed: bool) -> bool {
    stt_key_empty || !onboarding_completed
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── should_show_window_on_launch ─────────────────────────────────────
    // Truth table for the predicate that decides whether to surface the main
    // window at startup. The "key set + onboarding incomplete" row guards
    // the regression where dropping `onboarding_completed` for a re-test
    // left the user staring at the tray with no visible window.

    #[test]
    fn show_window_on_first_run_no_key_no_onboarding() {
        assert!(should_show_window_on_launch(true, false));
    }

    #[test]
    fn show_window_when_key_present_but_onboarding_incomplete() {
        // The regression. Previously the predicate was just
        // `stt_key_empty`, so this returned false and onboarding rendered
        // into a hidden window.
        assert!(should_show_window_on_launch(false, false));
    }

    #[test]
    fn show_window_when_key_missing_even_if_onboarding_marked_done() {
        // Edge case: onboarding flag stuck true but the user wiped the key
        // somehow. Surface the window so they can re-enter the key.
        assert!(should_show_window_on_launch(true, true));
    }

    #[test]
    fn keep_window_hidden_on_normal_launch() {
        // Configured + onboarded: tray-only launch.
        assert!(!should_show_window_on_launch(false, true));
    }

    #[test]
    fn test_parse_hotkey_ctrl_slash() {
        let s = parse_hotkey("Ctrl+/");
        assert!(s.is_some());
        let s = s.unwrap();
        assert_eq!(s.mods, Modifiers::CONTROL);
        assert_eq!(s.key, Code::Slash);
    }

    #[test]
    fn test_parse_hotkey_ctrl_shift_a() {
        let s = parse_hotkey("Ctrl+Shift+A");
        assert!(s.is_some());
        let s = s.unwrap();
        assert_eq!(s.mods, Modifiers::CONTROL | Modifiers::SHIFT);
        assert_eq!(s.key, Code::KeyA);
    }

    #[test]
    fn test_parse_hotkey_case_insensitive() {
        let s = parse_hotkey("cTrL+/");
        assert!(s.is_some());
        let s = s.unwrap();
        assert_eq!(s.mods, Modifiers::CONTROL);
        assert_eq!(s.key, Code::Slash);
    }

    #[test]
    fn test_parse_hotkey_f_keys() {
        for (key, expected) in [("F1", Code::F1), ("F12", Code::F12)] {
            let s = parse_hotkey(&format!("Ctrl+{}", key));
            assert!(s.is_some(), "Failed to parse Ctrl+{}", key);
            assert_eq!(s.unwrap().key, expected);
        }
    }

    #[test]
    fn test_parse_hotkey_meta_modifier() {
        for name in ["Meta", "Super", "Win", "Cmd"] {
            let s = parse_hotkey(&format!("{}+A", name));
            assert!(s.is_some(), "Failed to parse {}+A", name);
            assert_eq!(s.unwrap().mods, Modifiers::SUPER);
        }
    }

    #[test]
    fn test_parse_hotkey_no_modifier() {
        let s = parse_hotkey("A");
        assert!(s.is_some());
        assert_eq!(s.unwrap().mods, Modifiers::empty());
    }

    #[test]
    fn test_parse_hotkey_invalid_key() {
        let s = parse_hotkey("Alt+InvalidKey");
        assert!(s.is_none());
    }

    #[test]
    fn test_parse_hotkey_empty_string() {
        let s = parse_hotkey("");
        assert!(s.is_none());
    }

    #[test]
    fn test_parse_hotkey_digits() {
        let s = parse_hotkey("Ctrl+0");
        assert!(s.is_some());
        assert_eq!(s.unwrap().key, Code::Digit0);

        let s = parse_hotkey("Ctrl+9");
        assert!(s.is_some());
        assert_eq!(s.unwrap().key, Code::Digit9);
    }

    #[test]
    fn test_parse_hotkey_navigation_keys() {
        for (key, expected) in [
            ("Enter", Code::Enter),
            ("Tab", Code::Tab),
            ("Escape", Code::Escape),
            ("Backspace", Code::Backspace),
            ("Delete", Code::Delete),
            ("Up", Code::ArrowUp),
            ("Down", Code::ArrowDown),
        ] {
            let s = parse_hotkey(&format!("Alt+{}", key));
            assert!(s.is_some(), "Failed to parse Alt+{}", key);
            assert_eq!(s.unwrap().key, expected);
        }
    }
}
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::from_default_env().add_directive(
                "opentypeless=debug"
                    .parse()
                    .expect("static directive is valid"),
            ),
        )
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_sql::Builder::default().build())
        .plugin(tauri_plugin_store::Builder::default().build())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            // Focus the main window so a second launch surfaces the existing instance.
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .setup(|app| {
            // Open devtools only when the "devtools" feature is explicitly enabled
            #[cfg(feature = "devtools")]
            {
                if let Some(window) = app.get_webview_window("main") {
                    window.open_devtools();
                }
                if let Some(window) = app.get_webview_window("capsule") {
                    window.open_devtools();
                }
            }

            let app_handle = app.handle().clone();

            // Initialize data directory and database
            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            let db_path = data_dir.join("opentypeless.db");

            // Initialize stores
            let config_manager = storage::ConfigManager::new(app_handle.clone());
            let history_store = storage::HistoryStore::new(db_path.clone())
                .map_err(|e| anyhow::anyhow!("Failed to init history store: {}", e))?;
            let dictionary_store = std::sync::Arc::new(
                storage::DictionaryStore::new(db_path)
                    .map_err(|e| anyhow::anyhow!("Failed to init dictionary store: {}", e))?,
            );
            let pipeline_handle = pipeline::PipelineHandle::new(app_handle.clone());

            // Load initial config to get hotkey
            let initial_config =
                tauri::async_runtime::block_on(config_manager.load()).unwrap_or_default();
            let shortcut = parse_hotkey(&initial_config.hotkey).unwrap_or_else(default_shortcut);

            // Enforce history retention once at launch. This is the path that
            // matters when history is disabled: nothing is inserted, so the
            // insert-time prune never runs and old rows would linger forever.
            // Logged even on success: this is the one destructive prune that runs
            // unattended, so without a count a user reporting "my history is gone"
            // can't be distinguished from db corruption or an accidental Clear all.
            match tauri::async_runtime::block_on(
                history_store.prune_older_than(initial_config.history_retention_days),
            ) {
                Ok(0) => {}
                Ok(removed) => tracing::info!(
                    "history retention pruned {} entries at startup (retention {} days)",
                    removed,
                    initial_config.history_retention_days
                ),
                Err(e) => tracing::warn!("history retention prune at startup failed: {}", e),
            }

            app.manage(config_manager);
            app.manage(history_store);
            app.manage(dictionary_store);
            app.manage(pipeline_handle);
            app.manage(HotkeyModeCache(Arc::new(Mutex::new(
                initial_config.hotkey_mode.clone(),
            ))));
            app.manage(CloseToTrayCache(Arc::new(Mutex::new(
                initial_config.close_to_tray,
            ))));

            // Sync auto-start state with system
            {
                use tauri_plugin_autostart::ManagerExt;
                let autolaunch = app.handle().autolaunch();
                let is_enabled = autolaunch.is_enabled().unwrap_or(false);
                if initial_config.auto_start && !is_enabled {
                    let _ = autolaunch.enable();
                } else if !initial_config.auto_start && is_enabled {
                    let _ = autolaunch.disable();
                }
            }

            // Register global shortcut from config
            let handler = build_shortcut_handler(app_handle.clone());
            app.handle().plugin(
                tauri_plugin_global_shortcut::Builder::new()
                    .with_handler(handler)
                    .build(),
            )?;
            if let Err(e) = app.global_shortcut().register(shortcut) {
                tracing::warn!(
                    "Failed to register shortcut '{}' (may be occupied): {e}",
                    initial_config.hotkey
                );
            }

            // System tray
            let tray_menu = build_tray_menu(&app_handle, false, true)
                .map_err(|e| anyhow::anyhow!("Failed to build tray menu: {}", e))?;

            let tray = TrayIconBuilder::new()
                .icon(
                    app.default_window_icon()
                        .expect("default window icon missing")
                        .clone(),
                )
                .menu(&tray_menu)
                .tooltip("OpenTypeless")
                .on_menu_event(move |app, event| match event.id.as_ref() {
                    "quit" => {
                        app.exit(0);
                    }
                    "show_hide" => {
                        if let Some(window) = app.get_webview_window("main") {
                            let visible = window.is_visible().unwrap_or(false);
                            if visible {
                                let _ = window.hide();
                            } else {
                                let _ = window.show();
                                let _ = window.set_focus();
                            }
                            refresh_tray(app);
                        }
                    }
                    "record" => {
                        let handle = app.clone();
                        tauri::async_runtime::spawn(async move {
                            let pipeline = handle.state::<pipeline::PipelineHandle>();
                            if pipeline.current_state() == pipeline::PipelineState::Idle {
                                if let Err(e) = pipeline.start().await {
                                    tracing::error!("Tray start recording failed: {}", e);
                                }
                            } else if pipeline.current_state() == pipeline::PipelineState::Recording
                            {
                                if let Err(e) = pipeline.stop().await {
                                    tracing::error!("Tray stop recording failed: {}", e);
                                }
                            }
                        });
                    }
                    "settings" => {
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.emit("tray:settings", ());
                            let _ = window.show();
                            let _ = window.set_focus();
                            refresh_tray(app);
                        }
                    }
                    "history" => {
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.emit("tray:history", ());
                            let _ = window.show();
                            let _ = window.set_focus();
                            refresh_tray(app);
                        }
                    }
                    "about" => {
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.emit("tray:about", ());
                            let _ = window.show();
                            let _ = window.set_focus();
                            refresh_tray(app);
                        }
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    let should_show = matches!(
                        event,
                        TrayIconEvent::Click {
                            button: MouseButton::Left,
                            button_state: MouseButtonState::Up,
                            ..
                        } | TrayIconEvent::DoubleClick {
                            button: MouseButton::Left,
                            ..
                        }
                    );
                    if should_show {
                        let app = tray.app_handle();
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                            refresh_tray(app);
                        }
                    }
                })
                .build(app)?;

            app.manage(TrayHandle {
                tray: Mutex::new(tray),
            });

            // Close-to-tray: intercept window close
            if let Some(main_window) = app.get_webview_window("main") {
                let handle = app.handle().clone();
                main_window.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        let close_to_tray = *handle
                            .state::<CloseToTrayCache>()
                            .0
                            .lock()
                            .unwrap_or_else(|e| e.into_inner());
                        if close_to_tray {
                            api.prevent_close();
                            // Save window state before hiding (skip if minimized)
                            if let Some(w) = handle.get_webview_window("main") {
                                if let (Ok(pos), Ok(size)) = (w.outer_position(), w.outer_size()) {
                                    if pos.x > -1000
                                        && pos.y > -1000
                                        && size.width >= 720
                                        && size.height >= 480
                                    {
                                        let ws = WindowState {
                                            x: pos.x,
                                            y: pos.y,
                                            width: size.width,
                                            height: size.height,
                                        };
                                        if let Ok(store) = handle.store("settings.json") {
                                            if let Ok(val) = serde_json::to_value(&ws) {
                                                store.set("window_state", val);
                                                let _ = store.save();
                                            }
                                        }
                                    }
                                }
                                let _ = w.hide();
                            }
                            refresh_tray(&handle);
                        }
                    }
                });
            }

            // Restore window state from previous session
            if let Ok(store) = app.handle().store("settings.json") {
                if let Some(val) = store.get("window_state") {
                    if let Ok(ws) = serde_json::from_value::<WindowState>(val.clone()) {
                        // Validate: skip if coordinates are off-screen (e.g. -32000 from minimized state)
                        if ws.x > -1000 && ws.y > -1000 && ws.width >= 720 && ws.height >= 480 {
                            if let Some(window) = app.get_webview_window("main") {
                                let _ = window.set_position(tauri::Position::Physical(
                                    tauri::PhysicalPosition::new(ws.x, ws.y),
                                ));
                                let _ = window.set_size(tauri::Size::Physical(
                                    tauri::PhysicalSize::new(ws.width, ws.height),
                                ));
                            }
                        }
                    }
                }
            }

            // OpenTypeless lives in the tray, so launches stay hidden by default.
            // We surface the window when the user is still in onboarding —
            // either no STT key yet, or `onboarding_completed` missing / false.
            // Predicate lives in `should_show_window_on_launch` so the decision
            // is unit-tested independently of the Tauri runtime.
            let onboarding_completed = app
                .handle()
                .store("settings.json")
                .ok()
                .and_then(|s| s.get("onboarding_completed"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if should_show_window_on_launch(
                initial_config.stt_api_key.is_empty(),
                onboarding_completed,
            ) {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }

            // Make the capsule visible inside other apps' fullscreen Spaces.
            // Three macOS-specific things have to line up:
            //   1. `CanJoinAllSpaces` (set by `visibleOnAllWorkspaces` in
            //      tauri.conf.json) — lets the window join every Space.
            //   2. `FullScreenAuxiliary` + a window level above
            //      `NSFloatingWindowLevel` (set in
            //      `configure_capsule_collection_behavior`) — lets the window
            //      render above a fullscreen app's content.
            //   3. App activation policy `.accessory` — `.regular` apps are
            //      excluded from other apps' fullscreen Spaces by macOS,
            //      regardless of window flags. Switching to `.accessory`
            //      models OpenTypeless as a status-bar utility (no Dock icon),
            //      which is consistent with the tray-driven UX.
            #[cfg(target_os = "macos")]
            {
                let _ = app
                    .handle()
                    .set_activation_policy(tauri::ActivationPolicy::Accessory);
                if let Some(capsule) = app.get_webview_window("capsule") {
                    if let Ok(ns_window) = capsule.ns_window() {
                        configure_capsule_collection_behavior(ns_window);
                    }
                }
            }

            tracing::info!("OpenTypeless started");

            // P1-2: Pre-warm HTTP connection pool in background
            let warm_handle = app_handle.clone();
            tauri::async_runtime::spawn(async move {
                let pipeline = warm_handle.state::<pipeline::PipelineHandle>();
                pipeline.pre_warm().await;
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            start_recording,
            stop_recording,
            abort_recording,
            check_accessibility_permission,
            request_accessibility_permission,
            check_microphone_permission,
            request_microphone_permission,
            get_config,
            update_config,
            test_stt_connection,
            test_llm_connection,
            bench_stt_connection,
            bench_llm_connection,
            fetch_llm_models,
            get_history,
            clear_history,
            get_dictionary,
            add_dictionary_entry,
            remove_dictionary_entry,
            correction_undo,
            update_hotkey,
            pause_hotkey,
            resume_hotkey,
            set_auto_start,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
