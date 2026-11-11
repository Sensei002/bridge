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
    MFT_REGISTER_TYPE_INFO, MFT_SET_TYPE_TEST_ONLY, MFT_OUTPUT_DATA_BUFFER,
    MFT_OUTPUT_STREAM_CAN_PROVIDE_SAMPLES, MFT_OUTPUT_STREAM_PROVIDES_SAMPLES,
    MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, MFT_MESSAGE_NOTIFY_END_OF_STREAM,
    MFT_MESSAGE_NOTIFY_END_STREAMING, MFT_MESSAGE_NOTIFY_START_OF_STREAM,
    MFCreateMediaType, MFCreateMemoryBuffer, MFCreateSample, MF_E_TRANSFORM_NEED_MORE_INPUT,
    MFMediaType_Video, MFVideoFormat_H264, MFVideoFormat_NV12,
    MF_MT_AVG_BITRATE, MF_MT_FRAME_RATE, MF_MT_FRAME_SIZE, MF_MT_MAJOR_TYPE,
    MF_MT_SUBTYPE, MFShutdown, MFStartup, MFSTARTUP_FULL, MF_VERSION,
};
use windows::Win32::System::Com::{CoInitializeEx, CoTaskMemFree, CoUninitialize, COINIT_MULTITHREADED};

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

/// Owns COM initialization for the current worker thread.
pub struct ComApartment;

impl ComApartment {
    pub fn start() -> Result<Self, String> {
        unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }
            .map_err(|error| format!("COM MTA initialization failed: {error}"))?;
        Ok(Self)
    }
}

impl Drop for ComApartment {
    fn drop(&mut self) {
        unsafe { CoUninitialize() };
    }
}

/// Outcome of submitting one NV12 sample to a synchronous H.264 encoder.
pub enum FirstSampleResult {
    Encoded { encoder_name: String, bytes: usize },
    Buffered { encoder_name: String },
}

/// Submit one bounded NV12 frame to the first compatible synchronous encoder.
///
/// This is deliberately a one-sample diagnostic: the transform is drained and
/// shut down afterward, and the returned bytes are never sent to the peer.
pub fn submit_first_nv12_sample(frame: &bridge_transfer::Nv12Frame) -> Result<FirstSampleResult, String> {
    if frame.width != 1080 || frame.height != 2340 {
        return Err(format!(
            "first-sample probe expects 1080x2340 NV12; received {}x{}",
            frame.width, frame.height
        ));
    }
    let expected = (frame.width as usize)
        .checked_mul(frame.height as usize)
        .and_then(|y| y.checked_add(y / 2))
        .ok_or_else(|| "NV12 input size overflow".to_string())?;
    if frame.pixels.len() != expected || expected > 64 * 1024 * 1024 {
        return Err("NV12 input buffer has an invalid or oversized layout".into());
    }

    let flags: MFT_ENUM_FLAG = MFT_ENUM_FLAG_HARDWARE
        | MFT_ENUM_FLAG_SYNCMFT
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
    }.map_err(|error| format!("synchronous H.264 encoder enumeration failed: {error}"))?;

    let mut last_error = None;
    let mut result = None;
    if !activations.is_null() {
        for index in 0..count as usize {
            let activation = unsafe { read(activations.add(index)) };
            let Some(activation) = activation else { continue };
            let attempt = (|| -> Result<FirstSampleResult, String> {
                let name = unsafe {
                    let length = activation.GetStringLength(&MFT_FRIENDLY_NAME_Attribute)
                        .map_err(|error| format!("could not read encoder name: {error}"))?;
                    let mut buffer = vec![0_u16; length as usize + 1];
                    activation.GetString(&MFT_FRIENDLY_NAME_Attribute, &mut buffer, None)
                        .map_err(|error| format!("could not read encoder name: {error}"))?;
                    String::from_utf16_lossy(&buffer[..length as usize])
                };
                let transform = unsafe { activation.ActivateObject::<IMFTransform>() }
                    .map_err(|error| format!("{name}: activation failed: {error}"))?;
                let configured = unsafe { configure_nv12_h264_profile(&transform) };
                if let Err(error) = configured {
                    drop(transform);
                    let _ = unsafe { activation.ShutdownObject() };
                    return Err(format!("{name}: {error}"));
                }

                let outcome = (|| -> Result<FirstSampleResult, String> {
                    unsafe {
                        transform.ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0)
                            .map_err(|error| format!("begin-streaming message failed: {error}"))?;
                        transform.ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0)
                            .map_err(|error| format!("start-of-stream message failed: {error}"))?;
                    }

                    let input_len = u32::try_from(frame.pixels.len())
                        .map_err(|_| "NV12 input exceeds Media Foundation buffer limits".to_string())?;
                    let input_buffer = unsafe { MFCreateMemoryBuffer(input_len) }
                        .map_err(|error| format!("could not allocate NV12 input buffer: {error}"))?;
                    let mut input_data = null_mut();
                    unsafe { input_buffer.Lock(&mut input_data, None, None) }
                        .map_err(|error| format!("could not lock NV12 input buffer: {error}"))?;
                    unsafe {
                        std::ptr::copy_nonoverlapping(frame.pixels.as_ptr(), input_data, frame.pixels.len());
                    }
                    let unlock_result = unsafe { input_buffer.Unlock() };
                    unlock_result.map_err(|error| format!("could not unlock NV12 input buffer: {error}"))?;
                    unsafe { input_buffer.SetCurrentLength(input_len) }
                        .map_err(|error| format!("could not set NV12 input length: {error}"))?;

                    let input_sample = unsafe { MFCreateSample() }
                        .map_err(|error| format!("could not create NV12 input sample: {error}"))?;
                    unsafe {
                        input_sample.AddBuffer(&input_buffer)
                            .map_err(|error| format!("could not attach NV12 input buffer: {error}"))?;
                        input_sample.SetSampleTime((frame.timestamp_ns / 100) as i64)
                            .map_err(|error| format!("could not set NV12 sample timestamp: {error}"))?;
                        input_sample.SetSampleDuration(166_667)
                            .map_err(|error| format!("could not set NV12 sample duration: {error}"))?;
                        transform.ProcessInput(0, &input_sample, 0)
                            .map_err(|error| format!("encoder rejected NV12 input sample: {error}"))?;
                    }

                    let stream_info = unsafe { transform.GetOutputStreamInfo(0) }
                        .map_err(|error| format!("could not query H.264 output stream: {error}"))?;
                    let provides_sample = stream_info.dwFlags & MFT_OUTPUT_STREAM_PROVIDES_SAMPLES != 0;
                    let can_provide_sample = stream_info.dwFlags & MFT_OUTPUT_STREAM_CAN_PROVIDE_SAMPLES != 0;
                    let output_sample = if provides_sample || can_provide_sample {
                        None
                    } else {
                        if stream_info.cbSize == 0 || stream_info.cbSize as usize > 16 * 1024 * 1024 {
                            return Err("encoder requested an invalid H.264 output buffer size".into());
                        }
                        let buffer = unsafe { MFCreateMemoryBuffer(stream_info.cbSize) }
                            .map_err(|error| format!("could not allocate H.264 output buffer: {error}"))?;
                        let sample = unsafe { MFCreateSample() }
                            .map_err(|error| format!("could not create H.264 output sample: {error}"))?;
                        unsafe { sample.AddBuffer(&buffer) }
                            .map_err(|error| format!("could not attach H.264 output buffer: {error}"))?;
                        Some(sample)
                    };
                    let mut output = MFT_OUTPUT_DATA_BUFFER {
                        dwStreamID: 0,
                        pSample: output_sample,
                        dwStatus: 0,
                        pEvents: None,
                    };
                    let mut output_status = 0_u32;
                    match unsafe { transform.ProcessOutput(0, std::slice::from_mut(&mut output), &mut output_status) } {
                        Ok(()) => {}
                        Err(error) if error.code() == windows::Win32::Media::MediaFoundation::MF_E_TRANSFORM_NEED_MORE_INPUT => {
                            return Ok(FirstSampleResult::Buffered { encoder_name: name.clone() });
                        }
                        Err(error) => return Err(format!("encoder ProcessOutput failed: {error}")),
                    }
                    let Some(sample) = output.pSample else {
                        return Ok(FirstSampleResult::Buffered { encoder_name: name.clone() });
                    };
                    let output_buffer = unsafe { sample.ConvertToContiguousBuffer() }
                        .map_err(|error| format!("could not access H.264 output buffer: {error}"))?;
                    let output_len = unsafe { output_buffer.GetCurrentLength() }
                        .map_err(|error| format!("could not read H.264 output length: {error}"))?;
                    if output_len == 0 {
                        return Ok(FirstSampleResult::Buffered { encoder_name: name.clone() });
                    }
                    if output_len as usize > 16 * 1024 * 1024 {
                        return Err("encoder produced an H.264 output larger than the 16 MiB access-unit limit".into());
                    }
                    Ok(FirstSampleResult::Encoded { encoder_name: name.clone(), bytes: output_len as usize })
                })();

                let _ = unsafe { transform.ProcessMessage(MFT_MESSAGE_NOTIFY_END_OF_STREAM, 0) };
                let _ = unsafe { transform.ProcessMessage(MFT_MESSAGE_NOTIFY_END_STREAMING, 0) };
                drop(transform);
                let _ = unsafe { activation.ShutdownObject() };
                outcome
            })();
            match attempt {
                Ok(value) => { result = Some(value); drop(activation); break; }
                Err(error) => { last_error = Some(error); drop(activation); }
            }
        }
        unsafe { CoTaskMemFree(Some(activations.cast())) };
    }
    result.ok_or_else(|| last_error.unwrap_or_else(|| "no synchronous H.264 encoder was available".into()))
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
