// gui.rs — Minimal GUI shell for FROST (header + theme options only)

use eframe::egui;
use crate::compressor::CryomechController;
use crate::lakeshore350::LakeShore350Controller;
use crate::lakeshore370::LakeShore370Controller;
use crate::lakeshore625::LakeShore625Controller;
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use std::time::{Duration, Instant};

/// Launch the graphical user interface.
pub fn run() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([800.0, 600.0])
            .with_title("FROST - Fridge Remote Operations, Status, and Thermometry"),
        ..Default::default()
    };

    eframe::run_native(
        "FROST",
        options,
        Box::new(|cc| {
            apply_fonts(&cc.egui_ctx);
            Box::new(FrostApp::default())
        }),
    )
}

fn apply_fonts(ctx: &egui::Context) {
    ctx.set_fonts(egui::FontDefinitions::default());
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Theme {
    Default,
    LightBlue,
    Purple,
    Dark,
    White,
    Black,
    Red,
    Green,
    Blue,
    Yellow,
    Cyan,
    Magenta,
    Gray,
    LightGray,
    DarkGray,
    EguiLightBlue,
    EguiLightGreen,
    EguiLightRed,
}

struct FrostApp {
    selected_theme: Theme,
    lakeshore_350: LakeShore350Controller,
    lakeshore_370: LakeShore370Controller,
    temperatures: TemperatureReadings,
    last_temp_update: Instant,
    /// Status / error from the last "Record Temps" button press.
    record_result: Option<Result<String, String>>,
    /// Stop flag for the background recording thread (None = not recording).
    recording_stop_flag: Option<Arc<AtomicBool>>,
    /// Path of the CSV file currently being recorded to.
    recording_csv_path: Option<String>,
    // ── Compressor ───────────────────────────────────────────
    compressor: CryomechController,
    /// Whether the compressor is believed to be running (GUI state).
    compressor_running: bool,
    /// Last status string from get_status().
    compressor_status: String,
    /// Timestamp of last compressor status poll.
    last_compressor_update: Instant,
    /// Any error from the last compressor start/stop attempt.
    compressor_error: Option<String>,
    // ── Magnet / ADR ─────────────────────────────────────────
    lakeshore_625: LakeShore625Controller,
    /// Target current for the ramp (A); defaults to 9.44 A.
    magnet_target_current: f64,
    /// Cached output of the last `get_limits()` call.
    magnet_limits: String,
    /// Timestamp of the last magnet limits poll.
    last_magnet_limits_update: Instant,
    /// Any error from the last magnet ramp attempt.
    magnet_error: Option<String>,
    /// Editable limit fields (populated from LIMIT? and sent via LIMIT).
    magnet_edit_current_limit: f64,
    magnet_edit_voltage_limit: f64,
    magnet_edit_rate_limit: f64,
    /// Error/success message from the last "Set Limits" action.
    magnet_limits_set_msg: Option<Result<(), String>>,
    /// Editable ramp rate (A/s), synced from RATE?.
    magnet_edit_ramp_rate: f64,
    /// Error/success from the last "Set Rate" action.
    magnet_rate_set_msg: Option<Result<(), String>>,
    /// Editable compliance voltage (V), synced from SETV?.
    magnet_edit_compliance_voltage: f64,
    /// Error/success from the last "Set Compliance" action.
    magnet_compliance_set_msg: Option<Result<(), String>>,
    /// Cached quench detection status (QNCH?).
    magnet_quench_status: String,
    /// Live readback values polled every 30 s (RDGI?, RDGV?, RDGF?).
    magnet_live_current: String,
    magnet_live_voltage: String,
    magnet_live_field: String,
}

#[derive(Default)]
struct TemperatureReadings {
    ls350_a: String,    // 3-head
    ls350_b: String,    // ADR
    ls350_c: String,    // 4-head
    ls350_d2: String,   // Switch voltage (no temp)
    ls350_d3: String,   // 4K stage
    ls350_d4: String,   // 3-pump
    ls350_d5: String,   // 4-pump
    ls370_1: String,    // Input 1
    error_message: Option<String>,
}

impl Default for FrostApp {
    fn default() -> Self {
        // Resume recording if a session was active when the GUI was last closed.
        let (recording_stop_flag, recording_csv_path, record_result) =
            if crate::record_temps::is_recording_active() {
                match crate::record_temps::start_recording_loop(30, "temps") {
                    Ok((path, flag)) => (
                        Some(flag),
                        Some(path.clone()),
                        Some(Ok(format!("Resumed recording → {}", path))),
                    ),
                    Err(e) => (None, None, Some(Err(e))),
                }
            } else {
                (None, None, None)
            };

        Self {
            selected_theme: Theme::EguiLightBlue,
            lakeshore_350: LakeShore350Controller::default(),
            lakeshore_370: LakeShore370Controller::default(),
            temperatures: TemperatureReadings::default(),
            last_temp_update: Instant::now() - Duration::from_secs(10),
            record_result,
            recording_stop_flag,
            recording_csv_path,
            compressor: CryomechController::default(),
            compressor_running: false,   // synced on first status poll
            compressor_status: String::new(),
            last_compressor_update: Instant::now() - Duration::from_secs(35),
            compressor_error: None,
            lakeshore_625: LakeShore625Controller::default(),
            magnet_target_current: 9.44,
            magnet_limits: String::new(),
            last_magnet_limits_update: Instant::now() - Duration::from_secs(35),
            magnet_error: None,
            magnet_edit_current_limit: 10.0,
            magnet_edit_voltage_limit: 1.0,
            magnet_edit_rate_limit: 0.1,
            magnet_limits_set_msg: None,
            magnet_edit_ramp_rate: 0.01,
            magnet_rate_set_msg: None,
            magnet_edit_compliance_voltage: 1.0,
            magnet_compliance_set_msg: None,
            magnet_quench_status: String::new(),
            magnet_live_current: String::new(),
            magnet_live_voltage: String::new(),
            magnet_live_field: String::new(),
        }
    }
}

impl eframe::App for FrostApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.apply_theme(ctx);

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add_space(8.0);

            ui.add(
                egui::Label::new(
                    egui::RichText::new("FROST")
                        .size(48.0)
                        .strong()
                        .color(egui::Color32::from_rgb(30, 30, 120)),
                )
                .selectable(false),
            );
            ui.label("Fridge Remote Operations, Status, and Thermometry");

            ui.add_space(14.0);
            ui.separator();
            ui.add_space(10.0);

            ui.strong("Theme / Color");

            ui.horizontal_wrapped(|ui| {
                ui.label("Theme:");
                ui.selectable_value(&mut self.selected_theme, Theme::Default, "Default");
                ui.selectable_value(&mut self.selected_theme, Theme::LightBlue, "Light Blue");
                ui.selectable_value(&mut self.selected_theme, Theme::Purple, "Purple");
                ui.selectable_value(&mut self.selected_theme, Theme::Dark, "Dark");
            });

            ui.horizontal_wrapped(|ui| {
                ui.label("Colors:");
                ui.selectable_value(&mut self.selected_theme, Theme::White, "White");
                ui.selectable_value(&mut self.selected_theme, Theme::Black, "Black");
                ui.selectable_value(&mut self.selected_theme, Theme::Red, "Red");
                ui.selectable_value(&mut self.selected_theme, Theme::Green, "Green");
                ui.selectable_value(&mut self.selected_theme, Theme::Blue, "Blue");
                ui.selectable_value(&mut self.selected_theme, Theme::Yellow, "Yellow");
            });

            ui.horizontal_wrapped(|ui| {
                ui.label("More:");
                ui.selectable_value(&mut self.selected_theme, Theme::Cyan, "Cyan");
                ui.selectable_value(&mut self.selected_theme, Theme::Magenta, "Magenta");
                ui.selectable_value(&mut self.selected_theme, Theme::Gray, "Gray");
                ui.selectable_value(&mut self.selected_theme, Theme::LightGray, "Light Gray");
                ui.selectable_value(&mut self.selected_theme, Theme::DarkGray, "Dark Gray");
            });

            ui.horizontal_wrapped(|ui| {
                ui.label("Egui:");
                ui.selectable_value(&mut self.selected_theme, Theme::EguiLightBlue, "Egui Light Blue");
                ui.selectable_value(&mut self.selected_theme, Theme::EguiLightGreen, "Egui Light Green");
                ui.selectable_value(&mut self.selected_theme, Theme::EguiLightRed, "Egui Light Red");
            });

            ui.add_space(20.0);
            ui.separator();
            ui.add_space(10.0);

            // Compressor section
            self.update_compressor_status_if_needed();
            self.show_compressor_section(ui);

            ui.add_space(20.0);
            ui.separator();
            ui.add_space(10.0);

            // Magnet / ADR section
            self.update_magnet_limits_if_needed();
            self.show_magnet_section(ui);

            ui.add_space(20.0);
            ui.separator();
            ui.add_space(10.0);

            // Thermometry header
            ui.add(
                egui::Label::new(
                    egui::RichText::new("Thermometry")
                        .size(32.0)
                        .strong()
                        .color(egui::Color32::from_rgb(40, 40, 140)),
                )
                .selectable(false),
            );
            ui.add_space(10.0);

            // Temperature readings section
            self.update_temperatures_if_needed();
            self.show_temperature_display(ui);
        });
    }
}

impl FrostApp {
    fn apply_theme(&self, ctx: &egui::Context) {
        let mut style = (*ctx.style()).clone();

        match self.selected_theme {
            Theme::Default => {
                style = egui::Style::default();
            }
            Theme::LightBlue => {
                style.visuals.dark_mode = false;
                style.visuals.window_fill = egui::Color32::from_rgb(230, 240, 255);
                style.visuals.panel_fill = egui::Color32::from_rgb(240, 245, 255);
            }
            Theme::Purple => {
                style.visuals.dark_mode = false;
                style.visuals.window_fill = egui::Color32::from_rgb(240, 230, 255);
                style.visuals.panel_fill = egui::Color32::from_rgb(245, 240, 255);
            }
            Theme::Dark => {
                style.visuals.dark_mode = true;
                style.visuals.window_fill = egui::Color32::from_rgb(30, 30, 40);
                style.visuals.panel_fill = egui::Color32::from_rgb(25, 25, 35);
            }
            Theme::White => {
                style.visuals.dark_mode = false;
                style.visuals.window_fill = egui::Color32::WHITE;
                style.visuals.panel_fill = egui::Color32::from_rgb(250, 250, 250);
            }
            Theme::Black => {
                style.visuals.dark_mode = true;
                style.visuals.window_fill = egui::Color32::BLACK;
                style.visuals.panel_fill = egui::Color32::from_rgb(20, 20, 20);
            }
            Theme::Red => {
                style.visuals.dark_mode = false;
                style.visuals.window_fill = egui::Color32::from_rgb(255, 230, 230);
                style.visuals.panel_fill = egui::Color32::from_rgb(255, 240, 240);
            }
            Theme::Green => {
                style.visuals.dark_mode = false;
                style.visuals.window_fill = egui::Color32::from_rgb(230, 255, 230);
                style.visuals.panel_fill = egui::Color32::from_rgb(240, 255, 240);
            }
            Theme::Blue => {
                style.visuals.dark_mode = false;
                style.visuals.window_fill = egui::Color32::from_rgb(230, 235, 255);
                style.visuals.panel_fill = egui::Color32::from_rgb(240, 242, 255);
            }
            Theme::Yellow => {
                style.visuals.dark_mode = false;
                style.visuals.window_fill = egui::Color32::from_rgb(255, 255, 225);
                style.visuals.panel_fill = egui::Color32::from_rgb(255, 255, 240);
            }
            Theme::Cyan => {
                style.visuals.dark_mode = false;
                style.visuals.window_fill = egui::Color32::from_rgb(225, 255, 255);
                style.visuals.panel_fill = egui::Color32::from_rgb(240, 255, 255);
            }
            Theme::Magenta => {
                style.visuals.dark_mode = false;
                style.visuals.window_fill = egui::Color32::from_rgb(255, 225, 255);
                style.visuals.panel_fill = egui::Color32::from_rgb(255, 240, 255);
            }
            Theme::Gray => {
                style.visuals.dark_mode = false;
                style.visuals.window_fill = egui::Color32::from_rgb(235, 235, 235);
                style.visuals.panel_fill = egui::Color32::from_rgb(245, 245, 245);
            }
            Theme::LightGray => {
                style.visuals.dark_mode = false;
                style.visuals.window_fill = egui::Color32::from_rgb(245, 245, 245);
                style.visuals.panel_fill = egui::Color32::from_rgb(250, 250, 250);
            }
            Theme::DarkGray => {
                style.visuals.dark_mode = true;
                style.visuals.window_fill = egui::Color32::from_rgb(45, 45, 45);
                style.visuals.panel_fill = egui::Color32::from_rgb(35, 35, 35);
            }
            Theme::EguiLightBlue => {
                style.visuals = egui::Visuals::light();
                style.visuals.window_fill = egui::Color32::from_rgb(232, 240, 255);
                style.visuals.panel_fill = egui::Color32::from_rgb(244, 248, 255);
            }
            Theme::EguiLightGreen => {
                style.visuals = egui::Visuals::light();
                style.visuals.window_fill = egui::Color32::from_rgb(235, 255, 235);
                style.visuals.panel_fill = egui::Color32::from_rgb(245, 255, 245);
            }
            Theme::EguiLightRed => {
                style.visuals = egui::Visuals::light();
                style.visuals.window_fill = egui::Color32::from_rgb(255, 235, 235);
                style.visuals.panel_fill = egui::Color32::from_rgb(255, 245, 245);
            }
        }

        // Keep buttons readable with consistent white/blue styling
        if style.visuals.dark_mode {
            style.visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(45, 45, 55);
            style.visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(55, 80, 130);
            style.visuals.widgets.active.bg_fill = egui::Color32::from_rgb(45, 110, 220);
        } else {
            style.visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(255, 255, 255);
            style.visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(210, 230, 255);
            style.visuals.widgets.active.bg_fill = egui::Color32::from_rgb(120, 170, 255);
        }

        // Add subtle outline so white buttons are visible
        style.visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1.0, egui::Color32::from_gray(120));
        style.visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, egui::Color32::from_gray(140));
        style.visuals.widgets.active.bg_stroke = egui::Stroke::new(1.0, egui::Color32::from_gray(160));

        ctx.set_style(style);
    }

    /// Poll magnet limits, ramp rate, compliance voltage, and set-current every 30 seconds.
    fn update_magnet_limits_if_needed(&mut self) {
        if self.last_magnet_limits_update.elapsed() >= Duration::from_secs(30) {
            // LIMIT?
            self.lakeshore_625.get_limits();
            if let Some(e) = &self.lakeshore_625.error_message.clone() {
                self.magnet_limits = format!("Error: {}", e);
            } else {
                let output = self.lakeshore_625.output.clone();
                self.magnet_limits = output.clone();
                if let Some((c, v, r)) = parse_limits_from_output(&output) {
                    self.magnet_edit_current_limit = c;
                    self.magnet_edit_voltage_limit = v;
                    self.magnet_edit_rate_limit = r;
                }
            }
            // SETI? — sync target current box
            self.lakeshore_625.get_set_current();
            if self.lakeshore_625.error_message.is_none() {
                let out = self.lakeshore_625.output.clone();
                if let Some(val) = parse_single_value(&out) {
                    self.magnet_target_current = val;
                }
            }
            // RATE?
            self.lakeshore_625.get_ramp_rate();
            if self.lakeshore_625.error_message.is_none() {
                let out = self.lakeshore_625.output.clone();
                if let Some(val) = parse_single_value(&out) {
                    self.magnet_edit_ramp_rate = val;
                }
            }
            // SETV?
            self.lakeshore_625.get_compliance_voltage();
            if self.lakeshore_625.error_message.is_none() {
                let out = self.lakeshore_625.output.clone();
                if let Some(val) = parse_single_value(&out) {
                    self.magnet_edit_compliance_voltage = val;
                }
            }
            // QNCH?
            self.lakeshore_625.get_quench_status();
            if self.lakeshore_625.error_message.is_none() {
                self.magnet_quench_status = self.lakeshore_625.output.clone();
            }
            // Live readings: RDGI?, RDGV?, RDGF?
            if let Ok(v) = self.lakeshore_625.get_current() { self.magnet_live_current = v; }
            if let Ok(v) = self.lakeshore_625.get_voltage() { self.magnet_live_voltage = v; }
            if let Ok(v) = self.lakeshore_625.get_field()   { self.magnet_live_field   = v; }
            self.last_magnet_limits_update = Instant::now();
        }
    }

    /// Draw the Magnet / ADR ramp controls and limits block.
    fn show_magnet_section(&mut self, ui: &mut egui::Ui) {
        ui.add(
            egui::Label::new(
                egui::RichText::new("Magnet / ADR")
                    .size(32.0)
                    .strong()
                    .color(egui::Color32::from_rgb(40, 40, 140)),
            )
            .selectable(false),
        );
        ui.add_space(6.0);

        // ── Live readback cards ───────────────────────────────────
        {
            let current_str = if self.magnet_live_current.is_empty() {
                "—".to_string()
            } else {
                format!("{} A", self.magnet_live_current)
            };
            let voltage_str = if self.magnet_live_voltage.is_empty() {
                "—".to_string()
            } else {
                format!("{} V", self.magnet_live_voltage)
            };
            let field_str = if self.magnet_live_field.is_empty() {
                "—".to_string()
            } else {
                format!("{} T", self.magnet_live_field)
            };

            let cards: &[(&str, &str, &str)] = &[
                ("Output Current", "LS625 · RDGI?", &current_str),
                ("Output Voltage", "LS625 · RDGV?", &voltage_str),
                ("Magnetic Field", "LS625 · RDGF?", &field_str),
            ];

            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);
                for &(name, id, val) in cards {
                    egui::Frame::none()
                        .fill(egui::Color32::from_rgb(218, 235, 218))
                        .stroke(egui::Stroke::new(1.5, egui::Color32::from_rgb(80, 140, 80)))
                        .rounding(egui::Rounding::same(8.0))
                        .inner_margin(egui::Margin::same(10.0))
                        .show(ui, |ui| {
                            ui.set_min_width(130.0);
                            ui.vertical(|ui| {
                                ui.add(egui::Label::new(
                                    egui::RichText::new(name).strong().size(14.0),
                                ).selectable(false));
                                ui.add(egui::Label::new(
                                    egui::RichText::new(id)
                                        .size(10.5)
                                        .color(egui::Color32::from_gray(110)),
                                ).selectable(false));
                                ui.add_space(4.0);
                                ui.add(egui::Label::new(
                                    egui::RichText::new(val).size(13.0).monospace(),
                                ).selectable(false));
                            });
                        });
                }
            });
        }

        ui.add_space(8.0);

        // ── Ramp controls ────────────────────────────────────────
        ui.horizontal(|ui| {
            ui.label("Target Current (A):");
            ui.add(
                egui::DragValue::new(&mut self.magnet_target_current)
                    .speed(0.01)
                    .clamp_range(0.0_f64..=60.1_f64)
                    .fixed_decimals(2),
            );

            ui.add_space(8.0);

            let btn = egui::Button::new(
                egui::RichText::new("▶  Ramp Magnet").strong(),
            )
            .fill(egui::Color32::from_rgb(40, 80, 170));

            if ui.add(btn).clicked() {
                let target = self.magnet_target_current;
                match self.lakeshore_625.set_current(target) {
                    Ok(()) => {
                        self.magnet_error = None;
                        // Refresh limits immediately after ramping
                        self.lakeshore_625.get_limits();
                        if self.lakeshore_625.error_message.is_none() {
                            self.magnet_limits = self.lakeshore_625.output.clone();
                        }
                        self.last_magnet_limits_update = Instant::now();
                    }
                    Err(e) => self.magnet_error = Some(e),
                }
            }
        });

        // Error line
        if let Some(ref e) = self.magnet_error {
            ui.colored_label(egui::Color32::RED, format!("Magnet error: {}", e));
        }

        ui.add_space(6.0);

        // ── Two-column settings block ─────────────────────────────
        ui.columns(2, |cols| {
            // ── Left column: Ramp rate & Compliance voltage ──────
            cols[0].strong("Ramp Rate & Compliance");
            cols[0].add_space(4.0);

            egui::Grid::new("magnet_ramp_grid")
                .num_columns(4)
                .spacing([8.0, 6.0])
                .show(&mut cols[0], |ui| {
                    // Ramp rate row
                    ui.label("Ramp rate:");
                    ui.add(
                        egui::DragValue::new(&mut self.magnet_edit_ramp_rate)
                            .speed(0.001)
                            .clamp_range(0.0001_f64..=99.999_f64)
                            .fixed_decimals(4),
                    );
                    ui.label("A/s");
                    let rate_btn = egui::Button::new(egui::RichText::new("Set Rate").strong())
                        .fill(egui::Color32::from_rgb(80, 120, 60));
                    if ui.add(rate_btn).clicked() {
                        let r = self.magnet_edit_ramp_rate;
                        match self.lakeshore_625.set_ramp_rate(r) {
                            Ok(()) => {
                                self.lakeshore_625.get_ramp_rate();
                                if self.lakeshore_625.error_message.is_none() {
                                    let out = self.lakeshore_625.output.clone();
                                    if let Some(val) = parse_single_value(&out) {
                                        self.magnet_edit_ramp_rate = val;
                                    }
                                }
                                self.magnet_rate_set_msg = Some(Ok(()));
                            }
                            Err(e) => self.magnet_rate_set_msg = Some(Err(e)),
                        }
                    }
                    ui.end_row();

                    // Rate feedback
                    ui.label("");
                    if let Some(ref msg) = self.magnet_rate_set_msg.clone() {
                        match msg {
                            Ok(()) => { ui.colored_label(egui::Color32::DARK_GREEN, "✔ Rate set."); }
                            Err(e) => { ui.colored_label(egui::Color32::RED, e.as_str()); }
                        }
                    }
                    ui.end_row();

                    // Compliance voltage row
                    ui.label("Compliance V:");
                    ui.add(
                        egui::DragValue::new(&mut self.magnet_edit_compliance_voltage)
                            .speed(0.01)
                            .clamp_range(0.1_f64..=5.0_f64)
                            .fixed_decimals(2),
                    );
                    ui.label("V");
                    let comp_btn = egui::Button::new(egui::RichText::new("Set Compliance").strong())
                        .fill(egui::Color32::from_rgb(80, 120, 60));
                    if ui.add(comp_btn).clicked() {
                        let v = self.magnet_edit_compliance_voltage;
                        match self.lakeshore_625.set_compliance_voltage(v) {
                            Ok(()) => {
                                self.lakeshore_625.get_compliance_voltage();
                                if self.lakeshore_625.error_message.is_none() {
                                    let out = self.lakeshore_625.output.clone();
                                    if let Some(val) = parse_single_value(&out) {
                                        self.magnet_edit_compliance_voltage = val;
                                    }
                                }
                                self.magnet_compliance_set_msg = Some(Ok(()));
                            }
                            Err(e) => self.magnet_compliance_set_msg = Some(Err(e)),
                        }
                    }
                    ui.end_row();

                    // Compliance feedback
                    ui.label("");
                    if let Some(ref msg) = self.magnet_compliance_set_msg.clone() {
                        match msg {
                            Ok(()) => { ui.colored_label(egui::Color32::DARK_GREEN, "✔ Compliance set."); }
                            Err(e) => { ui.colored_label(egui::Color32::RED, e.as_str()); }
                        }
                    }
                    ui.end_row();
                });

            // ── Right column: Limits ─────────────────────────────
            cols[1].strong("Limits (LIMIT?)");

            if self.magnet_limits.starts_with("Error:") {
                cols[1].colored_label(egui::Color32::RED, &self.magnet_limits.clone());
            } else if self.magnet_limits.is_empty() {
                cols[1].label("(pending first poll…)");
            }

            egui::Grid::new("magnet_limits_grid")
                .num_columns(3)
                .spacing([8.0, 4.0])
                .show(&mut cols[1], |ui| {
                    ui.label("Current limit:");
                    ui.add(
                        egui::DragValue::new(&mut self.magnet_edit_current_limit)
                            .speed(0.1)
                            .clamp_range(0.0_f64..=60.1_f64)
                            .fixed_decimals(2),
                    );
                    ui.label("A");
                    ui.end_row();

                    ui.label("Voltage limit:");
                    ui.add(
                        egui::DragValue::new(&mut self.magnet_edit_voltage_limit)
                            .speed(0.01)
                            .clamp_range(0.1_f64..=5.0_f64)
                            .fixed_decimals(2),
                    );
                    ui.label("V");
                    ui.end_row();

                    ui.label("Rate limit:");
                    ui.add(
                        egui::DragValue::new(&mut self.magnet_edit_rate_limit)
                            .speed(0.001)
                            .clamp_range(0.0001_f64..=99.999_f64)
                            .fixed_decimals(4),
                    );
                    ui.label("A/s");
                    ui.end_row();
                });

            // Set Limits button — wrapped in horizontal so it never stretches full column width
            cols[1].horizontal(|ui| {
                let set_btn = egui::Button::new(egui::RichText::new("Set Limits").strong())
                    .fill(egui::Color32::from_rgb(80, 120, 60));
                if ui.add(set_btn).clicked() {
                    let c = self.magnet_edit_current_limit;
                    let v = self.magnet_edit_voltage_limit;
                    let r = self.magnet_edit_rate_limit;
                    match self.lakeshore_625.set_limits(c, v, r) {
                        Ok(()) => {
                            self.lakeshore_625.get_limits();
                            if self.lakeshore_625.error_message.is_none() {
                                let output = self.lakeshore_625.output.clone();
                                self.magnet_limits = output.clone();
                                if let Some((pc, pv, pr)) = parse_limits_from_output(&output) {
                                    self.magnet_edit_current_limit = pc;
                                    self.magnet_edit_voltage_limit = pv;
                                    self.magnet_edit_rate_limit = pr;
                                }
                            }
                            self.last_magnet_limits_update = Instant::now();
                            self.magnet_limits_set_msg = Some(Ok(()));
                        }
                        Err(e) => self.magnet_limits_set_msg = Some(Err(e)),
                    }
                }
                if let Some(ref msg) = self.magnet_limits_set_msg.clone() {
                    match msg {
                        Ok(()) => { ui.colored_label(egui::Color32::DARK_GREEN, "✔ Limits updated."); }
                        Err(e) => { ui.colored_label(egui::Color32::RED, format!("Error: {}", e)); }
                    }
                }
            });

            // Quench status printout
            if !self.magnet_quench_status.is_empty() {
                cols[1].add_space(4.0);
                for line in self.magnet_quench_status.lines() {
                    cols[1].label(line);
                }
            }
        });

        ui.add_space(4.0);
        if !self.magnet_limits.is_empty() && !self.magnet_limits.starts_with("Error:") {
            ui.label(format!(
                "Last updated: {:.1}s ago  (refreshes every 30 s)",
                self.last_magnet_limits_update.elapsed().as_secs_f32()
            ));
        }
    }

    /// Update temperature readings every 30 seconds
    fn update_temperatures_if_needed(&mut self) {
        if self.last_temp_update.elapsed() >= Duration::from_secs(30) {
            self.read_all_temperatures();
            self.last_temp_update = Instant::now();
        }
    }

    /// Read temperatures from both Lakeshore controllers
    fn read_all_temperatures(&mut self) {
        self.temperatures.error_message = None;

        // Lakeshore 350 temperatures
        // Input A (3-head) - uses calibration
        self.temperatures.ls350_a = self.read_350_temperature_kelvin("A");
        
        // Input B (ADR) - uses KRDG?
        self.temperatures.ls350_b = self.read_350_temperature_kelvin("B");
        
        // Input C (4-head) - uses calibration  
        self.temperatures.ls350_c = self.read_350_temperature_kelvin("C");
        
        // Input D2 (Switch) - voltage sensor, converted to temperature
        self.temperatures.ls350_d2 = self.read_350_temperature_kelvin("D2");
        
        // Input D3 (4K stage) - uses KRDG?
        self.temperatures.ls350_d3 = self.read_350_temperature_kelvin("D3");
        
        // Input D4 (3-pump) - uses calibration
        self.temperatures.ls350_d4 = self.read_350_temperature_kelvin("D4");
        
        // Input D5 (4-pump) - uses calibration
        self.temperatures.ls350_d5 = self.read_350_temperature_kelvin("D5");

        // Lakeshore 370 Input 1
        self.temperatures.ls370_1 = self.read_370_temperature_kelvin(1);
    }

    /// Read temperature from Lakeshore 350 input using appropriate method
    fn read_350_temperature_kelvin(&mut self, input: &str) -> String {
        // Use the intelligent reading method for all inputs
        let old_output = self.lakeshore_350.output.clone();
        self.lakeshore_350.read_input_intelligent(input);
        let result = self.extract_temperature_value(&self.lakeshore_350.output);
        // Restore the original output to avoid side effects
        self.lakeshore_350.output = old_output;
        result
    }

    /// Read temperature from Lakeshore 370 input
    fn read_370_temperature_kelvin(&self, input: u8) -> String {
        match self.lakeshore_370.read_kelvin(input) {
            Ok(k_str) => self.format_kelvin_value(&k_str),
            Err(e) => format!("ERROR ({})", e)
        }
    }

    /// Extract temperature value from output string
    fn extract_temperature_value(&self, temp_str: &str) -> String {
        if temp_str.contains("ERROR") {
            return temp_str.to_string();
        }
        
        // Look for pattern like "1.234 K" after the arrow
        if let Some(arrow_pos) = temp_str.find("→") {
            let after_arrow = &temp_str[arrow_pos + 3..]; // Skip "→ "
            if let Some(k_pos) = after_arrow.find(" K") {
                let temp_part = &after_arrow[..k_pos + 2]; // Include " K"
                return temp_part.trim().to_string();
            }
        }
        
        // Look for pattern like ": 1.234 K" 
        if let Some(colon_pos) = temp_str.rfind(":") {
            let after_colon = &temp_str[colon_pos + 1..].trim();
            if after_colon.ends_with(" K") {
                return after_colon.to_string();
            }
        }
        
        // Fallback: return the whole string trimmed
        temp_str.trim().to_string()
    }

    /// Format Kelvin value string into readable format
    fn format_kelvin_value(&self, k_str: &str) -> String {
        match k_str.parse::<f64>() {
            Ok(k) if k > 0.0 => format!("{:.4} K", k),
            Ok(_) => format!("{} (overload)", k_str),
            Err(_) => k_str.to_string()
        }
    }

    /// Poll compressor status every 30 seconds.
    fn update_compressor_status_if_needed(&mut self) {
        if self.last_compressor_update.elapsed() >= Duration::from_secs(30) {
            self.compressor.get_status();
            if let Some(e) = &self.compressor.error_message {
                self.compressor_status = format!("Error: {}", e);
            } else {
                self.compressor_status = self.compressor.status_output.clone();
                // Sync running state from the hardware response
                self.compressor_running = self.compressor.status_output
                    .lines()
                    .any(|l| l.contains("Running:") && l.contains("Yes"));
            }
            self.last_compressor_update = Instant::now();
        }
    }

    /// Draw the compressor start/stop button and status block.
    fn show_compressor_section(&mut self, ui: &mut egui::Ui) {
        ui.add(
            egui::Label::new(
                egui::RichText::new("Compressor")
                    .size(32.0)
                    .strong()
                    .color(egui::Color32::from_rgb(40, 40, 140)),
            )
            .selectable(false),
        );
        ui.add_space(6.0);

        // ── Start / Stop button ──────────────────────────────────
        ui.horizontal(|ui| {
            if self.compressor_running {
                let btn = egui::Button::new(
                    egui::RichText::new("⏹  Stop Compressor").strong()
                )
                .fill(egui::Color32::from_rgb(180, 40, 40));
                if ui.add(btn).clicked() {
                    match self.compressor.stop_compressor() {
                        Ok(()) => {
                            self.compressor_running = false;
                            self.compressor_error = None;
                            // Immediately refresh status
                            self.compressor.get_status();
                            self.compressor_status = self.compressor.status_output.clone();
                            self.last_compressor_update = Instant::now();
                        }
                        Err(e) => self.compressor_error = Some(e),
                    }
                }
            } else {
                let btn = egui::Button::new(
                    egui::RichText::new("▶  Start Pulse Tube Cooldown").strong()
                )
                .fill(egui::Color32::from_rgb(30, 120, 60));
                if ui.add(btn).clicked() {
                    match self.compressor.start_compressor() {
                        Ok(()) => {
                            self.compressor_running = true;
                            self.compressor_error = None;
                            // Immediately refresh status
                            self.compressor.get_status();
                            self.compressor_status = self.compressor.status_output.clone();
                            self.last_compressor_update = Instant::now();
                        }
                        Err(e) => self.compressor_error = Some(e),
                    }
                }
            }
        });

        // Error line
        if let Some(ref e) = self.compressor_error {
            ui.colored_label(egui::Color32::RED, format!("Compressor error: {}", e));
        }

        ui.add_space(6.0);

        // ── Status block ─────────────────────────────────────────
        if !self.compressor_status.is_empty() {
            ui.strong("Compressor Status:");
            for line in self.compressor_status.lines() {
                ui.label(line);
            }
            ui.label(format!(
                "Last updated: {:.1}s ago  (refreshes every 30 s)",
                self.last_compressor_update.elapsed().as_secs_f32()
            ));
        } else {
            ui.label("Compressor status: (pending first poll…)");
        }
    }

    /// Display temperature readings and a Record Temps button in the GUI.
    fn show_temperature_display(&mut self, ui: &mut egui::Ui) {
        // ── Record Temperatures button (toggles start / stop) ────
        let is_recording = self.recording_stop_flag
            .as_ref()
            .map(|f| !f.load(Ordering::Relaxed))
            .unwrap_or(false);

        ui.horizontal(|ui| {
            ui.strong("Temperature Readings");
            ui.add_space(12.0);

            if is_recording {
                // ── STOP button ──────────────────────────────────
                let btn = egui::Button::new(
                    egui::RichText::new("⏹  Stop Recording Temperatures").strong()
                )
                .fill(egui::Color32::from_rgb(180, 40, 40));
                if ui.add(btn).clicked() {
                    if let Some(flag) = &self.recording_stop_flag {
                        flag.store(true, Ordering::Relaxed);
                    }
                    self.recording_stop_flag = None;
                    crate::record_temps::clear_recording_active();
                    self.record_result = Some(Ok(
                        format!("Recording stopped. File: {}",
                            self.recording_csv_path.as_deref().unwrap_or("unknown"))
                    ));
                }
            } else {
                // ── START button ─────────────────────────────────
                let btn = egui::Button::new(
                    egui::RichText::new("⏺  Record Temperatures").strong()
                )
                .fill(egui::Color32::from_rgb(30, 120, 60));
                if ui.add(btn).clicked() {
                    match crate::record_temps::start_recording_loop(30, "temps") {
                        Ok((path, flag)) => {
                            self.recording_csv_path = Some(path.clone());
                            self.recording_stop_flag = Some(flag);
                            self.record_result = Some(Ok(
                                format!("Recording to: {}", path)
                            ));
                        }
                        Err(e) => {
                            self.record_result = Some(Err(e));
                        }
                    }
                }
            }
        });


        // ── Record result status line ────────────────────────────
        if let Some(ref res) = self.record_result {
            match res {
                Ok(msg)  => { ui.colored_label(egui::Color32::DARK_GREEN, msg); }
                Err(err) => { ui.colored_label(egui::Color32::RED, format!("Record error: {}", err)); }
            }
        }

        ui.add_space(6.0);

        if let Some(ref error) = self.temperatures.error_message {
            ui.colored_label(egui::Color32::RED, format!("Error: {}", error));
            ui.add_space(4.0);
        }

        // Collect values before closures to avoid borrow conflicts
        let d3    = self.temperatures.ls350_d3.clone();
        let adr   = self.temperatures.ls350_b.clone();
        let d2    = self.temperatures.ls350_d2.clone();
        let head3 = self.temperatures.ls350_a.clone();
        let head4 = self.temperatures.ls350_c.clone();
        let pump3 = self.temperatures.ls350_d4.clone();
        let pump4 = self.temperatures.ls350_d5.clone();
        let ls370 = self.temperatures.ls370_1.clone();
        let adr_temp = adr.split('\u{2192}').nth(1)
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|| adr.clone());
        let elapsed = self.last_temp_update.elapsed().as_secs_f32();

        // ── Sensor cards ─────────────────────────────────────────
        let sensors_350: &[(&str, &str, &str)] = &[
            ("4K Stage", "LS350 · D3", &d3),
            ("ADR",          "LS350 · B",   &adr_temp),
            ("Switch",       "LS350 · D2",  &d2),
            ("3-Head",       "LS350 · A",   &head3),
            ("4-Head",       "LS350 · C",   &head4),
            ("3-Pump",       "LS350 · D4",  &pump3),
            ("4-Pump",       "LS350 · D5",  &pump4),
            ("Device Stage", "LS370 · In1", &ls370),
        ];

        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);
            for &(name, id, val) in sensors_350 {
                egui::Frame::none()
                    .fill(egui::Color32::from_rgb(218, 230, 255))
                    .stroke(egui::Stroke::new(1.5, egui::Color32::from_rgb(100, 130, 200)))
                    .rounding(egui::Rounding::same(8.0))
                    .inner_margin(egui::Margin::same(10.0))
                    .show(ui, |ui| {
                        ui.set_min_width(130.0);
                        ui.vertical(|ui| {
                            ui.add(egui::Label::new(
                                egui::RichText::new(name).strong().size(14.0),
                            ).selectable(false));
                            ui.add(egui::Label::new(
                                egui::RichText::new(id)
                                    .size(10.5)
                                    .color(egui::Color32::from_gray(110)),
                            ).selectable(false));
                            ui.add_space(4.0);
                            ui.add(egui::Label::new(
                                egui::RichText::new(val).size(13.0).monospace(),
                            ).selectable(false));
                        });
                    });
            }
        });


        ui.add_space(8.0);
        ui.label(format!("Last updated: {:.1}s ago", elapsed));
        ui.label("Updates every 30 seconds");
    }
}

// ── Helpers ───────────────────────────────────────────────────

/// Extracts a single f64 from strings like:
///   "Set current: 9.44 A"  →  9.44
///   "Ramp rate: 0.0100 A/s"  →  0.01
///   "Compliance voltage: 1.0 V"  →  1.0
/// (always the third whitespace-separated token)
fn parse_single_value(output: &str) -> Option<f64> {
    output.split_whitespace().nth(2)?.parse().ok()
}

/// Parse the formatted output of `get_limits()` into (current, voltage, rate).
/// Expected format:
///   "Current limit: X A\nVoltage limit: Y V\nRate limit:    Z A/s"
fn parse_limits_from_output(output: &str) -> Option<(f64, f64, f64)> {
    let mut current = None;
    let mut voltage = None;
    let mut rate = None;
    for line in output.lines() {
        let mut parts = line.split_whitespace();
        match parts.next() {
            Some("Current") => { current = parts.nth(1).and_then(|s| s.parse().ok()); }
            Some("Voltage") => { voltage = parts.nth(1).and_then(|s| s.parse().ok()); }
            Some("Rate")    => { rate    = parts.nth(1).and_then(|s| s.parse().ok()); }
            _ => {}
        }
    }
    match (current, voltage, rate) {
        (Some(c), Some(v), Some(r)) => Some((c, v, r)),
        _ => None,
    }
}
