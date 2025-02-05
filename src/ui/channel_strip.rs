use crate::core::*;
use eframe::egui;

pub struct ChannelStripWindow {
    track_id: String,
    track_name: String,
    window_size: egui::Vec2,
}

impl ChannelStripWindow {
    pub fn new(track_id: String, track_name: String) -> Self {
        Self {
            track_id,
            track_name,
            window_size: egui::Vec2::new(300.0, 600.0),
        }
    }

    pub fn show(&mut self, ctx: &egui::Context, state: &mut DawState) {
        egui::Window::new(format!("Channel: {}", self.track_name))
            .default_width(300.0)
            .resizable(true)
            .collapsible(false)
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    self.draw_input_section(ui);
                    ui.add_space(8.0);
                    self.draw_fx_section(ui);
                    ui.add_space(8.0);
                    self.draw_output_section(ui);
                });
            });
    }

    fn draw_input_section(&mut self, ui: &mut egui::Ui) {
        egui::Frame::new()
            .fill(ui.style().visuals.extreme_bg_color)
            .stroke(ui.style().visuals.widgets.noninteractive.bg_stroke)
            .corner_radius(4.0)
            .inner_margin(8.0)
            .show(ui, |ui| {
                ui.vertical(|ui| {
                    ui.heading(&self.track_name);
                    ui.add_space(4.0);

                    // Input gain
                    ui.label("Input Gain");
                    let mut gain = 0.0;
                    ui.add(
                        egui::Slider::new(&mut gain, -60.0..=6.0)
                            .text("dB")
                            .vertical(),
                    );

                    // Phase invert button
                    ui.add_space(4.0);
                    if ui.button("ø").clicked() {}
                });
            });
    }

    fn draw_fx_section(&mut self, ui: &mut egui::Ui) {
        egui::Frame::new()
            .fill(ui.style().visuals.extreme_bg_color)
            .stroke(ui.style().visuals.widgets.noninteractive.bg_stroke)
            .corner_radius(4.0)
            .inner_margin(8.0)
            .show(ui, |ui| {
                ui.vertical(|ui| {
                    ui.heading("Effects");
                    ui.add_space(8.0);

                    // Draw 8 simple effect slots
                    for i in 0..4 {
                        egui::Frame::new()
                            .fill(ui.style().visuals.faint_bg_color)
                            .stroke(ui.style().visuals.widgets.noninteractive.bg_stroke)
                            .corner_radius(2.0)
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    if ui.button("⏽").clicked() {}
                                    ui.label(format!("Effect Slot {}", i + 1));
                                });
                            });
                        ui.add_space(4.0);
                    }
                });
            });
    }

    fn draw_output_section(&mut self, ui: &mut egui::Ui) {
        egui::Frame::new()
            .fill(ui.style().visuals.extreme_bg_color)
            .stroke(ui.style().visuals.widgets.noninteractive.bg_stroke)
            .corner_radius(4.0)
            .inner_margin(8.0)
            .show(ui, |ui| {
                ui.vertical(|ui| {
                    // Pan control
                    ui.label("Pan");
                    let mut pan = 0.0;
                    ui.add(egui::Slider::new(&mut pan, -1.0..=1.0));

                    ui.add_space(8.0);

                    // Output fader
                    ui.label("Output");
                    let mut gain = 0.0;
                    ui.add(
                        egui::Slider::new(&mut gain, -60.0..=6.0)
                            .text("dB")
                            .vertical(),
                    );

                    // Mute/Solo buttons
                    ui.horizontal(|ui| {
                        if ui.button("M").clicked() {}
                        if ui.button("S").clicked() {}
                    });
                });
            });
    }
}
