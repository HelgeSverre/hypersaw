use crate::core::{
    CommandManager, DawCommand, DawState, EditorView, MessageType, MidiMessage, Project, SnapMode,
    StatusMessage, Track, TrackType,
};
use crate::ui::piano_roll::PianoRoll;
use crate::ui::plugin_browser::PluginBrowser;
use crate::ui::Timeline;
use eframe::egui;
use eframe::emath::Align;
use egui::Key;
use std::path::PathBuf;
use std::time::Duration;
use uuid::Uuid;

pub struct SupersawApp {
    state: DawState,
    command_manager: CommandManager,
    midi_output: Option<midir::MidiOutputConnection>,
    midi_ports: Vec<String>,
    file_dialog: Option<FileDialog>,

    // Views
    timeline: Timeline,
    piano_roll: PianoRoll,
    plugin_browser: PluginBrowser,
}

enum FileDialog {
    SaveProject,
    LoadProject,
    ImportAudio,
    ImportMidi,
}

impl SupersawApp {
    fn handle_key_action(&mut self, action: KeyAction) {
        match action {
            KeyAction::TogglePlay => {
                if let Err(e) = self.command_manager.execute(
                    if self.state.playing {
                        DawCommand::PausePlayback
                    } else {
                        DawCommand::StartPlayback
                    },
                    &mut self.state,
                ) {
                    eprintln!("Failed to toggle playback: {}", e);
                    self.state
                        .status
                        .error(format!("Failed to toggle playback: {}", e));
                }
            }
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
            Ok(midi_out) => midi_out
                .ports()
                .iter()
                .filter_map(|port| midi_out.port_name(port).ok())
                .collect(),
            Err(err) => {
                eprintln!("Error creating MIDI output: {}", err);
                Vec::new()
            }
        }
    }

    fn connect_midi_port(&mut self, port_name: &str) -> Result<(), Box<dyn std::error::Error>> {
        // First disconnect any existing connection
        self.midi_output = None;

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

    fn send_midi_message(
        &mut self,
        channel: u8,
        message: &MidiMessage,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(midi_out) = &mut self.midi_output {
            match message {
                MidiMessage::NoteOn { key, velocity, .. } => {
                    let midi_message = [0x90 | (channel - 1), *key, *velocity];
                    midi_out.send(&midi_message)?;
                }
                MidiMessage::NoteOff { key, velocity, .. } => {
                    let midi_message = [0x80 | (channel - 1), *key, *velocity];
                    midi_out.send(&midi_message)?;
                }
                MidiMessage::ControlChange {
                    controller, value, ..
                } => {
                    let midi_message = [0xB0 | (channel - 1), *controller, *value];
                    midi_out.send(&midi_message)?;
                }
                // Add other MIDI message types as needed
                _ => {}
            }
            Ok(())
        } else {
            Err("No MIDI output connected".into())
        }
    }

    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // Set up MIDI output
        let midi_ports = Self::scan_midi_ports();
        let midi_out: Option<midir::MidiOutputConnection> = None;

        // Create the app instance
        let midi_ports = Self::scan_midi_ports();
        let mut timeline = Timeline::default();
        timeline.update_midi_ports(midi_ports.clone());
        
        let mut app = Self {
            // TODO: reconsider where this should "live"
            state: DawState::new(),
            midi_output: None,
            midi_ports,
            file_dialog: None,
            timeline,
            piano_roll: PianoRoll::default(),
            command_manager: CommandManager::default(),
            plugin_browser: PluginBrowser::default(),
        };

        app.state.status.set_message(
            StatusMessage::new("Initialized successfully", MessageType::Success)
                .with_duration(Duration::from_secs(1)),
        );

        let dummy_midis = [
            "data/4bars.mid",
            // "data/emotions.mid",
            // "data/silentium.mid",
        ];

        for midi_file in dummy_midis.iter() {
            let file_path = PathBuf::from(midi_file);

            if let Err(e) = app
                .state
                .project
                .create_midi_track_from_file_path(&file_path)
            {
                app.state
                    .status
                    .error(format!("Failed to create track from MIDI file: {}", e));
            }
        }

        app
    }

    fn draw_transport(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.set_min_height(32.0);

            // Play/Stop button
            if ui
                .button(if self.state.playing { "⏹" } else { "▶" })
                .clicked()
            {
                self.state.playing = !self.state.playing;
                if self.state.playing {
                    self.state.last_update = Some(std::time::Instant::now());
                }
            }

            if ui.button("⏮").clicked() {
                // Return to start
                self.state.current_time = 0.0;
            }

            // Toggle metronome
            if ui.button("M").clicked() {
                if let Err(e) = self.command_manager.execute(
                    if self.state.metronome {
                        DawCommand::DisableMetronome
                    } else {
                        DawCommand::EnableMetronome
                    },
                    &mut self.state,
                ) {
                    self.state
                        .status
                        .error(format!("Failed to toggle metronome: {}", e));
                }
            }

            if ui.button("Rec").clicked() {
                self.state.recording = !self.state.recording
            }

            ui.separator();

            ui.label(format!("BPM: {:.1}", self.state.project.bpm));

            for (label, delta) in [("−", -1.0), ("+", 1.0)] {
                if ui.button(label).clicked() {
                    if let Err(e) = self.command_manager.execute(
                        DawCommand::SetBpm {
                            bpm: self.state.project.bpm + delta,
                        },
                        &mut self.state,
                    ) {
                        self.state.status.error(format!("Failed to set BPM: {}", e));
                    }
                }
            }

            ui.separator();
            egui::ComboBox::from_label("Snap")
                .selected_text(self.state.snap_mode.display_name())
                .show_ui(ui, |ui| {
                    for snap_mode in [
                        SnapMode::None,
                        SnapMode::Bar,
                        SnapMode::Beat,
                        SnapMode::Halfbeat,
                        SnapMode::Quarter,
                        SnapMode::Eighth,
                        SnapMode::Triplet,
                    ] {
                        if ui
                            .selectable_value(
                                &mut self.state.snap_mode,
                                snap_mode,
                                snap_mode.display_name(),
                            )
                            .clicked()
                        {
                            if let Err(e) = self
                                .command_manager
                                .execute(DawCommand::SetSnapMode { snap_mode }, &mut self.state)
                            {
                                self.state
                                    .status
                                    .error(format!("Failed to set snap mode: {}", e));
                            }
                        }
                    }
                });

            ui.separator();

            // Display formatted time
            let minutes = (self.state.current_time / 60.0).floor();
            let seconds = (self.state.current_time % 60.0).floor();
            let frames = ((self.state.current_time % 1.0) * 30.0).floor(); // Assuming 30fps
            ui.label(format!("{:02}:{:02}:{:02}", minutes, seconds, frames));

            ui.separator();

            let mut loop_enabled = self.state.loop_enabled;
            if ui.toggle_value(&mut loop_enabled, "⟲").clicked() {
                self.state.loop_enabled = loop_enabled;
            }
            if ui.button("Set Start").clicked() {
                self.state.loop_start = self.state.current_time;
            }
            if ui.button("Set End").clicked() {
                self.state.loop_end = self.state.current_time;
            }

            let loop_range = format!(
                "{:.1}s - {:.1}s",
                self.state.loop_start, self.state.loop_end
            );
            ui.label(loop_range);

            ui.separator();

            if ui.button("Arrangement").clicked() {
                self.state.current_view = EditorView::Arrangement;
            }
            
            ui.separator();
            
            // MIDI settings menu
            ui.menu_button("MIDI", |ui| {
                if ui.button("Refresh MIDI Ports").clicked() {
                    self.midi_ports = Self::scan_midi_ports();
                    self.timeline.update_midi_ports(self.midi_ports.clone());
                    ui.close_menu();
                }
            });
        });
    }


    fn import_midi_file(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(file_path) = rfd::FileDialog::new()
            .set_title("Select MIDI File")
            .add_filter("MIDI Files", &["mid", "midi"])
            .set_directory(std::env::current_dir().unwrap())
            .pick_file()
        {
            let track_id = self
                .state
                .project
                .create_midi_track_from_file_path(&file_path)?;

            // Select the newly created track
            self.state.selected_track = Some(track_id);

            self.state.status.success(format!(
                "Imported MIDI file: {}",
                file_path.file_name().unwrap_or_default().to_string_lossy()
            ));
        }

        Ok(())
    }
}

enum KeyAction {
    TogglePlay,
    LoadProject,
    SaveProject,
    Undo,
    Redo,
}

impl eframe::App for SupersawApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.state.update_playhead();

        // Send MIDI events during playback
        if self.state.playing {
            // Get all MIDI events for the current time step
            let lookahead = 0.01;
            let start_time = self.state.current_time;
            let end_time = self.state.current_time + lookahead;

            let events = self
                .state
                .project
                .get_all_events_in_time_range(start_time, end_time);

            for (track_id, event) in events {
                // Find the track for this event
                if let Some(track) = self.state.project.tracks.iter().find(|t| t.id == track_id) {
                    // If it's a MIDI track, send the event
                    if let TrackType::Midi {
                        channel,
                        device_name,
                    } = &track.track_type
                    {
                        if let Some(device) = device_name {
                            if !device.is_empty() && !track.is_muted {
                                // Check if track is soloed, or if no tracks are soloed
                                let any_soloed =
                                    self.state.project.tracks.iter().any(|t| t.is_soloed);
                                if !any_soloed || track.is_soloed {
                                    if let Err(e) = self.send_midi_message(*channel, &event.message)
                                    {
                                        // Log the error, but don't show in UI to avoid spam
                                        eprintln!("Failed to send MIDI message: {}", e);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

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

            if i.key_pressed(Key::S) && (i.modifiers.ctrl || i.modifiers.command) {
                self.handle_key_action(KeyAction::SaveProject);
            }

            if i.key_pressed(Key::Space) {
                self.handle_key_action(KeyAction::TogglePlay);
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

                ui.menu_button("Plugins", |ui| {
                    if ui.button("Browse Plugins...").clicked() {
                        self.plugin_browser.show_browser();
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

        // Update timeline with current MIDI ports
        self.timeline.update_midi_ports(self.midi_ports.clone());

        // Draw the main content area
        egui::CentralPanel::default().show(ctx, |ui| match &self.state.current_view {
            EditorView::Arrangement => {
                let commands = self.timeline.show(ui, &mut self.state);
                for command in commands {
                    if let Err(e) = self.command_manager.execute(command, &mut self.state) {
                        eprintln!("timeline: Command failed: {}", e);
                        self.state.status.error(format!("Command failed: {}", e));
                    }
                }
                
                // Handle pending MIDI connections from timeline
                let pending_connections = self.timeline.take_pending_midi_connections();
                for (track_id, device_name) in pending_connections {
                    if device_name.is_empty() {
                        // Disconnect
                        self.midi_output = None;
                        self.state
                            .status
                            .info("MIDI output disconnected".to_string());
                            
                        // Update track device name
                        if let Some(track) = self.state.project.tracks.iter_mut().find(|t| t.id == track_id) {
                            if let TrackType::Midi { device_name: ref mut dev_name, .. } = &mut track.track_type {
                                *dev_name = None;
                            }
                        }
                    } else {
                        // Connect to the port
                        if let Err(e) = self.connect_midi_port(&device_name) {
                            self.state
                                .status
                                .error(format!("Failed to connect to MIDI port: {}", e));
                        } else {
                            self.state
                                .status
                                .success(format!("Connected to MIDI port: {}", device_name));
                                
                            // Update track device name
                            if let Some(track) = self.state.project.tracks.iter_mut().find(|t| t.id == track_id) {
                                if let TrackType::Midi { device_name: ref mut dev_name, .. } = &mut track.track_type {
                                    *dev_name = Some(device_name);
                                }
                            }
                        }
                    }
                }
            }
            EditorView::PianoRoll { .. } => {
                let commands = self.piano_roll.show(ui, &mut self.state);
                for command in commands {
                    println!("command: {:?}", command);

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

        // Handle plugin browser
        let commands = self.plugin_browser.show(ctx, &mut self.state);
        for command in commands {
            if let Err(e) = self.command_manager.execute(command, &mut self.state) {
                self.state
                    .status
                    .error(format!("Plugin browser command failed: {}", e));
            }
        }

        // MIDI editor functionality is now integrated into the piano roll

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

                FileDialog::ImportMidi => {
                    if let Err(e) = self.import_midi_file() {
                        self.state
                            .status
                            .error(format!("Failed to import MIDI file: {}", e));
                    }
                    self.file_dialog = None;
                }

                _ => {
                    self.file_dialog = None;
                }
            }
        }

        // Request continuous repaints while playing
        if self.state.playing {
            ctx.request_repaint();
        }
    }
}
