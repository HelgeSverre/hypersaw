#![allow(unused_variables)]
#![allow(unused_imports)]

mod core;
mod ui;

use eframe::NativeOptions;

fn main() -> eframe::Result<()> {
    let options = NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_decorations(true)
            .with_inner_size([1280.0, 720.0])
            .with_min_inner_size([800.0, 600.0])
            .with_transparent(true)
            .with_title("HyperSaw"),
        ..Default::default()
    };

    // Run the app
    eframe::run_native(
        "HyperSaw",
        options,
        Box::new(|cc| Ok(Box::new(ui::SupersawApp::new(cc)))),
    )
}
