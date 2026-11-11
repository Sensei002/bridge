//! Lifetime-safe initialization for the Windows Media Foundation runtime.
//!
//! This is the first stage of the H.264 encoder path. It deliberately does not
//! claim that an H.264 encoder is present; encoder MFT discovery and activation
//! are performed by the next stage.

use windows::Win32::Media::MediaFoundation::{
    MFShutdown, MFStartup, MFSTARTUP_FULL, MF_VERSION,
};

/// Owns one successful MFStartup call and balances it with MFShutdown.
pub struct MediaFoundationRuntime {
    _private: (),
}

impl MediaFoundationRuntime {
    /// Initialize Media Foundation before starting any encoder/MFT work.
    pub fn start() -> Result<Self, String> {
        unsafe { MFStartup(MF_VERSION, MFSTARTUP_FULL) }
            .map_err(|error| format!("Media Foundation initialization failed: {error}"))?;
        Ok(Self { _private: () })
    }
}

impl Drop for MediaFoundationRuntime {
    fn drop(&mut self) {
        // MFShutdown is best-effort during teardown. Startup succeeded exactly
        // once for this guard, so it must be paired exactly once.
        let _ = unsafe { MFShutdown() };
    }
}
