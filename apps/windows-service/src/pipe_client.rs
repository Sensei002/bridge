use std::{ffi::OsString, io, os::windows::ffi::OsStringExt, path::PathBuf};
use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE, SYNCHRONIZE, WAIT_TIMEOUT},
    System::{
        Pipes::GetNamedPipeClientProcessId,
        Threading::{
            OpenProcess, QueryFullProcessImageNameW, WaitForSingleObject,
            PROCESS_QUERY_LIMITED_INFORMATION,
        },
    },
};

/// Kernel-reported identity metadata for the process connected to the service pipe.
///
/// The process handle is deliberately retained for the complete request lifetime.
/// This pins the kernel process object, preventing a PID from being recycled and
/// mistaken for the original client between identification and authorization.
///
/// This remains attribution, not publisher authentication: image paths are not
/// proof of signer identity. Mutating display commands remain fail-closed until
/// the service verifies the signed client and binds an authenticated Noise peer.
#[derive(Debug)]
pub struct PipeClientIdentity {
    pub process_id: u32,
    pub image_path: PathBuf,
    process: OwnedProcessHandle,
}

impl PipeClientIdentity {
    /// Reject a client that exited while its pipe request was being processed.
    pub fn ensure_alive(&self) -> io::Result<()> {
        let state = unsafe { WaitForSingleObject(self.process.0, 0) };
        if state == WAIT_TIMEOUT {
            Ok(())
        } else if state == 0 {
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "named-pipe client process exited during the request",
            ))
        } else {
            Err(io::Error::last_os_error())
        }
    }
}

#[derive(Debug)]
struct OwnedProcessHandle(HANDLE);

impl Drop for OwnedProcessHandle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { CloseHandle(self.0); }
        }
    }
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

    // SYNCHRONIZE lets us retain and check this exact process object rather than
    // reopening a PID later, which would be vulnerable to PID reuse.
    let process = unsafe {
        OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE, 0, process_id)
    };
    if process.is_null() {
        return Err(io::Error::last_os_error());
    }
    let process = OwnedProcessHandle(process);

    let image_path = query_image_path(process.0)?;
    let identity = PipeClientIdentity { process_id, image_path, process };
    identity.ensure_alive()?;
    Ok(identity)
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
