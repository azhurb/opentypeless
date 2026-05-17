fn main() {
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os == "macos" {
        // Microphone permission needs AVCaptureDevice — an ObjC API with a
        // block-based completion handler. Doing that through raw objc_msgSend
        // is painful; a small ObjC shim exposes plain-C entry points the Rust
        // side can call exactly like the AX FFI in pipeline.rs.
        cc::Build::new()
            .file("src/audio/mic_permission.m")
            .flag("-fobjc-arc")
            .compile("otl_mic_permission");
        println!("cargo:rustc-link-lib=framework=AVFoundation");
        println!("cargo:rerun-if-changed=src/audio/mic_permission.m");
    }
    tauri_build::build()
}
