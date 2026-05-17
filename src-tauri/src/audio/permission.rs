//! macOS Microphone permission probe + request.
//!
//! On macOS the input device opens via cpal which auto-triggers the system
//! Microphone dialog on first record. That's the wrong UX moment — the dialog
//! is one-shot per install, so a dismissed or denied dialog leaves the user
//! stuck. We pre-prompt during onboarding so the user sees the dialog while
//! they're paying attention.
//!
//! The native side lives in `mic_permission.m` (compiled by build.rs); this
//! file is the Rust-facing FFI binding, modelled on the AX FFI in pipeline.rs.

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MicAuthStatus {
    NotDetermined,
    Restricted,
    Denied,
    Authorized,
}

#[cfg(target_os = "macos")]
extern "C" {
    fn otl_mic_authorization_status() -> i32;
    fn otl_mic_request_access(
        cb: extern "C" fn(i32, *mut std::ffi::c_void),
        ctx: *mut std::ffi::c_void,
    );
}

/// Synchronously read the current AVCaptureDevice authorization status for
/// audio. Returns `Authorized` on non-macOS platforms — there's no equivalent
/// per-app gate to surface to the user.
pub fn check_microphone_permission() -> MicAuthStatus {
    #[cfg(target_os = "macos")]
    {
        match unsafe { otl_mic_authorization_status() } {
            0 => MicAuthStatus::NotDetermined,
            1 => MicAuthStatus::Restricted,
            2 => MicAuthStatus::Denied,
            3 => MicAuthStatus::Authorized,
            _ => MicAuthStatus::NotDetermined,
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        MicAuthStatus::Authorized
    }
}

/// Trigger the macOS Microphone prompt and await the user's response. The
/// dialog is one-shot per install: if the status is already `Restricted` or
/// `Denied`, this returns immediately without prompting (caller should route
/// the user to System Settings instead).
///
/// Returns `true` if the user granted, `false` otherwise. On non-macOS this is
/// a no-op returning `true`.
pub async fn request_microphone_permission() -> bool {
    #[cfg(target_os = "macos")]
    {
        let (tx, rx) = tokio::sync::oneshot::channel::<bool>();
        // The completion block fires on an arbitrary dispatch queue, so the
        // sender must outlive this scope; transfer ownership to the C side
        // via Box::into_raw and reclaim it in the callback.
        let ctx = Box::into_raw(Box::new(tx)) as *mut std::ffi::c_void;

        extern "C" fn on_complete(granted: i32, ctx: *mut std::ffi::c_void) {
            let tx: Box<tokio::sync::oneshot::Sender<bool>> =
                unsafe { Box::from_raw(ctx as *mut _) };
            let _ = tx.send(granted != 0);
        }

        unsafe { otl_mic_request_access(on_complete, ctx) };
        rx.await.unwrap_or(false)
    }
    #[cfg(not(target_os = "macos"))]
    {
        true
    }
}
