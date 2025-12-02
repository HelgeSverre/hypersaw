use crate::core::{
    CommandManager, DawCommand, DawState, EditorView, MessageType, MidiMessage, MidiEngineCommand,
    Project, SnapMode, StatusMessage, Track, TrackType, RecordingMode,
};
use crate::ui::piano_roll::PianoRoll;
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
    midi_output_ports: Vec<(String, usize)>,
    midi_input_ports: Vec<(String, usize)>,
    file_dialog: Option<FileDialog>,
    last_bpm_sent: Option<f64>,
    last_scheduled_beat: f64, // Watermark to prevent duplicate scheduling

    // Views
    timeline: Timeline,
    piano_roll: PianoRoll,
}

enum FileDialog {
    SaveProject,
    LoadProject,
    ImportMidi,
}

impl SupersawApp {
    fn reset_scheduling_watermark(&mut self) {
        self.last_scheduled_beat = 0.0;
    }
    
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
                // Reset watermark on playback state change
                self.reset_scheduling_watermark();
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
    fn scan_midi_output_ports() -> Vec<(String, usize)> {
        crate::core::MidiEngineHandle::scan_midi_output_ports()
    }
    
    fn scan_midi_input_ports() -> Vec<(String, usize)> {
        crate::core::MidiEngineHandle::scan_midi_input_ports()
    }

    fn connect_midi_output_port(&mut self, port_name: &str) -> Result<(), Box<dyn std::error::Error>> {
        // Find the port index
        if let Some((_, port_index)) = self.midi_output_ports.iter().find(|(name, _)| name == port_name) {
            if let Some(engine) = &self.state.midi_engine {
                engine.lock().send_command(MidiEngineCommand::AddOutputPort(
                    port_name.to_string(),
                    *port_index,
                ));
                return Ok(());
            }
        }
        
        Err("MIDI port not found".into())
    }


    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // Set up MIDI ports
        let midi_output_ports = Self::scan_midi_output_ports();
        let midi_input_ports = Self::scan_midi_input_ports();
        let mut timeline = Timeline::default();
        timeline.update_midi_ports(midi_output_ports.iter().map(|(name, _)| name.clone()).collect());
        
        let mut app = Self {
            state: DawState::new(),
            midi_output_ports,
            midi_input_ports,
            file_dialog: None,
            last_bpm_sent: None,
            last_scheduled_beat: 0.0,
            timeline,
            piano_roll: PianoRoll::default(),
            command_manager: CommandManager::default(),
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
    
    fn schedule_midi_events(&mut self) {
        // Get all clips and schedule their events
        let current_beats = (self.state.current_time / 60.0) * self.state.project.bpm;
        let lookahead_beats = 0.1; // Schedule 100ms ahead
        
        // Only schedule events beyond the watermark to prevent duplicates
        if current_beats <= self.last_scheduled_beat {
            return;
        }
        
        let schedule_from = self.last_scheduled_beat.max(current_beats);
        let schedule_to = current_beats + lookahead_beats;
        
        for track in &self.state.project.tracks {
            if track.is_muted {
                continue;
            }
            
            // Check solo status
            let any_soloed = self.state.project.tracks.iter().any(|t| t.is_soloed);
            if any_soloed && !track.is_soloed {
                continue;
            }
            
            if let TrackType::Midi { channel, device_name } = &track.track_type {
                let port_id = device_name.clone().unwrap_or_default();
                
                for clip in &track.clips {
                    match clip {
                        crate::core::Clip::Midi { start_time, length, midi_data, .. } => {
                            // Check if clip is in range
                            let clip_start_beats = (*start_time / 60.0) * self.state.project.bpm;
                            let clip_length_beats = (*length / 60.0) * self.state.project.bpm;
                            let clip_end_beats = clip_start_beats + clip_length_beats;
                            
                            if schedule_to >= clip_start_beats && schedule_from <= clip_end_beats {
                                if let Some(events) = midi_data {
                                    // Get events in the scheduling window (only new events)
                                    let relative_start = (schedule_from - clip_start_beats).max(0.0);
                                    let relative_end = (schedule_to - clip_start_beats).min(clip_length_beats);
                                    
                                    let start_time_seconds = relative_start * 60.0 / self.state.project.bpm;
                                    let end_time_seconds = relative_end * 60.0 / self.state.project.bpm;
                                    
                                    let events_in_range = events.get_events_in_range(start_time_seconds, end_time_seconds);
                                    
                                    // Schedule each event with the MIDI engine
                                    if let Some(engine) = &self.state.midi_engine {
                                        for event in events_in_range {
                                            let absolute_time_beats = clip_start_beats + (event.time * self.state.project.bpm / 60.0);
                                            
                                            // Only schedule if beyond watermark
                                            if absolute_time_beats > self.last_scheduled_beat {
                                                engine.lock().send_command(MidiEngineCommand::ScheduleEvent {
                                                    time_in_beats: absolute_time_beats,
                                                    port_id: port_id.clone(),
                                                    message: event.message.clone(),
                                                    track_id: track.id.clone(),
                                                });
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        
        // Update watermark
        self.last_scheduled_beat = schedule_to;
    }

    fn draw_transport(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.set_min_height(32.0);

            // ===== ESSENTIAL CONTROLS =====

            // Play/Stop button
            if ui
                .button(if self.state.playing { "⏹" } else { "▶" })
                .clicked()
            {
                self.state.playing = !self.state.playing;
                if self.state.playing {
                    self.state.last_update = Some(std::time::Instant::now());
                    if let Some(engine) = &self.state.midi_engine {
                        engine.lock().send_command(MidiEngineCommand::Start);
                    }
                } else {
                    if let Some(engine) = &self.state.midi_engine {
                        engine.lock().send_command(MidiEngineCommand::Stop);
                    }
                }
            }

            // Return to start
            if ui.button("⏮").clicked() {
                self.state.current_time = 0.0;
                if let Some(engine) = &self.state.midi_engine {
                    engine.lock().send_command(MidiEngineCommand::SetPosition(0.0));
                }
            }

            // Recording button with color state
            let rec_button = if self.state.count_in_active {
                ui.add(egui::Button::new("⏺").fill(egui::Color32::YELLOW))
            } else if self.state.recording_track.is_some() {
                ui.add(egui::Button::new("⏺").fill(egui::Color32::RED))
            } else {
                ui.button("⏺")
            };

            if rec_button.clicked() {
                if let Some(track_id) = &self.state.selected_track {
                    if self.state.recording_track.is_some() {
                        if let Err(e) = self.command_manager.execute(
                            DawCommand::StopMidiRecording {
                                track_id: track_id.clone(),
                                create_take: true,
                            },
                            &mut self.state,
                        ) {
                            self.state.status.error(format!("Failed to stop recording: {}", e));
                        }
                    } else {
                        if let Err(e) = self.command_manager.execute(
                            DawCommand::StartMidiRecording {
                                track_id: track_id.clone(),
                                mode: self.state.recording_mode,
                            },
                            &mut self.state,
                        ) {
                            self.state.status.error(format!("Failed to start recording: {}", e));
                        }
                    }
                } else {
                    self.state.status.warning("Select a track to record on".to_string());
                }
            }

            // Metronome toggle with visual state
            let metro_btn = if self.state.metronome {
                ui.add(egui::Button::new("M").fill(egui::Color32::from_rgb(80, 120, 200)))
            } else {
                ui.button("M")
            };
            if metro_btn.clicked() {
                if let Err(e) = self.command_manager.execute(
                    if self.state.metronome { DawCommand::DisableMetronome } else { DawCommand::EnableMetronome },
                    &mut self.state,
                ) {
                    self.state.status.error(format!("Failed to toggle metronome: {}", e));
                }
            }

            ui.separator();

            // BPM display and controls
            ui.label(format!("{:.0}", self.state.project.bpm));
            for (label, delta) in [("−", -1.0), ("+", 1.0)] {
                if ui.small_button(label).clicked() {
                    let new_bpm = (self.state.project.bpm + delta).clamp(20.0, 400.0);
                    if let Err(e) = self.command_manager.execute(
                        DawCommand::SetBpm { bpm: new_bpm },
                        &mut self.state,
                    ) {
                        self.state.status.error(format!("Failed to set BPM: {}", e));
                    }
                }
            }

            ui.separator();

            // Bar:Beat:Tick time display
            let bbt = crate::core::TimeUtils::format_bar_beat_tick(
                self.state.current_time,
                self.state.project.bpm,
                self.state.project.ppq,
            );
            ui.label(egui::RichText::new(bbt).monospace());

            // Count-in status (only when active)
            if self.state.count_in_active {
                if let Some(start_time) = self.state.count_in_start_time {
                    let count_in_duration = (self.state.count_in_bars as f64 * 4.0 * 60.0) / self.state.project.bpm;
                    let elapsed = self.state.current_time - start_time;
                    let remaining_beats = ((count_in_duration - elapsed) * self.state.project.bpm / 60.0).ceil() as u32;
                    ui.colored_label(egui::Color32::YELLOW, format!("⏱ {}", remaining_beats));
                }
            }

            ui.separator();

            // Loop toggle
            let loop_btn = if self.state.loop_enabled {
                ui.add(egui::Button::new("⟲").fill(egui::Color32::from_rgb(80, 160, 80)))
            } else {
                ui.button("⟲")
            };
            if loop_btn.clicked() {
                self.state.loop_enabled = !self.state.loop_enabled;
            }

            ui.separator();

            // ===== SETTINGS DROPDOWN =====
            ui.menu_button("⚙", |ui| {
                ui.set_min_width(220.0);

                // Snap Mode
                ui.horizontal(|ui| {
                    ui.label("Snap:");
                    egui::ComboBox::from_id_salt("snap_settings")
                        .selected_text(self.state.snap_mode.display_name())
                        .show_ui(ui, |ui| {
                            for snap_mode in [
                                SnapMode::None, SnapMode::Bar, SnapMode::Beat,
                                SnapMode::Halfbeat, SnapMode::Quarter, SnapMode::Eighth, SnapMode::Triplet,
                            ] {
                                if ui.selectable_value(&mut self.state.snap_mode, snap_mode, snap_mode.display_name()).clicked() {
                                    let _ = self.command_manager.execute(DawCommand::SetSnapMode { snap_mode }, &mut self.state);
                                }
                            }
                        });
                });

                // Recording Mode
                ui.horizontal(|ui| {
                    ui.label("Record:");
                    egui::ComboBox::from_id_salt("rec_mode_settings")
                        .selected_text(format!("{:?}", self.state.recording_mode))
                        .show_ui(ui, |ui| {
                            use crate::core::RecordingMode;
                            if ui.selectable_label(matches!(self.state.recording_mode, RecordingMode::Overdub), "Overdub").clicked() {
                                self.state.recording_mode = RecordingMode::Overdub;
                            }
                            if ui.selectable_label(matches!(self.state.recording_mode, RecordingMode::Replace), "Replace").clicked() {
                                self.state.recording_mode = RecordingMode::Replace;
                            }
                            if ui.selectable_label(matches!(self.state.recording_mode, RecordingMode::PunchInOut), "Punch In/Out").clicked() {
                                self.state.recording_mode = RecordingMode::PunchInOut;
                            }
                        });
                });

                // Count-in
                ui.horizontal(|ui| {
                    ui.label("Count-in:");
                    egui::ComboBox::from_id_salt("countin_settings")
                        .selected_text(if self.state.count_in_bars == 0 { "Off".to_string() } else { format!("{} bars", self.state.count_in_bars) })
                        .show_ui(ui, |ui| {
                            for bars in [0u32, 1, 2, 4] {
                                let label = if bars == 0 { "Off".to_string() } else { format!("{} bars", bars) };
                                if ui.selectable_label(self.state.count_in_bars == bars, label).clicked() {
                                    let _ = self.command_manager.execute(DawCommand::SetCountInBars { bars }, &mut self.state);
                                }
                            }
                        });
                });

                // Quantize on record
                let mut quantize_on_record = self.state.recording_coordinator.as_ref()
                    .map(|rc| rc.lock().get_config().quantize_on_record)
                    .unwrap_or(false);
                if ui.checkbox(&mut quantize_on_record, "Quantize on record").changed() {
                    if let Some(rc) = &self.state.recording_coordinator {
                        let mut config = rc.lock().get_config();
                        config.quantize_on_record = quantize_on_record;
                        rc.lock().update_config(config);
                    }
                }

                ui.separator();

                // Loop Range
                ui.menu_button(format!("Loop: {:.1}s - {:.1}s", self.state.loop_start, self.state.loop_end), |ui| {
                    if ui.button("Set Start to Playhead").clicked() {
                        self.state.loop_start = self.state.current_time;
                        ui.close_menu();
                    }
                    if ui.button("Set End to Playhead").clicked() {
                        self.state.loop_end = self.state.current_time;
                        ui.close_menu();
                    }
                });

                // Punch In/Out
                let punch_label = match (self.state.punch_in, self.state.punch_out) {
                    (Some(i), Some(o)) => format!("Punch: {:.1}s - {:.1}s", i, o),
                    (Some(i), None) => format!("Punch In: {:.1}s", i),
                    (None, Some(o)) => format!("Punch Out: {:.1}s", o),
                    _ => "Punch: Off".to_string(),
                };
                ui.menu_button(punch_label, |ui| {
                    if ui.button("Set Punch In").clicked() {
                        let _ = self.command_manager.execute(
                            DawCommand::SetPunchPoints { punch_in: Some(self.state.current_time), punch_out: self.state.punch_out },
                            &mut self.state,
                        );
                        ui.close_menu();
                    }
                    if ui.button("Set Punch Out").clicked() {
                        let _ = self.command_manager.execute(
                            DawCommand::SetPunchPoints { punch_in: self.state.punch_in, punch_out: Some(self.state.current_time) },
                            &mut self.state,
                        );
                        ui.close_menu();
                    }
                    if ui.button("Clear").clicked() {
                        let _ = self.command_manager.execute(
                            DawCommand::SetPunchPoints { punch_in: None, punch_out: None },
                            &mut self.state,
                        );
                        ui.close_menu();
                    }
                });

                ui.separator();

                // MIDI Ports
                ui.menu_button("MIDI Ports", |ui| {
                    if ui.button("Refresh Ports").clicked() {
                        self.midi_output_ports = Self::scan_midi_output_ports();
                        self.midi_input_ports = Self::scan_midi_input_ports();
                        self.timeline.update_midi_ports(self.midi_output_ports.iter().map(|(name, _)| name.clone()).collect());
                        self.state.status.success("MIDI ports refreshed".to_string());
                        ui.close_menu();
                    }
                    ui.separator();
                    if self.midi_input_ports.is_empty() {
                        ui.label("No input ports");
                    } else {
                        for (port_name, port_index) in &self.midi_input_ports {
                            if ui.button(format!("+ {}", port_name)).clicked() {
                                if let Some(engine) = &self.state.midi_engine {
                                    engine.lock().send_command(MidiEngineCommand::AddInputPort(port_name.clone(), *port_index));
                                    self.state.status.success(format!("Enabled: {}", port_name));
                                }
                                ui.close_menu();
                            }
                        }
                    }
                });
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
        
        // Check count-in completion
        if self.state.count_in_active {
            if let Some(start_time) = self.state.count_in_start_time {
                let count_in_duration = (self.state.count_in_bars as f64 * 4.0 * 60.0) / self.state.project.bpm;
                let elapsed = self.state.current_time - start_time;
                
                if elapsed >= count_in_duration {
                    // Count-in complete, start actual recording
                    self.state.count_in_active = false;
                    self.state.count_in_start_time = None;
                    
                    // Check if metronome should continue during recording
                    let should_disable_metronome = if let Some(rc) = &self.state.recording_coordinator {
                        let config = rc.lock().get_config();
                        !config.metronome_during_record && !self.state.metronome
                    } else {
                        !self.state.metronome
                    };
                    
                    // Disable metronome if it was only for count-in
                    if should_disable_metronome {
                        if let Some(engine) = &self.state.midi_engine {
                            engine.lock().send_command(crate::core::MidiEngineCommand::SetMetronomeEnabled(false));
                        }
                    }
                    
                    // Start actual recording
                    if let Some(track_id) = &self.state.recording_track.clone() {
                        if let Some(recording_coordinator) = &self.state.recording_coordinator {
                            recording_coordinator.lock().start_recording(
                                track_id.clone(),
                                None,
                                self.state.recording_mode,
                                self.state.punch_in,
                                self.state.punch_out,
                            );
                        }
                        self.state.status.info(format!("Recording started after count-in"));
                    }
                }
            }
        }

        // Update MIDI engine and recording coordinator with tempo changes (only when BPM changes)
        if Some(self.state.project.bpm) != self.last_bpm_sent {
            if let Some(engine) = &self.state.midi_engine {
                let engine = engine.lock();
                engine.send_command(MidiEngineCommand::SetTempo(self.state.project.bpm));
            }
            
            // Also update recording coordinator
            if let Some(coordinator) = &self.state.recording_coordinator {
                coordinator.lock().send_command(crate::core::RecordingCommand::SetTempo(self.state.project.bpm));
            }
            
            self.last_bpm_sent = Some(self.state.project.bpm);
        }
        
        if let Some(engine) = &self.state.midi_engine {
            let engine = engine.lock();
            
            // Process any messages from the MIDI engine
            while let Some(message) = engine.try_recv_message() {
                match message {
                    crate::core::MidiEngineMessage::PositionUpdate(beats) => {
                        // Convert beats to seconds for display
                        let seconds = (beats / self.state.project.bpm) * 60.0;
                        if self.state.playing {
                            self.state.current_time = seconds;
                        }
                    }
                    crate::core::MidiEngineMessage::MidiInput(port_id, midi_message, timestamp) => {
                        // Forward to recording coordinator via channel
                        if let Some(sender) = &self.state.midi_input_sender {
                            let _ = sender.send((port_id, midi_message, timestamp));
                        }
                    }
                    _ => {} // Handle other messages as needed
                }
            }
        }
        
        // Process recording events
        if let Some(recording_coordinator) = &self.state.recording_coordinator {
            let coordinator = recording_coordinator.lock();
            while let Some(event) = coordinator.try_recv_event() {
                use crate::core::RecordingEvent;
                match event {
                    RecordingEvent::RecordingStarted { track_id, .. } => {
                        self.state.status.info(format!("Recording started on track {}", track_id));
                    }
                    RecordingEvent::RecordingStopped { track_id, events_recorded } => {
                        self.state.status.info(format!("Recording stopped on track {}: {} events", track_id, events_recorded));
                    }
                    RecordingEvent::EventsRecorded { track_id, events } => {
                        // Create a MIDI clip from the recorded events
                        if let Some(track) = self.state.project.tracks.iter_mut().find(|t| t.id == track_id) {
                            if events.is_empty() {
                                continue;
                            }
                            
                            // Calculate time bounds
                            let first_timestamp = events.first().map(|e| e.timestamp_beats).unwrap_or(0.0);
                            let last_timestamp = events.last().map(|e| e.timestamp_beats).unwrap_or(first_timestamp);
                            
                            // Convert beats to seconds for clip placement
                            let start_time = (first_timestamp / self.state.project.bpm) * 60.0;
                            let end_time = (last_timestamp / self.state.project.bpm) * 60.0;
                            let length = (end_time - start_time + 1.0).max(1.0); // At least 1 second
                            
                            // Handle Replace mode - remove overlapping clips
                            if self.state.recording_mode == crate::core::RecordingMode::Replace {
                                // Find and remove clips that overlap with the recording range
                                track.clips.retain(|clip| {
                                    match clip {
                                        crate::core::Clip::Midi { start_time: clip_start, length: clip_length, .. } => {
                                            let clip_end = clip_start + clip_length;
                                            // Keep clip if it doesn't overlap
                                            clip_end <= start_time || *clip_start >= end_time
                                        }
                                        _ => true, // Keep audio clips
                                    }
                                });
                            }
                            
                            // Create new MIDI event store
                            let mut midi_data = crate::core::MidiEventStore::new(self.state.project.ppq);
                            
                            // Check if quantization is enabled
                            let quantize_config = self.state.recording_coordinator
                                .as_ref()
                                .map(|rc| rc.lock().get_config())
                                .unwrap_or_default();
                            
                            // Convert recorded events to MIDI events
                            if quantize_config.quantize_on_record {
                                // Group events into note on/off pairs for quantization
                                let mut note_starts: std::collections::HashMap<(u8, u8), (f64, u8)> = std::collections::HashMap::new();
                                let mut quantized_notes = Vec::new();
                                
                                for recorded_event in events {
                                    let relative_time = recorded_event.timestamp_beats - first_timestamp;
                                    
                                    match &recorded_event.message {
                                        crate::core::MidiMessage::NoteOn { channel, key, velocity } => {
                                            note_starts.insert((*channel, *key), (relative_time, *velocity));
                                        }
                                        crate::core::MidiMessage::NoteOff { channel, key, .. } => {
                                            if let Some((start_time, velocity)) = note_starts.remove(&(*channel, *key)) {
                                                // Apply quantization to note start
                                                let grid_interval = match self.state.snap_mode {
                                                    crate::core::SnapMode::None => continue, // Skip quantization
                                                    crate::core::SnapMode::Bar => 4.0,
                                                    crate::core::SnapMode::Beat => 1.0,
                                                    crate::core::SnapMode::Halfbeat => 0.5,
                                                    crate::core::SnapMode::Quarter => 0.25,
                                                    crate::core::SnapMode::Eighth => 0.125,
                                                    crate::core::SnapMode::Sixteenth => 0.0625,
                                                    crate::core::SnapMode::Triplet => 1.0 / 3.0,
                                                    crate::core::SnapMode::SixteenthTriplet => 1.0 / 6.0,
                                                    crate::core::SnapMode::ThirtySecond => 0.03125,
                                                };
                                                
                                                let quantized_start = (start_time / grid_interval).round() * grid_interval;
                                                let duration = relative_time - start_time;
                                                
                                                // Create quantized note
                                                let note = crate::core::Note {
                                                    id: uuid::Uuid::new_v4().to_string(),
                                                    channel: *channel,
                                                    key: *key,
                                                    velocity,
                                                    start_time: quantized_start,
                                                    duration,
                                                    start_tick: midi_data.time_to_tick(quantized_start),
                                                    duration_ticks: midi_data.time_to_tick(duration),
                                                };
                                                quantized_notes.push(note);
                                            }
                                        }
                                        _ => {
                                            // Non-note events pass through unchanged
                                            let event = crate::core::MidiEvent {
                                                id: uuid::Uuid::new_v4().to_string(),
                                                time: relative_time,
                                                tick: (relative_time * self.state.project.ppq as f64) as u32,
                                                message: recorded_event.message,
                                            };
                                            midi_data.add_event(event);
                                        }
                                    }
                                }
                                
                                // Add all quantized notes
                                for note in quantized_notes {
                                    midi_data.add_note(note);
                                }
                            } else {
                                // No quantization - add events as-is
                                for recorded_event in events {
                                    let relative_time = recorded_event.timestamp_beats - first_timestamp;
                                    let event = crate::core::MidiEvent {
                                        id: uuid::Uuid::new_v4().to_string(),
                                        time: relative_time,
                                        tick: (relative_time * self.state.project.ppq as f64) as u32,
                                        message: recorded_event.message,
                                    };
                                    midi_data.add_event(event);
                                }
                            }
                            
                            // Save recorded MIDI to file
                            let clip_id = uuid::Uuid::new_v4().to_string();
                            
                            // Determine file path
                            let file_path = if let Some(project_path) = &self.state.project.project_path {
                                // Save in project's midi directory
                                let midi_dir = project_path.join("midi");
                                std::fs::create_dir_all(&midi_dir).ok();
                                midi_dir.join(format!("{}.mid", clip_id))
                            } else {
                                // No project path, save in temp location
                                let temp_dir = std::env::temp_dir().join("hypersaw_recordings");
                                std::fs::create_dir_all(&temp_dir).ok();
                                temp_dir.join(format!("{}.mid", clip_id))
                            };
                            
                            // Save MIDI data to file
                            if let Err(e) = midi_data.save_to_file(&file_path) {
                                self.state.status.error(format!("Failed to save recorded MIDI: {}", e));
                            }
                            
                            let clip = crate::core::Clip::Midi {
                                id: clip_id.clone(),
                                start_time,
                                length,
                                file_path,
                                midi_data: Some(midi_data),
                                loaded: true,
                                automation_lanes: Vec::new(),
                            };
                            
                            track.clips.push(clip);
                            self.state.selected_clip = Some(clip_id.clone());
                            
                            // Create a take for this recording
                            let take_number = track.takes.len() + 1;
                            let take = crate::core::Take {
                                id: uuid::Uuid::new_v4().to_string(),
                                track_id: track_id.clone(),
                                clip_id: clip_id.clone(),
                                name: format!("Take {}", take_number),
                                timestamp: std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .unwrap()
                                    .as_secs(),
                                is_muted: false,
                            };
                            
                            let take_id = take.id.clone();
                            track.takes.push(take);
                            track.active_take = Some(take_id);
                            
                            let mode_str = match self.state.recording_mode {
                                crate::core::RecordingMode::Replace => "replaced",
                                _ => "recorded",
                            };
                            self.state.status.success(format!("MIDI {} successfully", mode_str));
                        }
                    }
                    RecordingEvent::BufferOverflow { track_id, dropped_events } => {
                        self.state.status.error(format!(
                            "Recording buffer overflow on track {}: {} events dropped",
                            track_id, dropped_events
                        ));
                    }
                    RecordingEvent::MonitoringEvent { track_id, message } => {
                        // Forward monitored MIDI to the track's output
                        if let Some(track) = self.state.project.tracks.iter().find(|t| t.id == track_id) {
                            if let crate::core::TrackType::Midi { device_name, channel } = &track.track_type {
                                let port_id = device_name.clone().unwrap_or_default();
                                
                                // Send immediately via MIDI engine
                                if let Some(engine) = &self.state.midi_engine {
                                    // Adjust channel if needed
                                    let mut msg = message.clone();
                                    match &mut msg {
                                        MidiMessage::NoteOn { channel: ch, .. } |
                                        MidiMessage::NoteOff { channel: ch, .. } |
                                        MidiMessage::ControlChange { channel: ch, .. } => {
                                            *ch = *channel;
                                        }
                                        _ => {}
                                    }
                                    
                                    engine.lock().send_command(MidiEngineCommand::ScheduleEvent {
                                        time_in_beats: 0.0, // Immediate
                                        port_id,
                                        message: msg,
                                        track_id: track_id.clone(),
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }
        
        // Schedule MIDI events if playing (moved to a separate method for clarity)
        if self.state.playing {
            self.schedule_midi_events();
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
                });

                ui.menu_button("Edit", |ui| {
                    let can_undo = self.command_manager.can_undo();
                    let can_redo = self.command_manager.can_redo();
                    
                    if ui.add_enabled(can_undo, egui::Button::new("Undo")).clicked() {
                        self.handle_key_action(KeyAction::Undo);
                        ui.close_menu();
                    }
                    ui.ctx().style_mut(|style| {
                        if let Some(item) = style.text_styles.get_mut(&egui::TextStyle::Button) {
                            *item = egui::FontId::new(12.0, egui::FontFamily::Proportional);
                        }
                    });
                    ui.label("Ctrl+Z");
                    
                    if ui.add_enabled(can_redo, egui::Button::new("Redo")).clicked() {
                        self.handle_key_action(KeyAction::Redo);
                        ui.close_menu();
                    }
                    ui.label("Ctrl+Shift+Z");
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
        self.timeline.update_midi_ports(self.midi_output_ports.iter().map(|(name, _)| name.clone()).collect());

        // Draw the main content area
        egui::CentralPanel::default().show(ctx, |ui| match &self.state.current_view {
            EditorView::Arrangement => {
                let commands = self.timeline.show(ui, &mut self.state);
                for command in commands {
                    // Check if this is a command that affects scheduling
                    let affects_scheduling = matches!(command, 
                        DawCommand::SeekTime { .. } | 
                        DawCommand::StartPlayback | 
                        DawCommand::PausePlayback |
                        DawCommand::StopPlayback
                    );
                    
                    if let Err(e) = self.command_manager.execute(command, &mut self.state) {
                        eprintln!("timeline: Command failed: {}", e);
                        self.state.status.error(format!("Command failed: {}", e));
                    }
                    
                    // Reset scheduling watermark on seek/stop/pause
                    if affects_scheduling {
                        self.reset_scheduling_watermark();
                    }
                }
                
                // Handle pending MIDI connections from timeline
                let pending_connections = self.timeline.take_pending_midi_connections();
                for (track_id, device_name) in pending_connections {
                    if device_name.is_empty() {
                        // Disconnect - remove port from engine
                        if let Some(track) = self.state.project.tracks.iter().find(|t| t.id == track_id) {
                            if let TrackType::Midi { device_name: current_device, .. } = &track.track_type {
                                if let Some(current) = current_device {
                                    if let Some(engine) = &self.state.midi_engine {
                                        engine.lock().send_command(MidiEngineCommand::RemoveOutputPort(current.clone()));
                                    }
                                }
                            }
                        }
                        
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
                        if let Err(e) = self.connect_midi_output_port(&device_name) {
                            self.state
                                .status
                                .error(format!("Failed to connect to MIDI port: {}", e));
                        } else {
                            self.state
                                .status
                                .success(format!("Connected to MIDI port: {}", device_name));
                                
                            // Update track device name and routing
                            if let Some(track) = self.state.project.tracks.iter_mut().find(|t| t.id == track_id) {
                                if let TrackType::Midi { device_name: ref mut dev_name, .. } = &mut track.track_type {
                                    *dev_name = Some(device_name.clone());
                                }
                            }
                            
                            // Set port routing in the engine
                            if let Some(engine) = &self.state.midi_engine {
                                engine.lock().send_command(MidiEngineCommand::SetPortRouting(
                                    track_id.clone(),
                                    device_name,
                                ));
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
            }
        }

        // Request continuous repaints while playing
        if self.state.playing {
            ctx.request_repaint();
        }
    }
}
