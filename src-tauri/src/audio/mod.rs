pub mod capture;
pub mod permission;

pub use capture::{AudioCaptureHandle, AudioConfig, CaptureState};
pub use permission::{check_microphone_permission, request_microphone_permission, MicAuthStatus};
