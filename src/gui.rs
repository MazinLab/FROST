// gui.rs — Minimal GUI shell for FROST (header + theme options only)

use eframe::egui;

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
}

impl Default for FrostApp {
    fn default() -> Self {
        Self {
            selected_theme: Theme::EguiLightBlue,
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
}
