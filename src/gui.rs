// gui.rs — Minimal GUI shell for FROST (header + theme options only)

use eframe::egui;
use crate::worker::{DeviceSnapshot, GuiCommand, SerialWorker, POLL_INTERVAL};
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

/// Render a raw magnet reading string with its unit, or an em-dash if there is
/// no reading. Both no-data representations that can reach the snapshot — an
/// empty string (from `poll_magnet`, which uses `unwrap_or_default()` on a
/// failed read) and `"NO_RESPONSE"` (from the ADR readout state file written by
/// the ramp subprocess) — collapse to the same `—`, so a dead LS625 read looks
/// identical whether or not a ramp is in progress.
pub fn magnet_reading_display(raw: &str, unit: &str) -> String {
    let raw = raw.trim();
    if raw.is_empty() || raw == "NO_RESPONSE" {
        "—".to_string()
    } else {
        format!("{raw} {unit}")
    }
}

struct FrostApp {
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

    // ── Heatswitch ────────────────────────────────────────────
    heatswitch_error: Option<String>,

    // ── GL7 cooldown ─────────────────────────────────────────
    gl7_cooldown_csv_path: String,
    gl7_cooldown_result:   Option<Result<String, String>>,
    gl7_cooldown_child:    Option<std::process::Child>,
    /// PID read from lock file on startup — used to kill the subprocess after a GUI reload.
    gl7_cooldown_pid:      Option<u32>,

    // ── Temperature recording ─────────────────────────────────
    record_result: Option<Result<String, String>>,

    // ── ADR ramp ──────────────────────────────────────────────
    adr_ramp_rate:      f64,
    adr_ramp_current:   f64,
    adr_ramp_soak_mins: u64,
    adr_ramp_result:    Option<Result<(), String>>,
    /// Child handle when the subprocess was spawned in this GUI session.
    adr_ramp_child:     Option<std::process::Child>,
    /// PID from lock file — used to kill the subprocess after a GUI reload.
    adr_ramp_pid:       Option<u32>,
}

impl FrostApp {
    fn new(worker: SerialWorker) -> Self {
        Self {
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
            heatswitch_error:      None,
            gl7_cooldown_csv_path: String::new(),
            gl7_cooldown_result:   None,
            gl7_cooldown_child:    None,
            gl7_cooldown_pid:      crate::worker::get_gl7_cooldown_pid(),
            record_result:    None,
            adr_ramp_rate:      0.004,
            adr_ramp_current:   9.44,
            adr_ramp_soak_mins: 45,
            adr_ramp_result:    None,
            adr_ramp_child:     None,
            adr_ramp_pid:       crate::worker::get_adr_ramp_pid(),
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

    /// Gate a start against the worker's cached snapshot (Option A). Returns
    /// `Ok(())` if the process may start, or `Err(reason)` to display. When
    /// safety is OFF this always returns `Ok(())`.
    fn safety_gate(&self, snap: &DeviceSnapshot, context: &str) -> Result<(), String> {
        crate::safety::guard_start_from_snapshot(
            context,
            snap.compressor_running,
            snap.last_compressor_update.map(|t| t.elapsed()),
            &snap.temperatures.ls350_d3,
            snap.last_temp_update.map(|t| t.elapsed()),
            std::time::Duration::from_secs(crate::safety::SNAPSHOT_MAX_AGE_SECS),
        )
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
            if let Some(r) = s.recording_start_result.take() {
                self.record_result = Some(r);
            }
            if let Some(r) = s.heatswitch_cmd_result.take() {
                match r {
                    Ok(())  => self.heatswitch_error = None,
                    Err(e)  => self.heatswitch_error = Some(e),
                }
            }

            s.clone()
        };

        // ── 2. Sync edit fields when new poll data arrives ────────────────
        self.sync_edit_fields(&snap);

        // ── 3. Render ─────────────────────────────────────────────────────

        // Status bar — always visible at the top, does not scroll
        let status_frame = egui::Frame::none()
            .fill(egui::Color32::from_rgb(38, 55, 95))
            .inner_margin(egui::Margin::symmetric(14.0, 7.0));
        egui::TopBottomPanel::top("status_bar")
            .frame(status_frame)
            .show(ctx, |ui| {
                self.show_status_bar(ui, &snap);
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.add_space(8.0);

                // ── Safety interlock toggle + override banner ─────────────
                let safety_on = crate::safety::is_safety_enabled();
                ui.horizontal(|ui| {
                    let (label, fill) = if safety_on {
                        ("Safety: ON", egui::Color32::from_rgb(30, 130, 60))
                    } else {
                        ("Safety: OFF", egui::Color32::from_rgb(185, 30, 30))
                    };
                    let btn = egui::Button::new(
                        egui::RichText::new(label).strong().color(egui::Color32::WHITE),
                    )
                    .fill(fill);
                    if ui.add(btn).clicked() {
                        // Toggle and persist. A failure to re-enable leaves
                        // interlocks bypassed, so log it as a safety event and
                        // surface it in the compressor error slot.
                        if let Err(e) = crate::safety::set_safety(!safety_on) {
                            let msg = format!("Safety toggle to {} failed: {e}",
                                if safety_on { "OFF" } else { "ON" });
                            crate::safety::log_safety_event(&msg);
                            self.compressor_error = Some(msg);
                        }
                    }
                    ui.label(if safety_on {
                        "Start interlocks active"
                    } else {
                        "Start interlocks bypassed"
                    });
                });
                if !safety_on {
                    ui.add_space(2.0);
                    ui.label(
                        egui::RichText::new("⚠ Safety OFF — start interlocks are bypassed")
                            .strong()
                            .size(16.0)
                            .color(egui::Color32::from_rgb(220, 60, 60)),
                    );
                }
                ui.add_space(6.0);

                // Header row: FROST title on the left, heatswitch toggle on the right.
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
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
                    });

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let is_open = snap.heatswitch_is_open;
                        // right_to_left: first added → rightmost. Visual order left→right:
                        //   "Heatswitch"  [toggle]  OPEN/CLOSED
                        let (state_label, state_color) = match is_open {
                            Some(true)  => ("OPEN",    egui::Color32::from_rgb(20, 140, 20)),
                            Some(false) => ("CLOSED",  egui::Color32::from_rgb(140, 30, 30)),
                            None        => ("UNKNOWN", egui::Color32::from_rgb(130, 100, 0)),
                        };
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(state_label)
                                    .strong()
                                    .size(16.0)
                                    .color(state_color),
                            )
                            .selectable(false),
                        );
                        ui.add_space(6.0);
                        let resp = self.heatswitch_toggle(ui, is_open);
                        if resp.clicked() {
                            self.heatswitch_error = None;
                            match is_open {
                                Some(true) => self.worker.send(GuiCommand::CloseHeatswitch),
                                _          => self.worker.send(GuiCommand::OpenHeatswitch),
                            }
                        }
                        ui.add_space(8.0);
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new("Heatswitch")
                                    .size(13.0)
                                    .color(egui::Color32::from_rgb(100, 100, 140)),
                            )
                            .selectable(false),
                        );
                    });
                });

                if let Some(ref e) = self.heatswitch_error {
                    ui.colored_label(egui::Color32::RED, format!("Heatswitch error: {e}"));
                }

                ui.add_space(14.0);
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
            });
        });

        // ── 4. Repaint every second to keep "X s ago" counters ticking ───
        ctx.request_repaint_after(Duration::from_secs(1));
    }
}

impl FrostApp {
    fn show_status_bar(&self, ui: &mut egui::Ui, snap: &DeviceSnapshot) {
        let is_recording = snap.recording_active;

        let chips: &[(&str, bool)] = &[
            ("Compressor",  snap.compressor_running),
            ("ADR Ramp",    snap.adr_ramp_running),
            ("GL7",         snap.gl7_cooldown_active),
            ("Recording",   is_recording),
            ("Heatswitch",  snap.heatswitch_is_open == Some(true)),
        ];

        ui.horizontal(|ui| {
            ui.add(egui::Label::new(
                egui::RichText::new("STATUS")
                    .strong()
                    .size(11.0)
                    .color(egui::Color32::from_rgb(170, 195, 255)),
            ).selectable(false));

            ui.add_space(10.0);

            for &(label, active) in chips {
                let (bg, fg, dot) = if active {
                    (
                        egui::Color32::from_rgb(28, 90, 48),
                        egui::Color32::from_rgb(120, 230, 150),
                        "●",
                    )
                } else {
                    (
                        egui::Color32::from_rgb(50, 60, 88),
                        egui::Color32::from_rgb(170, 185, 220),
                        "○",
                    )
                };
                egui::Frame::none()
                    .fill(bg)
                    .rounding(egui::Rounding::same(5.0))
                    .inner_margin(egui::Margin::symmetric(9.0, 4.0))
                    .show(ui, |ui| {
                        ui.add(egui::Label::new(
                            egui::RichText::new(format!("{dot}  {label}"))
                                .size(12.5)
                                .color(fg)
                                .strong(),
                        ).selectable(false));
                    });
                ui.add_space(4.0);
            }
        });
    }

    fn apply_theme(&self, ctx: &egui::Context) {
        let mut style = (*ctx.style()).clone();
        style.visuals = egui::Visuals::light();
        style.visuals.window_fill = egui::Color32::from_rgb(232, 240, 255);
        style.visuals.panel_fill = egui::Color32::from_rgb(244, 248, 255);
        style.visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(255, 255, 255);
        style.visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(210, 230, 255);
        style.visuals.widgets.active.bg_fill = egui::Color32::from_rgb(120, 170, 255);
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
                    egui::RichText::new("⏹  Stop Compressor").strong().size(18.0)
                )
                .fill(egui::Color32::from_rgb(180, 40, 40));
                if ui.add(btn).clicked() {
                    self.compressor_error = None;
                    self.worker.send(GuiCommand::StopCompressor);
                }
            } else {
                let btn = egui::Button::new(
                    egui::RichText::new("▶  Start Pulse Tube Cooldown").strong().size(18.0)
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
                    "Last updated: {:.1}s ago  (refreshes every {} s)",
                    t.elapsed().as_secs_f32(),
                    POLL_INTERVAL.as_secs()
                ));
            }
        } else {
            ui.label("Compressor status: (pending first poll…)");
        }
    }

    /// Draw a physical toggle switch.  Returns the allocated response so the
    /// caller can detect clicks.  Thumb sits LEFT when open, RIGHT when closed.
    fn heatswitch_toggle(&self, ui: &mut egui::Ui, is_open: Option<bool>) -> egui::Response {
        let track_w = 120.0f32;
        let track_h = 40.0f32;
        let thumb_r = 15.0f32;

        let desired_size = egui::vec2(track_w, track_h);
        let (rect, response) = ui.allocate_exact_size(desired_size, egui::Sense::click());

        if ui.is_rect_visible(rect) {
            let painter = ui.painter();

            let track_color = match is_open {
                Some(true)  => egui::Color32::from_rgb(28, 155, 72),
                Some(false) => egui::Color32::from_rgb(62, 72, 92),
                None        => egui::Color32::from_rgb(135, 108, 28),
            };
            painter.rect_filled(rect, egui::Rounding::same(track_h / 2.0), track_color);

            if response.hovered() {
                painter.rect_filled(
                    rect,
                    egui::Rounding::same(track_h / 2.0),
                    egui::Color32::from_rgba_unmultiplied(255, 255, 255, 22),
                );
            }

            let center_y = rect.center().y;
            let label_color = egui::Color32::from_rgba_unmultiplied(255, 255, 255, 170);
            let font = egui::FontId::proportional(11.5);

            painter.text(
                egui::Pos2::new(rect.left() + 13.0, center_y),
                egui::Align2::LEFT_CENTER,
                "OPEN",
                font.clone(),
                label_color,
            );
            painter.text(
                egui::Pos2::new(rect.right() - 13.0, center_y),
                egui::Align2::RIGHT_CENTER,
                "CLOSE",
                font,
                label_color,
            );

            let thumb_x = match is_open {
                Some(true)  => rect.left()   + thumb_r + 3.0,
                Some(false) => rect.right()  - thumb_r - 3.0,
                None        => rect.center().x,
            };
            let thumb_center = egui::Pos2::new(thumb_x, center_y);

            painter.circle_filled(thumb_center, thumb_r, egui::Color32::WHITE);
            painter.circle_stroke(
                thumb_center,
                thumb_r,
                egui::Stroke::new(1.5, egui::Color32::from_rgba_unmultiplied(0, 0, 0, 55)),
            );
        }

        response
    }

    fn show_magnet_section(&mut self, ui: &mut egui::Ui, snap: &DeviceSnapshot, _ctx: &egui::Context) {
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

        // ── Detect natural subprocess exit ────────────────────────
        let mut just_exited = false;
        if let Some(ref mut child) = self.adr_ramp_child {
            match child.try_wait() {
                Ok(Some(_)) | Err(_) => { just_exited = true; }
                Ok(None) => {}
            }
        }
        if just_exited {
            self.adr_ramp_child = None;
            self.adr_ramp_pid   = None;
        }
        // Keep adr_ramp_pid fresh when running from a previous session.
        if snap.adr_ramp_running && self.adr_ramp_child.is_none() && self.adr_ramp_pid.is_none() {
            self.adr_ramp_pid = crate::worker::get_adr_ramp_pid();
        }

        // ── Start / Stop button ───────────────────────────────────
        ui.horizontal(|ui| {
            if snap.adr_ramp_running {
                let btn = egui::Button::new(
                    egui::RichText::new("⏹  Stop ADR Cooldown (Ramp Down)")
                        .strong()
                        .size(18.0)
                        .color(egui::Color32::WHITE),
                )
                .fill(egui::Color32::from_rgb(185, 30, 30));
                if ui.add(btn).clicked() {
                    // Kill the subprocess — by handle if available, else by stored PID.
                    if let Some(mut child) = self.adr_ramp_child.take() {
                        let _ = child.kill();
                        let _ = child.wait();
                    } else if let Some(pid) = self.adr_ramp_pid.take() {
                        let _ = std::process::Command::new("kill")
                            .args(["-9", &pid.to_string()])
                            .status();
                    }
                    self.adr_ramp_result = None;
                    self.worker.send(GuiCommand::StopAdrRamp);
                }
            } else {
                let btn = egui::Button::new(
                    egui::RichText::new("▶  Start ADR Cooldown")
                        .strong()
                        .size(18.0)
                        .color(egui::Color32::from_rgb(15, 30, 80)),
                )
                .fill(egui::Color32::from_rgb(140, 185, 255));
                if ui.add(btn).clicked() {
                    // Gate against the cached snapshot before spawning. The
                    // subprocess is told (via env var) the check is already done,
                    // so it won't re-read serial and race the worker's poll.
                    if let Err(reason) = self.safety_gate(&snap, "ADR ramp") {
                        self.adr_ramp_result = Some(Err(reason));
                    } else {
                        // Spawn frost adr ramp as a detached subprocess so it survives GUI restarts.
                        match std::env::current_exe()
                            .map_err(|e| e.to_string())
                            .and_then(|exe| {
                                std::process::Command::new(&exe)
                                    .args([
                                        "adr", "ramp",
                                        &self.adr_ramp_rate.to_string(),
                                        &self.adr_ramp_current.to_string(),
                                        "--soak-mins",
                                        &self.adr_ramp_soak_mins.to_string(),
                                    ])
                                    .env(crate::safety::GUI_CHECKED_ENV, "1")
                                    .spawn()
                                    .map_err(|e| e.to_string())
                            })
                        {
                            Ok(child) => {
                                self.adr_ramp_pid   = Some(child.id());
                                self.adr_ramp_child = Some(child);
                                self.adr_ramp_result = None;
                                self.worker.send(GuiCommand::RunAdrRamp);
                            }
                            Err(e) => {
                                self.adr_ramp_result = Some(Err(format!("Failed to start ramp process: {e}")));
                            }
                        }
                    }
                }
            }
        });

        // ── Result feedback ───────────────────────────────────────
        if let Some(ref res) = self.adr_ramp_result {
            ui.add_space(4.0);
            match res {
                Ok(())  => { ui.colored_label(egui::Color32::DARK_GREEN, "✔ ADR ramp sequence complete."); }
                Err(e)  => { ui.colored_label(egui::Color32::RED, format!("ADR ramp error: {e}")); }
            }
        }

        ui.add_space(8.0);

        // ── Live readback cards ──────────────────────────────────
        {
            let current_str = magnet_reading_display(&snap.magnet_current, "A");
            let voltage_str = magnet_reading_display(&snap.magnet_voltage, "V");
            let field_str   = magnet_reading_display(&snap.magnet_field, "T");

            let cards: &[(&str, &str)] = &[
                ("Output Current", &current_str),
                ("Output Voltage", &voltage_str),
                ("Magnetic Field", &field_str),
            ];

            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);
                for &(name, val) in cards {
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

        // Check whether the GL7 subprocess has finished naturally.
        let mut gl7_just_exited = false;
        if let Some(ref mut child) = self.gl7_cooldown_child {
            match child.try_wait() {
                Ok(Some(_)) | Err(_) => { gl7_just_exited = true; }
                Ok(None)             => {}  // still running
            }
        }
        if gl7_just_exited {
            self.gl7_cooldown_child = None;
            self.gl7_cooldown_pid   = None;
            crate::worker::clear_gl7_cooldown_persisted();
            self.worker.send(GuiCommand::Gl7CooldownActive(false));
        }

        // Single source of truth, shared with the top-bar chip: the worker keeps
        // snap.gl7_cooldown_active synced from live outputs + the lock file each
        // loop, so this survives a GUI restart and can never disagree with the
        // chip. The in-memory child/PID handles below are used ONLY to kill the
        // subprocess, never to decide the label (mirrors the ADR ramp section).
        let gl7_running = snap.gl7_cooldown_active;

        // When running externally (CLI or previous session), keep the stored PID
        // fresh so the stop button can kill the right process.
        if gl7_running && self.gl7_cooldown_child.is_none() && self.gl7_cooldown_pid.is_none() {
            self.gl7_cooldown_pid = crate::worker::get_gl7_cooldown_pid();
        }

        ui.horizontal(|ui| {
            if gl7_running {
                let btn = egui::Button::new(
                    egui::RichText::new("⏹  Stop GL7 Cooldown")
                        .strong()
                        .size(18.0)
                        .color(egui::Color32::WHITE),
                )
                .fill(egui::Color32::from_rgb(185, 30, 30));
                if ui.add(btn).clicked() {
                    // Kill the subprocess — by handle if available, else by stored PID.
                    if let Some(mut child) = self.gl7_cooldown_child.take() {
                        let _ = child.kill();
                        // Wait for the process to exit so the OS reclaims its
                        // serial port file descriptor before we queue writes.
                        let _ = child.wait();
                    } else if let Some(pid) = self.gl7_cooldown_pid.take() {
                        let _ = std::process::Command::new("kill").arg(pid.to_string()).status();
                    }
                    crate::worker::clear_gl7_cooldown_persisted();
                    // Zero all GL7 outputs.
                    for output in 1u8..=4 {
                        self.worker.send(GuiCommand::SetGl7Output { output, pct: 0.0 });
                    }
                    self.worker.send(GuiCommand::Gl7CooldownActive(false));
                    self.gl7_cooldown_result =
                        Some(Ok("GL7 cooldown stopped. All outputs set to 0%.".to_string()));
                }
            } else {
                let cooldown_btn = egui::Button::new(
                    egui::RichText::new("Start GL7 Cooldown")
                        .strong()
                        .size(18.0)
                        .color(egui::Color32::from_rgb(15, 30, 80)),
                )
                .fill(egui::Color32::from_rgb(140, 185, 255));
                if ui.add(cooldown_btn).clicked() {
                    let path = self.gl7_cooldown_csv_path.trim().to_string();
                    if path.is_empty() {
                        self.gl7_cooldown_result = Some(Err("No CSV path specified.".to_string()));
                    } else if let Err(reason) = self.safety_gate(&snap, "GL7 cooldown") {
                        // Gate against the cached snapshot before spawning; the
                        // subprocess is told the check is done (env var).
                        self.gl7_cooldown_result = Some(Err(reason));
                    } else {
                        let exe = std::env::current_exe()
                            .unwrap_or_else(|_| std::path::PathBuf::from("frost"));
                        match std::process::Command::new(&exe)
                            .args(["gl7", "cooldown", "--csv", &path])
                            .env(crate::safety::GUI_CHECKED_ENV, "1")
                            .spawn()
                        {
                            Ok(child) => {
                                let pid = child.id();
                                self.gl7_cooldown_pid   = Some(pid);
                                self.gl7_cooldown_child = Some(child);
                                crate::worker::set_gl7_cooldown_persisted(pid);
                                self.worker.send(GuiCommand::Gl7CooldownActive(true));
                                self.gl7_cooldown_result =
                                    Some(Ok(format!("GL7 cooldown started  (CSV: {path})")));
                            }
                            Err(e) => {
                                self.gl7_cooldown_result =
                                    Some(Err(format!("Failed to start cooldown: {e}")));
                            }
                        }
                    }
                }
            }
            ui.add_space(8.0);
            ui.add(
                egui::TextEdit::singleline(&mut self.gl7_cooldown_csv_path)
                    .desired_width(340.0)
                    .hint_text("path to temperature CSV…"),
            );
            if ui.button("Current Temperature Recording").clicked() {
                if let Some(ref p) = snap.recording_csv_path {
                    self.gl7_cooldown_csv_path = p.clone();
                }
            }
        });
        if gl7_running && self.gl7_cooldown_child.is_none() {
            // Running externally (CLI or reloaded while cooldown was in progress).
            ui.colored_label(
                egui::Color32::DARK_GREEN,
                "GL7 cooldown active (started from CLI or previous session).",
            );
        } else if let Some(ref res) = self.gl7_cooldown_result {
            match res {
                Ok(msg) => { ui.colored_label(egui::Color32::DARK_GREEN, msg); }
                Err(e)  => { ui.colored_label(egui::Color32::RED, e.as_str()); }
            }
        }
        ui.add_space(6.0);

        let output_names = ["4-Pump Heater", "3-Pump Heater", "4-Switch Heater", "3-Switch Heater"];

        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);
            for i in 0..4 {
                let output_num = i + 1;
                let label = output_names[i];
                egui::Frame::none()
                    .fill(egui::Color32::from_rgb(255, 230, 248))
                    .stroke(egui::Stroke::new(1.5, egui::Color32::from_rgb(195, 100, 165)))
                    .rounding(egui::Rounding::same(8.0))
                    .inner_margin(egui::Margin::same(10.0))
                    .show(ui, |ui| {
                        ui.set_min_width(180.0);
                        ui.vertical(|ui| {
                            ui.add(egui::Label::new(
                                egui::RichText::new(label).strong().size(14.0),
                            ).selectable(false));
                            ui.add_space(4.0);
                            let pct_str = match snap.gl7_polled_pct.get(i).and_then(|v| *v) {
                                Some(v) => format!("{v:.1} %"),
                                None    => "(pending…)".to_string(),
                            };
                            ui.add(egui::Label::new(
                                egui::RichText::new(&pct_str).size(13.0).monospace(),
                            ).selectable(false));
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
                "Last updated: {:.1}s ago  (refreshes every {} s)",
                t.elapsed().as_secs_f32(),
                POLL_INTERVAL.as_secs()
            ));
        } else {
            ui.label("(pending first poll…)");
        }

    }

    fn show_temperature_display(&mut self, ui: &mut egui::Ui, snap: &DeviceSnapshot) {
        let is_recording = snap.recording_active;

        ui.horizontal(|ui| {
            if is_recording {
                let btn = egui::Button::new(
                    egui::RichText::new("⏹  Stop Recording Temperatures")
                        .strong()
                        .size(18.0)
                        .color(egui::Color32::WHITE),
                )
                .fill(egui::Color32::from_rgb(185, 30, 30));
                if ui.add(btn).clicked() {
                    let path = snap.recording_csv_path.as_deref().unwrap_or("unknown").to_string();
                    self.worker.send(GuiCommand::StopRecording);
                    self.record_result = Some(Ok(format!("Recording stopped. File: {path}")));
                }
            } else {
                let btn = egui::Button::new(
                    egui::RichText::new("⏺  Record Temperatures")
                        .strong()
                        .size(18.0)
                        .color(egui::Color32::from_rgb(15, 30, 80)),
                )
                .fill(egui::Color32::from_rgb(140, 185, 255));
                if ui.add(btn).clicked() {
                    self.worker.send(GuiCommand::StartRecording {
                        interval_secs: POLL_INTERVAL.as_secs(),
                        output_dir: "temps".to_string(),
                        resume_path: None,
                    });
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
        let elapsed = snap.last_temp_update.map(|t| t.elapsed().as_secs_f32());

        // Input B (ADR) is intentionally not displayed — that port is currently
        // empty. It is still polled/logged everywhere else (worker, record-temps).
        let sensors: &[(&str, &str)] = &[
            ("4K Stage",     &t.ls350_d3),
            ("4-Switch",     &t.ls350_d2),
            ("3-Head",       &t.ls350_a),
            ("4-Head",       &t.ls350_c),
            ("3-Pump",       &t.ls350_d4),
            ("4-Pump",       &t.ls350_d5),
            ("Device Stage", &t.ls370_1),
        ];

        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);
            for &(name, val) in sensors {
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
        ui.label(format!("Updates every {} seconds", POLL_INTERVAL.as_secs()));
    }
}
