//! DXGI Desktop Duplication capture for the exact upstream VirtualDrivers output.
//!
//! Capture runs in the interactive desktop process (not the Session 0 service).
//! The output is selected by joining DXGI's \\.\DISPLAYx name to the GDI
//! adapter entry whose PnP hardware ID is ROOT\MttVDD. No primary-display or
//! display-index fallback is permitted.

use std::{mem::size_of, ptr::null, time::Instant};
use bridge_transfer::DisplayFrame;
use windows::{
    core::Interface,
    Win32::{
        Foundation::HMODULE,
        Graphics::{
            Direct3D::{D3D_DRIVER_TYPE_UNKNOWN, D3D_FEATURE_LEVEL_11_0},
            Direct3D11::{
                D3D11CreateDevice, D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                D3D11_MAP_READ, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING,
                D3D11_MAPPED_SUBRESOURCE, ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D,
            },
            Dxgi::{
                CreateDXGIFactory1, DXGI_OUTDUPL_FRAME_INFO,
                IDXGIAdapter, IDXGIFactory1, IDXGIOutput1, IDXGIOutputDuplication, IDXGIResource,
            },
            Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC},
        },
    },
};
use windows_sys::Win32::Graphics::Gdi::{EnumDisplayDevicesW, DISPLAY_DEVICEW};

const MAX_ADAPTERS: u32 = 16;
const MAX_OUTPUTS: u32 = 32;
const MAX_DIMENSION: u32 = 4096;
const MAX_FRAME_BYTES: usize = 64 * 1024 * 1024;
const DXGI_ERROR_WAIT_TIMEOUT_CODE: i32 = 0x887A0027_u32 as i32;
const DXGI_ERROR_ACCESS_LOST_CODE: i32 = 0x887A0026_u32 as i32;

/// Owns one DXGI duplication session for the upstream virtual monitor.
pub struct VddCapture {
    duplication: IDXGIOutputDuplication,
    context: ID3D11DeviceContext,
    staging: ID3D11Texture2D,
    width: u32,
    height: u32,
    sequence: u64,
    started: Instant,
}

struct AcquiredFrame<'a>(&'a IDXGIOutputDuplication);

impl Drop for AcquiredFrame<'_> {
    fn drop(&mut self) {
        let _ = unsafe { self.0.ReleaseFrame() };
    }
}

impl VddCapture {
    /// Find and duplicate only the output backed by ROOT\MttVDD.
    pub fn open() -> Result<Self, String> {
        let factory: IDXGIFactory1 = unsafe { CreateDXGIFactory1() }
            .map_err(|error| format!("DXGI factory creation failed: {error}"))?;

        for adapter_index in 0..MAX_ADAPTERS {
            let adapter = match unsafe { factory.EnumAdapters1(adapter_index) } {
                Ok(adapter) => adapter,
                Err(_) => break,
            };
            let dxgi_adapter: IDXGIAdapter = adapter.cast()
                .map_err(|error| format!("could not query DXGI adapter: {error}"))?;

            for output_index in 0..MAX_OUTPUTS {
                let output = match unsafe { dxgi_adapter.EnumOutputs(output_index) } {
                    Ok(output) => output,
                    Err(_) => break,
                };
                let output1: IDXGIOutput1 = output.cast()
                    .map_err(|error| format!("VDD output does not expose IDXGIOutput1: {error}"))?;
                let output_desc = unsafe { output1.GetDesc() }
                    .map_err(|error| format!("could not query DXGI output description: {error}"))?;
                let device_name = wide_field(&output_desc.DeviceName);
                if device_name.is_empty() || !is_upstream_vdd_output(&device_name) {
                    continue;
                }
                if !output_desc.AttachedToDesktop.as_bool() {
                    return Err(format!("upstream VDD output {device_name} exists but is not attached to the Windows desktop; select Extend in Display Settings"));
                }

                let mut device: Option<ID3D11Device> = None;
                let mut context: Option<ID3D11DeviceContext> = None;
                let feature_levels = [D3D_FEATURE_LEVEL_11_0];
                unsafe {
                    D3D11CreateDevice(
                        &dxgi_adapter,
                        D3D_DRIVER_TYPE_UNKNOWN,
                        HMODULE::default(),
                        D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                        Some(&feature_levels),
                        D3D11_SDK_VERSION,
                        Some(&mut device),
                        None,
                        Some(&mut context),
                    )
                }.map_err(|error| format!("could not create D3D11 device for {device_name}: {error}"))?;
                let device = device.ok_or_else(|| "D3D11CreateDevice returned no device".to_string())?;
                let context = context.ok_or_else(|| "D3D11CreateDevice returned no immediate context".to_string())?;
                let duplication = unsafe { output1.DuplicateOutput(&device) }
                    .map_err(|error| format!("DXGI could not duplicate upstream VDD output {device_name}: {error}"))?;
                let duplication_desc = unsafe { duplication.GetDesc() };
                let width = duplication_desc.ModeDesc.Width;
                let height = duplication_desc.ModeDesc.Height;
                if width == 0 || height == 0 || width > MAX_DIMENSION || height > MAX_DIMENSION {
                    return Err(format!("upstream VDD output has unsupported dimensions {width}x{height}"));
                }
                if duplication_desc.ModeDesc.Format != DXGI_FORMAT_B8G8R8A8_UNORM {
                    return Err(format!("upstream VDD output uses unsupported DXGI pixel format {:?}; expected BGRA8", duplication_desc.ModeDesc.Format));
                }

                let staging_desc = D3D11_TEXTURE2D_DESC {
                    Width: width,
                    Height: height,
                    MipLevels: 1,
                    ArraySize: 1,
                    Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                    Usage: D3D11_USAGE_STAGING,
                    BindFlags: 0,
                    CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
                    MiscFlags: 0,
                };
                let mut staging = None;
                unsafe { device.CreateTexture2D(&staging_desc, None, Some(&mut staging)) }
                    .map_err(|error| format!("could not allocate bounded DXGI staging texture: {error}"))?;
                let staging = staging.ok_or_else(|| "D3D11 returned no staging texture".to_string())?;

                return Ok(Self {
                    duplication, context, staging, width, height, sequence: 0,
                    started: Instant::now(),
                });
            }
        }

        Err("upstream VirtualDrivers display output was not found in DXGI; install the signed VDD and enable Extend in Windows Display Settings".into())
    }

    /// Return a complete BGRA8 frame, or None when no update arrived in 16 ms.
    pub fn next_frame(&mut self) -> Result<Option<DisplayFrame>, String> {
        let mut frame_info = DXGI_OUTDUPL_FRAME_INFO::default();
        let mut resource: Option<IDXGIResource> = None;
        match unsafe { self.duplication.AcquireNextFrame(16, &mut frame_info, &mut resource) } {
            Ok(()) => {}
            Err(error) if error.code().0 == DXGI_ERROR_WAIT_TIMEOUT_CODE => return Ok(None),
            Err(error) if error.code().0 == DXGI_ERROR_ACCESS_LOST_CODE => {
                return Err("DXGI desktop duplication access was lost; capture must be reopened after display topology or desktop mode changes".into());
            }
            Err(error) => return Err(format!("DXGI AcquireNextFrame failed: {error}")),
        }
        let _release = AcquiredFrame(&self.duplication);
        let resource = resource.ok_or_else(|| "DXGI returned an empty desktop resource".to_string())?;
        let texture: ID3D11Texture2D = resource.cast()
            .map_err(|error| format!("DXGI desktop resource is not an ID3D11Texture2D: {error}"))?;

        // Desktop Duplication can hand us a resource whose mode no longer matches
        // the staging texture after rotation, resolution changes, or topology
        // transitions. CopyResource requires compatible resources; detect this
        // before copying and let the caller reopen the duplication session.
        let source_desc = unsafe { texture.GetDesc() };
        validate_frame_texture(
            source_desc.Width,
            source_desc.Height,
            source_desc.Format,
            self.width,
            self.height,
        )?;

        unsafe { self.context.CopyResource(&self.staging, &texture) };
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        unsafe { self.context.Map(&self.staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped)) }
            .map_err(|error| format!("could not map DXGI staging texture for readback: {error}"))?;

        let row_bytes = (self.width as usize).checked_mul(4)
            .ok_or_else(|| "DXGI frame row size overflow".to_string())?;
        let pitch = mapped.RowPitch as usize;
        let total_bytes = row_bytes.checked_mul(self.height as usize)
            .ok_or_else(|| "DXGI frame size overflow".to_string())?;
        let result = if mapped.pData.is_null() || pitch < row_bytes || total_bytes > MAX_FRAME_BYTES {
            Err("DXGI returned an invalid or oversized mapped frame".to_string())
        } else {
            let mut pixels = vec![0_u8; total_bytes];
            let source = mapped.pData.cast::<u8>();
            for row in 0..self.height as usize {
                let source_row = unsafe { std::slice::from_raw_parts(source.add(row * pitch), row_bytes) };
                let target_start = row * row_bytes;
                pixels[target_start..target_start + row_bytes].copy_from_slice(source_row);
            }
            Ok(pixels)
        };
        unsafe { self.context.Unmap(&self.staging, 0) };
        let pixels = result?;

        self.sequence = self.sequence.saturating_add(1);
        let timestamp_ns = self.started.elapsed().as_nanos().min(u64::MAX as u128) as u64;
        Ok(Some(DisplayFrame {
            sequence: self.sequence,
            width: self.width,
            height: self.height,
            stride: row_bytes as u32,
            timestamp_ns: timestamp_ns.max(1),
            pixels,
        }))
    }

    pub fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }
}

fn validate_frame_texture(
    width: u32,
    height: u32,
    format: windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT,
    expected_width: u32,
    expected_height: u32,
) -> Result<(), String> {
    if width != expected_width || height != expected_height {
        return Err(format!(
            "upstream VDD mode changed from {}x{} to {}x{}; reopen DXGI capture and recreate its staging texture",
            expected_width, expected_height, width, height
        ));
    }
    if format != DXGI_FORMAT_B8G8R8A8_UNORM {
        return Err(format!(
            "upstream VDD frame format changed to {:?}; reopen capture and negotiate a supported format",
            format
        ));
    }
    Ok(())
}

fn wide_field(value: &[u16]) -> String {
    let end = value.iter().position(|unit| *unit == 0).unwrap_or(value.len());
    String::from_utf16_lossy(&value[..end])
}

fn is_upstream_vdd_output(device_name: &str) -> bool {
    for index in 0..64 {
        let mut device: DISPLAY_DEVICEW = unsafe { std::mem::zeroed() };
        device.cb = size_of::<DISPLAY_DEVICEW>() as u32;
        if unsafe { EnumDisplayDevicesW(null(), index, &mut device, 0) } == 0 {
            break;
        }
        let name = wide_field(&device.DeviceName);
        if !name.eq_ignore_ascii_case(device_name) {
            continue;
        }
        let hardware_id = wide_field(&device.DeviceID);
        return is_vdd_hardware_id(&hardware_id);
    }
    false
}

fn is_vdd_hardware_id(value: &str) -> bool {
    value.to_ascii_uppercase().starts_with("ROOT\\MTTVDD\\")
        || value.eq_ignore_ascii_case("ROOT\\MTTVDD")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_only_the_opened_bgra8_mode() {
        assert!(validate_frame_texture(1920, 1080, DXGI_FORMAT_B8G8R8A8_UNORM, 1920, 1080).is_ok());
        assert!(validate_frame_texture(1080, 1920, DXGI_FORMAT_B8G8R8A8_UNORM, 1920, 1080)
            .unwrap_err().contains("mode changed"));
        assert!(validate_frame_texture(1920, 1080, windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_R8G8B8A8_UNORM, 1920, 1080)
            .unwrap_err().contains("format changed"));
    }

    #[test]
    fn vdd_hardware_id_match_is_exact_to_upstream_root_device() {
        assert!(is_vdd_hardware_id("ROOT\\MttVDD\\0000"));
        assert!(is_vdd_hardware_id("root\\mttvdd"));
        assert!(!is_vdd_hardware_id("ROOT\\DISPLAY\\0000"));
        assert!(!is_vdd_hardware_id("PCI\\VEN_1234&DEV_5678"));
    }

    #[test]
    fn decodes_device_names_without_reading_past_the_nul() {
        assert_eq!(wide_field(&[b'V' as u16, b'D' as u16, 0, b'X' as u16]), "VD");
    }
}
