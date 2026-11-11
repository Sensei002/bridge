#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(windows)]
mod display_service;
#[cfg(windows)]
mod identity_store;
#[cfg(windows)]
mod updater;

use eframe::egui;
#[cfg(windows)]
use updater::{UpdateEvent, check_for_update};
#[cfg(windows)]
use bridge_network::{accept_noise_xx, connect_noise_xx, connect_to_peer, local_key_fingerprint, peer_from_event, DiscoveredPeer, DiscoveryRecord, LocalDiscovery, NoiseConnection};
#[cfg(windows)]
use bridge_core::trust::{SasConfirmation, TrustStore};
#[cfg(windows)]
use bridge_protocol::{DeviceDescriptor, DeviceId, Platform, PROTOCOL_VERSION};
#[cfg(windows)]
use bridge_transfer::{decode_packet, encode_packet, fragment_display_frame, offer_for_file, DisplayFrame, DisplayInputAction, DisplayInputEvent, FileOffer, IncomingFile, OutgoingFile, TransferControl, TransferPacket};
#[cfg(windows)]
use std::net::TcpListener;
#[cfg(windows)]
use std::path::PathBuf;
#[cfg(windows)]
use uuid::Uuid;
use std::sync::Arc;
#[cfg(windows)]
use std::sync::Mutex;
#[cfg(windows)]
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver};
#[cfg(windows)]
use std::sync::mpsc::Sender;
#[cfg(windows)]
use std::thread;
use std::time::Duration;
#[cfg(windows)]
use std::time::Instant;

const BG: egui::Color32 = egui::Color32::from_rgb(10, 11, 15);
const SIDEBAR: egui::Color32 = egui::Color32::from_rgb(13, 15, 19);
const PANEL: egui::Color32 = egui::Color32::from_rgb(20, 24, 29);
const PANEL_ALT: egui::Color32 = egui::Color32::from_rgb(25, 30, 35);
const BORDER: egui::Color32 = egui::Color32::from_rgb(43, 50, 56);
const ACCENT: egui::Color32 = egui::Color32::from_rgb(34, 211, 238);
const AMBER: egui::Color32 = egui::Color32::from_rgb(251, 191, 36);
const TEXT: egui::Color32 = egui::Color32::from_rgb(239, 242, 235);
const MUTED: egui::Color32 = egui::Color32::from_rgb(145, 154, 160);
const GREEN: egui::Color32 = egui::Color32::from_rgb(119, 211, 151);
#[cfg(windows)]
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5);
#[cfg(windows)]
const HEARTBEAT_PING: &[u8] = b"\0BRIDGE_HEARTBEAT_PING_V1";
#[cfg(windows)]
const HEARTBEAT_PONG: &[u8] = b"\0BRIDGE_HEARTBEAT_PONG_V1";
#[cfg(windows)]
const CLIPBOARD_PACKET_TAG: u8 = 0x03;
#[cfg(windows)]
const MAX_CLIPBOARD_TEXT_BYTES: usize = 16 * 1024;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page { Overview, Devices, Transfers, Clipboard, Tools, Settings }

impl Page {
    fn title(self) -> &'static str {
        match self {
            Self::Overview => "Overview",
            Self::Devices => "Devices",
            Self::Transfers => "Transfers",
            Self::Clipboard => "Clipboard",
            Self::Tools => "Device tools",
            Self::Settings => "Settings",
        }
    }
    fn subtitle(self) -> &'static str {
        match self {
            Self::Overview => "Your private space for all your devices.",
            Self::Devices => "Find and manage the devices you trust.",
            Self::Transfers => "Move files between your devices, privately.",
            Self::Clipboard => "Your clipboard, available across your ecosystem.",
            Self::Tools => "Turn your devices into useful extensions of each other.",
            Self::Settings => "Make Bridge work the way you want.",
        }
    }
}

#[cfg(windows)]
fn validate_peer_attestation_descriptor(descriptor: &DeviceDescriptor) -> Result<(), String> {
    if descriptor.platform == Platform::Android {
        match descriptor.attestation_public_key.as_deref() {
            Some(key) if key.len() == 64 && key.bytes().all(|byte| byte.is_ascii_hexdigit()) => Ok(()),
            _ => Err("Android peer did not provide a valid attestation public key".into()),
        }
    } else {
        Ok(())
    }
}

#[cfg(windows)]
fn decode_attestation_hex(value: &str, expected_bytes: usize) -> Result<Vec<u8>, String> {
    if value.len() != expected_bytes * 2 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("attestation field must be {} hexadecimal characters", expected_bytes * 2));
    }
    (0..value.len()).step_by(2)
        .map(|index| u8::from_str_radix(&value[index..index + 2], 16)
            .map_err(|error| error.to_string()))
        .collect()
}

#[cfg(windows)]
struct PendingApproval {
    connection: NoiseConnection,
    descriptor: DeviceDescriptor,
    already_trusted: bool,
    /// Incoming sessions need an approval acknowledgement when trust already exists.
    incoming: bool,
}

#[cfg(windows)]
struct AndroidAttestationResponse {
    request_id: Uuid,
    session_id: Uuid,
    nonce: String,
    device_fingerprint: String,
    signature: String,
}

#[cfg(windows)]
struct VerifiedAndroidAttestation {
    nonce: String,
    session_id: Uuid,
    device_fingerprint: String,
    signature: String,
}

#[cfg(windows)]
struct DesktopSession {
    connection: Arc<std::sync::Mutex<NoiseConnection>>,
    incoming: std::sync::Mutex<Receiver<Vec<u8>>>,
    incoming_offers: std::sync::Mutex<Receiver<FileOffer>>,
    clipboard_incoming: std::sync::Mutex<Receiver<String>>,
    challenge_responses: std::sync::Mutex<Receiver<AndroidAttestationResponse>>,
    display_inputs: std::sync::Mutex<Receiver<DisplayInputEvent>>,
    session_id: Uuid,
    attestation_public_key: Option<String>,
    stop: Arc<std::sync::atomic::AtomicBool>,
    alive: Arc<std::sync::atomic::AtomicBool>,
    pub remote_fingerprint: String,
}

#[cfg(windows)]
impl DesktopSession {
    fn start(connection: NoiseConnection, attestation_public_key: Option<String>) -> Result<Arc<Self>, String> {
        let reader_stream = connection.stream.try_clone().map_err(|error| error.to_string())?;
        reader_stream.set_read_timeout(Some(Duration::from_secs(15))).map_err(|error| error.to_string())?;
        let remote_fingerprint = connection.remote_fingerprint.clone();
        let session_id = Uuid::new_v4();
        let connection = Arc::new(std::sync::Mutex::new(connection));
        let reader_connection = Arc::clone(&connection);
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let reader_stop = Arc::clone(&stop);
        let alive = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let reader_alive = Arc::clone(&alive);
        let (sender, incoming) = mpsc::sync_channel(8);
        let (offer_sender, incoming_offers) = mpsc::channel();
        let (clipboard_sender, clipboard_incoming) = mpsc::sync_channel(4);
        let (challenge_sender, challenge_responses) = mpsc::sync_channel(8);
        let (display_input_sender, display_inputs) = mpsc::sync_channel(256);
        let reader_connection_for_thread = Arc::clone(&reader_connection);
        thread::Builder::new()
            .name("bridge-desktop-session-reader".into())
            .spawn(move || {
                while !reader_stop.load(Ordering::Acquire) {
                    let encrypted = match bridge_network::read_frame(&mut &reader_stream) {
                        Ok(frame) => frame,
                        Err(_) => break,
                    };
                    let plaintext = {
                        let mut guard = match reader_connection.lock() {
                            Ok(guard) => guard,
                            Err(_) => break,
                        };
                        let mut plaintext = vec![0_u8; bridge_network::MAX_FRAME_BYTES];
                        match guard.transport.read_message(&encrypted, &mut plaintext) {
                            Ok(length) => { plaintext.truncate(length); plaintext },
                            Err(_) => break,
                        }
                    };
                    if plaintext == HEARTBEAT_PING {
                        let mut guard = match reader_connection.lock() {
                            Ok(guard) => guard,
                            Err(_) => break,
                        };
                        let mut response = vec![0_u8; bridge_network::MAX_FRAME_BYTES];
                        let length = match guard.transport.write_message(HEARTBEAT_PONG, &mut response) {
                            Ok(length) => length,
                            Err(_) => break,
                        };
                        if bridge_network::write_frame(&mut guard.stream, &response[..length]).is_err() { break; }
                        continue;
                    }
                    if plaintext == HEARTBEAT_PONG { continue; }
                    if plaintext.first() == Some(&CLIPBOARD_PACKET_TAG) {
                        let text = &plaintext[1..];
                        if text.len() <= MAX_CLIPBOARD_TEXT_BYTES {
                            if let Ok(text) = String::from_utf8(text.to_vec()) {
                                let _ = clipboard_sender.try_send(text);
                            }
                        }
                        continue;
                    }
                    match decode_packet(&plaintext) {
                        Ok(TransferPacket::Control(TransferControl::FileOffer { offer })) => {
                            if offer_sender.send(offer).is_err() { break; }
                        }
                        Ok(TransferPacket::Control(TransferControl::DisplayChallengeResponse {
                            request_id, session_id, nonce, device_fingerprint, signature,
                        })) => {
                            let response = AndroidAttestationResponse {
                                request_id, session_id, nonce, device_fingerprint, signature,
                            };
                            if let Err(mpsc::TrySendError::Disconnected(_)) = challenge_sender.try_send(response) { break; }
                        }
                        Ok(TransferPacket::DisplayInput(event)) => {
                            // Preserve terminal releases even when a burst fills the bounded queue.
                            let terminal = matches!(event.action, DisplayInputAction::Up | DisplayInputAction::Cancel);
                            if terminal {
                                if display_input_sender.send(event).is_err() { break; }
                            } else {
                                let _ = display_input_sender.try_send(event);
                            }
                        }
                        _ => {
                            if sender.send(plaintext).is_err() { break; }
                        }
                    }
                }
                reader_stop.store(true, Ordering::Release);
                reader_alive.store(false, Ordering::Release);
                if let Ok(guard) = reader_connection_for_thread.lock() {
                    let _ = guard.stream.shutdown(std::net::Shutdown::Both);
                }
            })
            .map_err(|error| error.to_string())?;

        let heartbeat_connection = Arc::clone(&connection);
        let heartbeat_stop = Arc::clone(&stop);
        let heartbeat_alive = Arc::clone(&alive);
        thread::Builder::new()
            .name("bridge-desktop-session-heartbeat".into())
            .spawn(move || {
                while !heartbeat_stop.load(Ordering::Acquire) {
                    thread::sleep(HEARTBEAT_INTERVAL);
                    if heartbeat_stop.load(Ordering::Acquire) { break; }
                    let mut guard = match heartbeat_connection.lock() {
                        Ok(guard) => guard,
                        Err(_) => break,
                    };
                    let mut encrypted = vec![0_u8; bridge_network::MAX_FRAME_BYTES];
                    let length = match guard.transport.write_message(HEARTBEAT_PING, &mut encrypted) {
                        Ok(length) => length,
                        Err(_) => {
                            heartbeat_alive.store(false, Ordering::Release);
                            heartbeat_stop.store(true, Ordering::Release);
                            let _ = guard.stream.shutdown(std::net::Shutdown::Both);
                            break;
                        }
                    };
                    if bridge_network::write_frame(&mut guard.stream, &encrypted[..length]).is_err() {
                        heartbeat_alive.store(false, Ordering::Release);
                        heartbeat_stop.store(true, Ordering::Release);
                        let _ = guard.stream.shutdown(std::net::Shutdown::Both);
                        break;
                    }
                }
            })
            .map_err(|error| error.to_string())?;
        Ok(Arc::new(Self { connection, incoming: std::sync::Mutex::new(incoming), incoming_offers: std::sync::Mutex::new(incoming_offers), clipboard_incoming: std::sync::Mutex::new(clipboard_incoming), challenge_responses: std::sync::Mutex::new(challenge_responses), display_inputs: std::sync::Mutex::new(display_inputs), session_id, attestation_public_key, stop, alive, remote_fingerprint }))
    }

    fn is_alive(&self) -> bool {
        self.alive.load(Ordering::Acquire)
    }

    fn shutdown(&self) {
        self.alive.store(false, Ordering::Release);
        self.stop.store(true, Ordering::Release);
        if let Ok(connection) = self.connection.lock() {
            let _ = connection.stream.shutdown(std::net::Shutdown::Both);
        }
    }

    fn send(&self, payload: &[u8]) -> Result<(), String> {
        if !self.is_alive() {
            return Err("The connected device session has ended. Reconnect before transferring.".into());
        }
        if payload.is_empty() || payload.len() > bridge_network::MAX_FRAME_BYTES - 16 {
            return Err("session payload size is invalid".into());
        }
        let mut guard = self.connection.lock().map_err(|_| "session connection lock poisoned")?;
        let mut encrypted = vec![0_u8; bridge_network::MAX_FRAME_BYTES];
        let length = guard.transport.write_message(payload, &mut encrypted).map_err(|error| error.to_string())?;
        let result = bridge_network::write_frame(&mut guard.stream, &encrypted[..length])
            .map_err(|error| error.to_string());
        if result.is_err() {
            self.alive.store(false, Ordering::Release);
        }
        result
    }

    /// Request a signed Android proof over this live authenticated Noise session.
    /// This validates the response context and shape only; the Windows service must
    /// independently verify the Ed25519 signature against its pinned pairing key.
    fn relay_service_challenge(&self, nonce: &str, timeout: Duration) -> Result<VerifiedAndroidAttestation, String> {
        if nonce.len() != 64 || !nonce.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err("display-service challenge must be 64 hexadecimal characters".into());
        }
        if timeout.is_zero() || timeout > Duration::from_secs(10) {
            return Err("display challenge timeout must be between 1ms and 10s".into());
        }
        let request_id = Uuid::new_v4();
        let packet = encode_packet(&TransferPacket::Control(
            TransferControl::DisplayChallengeRequest {
                request_id,
                session_id: self.session_id,
                nonce: nonce.to_ascii_lowercase(),
            },
        )).map_err(|error| error.to_string())?;
        self.send(&packet)?;
        let deadline = Instant::now() + timeout;
        let responses = self.challenge_responses.lock()
            .map_err(|_| "display challenge response lock poisoned")?;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err("Android signed attestation response timed out".into());
            }
            let response = responses.recv_timeout(remaining)
                .map_err(|error| format!("Android signed attestation unavailable: {error}"))?;
            if response.request_id != request_id { continue; }
            let pinned_key = self.attestation_public_key.as_deref()
                .ok_or_else(|| "trusted Android pairing record has no pinned attestation key".to_string())?;
            let public_key = decode_attestation_hex(pinned_key, 32)?;
            let proof_nonce = decode_attestation_hex(&response.nonce, 32)?;
            let signature = decode_attestation_hex(&response.signature, 64)?;
            let proof = bridge_security::peer_attestation::AndroidPeerAttestation {
                nonce: proof_nonce.as_slice().try_into().map_err(|_| "invalid attestation nonce length")?,
                session_id: response.session_id,
                device_fingerprint: response.device_fingerprint.clone(),
                signature: signature.as_slice().try_into().map_err(|_| "invalid attestation signature length")?,
            };
            bridge_security::peer_attestation::verify_android_peer_attestation(
                &public_key,
                &proof,
                &decode_attestation_hex(nonce, 32)?,
                self.session_id,
                &self.remote_fingerprint,
            ).map_err(|error| format!("Android attestation signature verification failed: {error}"))?;
            return Ok(VerifiedAndroidAttestation {
                nonce: response.nonce,
                session_id: response.session_id,
                device_fingerprint: response.device_fingerprint,
                signature: response.signature,
            });
        }
    }

    fn receive_timeout(&self, timeout: Duration) -> Result<Vec<u8>, String> {
        self.incoming.lock().map_err(|_| "session receiver lock poisoned")?
            .recv_timeout(timeout).map_err(|error| error.to_string())
    }

    fn try_receive_packet(&self) -> Result<Option<Vec<u8>>, String> {
        match self.incoming.lock().map_err(|_| "session receiver lock poisoned")?.try_recv() {
            Ok(packet) => Ok(Some(packet)),
            Err(mpsc::TryRecvError::Empty) => Ok(None),
            Err(mpsc::TryRecvError::Disconnected) => Err("The secure session reader has stopped.".into()),
        }
    }

    fn try_receive_offer(&self) -> Option<FileOffer> {
        self.incoming_offers.lock().ok()?.try_recv().ok()
    }

    fn try_receive_clipboard_text(&self) -> Option<String> {
        self.clipboard_incoming.lock().ok()?.try_recv().ok()
    }

    fn try_receive_display_input(&self) -> Option<DisplayInputEvent> {
        self.display_inputs.lock().ok()?.try_recv().ok()
    }
}

#[cfg(windows)]
impl Drop for DesktopSession {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[cfg(windows)]
struct PendingIncomingTransfer {
    session: Arc<DesktopSession>,
    offer: FileOffer,
}

#[cfg(windows)]
enum TransferUiEvent {
    Status(String),
    Progress { sent: u64, total: u64, speed_bytes_per_sec: f64 },
    Complete(String),
    Failed(String),
}

#[cfg(windows)]
enum DisplayLeaseEvent {
    Status { session_key: String, message: String },
    OperationStatus { session_key: String, message: String },
}

#[cfg(windows)]
fn start_display_lease_lifecycle(
    session: Arc<DesktopSession>,
    stop: Arc<std::sync::atomic::AtomicBool>,
    lease_state: Arc<Mutex<Option<(String, display_service::ServiceLease)>>>,
    events: Sender<DisplayLeaseEvent>,
) -> Result<(), String> {
    thread::Builder::new()
        .name("bridge-display-lease-lifecycle".into())
        .spawn(move || {
            let session_key = format!("{}:{}", session.remote_fingerprint, session.session_id);
            let peer = session.remote_fingerprint
                .get(..8)
                .unwrap_or(&session.remote_fingerprint)
                .to_owned();
            let result = (|| -> Result<display_service::ServiceLease, String> {
                if stop.load(Ordering::Acquire) || !session.is_alive() {
                    return Err("authenticated Android session ended before attestation".into());
                }
                let pinned_key = session.attestation_public_key.as_deref()
                    .ok_or_else(|| "trusted Android pairing record has no pinned attestation key".to_string())?;
                let nonce = display_service::issue_attestation_challenge(
                    session.session_id,
                    &session.remote_fingerprint,
                    pinned_key,
                )?;
                if stop.load(Ordering::Acquire) || !session.is_alive() {
                    return Err("authenticated Android session ended during attestation".into());
                }
                let proof = session.relay_service_challenge(&nonce, Duration::from_secs(8))?;
                if stop.load(Ordering::Acquire) || !session.is_alive() {
                    return Err("authenticated Android session ended before lease issuance".into());
                }
                display_service::submit_attestation_response(
                    &proof.nonce,
                    proof.session_id,
                    &proof.device_fingerprint,
                    &proof.signature,
                )
            })();

            let mut lease = match result {
                Ok(lease) => lease,
                Err(error) => {
                    let message = if stop.load(Ordering::Acquire) || !session.is_alive() {
                        format!("Second screen lease stopped for Android device {peer}.")
                    } else {
                        format!("Second screen attestation failed for Android device {peer}: {error}")
                    };
                    let _ = events.send(DisplayLeaseEvent::Status {
                        session_key: session_key.clone(),
                        message,
                    });
                    return;
                }
            };

            if let Ok(mut active) = lease_state.lock() {
                *active = Some((session_key.clone(), lease.clone()));
            }
            let _ = events.send(DisplayLeaseEvent::Status {
                session_key: session_key.clone(),
                message: format!("Display lease active for Android device {peer}; renewing every 5 seconds."),
            });

            loop {
                // Poll cancellation in short intervals so replacing or disconnecting
                // a peer never leaves a detached renewal worker sleeping for long.
                for _ in 0..50 {
                    if stop.load(Ordering::Acquire) || !session.is_alive() {
                        if let Ok(mut active) = lease_state.lock() {
                            if active.as_ref().is_some_and(|(key, _)| key == &session_key) {
                                *active = None;
                            }
                        }
                        let _ = events.send(DisplayLeaseEvent::Status {
                            session_key: session_key.clone(),
                            message: format!("Display lease renewal stopped for Android device {peer}; the short-lived lease will expire automatically."),
                        });
                        return;
                    }
                    thread::sleep(Duration::from_millis(100));
                }

                match display_service::renew_with_lease(&lease, &session.remote_fingerprint) {
                    Ok(renewed) if renewed.id == lease.id => {
                        lease = renewed;
                        if let Ok(mut active) = lease_state.lock() {
                            *active = Some((session_key.clone(), lease.clone()));
                        }
                        let _ = events.send(DisplayLeaseEvent::Status {
                            session_key: session_key.clone(),
                            message: format!("Display lease active and renewed for Android device {peer}."),
                        });
                    }
                    Ok(_) => {
                        if let Ok(mut active) = lease_state.lock() {
                            if active.as_ref().is_some_and(|(key, _)| key == &session_key) { *active = None; }
                        }
                        let _ = events.send(DisplayLeaseEvent::Status {
                            session_key: session_key.clone(),
                            message: format!("Display lease identity changed for Android device {peer}; renewal stopped."),
                        });
                        return;
                    }
                    Err(error) => {
                        if let Ok(mut active) = lease_state.lock() {
                            if active.as_ref().is_some_and(|(key, _)| key == &session_key) { *active = None; }
                        }
                        let _ = events.send(DisplayLeaseEvent::Status {
                            session_key: session_key.clone(),
                            message: format!("Display lease renewal failed for Android device {peer}: {error}. A fresh attestation is required."),
                        });
                        return;
                    }
                }
            }
        })
        .map(|_| ())
        .map_err(|error| format!("could not start display lease lifecycle worker: {error}"))
}

#[cfg(windows)]
fn start_display_frame_stream(
    session: Arc<DesktopSession>,
    session_key: String,
    lease: display_service::ServiceLease,
    stop: Arc<std::sync::atomic::AtomicBool>,
    events: Sender<DisplayLeaseEvent>,
) -> Result<(), String> {
    thread::Builder::new()
        .name("bridge-display-frame-stream".into())
        .spawn(move || {
            let mut last_sequence = 0_u64;
            let mut announced = false;
            while !stop.load(Ordering::Acquire) && session.is_alive() {
                let started = Instant::now();
                match display_service::read_frame_with_lease(&lease, &session.remote_fingerprint) {
                    Ok(frame) if frame.sequence > last_sequence => {
                        let display_frame = DisplayFrame {
                            sequence: frame.sequence,
                            width: frame.width,
                            height: frame.height,
                            stride: frame.stride,
                            timestamp_ns: frame.timestamp_ns,
                            pixels: frame.pixels,
                        };
                        match fragment_display_frame(&display_frame) {
                            Ok(packets) => {
                                let mut sent_all = true;
                                for packet in packets {
                                    if stop.load(Ordering::Acquire) || !session.is_alive() {
                                        sent_all = false;
                                        break;
                                    }
                                    if let Err(error) = session.send(&packet) {
                                        let _ = events.send(DisplayLeaseEvent::Status {
                                            session_key: session_key.clone(),
                                            message: format!("Second screen stream stopped: {error}"),
                                        });
                                        stop.store(true, Ordering::Release);
                                        sent_all = false;
                                        break;
                                    }
                                }
                                if sent_all {
                                    last_sequence = display_frame.sequence;
                                    if !announced {
                                        announced = true;
                                        let _ = events.send(DisplayLeaseEvent::Status {
                                            session_key: session_key.clone(),
                                            message: format!(
                                                "Second screen streaming frames to Android ({}×{}).",
                                                display_frame.width, display_frame.height
                                            ),
                                        });
                                    }
                                }
                            }
                            Err(error) => {
                                let _ = events.send(DisplayLeaseEvent::Status {
                                    session_key: session_key.clone(),
                                    message: format!("Could not encode second-screen frame: {error}"),
                                });
                                stop.store(true, Ordering::Release);
                            }
                        }
                    }
                    Ok(_) => {}
                    Err(_) => {
                        // The capture callback can briefly have no complete frame while
                        // the indirect display is starting or changing modes. Retry.
                    }
                }
                let elapsed = started.elapsed();
                if elapsed < Duration::from_millis(33) {
                    thread::sleep(Duration::from_millis(33) - elapsed);
                }
            }
        })
        .map(|_| ())
        .map_err(|error| format!("could not start second-screen frame stream: {error}"))
}

#[cfg(windows)]
fn start_display_driver_operation(
    session: Arc<DesktopSession>,
    session_key: String,
    lease: display_service::ServiceLease,
    connect: bool,
    stop: Arc<std::sync::atomic::AtomicBool>,
    lease_state: Arc<Mutex<Option<(String, display_service::ServiceLease)>>>,
    events: Sender<DisplayLeaseEvent>,
) -> Result<(), String> {
    thread::Builder::new()
        .name(if connect { "bridge-display-connect".into() } else { "bridge-display-disconnect".into() })
        .spawn(move || {
            let result = if !session.is_alive() || stop.load(Ordering::Acquire) {
                Err("the authenticated Android session is no longer active".to_string())
            } else if connect {
                display_service::connect_with_lease(&lease, &session.remote_fingerprint)
            } else {
                display_service::disconnect_with_lease(&lease, &session.remote_fingerprint)
            };

            let message = match result {
                Ok(()) if connect => {
                    let message = "Windows display driver accepted the lease-authorized CONNECT request.".to_string();
                    if let Err(error) = start_display_frame_stream(
                        Arc::clone(&session),
                        session_key.clone(),
                        lease.clone(),
                        Arc::clone(&stop),
                        events.clone(),
                    ) {
                        let _ = events.send(DisplayLeaseEvent::OperationStatus {
                            session_key: session_key.clone(),
                            message: format!("Display connected, but frame streaming could not start: {error}"),
                        });
                    }
                    message
                },
                Ok(()) => {
                    stop.store(true, Ordering::Release);
                    if let Ok(mut active) = lease_state.lock() {
                        if active.as_ref().is_some_and(|(key, _)| key == &session_key) {
                            *active = None;
                        }
                    }
                    "Windows display driver accepted DISCONNECT; a fresh Android attestation is required before reconnecting.".to_string()
                }
                Err(error) => format!(
                    "Windows display {} request failed: {error}",
                    if connect { "connect" } else { "disconnect" }
                ),
            };
            let _ = events.send(DisplayLeaseEvent::OperationStatus { session_key, message });
        })
        .map(|_| ())
        .map_err(|error| format!("could not start display driver operation: {error}"))
}

#[cfg(windows)]
fn start_receive_file_transfer(
    session: Arc<DesktopSession>,
    offer: FileOffer,
    destination: PathBuf,
    events: Sender<TransferUiEvent>,
    cancel: Arc<std::sync::atomic::AtomicBool>,
) {
    let worker_events = events.clone();
    thread::Builder::new().name("bridge-file-receive".into()).spawn(move || {
        let result = (|| -> Result<String, String> {
            let mut accepted_offer = offer.clone();
            if let Some(name) = destination.file_name().and_then(|value| value.to_str()) {
                accepted_offer.file_name = name.to_owned();
            }
            let directory = destination.parent().ok_or("Choose a valid destination folder")?;
            let packet = encode_packet(&TransferPacket::Control(
                TransferControl::FileAccept { transfer_id: offer.transfer_id }
            )).map_err(|error| error.to_string())?;
            session.send(&packet)?;
            let mut incoming = IncomingFile::create(directory, accepted_offer.clone()).map_err(|error| error.to_string())?;
            let mut received = 0_u64;
            let mut last_sample = Instant::now();
            let mut last_bytes = 0_u64;
            while received < accepted_offer.file_size {
                if cancel.load(Ordering::Acquire) { return Err("Transfer cancelled by you.".into()); }
                let packet = match session.receive_timeout(Duration::from_secs(1)) {
                    Ok(packet) => packet,
                    Err(error) if error.contains("timed out") => continue,
                    Err(error) => return Err(error),
                };
                match decode_packet(&packet).map_err(|error| error.to_string())? {
                    TransferPacket::Chunk(chunk) => {
                        received = received.saturating_add(chunk.data.len() as u64);
                        incoming.write_chunk(chunk).map_err(|error| error.to_string())?;
                        let elapsed = last_sample.elapsed().as_secs_f64().max(0.001);
                        let speed = (received.saturating_sub(last_bytes)) as f64 / elapsed;
                        last_sample = Instant::now();
                        last_bytes = received;
                        let _ = worker_events.send(TransferUiEvent::Progress { sent: received, total: accepted_offer.file_size, speed_bytes_per_sec: speed });
                    }
                    _ => return Err("Unexpected packet while receiving file data.".into()),
                }
            }
            let saved = incoming.finish().map_err(|error| error.to_string())?;
            let packet = encode_packet(&TransferPacket::Control(
                TransferControl::FileComplete { transfer_id: offer.transfer_id }
            )).map_err(|error| error.to_string())?;
            session.send(&packet)?;
            Ok(format!("Received {} → {}", offer.file_name, saved.display()))
        })();
        match result {
            Ok(message) => { let _ = worker_events.send(TransferUiEvent::Complete(message)); }
            Err(error) => {
                if let Ok(packet) = encode_packet(&TransferPacket::Control(
                    TransferControl::FileCancel { transfer_id: offer.transfer_id }
                )) {
                    let _ = session.send(&packet);
                }
                let _ = worker_events.send(TransferUiEvent::Failed(error));
            }
        }
    }).map_err(|error| {
        let _ = events.send(TransferUiEvent::Failed(format!("Could not start receive worker: {error}")));
    }).ok();
}

#[cfg(windows)]
fn reject_incoming_transfer(session: &DesktopSession, offer: &FileOffer) {
    if let Ok(packet) = encode_packet(&TransferPacket::Control(TransferControl::FileReject {
        transfer_id: offer.transfer_id,
        reason: "Declined on receiving device".into(),
    })) {
        let _ = session.send(&packet);
    }
}

#[cfg(windows)]
fn start_file_transfer(
    session: Arc<DesktopSession>,
    path: PathBuf,
    events: Sender<TransferUiEvent>,
    cancel: Arc<std::sync::atomic::AtomicBool>,
) {
    let worker_events = events.clone();
    thread::Builder::new().name("bridge-file-send".into()).spawn(move || {
        let events = worker_events;
        let mut active_transfer_id = None;
        let result = (|| -> Result<String, String> {
            let offer = offer_for_file(&path).map_err(|error| error.to_string())?;
            let file_name = offer.file_name.clone();
            let transfer_id = offer.transfer_id;
            active_transfer_id = Some(transfer_id);
            let packet = encode_packet(&TransferPacket::Control(
                TransferControl::FileOffer { offer: offer.clone() }
            )).map_err(|error| error.to_string())?;
            session.send(&packet)?;
            let _ = events.send(TransferUiEvent::Status(format!("Waiting for {} to accept {file_name}…", session.remote_fingerprint[..8].to_owned())));
            let approval_deadline = Instant::now() + Duration::from_secs(600);
            let response = loop {
                if cancel.load(Ordering::Acquire) { return Err("Transfer cancelled by you.".into()); }
                match session.receive_timeout(Duration::from_secs(1)) {
                    Ok(packet) => break packet,
                    Err(error) if error.contains("timed out") && Instant::now() < approval_deadline => continue,
                    Err(error) => return Err(error),
                }
            };
            match decode_packet(&response).map_err(|error| error.to_string())? {
                TransferPacket::Control(TransferControl::FileAccept { transfer_id: accepted })
                    if accepted == transfer_id => {}
                TransferPacket::Control(TransferControl::FileReject { transfer_id: rejected, reason })
                    if rejected == transfer_id => return Err(format!("Transfer rejected: {reason}")),
                _ => return Err("The connected device returned an unexpected transfer response.".into()),
            }

            let mut outgoing = OutgoingFile::open(&path, offer).map_err(|error| error.to_string())?;
            let total = outgoing.total_bytes();
            let mut sent = 0_u64;
            let mut last_sample = Instant::now();
            let mut last_bytes = 0_u64;
            while let Some(chunk) = outgoing.next_chunk().map_err(|error| error.to_string())? {
                if cancel.load(Ordering::Acquire) { return Err("Transfer cancelled by you.".into()); }
                if let Some(packet) = session.try_receive_packet()? {
                    match decode_packet(&packet).map_err(|error| error.to_string())? {
                        TransferPacket::Control(TransferControl::FileCancel { transfer_id: cancelled })
                            if cancelled == transfer_id => {
                                return Err("The receiving device cancelled the transfer.".into());
                            }
                        _ => return Err("The receiving device sent an unexpected message during file transfer.".into()),
                    }
                }
                sent = sent.saturating_add(chunk.data.len() as u64);
                let packet = encode_packet(&TransferPacket::Chunk(chunk)).map_err(|error| error.to_string())?;
                session.send(&packet)?;
                let elapsed = last_sample.elapsed().as_secs_f64().max(0.001);
                let speed = (sent.saturating_sub(last_bytes)) as f64 / elapsed;
                last_sample = Instant::now();
                last_bytes = sent;
                let _ = events.send(TransferUiEvent::Progress { sent, total, speed_bytes_per_sec: speed });
            }
            let completion_deadline = Instant::now() + Duration::from_secs(600);
            let response = loop {
                if cancel.load(Ordering::Acquire) { return Err("Transfer cancelled by you.".into()); }
                match session.receive_timeout(Duration::from_secs(1)) {
                    Ok(packet) => break packet,
                    Err(error) if error.contains("timed out") && Instant::now() < completion_deadline => continue,
                    Err(error) => return Err(error),
                }
            };
            match decode_packet(&response).map_err(|error| error.to_string())? {
                TransferPacket::Control(TransferControl::FileComplete { transfer_id: completed })
                    if completed == transfer_id => Ok(format!("{file_name} sent successfully.")),
                TransferPacket::Control(TransferControl::FileCancel { transfer_id: cancelled })
                    if cancelled == transfer_id => Err("The receiving device cancelled the transfer.".into()),
                _ => Err("The connected device did not confirm the completed transfer.".into()),
            }
        })();
        match result {
            Ok(message) => { let _ = events.send(TransferUiEvent::Complete(message)); }
            Err(error) => {
                if let Some(transfer_id) = active_transfer_id {
                    if let Ok(packet) = encode_packet(&TransferPacket::Control(TransferControl::FileCancel { transfer_id })) {
                        let _ = session.send(&packet);
                    }
                }
                let _ = events.send(TransferUiEvent::Failed(error));
            }
        }
    }).map_err(|error| {
        let _ = events.send(TransferUiEvent::Failed(format!("Could not start transfer worker: {error}")));
    }).ok();
}

#[cfg(windows)]
struct ConnectionPermit(Arc<AtomicUsize>);

#[cfg(windows)]
impl Drop for ConnectionPermit {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

struct BridgeApp {
    selected: Page,
    show_pair_dialog: bool,
    clipboard_sync: bool,
    #[cfg(windows)]
    clipboard_draft: String,
    clipboard_received: Option<String>,
    clipboard_status: String,
    launch_at_login: bool,
    notifications: bool,
    auto_accept: bool,
    network_status: String,
    #[cfg(windows)]
    discovered_peers: Vec<DiscoveredPeer>,
    #[cfg(windows)]
    peer_rx: Receiver<DiscoveredPeer>,
    #[cfg(windows)]
    pairing_tx: Sender<PendingApproval>,
    #[cfg(windows)]
    pairing_rx: Receiver<PendingApproval>,
    #[cfg(windows)]
    pending_pairings: Vec<PendingApproval>,
    #[cfg(windows)]
    active_sessions: Vec<Arc<DesktopSession>>,
    #[cfg(windows)]
    display_challenge_session: Option<String>,
    #[cfg(windows)]
    display_lease_stop: Option<Arc<std::sync::atomic::AtomicBool>>,
    #[cfg(windows)]
    display_lease: Arc<Mutex<Option<(String, display_service::ServiceLease)>>>,
    #[cfg(windows)]
    display_lease_status: String,
    #[cfg(windows)]
    display_input_session: Option<String>,
    #[cfg(windows)]
    last_display_input_sequence: u64,
    #[cfg(windows)]
    display_lease_tx: Sender<DisplayLeaseEvent>,
    #[cfg(windows)]
    display_lease_rx: Receiver<DisplayLeaseEvent>,
    #[cfg(windows)]
    pending_incoming_transfers: Vec<PendingIncomingTransfer>,
    #[cfg(windows)]
    selected_transfer_fingerprint: Option<String>,
    #[cfg(windows)]
    transfer_tx: Sender<TransferUiEvent>,
    #[cfg(windows)]
    transfer_rx: Receiver<TransferUiEvent>,
    #[cfg(windows)]
    transfer_busy: bool,
    #[cfg(windows)]
    transfer_progress: f32,
    #[cfg(windows)]
    transfer_speed: f64,
    #[cfg(windows)]
    transfer_cancel: Option<Arc<std::sync::atomic::AtomicBool>>,
    #[cfg(windows)]
    transfer_status: String,
    #[cfg(windows)]
    trust_store: TrustStore,
    #[cfg(windows)]
    trust_store_error: Option<String>,
    #[cfg(windows)]
    update_channel: String,
    #[cfg(windows)]
    update_status: String,
    #[cfg(windows)]
    update_available: Option<(String, PathBuf)>,
    #[cfg(windows)]
    update_tx: Sender<UpdateEvent>,
    #[cfg(windows)]
    update_rx: Receiver<UpdateEvent>,
    network_rx: Receiver<String>,
    #[cfg(windows)]
    network_tx: Sender<String>,
    brand_texture: Option<egui::TextureHandle>,
}

impl Default for BridgeApp {
    fn default() -> Self {
        let (network_tx, network_rx) = mpsc::channel();
        let identity_status = secure_identity_status();
        #[cfg(windows)]
        let (peer_tx, peer_rx) = mpsc::channel();
        #[cfg(windows)]
        let (pairing_tx, pairing_rx) = mpsc::channel();
        #[cfg(windows)]
        let (transfer_tx, transfer_rx) = mpsc::channel();
        #[cfg(windows)]
        let (display_lease_tx, display_lease_rx) = mpsc::channel();
        #[cfg(windows)]
        let (update_tx, update_rx) = mpsc::channel();
        #[cfg(windows)]
        let update_channel = load_update_channel();
        #[cfg(windows)]
        check_for_update(update_channel.clone(), update_tx.clone());
        #[cfg(windows)]
        let (trust_store, trust_store_error) = match TrustStore::load(trust_store_path()) {
            Ok(store) => (store, None),
            Err(error) => (TrustStore::new(), Some(error.to_string())),
        };
        #[cfg(windows)]
        let initial_status = trust_store_error.as_ref()
            .map(|error| format!("Trusted-device registry unavailable: {error}. Pairing approvals are disabled."))
            .unwrap_or(identity_status);
        #[cfg(not(windows))]
        let initial_status = identity_status;
        #[cfg(windows)]
        {
            start_secure_listener(network_tx.clone(), pairing_tx.clone());
            start_local_discovery(network_tx.clone(), peer_tx);
        }
        #[cfg(not(windows))]
        let _ = &network_tx;
        Self {
            selected: Page::Overview,
            show_pair_dialog: false,
            clipboard_sync: false,
            #[cfg(windows)]
            clipboard_draft: String::new(),
            clipboard_received: None,
            clipboard_status: "Clipboard sharing is off.".into(),
            launch_at_login: false,
            notifications: true,
            auto_accept: false,
            network_status: initial_status,
            #[cfg(windows)]
            discovered_peers: Vec::new(),
            #[cfg(windows)]
            peer_rx,
            #[cfg(windows)]
            pairing_tx,
            #[cfg(windows)]
            pairing_rx,
            #[cfg(windows)]
            pending_pairings: Vec::new(),
            #[cfg(windows)]
            active_sessions: Vec::new(),
            #[cfg(windows)]
            display_challenge_session: None,
            #[cfg(windows)]
            display_lease_stop: None,
            #[cfg(windows)]
            display_lease: Arc::new(Mutex::new(None)),
            #[cfg(windows)]
            display_lease_status: "Waiting for a trusted Android device.".into(),
            #[cfg(windows)]
            display_input_session: None,
            #[cfg(windows)]
            last_display_input_sequence: 0,
            #[cfg(windows)]
            display_lease_tx,
            #[cfg(windows)]
            display_lease_rx,
            #[cfg(windows)]
            pending_incoming_transfers: Vec::new(),
            #[cfg(windows)]
            selected_transfer_fingerprint: None,
            #[cfg(windows)]
            transfer_tx,
            #[cfg(windows)]
            transfer_rx,
            #[cfg(windows)]
            transfer_busy: false,
            #[cfg(windows)]
            transfer_progress: 0.0,
            #[cfg(windows)]
            transfer_speed: 0.0,
            #[cfg(windows)]
            transfer_cancel: None,
            #[cfg(windows)]
            transfer_status: "Choose a connected trusted device to send a file.".into(),
            #[cfg(windows)]
            trust_store,
            #[cfg(windows)]
            trust_store_error,
            #[cfg(windows)]
            update_channel,
            #[cfg(windows)]
            update_status: "Choose a channel and check for updates.".into(),
            #[cfg(windows)]
            update_available: None,
            #[cfg(windows)]
            update_tx,
            #[cfg(windows)]
            update_rx,
            network_rx,
            #[cfg(windows)]
            network_tx,
            brand_texture: None,
        }
    }
}

#[cfg(windows)]
fn update_channel_path() -> PathBuf {
    std::env::var_os("APPDATA").map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("Bridge")
        .join("update-channel.txt")
}

#[cfg(windows)]
fn load_update_channel() -> String {
    if std::fs::read_to_string(update_channel_path()).map(|value| value.trim() == "stable").unwrap_or(false) {
        "stable".into()
    } else {
        "development".into()
    }
}

#[cfg(windows)]
fn save_update_channel(channel: &str) {
    let path = update_channel_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(path, if channel == "stable" { "stable" } else { "development" });
}

#[cfg(windows)]
const NETWORK_PORT: u16 = 45821;

#[cfg(windows)]
fn local_device_id() -> DeviceId {
    let path = std::env::var_os("APPDATA").map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("Bridge")
        .join("device-id");
    if let Ok(value) = std::fs::read_to_string(&path) {
        if let Ok(id) = uuid::Uuid::parse_str(value.trim()) {
            return DeviceId(id);
        }
    }
    let id = DeviceId::new();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(path, id.0.to_string());
    id
}

#[cfg(windows)]
fn trust_store_path() -> PathBuf {
    std::env::var_os("APPDATA").map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("Bridge")
        .join("trusted-devices.json")
}

#[cfg(windows)]
fn start_secure_listener(status: Sender<String>, pending: Sender<PendingApproval>) {
    thread::spawn(move || {
        let private_key = match identity_store::load_or_create() {
            Ok(key) => key,
            Err(error) => { let _ = status.send(format!("Secure listener unavailable: {error}")); return; }
        };
        let fingerprint = match local_key_fingerprint(&private_key) {
            Ok(value) => value,
            Err(error) => { let _ = status.send(format!("Secure listener unavailable: {error}")); return; }
        };
        let local = DeviceDescriptor {
            id: local_device_id(),
            display_name: "Bridge Windows".into(),
            platform: Platform::Windows,
            protocol_version: PROTOCOL_VERSION,
            attestation_public_key: None,
        };
        let listener = match TcpListener::bind(("0.0.0.0", NETWORK_PORT)) {
            Ok(listener) => listener,
            Err(error) => { let _ = status.send(format!("Secure listener could not bind: {error}")); return; }
        };
        let _ = status.send(format!("Secure pairing listener ready · identity {}", &fingerprint[..8]));
        // Bound concurrent unauthenticated handshakes to prevent thread exhaustion.
        const MAX_PENDING_HANDSHAKES: usize = 8;
        let active = Arc::new(AtomicUsize::new(0));
        for incoming in listener.incoming() {
            match incoming {
                Ok(stream) => {
                    if active.fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                        (count < MAX_PENDING_HANDSHAKES).then_some(count + 1)
                    }).is_err() {
                        let _ = status.send("Pairing busy · rejected an additional incoming connection".into());
                        drop(stream);
                        continue;
                    }
                    let permit = ConnectionPermit(Arc::clone(&active));
                    let key = private_key.clone();
                    let local = local.clone();
                    let status = status.clone();
                    let pending = pending.clone();
                    thread::spawn(move || {
                        let _permit = permit;
                        let result = accept_noise_xx(stream, &key, None)
                            .and_then(|mut connection| {
                                let descriptor = connection.exchange_descriptors(&local)?;
                                Ok((connection, descriptor))
                            });
                        match result {
                            Ok((connection, descriptor)) => {
                                if let Err(error) = validate_peer_attestation_descriptor(&descriptor) {
                                    let _ = status.send(format!("Rejected Android pairing: {error}"));
                                    return;
                                }
                                let store = match TrustStore::load(trust_store_path()) {
                                    Ok(store) => store,
                                    Err(error) => {
                                        let _ = status.send(format!("Trust registry unavailable; rejecting pairing: {error}"));
                                        return;
                                    }
                                };
                                let already_trusted = if let Some(trusted) = store.get(&descriptor.id) {
                                    if !trusted.public_key_fingerprint.eq_ignore_ascii_case(&connection.remote_fingerprint) {
                                        let _ = status.send(format!("Rejected identity change for trusted device {}", descriptor.display_name));
                                        return;
                                    }
                                    true
                                } else { false };
                                let message = if already_trusted {
                                    format!("Secure session re-established with {}", descriptor.display_name)
                                } else {
                                    format!("Pairing request from {} · awaiting approval on this device", descriptor.display_name)
                                };
                                let _ = status.send(message);
                                if pending.send(PendingApproval {
                                    connection, descriptor, already_trusted, incoming: true
                                }).is_err() {
                                    let _ = status.send("Pairing window closed; pending session discarded".into());
                                }
                            }
                            Err(error) => { let _ = status.send(format!("Rejected incoming pairing: {error}")); }
                        }
                    });
                }
                Err(error) => { let _ = status.send(format!("Secure listener error: {error}")); }
            }
        }
    });
}

#[cfg(windows)]
fn start_pairing(peer: DiscoveredPeer, expected: Option<String>, status: Sender<String>, pending: Sender<PendingApproval>) {
    thread::spawn(move || {
        let result = identity_store::load_or_create()
            .map_err(|error| format!("Identity unavailable: {error}"))
            .and_then(|key| {
                let stream = connect_to_peer(&peer).map_err(|error| error.to_string())?;
                let mut connection = connect_noise_xx(stream, &key, expected.as_deref())
                    .map_err(|error| error.to_string())?;
                let local = DeviceDescriptor {
                    id: local_device_id(),
                    display_name: "Bridge Windows".into(),
                    platform: Platform::Windows,
                    protocol_version: PROTOCOL_VERSION,
                    attestation_public_key: None,
                };
                let descriptor = connection.exchange_descriptors(&local).map_err(|error| error.to_string())?;
                validate_peer_attestation_descriptor(&descriptor)?;
                let store = TrustStore::load(trust_store_path()).map_err(|error| error.to_string())?;
                let already_trusted = if let Some(trusted) = store.get(&descriptor.id) {
                    if !trusted.public_key_fingerprint.eq_ignore_ascii_case(&connection.remote_fingerprint) {
                        return Err("trusted peer fingerprint mismatch".into());
                    }
                    true
                } else { false };
                Ok(PendingApproval { connection, descriptor, already_trusted, incoming: false })
            });
        match result {
            Ok(mut approval) => {
                if !approval.already_trusted {
                    if let Err(error) = receive_pairing_decision(&mut approval.connection) {
                        let _ = status.send(format!("Pairing was not approved: {error}"));
                        return;
                    }
                    let confirmation = match SasConfirmation::user_approved(&approval.connection.remote_fingerprint) {
                        Ok(value) => value,
                        Err(error) => { let _ = status.send(format!("Could not verify approved device: {error}")); return; }
                    };
                    let mut store = match TrustStore::load(trust_store_path()) {
                        Ok(value) => value,
                        Err(error) => { let _ = status.send(format!("Trust registry unavailable: {error}")); return; }
                    };
                    if let Err(error) = store.trust_and_save(approval.descriptor.clone(), confirmation, now_ms(), trust_store_path()) {
                        let _ = status.send(format!("Could not save approved device: {error}"));
                        return;
                    }
                    approval.already_trusted = true;
                }
                if pending.send(approval).is_err() {
                    let _ = status.send("Pairing window closed; session discarded".into());
                }
            }
            Err(error) => { let _ = status.send(format!("Secure pairing failed: {error}")); }
        }
    });
}

#[cfg(windows)]
fn send_pairing_decision(connection: &mut NoiseConnection, decision: &str) -> Result<(), String> {
    let message = serde_json::json!({"type":"pairing_decision","decision":decision}).to_string();
    let mut encrypted = vec![0_u8; bridge_network::MAX_FRAME_BYTES];
    let length = connection.transport.write_message(message.as_bytes(), &mut encrypted)
        .map_err(|error| error.to_string())?;
    bridge_network::write_frame(&mut connection.stream, &encrypted[..length])
        .map_err(|error| error.to_string())
}

#[cfg(windows)]
fn receive_pairing_decision(connection: &mut NoiseConnection) -> Result<(), String> {
    connection.stream.set_read_timeout(Some(Duration::from_secs(120)))
        .map_err(|error| error.to_string())?;
    let result = (|| {
        let encrypted = bridge_network::read_frame(&mut connection.stream).map_err(|error| error.to_string())?;
        let mut plaintext = vec![0_u8; bridge_network::MAX_FRAME_BYTES];
        let length = connection.transport.read_message(&encrypted, &mut plaintext)
            .map_err(|error| error.to_string())?;
        let message: serde_json::Value = serde_json::from_slice(&plaintext[..length])
            .map_err(|error| error.to_string())?;
        if message["type"] == "pairing_decision" && message["decision"] == "accepted" {
            Ok(())
        } else {
            Err("the other device rejected the request".into())
        }
    })();
    let _ = connection.stream.set_read_timeout(Some(Duration::from_secs(10)));
    result
}

#[cfg(windows)]
fn start_local_discovery(status: Sender<String>, peers: Sender<DiscoveredPeer>) {
    thread::spawn(move || {
        let private_key = match identity_store::load_or_create() {
            Ok(key) => key,
            Err(error) => {
                let _ = status.send(format!("Local discovery unavailable: {error}"));
                return;
            }
        };
        let fingerprint = match local_key_fingerprint(&private_key) {
            Ok(value) => value,
            Err(error) => {
                let _ = status.send(format!("Local discovery unavailable: {error}"));
                return;
            }
        };
        let suffix = &fingerprint[..8];
        let record = DiscoveryRecord {
            device_id: local_device_id().0.to_string(),
            display_name: "Bridge Windows".into(),
            platform: Platform::Windows,
            protocol_version: PROTOCOL_VERSION,
            public_key_fingerprint: fingerprint.clone(),
        };

        // Recreate the mDNS daemon periodically. Windows can change Wi-Fi
        // interfaces/IP addresses while Bridge remains open; a daemon created
        // before that transition may keep advertising on the old interface.
        // Refreshing the daemon restores advertisement and browsing without
        // touching trusted identities or authenticated-session state.
        loop {
            let (discovery, events) = match LocalDiscovery::start(
                &format!("Bridge-{suffix}"),
                &format!("bridge-{suffix}.local."),
                NETWORK_PORT,
                &record,
            ) {
                Ok(value) => value,
                Err(error) => {
                    let _ = status.send(format!("Local discovery restarting after error: {error}"));
                    thread::sleep(Duration::from_secs(5));
                    continue;
                }
            };
            let _ = status.send("Bridge is advertising on the local network. Discovered peers remain unverified.".into());
            loop {
                match events.recv_timeout(Duration::from_secs(15)) {
                    Ok(event) => {
                        if let Some(peer) = peer_from_event(&event) {
                            if peer.record.public_key_fingerprint.eq_ignore_ascii_case(&fingerprint) {
                                continue;
                            }
                            let _ = status.send(format!(
                                "Found unverified local peer: {} · {}",
                                peer.record.display_name, peer.address,
                            ));
                            let _ = peers.send(peer);
                        }
                    }
                    // A timeout or channel error triggers a daemon refresh.
                    // This also covers Wi-Fi changes that do not surface as mDNS errors.
                    Err(_) => break,
                }
            }
            drop(events);
            let _ = discovery.shutdown();
            thread::sleep(Duration::from_secs(2));
        }
    });
}

fn panel<R>(ui: &mut egui::Ui, contents: impl FnOnce(&mut egui::Ui) -> R) -> egui::InnerResponse<R> {
    egui::Frame::new()
        .fill(PANEL)
        .stroke(egui::Stroke::new(1.0_f32, BORDER))
        .corner_radius(egui::CornerRadius::same(13))
        .inner_margin(egui::Margin::same(18))
        .show(ui, contents)
}

fn eyebrow(ui: &mut egui::Ui, value: &str) {
    ui.label(egui::RichText::new(value.to_uppercase()).size(10.0).strong().color(ACCENT));
}

fn muted(ui: &mut egui::Ui, value: &str) {
    ui.label(egui::RichText::new(value).size(12.0).color(MUTED));
}

fn page_header(ui: &mut egui::Ui, page: Page) {
    ui.label(egui::RichText::new(page.title()).size(27.0).strong().color(TEXT));
    muted(ui, page.subtitle());
    ui.add_space(20.0);
}

fn primary_button(ui: &mut egui::Ui, label: &str) -> bool {
    ui.add(egui::Button::new(egui::RichText::new(label).strong().color(egui::Color32::from_rgb(15, 19, 13)))
        .fill(ACCENT)
        .corner_radius(egui::CornerRadius::same(8))
        .min_size(egui::vec2(0.0, 36.0))).clicked()
}

fn metric(ui: &mut egui::Ui, value: &str, label: &str, note: &str) {
    panel(ui, |ui| {
        ui.label(egui::RichText::new(value).size(25.0).strong().color(TEXT));
        ui.add_space(4.0);
        ui.label(egui::RichText::new(label).size(12.0).strong().color(TEXT));
        muted(ui, note);
    });
}

fn feature_row(ui: &mut egui::Ui, symbol: &str, title: &str, detail: &str, status: &str) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(symbol).size(19.0).color(ACCENT));
        ui.vertical(|ui| {
            ui.label(egui::RichText::new(title).strong().color(TEXT));
            muted(ui, detail);
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            muted(ui, status);
        });
    });
}

impl eframe::App for BridgeApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        ctx.request_repaint_after(Duration::from_millis(500));
        for message in self.network_rx.try_iter() {
            self.network_status = message;
        }
        #[cfg(windows)]
        for event in self.update_rx.try_iter() {
            match event {
                UpdateEvent::Status(message) => self.update_status = message,
                UpdateEvent::Available { version, installer } => {
                    self.update_status = format!("Bridge {version} is ready to install.");
                    self.update_available = Some((version, installer));
                }
            }
        }
        #[cfg(windows)]
        {
            // Remove sessions whose reader observed EOF or a transport failure.
            // Reconnected peers must never keep selecting an older dead session.
            self.active_sessions.retain(|session| session.is_alive());
            // Only Android peers with a pairing-pinned attestation key can
            // receive a Second Screen authorization lease.
            let challenge_session = self.active_sessions.iter()
                .find(|session| session.attestation_public_key.is_some())
                .cloned();
            let challenge_fingerprint = challenge_session.as_ref().map(|session| {
                format!("{}:{}", session.remote_fingerprint, session.session_id)
            });
            if self.display_challenge_session != challenge_fingerprint {
                if let Some(stop) = self.display_lease_stop.take() {
                    stop.store(true, Ordering::Release);
                }
                if let Ok(mut active) = self.display_lease.lock() {
                    *active = None;
                }
                self.display_challenge_session = challenge_fingerprint;
                if let Some(session) = challenge_session {
                    self.display_lease_status = format!(
                        "Verifying Android device {} and requesting a display lease…",
                        session.remote_fingerprint.get(..8).unwrap_or(&session.remote_fingerprint)
                    );
                    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
                    self.display_lease_stop = Some(Arc::clone(&stop));
                    if let Err(error) = start_display_lease_lifecycle(
                        session,
                        stop,
                        Arc::clone(&self.display_lease),
                        self.display_lease_tx.clone(),
                    ) {
                        self.display_lease_status = error;
                    }
                } else {
                    self.display_lease_status = "No authenticated Android session; display lease is inactive.".into();
                }
            }
            for event in self.display_lease_rx.try_iter() {
                match event {
                    DisplayLeaseEvent::Status { session_key, message }
                    | DisplayLeaseEvent::OperationStatus { session_key, message } => {
                        if self.display_challenge_session.as_deref() == Some(session_key.as_str()) {
                            self.display_lease_status = message;
                        }
                    }
                }
            }
            let active_display_key = self.display_lease.lock().ok()
                .and_then(|active| active.as_ref().map(|(key, _)| key.clone()));
            if self.display_input_session != active_display_key {
                let _ = inject_display_pointer_release();
                self.display_input_session = active_display_key.clone();
                self.last_display_input_sequence = 0;
            }
            for session in &self.active_sessions {
                let session_key = format!("{}:{}", session.remote_fingerprint, session.session_id);
                while let Some(event) = session.try_receive_display_input() {
                    if !session.is_alive()
                        || active_display_key.as_deref() != Some(session_key.as_str())
                        || self.display_challenge_session.as_deref() != Some(session_key.as_str())
                        || event.sequence <= self.last_display_input_sequence
                    {
                        continue;
                    }
                    self.last_display_input_sequence = event.sequence;
                    if event.pointer_id == 0 {
                        if let Err(error) = inject_display_pointer_event(event) {
                            self.display_lease_status = format!("Second-screen pointer input rejected: {error}");
                        }
                    }
                }
            }
            for session in &self.active_sessions {
                while let Some(text) = session.try_receive_clipboard_text() {
                    if self.clipboard_sync {
                        self.clipboard_received = Some(text);
                        self.clipboard_status = "Received text from a trusted device.".into();
                    }
                }
            }
            for session in &self.active_sessions {
                while let Some(offer) = session.try_receive_offer() {
                    self.pending_incoming_transfers.push(PendingIncomingTransfer {
                        session: Arc::clone(session),
                        offer,
                    });
                }
            }
        }
        #[cfg(windows)]
        for event in self.transfer_rx.try_iter() {
            match event {
                TransferUiEvent::Status(message) => self.transfer_status = message,
                TransferUiEvent::Progress { sent, total, speed_bytes_per_sec } => {
                    self.transfer_progress = if total == 0 { 1.0 } else { sent as f32 / total as f32 };
                    self.transfer_speed = speed_bytes_per_sec;
                    self.transfer_status = format!("{} / {} MB", sent as f64 / 1_048_576.0, total as f64 / 1_048_576.0);
                }
                TransferUiEvent::Complete(message) => {
                    self.transfer_busy = false;
                    self.transfer_progress = 1.0;
                    self.transfer_speed = 0.0;
                    self.transfer_cancel = None;
                    self.transfer_status = message;
                }
                TransferUiEvent::Failed(error) => {
                    self.transfer_busy = false;
                    self.transfer_progress = 0.0;
                    self.transfer_speed = 0.0;
                    self.transfer_cancel = None;
                    self.transfer_status = error;
                }
            }
        }
        #[cfg(windows)]
        {
            for peer in self.peer_rx.try_iter() {
                // mDNS may resolve one physical device over both IPv4 and IPv6.
                // Device identity, not socket address, defines one discovered row.
                if let Some(existing) = self.discovered_peers.iter_mut()
                    .find(|known| {
                        known.record.device_id == peer.record.device_id
                            || known.record.public_key_fingerprint.eq_ignore_ascii_case(
                                &peer.record.public_key_fingerprint,
                            )
                    })
                {
                    // A single authenticated device may be resolved through both
                    // IPv4 and IPv6. Device ID is the primary key; the advertised
                    // public-key fingerprint is a stable fallback if a client has
                    // inconsistent discovery IDs. Prefer IPv4 for Windows Connect.
                    if peer.address.is_ipv4() || !existing.address.is_ipv4() {
                        *existing = peer;
                    }
                } else {
                    self.discovered_peers.push(peer);
                }
            }
            for mut pairing in self.pairing_rx.try_iter() {
                // Pairing workers persist trust independently. Refresh the UI's snapshot
                // before rendering so a successful connection is reflected immediately.
                match TrustStore::load(trust_store_path()) {
                    Ok(store) => {
                        self.trust_store = store;
                        self.trust_store_error = None;
                    }
                    Err(error) => {
                        self.trust_store_error = Some(error.to_string());
                        self.network_status = format!("Could not refresh trusted devices: {error}");
                    }
                }
                if pairing.already_trusted {
                    // A trusted inbound peer is auto-approved, but the initiator still
                    // waits for this encrypted acknowledgement before marking the session ready.
                    if pairing.incoming {
                        if let Err(error) = send_pairing_decision(&mut pairing.connection, "accepted") {
                            self.network_status = format!("Could not confirm trusted session: {error}");
                            continue;
                        }
                    }
                    self.network_status = format!("Secure session established with {}.", pairing.descriptor.display_name);
                    match DesktopSession::start(pairing.connection, pairing.descriptor.attestation_public_key.clone()) {
                        Ok(session) => {
                            for existing in &self.active_sessions {
                                if existing.remote_fingerprint.eq_ignore_ascii_case(&session.remote_fingerprint) {
                                    existing.shutdown();
                                }
                            }
                            self.active_sessions.retain(|existing| existing.remote_fingerprint != session.remote_fingerprint);
                            self.active_sessions.push(session);
                        }
                        Err(error) => self.network_status = format!("Could not start secure session dispatcher: {error}"),
                    }
                } else {
                    self.pending_pairings.push(pairing);
                }
            }
        }
        ctx.request_repaint_after(Duration::from_millis(250));
        if self.brand_texture.is_none() {
            let logo = image::load_from_memory(include_bytes!("../../../assets/bridge-app-icon.png"))
                .expect("Bridge brand icon must be a valid PNG").to_rgba8();
            let size = [logo.width() as usize, logo.height() as usize];
            let pixels = egui::ColorImage::from_rgba_unmultiplied(size, logo.as_raw());
            self.brand_texture = Some(ctx.load_texture("bridge-brand", pixels, egui::TextureOptions::LINEAR));
        }
        let brand_texture = self.brand_texture.clone();
        let mut visuals = egui::Visuals::dark();
        visuals.window_fill = PANEL;
        visuals.panel_fill = BG;
        visuals.override_text_color = Some(TEXT);
        visuals.widgets.noninteractive.bg_fill = PANEL;
        visuals.widgets.inactive.bg_fill = PANEL_ALT;
        visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(16, 45, 53);
        visuals.widgets.active.bg_fill = egui::Color32::from_rgb(18, 70, 82);
        visuals.selection.bg_fill = egui::Color32::from_rgb(16, 65, 78);
        visuals.selection.stroke = egui::Stroke::new(1.0_f32, ACCENT);
        ctx.set_visuals(visuals);

        egui::TopBottomPanel::top("topbar")
            .frame(egui::Frame::new().fill(SIDEBAR).inner_margin(egui::Margin::symmetric(20, 12)))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if let Some(texture) = &brand_texture {
                        ui.image((texture.id(), egui::vec2(28.0, 28.0)));
                    }
                    ui.label(egui::RichText::new("BRIDGE").size(12.0).strong().color(TEXT));
                    ui.separator();
                    muted(ui, "Private device workspace");
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(egui::RichText::new("●  LOCAL MODE").size(10.0).strong().color(GREEN));
                        ui.separator();
                        muted(ui, "v0.3 · Preview");
                    });
                });
            });

        egui::SidePanel::left("navigation")
            .exact_width(218.0)
            .frame(egui::Frame::new().fill(SIDEBAR).inner_margin(egui::Margin::same(16)))
            .show(ctx, |ui| {
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if let Some(texture) = &brand_texture {
                        ui.image((texture.id(), egui::vec2(46.0, 46.0)));
                    }
                    ui.vertical(|ui| {
                        ui.label(egui::RichText::new("BRIDGE").size(17.0).strong());
                        muted(ui, "EVERY DEVICE. ONE SPACE.");
                    });
                });
                ui.add_space(34.0);
                eyebrow(ui, "Workspace");
                ui.add_space(10.0);
                for (page, symbol) in [
                    (Page::Overview, "◈"),
                    (Page::Devices, "⌁"),
                    (Page::Transfers, "⇧"),
                    (Page::Clipboard, "▤"),
                    (Page::Tools, "⌘"),
                    (Page::Settings, "⚙"),
                ] {
                    let selected = self.selected == page;
                    let response = ui.add_sized(
                        [ui.available_width(), 39.0],
                        egui::Button::new(
                            egui::RichText::new(format!("   {symbol}     {}", page.title()))
                                .color(if selected { ACCENT } else { TEXT }),
                        )
                        .fill(if selected {
                            egui::Color32::from_rgb(16, 45, 53)
                        } else {
                            PANEL_ALT
                        })
                        .stroke(egui::Stroke::new(
                            1.0_f32,
                            if selected { ACCENT } else { BORDER },
                        ))
                        .corner_radius(egui::CornerRadius::same(8)),
                    );
                    if response.clicked() { self.selected = page; }
                    ui.add_space(4.0);
                }
                ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                    panel(ui, |ui| {
                        ui.horizontal(|ui| {
                            egui::Frame::new().fill(egui::Color32::from_rgb(38, 54, 44))
                                .corner_radius(egui::CornerRadius::same(20)).inner_margin(egui::Margin::same(7))
                                .show(ui, |ui| { ui.label(egui::RichText::new("●").color(GREEN)); });
                            ui.vertical(|ui| {
                                ui.label(egui::RichText::new("This device").strong().size(12.0));
                                #[cfg(windows)]
                                {
                                    let connected = self.active_sessions.len();
                                    muted(ui, &if connected == 0 {
                                        "Waiting for pairing".to_owned()
                                    } else if connected == 1 {
                                        "1 device connected".to_owned()
                                    } else {
                                        format!("{connected} devices connected")
                                    });
                                }
                                #[cfg(not(windows))]
                                muted(ui, "Waiting for pairing");
                            });
                        });
                    });
                    ui.add_space(12.0);
                    muted(ui, "BRIDGE · BUILT FOR YOUR DEVICES");
                });
            });

        egui::CentralPanel::default().frame(egui::Frame::new().fill(BG).inner_margin(egui::Margin::same(25))).show(ctx, |ui| {
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                page_header(ui, self.selected);
                match self.selected {
                    Page::Overview => {
                        #[cfg(windows)]
                        let trusted_count = self.trust_store.list().len();
                        #[cfg(not(windows))]
                        let trusted_count = 0usize;
                        #[cfg(windows)]
                        let active_count = self.active_sessions.len();
                        #[cfg(not(windows))]
                        let active_count = 0usize;
                        ui.columns(2, |columns| {
                            panel(&mut columns[0], |ui| {
                                eyebrow(ui, "Your ecosystem");
                                ui.add_space(10.0);
                                ui.label(egui::RichText::new("Everything works\nbetter together.").size(29.0).strong().color(TEXT));
                                ui.add_space(8.0);
                                muted(ui, "A private connection between your PC, phone and the devices you trust.");
                                ui.add_space(18.0);
                                if primary_button(ui, "＋  Pair a new device") { self.show_pair_dialog = true; }
                            });
                            panel(&mut columns[1], |ui| {
                                eyebrow(ui, "Connection status");
                                ui.add_space(10.0);
                                ui.horizontal(|ui| {
                                    ui.label(egui::RichText::new("◎").size(30.0).color(ACCENT));
                                    ui.vertical(|ui| {
                                        ui.label(egui::RichText::new(if active_count == 0 { "Ready when you are" } else { "Secure session active" }).strong().size(16.0));
                                        muted(ui, &format!("{active_count} active secure session(s)"));
                                    });
                                });
                                ui.add_space(12.0);
                                muted(ui, "Your devices stay yours. Pair only the devices you choose.");
                            });
                        });
                        ui.add_space(18.0);
                        eyebrow(ui, "At a glance");
                        ui.add_space(10.0);
                        ui.columns(3, |cols| {
                            metric(&mut cols[0], &trusted_count.to_string(), "Trusted devices", if trusted_count == 1 { "1 device approved" } else { "Approved devices on this PC" });
                            metric(&mut cols[1], "0", "Active transfers", "No files moving");
                            metric(&mut cols[2], "Private", "Network mode", "Local-first by design");
                        });
                        ui.add_space(22.0);
                        ui.columns(2, |cols| {
                            panel(&mut cols[0], |ui| {
                                ui.label(egui::RichText::new("Quick access").size(16.0).strong());
                                ui.add_space(14.0);
                                for (page, symbol, label) in [
                                    (Page::Devices, "⌁", "Manage your devices"),
                                    (Page::Transfers, "⇧", "Send and receive files"),
                                    (Page::Clipboard, "▤", "Clipboard sharing"),
                                    (Page::Tools, "⌘", "Explore device tools"),
                                ] {
                                    if ui.button(format!("{symbol}    {label}    ↗")).clicked() { self.selected = page; }
                                    ui.add_space(5.0);
                                }
                            });
                            panel(&mut cols[1], |ui| {
                                ui.label(egui::RichText::new("Designed around you").size(16.0).strong());
                                ui.add_space(14.0);
                                feature_row(ui, "⌁", "Private by default", "No mandatory account or cloud relay.", "CORE");
                                ui.add_space(16.0);
                                feature_row(ui, "◇", "Your permission, always", "Capabilities are opt-in.", "CONTROL");
                                ui.add_space(16.0);
                                feature_row(ui, "↔", "Made for every screen", "Windows, Android and iOS.", "CROSS-PLATFORM");
                            });
                        });
                    }
                    Page::Devices => {
                        panel(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.vertical(|ui| {
                                    eyebrow(ui, "Device discovery");
                                    ui.add_space(6.0);
                                    ui.label(egui::RichText::new("Bring your devices together").size(20.0).strong());
                                    muted(ui, "Discover devices on your local network and pair only the ones you trust.");
                                });
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if primary_button(ui, "＋  Pair device") { self.show_pair_dialog = true; }
                                });
                            });
                        });
                        ui.add_space(16.0);
                        panel(ui, |ui| {
                            ui.vertical_centered(|ui| {
                                ui.add_space(24.0);
                                ui.label(egui::RichText::new("⌁").size(42.0).color(ACCENT));
                                ui.add_space(8.0);
                                ui.label(egui::RichText::new("No devices here yet").size(19.0).strong());
                                muted(ui, "Your trusted devices will appear here after pairing.");
                                ui.add_space(15.0);
                                if primary_button(ui, "Start pairing") { self.show_pair_dialog = true; }
                                ui.add_space(20.0);
                            });
                        });
                        ui.add_space(16.0);
                        #[cfg(windows)]
                        {
                            panel(ui, |ui| {
                                eyebrow(ui, "DISCOVERED ON YOUR NETWORK");
                                ui.add_space(8.0);
                                let trusted_devices = self.trust_store.list();
                                let visible_peers = self.discovered_peers.iter().enumerate()
                                    .filter(|(_, peer)| !trusted_devices.iter().any(|device| device.descriptor.id.0.to_string() == peer.record.device_id))
                                    .collect::<Vec<_>>();
                                if visible_peers.is_empty() {
                                    muted(ui, "No new devices found. Trusted devices are listed below.");
                                } else {
                                    let mut pair_target = None;
                                    for (index, peer) in visible_peers {
                                        ui.horizontal(|ui| {
                                            ui.vertical(|ui| {
                                                ui.label(egui::RichText::new(&peer.record.display_name).color(TEXT).strong());
                                                muted(ui, &format!("{} · {}", peer.address, &peer.record.public_key_fingerprint[..8]));
                                            });
                                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                if primary_button(ui, "Connect") { pair_target = Some(index); }
                                                muted(ui, "UNVERIFIED");
                                            });
                                        });
                                        ui.add_space(6.0);
                                    }
                                    if let Some(index) = pair_target {
                                        let peer = self.discovered_peers[index].clone();
                                        let expected = self.trust_store.list().into_iter()
                                            .find(|device| device.public_key_fingerprint.eq_ignore_ascii_case(&peer.record.public_key_fingerprint))
                                            .map(|device| device.public_key_fingerprint.clone());
                                        start_pairing(peer, expected, self.network_tx.clone(), self.pairing_tx.clone());
                                        self.network_status = "Starting authenticated Noise XX pairing…".into();
                                    }
                                }
                            });

                        }
                        ui.add_space(16.0);
                        #[cfg(windows)]
                        {
                            let mut revoke_device = None;
                            panel(ui, |ui| {
                                eyebrow(ui, "TRUSTED DEVICES");
                                if let Some(error) = &self.trust_store_error {
                                    ui.label(egui::RichText::new(format!("Registry is unreadable; approvals and revocations are disabled: {error}"))
                                        .size(11.0).color(egui::Color32::from_rgb(239, 68, 68)));
                                }
                                let devices = self.trust_store.list();
                                if devices.is_empty() {
                                    muted(ui, "No trusted devices yet. Pairing requires matching verification codes and approval on both devices.");
                                } else {
                                    for device in devices {
                                        ui.horizontal(|ui| {
                                            ui.vertical(|ui| {
                                                ui.label(egui::RichText::new(&device.descriptor.display_name).strong().color(TEXT));
                                                muted(ui, &format!("{:?} · {}", device.descriptor.platform, &device.public_key_fingerprint[..16]));
                                            });
                                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                if ui.button(egui::RichText::new("Revoke").color(egui::Color32::from_rgb(239, 68, 68))).clicked() {
                                                    revoke_device = Some(device.descriptor.id);
                                                }
                                            });
                                        });
                                        ui.add_space(5.0);
                                    }
                                }
                            });
                            if let Some(id) = revoke_device {
                                match self.trust_store.revoke_and_save(&id, trust_store_path()) {
                                    Ok(device) => {
                                        for session in &self.active_sessions {
                                            if session.remote_fingerprint.eq_ignore_ascii_case(&device.public_key_fingerprint) {
                                                session.shutdown();
                                            }
                                        }
                                        self.active_sessions.retain(|session| session.remote_fingerprint != device.public_key_fingerprint);
                                        self.network_status = format!("Trust revoked for {}. Its active session was closed.", device.descriptor.display_name);
                                    }
                                    Err(error) => self.network_status = format!("Could not revoke device: {error}"),
                                }
                            }

                        }
                        ui.add_space(16.0);
                        panel(ui, |ui| {
                            eyebrow(ui, "SECURE TRANSPORT");
                            ui.label(egui::RichText::new("Authenticated local sessions").strong().size(17.0));
                            muted(ui, "New devices must be accepted on the receiving device. Previously trusted devices reconnect automatically.");
                            ui.label(egui::RichText::new(&self.network_status).size(12.0).color(AMBER));
                        });
                    }
                    Page::Transfers => {
                        #[cfg(windows)]
                        {
                            let targets = self.trust_store.list().into_iter().filter_map(|device| {
                                self.active_sessions.iter()
                                    .find(|session| session.remote_fingerprint.eq_ignore_ascii_case(&device.public_key_fingerprint))
                                    .map(|session| (device.public_key_fingerprint.clone(), device.descriptor.display_name.clone(), Arc::clone(session)))
                            }).collect::<Vec<_>>();
                            let mut accept_incoming: Option<(usize, PathBuf)> = None;
                            let mut reject_incoming: Option<usize> = None;
                            if !self.pending_incoming_transfers.is_empty() {
                                panel(ui, |ui| {
                                    eyebrow(ui, "INCOMING FILE REQUESTS");
                                    ui.add_space(6.0);
                                    for (index, pending) in self.pending_incoming_transfers.iter().enumerate() {
                                        ui.horizontal(|ui| {
                                            ui.vertical(|ui| {
                                                ui.label(egui::RichText::new(&pending.offer.file_name).strong().color(TEXT));
                                                muted(ui, &format!(
                                                    "{} · {:.2} MB · encrypted and SHA-256 verified",
                                                    &pending.session.remote_fingerprint[..8.min(pending.session.remote_fingerprint.len())],
                                                    pending.offer.file_size as f64 / 1_048_576.0
                                                ));
                                            });
                                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                if primary_button(ui, "Decline") { reject_incoming = Some(index); }
                                                if primary_button(ui, "Accept…") {
                                                    if let Some(path) = rfd::FileDialog::new()
                                                        .set_file_name(&pending.offer.file_name)
                                                        .save_file()
                                                    {
                                                        accept_incoming = Some((index, path));
                                                    }
                                                }
                                            });
                                        });
                                        ui.add_space(8.0);
                                    }
                                });
                                if let Some(index) = reject_incoming {
                                    if index < self.pending_incoming_transfers.len() {
                                        let pending = self.pending_incoming_transfers.remove(index);
                                        reject_incoming_transfer(&pending.session, &pending.offer);
                                        self.transfer_status = format!("Declined {}.", pending.offer.file_name);
                                    }
                                }
                                if let Some((index, path)) = accept_incoming {
                                    if index < self.pending_incoming_transfers.len() {
                                        let pending = self.pending_incoming_transfers.remove(index);
                                        self.transfer_busy = true;
                                        self.transfer_progress = 0.0;
                                        self.transfer_speed = 0.0;
                                        let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
                                        self.transfer_cancel = Some(Arc::clone(&cancel));
                                        self.transfer_status = format!("Receiving {}…", pending.offer.file_name);
                                        start_receive_file_transfer(
                                            pending.session,
                                            pending.offer,
                                            path,
                                            self.transfer_tx.clone(),
                                            cancel,
                                        );
                                    }
                                }
                            }
                            if self.selected_transfer_fingerprint.is_none() {
                                self.selected_transfer_fingerprint = targets.first().map(|target| target.0.clone());
                            }
                            panel(ui, |ui| {
                                eyebrow(ui, "SECURE FILE TRANSFER");
                                ui.add_space(8.0);
                                ui.label(egui::RichText::new("Send a file to a trusted device").size(20.0).strong());
                                muted(ui, "Files are sent over the existing authenticated, encrypted local session.");
                                ui.add_space(14.0);
                                let selected_name = targets.iter()
                                    .find(|target| Some(&target.0) == self.selected_transfer_fingerprint.as_ref())
                                    .map(|target| target.1.as_str())
                                    .unwrap_or("No connected trusted device");
                                egui::ComboBox::from_id_salt("bridge_transfer_target")
                                    .selected_text(selected_name)
                                    .width(260.0)
                                    .show_ui(ui, |ui| {
                                        for (fingerprint, name, _) in &targets {
                                            ui.selectable_value(&mut self.selected_transfer_fingerprint, Some(fingerprint.clone()), name);
                                        }
                                    });
                                ui.add_space(12.0);
                                let can_send = !self.transfer_busy && !targets.is_empty()
                                    && self.selected_transfer_fingerprint.as_ref().is_some_and(|fingerprint| targets.iter().any(|target| &target.0 == fingerprint));
                                if ui.add_enabled(can_send, egui::Button::new(egui::RichText::new("＋  Choose file and send").strong().color(TEXT))
                                    .fill(egui::Color32::from_rgb(12, 105, 121))
                                    .corner_radius(egui::CornerRadius::same(8))
                                    .min_size(egui::vec2(210.0, 38.0))).clicked() {
                                    let selected_session = targets.iter()
                                        .find(|target| Some(&target.0) == self.selected_transfer_fingerprint.as_ref())
                                        .map(|target| Arc::clone(&target.2));
                                    if let (Some(session), Some(path)) = (selected_session, rfd::FileDialog::new().pick_file()) {
                                        self.transfer_busy = true;
                                        self.transfer_progress = 0.0;
                                        self.transfer_speed = 0.0;
                                        let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
                                        self.transfer_cancel = Some(Arc::clone(&cancel));
                                        self.transfer_status = format!("Preparing {}…", path.file_name().and_then(|name| name.to_str()).unwrap_or("file"));
                                        start_file_transfer(session, path, self.transfer_tx.clone(), cancel);
                                    }
                                }
                                ui.add_space(14.0);
                                if self.transfer_busy {
                                    ui.add(egui::ProgressBar::new(self.transfer_progress).show_percentage().animate(true));
                                    ui.horizontal(|ui| {
                                        muted(ui, &format!("{:.2} MB/s", self.transfer_speed / 1_048_576.0));
                                        if ui.button("Cancel transfer").clicked() {
                                            if let Some(cancel) = &self.transfer_cancel { cancel.store(true, Ordering::Release); }
                                            self.transfer_status = "Cancelling transfer…".into();
                                        }
                                    });
                                }
                                muted(ui, &self.transfer_status);
                                if targets.is_empty() {
                                    ui.add_space(8.0);
                                    muted(ui, "No active secure sessions. Connect to a trusted device on the Devices page first.");
                                }
                            });
                            ui.add_space(12.0);
                            panel(ui, |ui| {
                                eyebrow(ui, "TRANSFER SAFETY");
                                feature_row(ui, "◇", "Encrypted in transit", "Uses the authenticated Noise session; no cloud relay.", "ACTIVE");
                                ui.add_space(10.0);
                                feature_row(ui, "✓", "Integrity checked", "SHA-256 verified before the receiver publishes the file.", "ACTIVE");
                                ui.add_space(10.0);
                                feature_row(ui, "▣", "No accidental overwrite", "The receiver chooses a collision-safe destination name.", "ACTIVE");
                            });
                        }
                        #[cfg(not(windows))]
                        panel(ui, |ui| {
                            ui.vertical_centered(|ui| {
                                ui.label(egui::RichText::new("File transfer is available in the Windows build").strong());
                                muted(ui, "Android and iOS transfer workflows are being connected next.");
                            });
                        });
                    }
                    Page::Clipboard => {
                        panel(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.vertical(|ui| {
                                    eyebrow(ui, "Universal clipboard");
                                    ui.label(egui::RichText::new("Share text securely").size(20.0).strong());
                                    muted(ui, "Send text to your connected, trusted devices.");
                                });
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    ui.checkbox(&mut self.clipboard_sync, "Enable sharing");
                                });
                            });
                        });
                        ui.add_space(14.0);
                        panel(ui, |ui| {
                            ui.label(egui::RichText::new("Send text").size(16.0).strong());
                            #[cfg(windows)]
                            ui.add_enabled_ui(self.clipboard_sync, |ui| {
                                ui.add(egui::TextEdit::multiline(&mut self.clipboard_draft)
                                    .desired_rows(4)
                                    .hint_text("Type or paste text to share…"));
                                let connected = self.active_sessions.iter().filter(|session| session.is_alive()).count();
                                if ui.add_enabled(!self.clipboard_draft.is_empty() && connected > 0,
                                    egui::Button::new(format!("Send to {connected} connected device(s)"))).clicked() {
                                    let text = self.clipboard_draft.as_bytes();
                                    if text.len() > MAX_CLIPBOARD_TEXT_BYTES {
                                        self.clipboard_status = "Text exceeds the 16 KiB clipboard limit.".into();
                                    } else {
                                        let mut packet = Vec::with_capacity(text.len() + 1);
                                        packet.push(CLIPBOARD_PACKET_TAG);
                                        packet.extend_from_slice(text);
                                        let mut sent = 0usize;
                                        for session in &self.active_sessions {
                                            if session.is_alive() && session.send(&packet).is_ok() { sent += 1; }
                                        }
                                        self.clipboard_status = format!("Sent text to {sent} connected device(s).");
                                        if sent > 0 { self.clipboard_draft.clear(); }
                                    }
                                }
                            });
                            #[cfg(not(windows))]
                            muted(ui, "Clipboard sharing is currently available in the Windows build.");
                        });
                        ui.add_space(12.0);
                        panel(ui, |ui| {
                            ui.label(egui::RichText::new("Received text").size(16.0).strong());
                            if let Some(text) = &self.clipboard_received {
                                let mut received_text = text.clone();
                                ui.add(egui::TextEdit::multiline(&mut received_text).desired_rows(4));
                                muted(ui, "Select the text above and press Ctrl+C to copy it.");
                            } else {
                                muted(ui, "No shared text received yet.");
                            }
                            muted(ui, &self.clipboard_status);
                        });
                        muted(ui, "Bridge never reads your system clipboard automatically. Sharing requires an explicit send action.");
                    }
                    Page::Tools => {
                        let tools = [
                            ("♫", "Phone as microphone", "Use your phone as a wireless input for your PC.", "PLANNED"),
                            ("▣", "Phone as webcam", "Bring your mobile camera into desktop apps.", "PLANNED"),
                            ("◖", "PC audio on phone", "Listen to your computer from another device.", "PLANNED"),
                            ("⌖", "Remote input", "Use a trusted phone or tablet as a controller.", "PLANNED"),
                            ("▤", "Phone notifications", "See selected mobile notifications on desktop.", "PLANNED"),
                            ("▦", "Second screen", "Secure Android attestation and renewable display lease lifecycle.", "LEASE-GATED"),
                        ];
                        #[cfg(windows)]
                        {
                            let active_lease = self.display_lease.lock().ok()
                                .and_then(|active| active.as_ref().map(|(key, lease)| (key.clone(), lease.clone())));
                            let lease_available = active_lease.is_some();
                            let mut connect_clicked = false;
                            let mut disconnect_clicked = false;
                            panel(ui, |ui| {
                                eyebrow(ui, "SECOND SCREEN · AUTHORIZATION");
                                ui.label(egui::RichText::new("Display-service lease").strong().size(16.0).color(TEXT));
                                muted(ui, &self.display_lease_status);
                                muted(ui, "The lease is bound to this Windows process and the authenticated Android device. Renewal stops when the session ends; an abandoned lease expires automatically.");
                                ui.add_space(10.0);
                                ui.horizontal(|ui| {
                                    if ui.add_enabled(lease_available, egui::Button::new("Enable virtual display")).clicked() {
                                        connect_clicked = true;
                                    }
                                    if ui.add_enabled(lease_available, egui::Button::new("Stop virtual display")).clicked() {
                                        disconnect_clicked = true;
                                    }
                                });
                                muted(ui, "These controls request driver operations only. They do not yet create or stream a Windows extended monitor.");
                            });
                            if connect_clicked || disconnect_clicked {
                                if let Some((session_key, lease)) = active_lease {
                                    let session = self.active_sessions.iter()
                                        .find(|session| {
                                            format!("{}:{}", session.remote_fingerprint, session.session_id) == session_key
                                        })
                                        .cloned();
                                    if let Some(session) = session {
                                        if let Some(stop) = self.display_lease_stop.as_ref().cloned() {
                                            if let Err(error) = start_display_driver_operation(
                                                session,
                                                session_key,
                                                lease,
                                                connect_clicked,
                                                stop,
                                                Arc::clone(&self.display_lease),
                                                self.display_lease_tx.clone(),
                                            ) {
                                                self.display_lease_status = error;
                                            } else {
                                                self.display_lease_status = if connect_clicked {
                                                    "Sending lease-authorized CONNECT request to the Windows display service…".into()
                                                } else {
                                                    "Sending lease-authorized DISCONNECT request to the Windows display service…".into()
                                                };
                                            }
                                        } else {
                                            self.display_lease_status = "Display lease lifecycle is not active.".into();
                                        }
                                    } else {
                                        self.display_lease_status = "The Android session for this display lease is no longer active.".into();
                                    }
                                }
                            }
                            ui.add_space(12.0);
                        }
                        for row in tools.chunks(2) {
                            ui.columns(2, |cols| {
                                for (index, item) in row.iter().enumerate() {
                                    panel(&mut cols[index], |ui| {
                                        ui.label(egui::RichText::new(item.0).size(25.0).color(ACCENT));
                                        ui.add_space(8.0);
                                        ui.label(egui::RichText::new(item.1).strong().size(15.0));
                                        muted(ui, item.2);
                                        ui.add_space(10.0);
                                        muted(ui, item.3);
                                    });
                                }
                            });
                            ui.add_space(12.0);
                        }
                    }
                    Page::Settings => {
                        panel(ui, |ui| {
                            eyebrow(ui, "Preferences");
                            ui.add_space(8.0);
                            ui.label(egui::RichText::new("Your Bridge experience").size(18.0).strong());
                            ui.add_space(14.0);
                            ui.checkbox(&mut self.launch_at_login, "Launch Bridge when I sign in");
                            ui.add_space(8.0);
                            ui.checkbox(&mut self.notifications, "Show desktop notifications");
                            ui.add_space(8.0);
                            ui.checkbox(&mut self.auto_accept, "Automatically accept requests from trusted devices");
                        });
                        ui.add_space(14.0);
                        panel(ui, |ui| {
                            eyebrow(ui, "Privacy & security");
                            ui.add_space(8.0);
                            feature_row(ui, "◇", "Local-first connection", "Designed to work without a Bridge account.", "BY DESIGN");
                            ui.add_space(14.0);
                            feature_row(ui, "⌑", "Permission-based access", "Choose what each paired device can do.", "PLANNED");
                            ui.add_space(14.0);
                            feature_row(ui, "⚿", "Encrypted transport", "Secure channel foundation is in the shared core.", "FOUNDATION");
                        });
                        ui.add_space(14.0);
                        #[cfg(windows)]
                        {
                            panel(ui, |ui| {
                                eyebrow(ui, "Software updates");
                                ui.add_space(6.0);
                                ui.horizontal(|ui| {
                                    ui.label("Update channel");
                                    if ui.selectable_label(self.update_channel == "development", "Development").clicked() {
                                        self.update_channel = "development".into();
                                        save_update_channel(&self.update_channel);
                                        self.update_available = None;
                                        self.update_status = "Development channel selected.".into();
                                    }
                                    if ui.selectable_label(self.update_channel == "stable", "Stable").clicked() {
                                        self.update_channel = "stable".into();
                                        save_update_channel(&self.update_channel);
                                        self.update_available = None;
                                        self.update_status = "Stable channel selected.".into();
                                    }
                                });
                                muted(ui, if self.update_channel == "stable" {
                                    "Stable releases will appear when the Stable channel launches."
                                } else {
                                    "Development builds include the newest features and may be less stable."
                                });
                                if primary_button(ui, "Check for updates") {
                                    self.update_available = None;
                                    check_for_update(self.update_channel.clone(), self.update_tx.clone());
                                }
                                muted(ui, &self.update_status);
                                if let Some((version, installer)) = self.update_available.clone() {
                                    if primary_button(ui, &format!("Install Bridge {version} and restart")) {
                                        let launched = std::process::Command::new("msiexec")
                                            .arg("/i").arg(&installer)
                                            .args(["/passive", "/norestart"])
                                            .spawn().is_ok();
                                        if launched {
                                            std::process::exit(0);
                                        } else {
                                            self.update_status = "Could not launch the Windows installer.".into();
                                        }
                                    }
                                }
                            });
                            ui.add_space(14.0);
                        }

                        panel(ui, |ui| {
                            eyebrow(ui, "About");
                            ui.label(egui::RichText::new("Bridge for Windows").strong());
                            muted(ui, &format!("Version {} · Native Rust interface", option_env!("BRIDGE_BUILD_VERSION").unwrap_or(env!("CARGO_PKG_VERSION"))));
                        });
                    }
                }
                ui.add_space(24.0);
                muted(ui, "BRIDGE PREVIEW  ·  Interface controls are interactive; device networking is being built separately.");
            });
        });

        #[cfg(windows)]
        if !self.pending_pairings.is_empty() {
            let mut approve_index = None;
            let mut reject_index = None;
            egui::Window::new("Pairing request")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                .show(ctx, |ui| {
                    ui.set_min_width(380.0);
                    for (index, pending) in self.pending_pairings.iter_mut().enumerate() {
                        eyebrow(ui, "NOISE XX · AUTHENTICATED PEER");
                        ui.label(egui::RichText::new("Allow this device to connect?").size(18.0).strong());
                        ui.label(egui::RichText::new(&pending.descriptor.display_name).size(20.0).strong());
                        muted(ui, &format!("{:?} · authenticated identity · {}", pending.descriptor.platform, &pending.connection.remote_fingerprint[..16]));
                        muted(ui, "Only accept if you recognize this device. Its connection is encrypted and its identity will be saved on this PC.");
                        ui.horizontal(|ui| {
                            if primary_button(ui, "Accept") { approve_index = Some(index); }
                            if ui.button("Reject").clicked() { reject_index = Some(index); }
                        });
                        ui.separator();
                    }
                });
            if let Some(index) = reject_index {
                let mut pending = self.pending_pairings.remove(index);
                let _ = send_pairing_decision(&mut pending.connection, "rejected");
                self.network_status = "Pairing request rejected.".into();
            }
            if let Some(index) = approve_index {
                let mut pending = self.pending_pairings.remove(index);
                if let Some(error) = &self.trust_store_error {
                    self.network_status = format!("Pairing approval blocked because the trust registry is unreadable: {error}");
                    return;
                }
                let result = SasConfirmation::user_approved(&pending.connection.remote_fingerprint)
                    .and_then(|confirmation| self.trust_store.trust_and_save(
                        pending.descriptor.clone(), confirmation, now_ms(), trust_store_path()
                    ));
                match result {
                    Ok(()) => {
                        if let Err(error) = send_pairing_decision(&mut pending.connection, "accepted") {
                            let _ = self.trust_store.revoke_and_save(&pending.descriptor.id, trust_store_path());
                            self.network_status = format!("Could not confirm pairing: {error}");
                        } else {
                            match DesktopSession::start(pending.connection, pending.descriptor.attestation_public_key.clone()) {
                                Ok(session) => {
                                    for existing in &self.active_sessions {
                                        if existing.remote_fingerprint.eq_ignore_ascii_case(&session.remote_fingerprint) {
                                            existing.shutdown();
                                        }
                                    }
                                    self.active_sessions.retain(|existing| {
                                        existing.remote_fingerprint != session.remote_fingerprint
                                    });
                                    self.active_sessions.push(session);
                                }
                                Err(error) => self.network_status = format!("Could not start secure session dispatcher: {error}"),
                            }
                            self.network_status = format!("{} is now connected and trusted.", pending.descriptor.display_name);
                        }
                    }
                    Err(error) => {
                        let _ = send_pairing_decision(&mut pending.connection, "rejected");
                        self.network_status = format!("Pairing was not trusted: {error}");
                    }
                }
            }
        }
        if self.show_pair_dialog {
            egui::Window::new("Secure pairing")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                .show(ctx, |ui| {
                    ui.set_min_width(340.0);
                    eyebrow(ui, "AUTHENTICATED DEVICE PAIRING");
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new("Secure pairing is active").size(19.0).strong());
                    muted(ui, "Bridge automatically discovers nearby devices. Choose one and press Connect; the other device will show an approval popup.");
                    ui.add_space(10.0);
                    muted(ui, "Previously trusted devices reconnect automatically. New devices are never trusted without approval on the receiving device.");
                    ui.add_space(12.0);
                    if ui.button("Close").clicked() { self.show_pair_dialog = false; }
                });
        }
    }
}

#[cfg(windows)]
fn inject_display_pointer_event(event: DisplayInputEvent) -> Result<(), String> {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_ABSOLUTE,
        MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MOVE,
        MOUSEEVENTF_VIRTUALDESK, MOUSEINPUT,
    };
    let mut flags = MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK;
    match event.action {
        DisplayInputAction::Down => flags |= MOUSEEVENTF_MOVE | MOUSEEVENTF_LEFTDOWN,
        DisplayInputAction::Move => flags |= MOUSEEVENTF_MOVE,
        DisplayInputAction::Up => flags |= MOUSEEVENTF_MOVE | MOUSEEVENTF_LEFTUP,
        DisplayInputAction::Cancel => flags |= MOUSEEVENTF_LEFTUP,
    }
    let (absolute_x, absolute_y) = bridge_monitor_absolute_coordinates(event.x, event.y)?;
    let input = INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 { mi: MOUSEINPUT {
            dx: absolute_x, dy: absolute_y, mouseData: 0,
            dwFlags: flags, time: 0, dwExtraInfo: 0,
        }},
    };
    let inserted = unsafe { SendInput(1, &input, std::mem::size_of::<INPUT>() as i32) };
    if inserted != 1 {
        return Err(format!("Windows rejected pointer input (error {})", unsafe {
            windows_sys::Win32::Foundation::GetLastError()
        }));
    }
    Ok(())
}


#[cfg(windows)]
fn bridge_monitor_absolute_coordinates(x: u16, y: u16) -> Result<(i32, i32), String> {
    use windows_sys::Win32::Graphics::Gdi::{
        EnumDisplayDevicesW, EnumDisplayMonitors, GetMonitorInfoW, DISPLAY_DEVICEW,
        HDC, HMONITOR, MONITORINFO, MONITORINFOEXW, RECT,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetSystemMetrics, SM_CXSCREEN, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN,
        SM_CYSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
    };

    struct MonitorSearch { rect: Option<(i32, i32, i32, i32)> }

    unsafe extern "system" fn find_bridge_monitor(
        monitor: HMONITOR,
        _dc: HDC,
        _clip: *mut RECT,
        data: isize,
    ) -> i32 {
        let search = &mut *(data as *mut MonitorSearch);
        let mut info: MONITORINFOEXW = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
        if GetMonitorInfoW(monitor, &mut info as *mut MONITORINFOEXW as *mut MONITORINFO) == 0 {
            return 1;
        }
        let mut device: DISPLAY_DEVICEW = std::mem::zeroed();
        device.cb = std::mem::size_of::<DISPLAY_DEVICEW>() as u32;
        if EnumDisplayDevicesW(info.szDevice.as_ptr(), 0, &mut device, 0) == 0 {
            return 1;
        }
        let description = String::from_utf16_lossy(&device.DeviceString)
            .trim_end_matches('\0').to_ascii_lowercase();
        let device_id = String::from_utf16_lossy(&device.DeviceID)
            .trim_end_matches('\0').to_ascii_lowercase();
        if description.contains("bridge virtual display")
            || description.contains("bridge second screen")
            || device_id.contains("root\\bridgeindirectdisplay")
        {
            search.rect = Some((
                info.rcMonitor.left, info.rcMonitor.top,
                info.rcMonitor.right, info.rcMonitor.bottom,
            ));
            return 0;
        }
        1
    }

    let mut search = MonitorSearch { rect: None };
    let enumerated = unsafe {
        EnumDisplayMonitors(
            std::ptr::null_mut(),
            std::ptr::null(),
            Some(find_bridge_monitor),
            &mut search as *mut MonitorSearch as isize,
        )
    };
    if enumerated == 0 {
        return Err("Windows could not enumerate active displays".into());
    }
    let monitor = search.rect.ok_or_else(|| {
        "Bridge virtual monitor is not active; refusing to inject pointer input onto another display".to_string()
    })?;

    let virtual_left = unsafe { GetSystemMetrics(SM_XVIRTUALSCREEN) };
    let virtual_top = unsafe { GetSystemMetrics(SM_YVIRTUALSCREEN) };
    let virtual_width = unsafe { GetSystemMetrics(SM_CXVIRTUALSCREEN) };
    let virtual_height = unsafe { GetSystemMetrics(SM_CYVIRTUALSCREEN) };
    let _ = (SM_CXSCREEN, SM_CYSCREEN);
    map_monitor_point_to_virtual_desktop(
        x, y, monitor,
        (virtual_left, virtual_top, virtual_left + virtual_width, virtual_top + virtual_height),
    )
}

#[cfg(windows)]
fn map_monitor_point_to_virtual_desktop(
    x: u16,
    y: u16,
    monitor: (i32, i32, i32, i32),
    virtual_desktop: (i32, i32, i32, i32),
) -> Result<(i32, i32), String> {
    let (left, top, right, bottom) = monitor;
    let (virtual_left, virtual_top, virtual_right, virtual_bottom) = virtual_desktop;
    let monitor_width = i64::from(right) - i64::from(left);
    let monitor_height = i64::from(bottom) - i64::from(top);
    let virtual_width = i64::from(virtual_right) - i64::from(virtual_left);
    let virtual_height = i64::from(virtual_bottom) - i64::from(virtual_top);
    if monitor_width <= 0 || monitor_height <= 0 || virtual_width <= 1 || virtual_height <= 1
        || left < virtual_left || top < virtual_top
        || right > virtual_right || bottom > virtual_bottom
    {
        return Err("Windows returned invalid Bridge or virtual-desktop bounds".into());
    }

    let monitor_x = i64::from(left)
        + (i64::from(x) * (monitor_width - 1) + 32_767) / 65_535;
    let monitor_y = i64::from(top)
        + (i64::from(y) * (monitor_height - 1) + 32_767) / 65_535;
    let absolute_x = ((monitor_x - i64::from(virtual_left)) * 65_535
        + (virtual_width - 1) / 2) / (virtual_width - 1);
    let absolute_y = ((monitor_y - i64::from(virtual_top)) * 65_535
        + (virtual_height - 1) / 2) / (virtual_height - 1);
    Ok((
        absolute_x.clamp(0, 65_535) as i32,
        absolute_y.clamp(0, 65_535) as i32,
    ))
}

#[cfg(all(test, windows))]
mod display_pointer_mapping_tests {
    use super::map_monitor_point_to_virtual_desktop;

    #[test]
    fn maps_bridge_monitor_to_its_position_on_the_virtual_desktop() {
        let bounds = (1920, 0, 3000, 2340);
        let desktop = (0, 0, 3000, 2340);
        assert_eq!(map_monitor_point_to_virtual_desktop(0, 0, bounds, desktop).unwrap(), (41_943, 0));
        assert_eq!(map_monitor_point_to_virtual_desktop(u16::MAX, u16::MAX, bounds, desktop).unwrap(), (65_535, 65_535));
    }

    #[test]
    fn supports_monitors_left_of_the_primary_display() {
        let bounds = (-1080, 0, 0, 2340);
        let desktop = (-1080, 0, 1920, 2340);
        assert_eq!(map_monitor_point_to_virtual_desktop(0, 0, bounds, desktop).unwrap(), (0, 0));
        assert!(map_monitor_point_to_virtual_desktop(u16::MAX, 0, bounds, desktop).unwrap().0 < 65_535);
    }

    #[test]
    fn rejects_invalid_or_outside_monitor_bounds() {
        assert!(map_monitor_point_to_virtual_desktop(0, 0, (0, 0, 0, 100), (0, 0, 100, 100)).is_err());
        assert!(map_monitor_point_to_virtual_desktop(0, 0, (100, 0, 200, 100), (0, 0, 100, 100)).is_err());
    }
}

#[cfg(windows)]
fn inject_display_pointer_release() -> Result<(), String> {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_LEFTUP, MOUSEINPUT,
    };
    let input = INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 { mi: MOUSEINPUT {
            dx: 0, dy: 0, mouseData: 0, dwFlags: MOUSEEVENTF_LEFTUP,
            time: 0, dwExtraInfo: 0,
        }},
    };
    let inserted = unsafe { SendInput(1, &input, std::mem::size_of::<INPUT>() as i32) };
    if inserted == 1 { Ok(()) } else {
        Err(format!("Windows could not release the second-screen pointer (error {})", unsafe {
            windows_sys::Win32::Foundation::GetLastError()
        }))
    }
}

#[cfg(windows)]
fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default().as_millis().min(u64::MAX as u128) as u64
}

fn main() -> eframe::Result<()> {
    let icon = image::load_from_memory(include_bytes!("../../../assets/bridge-app-icon.png"))
        .expect("Bridge app icon must be a valid PNG").to_rgba8();
    let (icon_width, icon_height) = icon.dimensions();
    let icon_data = egui::IconData { rgba: icon.into_raw(), width: icon_width, height: icon_height };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Bridge")
            .with_icon(Arc::new(icon_data))
            .with_inner_size([1280.0, 840.0])
            .with_min_inner_size([900.0, 650.0]),
        ..Default::default()
    };
    eframe::run_native("Bridge", options, Box::new(|_cc| Ok(Box::<BridgeApp>::default())))
}

fn secure_identity_status() -> String {
    #[cfg(windows)]
    {
        return match identity_store::load_or_create() {
            Ok(_private_key) => "Device identity protected by Windows Credential Manager".into(),
            Err(error) => format!("Secure device identity unavailable: {error}"),
        };
    }
    #[cfg(not(windows))]
    {
        "Secure device identity is available in the Windows build".into()
    }
}
