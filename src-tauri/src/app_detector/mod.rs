use serde::{Deserialize, Serialize};

use crate::llm::AppType;

pub mod cli_detect;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppContext {
    pub app_name: String,
    pub window_title: String,
    pub app_type: AppType,
    /// macOS bundle identifier (e.g. `com.googlecode.iterm2`). `None` on
    /// platforms that don't have bundle IDs (Windows/Linux) or when the
    /// foreground process can't be resolved. Used by the paste chunker to
    /// recognize terminal emulators and IDE terminal panels.
    #[serde(default)]
    pub bundle_id: Option<String>,
    /// Process id of the foreground application. `None` when it can't be
    /// resolved. Used by the paste chunker to find a coding CLI running inside
    /// the focused terminal/IDE (see [`cli_detect`]).
    #[serde(default)]
    pub pid: Option<i32>,
}

impl Default for AppContext {
    fn default() -> Self {
        Self {
            app_name: String::new(),
            window_title: String::new(),
            app_type: AppType::General,
            bundle_id: None,
            pid: None,
        }
    }
}

/// The pid of the frontmost application, or `None` when there isn't one.
///
/// Split out from [`detect_current_app`] because the Accessibility reads in
/// `correction::ax_macos` need only this, and one of them runs on every poll of
/// the correction watcher — building the app name and bundle ID alongside it
/// would be waste on that path.
#[cfg(target_os = "macos")]
pub fn frontmost_pid() -> Option<i32> {
    let pid = macos_ffi::frontmost_app_info().2;
    (pid > 0).then_some(pid)
}

pub fn detect_current_app() -> AppContext {
    #[cfg(target_os = "windows")]
    {
        windows_detect()
    }
    #[cfg(target_os = "macos")]
    {
        macos_detect()
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        AppContext::default()
    }
}

#[cfg(target_os = "macos")]
fn macos_detect() -> AppContext {
    // Spawning `osascript "tell application System Events ..."` three times in
    // a row used to dominate the hot path on the hotkey-press side: each
    // shell-out costs ~50–150 ms cold, and the previous implementation issued
    // three sequential calls before audio capture could even start. Replace
    // with NSWorkspace (for app name / bundle id / pid) plus an AX lookup for
    // the focused window title. Both are in-process; total cost is <5 ms.
    let (app_name, bundle_id, pid) = macos_ffi::frontmost_app_info();
    let window_title = if pid > 0 {
        macos_ffi::focused_window_title(pid).unwrap_or_default()
    } else {
        String::new()
    };
    let app_type = classify_app(&app_name);
    AppContext {
        app_name,
        window_title,
        app_type,
        bundle_id,
        pid: if pid > 0 { Some(pid) } else { None },
    }
}

#[cfg(target_os = "macos")]
mod macos_ffi {
    use std::ffi::{c_char, c_void};
    use std::ptr;

    #[link(name = "objc", kind = "dylib")]
    extern "C" {
        fn sel_registerName(name: *const c_char) -> *mut c_void;
        fn objc_getClass(name: *const c_char) -> *mut c_void;
    }
    extern "C" {
        fn objc_msgSend();
    }

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXUIElementCreateApplication(pid: i32) -> *mut c_void;
        fn AXUIElementCopyAttributeValue(
            element: *mut c_void,
            attribute: *mut c_void,
            out_value: *mut *mut c_void,
        ) -> i32;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFRelease(cf: *mut c_void);
        fn CFGetTypeID(cf: *mut c_void) -> usize;
        fn CFStringGetTypeID() -> usize;
        fn CFStringGetLength(s: *mut c_void) -> isize;
        fn CFStringGetCString(
            s: *mut c_void,
            buffer: *mut u8,
            buffer_size: isize,
            encoding: u32,
        ) -> u8;
        fn CFStringCreateWithCString(
            alloc: *mut c_void,
            cstr: *const u8,
            encoding: u32,
        ) -> *mut c_void;
    }

    const K_CF_STRING_ENCODING_UTF8: u32 = 0x08000100;

    // The AX attribute extern statics (kAXFocusedWindowAttribute,
    // kAXTitleAttribute) live in HIServices and aren't always reachable
    // through the ApplicationServices umbrella in release linker configs.
    // Build the CFStrings at runtime from the well-known names instead —
    // identical value, no linkage dependency. Same trick as
    // `correction/ax_macos.rs::cfstr`.
    unsafe fn cfstr(name: &[u8]) -> *mut c_void {
        CFStringCreateWithCString(ptr::null_mut(), name.as_ptr(), K_CF_STRING_ENCODING_UTF8)
    }

    unsafe fn cf_string_to_rust(cf: *mut c_void) -> Option<String> {
        if cf.is_null() {
            return None;
        }
        if CFGetTypeID(cf) != CFStringGetTypeID() {
            return None;
        }
        let len_chars = CFStringGetLength(cf);
        let buf_size = (len_chars * 4 + 1).max(16);
        let mut buf: Vec<u8> = vec![0u8; buf_size as usize];
        if CFStringGetCString(cf, buf.as_mut_ptr(), buf_size, K_CF_STRING_ENCODING_UTF8) == 0 {
            return None;
        }
        let nul = buf.iter().position(|b| *b == 0).unwrap_or(buf.len());
        buf.truncate(nul);
        String::from_utf8(buf).ok()
    }

    /// Returns `(localized_name, bundle_id, pid)` of the frontmost app via
    /// `NSWorkspace.sharedWorkspace.frontmostApplication`. Empty strings /
    /// `None` / `0` on any failure (no frontmost app, daemon with no bundle,
    /// etc.) — same graceful-degradation contract as the previous osascript
    /// implementation.
    pub fn frontmost_app_info() -> (String, Option<String>, i32) {
        unsafe {
            let msg_id: unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void =
                std::mem::transmute(objc_msgSend as *const c_void);
            let msg_i32: unsafe extern "C" fn(*mut c_void, *mut c_void) -> i32 =
                std::mem::transmute(objc_msgSend as *const c_void);

            let workspace_class = objc_getClass(c"NSWorkspace".as_ptr());
            if workspace_class.is_null() {
                return (String::new(), None, 0);
            }
            let shared_sel = sel_registerName(c"sharedWorkspace".as_ptr());
            let workspace = msg_id(workspace_class, shared_sel);
            if workspace.is_null() {
                return (String::new(), None, 0);
            }

            let frontmost_sel = sel_registerName(c"frontmostApplication".as_ptr());
            let app = msg_id(workspace, frontmost_sel);
            if app.is_null() {
                return (String::new(), None, 0);
            }

            let name_sel = sel_registerName(c"localizedName".as_ptr());
            let bundle_sel = sel_registerName(c"bundleIdentifier".as_ptr());
            let pid_sel = sel_registerName(c"processIdentifier".as_ptr());

            let name_ns = msg_id(app, name_sel);
            let bundle_ns = msg_id(app, bundle_sel);
            let pid = msg_i32(app, pid_sel);

            // NSString is toll-free bridged with CFString, so we can read the
            // characters via CFStringGetCString without bridging through ObjC.
            // Don't CFRelease these — they are autoreleased return values
            // owned by the autorelease pool, not retained for us.
            let app_name = if name_ns.is_null() {
                String::new()
            } else {
                cf_string_to_rust(name_ns).unwrap_or_default()
            };
            let bundle_id = if bundle_ns.is_null() {
                None
            } else {
                cf_string_to_rust(bundle_ns).filter(|s| !s.is_empty())
            };

            (app_name, bundle_id, pid)
        }
    }

    /// Returns the title of the focused window of the running app with the
    /// given pid, via AX. Returns `None` if Accessibility is not granted, if
    /// the app has no focused window, or if any AX call fails — the chunker
    /// treats an empty title the same as today.
    pub fn focused_window_title(pid: i32) -> Option<String> {
        unsafe {
            if !crate::pipeline::is_accessibility_trusted() {
                return None;
            }
            let app_ax = AXUIElementCreateApplication(pid);
            if app_ax.is_null() {
                return None;
            }
            let focused_attr = cfstr(b"AXFocusedWindow\0");
            let title_attr = cfstr(b"AXTitle\0");
            if focused_attr.is_null() || title_attr.is_null() {
                if !focused_attr.is_null() {
                    CFRelease(focused_attr);
                }
                if !title_attr.is_null() {
                    CFRelease(title_attr);
                }
                CFRelease(app_ax);
                return None;
            }

            let mut window: *mut c_void = ptr::null_mut();
            let err1 = AXUIElementCopyAttributeValue(app_ax, focused_attr, &mut window);
            if err1 != 0 || window.is_null() {
                CFRelease(focused_attr);
                CFRelease(title_attr);
                CFRelease(app_ax);
                return None;
            }

            let mut title_ref: *mut c_void = ptr::null_mut();
            let err2 = AXUIElementCopyAttributeValue(window, title_attr, &mut title_ref);
            let title = if err2 == 0 && !title_ref.is_null() {
                cf_string_to_rust(title_ref)
            } else {
                None
            };
            if !title_ref.is_null() {
                CFRelease(title_ref);
            }
            CFRelease(window);
            CFRelease(focused_attr);
            CFRelease(title_attr);
            CFRelease(app_ax);
            title
        }
    }
}

#[cfg(target_os = "windows")]
fn windows_detect() -> AppContext {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;

    unsafe {
        let hwnd = windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow();
        if hwnd.is_null() {
            return AppContext::default();
        }

        // Get window title
        let mut title_buf = [0u16; 512];
        let len = windows_sys::Win32::UI::WindowsAndMessaging::GetWindowTextW(
            hwnd,
            title_buf.as_mut_ptr(),
            title_buf.len() as i32,
        );
        let window_title = if len > 0 {
            OsString::from_wide(&title_buf[..len as usize])
                .to_string_lossy()
                .to_string()
        } else {
            String::new()
        };

        // Get process name
        let mut pid = 0u32;
        windows_sys::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId(hwnd, &mut pid);

        let app_name = get_process_name(pid).unwrap_or_default();
        let app_type = classify_app(&app_name);

        AppContext {
            app_name,
            window_title,
            app_type,
            bundle_id: None,
            pid: if pid > 0 { Some(pid as i32) } else { None },
        }
    }
}

#[cfg(target_os = "windows")]
unsafe fn get_process_name(pid: u32) -> Option<String> {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;

    if pid == 0 {
        return None;
    }

    let handle = windows_sys::Win32::System::Threading::OpenProcess(
        windows_sys::Win32::System::Threading::PROCESS_QUERY_LIMITED_INFORMATION,
        0,
        pid,
    );
    if handle.is_null() {
        return None;
    }

    let mut buf = [0u16; 260];
    let mut size = buf.len() as u32;
    let ok = windows_sys::Win32::System::Threading::QueryFullProcessImageNameW(
        handle,
        0,
        buf.as_mut_ptr(),
        &mut size,
    );
    let _ = windows_sys::Win32::Foundation::CloseHandle(handle);

    if ok != 0 && size > 0 {
        let path = OsString::from_wide(&buf[..size as usize])
            .to_string_lossy()
            .to_string();
        path.rsplit('\\').next().map(|s| s.to_string())
    } else {
        None
    }
}

#[allow(dead_code)]
fn classify_app(app_name: &str) -> AppType {
    let name = app_name.to_lowercase();
    if ["outlook", "gmail", "thunderbird", "mail"]
        .iter()
        .any(|k| name.contains(k))
    {
        AppType::Email
    } else if ["slack", "discord", "wechat", "telegram", "teams"]
        .iter()
        .any(|k| name.contains(k))
    {
        AppType::Chat
    } else if ["code", "intellij", "vim", "nvim", "cursor"]
        .iter()
        .any(|k| name.contains(k))
    {
        AppType::Code
    } else if ["word", "docs", "notion", "obsidian", "typora"]
        .iter()
        .any(|k| name.contains(k))
    {
        AppType::Document
    } else {
        AppType::General
    }
}
