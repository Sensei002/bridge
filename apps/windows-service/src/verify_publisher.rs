//! Fail-closed Authenticode verification for the privileged display service.
//!
//! The expected Bridge desktop publisher is provisioned by the elevated installer
//! beside this service executable in Program Files. The file is protected by the
//! service-directory ACL (SYSTEM/Administrators write; Users read-only). It contains
//! the SHA-256 hash of the desktop signer's leaf certificate, not the driver signer.

use std::{fs, io, os::windows::ffi::OsStrExt, path::Path};

use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE},
    Security::{
        Cryptography::{CertGetCertificateContextProperty, CERT_CONTEXT, CERT_SHA256_HASH_PROP_ID},
        WinTrust::{
            WinVerifyTrust, WTHelperGetProvCertFromChain, WTHelperGetProvSignerFromChain,
            WTHelperProvDataFromStateData, CRYPT_PROVIDER_CERT, CRYPT_PROVIDER_SGNR,
            WINTRUST_ACTION_GENERIC_VERIFY_V2, WINTRUST_DATA, WINTRUST_DATA_0,
            WINTRUST_FILE_INFO, WTD_CHOICE_FILE, WTD_REVOKE_WHOLECHAIN,
            WTD_REVOCATION_CHECK_CHAIN, WTD_STATEACTION_CLOSE, WTD_STATEACTION_VERIFY,
            WTD_UI_NONE,
        },
    },
    Storage::FileSystem::CreateFileW,
};

const GENERIC_READ: u32 = 0x8000_0000;
const FILE_SHARE_READ: u32 = 0x0000_0001;
const OPEN_EXISTING: u32 = 3;
const FILE_ATTRIBUTE_NORMAL: u32 = 0x0000_0080;

struct OwnedFile(HANDLE);

impl Drop for OwnedFile {
    fn drop(&mut self) {
        if !self.0.is_null() && self.0 as isize != -1 {
            unsafe { CloseHandle(self.0); }
        }
    }
}

/// Verify the process image's Authenticode trust chain and pin its leaf
/// certificate to the publisher hash installed beside the privileged service.
pub fn verify_client_publisher(image_path: &Path) -> io::Result<()> {
    let service_path = std::env::current_exe()?;
    let service_directory = service_path.parent().ok_or_else(|| {
        io::Error::new(io::ErrorKind::NotFound, "Bridge Display Service directory is unavailable")
    })?;
    let pin_path = service_directory.join("bridge-desktop-publisher.sha256");
    let pin_contents = fs::read_to_string(&pin_path).map_err(|error| {
        io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("Bridge desktop publisher pin is unavailable: {error}"),
        )
    })?;
    let expected = normalize_sha256(&pin_contents)?;

    let path: Vec<u16> = image_path.as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    // Do not permit concurrent writes or replacement while WinVerifyTrust
    // evaluates the exact file handle. The path itself comes from the kernel's
    // named-pipe client process query, never from the request payload.
    let file = unsafe {
        CreateFileW(
            path.as_ptr(),
            GENERIC_READ,
            FILE_SHARE_READ,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            std::ptr::null_mut(),
        )
    };
    if file.is_null() || file as isize == -1 {
        return Err(io::Error::last_os_error());
    }
    let file = OwnedFile(file);

    let mut file_info = WINTRUST_FILE_INFO {
        cbStruct: std::mem::size_of::<WINTRUST_FILE_INFO>() as u32,
        pcwszFilePath: path.as_ptr(),
        hFile: file.0,
        pgKnownSubject: std::ptr::null_mut(),
    };
    let mut data = WINTRUST_DATA {
        cbStruct: std::mem::size_of::<WINTRUST_DATA>() as u32,
        pPolicyCallbackData: std::ptr::null_mut(),
        pSIPClientData: std::ptr::null_mut(),
        dwUIChoice: WTD_UI_NONE,
        fdwRevocationChecks: WTD_REVOKE_WHOLECHAIN,
        dwUnionChoice: WTD_CHOICE_FILE,
        Anonymous: WINTRUST_DATA_0 { pFile: &mut file_info },
        dwStateAction: WTD_STATEACTION_VERIFY,
        hWVTStateData: std::ptr::null_mut(),
        pwszURLReference: std::ptr::null_mut(),
        dwProvFlags: WTD_REVOCATION_CHECK_CHAIN,
        dwUIContext: 0,
        pSignatureSettings: std::ptr::null_mut(),
    };
    let mut action = WINTRUST_ACTION_GENERIC_VERIFY_V2;
    let trust_status = unsafe {
        WinVerifyTrust(std::ptr::null_mut(), &mut action, &mut data as *mut _ as *mut _)
    };

    let signer_hash = if trust_status == 0 {
        signer_sha256(data.hWVTStateData)
    } else {
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("Windows Authenticode trust verification failed (0x{:08X})", trust_status as u32),
        ))
    };

    // Always release WinTrust provider state after a verification attempt.
    data.dwStateAction = WTD_STATEACTION_CLOSE;
    unsafe {
        WinVerifyTrust(std::ptr::null_mut(), &mut action, &mut data as *mut _ as *mut _);
    }

    let actual = signer_hash?;
    if !constant_time_eq(&actual, &expected) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "desktop executable signer does not match the installed Bridge publisher pin",
        ));
    }
    Ok(())
}

fn signer_sha256(state: HANDLE) -> io::Result<String> {
    if state.is_null() {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, "Authenticode provider returned no verification state"));
    }
    let provider = unsafe { WTHelperProvDataFromStateData(state) };
    if provider.is_null() {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, "Authenticode provider data is unavailable"));
    }
    let signer: *mut CRYPT_PROVIDER_SGNR =
        unsafe { WTHelperGetProvSignerFromChain(provider, 0, 0, 0) };
    if signer.is_null() {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, "Authenticode signature has no primary signer"));
    }
    let certificate: *mut CRYPT_PROVIDER_CERT =
        unsafe { WTHelperGetProvCertFromChain(signer, 0) };
    if certificate.is_null() {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, "Authenticode signer certificate is unavailable"));
    }
    let context: *const CERT_CONTEXT = unsafe { (*certificate).pCert };
    if context.is_null() {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, "Authenticode signer certificate context is invalid"));
    }

    let mut hash = [0_u8; 32];
    let mut length = hash.len() as u32;
    let ok = unsafe {
        CertGetCertificateContextProperty(
            context,
            CERT_SHA256_HASH_PROP_ID,
            hash.as_mut_ptr() as *mut _,
            &mut length,
        )
    };
    if ok == 0 || length as usize != hash.len() {
        return Err(io::Error::last_os_error());
    }
    Ok(hash.iter().map(|byte| format!("{byte:02X}")).collect())
}

fn normalize_sha256(value: &str) -> io::Result<String> {
    let normalized: String = value.chars()
        .filter(|character| *character != ':' && !character.is_ascii_whitespace())
        .collect();
    if normalized.len() != 64 || !normalized.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "installed Bridge desktop publisher pin must contain exactly 64 hexadecimal characters",
        ));
    }
    Ok(normalized.to_ascii_uppercase())
}

fn constant_time_eq(left: &str, right: &str) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.bytes().zip(right.bytes()).fold(0_u8, |difference, (a, b)| difference | (a ^ b)) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_only_well_formed_sha256_pins() {
        assert_eq!(normalize_sha256(&"ab".repeat(32)).unwrap(), "AB".repeat(32));
        assert_eq!(normalize_sha256(&("AB:".to_owned() + &"CD:".repeat(30) + "CD")).unwrap(), "AB".to_owned() + "CD".repeat(31));
        assert!(normalize_sha256("").is_err());
        assert!(normalize_sha256(&"a".repeat(40)).is_err());
        assert!(normalize_sha256(&"g".repeat(64)).is_err());
    }

    #[test]
    fn compares_pins_without_early_byte_exit() {
        let expected = "A1".repeat(32);
        assert!(constant_time_eq(&expected, &expected));
        assert!(!constant_time_eq(&expected, &("A1".repeat(31) + "A2")));
        assert!(!constant_time_eq(&expected, "short"));
    }
}
