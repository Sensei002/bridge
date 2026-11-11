//! Lifetime-safe initialization for the Windows Media Foundation runtime.
//!
//! This is the first stage of the H.264 encoder path. It deliberately does not
//! claim that an H.264 encoder is present; encoder MFT discovery and activation
//! are performed by the next stage.

use std::ptr::{null, null_mut, read};

use windows::Win32::Media::MediaFoundation::{
    IMFActivate, IMFTransform, MFTEnumEx, MFT_CATEGORY_VIDEO_ENCODER, MFT_ENUM_FLAG,
    MFT_ENUM_FLAG_ASYNCMFT, MFT_ENUM_FLAG_HARDWARE, MFT_ENUM_FLAG_LOCALMFT,
    MFT_ENUM_FLAG_SORTANDFILTER, MFT_ENUM_FLAG_SYNCMFT, MFT_FRIENDLY_NAME_Attribute,
    MFT_REGISTER_TYPE_INFO, MFMediaType_Video, MFVideoFormat_H264,
    MFShutdown, MFStartup, MFSTARTUP_FULL, MF_VERSION,
};
use windows::Win32::System::Com::CoTaskMemFree;

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

/// An encoder MFT registered with Media Foundation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct H264EncoderCandidate {
    pub friendly_name: String,
    pub activation_succeeded: bool,
    pub input_streams: Option<u32>,
    pub output_streams: Option<u32>,
    pub activation_error: Option<String>,
}

/// Enumerate registered video encoder MFTs that advertise H.264 output.
///
/// Hardware and software candidates are both returned. The hardware flag is
/// preserved so the caller can prefer GPU-backed transforms without assuming
/// that a hardware MFT is always available.
pub fn discover_h264_encoders() -> Result<Vec<H264EncoderCandidate>, String> {
    let flags: MFT_ENUM_FLAG = MFT_ENUM_FLAG_HARDWARE
        | MFT_ENUM_FLAG_SYNCMFT
        | MFT_ENUM_FLAG_ASYNCMFT
        | MFT_ENUM_FLAG_LOCALMFT
        | MFT_ENUM_FLAG_SORTANDFILTER;
    let output_type = MFT_REGISTER_TYPE_INFO {
        guidMajorType: MFMediaType_Video,
        guidSubtype: MFVideoFormat_H264,
    };
    let mut activations: *mut Option<IMFActivate> = null_mut();
    let mut count = 0_u32;
    unsafe {
        MFTEnumEx(
            MFT_CATEGORY_VIDEO_ENCODER,
            flags,
            None,
            Some(&output_type),
            &mut activations,
            &mut count,
        )
    }.map_err(|error| format!("Media Foundation H.264 encoder enumeration failed: {error}"))?;

    let mut candidates = Vec::with_capacity(count as usize);
    if !activations.is_null() {
        for index in 0..count as usize {
            let activation = unsafe { read(activations.add(index)) };
            if let Some(activation) = activation {
                let friendly_name = unsafe {
                    let length = activation.GetStringLength(&MFT_FRIENDLY_NAME_Attribute)
                        .map_err(|error| format!("could not read encoder name: {error}"))?;
                    let mut buffer = vec![0_u16; length as usize + 1];
                    activation.GetString(&MFT_FRIENDLY_NAME_Attribute, &mut buffer, None)
                        .map_err(|error| format!("could not read encoder name: {error}"))?;
                    String::from_utf16_lossy(&buffer[..length as usize])
                };
                let activation_result = unsafe { activation.ActivateObject::<IMFTransform>() };
                let (activation_succeeded, input_streams, output_streams, activation_error) =
                    match activation_result {
                        Ok(transform) => {
                            let mut inputs = 0_u32;
                            let mut outputs = 0_u32;
                            let stream_result = unsafe {
                                transform.GetStreamCount(&mut inputs, &mut outputs)
                            };
                            drop(transform);
                            let _ = unsafe { activation.ShutdownObject() };
                            match stream_result {
                                Ok(()) => (true, Some(inputs), Some(outputs), None),
                                Err(error) => (true, None, None, Some(format!(
                                    "activated but stream enumeration failed: {error}"
                                ))),
                            }
                        }
                        Err(error) => (
                            false,
                            None,
                            None,
                            Some(format!("encoder activation failed: {error}")),
                        ),
                    };
                candidates.push(H264EncoderCandidate {
                    friendly_name,
                    activation_succeeded,
                    input_streams,
                    output_streams,
                    activation_error,
                });
                drop(activation);
            }
        }
        unsafe { CoTaskMemFree(Some(activations.cast())) };
    }
    candidates.sort_by(|left, right| left.friendly_name.cmp(&right.friendly_name));
    candidates.dedup_by(|left, right| left.friendly_name == right.friendly_name);
    Ok(candidates)
}
