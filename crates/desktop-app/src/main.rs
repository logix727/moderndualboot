#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod preview;

use eframe::egui::{self, Color32, RichText, Rounding, Stroke, Vec2};
use modernboot_installer::{
    diagnose_system, fix_boot_order, install_modernboot, read_esp_config, reboot_to_firmware,
    reboot_to_os, relaunch_as_admin, scan_esp_oses, set_boot_next, uninstall_modernboot,
    write_esp_config, FirmwareBootEntry, SystemStatus,
};
use modernboot_shared::{BootConfig, BootEntry, OsKind, ThemePreset};
use std::time::{Duration, Instant};

const EMBEDDED_EFI_BYTES: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/modernboot-efi.efi"));

fn efi_embedded() -> bool {
    option_env!("MODERNBOOT_EFI_EMBEDDED") == Some("1")
}

fn main() -> eframe::Result<()> {
    env_logger::init();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1120.0, 740.0])
            .with_min_inner_size([860.0, 580.0])
            .with_title("ModernDualBoot — UEFI Boot Manager"),
        ..Default::default()
    };

    eframe::run_native(
        "ModernDualBoot",
        options,
        Box::new(|cc| {
            apply_dark_theme(&cc.egui_ctx);
            Ok(Box::new(ModernBootApp::new()))
        }),
    )
}

fn apply_dark_theme(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.window_fill = Color32::from_rgb(13, 17, 26);
    visuals.panel_fill = Color32::from_rgb(16, 21, 33);
    visuals.extreme_bg_color = Color32::from_rgb(10, 13, 20);
    visuals.faint_bg_color = Color32::from_rgb(24, 31, 48);
    visuals.widgets.noninteractive.bg_fill = Color32::from_rgb(22, 29, 44);
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, Color32::from_rgb(42, 54, 80));
    visuals.widgets.inactive.bg_fill = Color32::from_rgb(28, 37, 56);
    visuals.widgets.hovered.bg_fill = Color32::from_rgb(38, 51, 77);
    visuals.widgets.active.bg_fill = Color32::from_rgb(59, 130, 246);
    visuals.selection.bg_fill = Color32::from_rgba_unmultiplied(59, 130, 246, 90);
    visuals.selection.stroke = Stroke::new(1.0_f32, Color32::from_rgb(96, 165, 250));
    ctx.set_visuals(visuals);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NavTab {
    Dashboard,
    BootEntries,
    ThemePreview,
    NvramInspector,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ToastKind {
    Info,
    Success,
    Error,
}

struct ToastMessage {
    text: String,
    kind: ToastKind,
    created_at: Instant,
}

struct ModernBootApp {
    active_tab: NavTab,
    status: SystemStatus,
    config: BootConfig,
    preview_selected_idx: usize,
    custom_accent_rgb: [u8; 3],
    use_custom_accent: bool,

    // New custom entry form
    new_entry_label: String,
    new_entry_path: String,
    new_entry_kind: OsKind,

    // Confirmation flags for destructive/reboot actions
    confirm_uninstall: bool,
    confirm_reboot_firmware: bool,

    // Activity logs & toast
    toast: Option<ToastMessage>,
    logs: Vec<String>,
    show_logs: bool,
}

impl ModernBootApp {
    fn new() -> Self {
        let status = diagnose_system();
        let mut config = match read_esp_config() {
            Ok(Some(cfg)) => cfg,
            Ok(None) | Err(_) => BootConfig::default(),
        };

        if config.entries.is_empty() {
            if let Ok(detected) = scan_esp_oses() {
                config.entries = detected;
                if config.default_entry_id.is_none() {
                    config.default_entry_id = config.entries.first().map(|e| e.id.clone());
                }
            }
        }

        let use_custom_accent = config.theme.custom_accent.is_some();
        let custom_accent_rgb = config
            .theme
            .custom_accent
            .unwrap_or_else(|| config.theme.preset.colors().accent);

        let mut app = Self {
            active_tab: NavTab::Dashboard,
            status,
            config,
            preview_selected_idx: 0,
            custom_accent_rgb,
            use_custom_accent,
            new_entry_label: String::new(),
            new_entry_path: String::from("\\EFI\\"),
            new_entry_kind: OsKind::Linux,
            confirm_uninstall: false,
            confirm_reboot_firmware: false,
            toast: None,
            logs: Vec::new(),
            show_logs: false,
        };
        app.log(
            ToastKind::Info,
            "Diagnostics loaded. Ready to manage ModernDualBoot.",
        );
        app
    }

    fn log(&mut self, kind: ToastKind, msg: impl Into<String>) {
        let text = msg.into();
        match kind {
            ToastKind::Info => log::info!("{text}"),
            ToastKind::Success => log::info!("SUCCESS: {text}"),
            ToastKind::Error => log::error!("{text}"),
        }
        self.logs.push(text.clone());
        if self.logs.len() > 100 {
            self.logs.remove(0);
        }
        self.toast = Some(ToastMessage {
            text,
            kind,
            created_at: Instant::now(),
        });
    }

    fn refresh_diagnostics(&mut self) {
        self.status = diagnose_system();
        self.log(ToastKind::Info, "Refreshed system & UEFI NVRAM diagnostics.");
    }

    fn sync_custom_accent(&mut self) {
        self.config.theme.custom_accent = if self.use_custom_accent {
            Some(self.custom_accent_rgb)
        } else {
            None
        };
    }

    fn save_config_to_esp(&mut self) {
        self.sync_custom_accent();
        match write_esp_config(&self.config) {
            Ok(()) => self.log(ToastKind::Success, "Saved config.json to ESP."),
            Err(e) => self.log(ToastKind::Error, format!("Failed to save config: {e}")),
        }
    }

    fn rescan_esp(&mut self) {
        match scan_esp_oses() {
            Ok(found) => {
                let mut added = 0usize;
                for entry in found {
                    let exists = self.config.entries.iter().any(|existing| {
                        existing
                            .efi_path
                            .replace('/', "\\")
                            .eq_ignore_ascii_case(&entry.efi_path.replace('/', "\\"))
                    });
                    if !exists {
                        self.config.entries.push(entry);
                        added += 1;
                    }
                }
                if self.config.default_entry_id.is_none() {
                    self.config.default_entry_id = self.config.entries.first().map(|e| e.id.clone());
                }
                self.log(
                    ToastKind::Success,
                    format!("ESP scan complete: {added} new OS entry(s) added."),
                );
            }
            Err(e) => self.log(ToastKind::Error, format!("ESP scan failed: {e}")),
        }
    }
}

impl eframe::App for ModernBootApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Expire toast after 6 seconds
        if let Some(toast) = &self.toast {
            if toast.created_at.elapsed() > Duration::from_secs(6) {
                self.toast = None;
            }
        }

        // Top Admin Warning Banner (Windows / Linux)
        if !self.status.is_admin {
            egui::TopBottomPanel::top("admin_banner")
                .frame(
                    egui::Frame::none()
                        .fill(Color32::from_rgb(92, 42, 18))
                        .inner_margin(egui::Margin::symmetric(16.0, 10.0)),
                )
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new("⚠ Not running as Administrator/root.")
                                .strong()
                                .color(Color32::from_rgb(254, 215, 170)),
                        );
                        ui.label(
                            RichText::new(
                                "ESP mounting and UEFI NVRAM edits require elevated privileges.",
                            )
                            .color(Color32::from_rgb(253, 186, 116)),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if cfg!(windows) {
                                if ui.button("Relaunch as Administrator").clicked() {
                                    match relaunch_as_admin() {
                                        Ok(()) => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
                                        Err(e) => self.log(
                                            ToastKind::Error,
                                            format!("Elevation failed: {e}"),
                                        ),
                                    }
                                }
                            } else {
                                ui.label(
                                    RichText::new("Run with: sudo modernboot-app")
                                        .monospace()
                                        .color(Color32::WHITE),
                                );
                            }
                        });
                    });
                });
        }

        // Bottom Status Toast & Log Drawer
        egui::TopBottomPanel::bottom("status_bar")
            .frame(
                egui::Frame::none()
                    .fill(Color32::from_rgb(12, 16, 24))
                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(35, 45, 68)))
                    .inner_margin(egui::Margin::symmetric(14.0, 8.0)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if let Some(toast) = &self.toast {
                        let color = match toast.kind {
                            ToastKind::Info => Color32::from_rgb(147, 197, 253),
                            ToastKind::Success => Color32::from_rgb(110, 231, 183),
                            ToastKind::Error => Color32::from_rgb(252, 165, 165),
                        };
                        ui.label(RichText::new(&toast.text).color(color));
                    } else {
                        ui.label(
                            RichText::new("Ready")
                                .color(Color32::from_rgb(148, 163, 184)),
                        );
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let toggle_label = if self.show_logs {
                            "Hide Logs"
                        } else {
                            "Activity Log"
                        };
                        if ui.button(toggle_label).clicked() {
                            self.show_logs = !self.show_logs;
                        }
                    });
                });

                if self.show_logs {
                    ui.add_space(6.0);
                    egui::ScrollArea::vertical()
                        .max_height(110.0)
                        .stick_to_bottom(true)
                        .show(ui, |ui| {
                            for line in &self.logs {
                                ui.label(
                                    RichText::new(line)
                                        .monospace()
                                        .size(11.5)
                                        .color(Color32::from_rgb(148, 163, 184)),
                                );
                            }
                        });
                }
            });

        // Left Navigation Sidebar
        egui::SidePanel::left("sidebar")
            .exact_width(215.0)
            .resizable(false)
            .frame(
                egui::Frame::none()
                    .fill(Color32::from_rgb(13, 18, 29))
                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(32, 42, 64)))
                    .inner_margin(egui::Margin::same(16.0)),
            )
            .show(ctx, |ui| {
                ui.label(
                    RichText::new("ModernDualBoot")
                        .size(20.0)
                        .strong()
                        .color(Color32::from_rgb(241, 245, 249)),
                );
                ui.label(
                    RichText::new("UEFI Boot Manager")
                        .size(12.0)
                        .color(Color32::from_rgb(148, 163, 184)),
                );
                ui.add_space(18.0);

                let tabs = [
                    (NavTab::Dashboard, "Dashboard"),
                    (NavTab::BootEntries, "Boot Entries"),
                    (NavTab::ThemePreview, "Theme & Preview"),
                    (NavTab::NvramInspector, "NVRAM Inspector"),
                ];

                for (tab, label) in tabs {
                    let selected = self.active_tab == tab;
                    let fill = if selected {
                        Color32::from_rgb(37, 99, 235)
                    } else {
                        Color32::TRANSPARENT
                    };
                    let text_color = if selected {
                        Color32::WHITE
                    } else {
                        Color32::from_rgb(203, 213, 225)
                    };
                    let btn = egui::Button::new(RichText::new(label).size(14.0).color(text_color))
                        .fill(fill)
                        .rounding(Rounding::same(8.0))
                        .min_size(Vec2::new(ui.available_width(), 36.0));
                    if ui.add(btn).clicked() {
                        self.active_tab = tab;
                    }
                    ui.add_space(4.0);
                }

                ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                    let efi_badge = if efi_embedded() && !EMBEDDED_EFI_BYTES.is_empty() {
                        format!("EFI payload: {} KB", EMBEDDED_EFI_BYTES.len() / 1024)
                    } else {
                        "EFI payload: placeholder".to_string()
                    };
                    ui.label(
                        RichText::new(efi_badge)
                            .size(11.0)
                            .color(Color32::from_rgb(100, 116, 139)),
                    );
                    ui.add_space(6.0);
                    if ui
                        .add(
                            egui::Button::new("Refresh Status")
                                .min_size(Vec2::new(ui.available_width(), 32.0)),
                        )
                        .clicked()
                    {
                        self.refresh_diagnostics();
                    }
                });
            });

        // Main Content Area
        egui::CentralPanel::default()
            .frame(
                egui::Frame::none()
                    .fill(Color32::from_rgb(16, 21, 33))
                    .inner_margin(egui::Margin::same(24.0)),
            )
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| match self.active_tab {
                    NavTab::Dashboard => self.render_dashboard(ui),
                    NavTab::BootEntries => self.render_boot_entries(ui),
                    NavTab::ThemePreview => self.render_theme_preview(ui),
                    NavTab::NvramInspector => self.render_nvram_inspector(ui),
                });
            });
    }
}

impl ModernBootApp {
    fn render_dashboard(&mut self, ui: &mut egui::Ui) {
        ui.heading(RichText::new("Dashboard").size(24.0).strong());
        ui.label(
            RichText::new("Install, update, and monitor your UEFI dual-boot configuration.")
                .color(Color32::from_rgb(148, 163, 184)),
        );
        ui.add_space(16.0);

        // 1-Click Actions Card
        section_card(ui, "Quick Actions", |ui| {
            ui.horizontal_wrapped(|ui| {
                let install_label = if self.status.modernboot_installed {
                    "Update ModernBoot"
                } else {
                    "1-Click Install ModernBoot"
                };

                let install_btn = egui::Button::new(
                    RichText::new(install_label).strong().color(Color32::WHITE),
                )
                .fill(Color32::from_rgb(37, 99, 235))
                .min_size(Vec2::new(190.0, 38.0));

                if ui.add(install_btn).clicked() {
                    if !efi_embedded() || EMBEDDED_EFI_BYTES.is_empty() {
                        self.log(
                            ToastKind::Error,
                            "Embedded modernboot-efi.efi is a placeholder. Build modernboot-efi first.",
                        );
                    } else {
                        self.sync_custom_accent();
                        match install_modernboot(EMBEDDED_EFI_BYTES, &self.config) {
                            Ok(report) => {
                                self.log(
                                    ToastKind::Success,
                                    format!(
                                        "Installed to {} (Boot{:04X}).",
                                        report.efi_installed_path,
                                        report.boot_entry_id.unwrap_or(0)
                                    ),
                                );
                                self.status = diagnose_system();
                            }
                            Err(e) => {
                                self.log(ToastKind::Error, format!("Install failed: {e}"));
                            }
                        }
                    }
                }

                if ui
                    .add(egui::Button::new("Save Config to ESP").min_size(Vec2::new(150.0, 38.0)))
                    .clicked()
                {
                    self.save_config_to_esp();
                }

                let fix_fill = if self.status.modernboot_installed && !self.status.modernboot_is_first {
                    Color32::from_rgb(217, 119, 6)
                } else {
                    Color32::from_rgb(30, 41, 59)
                };
                if ui
                    .add(
                        egui::Button::new("Fix Boot Order")
                            .fill(fix_fill)
                            .min_size(Vec2::new(135.0, 38.0)),
                    )
                    .clicked()
                {
                    match fix_boot_order() {
                        Ok(order) => {
                            self.log(
                                ToastKind::Success,
                                format!("BootOrder updated ({} entries).", order.len()),
                            );
                            self.status = diagnose_system();
                        }
                        Err(e) => {
                            self.log(ToastKind::Error, format!("Fix Boot Order failed: {e}"));
                        }
                    }
                }

                if !self.confirm_uninstall {
                    if ui
                        .add(egui::Button::new("Uninstall").min_size(Vec2::new(105.0, 38.0)))
                        .clicked()
                    {
                        self.confirm_uninstall = true;
                    }
                } else {
                    if ui
                        .add(
                            egui::Button::new(
                                RichText::new("Confirm Uninstall").color(Color32::WHITE),
                            )
                            .fill(Color32::from_rgb(185, 28, 28))
                            .min_size(Vec2::new(140.0, 38.0)),
                        )
                        .clicked()
                    {
                        self.confirm_uninstall = false;
                        match uninstall_modernboot() {
                            Ok(()) => {
                                self.log(ToastKind::Success, "ModernBoot uninstalled.");
                                self.status = diagnose_system();
                            }
                            Err(e) => {
                                self.log(ToastKind::Error, format!("Uninstall failed: {e}"));
                            }
                        }
                    }
                    if ui.button("Cancel").clicked() {
                        self.confirm_uninstall = false;
                    }
                }

                if !self.confirm_reboot_firmware {
                    if ui
                        .add(
                            egui::Button::new("Reboot to Firmware")
                                .min_size(Vec2::new(150.0, 38.0)),
                        )
                        .clicked()
                    {
                        self.confirm_reboot_firmware = true;
                    }
                } else {
                    if ui
                        .add(
                            egui::Button::new(
                                RichText::new("Confirm Reboot Now").color(Color32::WHITE),
                            )
                            .fill(Color32::from_rgb(180, 83, 9))
                            .min_size(Vec2::new(155.0, 38.0)),
                        )
                        .clicked()
                    {
                        self.confirm_reboot_firmware = false;
                        if let Err(e) = reboot_to_firmware() {
                            self.log(
                                ToastKind::Error,
                                format!("Reboot to firmware failed: {e}"),
                            );
                        }
                    }
                    if ui.button("Cancel").clicked() {
                        self.confirm_reboot_firmware = false;
                    }
                }
            });
        });

        ui.add_space(14.0);

        // Health Cards Grid
        ui.columns(2, |cols| {
            status_card(
                &mut cols[0],
                "Privileges & Boot Mode",
                if self.status.is_admin && self.status.is_uefi {
                    StatusTone::Ok
                } else {
                    StatusTone::Warn
                },
                &[
                    ("Administrator / Root", yes_no(self.status.is_admin)),
                    ("Firmware Mode", if self.status.is_uefi { "UEFI" } else { "Legacy / Unknown" }),
                ],
            );

            cols[0].add_space(12.0);

            let sb_text = match self.status.secure_boot_enabled {
                Some(true) => "Enabled (signing required)",
                Some(false) => "Disabled",
                None => "Unknown",
            };
            let sb_tone = match self.status.secure_boot_enabled {
                Some(false) => StatusTone::Ok,
                Some(true) => StatusTone::Warn,
                None => StatusTone::Neutral,
            };
            status_card(
                &mut cols[0],
                "Secure Boot",
                sb_tone,
                &[("State", sb_text)],
            );

            let esp_label = self
                .status
                .esp_path
                .as_deref()
                .unwrap_or("Not mounted / Not found");
            status_card(
                &mut cols[1],
                "ESP & NVRAM Status",
                if self.status.modernboot_installed && self.status.modernboot_is_first {
                    StatusTone::Ok
                } else if self.status.esp_found {
                    StatusTone::Neutral
                } else {
                    StatusTone::Warn
                },
                &[
                    ("EFI System Partition", esp_label),
                    ("ModernBoot Installed", yes_no(self.status.modernboot_installed)),
                    ("First in BootOrder", yes_no(self.status.modernboot_is_first)),
                ],
            );

            cols[1].add_space(12.0);

            let bl_text = match self.status.bitlocker_active {
                Some(true) => "Active on C: (Back up recovery key!)",
                Some(false) => "Inactive",
                None => "N/A or Unknown",
            };
            let bl_tone = match self.status.bitlocker_active {
                Some(true) => StatusTone::Warn,
                Some(false) => StatusTone::Ok,
                None => StatusTone::Neutral,
            };
            status_card(
                &mut cols[1],
                "BitLocker Encryption",
                bl_tone,
                &[("Status", bl_text)],
            );
        });

        if !self.status.warnings.is_empty() {
            ui.add_space(14.0);
            section_card(ui, "Diagnostic Warnings", |ui| {
                for w in &self.status.warnings {
                    ui.label(
                        RichText::new(format!("• {w}"))
                            .color(Color32::from_rgb(253, 186, 116)),
                    );
                }
            });
        }
    }

    fn render_boot_entries(&mut self, ui: &mut egui::Ui) {
        ui.heading(RichText::new("Boot Entries").size(24.0).strong());
        ui.label(
            RichText::new("Reorder, rename, hide, or set default operating systems.")
                .color(Color32::from_rgb(148, 163, 184)),
        );
        ui.add_space(14.0);

        section_card(ui, "Menu Behavior", |ui| {
            ui.horizontal(|ui| {
                ui.label("Auto-boot timeout (seconds):");
                ui.add(egui::Slider::new(&mut self.config.timeout_seconds, 0..=30));
                if self.config.timeout_seconds == 0 {
                    ui.label(RichText::new("(Wait indefinitely)").color(Color32::GRAY));
                }
            });
            ui.horizontal(|ui| {
                ui.checkbox(&mut self.config.auto_scan, "Auto-scan ESP at boot");
                ui.checkbox(
                    &mut self.config.show_firmware_setup,
                    "Show UEFI Firmware Setup option",
                );
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui.button("Rescan ESP").clicked() {
                    self.rescan_esp();
                }
                if ui.button("Save Config to ESP").clicked() {
                    self.save_config_to_esp();
                }
            });
        });

        ui.add_space(12.0);

        // Entries List
        let mut move_up: Option<usize> = None;
        let mut move_down: Option<usize> = None;
        let mut delete_idx: Option<usize> = None;
        let mut reboot_idx: Option<usize> = None;
        let len = self.config.entries.len();

        section_card(ui, &format!("Configured Entries ({len})"), |ui| {
            if self.config.entries.is_empty() {
                ui.label(
                    RichText::new("No boot entries configured. Click 'Rescan ESP' or add one below.")
                        .color(Color32::from_rgb(148, 163, 184)),
                );
            }

            for (i, entry) in self.config.entries.iter_mut().enumerate() {
                let is_default = self.config.default_entry_id.as_deref() == Some(entry.id.as_str());
                egui::Frame::none()
                    .fill(Color32::from_rgb(20, 27, 42))
                    .stroke(Stroke::new(
                        1.0_f32,
                        if is_default {
                            Color32::from_rgb(59, 130, 246)
                        } else {
                            Color32::from_rgb(38, 49, 74)
                        },
                    ))
                    .rounding(Rounding::same(8.0))
                    .inner_margin(egui::Margin::same(10.0))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            if ui.add_enabled(i > 0, egui::Button::new("↑")).clicked() {
                                move_up = Some(i);
                            }
                            if ui
                                .add_enabled(i + 1 < len, egui::Button::new("↓"))
                                .clicked()
                            {
                                move_down = Some(i);
                            }

                            ui.add(
                                egui::TextEdit::singleline(&mut entry.label)
                                    .desired_width(180.0)
                                    .hint_text("Label"),
                            );

                            egui::ComboBox::from_id_salt(format!("os_kind_{i}"))
                                .selected_text(entry.os_kind.display_name())
                                .width(120.0)
                                .show_ui(ui, |ui| {
                                    for kind in [
                                        OsKind::Windows,
                                        OsKind::Ubuntu,
                                        OsKind::Fedora,
                                        OsKind::Arch,
                                        OsKind::Debian,
                                        OsKind::SteamOS,
                                        OsKind::Linux,
                                        OsKind::MacOS,
                                        OsKind::UefiShell,
                                        OsKind::Unknown,
                                    ] {
                                        ui.selectable_value(
                                            &mut entry.os_kind,
                                            kind,
                                            kind.display_name(),
                                        );
                                    }
                                });

                            ui.add(
                                egui::TextEdit::singleline(&mut entry.efi_path)
                                    .desired_width(230.0)
                                    .hint_text("\\EFI\\...\\boot.efi"),
                            );

                            ui.checkbox(&mut entry.hidden, "Hidden");

                            if is_default {
                                ui.label(
                                    RichText::new("★ Default")
                                        .strong()
                                        .color(Color32::from_rgb(96, 165, 250)),
                                );
                            } else if ui.button("Set Default").clicked() {
                                self.config.default_entry_id = Some(entry.id.clone());
                            }

                            if ui.button("Reboot to this OS").clicked() {
                                reboot_idx = Some(i);
                            }

                            if ui.button("✕").clicked() {
                                delete_idx = Some(i);
                            }
                        });
                    });
                ui.add_space(6.0);
            }
        });

        if let Some(i) = move_up {
            self.config.entries.swap(i, i - 1);
        }
        if let Some(i) = move_down {
            self.config.entries.swap(i, i + 1);
        }
        if let Some(i) = delete_idx {
            let removed = self.config.entries.remove(i);
            if self.config.default_entry_id.as_deref() == Some(removed.id.as_str()) {
                self.config.default_entry_id = self.config.entries.first().map(|e| e.id.clone());
            }
        }
        if let Some(i) = reboot_idx {
            let entry = self.config.entries[i].clone();
            if let Err(e) = reboot_to_os(&entry) {
                self.log(ToastKind::Error, format!("Reboot to '{}' failed: {e}", entry.label));
            }
        }

        ui.add_space(12.0);

        // Add Custom Entry
        section_card(ui, "Add Custom Boot Entry", |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label("Label:");
                ui.add(
                    egui::TextEdit::singleline(&mut self.new_entry_label)
                        .desired_width(160.0)
                        .hint_text("e.g. Arch Linux"),
                );
                ui.label("OS:");
                egui::ComboBox::from_id_salt("new_os_kind")
                    .selected_text(self.new_entry_kind.display_name())
                    .show_ui(ui, |ui| {
                        for kind in [
                            OsKind::Windows,
                            OsKind::Ubuntu,
                            OsKind::Fedora,
                            OsKind::Arch,
                            OsKind::Debian,
                            OsKind::SteamOS,
                            OsKind::Linux,
                            OsKind::MacOS,
                            OsKind::UefiShell,
                            OsKind::Unknown,
                        ] {
                            ui.selectable_value(
                                &mut self.new_entry_kind,
                                kind,
                                kind.display_name(),
                            );
                        }
                    });
                ui.label("EFI Path:");
                ui.add(
                    egui::TextEdit::singleline(&mut self.new_entry_path)
                        .desired_width(240.0),
                );
                if ui.button("+ Add Entry").clicked() {
                    let label = self.new_entry_label.trim();
                    let path = self.new_entry_path.trim();
                    if label.is_empty() || path.is_empty() {
                        self.log(ToastKind::Error, "Label and EFI path are required.");
                    } else {
                        let id = label
                            .to_lowercase()
                            .chars()
                            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
                            .collect::<String>();
                        self.config.entries.push(BootEntry {
                            id: format!("{id}-{}", self.config.entries.len() + 1),
                            label: label.to_string(),
                            os_kind: self.new_entry_kind,
                            efi_path: path.to_string(),
                            hidden: false,
                            auto_detected: false,
                            icon_override: None,
                        });
                        self.new_entry_label.clear();
                        self.log(ToastKind::Success, "Added custom boot entry.");
                    }
                }
            });
        });
    }

    fn render_theme_preview(&mut self, ui: &mut egui::Ui) {
        ui.heading(RichText::new("Theme & Live Boot Screen Preview").size(24.0).strong());
        ui.label(
            RichText::new("Customize your UEFI boot menu appearance and preview it in real time.")
                .color(Color32::from_rgb(148, 163, 184)),
        );
        ui.add_space(14.0);

        section_card(ui, "Theme Controls", |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label("Preset:");
                for preset in ThemePreset::ALL {
                    let selected = self.config.theme.preset == *preset;
                    if ui
                        .selectable_label(selected, preset.display_name())
                        .clicked()
                    {
                        self.config.theme.preset = *preset;
                        if !self.use_custom_accent {
                            self.custom_accent_rgb = preset.colors().accent;
                        }
                    }
                }
            });

            ui.add_space(6.0);

            ui.horizontal_wrapped(|ui| {
                if ui
                    .checkbox(&mut self.use_custom_accent, "Custom Accent Color")
                    .changed()
                {
                    self.sync_custom_accent();
                }

                if self.use_custom_accent {
                    if ui
                        .color_edit_button_srgb(&mut self.custom_accent_rgb)
                        .changed()
                    {
                        self.sync_custom_accent();
                    }
                }

                ui.add_space(12.0);
                ui.label("Corner Radius:");
                let mut radius = self.config.theme.corner_radius as u32;
                if ui.add(egui::Slider::new(&mut radius, 0..=24)).changed() {
                    self.config.theme.corner_radius = radius as u16;
                }

                ui.add_space(12.0);
                ui.checkbox(
                    &mut self.config.theme.show_clock_or_timer,
                    "Show Timer",
                );

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Save Theme to ESP").clicked() {
                        self.save_config_to_esp();
                    }
                });
            });
        });

        ui.add_space(14.0);

        section_card(ui, "Live UEFI Boot Screen Preview (Interactive)", |ui| {
            preview::render_boot_preview(ui, &self.config, &mut self.preview_selected_idx);
        });
    }

    fn render_nvram_inspector(&mut self, ui: &mut egui::Ui) {
        ui.heading(RichText::new("UEFI NVRAM Inspector").size(24.0).strong());
        ui.label(
            RichText::new("Inspect raw firmware BootOrder and BootXXXX load options.")
                .color(Color32::from_rgb(148, 163, 184)),
        );
        ui.add_space(14.0);

        section_card(ui, "BootOrder", |ui| {
            if self.status.boot_order.is_empty() {
                ui.label(
                    RichText::new("No UEFI BootOrder entries available (non-UEFI or unprivileged).")
                        .color(Color32::from_rgb(148, 163, 184)),
                );
            } else {
                let order_str = self
                    .status
                    .boot_order
                    .iter()
                    .map(|id| format!("Boot{id:04X}"))
                    .collect::<Vec<_>>()
                    .join("  →  ");
                ui.label(RichText::new(order_str).monospace().size(13.5));
            }
        });

        ui.add_space(12.0);

        let mut boot_next_target: Option<u16> = None;
        section_card(
            ui,
            &format!("Firmware Boot Entries ({})", self.status.boot_entries.len()),
            |ui| {
                if self.status.boot_entries.is_empty() {
                    ui.label(
                        RichText::new("No BootXXXX variables found.")
                            .color(Color32::from_rgb(148, 163, 184)),
                    );
                } else {
                    for fw in &self.status.boot_entries {
                        boot_next_target = render_nvram_row(ui, fw).or(boot_next_target);
                        ui.add_space(6.0);
                    }
                }
            },
        );

        if let Some(id) = boot_next_target {
            match set_boot_next(id) {
                Ok(()) => self.log(
                    ToastKind::Success,
                    format!("Set UEFI BootNext to Boot{id:04X} for next reboot."),
                ),
                Err(e) => self.log(
                    ToastKind::Error,
                    format!("Failed to set BootNext to Boot{id:04X}: {e}"),
                ),
            }
        }
    }
}

fn render_nvram_row(ui: &mut egui::Ui, fw: &FirmwareBootEntry) -> Option<u16> {
    let mut clicked_boot_next = None;
    egui::Frame::none()
        .fill(Color32::from_rgb(20, 27, 42))
        .stroke(Stroke::new(1.0_f32, Color32::from_rgb(38, 49, 74)))
        .rounding(Rounding::same(8.0))
        .inner_margin(egui::Margin::same(10.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(fw.boot_var_name())
                        .monospace()
                        .strong()
                        .color(Color32::from_rgb(96, 165, 250)),
                );
                let active_color = if fw.is_active {
                    Color32::from_rgb(110, 231, 183)
                } else {
                    Color32::from_rgb(148, 163, 184)
                };
                ui.label(
                    RichText::new(if fw.is_active { "[ACTIVE]" } else { "[INACTIVE]" })
                        .monospace()
                        .size(11.0)
                        .color(active_color),
                );
                ui.label(RichText::new(&fw.description).strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Set BootNext").clicked() {
                        clicked_boot_next = Some(fw.id);
                    }
                });
            });
            ui.horizontal(|ui| {
                let path = fw.efi_path.as_deref().unwrap_or("<no FilePath node>");
                ui.label(
                    RichText::new(format!(
                        "Path: {path}   •   DevicePath: {} bytes   •   OptionalData: {} bytes",
                        fw.device_path_raw.len(),
                        fw.optional_data.len()
                    ))
                    .monospace()
                    .size(11.5)
                    .color(Color32::from_rgb(148, 163, 184)),
                );
            });
        });
    clicked_boot_next
}

fn section_card(ui: &mut egui::Ui, title: &str, add_contents: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::none()
        .fill(Color32::from_rgb(22, 29, 44))
        .stroke(Stroke::new(1.0_f32, Color32::from_rgb(40, 52, 78)))
        .rounding(Rounding::same(10.0))
        .inner_margin(egui::Margin::same(16.0))
        .show(ui, |ui| {
            ui.label(
                RichText::new(title)
                    .size(15.0)
                    .strong()
                    .color(Color32::from_rgb(226, 232, 240)),
            );
            ui.add_space(10.0);
            add_contents(ui);
        });
}

enum StatusTone {
    Ok,
    Warn,
    Neutral,
}

fn status_card(ui: &mut egui::Ui, title: &str, tone: StatusTone, rows: &[(&str, &str)]) {
    let accent = match tone {
        StatusTone::Ok => Color32::from_rgb(52, 211, 153),
        StatusTone::Warn => Color32::from_rgb(251, 191, 36),
        StatusTone::Neutral => Color32::from_rgb(148, 163, 184),
    };
    egui::Frame::none()
        .fill(Color32::from_rgb(22, 29, 44))
        .stroke(Stroke::new(1.0_f32, Color32::from_rgb(40, 52, 78)))
        .rounding(Rounding::same(10.0))
        .inner_margin(egui::Margin::same(14.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("●").color(accent));
                ui.label(
                    RichText::new(title)
                        .size(14.5)
                        .strong()
                        .color(Color32::from_rgb(226, 232, 240)),
                );
            });
            ui.add_space(8.0);
            for (k, v) in rows {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format!("{k}:"))
                            .color(Color32::from_rgb(148, 163, 184)),
                    );
                    ui.label(RichText::new(*v).color(Color32::from_rgb(241, 245, 249)));
                });
            }
        });
}

const fn yes_no(b: bool) -> &'static str {
    if b {
        "Yes"
    } else {
        "No"
    }
}
