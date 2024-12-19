use crate::core::{Clip, DawState, Track, TrackType};
use crate::ui::Timeline;
use eframe::egui;
use std::path::PathBuf;
use uuid::Uuid;

pub struct SupersawApp {
    state: DawState,
    midi_output: Option<midir::MidiOutputConnection>,
    midi_ports: Vec<String>,
    file_dialog: Option<FileDialog>,
    timeline: Timeline,
}

enum FileDialog {
    SaveProject,
    LoadProject,
    ImportAudio,
    ImportMidi,
}

impl SupersawApp {
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
        };

        // Add test track
        let test_track = Track {
            id: Uuid::new_v4().to_string(),
            name: "Test Track".to_string(),
            track_type: TrackType::Midi {
                channel: 1,
                device_name: String::new(),
            },
            clips: vec![Clip::Midi {
                id: Uuid::new_v4().to_string(),
                start_time: 0.0,
                length: 4.0,
                file_path: PathBuf::from("test.mid"),
            }],
            is_muted: false,
            is_soloed: false,
        };

        app.state.project.tracks.push(test_track);

        app
    }

    fn draw_transport(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if ui
                .button(if self.state.playing { "⏹" } else { "▶" })
                .clicked()
            {
                self.state.playing = !self.state.playing;
                // TODO: Implement actual transport control
            }

            if ui.button("⏺").clicked() {
                self.state.recording = !self.state.recording;
                // TODO: Implement recording
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
                    ui.checkbox(&mut track.is_muted, "M");
                    ui.checkbox(&mut track.is_soloed, "S");

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
                });
            }

            if ui.button("+ Add Track").clicked() {
                self.show_add_track_menu();
            }
        });
    }

    fn show_add_track_menu(&mut self) {
        // TODO: Implement track creation dialog
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

impl eframe::App for SupersawApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
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
                });

                ui.menu_button("Track", |ui| {
                    if ui.button("Add MIDI Track").clicked() {
                        // TODO: Implement MIDI track creation
                        ui.close_menu();
                    }
                    if ui.button("Add Drum Rack").clicked() {
                        // TODO: Implement drum rack creation
                        ui.close_menu();
                    }
                    if ui.button("Add Audio Track").clicked() {
                        // TODO: Implement audio track creation
                        ui.close_menu();
                    }
                });
            });
        });

        egui::TopBottomPanel::bottom("transport").show(ctx, |ui| {
            self.draw_transport(ui);
        });

        egui::SidePanel::left("tracks")
            .default_width(200.0)
            .show(ctx, |ui| {
                self.draw_track_list(ui);
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            self.timeline.show(ui, &mut self.state);
        });

        // Handle file dialogs
        if let Some(dialog_type) = &self.file_dialog {
            // TODO: Implement actual file dialog
            // For now, just clear the dialog state
            self.file_dialog = None;
        }
    }
}
