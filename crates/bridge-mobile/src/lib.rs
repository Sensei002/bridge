//! Stable C/JNI boundary for using the shared Noise XX implementation on mobile.
use std::{
    collections::HashMap,
    ffi::{CStr, CString},
    net::{SocketAddr, TcpListener, TcpStream, ToSocketAddrs},
    os::raw::c_char,
    sync::{Mutex, OnceLock, atomic::{AtomicU64, Ordering}},
    thread,
    time::{Duration, Instant},
};

use bridge_network::{accept_noise_xx, connect_noise_xx, NoiseConnection};
use bridge_protocol::DeviceDescriptor;
use serde::{Deserialize, Serialize};

static NEXT_HANDLE: AtomicU64 = AtomicU64::new(1);
static SESSIONS: OnceLock<Mutex<HashMap<u64, NoiseConnection>>> = OnceLock::new();

fn sessions() -> &'static Mutex<HashMap<u64, NoiseConnection>> {
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
    timeout_ms: Option<u64>,
}

#[derive(Serialize)]
struct PairResult {
    ok: bool,
    handle: Option<u64>,
    sas: Option<String>,
    remote_fingerprint: Option<String>,
    remote_device: Option<DeviceDescriptor>,
    error: Option<String>,
}

fn result_error(error: impl ToString) -> PairResult {
    PairResult {
        ok: false, handle: None, sas: None, remote_fingerprint: None,
        remote_device: None, error: Some(error.to_string()),
    }
}

fn decode_hex(value: &str) -> Result<Vec<u8>, String> {
    if value.len() != 64 || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("Noise private key must be 64 hexadecimal characters".into());
    }
    (0..value.len()).step_by(2)
        .map(|i| u8::from_str_radix(&value[i..i + 2], 16).map_err(|e| e.to_string()))
        .collect()
}

fn register(mut connection: NoiseConnection, local: &DeviceDescriptor) -> Result<PairResult, String> {
    let remote = connection.exchange_descriptors(local).map_err(|e| e.to_string())?;
    let handle = NEXT_HANDLE.fetch_add(1, Ordering::Relaxed);
    let result = PairResult {
        ok: true,
        handle: Some(handle),
        sas: Some(connection.sas.clone()),
        remote_fingerprint: Some(connection.remote_fingerprint.clone()),
        remote_device: Some(remote),
        error: None,
    };
    sessions().lock().map_err(|_| "session registry poisoned")?.insert(handle, connection);
    Ok(result)
}

fn pair_connect(request: PairRequest) -> Result<PairResult, String> {
    request.local_device.validate().map_err(|e| e.to_string())?;
    let key = decode_hex(&request.private_key_hex)?;
    let host = request.host.ok_or("missing peer host")?;
    let address = (host.as_str(), request.port).to_socket_addrs()
        .map_err(|e| e.to_string())?
        .next().ok_or("peer host did not resolve")?;
    let stream = TcpStream::connect_timeout(&address, Duration::from_secs(5))
        .map_err(|e| e.to_string())?;
    let connection = connect_noise_xx(stream, &key, request.expected_fingerprint.as_deref())
        .map_err(|e: bridge_network::NetworkError| e.to_string())?;
    register(connection, &request.local_device)
}

fn pair_accept(request: PairRequest) -> Result<PairResult, String> {
    request.local_device.validate().map_err(|e| e.to_string())?;
    let key = decode_hex(&request.private_key_hex)?;
    let ip = request.bind_ip.unwrap_or_else(|| "0.0.0.0".into());
    let address: SocketAddr = format!("{ip}:{}", request.port).parse().map_err(|e| e.to_string())?;
    let listener = TcpListener::bind(address).map_err(|e| e.to_string())?;
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
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
    let connection = accept_noise_xx(stream, &key, request.expected_fingerprint.as_deref())
        .map_err(|e| e.to_string())?;
    register(connection, &request.local_device)
}

#[no_mangle]
pub unsafe extern "C" fn bridge_identity_fingerprint(private_key_hex: *const c_char) -> *mut c_char {
    if private_key_hex.is_null() { return CString::new("{\"ok\":false,\"error\":\"null key\"}").unwrap().into_raw(); }
    let result: Result<String, String> = CStr::from_ptr(private_key_hex).to_str()
        .map_err(|e: std::str::Utf8Error| e.to_string())
        .and_then(decode_hex)
        .and_then(|key| bridge_network::local_key_fingerprint(&key).map_err(|e| e.to_string()));
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

#[no_mangle]
pub unsafe extern "C" fn bridge_pair_connect(request_json: *const c_char) -> *mut c_char {
    if request_json.is_null() { return to_json(Err("null request".into())).into_raw(); }
    let request = match CStr::from_ptr(request_json).to_str()
        .map_err(|e| e.to_string())
        .and_then(|s| serde_json::from_str::<PairRequest>(s).map_err(|e| e.to_string())) {
        Ok(request) => request,
        Err(error) => return to_json(Err(error)).into_raw(),
    };
    to_json(pair_connect(request)).into_raw()
}

#[no_mangle]
pub unsafe extern "C" fn bridge_pair_accept(request_json: *const c_char) -> *mut c_char {
    if request_json.is_null() { return to_json(Err("null request".into())).into_raw(); }
    let request = match CStr::from_ptr(request_json).to_str()
        .map_err(|e| e.to_string())
        .and_then(|s| serde_json::from_str::<PairRequest>(s).map_err(|e| e.to_string())) {
        Ok(request) => request,
        Err(error) => return to_json(Err(error)).into_raw(),
    };
    to_json(pair_accept(request)).into_raw()
}

#[no_mangle]
pub unsafe extern "C" fn bridge_session_send(handle: u64, message: *const c_char) -> *mut c_char {
    if message.is_null() { return to_json(Err("null message".into())).into_raw(); }
    let message = match CStr::from_ptr(message).to_str() {
        Ok(value) => value.as_bytes(),
        Err(error) => return to_json(Err(error.to_string())).into_raw(),
    };
    let result: Result<(), String> = sessions().lock().map_err(|_| "session registry poisoned".to_string())
        .and_then(|mut map| {
            let connection = map.get_mut(&handle).ok_or_else(|| "session not found".to_string())?;
            let mut encrypted = vec![0_u8; 65_535];
            let length = connection.transport.write_message(message, &mut encrypted)
                .map_err(|e| e.to_string())?;
            bridge_network::write_frame(&mut connection.stream, &encrypted[..length])
                .map_err(|e| e.to_string())
        });
    let response = result.map(|()| PairResult {
        ok: true, handle: Some(handle), sas: None, remote_fingerprint: None,
        remote_device: None, error: None,
    });
    to_json(response).into_raw()
}

#[no_mangle]
pub unsafe extern "C" fn bridge_session_receive(handle: u64) -> *mut c_char {
    let result: Result<String, String> = sessions().lock().map_err(|_| "session registry poisoned".to_string())
        .and_then(|mut map| {
            let connection = map.get_mut(&handle).ok_or_else(|| "session not found".to_string())?;
            let encrypted = bridge_network::read_frame(&mut connection.stream)
                .map_err(|e| e.to_string())?;
            let mut plaintext = vec![0_u8; 65_535];
            let length = connection.transport.read_message(&encrypted, &mut plaintext)
                .map_err(|e| e.to_string())?;
            String::from_utf8(plaintext[..length].to_vec()).map_err(|e| e.to_string())
        });
    let json = serde_json::json!({"ok": result.is_ok(), "message": result.as_ref().ok(), "error": result.err()});
    CString::new(json.to_string()).unwrap().into_raw()
}

#[no_mangle]
pub extern "C" fn bridge_session_close(handle: u64) {
    if let Ok(mut map) = sessions().lock() { map.remove(&handle); }
}

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
        let output = unsafe { bridge_session_receive(handle as u64) };
        let response = unsafe { CStr::from_ptr(output).to_string_lossy().into_owned() };
        unsafe { bridge_string_free(output); }
        env.new_string(response).map(|value| value.into_raw()).unwrap_or(std::ptr::null_mut())
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
    ) -> String {
        let value = serde_json::json!({
            "host": host,
            "bind_ip": bind_ip,
            "port": port,
            "timeout_ms": 5000,
            "private_key_hex": private_key.iter().map(|byte| format!("{byte:02x}")).collect::<String>(),
            "local_device": device,
            "expected_fingerprint": null
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
        let accept_request = request(None, Some("127.0.0.1"), port, &responder_key.private, responder_device);
        let responder = thread::spawn(move || {
            let input = CString::new(accept_request).unwrap();
            unsafe { take_json(bridge_pair_accept(input.as_ptr())) }
        });
        thread::sleep(Duration::from_millis(150));

        let connect_request = request(Some("127.0.0.1"), None, port, &initiator_key.private, initiator_device);
        let input = CString::new(connect_request).unwrap();
        let initiator = unsafe { take_json(bridge_pair_connect(input.as_ptr())) };
        let responder = responder.join().unwrap();

        assert_eq!(initiator["ok"], true);
        assert_eq!(responder["ok"], true);
        assert_eq!(initiator["sas"], responder["sas"]);
        assert_eq!(initiator["remote_device"]["platform"], "android");
        assert_eq!(responder["remote_device"]["platform"], "ios");

        let initiator_handle = initiator["handle"].as_u64().unwrap();
        let responder_handle = responder["handle"].as_u64().unwrap();
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
