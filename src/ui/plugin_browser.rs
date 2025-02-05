// src/ui/plugin_browser.rs
use crate::core::*;
use eframe::egui;
use egui::{Id, Margin};
use std::path::PathBuf;

pub struct PluginBrowser {
    visible: bool,
    scan_paths: Vec<PathBuf>,
    plugins: Vec<PluginInfo>,
    selected_plugin: Option<usize>,
    filter_text: String,
    category_filter: Option<String>,
    is_scanning: bool,
    command_collector: CommandCollector,
}

#[derive(Clone, Debug)]
struct PluginInfo {
    name: String,
    path: PathBuf,
    category: String,
    format: PluginFormat,
    manufacturer: String,
    is_instrument: bool,
}

#[derive(Clone, Debug)]
enum PluginFormat {
    VST3,
    CLAP,
}

impl Default for PluginBrowser {
    fn default() -> Self {
        Self {
            visible: false,
            scan_paths: vec![
                PathBuf::from("/Library/Audio/Plug-ins/VST3"),
                PathBuf::from("/Library/Audio/Plug-ins/CLAP"),
                // Add default paths for Windows/Linux
            ],
            plugins: Vec::new(),
            selected_plugin: None,
            filter_text: String::new(),
            category_filter: None,
            is_scanning: false,
            command_collector: CommandCollector::new(),
        }
    }
}

impl PluginBrowser {
    pub fn show(&mut self, ctx: &egui::Context, state: &mut DawState) -> Vec<DawCommand> {
        if !self.visible {
            return vec![];
        }

        // Create a dark overlay behind the browser
        egui::Area::new(Id::new("plugin_browser_overlay"))
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .movable(false)
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                // Draw a semi-transparent dark background
                let screen_rect = ui.ctx().screen_rect();
                ui.painter()
                    .rect_filled(screen_rect, 0.0, egui::Color32::from_black_alpha(192));
            });

        // Main browser window
        egui::Area::new(Id::new("plugin_browser"))
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .default_size(egui::vec2(800.0, 600.0))
            .movable(false)
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                ui.set_max_size(egui::vec2(800.0, 600.0));

                egui::Frame::window(&ctx.style())
                    .inner_margin(Margin::same(10.0))
                    .show(ui, |ui| {
                        self.draw_browser_contents(ui, state);
                    });
            });

        self.command_collector.take_commands()
    }

    fn draw_browser_contents(&mut self, ui: &mut egui::Ui, state: &mut DawState) {
        ui.vertical(|ui| {
            // Header
            ui.horizontal(|ui| {
                ui.heading("Plugin Browser");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("×").clicked() {
                        self.visible = false;
                    }
                });
            });
            ui.add_space(8.0);

            // Search and filters
            ui.horizontal(|ui| {
                ui.label("Search:");
                ui.text_edit_singleline(&mut self.filter_text);

                ui.separator();

                ui.label("Category:");
                egui::ComboBox::from_label("")
                    .selected_text(self.category_filter.as_deref().unwrap_or("All"))
                    .show_ui(ui, |ui| {
                        if ui
                            .selectable_value(&mut self.category_filter, None, "All")
                            .clicked()
                        {
                            self.category_filter = None;
                        }
                        for category in &["Instrument", "Effect", "Dynamics", "EQ", "Reverb"] {
                            if ui
                                .selectable_value(
                                    &mut self.category_filter,
                                    Some(category.to_string()),
                                    *category,
                                )
                                .clicked()
                            {
                                self.category_filter = Some(category.to_string());
                            }
                        }
                    });

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Rescan").clicked() {
                        self.scan_plugins();
                    }
                });
            });
            ui.add_space(8.0);

            // Main browser area with plugin list and details
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        // Plugin list (left side)
                        ui.vertical(|ui| {
                            ui.set_min_width(300.0);
                            ui.set_max_width(300.0);

                            let filtered_plugins =
                                self.plugins.iter().enumerate().filter(|(_, p)| {
                                    let name_matches = p
                                        .name
                                        .to_lowercase()
                                        .contains(&self.filter_text.to_lowercase());
                                    let category_matches = self
                                        .category_filter
                                        .as_ref()
                                        .map(|c| p.category == *c)
                                        .unwrap_or(true);
                                    name_matches && category_matches
                                });

                            for (idx, plugin) in filtered_plugins {
                                let is_selected = self.selected_plugin == Some(idx);
                                let response = ui.selectable_label(is_selected, &plugin.name);

                                if response.clicked() {
                                    self.selected_plugin = Some(idx);
                                }

                                if response.double_clicked() {
                                    println!("Load Plugin: {:?}", plugin.path);

                                    if let Some(track_id) = &state.selected_track {
                                        // self.command_collector.add_command(
                                        //     DawCommand::LoadPlugin {
                                        //         track_id: track_id.clone(),
                                        //         path: plugin.path.clone(),
                                        //     },
                                        // );

                                        self.visible = false;
                                    }
                                }
                            }
                        });

                        ui.separator();

                        // Plugin details (right side)
                        ui.vertical(|ui| {
                            if let Some(idx) = self.selected_plugin {
                                if let Some(plugin) = self.plugins.get(idx) {
                                    ui.heading(&plugin.name);
                                    ui.add_space(8.0);

                                    ui.label(format!("Manufacturer: {}", plugin.manufacturer));
                                    ui.label(format!("Category: {}", plugin.category));
                                    ui.label(format!("Format: {:?}", plugin.format));
                                    ui.label(format!("Path: {}", plugin.path.display()));

                                    ui.add_space(16.0);

                                    if ui.button("Load Plugin").clicked() {
                                        println!("Load Plugin: {:?}", plugin.path);
                                        if let Some(track_id) = &state.selected_track {
                                            // self.command_collector.add_command(
                                            //     DawCommand::LoadPlugin {
                                            //         track_id: track_id.clone(),
                                            //         path: plugin.path.clone(),
                                            //     },
                                            // );

                                            self.visible = false;
                                        }
                                    }
                                }
                            } else {
                                ui.centered_and_justified(|ui| {
                                    ui.label("Select a plugin to view details");
                                });
                            }
                        });
                    });
                });

            // Status bar
            ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                ui.label(format!("{} plugins found", self.plugins.len()));
            });
        });
    }

    fn scan_plugins(&mut self) {
        self.is_scanning = true;
        self.plugins.clear();

        // TODO: Implement actual plugin scanning
        // For now, just add some dummy plugins
        self.plugins.extend(vec![
            PluginInfo {
                name: "Example Synth".into(),
                path: PathBuf::from("/plugins/example_synth.vst3"),
                category: "Instrument".into(),
                format: PluginFormat::VST3,
                manufacturer: "Example Audio".into(),
                is_instrument: true,
            },
            PluginInfo {
                name: "Example Reverb".into(),
                path: PathBuf::from("/plugins/example_reverb.vst3"),
                category: "Reverb".into(),
                format: PluginFormat::VST3,
                manufacturer: "Example Audio".into(),
                is_instrument: false,
            },
        ]);

        self.is_scanning = false;
    }

    pub fn show_browser(&mut self) {
        self.visible = true;
        if self.plugins.is_empty() {
            self.scan_plugins();
        }
    }

    pub fn hide_browser(&mut self) {
        self.visible = false;
    }
}
