use eframe::egui;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([800.0, 600.0])
            .with_title("FROST - Fridge Remote Operations, Software, and Thermometry"),
        ..Default::default()
    };

    eframe::run_native(
        "FROST",
        options,
        Box::new(|_cc| Box::new(FrostApp::default())),
    )
}

#[derive(Default)]
struct FrostApp {
    name: String,
}

impl eframe::App for FrostApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("🧊 FROST");
            ui.label("Fridge Remote Operations, Software, and Thermometry");
            ui.separator();
            
            ui.horizontal(|ui| {
                ui.label("Your name: ");
                ui.text_edit_singleline(&mut self.name);
            });
            
            ui.horizontal(|ui| {
                if ui.button("Say hello!").clicked() {
                    println!("Hello {}!", self.name);
                }
            });
            
            ui.separator();
            ui.label("🔧 Future integrations:");
            ui.label("• Lakeshore 625 - Superconducting Magnet Power Supply");
            ui.label("• Lakeshore 370 - AC Resistance Bridge");  
            ui.label("• Lakeshore 350 - Temperature Controller");
            ui.label("• Heatswitch Driver - Zaber Stepper Motor");
            ui.label("• Cryomech Driver - Pulse Tube Compressor");
        });
    }
}