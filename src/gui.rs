// gui.rs — Minimal GUI shell for FROST (header + theme options only)

use eframe::egui;
use crate::worker::{DeviceSnapshot, GuiCommand, SerialWorker};
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
            let worker = SerialWorker::spawn(cc.egui_ctx.clone());
            Box::new(FrostApp::new(worker))
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
    worker: SerialWorker,

    // ── User-editable fields (live on the GUI side) ───────────
    magnet_target_current:        f64,
    magnet_edit_current_limit:    f64,
    magnet_edit_voltage_limit:    f64,
    magnet_edit_rate_limit:       f64,
    magnet_edit_ramp_rate:        f64,
    magnet_edit_compliance_voltage: f64,
    gl7_edit_pct: Vec<f64>,

    // ── Sync tracking: detect new poll data to refresh edit fields ──
    last_synced_magnet: Option<Instant>,
    last_synced_gl7:    Option<Instant>,

    // ── Command feedback (drained from snapshot each frame) ───
    compressor_error:         Option<String>,
    magnet_error:             Option<String>,
    magnet_limits_set_msg:    Option<Result<(), String>>,
    magnet_rate_set_msg:      Option<Result<(), String>>,
    magnet_compliance_set_msg: Option<Result<(), String>>,
    gl7_set_msg: Vec<Option<Result<(), String>>>,

    // ── Temperature recording ─────────────────────────────────
    record_result:        Option<Result<String, String>>,
    recording_stop_flag:  Option<Arc<AtomicBool>>,
    recording_csv_path:   Option<String>,

    // ── ADR ramp ──────────────────────────────────────────────
    adr_ramp_rate:      f64,
    adr_ramp_current:   f64,
    adr_ramp_soak_mins: u64,
    adr_ramp_result:    Option<Result<(), String>>,
}

impl FrostApp {
    fn new(worker: SerialWorker) -> Self {
        let (recording_stop_flag, recording_csv_path, record_result) =
            if crate::record_temps::is_recording_active() {
                match crate::record_temps::start_recording_loop(30, "temps") {
                    Ok((path, flag)) => (
                        Some(flag),
                        Some(path.clone()),
                        Some(Ok(format!("Resumed recording → {path}"))),
                    ),
                    Err(e) => (None, None, Some(Err(e))),
                }
            } else {
                (None, None, None)
            };

        Self {
            selected_theme: Theme::EguiLightBlue,
            worker,
            magnet_target_current:          9.44,
            magnet_edit_current_limit:      10.0,
            magnet_edit_voltage_limit:       1.0,
            magnet_edit_rate_limit:          0.1,
            magnet_edit_ramp_rate:           0.01,
            magnet_edit_compliance_voltage:  1.0,
            gl7_edit_pct: vec![0.0; 4],
            last_synced_magnet: None,
            last_synced_gl7:    None,
            compressor_error:          None,
            magnet_error:              None,
            magnet_limits_set_msg:     None,
            magnet_rate_set_msg:       None,
            magnet_compliance_set_msg: None,
            gl7_set_msg: vec![None, None, None, None],
            record_result,
            recording_stop_flag,
            recording_csv_path,
            adr_ramp_rate:      0.004,
            adr_ramp_current:   9.44,
            adr_ramp_soak_mins: 45,
            adr_ramp_result:    None,
        }
    }

    /// Sync hardware-polled values into GUI edit fields when new data arrives.
    fn sync_edit_fields(&mut self, snap: &DeviceSnapshot) {
        if snap.last_magnet_update != self.last_synced_magnet {
            if let Some(v) = snap.magnet_polled_current_limit      { self.magnet_edit_current_limit      = v; }
            if let Some(v) = snap.magnet_polled_voltage_limit      { self.magnet_edit_voltage_limit      = v; }
            if let Some(v) = snap.magnet_polled_rate_limit         { self.magnet_edit_rate_limit         = v; }
            if let Some(v) = snap.magnet_polled_ramp_rate          { self.magnet_edit_ramp_rate          = v; self.adr_ramp_rate = v; }
            if let Some(v) = snap.magnet_polled_compliance_voltage { self.magnet_edit_compliance_voltage = v; }
            if let Some(v) = snap.magnet_polled_target_current     { self.magnet_target_current          = v; }
            self.last_synced_magnet = snap.last_magnet_update;
        }
        if snap.last_gl7_update != self.last_synced_gl7 {
            for (i, pct) in snap.gl7_polled_pct.iter().enumerate() {
                if let Some(v) = pct { self.gl7_edit_pct[i] = *v; }
            }
            self.last_synced_gl7 = snap.last_gl7_update;
        }
    }
}

impl eframe::App for FrostApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.apply_theme(ctx);

        // ── 1. Drain command results and clone snapshot (brief mutex hold) ──
        let snap = {
            let mut s = self.worker.snapshot.lock().unwrap();

            if let Some(r) = s.compressor_cmd_result.take() {
                match r {
                    Ok(())  => self.compressor_error = None,
                    Err(e)  => self.compressor_error = Some(e),
                }
            }
            if let Some(r) = s.magnet_cmd_result.take() {
                match r {
                    Ok(())  => self.magnet_error = None,
                    Err(e)  => self.magnet_error = Some(e),
                }
            }
            if let Some(r) = s.magnet_rate_result.take()       { self.magnet_rate_set_msg       = Some(r); }
            if let Some(r) = s.magnet_compliance_result.take() { self.magnet_compliance_set_msg  = Some(r); }
            if let Some(r) = s.magnet_limits_result.take()     { self.magnet_limits_set_msg      = Some(r); }
            for i in 0..4 {
                if let Some(r) = s.gl7_set_results[i].take()   { self.gl7_set_msg[i]            = Some(r); }
            }
            if let Some(r) = s.adr_ramp_result.take() { self.adr_ramp_result = Some(r); }

            s.clone()
        };

        // ── 2. Sync edit fields when new poll data arrives ────────────────
        self.sync_edit_fields(&snap);

        // ── 3. Render ─────────────────────────────────────────────────────
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
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

                self.show_compressor_section(ui, &snap);

                ui.add_space(20.0);
                ui.separator();
                ui.add_space(10.0);

                self.show_magnet_section(ui, &snap, ctx);

                ui.add_space(20.0);
                ui.separator();
                ui.add_space(10.0);

                self.show_gl7_section(ui, &snap);

                ui.add_space(20.0);
                ui.separator();
                ui.add_space(10.0);

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

                self.show_temperature_display(ui, &snap);
            });
        });

        // ── 4. Repaint every second to keep "X s ago" counters ticking ───
        ctx.request_repaint_after(Duration::from_secs(1));
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

        if style.visuals.dark_mode {
            style.visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(45, 45, 55);
            style.visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(55, 80, 130);
            style.visuals.widgets.active.bg_fill = egui::Color32::from_rgb(45, 110, 220);
        } else {
            style.visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(255, 255, 255);
            style.visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(210, 230, 255);
            style.visuals.widgets.active.bg_fill = egui::Color32::from_rgb(120, 170, 255);
        }

        style.visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1.0, egui::Color32::from_gray(120));
        style.visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, egui::Color32::from_gray(140));
        style.visuals.widgets.active.bg_stroke = egui::Stroke::new(1.0, egui::Color32::from_gray(160));

        ctx.set_style(style);
    }

    fn show_compressor_section(&mut self, ui: &mut egui::Ui, snap: &DeviceSnapshot) {
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

        ui.horizontal(|ui| {
            if snap.compressor_running {
                let btn = egui::Button::new(
                    egui::RichText::new("⏹  Stop Compressor").strong()
                )
                .fill(egui::Color32::from_rgb(180, 40, 40));
                if ui.add(btn).clicked() {
                    self.compressor_error = None;
                    self.worker.send(GuiCommand::StopCompressor);
                }
            } else {
                let btn = egui::Button::new(
                    egui::RichText::new("▶  Start Pulse Tube Cooldown").strong()
                )
                .fill(egui::Color32::from_rgb(30, 120, 60));
                if ui.add(btn).clicked() {
                    self.compressor_error = None;
                    self.worker.send(GuiCommand::StartCompressor);
                }
            }
        });

        if let Some(ref e) = self.compressor_error {
            ui.colored_label(egui::Color32::RED, format!("Compressor error: {e}"));
        }

        ui.add_space(6.0);

        if !snap.compressor_status.is_empty() {
            for line in snap.compressor_status.lines() {
                if line.starts_with("Runtime:") {
                    continue;
                } else if line.starts_with("Running:") {
                    let is_yes = line.contains("Yes");
                    let color = if is_yes {
                        egui::Color32::from_rgb(20, 140, 20)
                    } else {
                        egui::Color32::from_rgb(160, 30, 30)
                    };
                    ui.add(egui::Label::new(
                        egui::RichText::new(line).strong().size(22.0).color(color),
                    ).selectable(false));
                } else if line.starts_with("Errors/Warnings:") {
                    let has_errors = line.contains("Yes");
                    let color = if has_errors {
                        egui::Color32::from_rgb(200, 80, 0)
                    } else {
                        egui::Color32::DARK_GREEN
                    };
                    ui.add(egui::Label::new(
                        egui::RichText::new(line).strong().size(18.0).color(color),
                    ).selectable(false));
                } else {
                    ui.label(line);
                }
            }
            if let Some(t) = snap.last_compressor_update {
                ui.label(format!(
                    "Last updated: {:.1}s ago  (refreshes every 30 s)",
                    t.elapsed().as_secs_f32()
                ));
            }
        } else {
            ui.label("Compressor status: (pending first poll…)");
        }
    }

    fn show_magnet_section(&mut self, ui: &mut egui::Ui, snap: &DeviceSnapshot, ctx: &egui::Context) {
        ui.add(
            egui::Label::new(
                egui::RichText::new("ADR Cooldown")
                    .size(32.0)
                    .strong()
                    .color(egui::Color32::from_rgb(40, 40, 140)),
            )
            .selectable(false),
        );
        ui.add_space(6.0);

        // ── Live readback cards ──────────────────────────────────
        {
            let current_str = if snap.magnet_current.is_empty() {
                "—".to_string()
            } else {
                format!("{} A", snap.magnet_current)
            };
            let voltage_str = if snap.magnet_voltage.is_empty() {
                "—".to_string()
            } else {
                format!("{} V", snap.magnet_voltage)
            };
            let field_str = if snap.magnet_field.is_empty() {
                "—".to_string()
            } else {
                format!("{} T", snap.magnet_field)
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

        // ── Interrupted-ramp warning (set when lock file found on startup) ──
        if snap.adr_ramp_interrupted {
            ui.add_space(4.0);
            ui.colored_label(
                egui::Color32::from_rgb(200, 120, 0),
                "⚠  ADR ramp was running when the GUI was last closed — it did not complete.",
            );
            ui.add_space(4.0);
        }

        // ── Start button / running indicator ─────────────────────
        if snap.adr_ramp_running {
            let elapsed = snap.adr_ramp_started
                .map(|t| t.elapsed().as_secs())
                .unwrap_or(0);
            let mins = elapsed / 60;
            let secs = elapsed % 60;
            let btn = egui::Button::new(
                egui::RichText::new(
                    format!("⏳  ADR Ramp Running…  {mins}m {secs:02}s elapsed")
                ).strong(),
            )
            .fill(egui::Color32::from_rgb(140, 100, 20));
            ui.add_enabled(false, btn);
        } else {
            let btn = egui::Button::new(
                egui::RichText::new("▶  Start Automated ADR Ramp").strong(),
            )
            .fill(egui::Color32::from_rgb(40, 80, 170));
            if ui.add(btn).clicked() {
                self.adr_ramp_result = None;
                self.worker.send(GuiCommand::RunAdrRamp {
                    rate:      self.adr_ramp_rate,
                    current:   self.adr_ramp_current,
                    soak_mins: self.adr_ramp_soak_mins,
                });
            }
        }

        // ── Live ramp log ─────────────────────────────────────────
        if !snap.adr_log_lines.is_empty() || snap.adr_ramp_running {
            ui.add_space(6.0);
            egui::Frame::none()
                .fill(egui::Color32::from_gray(18))
                .rounding(egui::Rounding::same(6.0))
                .inner_margin(egui::Margin::same(8.0))
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    egui::ScrollArea::vertical()
                        .id_source("adr_log_scroll")
                        .max_height(200.0)
                        .stick_to_bottom(true)
                        .show(ui, |ui| {
                            for line in &snap.adr_log_lines {
                                ui.add(egui::Label::new(
                                    egui::RichText::new(line)
                                        .monospace()
                                        .size(12.0)
                                        .color(egui::Color32::from_rgb(160, 210, 160)),
                                ).selectable(false));
                            }
                            if !snap.adr_status_line.is_empty() {
                                ui.add(egui::Label::new(
                                    egui::RichText::new(&snap.adr_status_line)
                                        .monospace()
                                        .size(12.0)
                                        .color(egui::Color32::YELLOW),
                                ).selectable(false));
                            }
                        });
                });
        }

        // Request repaint every second while running to keep the elapsed timer fresh.
        if snap.adr_ramp_running {
            ctx.request_repaint_after(std::time::Duration::from_secs(1));
        }

        // ── Result feedback ───────────────────────────────────────
        if let Some(ref res) = self.adr_ramp_result {
            ui.add_space(4.0);
            match res {
                Ok(())  => { ui.colored_label(egui::Color32::DARK_GREEN, "✔ ADR ramp sequence complete."); }
                Err(e)  => { ui.colored_label(egui::Color32::RED, format!("ADR ramp error: {e}")); }
            }
        }

        ui.add_space(8.0);

        // ── Ramp parameters ───────────────────────────────────────
        egui::Grid::new("adr_ramp_params_grid")
            .num_columns(6)
            .spacing([8.0, 6.0])
            .show(ui, |ui| {
                ui.label("Target current:");
                ui.add(
                    egui::DragValue::new(&mut self.adr_ramp_current)
                        .speed(0.01)
                        .clamp_range(0.0_f64..=60.0_f64)
                        .fixed_decimals(2)
                        .suffix(" A"),
                );
                ui.label("Ramp rate:");
                ui.add(
                    egui::DragValue::new(&mut self.adr_ramp_rate)
                        .speed(0.0001)
                        .clamp_range(0.0001_f64..=0.0055_f64)
                        .fixed_decimals(4)
                        .suffix(" A/s"),
                );
                ui.label("Soak duration:");
                let mut soak = self.adr_ramp_soak_mins as f64;
                if ui.add(
                    egui::DragValue::new(&mut soak)
                        .speed(1.0)
                        .clamp_range(1.0_f64..=480.0_f64)
                        .fixed_decimals(0)
                        .suffix(" min"),
                ).changed() {
                    self.adr_ramp_soak_mins = soak as u64;
                }
                ui.end_row();
            });

        // ── Compliance voltage & Limits (commented out, re-enable if needed) ──
        // ui.add_space(8.0);
        // ui.columns(2, |cols| {
        //     // ── Left column: Compliance voltage ──────────────────
        //     cols[0].strong("Compliance");
        //     cols[0].add_space(4.0);
        //     egui::Grid::new("magnet_ramp_grid")
        //         .num_columns(4)
        //         .spacing([8.0, 6.0])
        //         .show(&mut cols[0], |ui| {
        //             ui.label("Compliance V:");
        //             ui.add(
        //                 egui::DragValue::new(&mut self.magnet_edit_compliance_voltage)
        //                     .speed(0.01)
        //                     .clamp_range(0.1_f64..=5.0_f64)
        //                     .fixed_decimals(2),
        //             );
        //             ui.label("V");
        //             let comp_btn = egui::Button::new(egui::RichText::new("Set Compliance").strong())
        //                 .fill(egui::Color32::from_rgb(80, 120, 60));
        //             if ui.add(comp_btn).clicked() {
        //                 let v = self.magnet_edit_compliance_voltage;
        //                 self.magnet_compliance_set_msg = None;
        //                 self.worker.send(GuiCommand::SetMagnetCompliance { voltage: v });
        //             }
        //             ui.end_row();
        //             ui.label("");
        //             if let Some(ref msg) = self.magnet_compliance_set_msg.clone() {
        //                 match msg {
        //                     Ok(()) => { ui.colored_label(egui::Color32::DARK_GREEN, "✔ Compliance set."); }
        //                     Err(e) => { ui.colored_label(egui::Color32::RED, e.as_str()); }
        //                 }
        //             }
        //             ui.end_row();
        //         });
        //     // ── Right column: Limits ─────────────────────────────
        //     cols[1].strong("Limits (LIMIT?)");
        //     if snap.magnet_limits.starts_with("Error:") {
        //         cols[1].colored_label(egui::Color32::RED, &snap.magnet_limits.clone());
        //     } else if snap.magnet_limits.is_empty() {
        //         cols[1].label("(pending first poll…)");
        //     }
        //     egui::Grid::new("magnet_limits_grid")
        //         .num_columns(3)
        //         .spacing([8.0, 4.0])
        //         .show(&mut cols[1], |ui| {
        //             ui.label("Current limit:");
        //             ui.add(
        //                 egui::DragValue::new(&mut self.magnet_edit_current_limit)
        //                     .speed(0.1)
        //                     .clamp_range(0.0_f64..=60.1_f64)
        //                     .fixed_decimals(2),
        //             );
        //             ui.label("A");
        //             ui.end_row();
        //             ui.label("Voltage limit:");
        //             ui.add(
        //                 egui::DragValue::new(&mut self.magnet_edit_voltage_limit)
        //                     .speed(0.01)
        //                     .clamp_range(0.1_f64..=5.0_f64)
        //                     .fixed_decimals(2),
        //             );
        //             ui.label("V");
        //             ui.end_row();
        //             ui.label("Rate limit:");
        //             ui.add(
        //                 egui::DragValue::new(&mut self.magnet_edit_rate_limit)
        //                     .speed(0.001)
        //                     .clamp_range(0.0001_f64..=99.999_f64)
        //                     .fixed_decimals(4),
        //             );
        //             ui.label("A/s");
        //             ui.end_row();
        //         });
        //     cols[1].horizontal(|ui| {
        //         let set_btn = egui::Button::new(egui::RichText::new("Set Limits").strong())
        //             .fill(egui::Color32::from_rgb(80, 120, 60));
        //         if ui.add(set_btn).clicked() {
        //             let c = self.magnet_edit_current_limit;
        //             let v = self.magnet_edit_voltage_limit;
        //             let r = self.magnet_edit_rate_limit;
        //             self.magnet_limits_set_msg = None;
        //             self.worker.send(GuiCommand::SetMagnetLimits { current: c, voltage: v, rate: r });
        //         }
        //         if let Some(ref msg) = self.magnet_limits_set_msg.clone() {
        //             match msg {
        //                 Ok(()) => { ui.colored_label(egui::Color32::DARK_GREEN, "✔ Limits updated."); }
        //                 Err(e) => { ui.colored_label(egui::Color32::RED, format!("Error: {e}")); }
        //             }
        //         }
        //     });
        //     if !snap.magnet_quench.is_empty() {
        //         cols[1].add_space(4.0);
        //         for line in snap.magnet_quench.lines() {
        //             cols[1].label(line);
        //         }
        //     }
        // });
        // ui.add_space(4.0);
        // if let Some(t) = snap.last_magnet_update {
        //     if !snap.magnet_limits.is_empty() && !snap.magnet_limits.starts_with("Error:") {
        //         ui.label(format!(
        //             "Last updated: {:.1}s ago  (refreshes every 30 s)",
        //             t.elapsed().as_secs_f32()
        //         ));
        //     }
        // }
    }

    fn show_gl7_section(&mut self, ui: &mut egui::Ui, snap: &DeviceSnapshot) {
        ui.add(
            egui::Label::new(
                egui::RichText::new("GL7 Sorption Cooler")
                    .size(32.0)
                    .strong()
                    .color(egui::Color32::from_rgb(40, 40, 140)),
            )
            .selectable(false),
        );
        ui.add_space(6.0);

        let lines = snap.gl7_output_lines.clone();
        let output_names = ["4-Pump Heater", "3-Pump Heater", "4-Switch Heater", "3-Switch Heater"];

        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);
            for (i, (l1, l2)) in lines.iter().enumerate() {
                let output_num = i + 1;
                let label = output_names[i];
                egui::Frame::none()
                    .fill(egui::Color32::from_rgb(255, 243, 220))
                    .stroke(egui::Stroke::new(1.5, egui::Color32::from_rgb(180, 130, 50)))
                    .rounding(egui::Rounding::same(8.0))
                    .inner_margin(egui::Margin::same(10.0))
                    .show(ui, |ui| {
                        ui.set_min_width(220.0);
                        ui.vertical(|ui| {
                            ui.add(egui::Label::new(
                                egui::RichText::new(label).strong().size(14.0),
                            ).selectable(false));
                            ui.add(egui::Label::new(
                                egui::RichText::new(format!("LS350 · Output {output_num}"))
                                    .size(10.5)
                                    .color(egui::Color32::from_gray(110)),
                            ).selectable(false));
                            ui.add_space(4.0);
                            if l1.is_empty() {
                                ui.add(egui::Label::new(
                                    egui::RichText::new("(pending…)")
                                        .size(12.0)
                                        .monospace()
                                        .color(egui::Color32::from_gray(150)),
                                ).selectable(false));
                            } else {
                                ui.add(egui::Label::new(
                                    egui::RichText::new(l1.as_str()).size(12.0).monospace(),
                                ).selectable(false));
                                if !l2.is_empty() {
                                    ui.add(egui::Label::new(
                                        egui::RichText::new(l2.as_str()).size(12.0).monospace(),
                                    ).selectable(false));
                                }
                            }
                            ui.add_space(6.0);

                            ui.horizontal(|ui| {
                                ui.add(
                                    egui::DragValue::new(&mut self.gl7_edit_pct[i])
                                        .speed(0.5)
                                        .clamp_range(0.0_f64..=100.0_f64)
                                        .fixed_decimals(1)
                                        .suffix(" %"),
                                );
                                let set_btn = egui::Button::new(
                                    egui::RichText::new("Set").strong()
                                )
                                .fill(egui::Color32::from_rgb(80, 120, 60));
                                if ui.add(set_btn).clicked() {
                                    let pct = self.gl7_edit_pct[i];
                                    let out_num = output_num as u8;
                                    self.gl7_set_msg[i] = None;
                                    self.worker.send(GuiCommand::SetGl7Output { output: out_num, pct });
                                }
                            });
                            if let Some(ref msg) = self.gl7_set_msg[i].clone() {
                                match msg {
                                    Ok(()) => { ui.colored_label(egui::Color32::DARK_GREEN, "✔ Set."); }
                                    Err(e)  => { ui.colored_label(egui::Color32::RED, e.as_str()); }
                                }
                            }
                        });
                    });
            }
        });

        ui.add_space(4.0);
        if let Some(t) = snap.last_gl7_update {
            ui.label(format!(
                "Last updated: {:.1}s ago  (refreshes every 30 s)",
                t.elapsed().as_secs_f32()
            ));
        } else {
            ui.label("(pending first poll…)");
        }
    }

    fn show_temperature_display(&mut self, ui: &mut egui::Ui, snap: &DeviceSnapshot) {
        let is_recording = self.recording_stop_flag
            .as_ref()
            .map(|f| !f.load(Ordering::Relaxed))
            .unwrap_or(false);

        ui.horizontal(|ui| {
            ui.strong("Temperature Readings");
            ui.add_space(12.0);

            if is_recording {
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
                let btn = egui::Button::new(
                    egui::RichText::new("⏺  Record Temperatures").strong()
                )
                .fill(egui::Color32::from_rgb(30, 120, 60));
                if ui.add(btn).clicked() {
                    match crate::record_temps::start_recording_loop(30, "temps") {
                        Ok((path, flag)) => {
                            self.recording_csv_path = Some(path.clone());
                            self.recording_stop_flag = Some(flag);
                            self.record_result = Some(Ok(format!("Recording to: {path}")));
                        }
                        Err(e) => {
                            self.record_result = Some(Err(e));
                        }
                    }
                }
            }
        });

        if let Some(ref res) = self.record_result {
            match res {
                Ok(msg)  => { ui.colored_label(egui::Color32::DARK_GREEN, msg); }
                Err(err) => { ui.colored_label(egui::Color32::RED, format!("Record error: {err}")); }
            }
        }

        ui.add_space(6.0);

        let t   = &snap.temperatures;
        let adr_temp = t.ls350_b.split('\u{2192}').nth(1)
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|| t.ls350_b.clone());
        let elapsed = snap.last_temp_update.map(|t| t.elapsed().as_secs_f32());

        let sensors: &[(&str, &str, &str)] = &[
            ("4K Stage",     "LS350 · D3",  &t.ls350_d3),
            ("ADR",          "LS350 · B",   &adr_temp),
            ("Switch",       "LS350 · D2",  &t.ls350_d2),
            ("3-Head",       "LS350 · A",   &t.ls350_a),
            ("4-Head",       "LS350 · C",   &t.ls350_c),
            ("3-Pump",       "LS350 · D4",  &t.ls350_d4),
            ("4-Pump",       "LS350 · D5",  &t.ls350_d5),
            ("Device Stage", "LS370 · In1", &t.ls370_1),
        ];

        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);
            for &(name, id, val) in sensors {
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
        if let Some(e) = elapsed {
            ui.label(format!("Last updated: {e:.1}s ago"));
        } else {
            ui.label("(pending first poll…)");
        }
        ui.label("Updates every 30 seconds");
    }
}
