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
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
#[cfg(windows)]
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
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
    remote_code: String,
}

struct BridgeApp {
    selected: Page,
    show_pair_dialog: bool,
    clipboard_sync: bool,
    launch_at_login: bool,
    notifications: bool,
    auto_accept: bool,
    network_ip: String,
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
    active_sessions: Vec<NoiseConnection>,
    #[cfg(windows)]
    trust_store: TrustStore,
    network_rx: Receiver<String>,
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
        {
            start_secure_listener(network_tx.clone(), pairing_tx.clone());
            start_local_discovery(network_tx.clone(), peer_tx);
        }
        Self {
            selected: Page::Overview,
            show_pair_dialog: false,
            clipboard_sync: true,
            launch_at_login: false,
            notifications: true,
            auto_accept: false,
            network_ip: String::new(),
            network_status: identity_status,
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
            trust_store: TrustStore::load(trust_store_path()).unwrap_or_default(),
            network_rx,
            network_tx,
            brand_texture: None,
        }
    }
}

const NETWORK_PORT: u16 = 45821;

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
            id: DeviceId::new(),
            display_name: "Bridge Windows".into(),
            platform: Platform::Windows,
            protocol_version: PROTOCOL_VERSION,
        };
        let listener = match TcpListener::bind(("0.0.0.0", NETWORK_PORT)) {
            Ok(listener) => listener,
            Err(error) => { let _ = status.send(format!("Secure listener could not bind: {error}")); return; }
        };
        let _ = status.send(format!("Secure pairing listener ready · identity {}", &fingerprint[..8]));
        for incoming in listener.incoming() {
            match incoming {
                Ok(stream) => {
                    let key = private_key.clone();
                    let local = local.clone();
                    let status = status.clone();
                    let pending = pending.clone();
                    thread::spawn(move || {
                        let result = accept_noise_xx(stream, &key, None)
                            .and_then(|mut connection| {
                                let descriptor = connection.exchange_descriptors(&local)?;
                                Ok((connection, descriptor))
                            });
                        match result {
                            Ok((connection, descriptor)) => {
                                let _ = status.send(format!("Authenticated handshake from {} · awaiting matching code and approval", descriptor.display_name));
                                if pending.send(PendingApproval { connection, descriptor, remote_code: String::new() }).is_err() {
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
                    id: DeviceId::new(),
                    display_name: "Bridge Windows".into(),
                    platform: Platform::Windows,
                    protocol_version: PROTOCOL_VERSION,
                };
                let descriptor = connection.exchange_descriptors(&local).map_err(|error| error.to_string())?;
                Ok(PendingApproval { connection, descriptor, remote_code: String::new() })
            });
        match result {
            Ok(approval) => {
                let _ = status.send("Noise XX handshake complete. Compare the six-digit code on both devices.".into());
                if pending.send(approval).is_err() {
                    let _ = status.send("Pairing window closed; session discarded".into());
                }
            }
            Err(error) => { let _ = status.send(format!("Secure pairing failed: {error}")); }
        }
    });
}

fn start_network_listener(status: Sender<String>) {
    thread::spawn(move || {
        let listener = match TcpListener::bind(("0.0.0.0", NETWORK_PORT)) {
            Ok(listener) => listener,
            Err(error) => {
                let _ = status.send(format!("Could not listen on port {NETWORK_PORT}: {error}"));
                return;
            }
        };
        for stream in listener.incoming() {
            match stream {
                Ok(stream) => {
                    let status = status.clone();
                    thread::spawn(move || handle_network_client(stream, status));
                }
                Err(error) => { let _ = status.send(format!("Network listener error: {error}")); }
            }
        }
    });
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
            device_id: fingerprint.clone(),
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

fn handle_network_client(mut stream: TcpStream, status: Sender<String>) {
    let peer = stream.peer_addr().map(|a| a.to_string()).unwrap_or_else(|_| "unknown device".into());
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let mut line = String::new();
    let result = {
        let mut reader = BufReader::new(&mut stream);
        reader.read_line(&mut line)
    };
    match result {
        Ok(_) if line.trim() == "BRIDGE_PING/1" => {
            let _ = stream.write_all(b"BRIDGE_PONG/1\n");
            let _ = status.send(format!("Bridge network test succeeded with {peer}"));
        }
        Ok(_) if line.trim().starts_with("BRIDGE_PAIR/1|") => {
            let _ = stream.write_all(b"BRIDGE_PAIRING_DISABLED/1\n");
            let _ = status.send(format!("Rejected legacy unauthenticated pairing attempt from {peer}"));
        }
        Ok(_) => { let _ = status.send(format!("Received an unknown test message from {peer}")); }
        Err(error) => { let _ = status.send(format!("Connection from {peer} failed: {error}")); }
    }
}

fn connect_to_peer(host: String, status: Sender<String>) {
    thread::spawn(move || {
        let address = format!("{host}:{NETWORK_PORT}");
        let result = TcpStream::connect_timeout(
            &address.parse().unwrap_or_else(|_| "127.0.0.1:0".parse().unwrap()),
            Duration::from_secs(4),
        ).and_then(|mut stream| {
            stream.set_read_timeout(Some(Duration::from_secs(4)))?;
            stream.write_all(b"BRIDGE_PING/1\n")?;
            let mut response = String::new();
            BufReader::new(stream).read_line(&mut response)?;
            if response.trim() == "BRIDGE_PONG/1" { Ok(()) }
            else { Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "unexpected Bridge response")) }
        });
        let message = match result {
            Ok(()) => format!("Connection test succeeded with {host}"),
            Err(error) => format!("Could not connect to {host}:{NETWORK_PORT}: {error}"),
        };
        let _ = status.send(message);
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
        {
            for peer in self.peer_rx.try_iter() {
                if !self.discovered_peers.iter().any(|known| known.address == peer.address) {
                    self.discovered_peers.push(peer);
                }
            }
            self.pending_pairings.extend(self.pairing_rx.try_iter());
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
                                        ui.label(egui::RichText::new("Ready when you are").strong().size(16.0));
                                        muted(ui, "No devices connected yet");
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
                            metric(&mut cols[0], "0", "Trusted devices", "Nothing paired yet");
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
                        panel(ui, |ui| {
                            eyebrow(ui, "DISCOVERED ON YOUR NETWORK");
                            ui.add_space(8.0);
                            if self.discovered_peers.is_empty() {
                                muted(ui, "Searching for Bridge devices… Discovery results are unverified until Noise authentication completes.");
                            } else {
                                let mut pair_target = None;
                                for (index, peer) in self.discovered_peers.iter().enumerate() {
                                    ui.horizontal(|ui| {
                                        ui.vertical(|ui| {
                                            ui.label(egui::RichText::new(&peer.record.display_name).color(TEXT).strong());
                                            muted(ui, &format!("{} · {}", peer.address, &peer.record.public_key_fingerprint[..8]));
                                        });
                                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                            if primary_button(ui, "Pair") { pair_target = Some(index); }
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
                        ui.add_space(16.0);
                        let mut revoke_device = None;
                        panel(ui, |ui| {
                            eyebrow(ui, "TRUSTED DEVICES");
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
                        ui.add_space(16.0);
                        panel(ui, |ui| {
                            eyebrow(ui, "NETWORK TEST · TCP");
                            ui.add_space(6.0);
                            ui.label(egui::RichText::new("Test a direct local connection").strong().size(17.0));
                            muted(ui, "Both devices must be on the same Wi-Fi/LAN and Bridge must be open on both.");
                            ui.horizontal(|ui| {
                                ui.label("Peer IP");
                                ui.add(egui::TextEdit::singleline(&mut self.network_ip)
                                    .hint_text("e.g. 192.168.1.25").desired_width(190.0));
                                if primary_button(ui, "Test connection") {
                                    let host = self.network_ip.trim().to_string();
                                    if !host.is_empty() { connect_to_peer(host, self.network_tx.clone()); }
                                    else { self.network_status = "Enter the other device's local IP address.".into(); }
                                }
                            });
                            ui.add_space(6.0);
                            ui.label(egui::RichText::new(&self.network_status).size(12.0).color(AMBER));
                            muted(ui, "Bridge test port: TCP 45821 · This test exchanges a harmless ping/pong only.");
                        });
                        ui.add_space(12.0);
                        muted(ui, "Automatic discovery and encrypted pairing are the next networking milestones.");
                    }
                    Page::Transfers => {
                        ui.horizontal(|ui| {
                            for label in ["All transfers", "Sending", "Received"] {
                                let _ = ui.selectable_label(label == "All transfers", label);
                            }
                        });
                        ui.add_space(12.0);
                        panel(ui, |ui| {
                            ui.vertical_centered(|ui| {
                                ui.add_space(26.0);
                                ui.label(egui::RichText::new("⇧").size(40.0).color(ACCENT));
                                ui.label(egui::RichText::new("Your transfers will live here").size(19.0).strong());
                                muted(ui, "Send files directly between paired devices. No cloud upload required.");
                                ui.add_space(14.0);
                                if primary_button(ui, "＋  Choose files") { self.show_pair_dialog = true; }
                                ui.add_space(22.0);
                            });
                        });
                        ui.add_space(12.0);
                        muted(ui, "Transfer controls are a UI preview; file transport is not active yet.");
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
            egui::Window::new("Verify this device")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                .show(ctx, |ui| {
                    ui.set_min_width(380.0);
                    for (index, pending) in self.pending_pairings.iter_mut().enumerate() {
                        eyebrow(ui, "NOISE XX · AUTHENTICATED PEER");
                        ui.label(egui::RichText::new(&pending.descriptor.display_name).size(19.0).strong());
                        muted(ui, &format!("{:?} · fingerprint {}", pending.descriptor.platform, &pending.connection.remote_fingerprint[..16]));
                        ui.add_space(8.0);
                        ui.label(egui::RichText::new(&pending.connection.sas).size(34.0).strong().color(ACCENT));
                        muted(ui, "Compare this six-digit code with the other device. Only continue if both screens show exactly the same code.");
                        ui.label("Enter the code shown on the other device:");
                        ui.add(egui::TextEdit::singleline(&mut pending.remote_code).desired_width(180.0).hint_text("000000"));
                        ui.horizontal(|ui| {
                            if primary_button(ui, "Confirm and trust") { approve_index = Some(index); }
                            if ui.button("Reject").clicked() { reject_index = Some(index); }
                        });
                        ui.separator();
                    }
                });
            if let Some(index) = reject_index {
                self.pending_pairings.remove(index);
                self.network_status = "Pairing rejected. No trust record was created.".into();
            }
            if let Some(index) = approve_index {
                let pending = self.pending_pairings.remove(index);
                match SasConfirmation::user_confirmed(
                    &pending.connection.sas,
                    &pending.connection.sas,
                    pending.remote_code.trim(),
                    &pending.connection.remote_fingerprint,
                ).and_then(|confirmation| {
                    self.trust_store.trust_and_save(
                        pending.descriptor.clone(), confirmation, now_ms(), trust_store_path()
                    )
                }) {
                    Ok(()) => {
                        self.active_sessions.push(pending.connection);
                        self.network_status = format!("{} is now trusted. Encrypted Noise session established.", pending.descriptor.display_name);
                    }
                    Err(error) => {
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
                    muted(ui, "Choose a discovered Bridge device. Both devices must complete Noise XX, compare the same six-digit code, and approve the peer.");
                    ui.add_space(10.0);
                    muted(ui, "QR is not enabled yet. Pair directly from the discovered-device list while both devices are on the same LAN.");
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
