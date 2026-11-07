#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui;

const BG: egui::Color32 = egui::Color32::from_rgb(10, 11, 15);
const SIDEBAR: egui::Color32 = egui::Color32::from_rgb(13, 15, 19);
const PANEL: egui::Color32 = egui::Color32::from_rgb(20, 24, 29);
const PANEL_ALT: egui::Color32 = egui::Color32::from_rgb(25, 30, 35);
const BORDER: egui::Color32 = egui::Color32::from_rgb(43, 50, 56);
const ACCENT: egui::Color32 = egui::Color32::from_rgb(217, 255, 98);
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

struct BridgeApp {
    selected: Page,
    show_pair_dialog: bool,
    clipboard_sync: bool,
    launch_at_login: bool,
    notifications: bool,
    auto_accept: bool,
}

impl Default for BridgeApp {
    fn default() -> Self {
        Self {
            selected: Page::Overview,
            show_pair_dialog: false,
            clipboard_sync: true,
            launch_at_login: false,
            notifications: true,
            auto_accept: false,
        }
    }
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
        let mut visuals = egui::Visuals::dark();
        visuals.window_fill = PANEL;
        visuals.panel_fill = BG;
        visuals.override_text_color = Some(TEXT);
        visuals.widgets.noninteractive.bg_fill = PANEL;
        visuals.widgets.inactive.bg_fill = PANEL_ALT;
        visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(39, 47, 42);
        visuals.widgets.active.bg_fill = egui::Color32::from_rgb(55, 67, 48);
        visuals.selection.bg_fill = egui::Color32::from_rgb(58, 75, 43);
        visuals.selection.stroke = egui::Stroke::new(1.0_f32, ACCENT);
        ctx.set_visuals(visuals);

        egui::TopBottomPanel::top("topbar")
            .frame(egui::Frame::new().fill(SIDEBAR).inner_margin(egui::Margin::symmetric(20, 12)))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
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
                    egui::Frame::new().fill(ACCENT).corner_radius(egui::CornerRadius::same(9))
                        .inner_margin(egui::Margin::symmetric(10, 5)).show(ui, |ui| {
                            ui.label(egui::RichText::new("B").size(22.0).strong().color(BG));
                        });
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
                        egui::Button::selectable(selected, format!("   {symbol}     {}", page.title()))
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
                        muted(ui, "Discovery and pairing transport are not connected in this UI milestone.");
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

        if self.show_pair_dialog {
            egui::Window::new("Pair a device")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                .show(ctx, |ui| {
                    ui.set_min_width(330.0);
                    eyebrow(ui, "Secure device pairing");
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new("Connect a device you trust").size(19.0).strong());
                    ui.add_space(8.0);
                    muted(ui, "The pairing interface is ready for the discovery and QR handshake implementation.");
                    ui.add_space(14.0);
                    panel(ui, |ui| {
                        ui.vertical_centered(|ui| {
                            ui.label(egui::RichText::new("⌁").size(36.0).color(ACCENT));
                            ui.label(egui::RichText::new("Waiting for pairing service").strong());
                            muted(ui, "No QR code or network request is generated yet.");
                        });
                    });
                    ui.add_space(12.0);
                    if ui.button("Close").clicked() { self.show_pair_dialog = false; }
                });
        }
    }
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Bridge")
            .with_inner_size([1280.0, 840.0])
            .with_min_inner_size([900.0, 650.0]),
        ..Default::default()
    };
    eframe::run_native("Bridge", options, Box::new(|_cc| Ok(Box::<BridgeApp>::default())))
}
