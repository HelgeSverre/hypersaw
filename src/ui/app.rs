use crate::core::{
    Clip, CommandManager, DawState, EditorView, MessageType, Project, StatusMessage, Track,
    TrackType,
};
use crate::ui::piano_roll::PianoRoll;
use crate::ui::Timeline;
use eframe::egui;
use egui::debug_text::print;
use egui::Key;
use egui::Shape::Path;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;
use uuid::Uuid;

pub struct SupersawApp {
    state: DawState,
    command_manager: CommandManager,
    midi_output: Option<midir::MidiOutputConnection>,
    midi_ports: Vec<String>,
    file_dialog: Option<FileDialog>,
    // keymap: HashMap<Key, KeyAction>,
    timeline: Timeline,
    piano_roll: PianoRoll,
}

enum FileDialog {
    SaveProject,
    LoadProject,
    ImportAudio,
    ImportMidi,
}

impl SupersawApp {
    fn initialize_keymap() -> HashMap<Key, KeyAction> {
        use KeyAction::*;
        let mut keymap = HashMap::new();

        // Add key bindings
        keymap.insert(Key::O, LoadProject);
        keymap.insert(Key::S, SaveProject);
        keymap.insert(Key::Z, Undo);
        keymap.insert(Key::R, Redo);

        keymap
    }

    fn handle_key_action(&mut self, action: KeyAction) {
        match action {
            KeyAction::LoadProject => {
                self.file_dialog = Some(FileDialog::LoadProject);
            }
            KeyAction::SaveProject => {
                self.file_dialog = Some(FileDialog::SaveProject);
            }
            KeyAction::Undo => {
                if let Err(e) = self.command_manager.undo(&mut self.state) {
                    eprintln!("Undo failed: {}", e);
                    self.state.status.error(format!("Undo failed: {}", e));
                }
            }
            KeyAction::Redo => {
                if let Err(e) = self.command_manager.redo(&mut self.state) {
                    eprintln!("Redo failed: {}", e);
                    self.state.status.error(format!("Redo failed: {}", e));
                }
            }
        }
    }

    fn scan_midi_ports() -> Vec<String> {
        match midir::MidiOutput::new("Supersaw") {
            Ok(midi_out) => {
                let ports = midi_out.ports();
                ports
                    .iter()
                    .filter_map(|port| midi_out.port_name(port).ok())
                    .collect()
            }
            Err(err) => {
                eprintln!("Error creating MIDI output: {}", err);
                Vec::new()
            }
        }
    }

    fn connect_midi_port(&mut self, port_name: &str) -> Result<(), Box<dyn std::error::Error>> {
        let midi_out = midir::MidiOutput::new("Supersaw")?;
        let ports = midi_out.ports();

        for port in ports {
            if midi_out.port_name(&port)? == port_name {
                self.midi_output = Some(midi_out.connect(&port, "Supersaw")?);
                return Ok(());
            }
        }

        Err("MIDI port not found".into())
    }

    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // Initialize keymap
        let keymap = Self::initialize_keymap();

        // Set up MIDI output
        let midi_ports = Self::scan_midi_ports();
        let midi_out: Option<midir::MidiOutputConnection> = None;

        // Create the app instance
        let mut app = Self {
            state: DawState::new(),
            midi_output: None,
            midi_ports: Self::scan_midi_ports(),
            file_dialog: None,
            timeline: Timeline::default(),
            piano_roll: PianoRoll::default(),
            command_manager: CommandManager::new(),
            // keymap,
        };

        app.state.status.set_message(
            StatusMessage::new("Initialized successfully", MessageType::Success)
                .with_duration(Duration::from_secs(1)),
        );

        let dummy_midis = [
            "/Users/helge/code/hypersaw/data/moon-loves-the-sun.mid",
            "/Users/helge/code/hypersaw/data/emotions.mid",
            "/Users/helge/code/hypersaw/data/silentium.mid",
            "/Users/helge/code/hypersaw/data/system-f-out-of-the-blue.mid",
        ];

        // Add 4 test tracks
        for (i, midi_file) in dummy_midis.iter().enumerate() {
            let track = Track {
                id: Uuid::new_v4().to_string(),
                name: format!("Track {}", i + 1),
                track_type: TrackType::Midi {
                    channel: 1,
                    device_name: String::from(midi_file.split('/').last().unwrap()),
                },
                clips: vec![Clip::Midi {
                    id: Uuid::new_v4().to_string(),
                    start_time: 0.0,
                    length: 4.0,
                    file_path: PathBuf::from(midi_file),
                }],
                is_muted: false,
                is_soloed: false,
            };

            app.state.project.tracks.push(track);
        }

        app
    }

    fn draw_transport(&mut self, ui: &mut egui::Ui) {
        // todo: make fixed with buttons
        ui.horizontal(|ui| {
            ui.set_min_height(32.0);

            if ui
                .button(if self.state.playing { "⏹" } else { "▶" })
                .clicked()
            {
                self.state.playing = !self.state.playing;
            }

            if ui.button("rec").clicked() {
                self.state.recording = !self.state.recording;
            }

            ui.label(format!("BPM: {:.1}", self.state.project.bpm));
            if ui.button("−").clicked() && self.state.project.bpm > 20.0 {
                self.state.project.bpm -= 1.0;
            }
            if ui.button("+").clicked() && self.state.project.bpm < 300.0 {
                self.state.project.bpm += 1.0;
            }

            ui.label(format!("Time: {:.1}", self.state.current_time));
        });
    }

    fn draw_track_list(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical().show(ui, |ui| {
            for track in &mut self.state.project.tracks {
                ui.horizontal(|ui| {
                    let track_label = format!(
                        "{} ({})",
                        track.name,
                        match track.track_type {
                            TrackType::Midi { .. } => "MIDI",
                            TrackType::DrumRack { .. } => "Drum",
                            TrackType::Audio => "Audio",
                        }
                    );

                    if ui
                        .selectable_label(
                            self.state.selected_track == Some(track.id.clone()),
                            track_label,
                        )
                        .clicked()
                    {
                        self.state.selected_track = Some(track.id.clone());
                    }
                    ui.add_space(18.0);

                    ui.checkbox(&mut track.is_muted, "M");
                    ui.checkbox(&mut track.is_soloed, "S");
                });
            }

            if ui.button("+ Add Track").clicked() {
                self.show_add_track_menu();
            }
        });
    }

    fn show_add_track_menu(&mut self) {
        let track_id = Uuid::new_v4().to_string();
        self.state.project.tracks.push(Track {
            id: track_id.clone(),
            name: format!("Track {}", self.state.project.tracks.len() + 1),
            track_type: TrackType::Midi {
                channel: 1,
                device_name: String::new(),
            },
            clips: Vec::new(),
            is_muted: false,
            is_soloed: false,
        });
        self.state.selected_track = Some(track_id);
    }
}

enum KeyAction {
    LoadProject,
    SaveProject,
    Undo,
    Redo,
}

impl eframe::App for SupersawApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {


        // Keyboard shortcuts
        // SAVE -  Ctrl + S
        // REDO -  Shift + Ctrl + Z
        // UNDO -  Ctrl + Z
        ctx.input(|i| {
            if i.key_pressed(Key::Z) && (i.modifiers.ctrl || i.modifiers.command) {
                if i.modifiers.shift {
                    self.handle_key_action(KeyAction::Redo);
                } else {
                    self.handle_key_action(KeyAction::Undo);
                }
            }
        });

        egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("New Project").clicked() {
                        self.state = DawState::new();
                        ui.close_menu();
                    }
                    if ui.button("Save Project").clicked() {
                        self.file_dialog = Some(FileDialog::SaveProject);
                        ui.close_menu();
                    }
                    if ui.button("Load Project").clicked() {
                        self.file_dialog = Some(FileDialog::LoadProject);
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("Import MIDI...").clicked() {
                        self.file_dialog = Some(FileDialog::ImportMidi);
                        ui.close_menu();
                    }
                    if ui.button("Import Audio...").clicked() {
                        self.file_dialog = Some(FileDialog::ImportAudio);
                        ui.close_menu();
                    }
                });
            });
        });

        egui::TopBottomPanel::bottom("status_bar").show(ctx, |ui| {
            self.state.status.update(); // Clear expired messages

            if let Some(message) = self.state.status.get_message() {
                let color = match message.message_type {
                    MessageType::Info => ui.visuals().text_color(),
                    MessageType::Success => egui::Color32::GREEN,
                    MessageType::Warning => egui::Color32::YELLOW,
                    MessageType::Error => egui::Color32::RED,
                };
                ui.colored_label(color, &message.text);
            }
        });

        egui::TopBottomPanel::top("transport").show(ctx, |ui| {
            self.draw_transport(ui);
        });

        egui::SidePanel::left("tracks")
            .default_width(200.0)
            .show(ctx, |ui| {
                self.draw_track_list(ui);
            });

        egui::CentralPanel::default().show(ctx, |ui| match &self.state.current_view {
            EditorView::Arrangement => {
                let commands = self.timeline.show(ui, &mut self.state);
                for command in commands {
                    if let Err(e) = self.command_manager.execute(command, &mut self.state) {
                        eprintln!("timeline: Command failed: {}", e);
                        self.state.status.error(format!("Command failed: {}", e));
                    }
                }
            }
            EditorView::PianoRoll { .. } => {
                let commands = self.piano_roll.show(ui, &mut self.state);
                for command in commands {
                    if let Err(e) = self.command_manager.execute(command, &mut self.state) {
                        eprintln!("piano_roll: Command failed: {}", e);
                        self.state.status.error(format!("Command failed: {}", e));
                    }
                }
            }
            EditorView::SampleEditor { .. } => {
                ui.label("Sample Editor (Not Implemented)");
            }
        });

        // Handle file dialogs
        if let Some(dialog_type) = &self.file_dialog {
            match dialog_type {
                // TODO: Implement dialog for naming the project
                FileDialog::SaveProject => {
                    // For now, just save to a fixed test location
                    let path = std::env::current_dir()
                        .unwrap()
                        .join("projects")
                        .join(self.state.project.name.clone());

                    match self.state.project.save(&path) {
                        Err(e) => {
                            self.state.status.error("Failed to save project");
                            eprintln!("Failed to save project: {}", path.display());
                            eprintln!("error: {}", e);
                        }
                        Ok(..) => {
                            self.state.status.success("Project saved successfully");
                            println!("Project saved to: {}", path.display());
                        }
                    }

                    self.file_dialog = None;
                }
                FileDialog::LoadProject => {
                    // Use a file dialog to allow the user to select a project file
                    if let Some(file_path) = rfd::FileDialog::new()
                        .set_title("Select Project File")
                        .add_filter("Supersaw Project", &["supersaw"])
                        .set_directory(std::env::current_dir().unwrap())
                        .pick_file()
                    {
                        println!("Selected project file: {}", file_path.display());

                        match Project::load(&file_path) {
                            Ok(project) => {
                                self.state.project = project;
                                self.state.status.success("Project loaded successfully");
                            }
                            Err(e) => {
                                self.state.status.error("Failed to load project");
                                eprintln!("Failed to load project: {}", file_path.display());
                                eprintln!("Error: {}", e);
                            }
                        }
                    } else {
                        println!("No project file selected.");
                    }

                    self.file_dialog = None;
                }
                _ => {
                    self.file_dialog = None;
                }
            }
        }
    }
}
