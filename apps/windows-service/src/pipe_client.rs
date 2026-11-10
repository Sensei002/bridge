use std::{ffi::OsString, io, os::windows::ffi::OsStringExt, path::PathBuf};
use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE},
    System::{
        Pipes::GetNamedPipeClientProcessId,
        Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION},
    },
};

/// Kernel-reported identity metadata for the process connected to the service pipe.
///
/// This is attribution data, not authorization by itself. In particular, an image
/// path is not proof of publisher identity; mutating display commands remain
/// disabled until a signed-client policy and authenticated-session binding exist.
#[derive(Debug, Clone)]
pub struct PipeClientIdentity {
    pub process_id: u32,
    pub image_path: PathBuf,
}

pub fn identify(pipe: HANDLE) -> io::Result<PipeClientIdentity> {
    let mut process_id = 0_u32;
    let ok = unsafe { GetNamedPipeClientProcessId(pipe, &mut process_id) };
    if ok == 0 || process_id == 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "could not identify named-pipe client process",
        ));
    }

    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, process_id) };
    if process.is_null() {
        return Err(io::Error::last_os_error());
    }

    let result = query_image_path(process);
    unsafe { CloseHandle(process); }
    let image_path = result?;

    Ok(PipeClientIdentity { process_id, image_path })
}

fn query_image_path(process: HANDLE) -> io::Result<PathBuf> {
    let mut buffer = vec![0_u16; 32_768];
    let mut length = buffer.len() as u32;
    let ok = unsafe {
        QueryFullProcessImageNameW(process, 0, buffer.as_mut_ptr(), &mut length)
    };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    buffer.truncate(length as usize);
    Ok(PathBuf::from(OsString::from_wide(&buffer)))
}
