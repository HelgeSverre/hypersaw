#![allow(unused_variables)]
#![allow(unused_imports)]

mod core;
mod ui;

use eframe::NativeOptions;

fn main() -> eframe::Result<()> {
    let options = NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_decorations(false)
            .with_inner_size([1280.0, 720.0])
            .with_min_inner_size([800.0, 600.0])
            .with_transparent(true)
            .with_title("Supersaw"),
        ..Default::default()
    };

    let pa = match portaudio::PortAudio::new() {
        Ok(pa) => pa,
        Err(e) => {
            eprintln!("Failed to initialize PortAudio: {}", e);
            std::process::exit(1);
        }
    };

    // Run the app
    eframe::run_native(
        "Supersaw",
        options,
        Box::new(|cc| Ok(Box::new(ui::SupersawApp::new(cc)))),
    )
}
