//! Run Wry's upstream script/CSP tests against its patched SHA-2 dependency.
//! The Android-only source is included here so Windows CI checks it as well.
#[allow(dead_code)]
pub struct InitializationScript {
    pub script: String,
    pub for_main_frame_only: bool,
}

#[path = "../../vendor/wry/src/inject_initialization_scripts.rs"]
mod inject_initialization_scripts;
