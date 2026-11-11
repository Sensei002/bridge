//! Lifetime-safe initialization for the Windows Media Foundation runtime.
//!
//! This is the first stage of the H.264 encoder path. It deliberately does not
//! claim that an H.264 encoder is present; encoder MFT discovery and activation
//! are performed by the next stage.

use std::ptr::{null_mut, read};

use windows::Win32::Media::MediaFoundation::{
    IMFActivate, IMFTransform, MFTEnumEx, MFT_CATEGORY_VIDEO_ENCODER, MFT_ENUM_FLAG,
    MFT_ENUM_FLAG_ASYNCMFT, MFT_ENUM_FLAG_HARDWARE, MFT_ENUM_FLAG_LOCALMFT,
    MFT_ENUM_FLAG_SORTANDFILTER, MFT_ENUM_FLAG_SYNCMFT, MFT_FRIENDLY_NAME_Attribute,
    MFT_REGISTER_TYPE_INFO, MFT_SET_TYPE_TEST_ONLY, MFCreateMediaType,
    MFMediaType_Video, MFVideoFormat_H264, MFVideoFormat_NV12,
    MF_MT_AVG_BITRATE, MF_MT_FRAME_RATE, MF_MT_FRAME_SIZE, MF_MT_MAJOR_TYPE,
    MF_MT_SUBTYPE, MFShutdown, MFStartup, MFSTARTUP_FULL, MF_VERSION,
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
    pub supports_nv12_h264_1080x2340_60: bool,
    pub format_test_error: Option<String>,
    pub profile_configured: bool,
    pub configuration_error: Option<String>,
}


unsafe fn test_nv12_h264_profile(transform: &IMFTransform) -> Result<(), String> {
    let input = unsafe { MFCreateMediaType() }
        .map_err(|error| format!("could not create NV12 media type: {error}"))?;
    unsafe {
        input.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)
            .map_err(|error| format!("could not set NV12 major type: {error}"))?;
        input.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_NV12)
            .map_err(|error| format!("could not set NV12 subtype: {error}"))?;
        input.SetUINT64(&MF_MT_FRAME_SIZE, (1080_u64 << 32) | 2340)
            .map_err(|error| format!("could not set NV12 frame size: {error}"))?;
        input.SetUINT64(&MF_MT_FRAME_RATE, (60_u64 << 32) | 1)
            .map_err(|error| format!("could not set NV12 frame rate: {error}"))?;
    }
    let output = unsafe { MFCreateMediaType() }
        .map_err(|error| format!("could not create H.264 media type: {error}"))?;
    unsafe {
        output.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)
            .map_err(|error| format!("could not set H.264 major type: {error}"))?;
        output.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_H264)
            .map_err(|error| format!("could not set H.264 subtype: {error}"))?;
        output.SetUINT64(&MF_MT_FRAME_SIZE, (1080_u64 << 32) | 2340)
            .map_err(|error| format!("could not set H.264 frame size: {error}"))?;
        output.SetUINT64(&MF_MT_FRAME_RATE, (60_u64 << 32) | 1)
            .map_err(|error| format!("could not set H.264 frame rate: {error}"))?;
        output.SetUINT32(&MF_MT_AVG_BITRATE, 8_000_000)
            .map_err(|error| format!("could not set H.264 target bitrate: {error}"))?;
        transform.SetOutputType(0, &output, MFT_SET_TYPE_TEST_ONLY.0 as u32)
            .map_err(|error| format!("encoder rejected H.264 output profile: {error}"))?;
        transform.SetInputType(0, &input, MFT_SET_TYPE_TEST_ONLY.0 as u32)
            .map_err(|error| format!("encoder rejected NV12 input profile: {error}"))?;
    }
    Ok(())
}


/// Commit the target profile after the non-mutating compatibility probe.
/// The transform is shut down by the caller after diagnostics are collected;
/// this milestone deliberately does not submit input samples.
unsafe fn configure_nv12_h264_profile(transform: &IMFTransform) -> Result<(), String> {
    let input = unsafe { MFCreateMediaType() }
        .map_err(|error| format!("could not create NV12 media type: {error}"))?;
    unsafe {
        input.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)
            .map_err(|error| format!("could not set NV12 major type: {error}"))?;
        input.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_NV12)
            .map_err(|error| format!("could not set NV12 subtype: {error}"))?;
        input.SetUINT64(&MF_MT_FRAME_SIZE, (1080_u64 << 32) | 2340)
            .map_err(|error| format!("could not set NV12 frame size: {error}"))?;
        input.SetUINT64(&MF_MT_FRAME_RATE, (60_u64 << 32) | 1)
            .map_err(|error| format!("could not set NV12 frame rate: {error}"))?;
    }

    let output = unsafe { MFCreateMediaType() }
        .map_err(|error| format!("could not create H.264 media type: {error}"))?;
    unsafe {
        output.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)
            .map_err(|error| format!("could not set H.264 major type: {error}"))?;
        output.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_H264)
            .map_err(|error| format!("could not set H.264 subtype: {error}"))?;
        output.SetUINT64(&MF_MT_FRAME_SIZE, (1080_u64 << 32) | 2340)
            .map_err(|error| format!("could not set H.264 frame size: {error}"))?;
        output.SetUINT64(&MF_MT_FRAME_RATE, (60_u64 << 32) | 1)
            .map_err(|error| format!("could not set H.264 frame rate: {error}"))?;
        output.SetUINT32(&MF_MT_AVG_BITRATE, 8_000_000)
            .map_err(|error| format!("could not set H.264 target bitrate: {error}"))?;

        // Most encoder MFTs require output type negotiation before input type.
        transform.SetOutputType(0, &output, 0)
            .map_err(|error| format!("encoder rejected committed H.264 output type: {error}"))?;
        transform.SetInputType(0, &input, 0)
            .map_err(|error| format!("encoder rejected committed NV12 input type: {error}"))?;

        // Read back the committed types rather than treating Set*Type success
        // alone as proof that the transform retained the requested profile.
        let current_input = transform.GetInputCurrentType(0)
            .map_err(|error| format!("could not read committed NV12 input type: {error}"))?;
        let current_output = transform.GetOutputCurrentType(0)
            .map_err(|error| format!("could not read committed H.264 output type: {error}"))?;
        let input_size = current_input.GetUINT64(&MF_MT_FRAME_SIZE)
            .map_err(|error| format!("could not read committed NV12 dimensions: {error}"))?;
        let output_size = current_output.GetUINT64(&MF_MT_FRAME_SIZE)
            .map_err(|error| format!("could not read committed H.264 dimensions: {error}"))?;
        if input_size != ((1080_u64 << 32) | 2340)
            || output_size != ((1080_u64 << 32) | 2340)
        {
            return Err("encoder did not retain the requested 1080x2340 frame dimensions".into());
        }
    }
    Ok(())
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
                let (
                    activation_succeeded, input_streams, output_streams, activation_error,
                    supports_nv12_h264_1080x2340_60, format_test_error,
                    profile_configured, configuration_error,
                ) = match activation_result {
                    Ok(transform) => {
                        let mut inputs = 0_u32;
                        let mut outputs = 0_u32;
                        let stream_result = unsafe { transform.GetStreamCount(&mut inputs, &mut outputs) };
                        let profile_result = unsafe { test_nv12_h264_profile(&transform) };
                        let configuration_result = if profile_result.is_ok() {
                            unsafe { configure_nv12_h264_profile(&transform) }
                        } else {
                            Err("configuration skipped because the compatibility probe failed".into())
                        };
                        drop(transform);
                        let _ = unsafe { activation.ShutdownObject() };
                        let (input_streams, output_streams, activation_error) = match stream_result {
                            Ok(()) => (Some(inputs), Some(outputs), None),
                            Err(error) => (None, None, Some(format!(
                                "activated but stream enumeration failed: {error}"
                            ))),
                        };
                        let (supports_profile, format_error) = match profile_result {
                            Ok(()) => (true, None),
                            Err(error) => (false, Some(error)),
                        };
                        let (configured, config_error) = match configuration_result {
                            Ok(()) => (true, None),
                            Err(error) => (false, Some(error)),
                        };
                        (
                            true, input_streams, output_streams, activation_error,
                            supports_profile, format_error, configured, config_error,
                        )
                    }
                    Err(error) => (
                        false, None, None, Some(format!("encoder activation failed: {error}")),
                        false, None, false, None,
                    ),
                };
                candidates.push(H264EncoderCandidate {
                    friendly_name, activation_succeeded, input_streams, output_streams,
                    activation_error, supports_nv12_h264_1080x2340_60, format_test_error,
                    profile_configured, configuration_error,
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
