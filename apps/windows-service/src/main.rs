#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::{
    ffi::OsString,
    io,
    ptr::{null, null_mut},
    sync::mpsc,
    thread,
    time::Duration,
};
use windows_service::{
    define_windows_service,
    service::{
        ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus,
        ServiceType,
    },
    service_control_handler::{self, ServiceControlHandlerResult},
    service_dispatcher,
};
use windows_sys::Win32::{
    Foundation::{
        CloseHandle, GetLastError, ERROR_BROKEN_PIPE, ERROR_NO_DATA, ERROR_PIPE_CONNECTED,
        ERROR_PIPE_LISTENING, HANDLE, INVALID_HANDLE_VALUE,
    },
    Security::{
        ConvertStringSecurityDescriptorToSecurityDescriptorW, SECURITY_ATTRIBUTES,
        SDDL_REVISION_1,
    },
    System::{
        IO::{DeviceIoControl, ReadFile, WriteFile},
        Memory::LocalFree,
        Pipes::{
            ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, PIPE_ACCESS_DUPLEX,
            PIPE_NOWAIT, PIPE_READMODE_MESSAGE, PIPE_TYPE_MESSAGE,
        },
    },
};

mod driver;

const SERVICE_NAME: &str = "BridgeDisplayService";
const PIPE_NAME: &str = r"\\.\pipe\BridgeDisplayService";

fn main() -> Result<(), windows_service::Error> {
    service_dispatcher::start(SERVICE_NAME, ffi_service_main)
}

define_windows_service!(ffi_service_main, service_main);

fn service_main(_arguments: Vec<OsString>) {
    if let Err(error) = run_service() {
        eprintln!("Bridge Display Service stopped: {error}");
    }
}

fn run_service() -> Result<(), Box<dyn std::error::Error>> {
    let (shutdown_tx, shutdown_rx) = mpsc::channel::<()>();
    let status_handle = service_control_handler::register(SERVICE_NAME, move |control| {
        match control {
            ServiceControl::Stop | ServiceControl::Shutdown => {
                let _ = shutdown_tx.send(());
                ServiceControlHandlerResult::NoError
            }
            ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
            _ => ServiceControlHandlerResult::NotImplemented,
        }
    })?;

    status_handle.set_service_status(ServiceStatus {
        service_type: ServiceType::OWN_PROCESS,
        current_state: ServiceState::Running,
        controls_accepted: ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN,
        exit_code: ServiceExitCode::Win32(0),
        checkpoint: 0,
        wait_hint: Duration::default(),
        process_id: None,
    })?;

    let result = run_pipe_server(&shutdown_rx);

    let _ = status_handle.set_service_status(ServiceStatus {
        service_type: ServiceType::OWN_PROCESS,
        current_state: ServiceState::Stopped,
        controls_accepted: ServiceControlAccept::empty(),
        exit_code: ServiceExitCode::Win32(if result.is_ok() { 0 } else { 1 }),
        checkpoint: 0,
        wait_hint: Duration::default(),
        process_id: None,
    });
    result
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

fn create_pipe() -> io::Result<HANDLE> {
    // SYSTEM and Administrators have full access. Interactive users can only use
    // the narrow versioned command protocol; remote network clients are not allowed.
    let sddl = wide("D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GRGW;;;IU)");
    let mut descriptor = null_mut();
    let converted = unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            SDDL_REVISION_1,
            &mut descriptor,
            null_mut(),
        )
    };
    if converted == 0 {
        return Err(io::Error::last_os_error());
    }

    let mut security = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor,
        bInheritHandle: 0,
    };
    let name = wide(PIPE_NAME);
    let pipe = unsafe {
        CreateNamedPipeW(
            name.as_ptr(),
            PIPE_ACCESS_DUPLEX,
            PIPE_TYPE_MESSAGE | PIPE_READMODE_MESSAGE | PIPE_NOWAIT,
            1,
            512,
            512,
            0,
            &mut security,
        )
    };
    unsafe { LocalFree(descriptor as isize); }

    if pipe == INVALID_HANDLE_VALUE {
        Err(io::Error::last_os_error())
    } else {
        Ok(pipe)
    }
}

fn run_pipe_server(shutdown: &mpsc::Receiver<()>) -> io::Result<()> {
    while shutdown.try_recv().is_err() {
        let pipe = create_pipe()?;
        let name = wide(PIPE_NAME);
        let connected = loop {
            if shutdown.try_recv().is_ok() {
                unsafe { CloseHandle(pipe); }
                return Ok(());
            }
            let result = unsafe { ConnectNamedPipe(pipe, null_mut()) };
            if result != 0 {
                break true;
            }
            match unsafe { GetLastError() } {
                ERROR_PIPE_CONNECTED => break true,
                ERROR_PIPE_LISTENING => thread::sleep(Duration::from_millis(100)),
                error => {
                    unsafe { CloseHandle(pipe); }
                    return Err(io::Error::from_raw_os_error(error as i32));
                }
            }
        };

        if connected {
            let mut request = [0_u8; bridge_display_service::MAX_REQUEST_BYTES];
            let mut read = 0_u32;
            let read_ok = unsafe {
                ReadFile(pipe, request.as_mut_ptr(), request.len() as u32, &mut read, null_mut())
            };
            let response = if read_ok != 0 {
                handle_request(&request[..read as usize])
            } else {
                let error = unsafe { GetLastError() };
                if error == ERROR_NO_DATA {
                    b"BRIDGE_DISPLAY/1 ERROR empty request\n".to_vec()
                } else {
                    b"BRIDGE_DISPLAY/1 ERROR read failed\n".to_vec()
                }
            };
            let mut written = 0_u32;
            let _ = unsafe {
                WriteFile(pipe, response.as_ptr(), response.len() as u32, &mut written, null_mut())
            };
            unsafe {
                DisconnectNamedPipe(pipe);
                CloseHandle(pipe);
            }
        }
        let _ = name;
    }
    Ok(())
}

fn handle_request(bytes: &[u8]) -> Vec<u8> {
    use bridge_display_service::{DisplayStatus, Request};
    let request = match Request::parse(bytes) {
        Ok(request) => request,
        Err(error) => return format!("BRIDGE_DISPLAY/1 ERROR {error}\n").into_bytes(),
    };

    let result = match request {
        Request::Ping => Ok(b"BRIDGE_DISPLAY/1 PONG\n".to_vec()),
        Request::Query => driver::query().map(|status| {
            DisplayStatus {
                connected: status.is_connected != 0,
                supported_mode_count: status.supported_mode_count,
            }
            .encode()
            .into_bytes()
        }),
        Request::Connect => driver::set_connected(true)
            .map(|_| b"BRIDGE_DISPLAY/1 OK connect\n".to_vec()),
        Request::Disconnect => driver::set_connected(false)
            .map(|_| b"BRIDGE_DISPLAY/1 OK disconnect\n".to_vec()),
    };
    result.unwrap_or_else(|error| format!("BRIDGE_DISPLAY/1 ERROR {error}\n").into_bytes())
}

impl Drop for PipeHandle {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0); }
    }
}

struct PipeHandle(HANDLE);
