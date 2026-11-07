#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui;

struct BridgeApp {
    selected: Page,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page { Overview, Devices, Transfers, Settings }

impl Default for BridgeApp {
    fn default() -> Self { Self { selected: Page::Overview } }
}

impl eframe::App for BridgeApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let visuals = egui::Visuals::dark();
        ctx.set_visuals(visuals);
        egui::SidePanel::left("navigation")
            .exact_width(205.0)
            .frame(egui::Frame::side_top_panel(&ctx.style()).fill(egui::Color32::from_rgb(13, 15, 19)))
            .show(ctx, |ui| {
                ui.add_space(22.0);
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("B").size(25.0).strong().color(egui::Color32::from_rgb(217, 255, 98)));
                    ui.label(egui::RichText::new("BRIDGE").size(17.0).strong().letter_spacing(2.0));
                });
                ui.add_space(38.0);
                ui.label(egui::RichText::new("WORKSPACE").size(10.0).color(egui::Color32::GRAY));
                ui.add_space(10.0);
                for (page, label) in [
                    (Page::Overview, "◈   Overview"),
                    (Page::Devices, "⌁   Devices"),
                    (Page::Transfers, "⇧   Transfers"),
                    (Page::Settings, "⚙   Settings"),
                ] {
                    if ui.selectable_label(self.selected == page, label).clicked() {
                        self.selected = page;
                    }
                    ui.add_space(4.0);
                }
                ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                    ui.label(egui::RichText::new("●  Native Rust application").size(10.0).color(egui::Color32::from_rgb(150, 160, 145)));
                });
            });

        egui::CentralPanel::default()
            .frame(egui::Frame::central_panel(&ctx.style()).fill(egui::Color32::from_rgb(10, 11, 15)))
            .show(ctx, |ui| {
                ui.add_space(20.0);
                ui.label(egui::RichText::new("YOUR ECOSYSTEM").size(10.0).strong().color(egui::Color32::from_rgb(160, 175, 130)));
                ui.heading(egui::RichText::new("Bridge").size(30.0).strong());
                ui.label(egui::RichText::new("Your devices, connected when you need them.").color(egui::Color32::GRAY));
                ui.add_space(22.0);

                egui::Frame::group(ui.style())
                    .fill(egui::Color32::from_rgb(21, 25, 25))
                    .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(48, 57, 47)))
                    .corner_radius(egui::CornerRadius::same(14))
                    .inner_margin(egui::Margin::same(26))
                    .show(ui, |ui| {
                        ui.set_min_height(175.0);
                        ui.label(egui::RichText::new("BRIDGE NETWORK").size(10.0).strong().color(egui::Color32::from_rgb(217, 255, 98)));
                        ui.add_space(12.0);
                        ui.label(egui::RichText::new("One ecosystem. Every device.").size(27.0).strong());
                        ui.add_space(6.0);
                        ui.label("Bring your Windows PC, Android phone and iPhone together in one private workspace.");
                        ui.add_space(18.0);
                        ui.add_enabled(false, egui::Button::new("Pair a device  ↗"));
                    });

                ui.add_space(26.0);
                ui.heading("Your workspace");
                ui.add_space(10.0);
                ui.columns(3, |columns| {
                    for (column, (value, label)) in columns.iter_mut().zip([
                        ("0", "Paired devices"),
                        ("0", "Active transfers"),
                        ("Local", "Network mode"),
                    ]) {
                        egui::Frame::group(ui.style())
                            .fill(egui::Color32::from_rgb(17, 19, 24))
                            .corner_radius(egui::CornerRadius::same(10))
                            .inner_margin(egui::Margin::same(16))
                            .show(column, |ui| {
                                ui.label(egui::RichText::new(value).size(23.0).strong().color(egui::Color32::from_rgb(217, 255, 98)));
                                ui.label(egui::RichText::new(label).size(11.0).color(egui::Color32::GRAY));
                            });
                    }
                });
                ui.add_space(25.0);
                match self.selected {
                    Page::Overview => {
                        ui.heading("Made to work together");
                        ui.add_space(8.0);
                        ui.label("Clipboard sync     •     File transfer     •     Device controls");
                    }
                    Page::Devices => { ui.heading("Devices"); ui.label("Device discovery and pairing are coming in a later milestone."); }
                    Page::Transfers => { ui.heading("Transfers"); ui.label("Secure local file transfer is planned."); }
                    Page::Settings => { ui.heading("Settings"); ui.label("Privacy and capability controls will appear here."); }
                }
                ui.add_space(24.0);
                ui.label(egui::RichText::new("Foundation build — discovery, pairing and transfers are not active yet.").size(10.0).color(egui::Color32::GRAY));
            });
    }
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Bridge")
            .with_inner_size([1180.0, 780.0])
            .with_min_inner_size([760.0, 620.0]),
        ..Default::default()
    };
    eframe::run_native("Bridge", options, Box::new(|_cc| Ok(Box::<BridgeApp>::default())))
}
