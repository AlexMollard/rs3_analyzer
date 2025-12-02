mod loader;
mod stats;
mod flips;
mod model;
mod ui;

use eframe::egui;
use ui::RS3App;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1600.0, 1000.0])
            .with_min_inner_size([1200.0, 700.0]),
        ..Default::default()
    };

    eframe::run_native(
        "RS3 Market Analyzer",
        options,
        Box::new(|cc| {
            let mut fonts = egui::FontDefinitions::default();
            
            if let Ok(segoe_data) = std::fs::read("C:\\Windows\\Fonts\\segoeui.ttf") {
                fonts.font_data.insert(
                    "SegoeUI".to_owned(),
                    egui::FontData::from_owned(segoe_data).into(),
                );
                fonts.families.get_mut(&egui::FontFamily::Proportional)
                    .unwrap()
                    .insert(0, "SegoeUI".to_owned());
            }
            
            cc.egui_ctx.set_fonts(fonts);
            ui::set_custom_style(&cc.egui_ctx);
            Ok(Box::new(RS3App::new()))
        }),
    )
}
