// ============================================================
// gui.rs — All egui/eframe handling for FROST
//
// This is the place to customize:
//   - Fonts (see apply_fonts)
//   - Colors and themes (see apply_theme, Theme enum)
//   - Window layout and tab structure
//   - Widget styles, rounding, spacing, etc.
// ============================================================

use eframe::egui;

use crate::compressor::CryomechController;
use crate::heatswitch::HeatswitchController;

// ── Entry point called from main.rs ─────────────────────────
pub fn run() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([900.0, 700.0])
            .with_title("FROST - Fridge Remote Operations, Software, and Thermometry"),
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

// ── Font customization ───────────────────────────────────────
// Add custom fonts or resize the built-in ones here.
fn apply_fonts(ctx: &egui::Context) {
    let fonts = egui::FontDefinitions::default();

    // Example: increase the default proportional font size
    // fonts.font_data.insert(
    //     "my_font".to_owned(),
    //     egui::FontData::from_static(include_bytes!("../../assets/MyFont.ttf")),
    // );
    // fonts.families.entry(egui::FontFamily::Proportional)
    //     .or_default()
    //     .insert(0, "my_font".to_owned());

    ctx.set_fonts(fonts);
}

// ── Tab enum ────────────────────────────────────────────────
#[derive(Debug, PartialEq)]
enum Tab {
    Status,
    Thermometry,
    ADR,
    Compressor,
    HeatSwitch,
}

// ── Theme enum ───────────────────────────────────────────────
// Add new variants here to add more themes; wire them up in
// apply_theme() and the selectors in update().
#[derive(Debug, PartialEq)]
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

// ── App state ────────────────────────────────────────────────
struct FrostApp {
    // TODO: replace remaining placeholders with real controller types as modules are added
    _lakeshore350_output: String,
    _lakeshore350_error: Option<String>,
    _lakeshore370_output: String,
    _lakeshore370_error: Option<String>,
    cryomech: CryomechController,
    heatswitch: HeatswitchController,
    auto_refresh: bool,
    last_refresh: std::time::Instant,
    selected_tab: Tab,
    selected_theme: Theme,
}

impl Default for FrostApp {
    fn default() -> Self {
        Self {
            _lakeshore350_output: String::new(),
            _lakeshore350_error: None,
            _lakeshore370_output: String::new(),
            _lakeshore370_error: None,
            cryomech: CryomechController::default(),
            heatswitch: HeatswitchController::default(),
            auto_refresh: false,
            last_refresh: std::time::Instant::now(),
            selected_tab: Tab::Status,
            selected_theme: Theme::EguiLightBlue,
        }
    }
}

// ── Main update loop ─────────────────────────────────────────
impl eframe::App for FrostApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.apply_theme(ctx);

        // Auto-refresh every 5 s on the Thermometry tab
        if self.selected_tab == Tab::Thermometry
            && self.auto_refresh
            && self.last_refresh.elapsed().as_secs() >= 5
        {
            // TODO: call controller read methods here
            self.last_refresh = std::time::Instant::now();
            ctx.request_repaint();
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            // ── Header ──
            ui.add(egui::Label::new(
                egui::RichText::new("FROST")
                    .size(48.0)
                    .strong()
                    .color(egui::Color32::from_rgb(30, 30, 120)),
            ));
            ui.label("Fridge Remote Operations, Software, and Thermometry");

            // ── Theme selectors ──
            ui.horizontal(|ui| {
                ui.label("Theme:");
                ui.selectable_value(&mut self.selected_theme, Theme::Default, "Default");
                ui.selectable_value(&mut self.selected_theme, Theme::LightBlue, "Light Blue");
                ui.selectable_value(&mut self.selected_theme, Theme::Purple, "Purple");
                ui.selectable_value(&mut self.selected_theme, Theme::Dark, "Dark");
            });
            ui.horizontal(|ui| {
                ui.label("Colors:");
                ui.selectable_value(&mut self.selected_theme, Theme::White, "White");
                ui.selectable_value(&mut self.selected_theme, Theme::Black, "Black");
                ui.selectable_value(&mut self.selected_theme, Theme::Red, "Red");
                ui.selectable_value(&mut self.selected_theme, Theme::Green, "Green");
                ui.selectable_value(&mut self.selected_theme, Theme::Blue, "Blue");
                ui.selectable_value(&mut self.selected_theme, Theme::Yellow, "Yellow");
            });
            ui.horizontal(|ui| {
                ui.label("More:");
                ui.selectable_value(&mut self.selected_theme, Theme::Cyan, "Cyan");
                ui.selectable_value(&mut self.selected_theme, Theme::Magenta, "Magenta");
                ui.selectable_value(&mut self.selected_theme, Theme::Gray, "Gray");
                ui.selectable_value(&mut self.selected_theme, Theme::LightGray, "Light Gray");
                ui.selectable_value(&mut self.selected_theme, Theme::DarkGray, "Dark Gray");
            });
            ui.horizontal(|ui| {
                ui.label("Egui:");
                ui.selectable_value(&mut self.selected_theme, Theme::EguiLightBlue, "Egui Light Blue");
                ui.selectable_value(&mut self.selected_theme, Theme::EguiLightGreen, "Egui Light Green");
                ui.selectable_value(&mut self.selected_theme, Theme::EguiLightRed, "Egui Light Red");
            });

            ui.separator();

            // ── Tab bar ──
            ui.horizontal(|ui| {
                self.tab_button(ui, Tab::Status, "Status", 100.0);
                self.tab_button(ui, Tab::Thermometry, "Thermometry", 120.0);
                self.tab_button(ui, Tab::ADR, "ADR", 80.0);
                self.tab_button(ui, Tab::Compressor, "Compressor", 120.0);
                self.tab_button(ui, Tab::HeatSwitch, "Heat Switch", 120.0);
            });

            ui.separator();

            // ── Tab content ──
            match self.selected_tab {
                Tab::Status => self.show_status_tab(ui),
                Tab::Thermometry => self.show_thermometry_tab(ui),
                Tab::ADR => self.show_adr_tab(ui),
                Tab::Compressor => self.show_compressor_tab(ui),
                Tab::HeatSwitch => self.show_heatswitch_tab(ui),
            }
        });
    }
}

// ── Helper: render a tab button ──────────────────────────────
impl FrostApp {
    fn tab_button(&mut self, ui: &mut egui::Ui, tab: Tab, label: &str, width: f32) {
        let active = self.selected_tab == tab;
        let text = egui::RichText::new(label)
            .size(16.0)
            .strong()
            .color(egui::Color32::from_rgb(30, 30, 120));
        let mut btn = egui::Button::new(text).min_size(egui::Vec2::new(width, 40.0));
        if active {
            btn = btn.fill(egui::Color32::from_rgb(120, 180, 255));
        }
        if ui.add(btn).clicked() {
            self.selected_tab = tab;
        }
    }
}

// ── Theme application ────────────────────────────────────────
// To customise colors: edit the match arms below, or add new
// Theme variants above and a corresponding arm here.
impl FrostApp {
    fn apply_theme(&self, ctx: &egui::Context) {
        let mut style = (*ctx.style()).clone();

        match self.selected_theme {
            Theme::Default => {
                style = egui::Style::default();
            }
            Theme::LightBlue => {
                style.visuals.window_fill = egui::Color32::from_rgb(230, 240, 255);
                style.visuals.panel_fill = egui::Color32::from_rgb(240, 245, 255);
                style.visuals.widgets.noninteractive.bg_fill = egui::Color32::from_rgb(220, 235, 255);
                style.visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(200, 225, 255);
                style.visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(180, 215, 255);
                style.visuals.widgets.active.bg_fill = egui::Color32::from_rgb(160, 205, 255);
            }
            Theme::Purple => {
                style.visuals.window_fill = egui::Color32::from_rgb(240, 230, 255);
                style.visuals.panel_fill = egui::Color32::from_rgb(245, 240, 255);
                style.visuals.widgets.noninteractive.bg_fill = egui::Color32::from_rgb(235, 220, 255);
                style.visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(225, 200, 255);
                style.visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(215, 180, 255);
                style.visuals.widgets.active.bg_fill = egui::Color32::from_rgb(205, 160, 255);
            }
            Theme::Dark => {
                style.visuals.dark_mode = true;
                style.visuals.window_fill = egui::Color32::from_rgb(30, 30, 40);
                style.visuals.panel_fill = egui::Color32::from_rgb(25, 25, 35);
                style.visuals.widgets.noninteractive.bg_fill = egui::Color32::from_rgb(40, 40, 50);
                style.visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(50, 50, 60);
                style.visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(60, 60, 70);
                style.visuals.widgets.active.bg_fill = egui::Color32::from_rgb(70, 70, 80);
            }
            Theme::White => {
                style.visuals.window_fill = egui::Color32::WHITE;
                style.visuals.panel_fill = egui::Color32::from_rgb(250, 250, 250);
                style.visuals.widgets.noninteractive.bg_fill = egui::Color32::from_rgb(245, 245, 245);
                style.visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(235, 235, 235);
                style.visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(225, 225, 225);
                style.visuals.widgets.active.bg_fill = egui::Color32::from_rgb(215, 215, 215);
            }
            Theme::Black => {
                style.visuals.dark_mode = true;
                style.visuals.window_fill = egui::Color32::BLACK;
                style.visuals.panel_fill = egui::Color32::from_rgb(20, 20, 20);
                style.visuals.widgets.noninteractive.bg_fill = egui::Color32::from_rgb(30, 30, 30);
                style.visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(40, 40, 40);
                style.visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(50, 50, 50);
                style.visuals.widgets.active.bg_fill = egui::Color32::from_rgb(60, 60, 60);
            }
            Theme::Red => {
                style.visuals.window_fill = egui::Color32::from_rgb(255, 230, 230);
                style.visuals.panel_fill = egui::Color32::from_rgb(255, 240, 240);
                style.visuals.widgets.noninteractive.bg_fill = egui::Color32::from_rgb(255, 220, 220);
                style.visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(255, 200, 200);
                style.visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(255, 180, 180);
                style.visuals.widgets.active.bg_fill = egui::Color32::RED;
            }
            Theme::Green => {
                style.visuals.window_fill = egui::Color32::from_rgb(230, 255, 230);
                style.visuals.panel_fill = egui::Color32::from_rgb(240, 255, 240);
                style.visuals.widgets.noninteractive.bg_fill = egui::Color32::from_rgb(220, 255, 220);
                style.visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(200, 255, 200);
                style.visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(180, 255, 180);
                style.visuals.widgets.active.bg_fill = egui::Color32::GREEN;
            }
            Theme::Blue => {
                style.visuals.window_fill = egui::Color32::from_rgb(230, 230, 255);
                style.visuals.panel_fill = egui::Color32::from_rgb(240, 240, 255);
                style.visuals.widgets.noninteractive.bg_fill = egui::Color32::from_rgb(220, 220, 255);
                style.visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(200, 200, 255);
                style.visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(180, 180, 255);
                style.visuals.widgets.active.bg_fill = egui::Color32::BLUE;
            }
            Theme::Yellow => {
                style.visuals.window_fill = egui::Color32::from_rgb(255, 255, 230);
                style.visuals.panel_fill = egui::Color32::from_rgb(255, 255, 240);
                style.visuals.widgets.noninteractive.bg_fill = egui::Color32::from_rgb(255, 255, 220);
                style.visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(255, 255, 200);
                style.visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(255, 255, 180);
                style.visuals.widgets.active.bg_fill = egui::Color32::YELLOW;
            }
            Theme::Cyan => {
                style.visuals.window_fill = egui::Color32::from_rgb(230, 255, 255);
                style.visuals.panel_fill = egui::Color32::from_rgb(240, 255, 255);
                style.visuals.widgets.noninteractive.bg_fill = egui::Color32::from_rgb(220, 255, 255);
                style.visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(200, 255, 255);
                style.visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(180, 255, 255);
                style.visuals.widgets.active.bg_fill = egui::Color32::from_rgb(0, 255, 255);
            }
            Theme::Magenta => {
                style.visuals.window_fill = egui::Color32::from_rgb(255, 230, 255);
                style.visuals.panel_fill = egui::Color32::from_rgb(255, 240, 255);
                style.visuals.widgets.noninteractive.bg_fill = egui::Color32::from_rgb(255, 220, 255);
                style.visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(255, 200, 255);
                style.visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(255, 180, 255);
                style.visuals.widgets.active.bg_fill = egui::Color32::from_rgb(255, 0, 255);
            }
            Theme::Gray => {
                style.visuals.window_fill = egui::Color32::GRAY;
                style.visuals.panel_fill = egui::Color32::from_rgb(140, 140, 140);
                style.visuals.widgets.noninteractive.bg_fill = egui::Color32::from_rgb(160, 160, 160);
                style.visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(180, 180, 180);
                style.visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(200, 200, 200);
                style.visuals.widgets.active.bg_fill = egui::Color32::from_rgb(220, 220, 220);
            }
            Theme::LightGray => {
                style.visuals.window_fill = egui::Color32::LIGHT_GRAY;
                style.visuals.panel_fill = egui::Color32::from_rgb(210, 210, 210);
                style.visuals.widgets.noninteractive.bg_fill = egui::Color32::from_rgb(200, 200, 200);
                style.visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(190, 190, 190);
                style.visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(180, 180, 180);
                style.visuals.widgets.active.bg_fill = egui::Color32::from_rgb(170, 170, 170);
            }
            Theme::DarkGray => {
                style.visuals.dark_mode = true;
                style.visuals.window_fill = egui::Color32::DARK_GRAY;
                style.visuals.panel_fill = egui::Color32::from_rgb(50, 50, 50);
                style.visuals.widgets.noninteractive.bg_fill = egui::Color32::from_rgb(70, 70, 70);
                style.visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(90, 90, 90);
                style.visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(110, 110, 110);
                style.visuals.widgets.active.bg_fill = egui::Color32::from_rgb(130, 130, 130);
            }
            Theme::EguiLightBlue => {
                let dark_blue = egui::Color32::from_rgb(30, 30, 120);
                style.visuals.window_fill = egui::Color32::LIGHT_BLUE;
                style.visuals.panel_fill = egui::Color32::from_rgb(200, 220, 255);
                style.visuals.widgets.noninteractive.bg_fill = egui::Color32::LIGHT_BLUE;
                style.visuals.widgets.inactive.bg_fill = egui::Color32::LIGHT_BLUE;
                style.visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(140, 190, 255);
                style.visuals.widgets.active.bg_fill = egui::Color32::from_rgb(120, 180, 255);
                style.visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1.0, dark_blue);
                style.visuals.widgets.noninteractive.fg_stroke.color = dark_blue;
                style.visuals.widgets.inactive.fg_stroke.color = dark_blue;
                style.visuals.widgets.hovered.fg_stroke.color = dark_blue;
                style.visuals.widgets.active.fg_stroke.color = dark_blue;
                style.visuals.override_text_color = Some(dark_blue);
            }
            Theme::EguiLightGreen => {
                style.visuals.window_fill = egui::Color32::LIGHT_GREEN;
                style.visuals.panel_fill = egui::Color32::from_rgb(220, 255, 200);
                style.visuals.widgets.noninteractive.bg_fill = egui::Color32::from_rgb(210, 255, 180);
                style.visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(200, 255, 160);
                style.visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(190, 255, 140);
                style.visuals.widgets.active.bg_fill = egui::Color32::from_rgb(180, 255, 120);
            }
            Theme::EguiLightRed => {
                style.visuals.window_fill = egui::Color32::LIGHT_RED;
                style.visuals.panel_fill = egui::Color32::from_rgb(255, 200, 200);
                style.visuals.widgets.noninteractive.bg_fill = egui::Color32::from_rgb(255, 180, 180);
                style.visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(255, 160, 160);
                style.visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(255, 140, 140);
                style.visuals.widgets.active.bg_fill = egui::Color32::from_rgb(255, 120, 120);
            }
        }

        // Common rounding + text color applied to every non-Default theme
        if self.selected_theme != Theme::Default {
            let r = egui::Rounding::from(6.0);
            style.visuals.widgets.noninteractive.rounding = r;
            style.visuals.widgets.inactive.rounding = r;
            style.visuals.widgets.hovered.rounding = r;
            style.visuals.widgets.active.rounding = r;

            if style.visuals.override_text_color.is_none() {
                let text_color = egui::Color32::from_rgb(30, 30, 120);
                style.visuals.widgets.noninteractive.fg_stroke.color = text_color;
                style.visuals.widgets.inactive.fg_stroke.color = text_color;
                style.visuals.widgets.hovered.fg_stroke.color = text_color;
                style.visuals.widgets.active.fg_stroke.color = text_color;
            }
        }

        // ── Universal button style (all themes) ──────────────
        // Buttons are white at rest, light blue on hover, solid blue when
        // pressed or selected (tab headers use an explicit .fill() override
        // for the active state; special action buttons like Open/Close/Stop
        // use their own explicit .fill() and are unaffected by this).
        let btn_blue       = egui::Color32::from_rgb(120, 180, 255);
        let btn_hover_blue = egui::Color32::from_rgb(200, 225, 255);
        let btn_border     = egui::Color32::from_rgb(170, 185, 215);
        let dark_blue      = egui::Color32::from_rgb(30, 30, 120);

        style.visuals.widgets.inactive.bg_fill       = egui::Color32::WHITE;
        style.visuals.widgets.inactive.weak_bg_fill  = egui::Color32::WHITE;
        style.visuals.widgets.inactive.bg_stroke     = egui::Stroke::new(1.0, btn_border);
        style.visuals.widgets.inactive.fg_stroke     = egui::Stroke::new(1.5, dark_blue);

        style.visuals.widgets.hovered.bg_fill        = btn_hover_blue;
        style.visuals.widgets.hovered.weak_bg_fill   = btn_hover_blue;
        style.visuals.widgets.hovered.bg_stroke      = egui::Stroke::new(1.0, btn_blue);
        style.visuals.widgets.hovered.fg_stroke      = egui::Stroke::new(1.5, dark_blue);

        style.visuals.widgets.active.bg_fill         = btn_blue;
        style.visuals.widgets.active.weak_bg_fill    = btn_blue;
        style.visuals.widgets.active.bg_stroke       = egui::Stroke::new(1.5, dark_blue);
        style.visuals.widgets.active.fg_stroke       = egui::Stroke::new(1.5, dark_blue);

        ctx.set_style(style);
    }
}

// ── Tab content ───────────────────────────────────────────────
impl FrostApp {
    fn tab_heading(ui: &mut egui::Ui, text: &str) {
        ui.add(egui::Label::new(
            egui::RichText::new(text)
                .size(24.0)
                .strong()
                .color(egui::Color32::from_rgb(30, 30, 120)),
        ));
    }

    fn show_status_tab(&mut self, ui: &mut egui::Ui) {
        Self::tab_heading(ui, "Status");
        ui.label("System status information will be displayed here.");
        ui.separator();
        ui.label("Coming soon…");
    }

    fn show_thermometry_tab(&mut self, ui: &mut egui::Ui) {
        Self::tab_heading(ui, "Thermometry");

        ui.horizontal(|ui| {
            if ui.button("Refresh Temperatures").clicked() {
                // TODO: call controller read methods here
                self.last_refresh = std::time::Instant::now();
            }
            ui.checkbox(&mut self.auto_refresh, "Auto-refresh (5 s)");
        });

        ui.separator();

        if let Some(err) = &self._lakeshore350_error.clone() {
            ui.colored_label(egui::Color32::RED, format!("LakeShore 350 Error: {err}"));
        }
        if let Some(err) = &self._lakeshore370_error.clone() {
            ui.colored_label(egui::Color32::RED, format!("LakeShore 370 Error: {err}"));
        }

        ui.add_space(10.0);
        ui.strong("LakeShore 350 Temperature Controller");
        egui::ScrollArea::vertical()
            .max_height(300.0)
            .show(ui, |ui| {
                ui.add(
                    egui::TextEdit::multiline(&mut self._lakeshore350_output.as_str())
                        .font(egui::TextStyle::Monospace)
                        .desired_width(f32::INFINITY)
                        .desired_rows(15),
                );
            });

        ui.separator();
        ui.add_space(10.0);
        ui.strong("LakeShore 370 AC Resistance Bridge");
        if !self._lakeshore370_output.is_empty() {
            ui.label(self._lakeshore370_output.as_str());
        } else {
            ui.colored_label(egui::Color32::GRAY, "No data");
        }
    }

    fn show_adr_tab(&mut self, ui: &mut egui::Ui) {
        Self::tab_heading(ui, "ADR Control");
        ui.label("Adiabatic Demagnetization Refrigerator controls will be implemented here.");
        ui.separator();
        ui.label("Coming soon…");
    }

    fn show_heatswitch_tab(&mut self, ui: &mut egui::Ui) {
        Self::tab_heading(ui, "Heat Switch");
        ui.label("Zaber T-NM17A04 stepper motor control");
        ui.separator();

        // ── Primary heat-switch operations ──
        ui.horizontal(|ui| {
            let open_btn = egui::Button::new(
                egui::RichText::new("⬆  OPEN").size(18.0).strong()
            ).min_size(egui::Vec2::new(140.0, 50.0))
             .fill(egui::Color32::from_rgb(100, 200, 120));
            if ui.add(open_btn).clicked() {
                if let Err(e) = self.heatswitch.open() {
                    self.heatswitch.error_message = Some(e);
                } else {
                    self.heatswitch.error_message = None;
                    self.heatswitch.status_output = "Open command sent (CW 115200 steps)".to_string();
                }
            }

            let close_btn = egui::Button::new(
                egui::RichText::new("⬇  CLOSE").size(18.0).strong()
            ).min_size(egui::Vec2::new(140.0, 50.0))
             .fill(egui::Color32::from_rgb(200, 120, 100));
            if ui.add(close_btn).clicked() {
                if let Err(e) = self.heatswitch.close() {
                    self.heatswitch.error_message = Some(e);
                } else {
                    self.heatswitch.error_message = None;
                    self.heatswitch.status_output = "Close command sent (CCW 115200 steps)".to_string();
                }
            }

            ui.add_space(20.0);

            if ui.button("Home").clicked() {
                if let Err(e) = self.heatswitch.home() {
                    self.heatswitch.error_message = Some(e);
                } else {
                    self.heatswitch.error_message = None;
                    self.heatswitch.status_output = "Home command sent".to_string();
                }
            }
            if ui.button("Reset").clicked() {
                if let Err(e) = self.heatswitch.reset() {
                    self.heatswitch.error_message = Some(e);
                } else {
                    self.heatswitch.error_message = None;
                    self.heatswitch.status_output = "Reset (home) command sent".to_string();
                }
            }
        });

        ui.add_space(6.0);

        // ── Stop ──
        ui.horizontal(|ui| {
            let stop_btn = egui::Button::new(
                egui::RichText::new("STOP").size(16.0).strong()
            ).min_size(egui::Vec2::new(100.0, 36.0))
             .fill(egui::Color32::from_rgb(220, 180, 60));
            if ui.add(stop_btn).clicked() {
                if let Err(e) = self.heatswitch.stop() {
                    self.heatswitch.error_message = Some(e);
                } else {
                    self.heatswitch.error_message = None;
                    self.heatswitch.status_output = "Stop command sent".to_string();
                }
            }

            let estop_btn = egui::Button::new(
                egui::RichText::new("⚠ E-STOP").size(16.0).strong()
            ).min_size(egui::Vec2::new(120.0, 36.0))
             .fill(egui::Color32::from_rgb(220, 60, 60));
            if ui.add(estop_btn).clicked() {
                if let Err(e) = self.heatswitch.emergency_stop() {
                    self.heatswitch.error_message = Some(e);
                } else {
                    self.heatswitch.error_message = None;
                    self.heatswitch.status_output = "Emergency stop sent (3x)".to_string();
                }
            }

            ui.add_space(20.0);
            if ui.button("Refresh Status").clicked() {
                self.heatswitch.get_status();
            }
            if ui.button("Get Position").clicked() {
                self.heatswitch.get_position();
            }
        });

        ui.separator();

        // ── Manual move controls ──
        ui.strong("Manual Move");
        ui.add_space(4.0);

        // Move relative / CW / CCW
        ui.horizontal(|ui| {
            ui.label("Steps:");
            ui.add(egui::DragValue::new(&mut self.heatswitch.step_input).speed(100).clamp_range(1..=1_000_000));
            if ui.button("Move Rel +").clicked() {
                let s = self.heatswitch.step_input;
                if let Err(e) = self.heatswitch.move_relative(s) {
                    self.heatswitch.error_message = Some(e);
                } else {
                    self.heatswitch.error_message = None;
                    self.heatswitch.status_output = format!("Move rel +{} sent", s);
                }
            }
            if ui.button("Move Rel −").clicked() {
                let s = self.heatswitch.step_input;
                if let Err(e) = self.heatswitch.move_relative(-s) {
                    self.heatswitch.error_message = Some(e);
                } else {
                    self.heatswitch.error_message = None;
                    self.heatswitch.status_output = format!("Move rel -{} sent", s);
                }
            }
            if ui.button("CW").clicked() {
                let s = self.heatswitch.step_input;
                if let Err(e) = self.heatswitch.rotate_cw(s) {
                    self.heatswitch.error_message = Some(e);
                } else {
                    self.heatswitch.error_message = None;
                    self.heatswitch.status_output = format!("CW {} steps sent", s);
                }
            }
            if ui.button("CCW").clicked() {
                let s = self.heatswitch.step_input;
                if let Err(e) = self.heatswitch.rotate_ccw(s) {
                    self.heatswitch.error_message = Some(e);
                } else {
                    self.heatswitch.error_message = None;
                    self.heatswitch.status_output = format!("CCW {} steps sent", s);
                }
            }
        });

        // Safe moves (clamped to 1–1000 steps)
        ui.horizontal(|ui| {
            ui.label("Safe (≤1000):");
            if ui.button("Safe CW").clicked() {
                let s = self.heatswitch.step_input;
                if let Err(e) = self.heatswitch.safe_cw(s) {
                    self.heatswitch.error_message = Some(e);
                } else {
                    self.heatswitch.error_message = None;
                    self.heatswitch.status_output = format!("Safe CW {} steps sent", s.clamp(1, 1000));
                }
            }
            if ui.button("Safe CCW").clicked() {
                let s = self.heatswitch.step_input;
                if let Err(e) = self.heatswitch.safe_ccw(s) {
                    self.heatswitch.error_message = Some(e);
                } else {
                    self.heatswitch.error_message = None;
                    self.heatswitch.status_output = format!("Safe CCW {} steps sent", s.clamp(1, 1000));
                }
            }
        });

        // Move absolute
        ui.horizontal(|ui| {
            ui.label("Abs position:");
            ui.add(egui::DragValue::new(&mut self.heatswitch.abs_pos_input).speed(100));
            if ui.button("Move Absolute").clicked() {
                let p = self.heatswitch.abs_pos_input;
                if let Err(e) = self.heatswitch.move_absolute(p) {
                    self.heatswitch.error_message = Some(e);
                } else {
                    self.heatswitch.error_message = None;
                    self.heatswitch.status_output = format!("Move abs {} sent", p);
                }
            }
        });

        // Move velocity
        ui.horizontal(|ui| {
            ui.label("Velocity:");
            ui.add(egui::DragValue::new(&mut self.heatswitch.velocity_input).speed(10));
            if ui.button("Move Velocity").clicked() {
                let v = self.heatswitch.velocity_input;
                if let Err(e) = self.heatswitch.move_velocity(v) {
                    self.heatswitch.error_message = Some(e);
                } else {
                    self.heatswitch.error_message = None;
                    self.heatswitch.status_output = format!("Move vel {} sent", v);
                }
            }
        });

        ui.separator();

        // ── Error / status output ──
        if let Some(err) = self.heatswitch.error_message.clone() {
            ui.colored_label(egui::Color32::RED, format!("Error: {err}"));
            ui.separator();
        }

        ui.add_space(6.0);
        ui.strong("Status");
        if !self.heatswitch.status_output.is_empty() {
            egui::ScrollArea::vertical()
                .max_height(200.0)
                .show(ui, |ui| {
                    ui.add(
                        egui::TextEdit::multiline(&mut self.heatswitch.status_output.as_str())
                            .font(egui::TextStyle::Monospace)
                            .desired_width(f32::INFINITY)
                            .desired_rows(8),
                    );
                });
        } else {
            ui.colored_label(egui::Color32::GRAY, "Press \"Refresh Status\" to read motor state");
        }
    }

    fn show_compressor_tab(&mut self, ui: &mut egui::Ui) {
        Self::tab_heading(ui, "Compressor Control");

        ui.horizontal(|ui| {
            if ui.button("Refresh Status").clicked() {
                self.cryomech.get_status();
            }
            if ui.button("Start Compressor").clicked() {
                if let Err(e) = self.cryomech.start_compressor() {
                    self.cryomech.error_message = Some(e);
                } else {
                    self.cryomech.get_status();
                }
            }
            if ui.button("Stop Compressor").clicked() {
                if let Err(e) = self.cryomech.stop_compressor() {
                    self.cryomech.error_message = Some(e);
                } else {
                    self.cryomech.get_status();
                }
            }
        });

        ui.separator();

        ui.horizontal(|ui| {
            if ui.button("Get Temperature").clicked() {
                if let Err(e) = self.cryomech.get_temperature() {
                    self.cryomech.error_message = Some(e);
                }
            }
            if ui.button("Get Pressure").clicked() {
                if let Err(e) = self.cryomech.get_pressure() {
                    self.cryomech.error_message = Some(e);
                }
            }
            if ui.button("Get System Info").clicked() {
                if let Err(e) = self.cryomech.get_system_info() {
                    self.cryomech.error_message = Some(e);
                }
            }
        });

        ui.horizontal(|ui| {
            if ui.button("Get All Readings").clicked() {
                self.cryomech.get_all_readings();
            }
            if ui.button("Clear Min/Max").clicked() {
                if let Err(e) = self.cryomech.clear_min_max() {
                    self.cryomech.error_message = Some(e);
                }
            }
        });

        ui.separator();

        if let Some(err) = self.cryomech.error_message.clone() {
            ui.colored_label(egui::Color32::RED, format!("Error: {err}"));
            ui.separator();
        }

        ui.add_space(10.0);
        ui.strong("Current Status");
        if !self.cryomech.status_output.is_empty() {
            egui::ScrollArea::vertical()
                .max_height(200.0)
                .show(ui, |ui| {
                    ui.add(
                        egui::TextEdit::multiline(&mut self.cryomech.status_output.as_str())
                            .font(egui::TextStyle::Monospace)
                            .desired_width(f32::INFINITY)
                            .desired_rows(10),
                    );
                });
        } else {
            ui.colored_label(egui::Color32::GRAY, "No status data");
        }

        if !self.cryomech.all_output.is_empty() {
            ui.separator();
            ui.add_space(10.0);
            ui.strong("All Readings");
            egui::ScrollArea::vertical()
                .max_height(200.0)
                .show(ui, |ui| {
                    ui.add(
                        egui::TextEdit::multiline(&mut self.cryomech.all_output.as_str())
                            .font(egui::TextStyle::Monospace)
                            .desired_width(f32::INFINITY)
                            .desired_rows(10),
                    );
                });
        }
    }
}
