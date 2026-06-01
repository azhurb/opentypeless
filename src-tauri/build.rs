fn main() {
    // Microphone permission needs AVCaptureDevice — an ObjC API with a
    // block-based completion handler. Doing that through raw objc_msgSend
    // is painful; a small ObjC shim exposes plain-C entry points the Rust
    // side can call exactly like the AX FFI in pipeline.rs.
    //
    // The `cfg` gate (rather than a runtime `target_os` check) is required:
    // `cc` is declared as a build-dep only for macOS, so the path must not
    // be resolvable on other targets at compile time.
    #[cfg(target_os = "macos")]
    {
        cc::Build::new()
            .file("src/audio/mic_permission.m")
            .flag("-fobjc-arc")
            .compile("otl_mic_permission");
        println!("cargo:rustc-link-lib=framework=AVFoundation");
        println!("cargo:rerun-if-changed=src/audio/mic_permission.m");

        // Delayed-clipboard provider: lazy NSPasteboard rendering so the output
        // path can detect whether a paste actually landed (see clipboard.rs).
        cc::Build::new()
            .file("src/output/pasteboard_provider.m")
            .flag("-fobjc-arc")
            .compile("otl_pasteboard_provider");
        println!("cargo:rustc-link-lib=framework=AppKit");
        println!("cargo:rerun-if-changed=src/output/pasteboard_provider.m");
    }
    tauri_build::build()
}
