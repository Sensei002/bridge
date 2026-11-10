//! Stable C/JNI boundary for using the shared Noise XX implementation on mobile.
use std::{
    collections::HashMap,
    ffi::{CStr, CString},
    net::{SocketAddr, TcpListener, TcpStream, ToSocketAddrs},
    os::raw::c_char,
    sync::{Arc, Mutex, OnceLock, mpsc::{self, Receiver}, atomic::{AtomicBool, AtomicU64, Ordering}},
    thread,
    time::{Duration, Instant},
};

use bridge_network::{accept_noise_xx, connect_noise_xx, NoiseConnection};
use bridge_protocol::{DeviceDescriptor, DeviceId};
use serde::{Deserialize, Serialize};

const MAX_ACTIVE_SESSIONS: usize = 32;
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5);
const SESSION_READ_TIMEOUT: Duration = Duration::from_secs(15);
const HEARTBEAT_PING: &[u8] = b"\0BRIDGE_HEARTBEAT_PING_V1";
const HEARTBEAT_PONG: &[u8] = b"\0BRIDGE_HEARTBEAT_PONG_V1";
static NEXT_HANDLE: AtomicU64 = AtomicU64::new(1);
struct SessionEntry { connection: Arc<Mutex<NoiseConnection>>, incoming: Mutex<Receiver<Vec<u8>>>, stop: Arc<AtomicBool> }

static SESSIONS: OnceLock<Mutex<HashMap<u64, Arc<SessionEntry>>>> = OnceLock::new();

fn sessions() -> &'static Mutex<HashMap<u64, Arc<SessionEntry>>> {
    SESSIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

#[derive(Deserialize)]
struct PairRequest {
    host: Option<String>,
    bind_ip: Option<String>,
    port: u16,
    private_key_hex: String,
    local_device: DeviceDescriptor,
    expected_fingerprint: Option<String>,
    expected_device_id: Option<DeviceId>,
    timeout_ms: Option<u64>,
}

#[derive(Serialize)]
struct PairResult {
    ok: bool,
    handle: Option<u64>,
    remote_fingerprint: Option<String>,
    remote_device: Option<DeviceDescriptor>,
    error: Option<String>,
    role: Option<String>,
}

fn result_error(error: impl ToString) -> PairResult {
    PairResult {
        ok: false, handle: None, remote_fingerprint: None,
        remote_device: None, error: Some(error.to_string()), role: None,
    }
}

fn decode_hex(value: &str) -> Result<Vec<u8>, String> {
    if value.len() != 64 || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("Noise private key must be 64 hexadecimal characters".into());
    }
    (0..value.len()).step_by(2)
        .map(|i| u8::from_str_radix(&value[i..i + 2], 16).map_err(|e: std::num::ParseIntError| e.to_string()))
        .collect()
}

fn register(
    mut connection: NoiseConnection,
    local: &DeviceDescriptor,
    expected_device_id: Option<&DeviceId>,
    role: &'static str,
) -> Result<PairResult, String> {
    let remote = connection.exchange_descriptors(local).map_err(|e: bridge_network::NetworkError| e.to_string())?;
    if expected_device_id.is_some_and(|expected| *expected != remote.id) {
        return Err("authenticated device ID does not match the selected peer".into());
    }
    let mut registry = sessions().lock().map_err(|_| "session registry poisoned")?;
    if registry.len() >= MAX_ACTIVE_SESSIONS {
        return Err("active session limit reached".into());
    }
    let handle = NEXT_HANDLE.fetch_add(1, Ordering::Relaxed);
    let result = PairResult {
        ok: true,
        handle: Some(handle),
        remote_fingerprint: Some(connection.remote_fingerprint.clone()),
        remote_device: Some(remote),
        error: None,
        role: Some(role.into()),
    };
    let reader_stream = connection.stream.try_clone().map_err(|error| error.to_string())?;
    // Encrypted heartbeats keep idle sessions alive and bound stale-peer detection.
    let connection = Arc::new(Mutex::new(connection));
    reader_stream.set_read_timeout(Some(SESSION_READ_TIMEOUT)).map_err(|error| error.to_string())?;
    let (sender, incoming) = mpsc::sync_channel(8);
    let stop = Arc::new(AtomicBool::new(false));
    let reader_connection = Arc::clone(&connection);
    let reader_stop = Arc::clone(&stop);
    let reader_connection_for_thread = Arc::clone(&reader_connection);
    thread::Builder::new().name(format!("bridge-session-reader-{handle}")).spawn(move || {
        while !reader_stop.load(Ordering::Acquire) {
            let encrypted = match bridge_network::read_frame(&mut &reader_stream) { Ok(frame) => frame, Err(_) => break };
            let plaintext = {
                let mut guard = match reader_connection.lock() { Ok(guard) => guard, Err(_) => break };
                let mut plaintext = vec![0_u8; MAX_APPLICATION_BYTES];
                match guard.transport.read_message(&encrypted, &mut plaintext) {
                    Ok(length) => { plaintext.truncate(length); plaintext },
                    Err(_) => break,
                }
            };
            if plaintext == HEARTBEAT_PING {
                let mut guard = match reader_connection.lock() { Ok(guard) => guard, Err(_) => break };
                let mut response = vec![0_u8; 65_535];
                let length = match guard.transport.write_message(HEARTBEAT_PONG, &mut response) {
                    Ok(length) => length, Err(_) => break,
                };
                if bridge_network::write_frame(&mut guard.stream, &response[..length]).is_err() { break; }
                continue;
            }
            if plaintext == HEARTBEAT_PONG { continue; }
            if sender.send(plaintext).is_err() { break; }
        }
        reader_stop.store(true, Ordering::Release);
        if let Ok(guard) = reader_connection_for_thread.lock() {
            let _ = guard.stream.shutdown(std::net::Shutdown::Both);
        }
    }).map_err(|error| error.to_string())?;

    let heartbeat_connection = Arc::clone(&connection);
    let heartbeat_stop = Arc::clone(&stop);
    thread::Builder::new().name(format!("bridge-session-heartbeat-{handle}")).spawn(move || {
        while !heartbeat_stop.load(Ordering::Acquire) {
            thread::sleep(HEARTBEAT_INTERVAL);
            if heartbeat_stop.load(Ordering::Acquire) { break; }
            let mut guard = match heartbeat_connection.lock() { Ok(guard) => guard, Err(_) => break };
            let mut encrypted = vec![0_u8; 65_535];
            let length = match guard.transport.write_message(HEARTBEAT_PING, &mut encrypted) {
                Ok(length) => length,
                Err(_) => {
                    heartbeat_stop.store(true, Ordering::Release);
                    let _ = guard.stream.shutdown(std::net::Shutdown::Both);
                    break;
                }
            };
            if bridge_network::write_frame(&mut guard.stream, &encrypted[..length]).is_err() {
                heartbeat_stop.store(true, Ordering::Release);
                let _ = guard.stream.shutdown(std::net::Shutdown::Both);
                break;
            }
        }
    }).map_err(|error| error.to_string())?;
    registry.insert(handle, Arc::new(SessionEntry { connection, incoming: Mutex::new(incoming), stop }));
    Ok(result)
}

fn pair_connect(request: PairRequest) -> Result<PairResult, String> {
    request.local_device.validate().map_err(|e: bridge_protocol::ProtocolValidationError| e.to_string())?;
    if request.port == 0 { return Err("peer port must be non-zero".into()); }
    let key = decode_hex(&request.private_key_hex)?;
    let host = request.host.ok_or("missing peer host")?;
    let address = (host.as_str(), request.port).to_socket_addrs()
        .map_err(|e: std::io::Error| e.to_string())?
        .next().ok_or("peer host did not resolve")?;
    let stream = TcpStream::connect_timeout(&address, Duration::from_secs(5))
        .map_err(|e: std::io::Error| e.to_string())?;
    let connection = match connect_noise_xx(stream, &key, request.expected_fingerprint.as_deref()) {
        Ok(connection) => connection,
        Err(error) => return Err(error.to_string()),
    };
    register(connection, &request.local_device, request.expected_device_id.as_ref(), "initiator")
}

fn pair_accept(request: PairRequest) -> Result<PairResult, String> {
    request.local_device.validate().map_err(|e: bridge_protocol::ProtocolValidationError| e.to_string())?;
    if request.port == 0 { return Err("listener port must be non-zero".into()); }
    let key = decode_hex(&request.private_key_hex)?;
    let ip = request.bind_ip.unwrap_or_else(|| "0.0.0.0".into());
    let address: SocketAddr = format!("{ip}:{}", request.port).parse().map_err(|e: std::net::AddrParseError| e.to_string())?;
    let listener = TcpListener::bind(address).map_err(|e: std::io::Error| e.to_string())?;
    listener.set_nonblocking(true).map_err(|e: std::io::Error| e.to_string())?;
    let timeout = Duration::from_millis(request.timeout_ms.unwrap_or(1_000).clamp(250, 30_000));
    let started = Instant::now();
    let stream = loop {
        match listener.accept() {
            Ok((stream, _)) => break stream,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock && started.elapsed() < timeout => {
                thread::sleep(Duration::from_millis(75));
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                return Err("pairing listener timed out".into());
            }
            Err(error) => return Err(error.to_string()),
        }
    };
    let handshake: Result<NoiseConnection, bridge_network::NetworkError> =
        accept_noise_xx(stream, &key, request.expected_fingerprint.as_deref());
    let connection = match handshake {
        Ok(connection) => connection,
        Err(error) => return Err(format!("{error}")),
    };
    register(connection, &request.local_device, None, "responder")
}

/// Derive the SHA-256 fingerprint for a 32-byte Noise private key encoded as hex.
///
/// # Safety
/// `private_key_hex` must be null or point to a valid NUL-terminated UTF-8 C string.
/// The returned allocation must be released with `bridge_string_free`.
#[no_mangle]
pub unsafe extern "C" fn bridge_identity_fingerprint(private_key_hex: *const c_char) -> *mut c_char {
    if private_key_hex.is_null() { return CString::new("{\"ok\":false,\"error\":\"null key\"}").unwrap().into_raw(); }
    let result: Result<String, String> = CStr::from_ptr(private_key_hex).to_str()
        .map_err(|e: std::str::Utf8Error| e.to_string())
        .and_then(decode_hex)
        .and_then(|key| bridge_network::local_key_fingerprint(&key).map_err(|e: bridge_network::NetworkError| e.to_string()));
    let json = match result {
        Ok(fingerprint) => serde_json::json!({"ok": true, "fingerprint": fingerprint}),
        Err(error) => serde_json::json!({"ok": false, "error": error}),
    };
    CString::new(json.to_string()).unwrap().into_raw()
}

fn to_json(result: Result<PairResult, String>) -> CString {
    let value = result.unwrap_or_else(result_error);
    CString::new(serde_json::to_string(&value).unwrap_or_else(|_| "{\"ok\":false,\"error\":\"serialization failed\"}".into()))
        .unwrap_or_else(|_| CString::new("{\"ok\":false,\"error\":\"invalid response\"}").unwrap())
}

/// Derive the raw Ed25519 public key from a 32-byte attestation seed encoded as hex.
/// The private seed is never returned or persisted by this function.
///
/// # Safety
/// `seed_hex` must be null or point to a valid NUL-terminated UTF-8 C string.
/// The returned allocation must be released with `bridge_string_free`.
#[no_mangle]
pub unsafe extern "C" fn bridge_attestation_public_key(seed_hex: *const c_char) -> *mut c_char {
    if seed_hex.is_null() {
        return CString::new("{\\"ok\\":false,\\"error\\":\\"null seed\\"}").unwrap().into_raw();
    }
    let result: Result<String, String> = CStr::from_ptr(seed_hex).to_str()
        .map_err(|error: std::str::Utf8Error| error.to_string())
        .and_then(decode_hex)
        .and_then(|seed| bridge_security::peer_attestation::attestation_public_key(&seed)
            .map_err(|error| error.to_string()))
        .map(|key| key.iter().map(|byte| format!("{byte:02x}")).collect());
    let json = match result {
        Ok(public_key) => serde_json::json!({"ok": true, "public_key": public_key}),
        Err(error) => serde_json::json!({"ok": false, "error": error}),
    };
    CString::new(json.to_string()).unwrap().into_raw()
}

/// Establish an authenticated Noise XX session to the requested peer.
///
/// # Safety
/// `request_json` must be null or point to a valid NUL-terminated UTF-8 JSON C string.
/// The returned allocation must be released with `bridge_string_free`.
#[no_mangle]
pub unsafe extern "C" fn bridge_pair_connect(request_json: *const c_char) -> *mut c_char {
    if request_json.is_null() { return to_json(Err("null request".into())).into_raw(); }
    let request = match CStr::from_ptr(request_json).to_str()
        .map_err(|e: std::str::Utf8Error| e.to_string())
        .and_then(|s| serde_json::from_str::<PairRequest>(s).map_err(|e: serde_json::Error| e.to_string())) {
        Ok(request) => request,
        Err(error) => return to_json(Err(error)).into_raw(),
    };
    to_json(pair_connect(request)).into_raw()
}

/// Listen for and establish one authenticated Noise XX session.
///
/// # Safety
/// `request_json` must be null or point to a valid NUL-terminated UTF-8 JSON C string.
/// The returned allocation must be released with `bridge_string_free`.
#[no_mangle]
pub unsafe extern "C" fn bridge_pair_accept(request_json: *const c_char) -> *mut c_char {
    if request_json.is_null() { return to_json(Err("null request".into())).into_raw(); }
    let request = match CStr::from_ptr(request_json).to_str()
        .map_err(|e: std::str::Utf8Error| e.to_string())
        .and_then(|s| serde_json::from_str::<PairRequest>(s).map_err(|e: serde_json::Error| e.to_string())) {
        Ok(request) => request,
        Err(error) => return to_json(Err(error)).into_raw(),
    };
    to_json(pair_accept(request)).into_raw()
}


const MAX_APPLICATION_BYTES: usize = 65_519;

fn session_send_payload(handle: u64, message: &[u8]) -> Result<(), String> {
    if message.is_empty() || message.len() > MAX_APPLICATION_BYTES {
        return Err(format!("application message length must be 1..={MAX_APPLICATION_BYTES} bytes"));
    }
    let session = sessions().lock()
        .map_err(|_| "session registry poisoned".to_string())
        .and_then(|map| map.get(&handle).cloned().ok_or_else(|| "session not found".to_string()))?;
    let mut connection = session.connection.lock().map_err(|_| "session lock poisoned".to_string())?;
    let mut encrypted = vec![0_u8; 65_535];
    let length = connection.transport.write_message(message, &mut encrypted)
        .map_err(|e: snow::Error| e.to_string())?;
    bridge_network::write_frame(&mut connection.stream, &encrypted[..length])
        .map_err(|e: bridge_network::NetworkError| e.to_string())
}

fn session_receive_payload(handle: u64) -> Result<Vec<u8>, String> {
    let session = sessions().lock()
        .map_err(|_| "session registry poisoned".to_string())
        .and_then(|map| map.get(&handle).cloned().ok_or_else(|| "session not found".to_string()))?;
    let receiver = session.incoming.lock().map_err(|_| "session receive queue poisoned".to_string())?;
    receiver.recv_timeout(Duration::from_secs(120)).map_err(|error| match error {
        mpsc::RecvTimeoutError::Timeout => "session receive timed out".to_string(),
        mpsc::RecvTimeoutError::Disconnected => "session reader disconnected".to_string(),
    })
}

/// Encrypt and send one UTF-8 application message on an established session.
///
/// # Safety
/// `message` must be null or point to a valid NUL-terminated UTF-8 C string.
/// The returned allocation must be released with `bridge_string_free`.
#[no_mangle]
pub unsafe extern "C" fn bridge_session_send(handle: u64, message: *const c_char) -> *mut c_char {
    if message.is_null() { return to_json(Err("null message".into())).into_raw(); }
    let message = match CStr::from_ptr(message).to_str() {
        Ok(value) => value.as_bytes(),
        Err(error) => return to_json(Err(error.to_string())).into_raw(),
    };
    let result = session_send_payload(handle, message);
    let response = result.map(|()| PairResult {
        ok: true, handle: Some(handle), remote_fingerprint: None,
        remote_device: None, error: None, role: None,
    });
    to_json(response).into_raw()
}

/// Encrypt and send one bounded binary application message on an established session.
///
/// # Safety
/// `data` must point to `length` readable bytes, or be null only when rejected.
/// The returned allocation must be released with `bridge_string_free`.
#[no_mangle]
pub unsafe extern "C" fn bridge_session_send_bytes(handle: u64, data: *const u8, length: usize) -> *mut c_char {
    if data.is_null() { return to_json(Err("null byte payload".into())).into_raw(); }
    if length == 0 || length > MAX_APPLICATION_BYTES {
        return to_json(Err(format!("application message length must be 1..={MAX_APPLICATION_BYTES} bytes"))).into_raw();
    }
    let result = session_send_payload(handle, std::slice::from_raw_parts(data, length));
    let response = result.map(|()| PairResult {
        ok: true, handle: Some(handle), remote_fingerprint: None,
        remote_device: None, error: None, role: None,
    });
    to_json(response).into_raw()
}

#[no_mangle]
pub extern "C" fn bridge_session_receive(handle: u64) -> *mut c_char {
    let result = session_receive_payload(handle).and_then(|bytes| {
        String::from_utf8(bytes).map_err(|e| e.to_string())
    });
    let json = serde_json::json!({"ok": result.is_ok(), "message": result.as_ref().ok(), "error": result.err()});
    CString::new(json.to_string()).unwrap().into_raw()
}


#[no_mangle]
pub extern "C" fn bridge_session_close(handle: u64) {
    let entry = sessions().lock().ok().and_then(|mut map| map.remove(&handle));
    if let Some(entry) = entry {
        entry.stop.store(true, Ordering::Release);
        if let Ok(connection) = entry.connection.lock() { let _ = connection.stream.shutdown(std::net::Shutdown::Both); }
    }
}

/// Release a string allocated by another Bridge FFI function.
///
/// # Safety
/// `value` must be null or a pointer previously returned by this library and not yet freed.
#[no_mangle]
pub unsafe extern "C" fn bridge_string_free(value: *mut c_char) {
    if !value.is_null() { drop(CString::from_raw(value)); }
}

#[cfg(target_os = "android")]
mod android_jni {
    use super::*;
    use jni::{objects::{JClass, JString}, sys::jstring, JNIEnv};

    fn invoke(mut env: JNIEnv, request: JString, accept: bool) -> jstring {
        let text: String = match env.get_string(&request) {
            Ok(value) => value.into(),
            Err(error) => format!("{{\"ok\":false,\"error\":{:?}}}", error.to_string()),
        };
        let input = CString::new(text).unwrap_or_default();
        let output = unsafe {
            if accept { bridge_pair_accept(input.as_ptr()) } else { bridge_pair_connect(input.as_ptr()) }
        };
        let response = unsafe { CStr::from_ptr(output).to_string_lossy().into_owned() };
        unsafe { bridge_string_free(output); }
        env.new_string(response).map(|value| value.into_raw()).unwrap_or(std::ptr::null_mut())
    }

    #[no_mangle]
    pub extern "system" fn Java_app_bridge_android_BridgeNative_identityFingerprint(
        mut env: JNIEnv, _class: JClass, private_key_hex: JString
    ) -> jstring {
        let text: String = env.get_string(&private_key_hex).map(|v| v.into()).unwrap_or_default();
        let input = CString::new(text).unwrap_or_default();
        let output = unsafe { bridge_identity_fingerprint(input.as_ptr()) };
        let response = unsafe { CStr::from_ptr(output).to_string_lossy().into_owned() };
        unsafe { bridge_string_free(output); }
        env.new_string(response).map(|value| value.into_raw()).unwrap_or(std::ptr::null_mut())
    }

    #[no_mangle]
    pub extern "system" fn Java_app_bridge_android_BridgeNative_attestationPublicKey(
        mut env: JNIEnv, _class: JClass, seed_hex: JString
    ) -> jstring {
        let text: String = env.get_string(&seed_hex).map(|v| v.into()).unwrap_or_default();
        let input = CString::new(text).unwrap_or_default();
        let output = unsafe { bridge_attestation_public_key(input.as_ptr()) };
        let response = unsafe { CStr::from_ptr(output).to_string_lossy().into_owned() };
        unsafe { bridge_string_free(output); }
        env.new_string(response).map(|value| value.into_raw()).unwrap_or(std::ptr::null_mut())
    }

    #[no_mangle]
    pub extern "system" fn Java_app_bridge_android_BridgeNative_pairConnect(
        env: JNIEnv, _class: JClass, request: JString
    ) -> jstring { invoke(env, request, false) }

    #[no_mangle]
    pub extern "system" fn Java_app_bridge_android_BridgeNative_pairAccept(
        env: JNIEnv, _class: JClass, request: JString
    ) -> jstring { invoke(env, request, true) }


    #[no_mangle]
    pub extern "system" fn Java_app_bridge_android_BridgeNative_sessionSend(
        mut env: JNIEnv, _class: JClass, handle: jni::sys::jlong, message: JString
    ) -> jstring {
        let text: String = env.get_string(&message).map(|v| v.into()).unwrap_or_default();
        let input = CString::new(text).unwrap_or_default();
        let output = unsafe { bridge_session_send(handle as u64, input.as_ptr()) };
        let response = unsafe { CStr::from_ptr(output).to_string_lossy().into_owned() };
        unsafe { bridge_string_free(output); }
        env.new_string(response).map(|value| value.into_raw()).unwrap_or(std::ptr::null_mut())
    }

    #[no_mangle]
    pub extern "system" fn Java_app_bridge_android_BridgeNative_sessionReceive(
        env: JNIEnv, _class: JClass, handle: jni::sys::jlong
    ) -> jstring {
        let output = bridge_session_receive(handle as u64);
        let response = unsafe { CStr::from_ptr(output).to_string_lossy().into_owned() };
        unsafe { bridge_string_free(output); }
        env.new_string(response).map(|value| value.into_raw()).unwrap_or(std::ptr::null_mut())
    }

    #[no_mangle]
    pub extern "system" fn Java_app_bridge_android_BridgeNative_sessionSendBytes(
        mut env: JNIEnv, _class: JClass, handle: jni::sys::jlong, data: jni::objects::JByteArray
    ) -> jstring {
        let bytes = match env.convert_byte_array(&data) {
            Ok(bytes) => bytes,
            Err(error) => {
                let response = serde_json::json!({"ok": false, "error": error.to_string()}).to_string();
                return env.new_string(response).map(|value| value.into_raw()).unwrap_or(std::ptr::null_mut());
            }
        };
        let output = unsafe { bridge_session_send_bytes(handle as u64, bytes.as_ptr(), bytes.len()) };
        let response = unsafe { CStr::from_ptr(output).to_string_lossy().into_owned() };
        unsafe { bridge_string_free(output); }
        env.new_string(response).map(|value| value.into_raw()).unwrap_or(std::ptr::null_mut())
    }

    #[no_mangle]
    pub extern "system" fn Java_app_bridge_android_BridgeNative_sessionReceiveBytes(
        mut env: JNIEnv, _class: JClass, handle: jni::sys::jlong
    ) -> jni::sys::jbyteArray {
        match session_receive_payload(handle as u64) {
            Ok(bytes) => env.byte_array_from_slice(&bytes).map(|value| value.into_raw()).unwrap_or(std::ptr::null_mut()),
            // An idle but healthy session may have no application packet for a while.
            // Return an empty array for that timeout; reserve [0] for a closed reader.
            Err(error) if error == "session receive timed out" =>
                env.byte_array_from_slice(&[]).map(|value| value.into_raw()).unwrap_or(std::ptr::null_mut()),
            Err(_) => env.byte_array_from_slice(&[0_u8]).map(|value| value.into_raw()).unwrap_or(std::ptr::null_mut()),
        }
    }

    #[no_mangle]
    pub extern "system" fn Java_app_bridge_android_BridgeNative_sessionClose(
        _env: JNIEnv, _class: JClass, handle: jni::sys::jlong
    ) { bridge_session_close(handle as u64); }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bridge_protocol::{DeviceId, Platform, PROTOCOL_VERSION};
    use bridge_security::generate_static_keypair;
    use std::{ffi::{CStr, CString}, net::TcpListener, thread};

    fn request(
        host: Option<&str>,
        bind_ip: Option<&str>,
        port: u16,
        private_key: &[u8],
        device: DeviceDescriptor,
        expected_device_id: Option<DeviceId>,
    ) -> String {
        let value = serde_json::json!({
            "host": host,
            "bind_ip": bind_ip,
            "port": port,
            "timeout_ms": 5000,
            "private_key_hex": private_key.iter().map(|byte| format!("{byte:02x}")).collect::<String>(),
            "local_device": device,
            "expected_fingerprint": null,
            "expected_device_id": expected_device_id
        });
        value.to_string()
    }

    unsafe fn take_json(pointer: *mut c_char) -> serde_json::Value {
        let value = CStr::from_ptr(pointer).to_string_lossy().into_owned();
        bridge_string_free(pointer);
        serde_json::from_str(&value).unwrap()
    }

    #[test]
    fn native_ffi_pairing_and_encrypted_transport_round_trip() {
        let initiator_key = generate_static_keypair().unwrap();
        let responder_key = generate_static_keypair().unwrap();
        let initiator_device = DeviceDescriptor {
            id: DeviceId::new(), display_name: "Bridge iPhone".into(),
            platform: Platform::Ios, protocol_version: PROTOCOL_VERSION,
        };
        let responder_device = DeviceDescriptor {
            id: DeviceId::new(), display_name: "Bridge Android".into(),
            platform: Platform::Android, protocol_version: PROTOCOL_VERSION,
        };
        let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let responder_device_id = responder_device.id;
        let accept_request = request(None, Some("127.0.0.1"), port, &responder_key.private, responder_device, None);
        let responder = thread::spawn(move || {
            let input = CString::new(accept_request).unwrap();
            unsafe { take_json(bridge_pair_accept(input.as_ptr())) }
        });
        thread::sleep(Duration::from_millis(150));

        let connect_request = request(Some("127.0.0.1"), None, port, &initiator_key.private, initiator_device, Some(responder_device_id));
        let input = CString::new(connect_request).unwrap();
        let initiator = unsafe { take_json(bridge_pair_connect(input.as_ptr())) };
        let responder = responder.join().unwrap();

        assert_eq!(initiator["ok"], true);
        assert_eq!(responder["ok"], true);
        assert_eq!(initiator["role"], "initiator");
        assert_eq!(responder["role"], "responder");
        assert_eq!(initiator["remote_device"]["platform"], "android");
        assert_eq!(responder["remote_device"]["platform"], "ios");

        let initiator_handle = initiator["handle"].as_u64().unwrap();
        let responder_handle = responder["handle"].as_u64().unwrap();
        let decision = CString::new(r#"{"type":"pairing_decision","decision":"accepted"}"#).unwrap();
        // A blocking receive on one session must not hold the global registry
        // lock and prevent a different session from sending its response.
        let receiver = thread::spawn(move || unsafe {
            take_json(bridge_session_receive(initiator_handle))
        });
        thread::sleep(Duration::from_millis(50));
        let sent_decision = unsafe { take_json(bridge_session_send(responder_handle, decision.as_ptr())) };
        assert_eq!(sent_decision["ok"], true);
        let received_decision = receiver.join().unwrap();
        assert_eq!(received_decision["ok"], true);
        let parsed_decision: serde_json::Value = serde_json::from_str(received_decision["message"].as_str().unwrap()).unwrap();
        assert_eq!(parsed_decision["type"], "pairing_decision");
        assert_eq!(parsed_decision["decision"], "accepted");

        // The background reader must not hold the Noise session lock while
        // waiting for inbound data: a simultaneous outbound send must proceed.
        let waiting_receiver = thread::spawn(move || session_receive_payload(initiator_handle).unwrap());
        session_send_payload(initiator_handle, b"full-duplex request").unwrap();
        assert_eq!(session_receive_payload(responder_handle).unwrap(), b"full-duplex request");
        session_send_payload(responder_handle, b"full-duplex response").unwrap();
        assert_eq!(waiting_receiver.join().unwrap(), b"full-duplex response");
        let binary_payload = vec![0x00, 0xff, 0x42, 0x00, 0x80, 0x7f];
        session_send_payload(initiator_handle, &binary_payload).unwrap();
        assert_eq!(session_receive_payload(responder_handle).unwrap(), binary_payload);

        let message = CString::new("encrypted native bridge payload").unwrap();
        let sent = unsafe { take_json(bridge_session_send(initiator_handle, message.as_ptr())) };
        assert_eq!(sent["ok"], true);
        let received = unsafe { take_json(bridge_session_receive(responder_handle)) };
        assert_eq!(received["ok"], true);
        assert_eq!(received["message"], "encrypted native bridge payload");

        bridge_session_close(initiator_handle);
        bridge_session_close(responder_handle);
    }


}
