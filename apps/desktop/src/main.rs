#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(windows)]
mod identity_store;

use eframe::egui;
#[cfg(windows)]
use bridge_network::{accept_noise_xx, connect_noise_xx, connect_to_peer, local_key_fingerprint, peer_from_event, DiscoveredPeer, DiscoveryRecord, LocalDiscovery, NoiseConnection};
#[cfg(windows)]
use bridge_core::trust::{SasConfirmation, TrustStore};
#[cfg(windows)]
use bridge_protocol::{DeviceDescriptor, DeviceId, Platform, PROTOCOL_VERSION};
#[cfg(windows)]
use bridge_transfer::{decode_packet, encode_packet, offer_for_file, OutgoingFile, TransferControl, TransferPacket};
#[cfg(windows)]
use std::net::TcpListener;
#[cfg(windows)]
use std::path::PathBuf;
use std::sync::Arc;
#[cfg(windows)]
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver};
#[cfg(windows)]
use std::sync::mpsc::Sender;
#[cfg(windows)]
use std::thread;
use std::time::Duration;

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
struct PendingApproval {
    connection: NoiseConnection,
    descriptor: DeviceDescriptor,
    already_trusted: bool,
    /// Incoming sessions need an approval acknowledgement when trust already exists.
    incoming: bool,
}

#[cfg(windows)]
struct DesktopSession {
    connection: Arc<std::sync::Mutex<NoiseConnection>>,
    incoming: std::sync::Mutex<Receiver<Vec<u8>>>,
    stop: Arc<std::sync::atomic::AtomicBool>,
    pub remote_fingerprint: String,
}

#[cfg(windows)]
impl DesktopSession {
    fn start(connection: NoiseConnection) -> Result<Arc<Self>, String> {
        let reader_stream = connection.stream.try_clone().map_err(|error| error.to_string())?;
        reader_stream.set_read_timeout(None).map_err(|error| error.to_string())?;
        let remote_fingerprint = connection.remote_fingerprint.clone();
        let connection = Arc::new(std::sync::Mutex::new(connection));
        let reader_connection = Arc::clone(&connection);
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let reader_stop = Arc::clone(&stop);
        let (sender, incoming) = mpsc::channel();
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
                    if sender.send(plaintext).is_err() { break; }
                }
            })
            .map_err(|error| error.to_string())?;
        Ok(Arc::new(Self { connection, incoming: std::sync::Mutex::new(incoming), stop, remote_fingerprint }))
    }

    fn send(&self, payload: &[u8]) -> Result<(), String> {
        if payload.is_empty() || payload.len() > bridge_network::MAX_FRAME_BYTES - 16 {
            return Err("session payload size is invalid".into());
        }
        let mut guard = self.connection.lock().map_err(|_| "session connection lock poisoned")?;
        let mut encrypted = vec![0_u8; bridge_network::MAX_FRAME_BYTES];
        let length = guard.transport.write_message(payload, &mut encrypted).map_err(|error| error.to_string())?;
        bridge_network::write_frame(&mut guard.stream, &encrypted[..length]).map_err(|error| error.to_string())
    }

    fn receive_timeout(&self, timeout: Duration) -> Result<Vec<u8>, String> {
        self.incoming.lock().map_err(|_| "session receiver lock poisoned")?
            .recv_timeout(timeout).map_err(|error| error.to_string())
    }
}

#[cfg(windows)]
impl Drop for DesktopSession {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Ok(connection) = self.connection.lock() {
            let _ = connection.stream.shutdown(std::net::Shutdown::Both);
        }
    }
}

#[cfg(windows)]
enum TransferUiEvent {
    Status(String),
    Progress { sent: u64, total: u64 },
    Complete(String),
    Failed(String),
}

#[cfg(windows)]
fn start_file_transfer(
    session: Arc<DesktopSession>,
    path: PathBuf,
    events: Sender<TransferUiEvent>,
) {
    thread::Builder::new().name("bridge-file-send".into()).spawn(move || {
        let result = (|| -> Result<String, String> {
            let offer = offer_for_file(&path).map_err(|error| error.to_string())?;
            let file_name = offer.file_name.clone();
            let transfer_id = offer.transfer_id;
            let packet = encode_packet(&TransferPacket::Control(
                TransferControl::FileOffer { offer: offer.clone() }
            )).map_err(|error| error.to_string())?;
            session.send(&packet)?;
            let _ = events.send(TransferUiEvent::Status(format!("Waiting for {} to accept {file_name}…", session.remote_fingerprint[..8].to_owned())));
            let response = session.receive_timeout(Duration::from_secs(120))?;
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
            while let Some(chunk) = outgoing.next_chunk().map_err(|error| error.to_string())? {
                sent = sent.saturating_add(chunk.data.len() as u64);
                let packet = encode_packet(&TransferPacket::Chunk(chunk)).map_err(|error| error.to_string())?;
                session.send(&packet)?;
                let _ = events.send(TransferUiEvent::Progress { sent, total });
            }
            let response = session.receive_timeout(Duration::from_secs(120))?;
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
            Err(error) => { let _ = events.send(TransferUiEvent::Failed(error)); }
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
    transfer_status: String,
    #[cfg(windows)]
    trust_store: TrustStore,
    #[cfg(windows)]
    trust_store_error: Option<String>,
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
            clipboard_sync: true,
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
            transfer_status: "Choose a connected trusted device to send a file.".into(),
            #[cfg(windows)]
            trust_store,
            #[cfg(windows)]
            trust_store_error,
            network_rx,
            #[cfg(windows)]
            network_tx,
            brand_texture: None,
        }
    }
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
                };
                let descriptor = connection.exchange_descriptors(&local).map_err(|error| error.to_string())?;
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
        let (discovery, events) = match LocalDiscovery::start(
            &format!("Bridge-{suffix}"),
            &format!("bridge-{suffix}.local."),
            NETWORK_PORT,
            &record,
        ) {
            Ok(value) => value,
            Err(error) => {
                let _ = status.send(format!("Local discovery unavailable: {error}"));
                return;
            }
        };
        let _keep_service_registered = discovery;
        let _ = status.send("Bridge is advertising on the local network. Discovered peers remain unverified.".into());
        while let Ok(event) = events.recv() {
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
        for message in self.network_rx.try_iter() {
            self.network_status = message;
        }
        #[cfg(windows)]
        for event in self.transfer_rx.try_iter() {
            match event {
                TransferUiEvent::Status(message) => self.transfer_status = message,
                TransferUiEvent::Progress { sent, total } => {
                    self.transfer_progress = if total == 0 { 1.0 } else { sent as f32 / total as f32 };
                    self.transfer_status = format!("Sending {} of {} bytes…", sent, total);
                }
                TransferUiEvent::Complete(message) => {
                    self.transfer_busy = false;
                    self.transfer_progress = 1.0;
                    self.transfer_status = message;
                }
                TransferUiEvent::Failed(error) => {
                    self.transfer_busy = false;
                    self.transfer_progress = 0.0;
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
                    match DesktopSession::start(pairing.connection) {
                        Ok(session) => self.active_sessions.push(session),
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
                                    .map(|session| (device.public_key_fingerprint, device.descriptor.display_name, Arc::clone(session)))
                            }).collect::<Vec<_>>();
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
                                        self.transfer_status = format!("Preparing {}…", path.file_name().and_then(|name| name.to_str()).unwrap_or("file"));
                                        start_file_transfer(session, path, self.transfer_tx.clone());
                                    }
                                }
                                ui.add_space(14.0);
                                if self.transfer_busy {
                                    ui.add(egui::ProgressBar::new(self.transfer_progress).show_percentage().animate(true));
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
                                    ui.label(egui::RichText::new("Copy here. Paste there.").size(20.0).strong());
                                    muted(ui, "Share text between devices you have explicitly trusted.");
                                });
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    ui.checkbox(&mut self.clipboard_sync, "Enable clipboard sync");
                                });
                            });
                        });
                        ui.add_space(14.0);
                        panel(ui, |ui| {
                            ui.label(egui::RichText::new("Recent clipboard").size(16.0).strong());
                            ui.add_space(14.0);
                            ui.vertical_centered(|ui| {
                                ui.label(egui::RichText::new("▤").size(34.0).color(ACCENT));
                                ui.label(egui::RichText::new("Nothing copied yet").strong());
                                muted(ui, "Your recent shared text will appear here.");
                            });
                        });
                        ui.add_space(12.0);
                        muted(ui, "Clipboard monitoring and sync are not active in this build.");
                    }
                    Page::Tools => {
                        let tools = [
                            ("♫", "Phone as microphone", "Use your phone as a wireless input for your PC.", "PLANNED"),
                            ("▣", "Phone as webcam", "Bring your mobile camera into desktop apps.", "PLANNED"),
                            ("◖", "PC audio on phone", "Listen to your computer from another device.", "PLANNED"),
                            ("⌖", "Remote input", "Use a trusted phone or tablet as a controller.", "PLANNED"),
                            ("▤", "Phone notifications", "See selected mobile notifications on desktop.", "PLANNED"),
                            ("▦", "Second screen", "Explore a tablet as an extended workspace.", "PLANNED"),
                        ];
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
                        panel(ui, |ui| {
                            eyebrow(ui, "About");
                            ui.label(egui::RichText::new("Bridge for Windows").strong());
                            muted(ui, "Version 0.3 preview · Native Rust interface");
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
                            match DesktopSession::start(pending.connection) {
                                Ok(session) => self.active_sessions.push(session),
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
