// // // src/core/plugin.rs
// // use eframe::egui;
// // use std::collections::HashMap;
// // use std::path::{Path, PathBuf};
// // use std::sync::{Arc, Mutex};
// //
// // #[derive(Debug)]
// // pub struct PluginInstance {
// //     pub id: String,
// //     pub name: String,
// //     pub path: PathBuf,
// //     // pub plugin: Arc<Mutex<Box<dyn IPlugin>>>,
// //     pub parameters: Vec<PluginParameter>,
// //     pub window: Option<PluginWindow>,
// // }
// //
// // #[derive(Debug)]
// // pub struct PluginParameter {
// //     pub id: i32,
// //     pub name: String,
// //     pub value: f32,
// //     pub default: f32,
// //     pub min: f32,
// //     pub max: f32,
// // }
// //
// // // Separate window for plugin UIs
// // pub struct PluginWindow {
// //     window: eframe::Window,
// //     size: (u32, u32),
// //     plugin_id: String,
// // }
// //
// // impl PluginWindow {
// //     fn new(plugin_id: String, title: String) -> Self {
// //         let window = eframe::Window::new(title)
// //             .default_width(800.0)
// //             .default_height(600.0)
// //             .resizable(true);
// //
// //         Self {
// //             window,
// //             size: (800, 600),
// //             plugin_id,
// //         }
// //     }
// // }
// //
// // pub struct PluginManager {
// //     plugins: HashMap<String, PluginInstance>,
// //     factory_cache: HashMap<PathBuf, Arc<PluginFactory>>,
// //     host: Arc<Host>,
// // }
// //
// // impl PluginManager {
// //     pub fn new() -> Self {
// //         Self {
// //             plugins: HashMap::new(),
// //             factory_cache: HashMap::new(),
// //             host: Arc::new(Host::new()),
// //         }
// //     }
// //
// //     pub fn load_plugin(&mut self, path: &Path) -> Result<String, Box<dyn std::error::Error>> {
// //         // Load or get cached factory
// //         let factory = if let Some(factory) = self.factory_cache.get(path) {
// //             factory.clone()
// //         } else {
// //             let factory = Arc::new(PluginFactory::load(path)?);
// //             self.factory_cache
// //                 .insert(path.to_path_buf(), factory.clone());
// //             factory
// //         };
// //
// //         // Create plugin instance
// //         let plugin = factory
// //             .create_instance::<dyn IPlugin>(0)
// //             .ok_or("Failed to create plugin instance")?;
// //
// //         // Generate unique ID
// //         let id = uuid::Uuid::new_v4().to_string();
// //
// //         // Get plugin info
// //         let info = plugin.get_info();
// //
// //         // Initialize plugin
// //         plugin.initialize(self.host.clone())?;
// //
// //         // Create plugin instance
// //         let instance = PluginInstance {
// //             id: id.clone(),
// //             name: info.name.unwrap_or_else(|| "Unknown Plugin".to_string()),
// //             path: path.to_path_buf(),
// //             plugin: Arc::new(Mutex::new(plugin)),
// //             parameters: Vec::new(), // TODO: Load parameters
// //             window: None,
// //         };
// //
// //         // Store instance
// //         self.plugins.insert(id.clone(), instance);
// //
// //         Ok(id)
// //     }
// //
// //     pub fn show_plugin_ui(&mut self, plugin_id: &str) -> Result<(), Box<dyn std::error::Error>> {
// //         let instance = self.plugins.get_mut(plugin_id).ok_or("Plugin not found")?;
// //
// //         // Create window if it doesn't exist
// //         if instance.window.is_none() {
// //             let window =
// //                 PluginWindow::new(plugin_id.to_string(), format!("Plugin: {}", instance.name));
// //             instance.window = Some(window);
// //         }
// //
// //         Ok(())
// //     }
// //
// //     pub fn process_audio(
// //         &mut self,
// //         plugin_id: &str,
// //         input: &[f32],
// //         output: &mut [f32],
// //     ) -> Result<(), Box<dyn std::error::Error>> {
// //         let instance = self.plugins.get_mut(plugin_id).ok_or("Plugin not found")?;
// //
// //         // Lock plugin for processing
// //         let mut plugin = instance.plugin.lock().unwrap();
// //
// //         // TODO: Implement actual audio processing
// //         // This will depend on your audio engine architecture
// //
// //         Ok(())
// //     }
// // }
// //
// // // Add new commands
// // #[derive(Debug)]
// // pub enum DawCommand {
// //     // ... existing commands ...
// //     LoadPlugin {
// //         track_id: String,
// //         path: PathBuf,
// //     },
// //     ShowPluginUI {
// //         plugin_id: String,
// //     },
// //     SetPluginParameter {
// //         plugin_id: String,
// //         param_id: i32,
// //         value: f32,
// //     },
// // }
// //
// // // Add plugin support to Track
// // #[derive(Debug, Clone)]
// // pub enum TrackType {
// //     // ... existing variants ...
// //     Instrument {
// //         plugin_id: Option<String>,
// //         midi_channel: u8,
// //     },
// //     Effect {
// //         plugin_id: Option<String>,
// //     },
// // }
// //
// // // Implement plugin UI window
// // pub struct PluginEditorWindow {
// //     plugin_id: String,
// //     size: egui::Vec2,
// // }
// //
// // impl PluginEditorWindow {
// //     pub fn new(plugin_id: String) -> Self {
// //         Self {
// //             plugin_id,
// //             size: egui::Vec2::new(800.0, 600.0),
// //         }
// //     }
// // }
// //
// // impl eframe::App for PluginEditorWindow {
// //     fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
// //         egui::CentralPanel::default().show(ctx, |ui| {
// //             // Here you would render the plugin's UI
// //             // For native plugin windows, you'd attach them to this window
// //             ui.label("Plugin UI Window");
// //
// //             // Example parameter controls
// //             ui.add(egui::Slider::new(&mut 0.5, 0.0..=1.0).text("Parameter 1"));
// //             ui.add(egui::Slider::new(&mut 0.5, 0.0..=1.0).text("Parameter 2"));
// //         });
// //     }
// // }
// //
// // // Update SupersawApp implementation
// // impl SupersawApp {
// //     pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
// //         let mut app = Self {
// //             // ... existing initialization ...
// //             plugin_windows: Vec::new(),
// //         };
// //
// //         app
// //     }
// //
// //     fn handle_plugin_command(
// //         &mut self,
// //         command: DawCommand,
// //     ) -> Result<(), Box<dyn std::error::Error>> {
// //         match command {
// //             DawCommand::LoadPlugin { track_id, path } => {
// //                 let plugin_id = self.state.plugin_manager.load_plugin(&path)?;
// //
// //                 // Update track with plugin ID
// //                 if let Some(track) = self
// //                     .state
// //                     .project
// //                     .tracks
// //                     .iter_mut()
// //                     .find(|t| t.id == track_id)
// //                 {
// //                     match &mut track.track_type {
// //                         TrackType::Instrument { plugin_id: pid, .. } => {
// //                             *pid = Some(plugin_id);
// //                         }
// //                         TrackType::Effect { plugin_id: pid } => {
// //                             *pid = Some(plugin_id);
// //                         }
// //                         _ => return Err("Invalid track type for plugin".into()),
// //                     }
// //                 }
// //
// //                 Ok(())
// //             }
// //
// //             DawCommand::ShowPluginUI { plugin_id } => {
// //                 // Create new window for plugin
// //                 let options = eframe::NativeOptions {
// //                     viewport: egui::ViewportBuilder::default()
// //                         .with_inner_size([800.0, 600.0])
// //                         .with_title("Plugin Editor"),
// //                     ..Default::default()
// //                 };
// //
// //                 let plugin_window = PluginEditorWindow::new(plugin_id.clone());
// //
// //                 eframe::run_native(
// //                     &format!("Plugin: {}", plugin_id),
// //                     options,
// //                     Box::new(|_cc| Box::new(plugin_window)),
// //                 )?;
// //
// //                 Ok(())
// //             }
// //
// //             _ => Ok(()),
// //         }
// //     }
// // }
// //
// // // Add to your app.rs update function
// // impl eframe::App for SupersawApp {
// //     fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
// //         // ... existing update code ...
// //
// //         // Add plugin menu
// //         egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
// //             egui::menu::bar(ui, |ui| {
// //                 ui.menu_button("Plugins", |ui| {
// //                     if ui.button("Load Plugin...").clicked() {
// //                         if let Some(path) = rfd::FileDialog::new()
// //                             .add_filter("VST3 Plugins", &["vst3"])
// //                             .pick_file()
// //                         {
// //                             if let Some(track_id) = &self.state.selected_track {
// //                                 if let Err(e) = self.handle_plugin_command(DawCommand::LoadPlugin {
// //                                     track_id: track_id.clone(),
// //                                     path: path,
// //                                 }) {
// //                                     self.state
// //                                         .status
// //                                         .error(format!("Failed to load plugin: {}", e));
// //                                 }
// //                             }
// //                         }
// //                         ui.close_menu();
// //                     }
// //                 });
// //             });
// //         });
// //
// //         // Update track controls to show plugin options
// //         self.draw_track_list(ui, |ui, track| match &track.track_type {
// //             TrackType::Instrument {
// //                 plugin_id: Some(plugin_id),
// //                 ..
// //             }
// //             | TrackType::Effect {
// //                 plugin_id: Some(plugin_id),
// //             } => {
// //                 if ui.button("Edit Plugin").clicked() {
// //                     if let Err(e) = self.handle_plugin_command(DawCommand::ShowPluginUI {
// //                         plugin_id: plugin_id.clone(),
// //                     }) {
// //                         self.state
// //                             .status
// //                             .error(format!("Failed to show plugin UI: {}", e));
// //                     }
// //                 }
// //             }
// //             _ => {}
// //         });
// //     }
// // }
// //
// // use eframe::egui;
// // use raw_window_handle::RawWindowHandle;
// // use std::ffi::CString;
// // use vst3::plugin::PluginFactory;
// //
// // fn load_vst3_plugin(path: &str) -> Result<(), Box<dyn std::error::Error>> {
// //     let path = CString::new(path)?;
// //     let factory = PluginFactory::load(path.as_ref())?;
// //
// //     if let Some(plugin) = factory.create_instance::<vst3::plugin::IPlugin>(0) {
// //         println!("Loaded VST3 Plugin Successfully!");
// //
// //         // Check if the plugin has an editor
// //         if let Some(editor) = plugin.get_editor() {
// //             let editor_handle = editor.open(RawWindowHandle::Wayland); // Adjust for platform
// //             println!("Opened Plugin UI: {:?}", editor_handle);
// //         }
// //     } else {
// //         println!("Failed to create VST3 plugin instance.");
// //     }
// //
// //     Ok(())
// // }
// //
// // fn main() {
// //     let options = eframe::NativeOptions::default();
// //     eframe::run_native(
// //         "VST3 Host with Plugin UI",
// //         options,
// //         Box::new(|_cc| Box::new(MyApp::default())),
// //     );
// // }
// //
// // struct MyApp {
// //     plugin_loaded: bool,
// // }
// //
// // impl Default for MyApp {
// //     fn default() -> Self {
// //         Self {
// //             plugin_loaded: false,
// //         }
// //     }
// // }
// //
// // impl eframe::App for MyApp {
// //     fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
// //         egui::CentralPanel::default().show(ctx, |ui| {
// //             ui.heading("VST3 Host with egui");
// //
// //             if ui.button("Load VST3 Plugin").clicked() {
// //                 let plugin_path = "/path/to/plugin.vst3";
// //                 match load_vst3_plugin(plugin_path) {
// //                     Ok(_) => self.plugin_loaded = true,
// //                     Err(err) => ui.label(format!("Error: {}", err)),
// //                 }
// //             }
// //
// //             if self.plugin_loaded {
// //                 ui.label("Plugin Loaded Successfully!");
// //                 ui.label("Plugin GUI should be displayed in a separate window.");
// //             }
// //         });
// //     }
// // }
//
//
//
// use vst3::Steinberg::Vst::{IPluginFactory, IPluginBase, IEditController};
// use vst3::{ComPtr, ComWrapper};
// use raw_window_handle::HasRawWindowHandle;
// use std::path::Path;
// use std::error::Error;
//
//
//
// fn load_vst3_plugin(plugin_path: &str) -> Result<ComPtr<dyn IPluginFactory>, Box<dyn Error>> {
//     let module = unsafe { ComWrapper::load_library(Path::new(plugin_path))? };
//     let factory: ComPtr<dyn IPluginFactory> = module.get_class_factory()?;
//     println!("Loaded VST3 Plugin Successfully!");
//     Ok(factory)
// }
//
//
// fn main() {
//     let plugin_path = "/path/to/plugin.vst3";
//     match load_vst3_plugin(plugin_path) {
//         Ok(factory) => println!("Plugin factory loaded"),
//         Err(e) => eprintln!("Error: {}", e),
//     }
// }
