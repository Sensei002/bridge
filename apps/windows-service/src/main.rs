#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::{
    collections::HashMap,
    ffi::OsString,
    io,
    ptr::null_mut,
    sync::mpsc,
    thread,
    time::{Duration, Instant},
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
        CloseHandle, GetLastError, LocalFree, ERROR_BROKEN_PIPE, ERROR_NO_DATA, ERROR_PIPE_CONNECTED,
        ERROR_PIPE_LISTENING, HANDLE, INVALID_HANDLE_VALUE,
    },
    Security::{SECURITY_ATTRIBUTES},
    Security::Authorization::{
        ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
    },
    Storage::FileSystem::{ReadFile, WriteFile, FILE_FLAG_FIRST_PIPE_INSTANCE, PIPE_ACCESS_DUPLEX},
    System::{
        Pipes::{
            ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, PIPE_NOWAIT,
            PIPE_READMODE_MESSAGE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_MESSAGE,
        },
    },
};

mod driver;
mod pipe_client;
mod verify_publisher;

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
    Ok(result?)
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
            PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE,
            PIPE_TYPE_MESSAGE | PIPE_READMODE_MESSAGE | PIPE_NOWAIT | PIPE_REJECT_REMOTE_CLIENTS,
            1,
            512,
            512,
            0,
            &mut security,
        )
    };
    unsafe { LocalFree(descriptor); }

    if pipe == INVALID_HANDLE_VALUE {
        Err(io::Error::last_os_error())
    } else {
        Ok(pipe)
    }
}

fn run_pipe_server(shutdown: &mpsc::Receiver<()>) -> io::Result<()> {
    let mut challenges = bridge_display_service::challenge::ServiceChallengeRegistry::default();
    // Keep the original kernel process object alive from challenge issuance through
    // response consumption; a PID by itself is never used as a durable identity.
    let mut pinned_clients: HashMap<u32, pipe_client::PipeClientIdentity> = HashMap::new();
    while shutdown.try_recv().is_err() {
        for process_id in challenges.revoke_expired() {
            pinned_clients.remove(&process_id);
        }
        let pipe = create_pipe()?;
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
                ERROR_PIPE_LISTENING => {
                    thread::sleep(Duration::from_millis(100));
                    for process_id in challenges.revoke_expired() {
                        pinned_clients.remove(&process_id);
                    }
                    let exited = pinned_clients.iter()
                        .filter_map(|(process_id, identity)| identity.ensure_alive().is_err().then_some(*process_id))
                        .collect::<Vec<_>>();
                    for process_id in exited {
                        challenges.revoke_process(process_id);
                        pinned_clients.remove(&process_id);
                    }
                },
                error => {
                    unsafe { CloseHandle(pipe); }
                    return Err(io::Error::from_raw_os_error(error as i32));
                }
            }
        };

        if connected {
            let mut request = [0_u8; bridge_display_service::MAX_REQUEST_BYTES];
            let deadline = Instant::now() + Duration::from_secs(5);
            let (read, response) = loop {
                let mut read = 0_u32;
                let read_ok = unsafe {
                    ReadFile(pipe, request.as_mut_ptr(), request.len() as u32, &mut read, null_mut())
                };
                if read_ok != 0 {
                    let response = match pipe_client::identify(pipe) {
                        Ok(caller) => {
                            if let Err(error) = caller.ensure_alive() {
                                format!("BRIDGE_DISPLAY/1 ERROR client process is no longer valid: {error}\n").into_bytes()
                            } else {
                                eprintln!("Bridge Display Service request from PID {} ({})", caller.process_id, caller.image_path.display());
                                let response = handle_request(&request[..read as usize], &caller, &mut challenges, &mut pinned_clients);
                                if caller.ensure_alive().is_ok() {
                                    response
                                } else {
                                    b"BRIDGE_DISPLAY/1 ERROR client process exited during request\n".to_vec()
                                }
                            }
                        }
                        Err(error) => format!("BRIDGE_DISPLAY/1 ERROR caller identity unavailable: {error}\n").into_bytes(),
                    };
                    break (read, response);
                }
                match unsafe { GetLastError() } {
                    ERROR_NO_DATA if Instant::now() < deadline => thread::sleep(Duration::from_millis(20)),
                    ERROR_BROKEN_PIPE => break (0, b"BRIDGE_DISPLAY/1 ERROR client disconnected\n".to_vec()),
                    _ => break (0, b"BRIDGE_DISPLAY/1 ERROR read failed\n".to_vec()),
                }
            };
            let mut written = 0_u32;
            if read > 0 {
                let _ = unsafe {
                    WriteFile(pipe, response.as_ptr(), response.len() as u32, &mut written, null_mut())
                };
            }
            unsafe {
                DisconnectNamedPipe(pipe);
                CloseHandle(pipe);
            }
        }
    }
    Ok(())
}

fn handle_request(bytes: &[u8], caller: &pipe_client::PipeClientIdentity, challenges: &mut bridge_display_service::challenge::ServiceChallengeRegistry, pinned_clients: &mut HashMap<u32, pipe_client::PipeClientIdentity>) -> Vec<u8> {
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
        Request::Challenge => verify_publisher::verify_client_publisher(&caller.image_path)
            .and_then(|_| {
                let nonce = challenges.issue_after_client_verification(caller.process_id)
                    .map_err(|message| io::Error::new(io::ErrorKind::PermissionDenied, message))?;
                pinned_clients.insert(caller.process_id, caller.clone());
                Ok(format!("BRIDGE_DISPLAY/1 CHALLENGE nonce={nonce}\n").into_bytes())
            }),
        Request::ChallengeResponse(nonce) => verify_publisher::verify_client_publisher(&caller.image_path)
            .and_then(|_| {
                let pinned = pinned_clients.get(&caller.process_id).ok_or_else(|| {
                    io::Error::new(io::ErrorKind::PermissionDenied, "no challenge is pending for this process")
                })?;
                pinned.ensure_alive()?;
                caller.ensure_alive()?;
                let same_image = pinned.image_path.to_string_lossy()
                    .eq_ignore_ascii_case(&caller.image_path.to_string_lossy());
                if !same_image {
                    return Err(io::Error::new(io::ErrorKind::PermissionDenied, "challenge process image changed"));
                }
                challenges.consume_for_process(caller.process_id, &nonce)
                    .map_err(|message| io::Error::new(io::ErrorKind::PermissionDenied, message))?;
                pinned_clients.remove(&caller.process_id);
                Ok(b"BRIDGE_DISPLAY/1 OK challenge_verified\n".to_vec())
            }),
        Request::AttestationChallenge { session_id, device_fingerprint, pinned_public_key } =>
            verify_publisher::verify_client_publisher(&caller.image_path).and_then(|_| {
                let public_key = decode_hex_array::<32>(&pinned_public_key)?;
                let nonce = challenges.issue_attestation_after_client_verification(
                    caller.process_id,
                    session_id,
                    &device_fingerprint,
                    &public_key,
                ).map_err(|message| io::Error::new(io::ErrorKind::PermissionDenied, message))?;
                pinned_clients.insert(caller.process_id, caller.clone());
                Ok(format!("BRIDGE_DISPLAY/1 CHALLENGE nonce={nonce}\n").into_bytes())
            }),
        Request::AttestationResponse { nonce, session_id, device_fingerprint, signature } =>
            verify_publisher::verify_client_publisher(&caller.image_path).and_then(|_| {
                let pinned = pinned_clients.remove(&caller.process_id).ok_or_else(|| {
                    io::Error::new(io::ErrorKind::PermissionDenied, "no attestation challenge is pending for this process")
                })?;
                pinned.ensure_alive()?;
                caller.ensure_alive()?;
                let same_image = pinned.image_path.to_string_lossy()
                    .eq_ignore_ascii_case(&caller.image_path.to_string_lossy());
                if !same_image {
                    challenges.revoke_process(caller.process_id);
                    return Err(io::Error::new(io::ErrorKind::PermissionDenied, "challenge process image changed"));
                }
                let signature = decode_hex_array::<64>(&signature)?;
                challenges.consume_signed_attestation_for_process(
                    caller.process_id,
                    &nonce,
                    session_id,
                    &device_fingerprint,
                    &signature,
                ).map_err(|message| io::Error::new(io::ErrorKind::PermissionDenied, message))?;
                Ok(b"BRIDGE_DISPLAY/1 OK attestation_verified\n".to_vec())
            }),
        // Fail closed until the service can verify the caller process and bind
        // this request to a live, authenticated Android Second Screen session.
        // Merely reaching the interactive-user named pipe is not authorization.
        Request::Connect => verify_publisher::verify_client_publisher(&caller.image_path).and_then(|_| {
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "CONNECT is disabled until authenticated session binding is implemented",
            ))
        }),
        Request::Disconnect => verify_publisher::verify_client_publisher(&caller.image_path).and_then(|_| {
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "DISCONNECT is disabled until authenticated session binding is implemented",
            ))
        }),
    };
    result.unwrap_or_else(|error| format!("BRIDGE_DISPLAY/1 ERROR {error}\n").into_bytes())
}


/// Decode fixed-size hexadecimal IPC fields without accepting alternate encodings.
fn decode_hex_array<const N: usize>(value: &str) -> io::Result<[u8; N]> {
    if value.len() != N * 2 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid hexadecimal attestation field"));
    }
    let mut output = [0_u8; N];
    for (index, byte) in output.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid hexadecimal attestation field"))?;
    }
    Ok(output)
}
