use crate::core::{
    midi_channel_index, CommandManager, DawCommand, DawState, EditorView, MessageType,
    MidiEngineCommand, MidiEventStore, MidiMessage, Project, RecordingMode,
    RecordingSessionContext, SnapMode, StatusMessage, Track, TrackType,
};
use crate::ui::piano_roll::PianoRoll;
use crate::ui::Timeline;
use eframe::egui;
use eframe::emath::Align;
use egui::Key;
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::time::Duration;
use uuid::Uuid;

const TRANSPORT_ICON_BUTTON_SIZE: egui::Vec2 = egui::Vec2::new(28.0, 28.0);

pub struct SupersawApp {
    state: DawState,
    command_manager: CommandManager,
    midi_output_ports: Vec<(String, usize)>,
    midi_input_ports: Vec<(String, usize)>,
    file_dialog: Option<FileDialog>,
    save_as_name: String,
    save_as_name_needs_focus: bool,
    pending_project_action: Option<ProjectAction>,
    last_bpm_sent: Option<f64>,
    scheduled_through_beat: Option<f64>,
    pending_midi_routes: HashMap<String, Vec<String>>,

    // Views
    timeline: Timeline,
    piano_roll: PianoRoll,
}

#[derive(Clone, Copy)]
enum FileDialog {
    SaveAsName,
    SaveAsDirectory,
    LoadProject,
    ImportMidi,
}

#[derive(Clone, Copy)]
enum ProjectAction {
    New,
    Load,
}

#[derive(Clone, Copy)]
enum ProjectActionDecision {
    Save,
    Discard,
    Cancel,
}

fn recorded_events_to_midi(
    events: &[crate::core::RecordedEvent],
    ppq: u32,
    bpm: f64,
    snap_mode: SnapMode,
    quantize: bool,
    quantize_strength: f32,
    capture_end_seconds: Option<f64>,
) -> crate::core::MidiEventStore {
    let mut midi_data = crate::core::MidiEventStore::new(ppq);
    if events.is_empty() {
        return midi_data;
    }

    let seconds_per_beat = 60.0 / bpm;
    let grid_seconds = quantize
        .then(|| snap_mode.get_division(bpm))
        .filter(|division| *division > 0.0);
    let strength = f64::from(quantize_strength.clamp(0.0, 1.0));
    let mut note_starts: HashMap<(u8, u8), VecDeque<(f64, u8)>> = HashMap::new();

    for recorded_event in events {
        let relative_seconds = recorded_event.timestamp_beats.max(0.0) * seconds_per_beat;

        match &recorded_event.message {
            MidiMessage::NoteOn {
                channel,
                key,
                velocity,
            } if *velocity > 0 => {
                note_starts
                    .entry((*channel, *key))
                    .or_default()
                    .push_back((relative_seconds, *velocity));
            }
            MidiMessage::NoteOff { channel, key, .. }
            | MidiMessage::NoteOn {
                channel,
                key,
                velocity: 0,
            } => {
                let Some(starts) = note_starts.get_mut(&(*channel, *key)) else {
                    continue;
                };
                let Some((raw_start, velocity)) = starts.pop_front() else {
                    continue;
                };

                let start_time = grid_seconds.map_or(raw_start, |grid| {
                    let snapped = (raw_start / grid).round() * grid;
                    raw_start + (snapped - raw_start) * strength
                });
                let duration = (relative_seconds - raw_start).max(1.0 / 1000.0);
                midi_data.add_note(crate::core::Note {
                    id: Uuid::new_v4().to_string(),
                    channel: *channel,
                    key: *key,
                    velocity,
                    start_time,
                    duration,
                    start_tick: midi_data.time_to_tick(start_time),
                    duration_ticks: midi_data.time_to_tick(duration),
                });
            }
            _ => {
                midi_data.add_event(crate::core::MidiEvent {
                    id: Uuid::new_v4().to_string(),
                    time: relative_seconds,
                    tick: midi_data.time_to_tick(relative_seconds),
                    message: recorded_event.message.clone(),
                });
            }
        }
    }

    // A stopped recording can leave keys held. Close those notes at the last
    // captured timestamp so the performance is still editable and replayable.
    let recording_end = capture_end_seconds.unwrap_or_else(|| {
        events
            .last()
            .map(|event| event.timestamp_beats.max(0.0) * seconds_per_beat)
            .unwrap_or_default()
    });
    for ((channel, key), starts) in note_starts {
        for (raw_start, velocity) in starts {
            let start_time = grid_seconds.map_or(raw_start, |grid| {
                let snapped = (raw_start / grid).round() * grid;
                raw_start + (snapped - raw_start) * strength
            });
            let duration = (recording_end - raw_start).max(1.0 / 1000.0);
            midi_data.add_note(crate::core::Note {
                id: Uuid::new_v4().to_string(),
                channel,
                key,
                velocity,
                start_time,
                duration,
                start_tick: midi_data.time_to_tick(start_time),
                duration_ticks: midi_data.time_to_tick(duration),
            });
        }
    }

    midi_data
}

#[derive(Debug)]
struct RecordingPass {
    midi_data: MidiEventStore,
    start_time: f64,
    length: f64,
    completed: bool,
}

fn inclusive_upper_bound(value: f64) -> f64 {
    if value.is_finite() && value >= 0.0 {
        f64::from_bits(value.to_bits().saturating_add(1))
    } else {
        value
    }
}

fn count_in_duration_seconds(bars: u32, bpm: f64) -> Option<f64> {
    (bpm.is_finite() && bpm > 0.0).then_some(f64::from(bars) * 4.0 * 60.0 / bpm)
}

fn count_in_is_complete(elapsed_seconds: f64, bars: u32, bpm: f64) -> bool {
    count_in_duration_seconds(bars, bpm)
        .is_some_and(|duration| elapsed_seconds.max(0.0) >= duration)
}

fn take_completed_count_in_session(state: &mut DawState) -> Option<RecordingSessionContext> {
    if !state.count_in_active
        || !count_in_is_complete(
            state.count_in_elapsed,
            state.count_in_bars,
            state.project.bpm,
        )
    {
        return None;
    }

    state.count_in_active = false;
    state.count_in_start_time = None;
    state.count_in_elapsed = 0.0;

    let track_id = state.recording_track.clone()?;
    let mut session = state.pending_recording_session.take().unwrap_or_else(|| {
        crate::core::commands::recording_session_context(state, &track_id, state.recording_mode)
    });
    // Count-in completion defines only the actual capture origin; editing
    // intent was frozen when the count-in began.
    session.transport_start_seconds = state.current_time;
    Some(session)
}

fn apply_recording_to_target(
    target: &mut MidiEventStore,
    clip_start_seconds: f64,
    session: &RecordingSessionContext,
    source: &MidiEventStore,
) {
    match session.mode {
        RecordingMode::Overdub => {
            target.merge_from(source, session.transport_start_seconds - clip_start_seconds);
        }
        RecordingMode::Replace | RecordingMode::PunchInOut => {
            let source_end = source
                .get_last_event_time()
                .map(inclusive_upper_bound)
                .unwrap_or(1.0 / 1000.0);
            let (absolute_start, absolute_end, replacement) = if let Some((start, end)) = session
                .punch_range
                .filter(|_| session.mode == RecordingMode::PunchInOut)
            {
                let relative_start = (start - session.transport_start_seconds).max(0.0);
                let relative_end = (end - session.transport_start_seconds).max(0.0);
                (
                    start,
                    end,
                    source.extract_range(relative_start, relative_end),
                )
            } else {
                (
                    session.transport_start_seconds,
                    session.transport_start_seconds + source_end,
                    source.clone(),
                )
            };
            target.replace_range_from(
                absolute_start - clip_start_seconds,
                absolute_end - clip_start_seconds,
                &replacement,
            );
        }
    }
}

fn build_recording_passes(
    source: &MidiEventStore,
    session: &RecordingSessionContext,
    ppq: u32,
) -> Vec<RecordingPass> {
    let Some(last_event_time) = source.get_last_event_time() else {
        return Vec::new();
    };
    let Some((loop_start, loop_end)) = session.loop_range else {
        return vec![RecordingPass {
            midi_data: source.clone(),
            start_time: session.transport_start_seconds,
            length: last_event_time.max(1.0 / 1000.0),
            completed: true,
        }];
    };

    let loop_length = loop_end - loop_start;
    if !loop_length.is_finite() || loop_length <= f64::EPSILON {
        return Vec::new();
    }

    let starts_before_loop = session.transport_start_seconds < loop_start;
    let first_position = if starts_before_loop {
        session.transport_start_seconds
    } else {
        loop_start + (session.transport_start_seconds - loop_start).rem_euclid(loop_length)
    };
    let first_span = loop_end - first_position;
    let mut source_start = 0.0;
    let mut pass_index = 0usize;
    let mut passes = Vec::new();

    while source_start <= last_event_time {
        let pass_span = if pass_index == 0 {
            first_span
        } else {
            loop_length
        };
        let source_boundary = source_start + pass_span;
        let completed = last_event_time >= source_boundary;
        let extraction_end = if completed {
            source_boundary
        } else {
            inclusive_upper_bound(last_event_time)
        };
        let extracted = source.extract_range(source_start, extraction_end);

        if extracted.get_events().next().is_some() {
            let mut positioned = MidiEventStore::new(ppq);
            let destination_offset = if pass_index == 0 && !starts_before_loop {
                first_position - loop_start
            } else {
                0.0
            };
            positioned.merge_from(&extracted, destination_offset);
            passes.push(RecordingPass {
                midi_data: positioned,
                start_time: if pass_index == 0 && starts_before_loop {
                    session.transport_start_seconds
                } else {
                    loop_start
                },
                length: if pass_index == 0 && starts_before_loop {
                    first_span
                } else {
                    loop_length
                },
                completed,
            });
        }

        if !completed {
            break;
        }
        source_start = source_boundary;
        pass_index += 1;
    }

    passes
}

fn recording_asset_path(project_path: Option<&PathBuf>, clip_id: &str) -> PathBuf {
    let directory = project_path.map_or_else(
        || std::env::temp_dir().join("hypersaw_recordings"),
        |path| path.join("midi"),
    );
    let _ = std::fs::create_dir_all(&directory);
    directory.join(format!("{clip_id}.mid"))
}

impl SupersawApp {
    fn reset_scheduling_watermark(&mut self) {
        self.scheduled_through_beat = None;
    }

    fn rebuild_playback_schedule(&mut self) {
        self.reset_scheduling_watermark();
        if !self.state.playing {
            return;
        }

        if let Some(engine) = &self.state.midi_engine {
            let current_beat = self.state.current_time * self.state.project.bpm / 60.0;
            engine
                .lock()
                .send_command(MidiEngineCommand::SetPosition(current_beat));
        }
        self.schedule_midi_events();
    }

    fn pause_for_modal_dialog(&mut self) {
        if !self.state.playing {
            return;
        }

        self.state.playing = false;
        self.state.last_update = None;
        if let Some(engine) = &self.state.midi_engine {
            engine.lock().send_command(MidiEngineCommand::Stop);
        }
        self.reset_scheduling_watermark();
        self.state
            .status
            .info("Playback paused while the file picker is open");
    }

    fn save_project(&mut self) -> Result<PathBuf, Box<dyn std::error::Error>> {
        let path = self.state.project.save_current()?;
        self.command_manager.mark_project_saved();
        Ok(path)
    }

    fn open_save_as_dialog(&mut self) {
        self.save_as_name = self.state.project.name.clone();
        self.save_as_name_needs_focus = true;
        self.file_dialog = Some(FileDialog::SaveAsName);
    }

    fn request_project_save(&mut self) -> Result<Option<PathBuf>, Box<dyn std::error::Error>> {
        if self.state.project.project_path.is_some() {
            self.save_project().map(Some)
        } else {
            self.open_save_as_dialog();
            Ok(None)
        }
    }

    fn report_project_saved(&mut self, path: PathBuf) {
        self.state
            .status
            .success(format!("Project saved to {}", path.display()));
        if let Some(action) = self.pending_project_action.take() {
            self.continue_project_action(action);
        }
    }

    fn save_or_open_save_as(&mut self) {
        match self.request_project_save() {
            Ok(Some(path)) => self.report_project_saved(path),
            Ok(None) => {}
            Err(error) => self
                .state
                .status
                .error(format!("Failed to save project: {error}")),
        }
    }

    fn continue_project_action(&mut self, action: ProjectAction) {
        match action {
            ProjectAction::New => {
                self.install_project(Project::new("Untitled".to_string()));
                self.state.status.success("Created new project");
            }
            ProjectAction::Load => {
                self.file_dialog = Some(FileDialog::LoadProject);
            }
        }
    }

    fn request_project_action(&mut self, action: ProjectAction) {
        if self.command_manager.is_project_dirty() {
            self.pending_project_action = Some(action);
        } else {
            self.continue_project_action(action);
        }
    }

    fn install_project(&mut self, mut project: Project) {
        let old_track_ids: Vec<_> = self
            .state
            .project
            .tracks
            .iter()
            .map(|track| track.id.clone())
            .collect();

        if let Some(recording_track) = self.state.recording_track.take() {
            if let Some(coordinator) = &self.state.recording_coordinator {
                coordinator.lock().stop_recording(&recording_track, false);
            }
        }

        if let Some(engine) = &self.state.midi_engine {
            let engine = engine.lock();
            engine.send_command(MidiEngineCommand::Stop);
            for track_id in &old_track_ids {
                engine.send_command(MidiEngineCommand::ClearPortRouting(track_id.clone()));
            }
            engine.send_command(MidiEngineCommand::SetPosition(0.0));
            engine.send_command(MidiEngineCommand::SetTempo(project.bpm));
            engine.send_command(MidiEngineCommand::SetMetronomeEnabled(false));
        }

        if let Some(coordinator) = &self.state.recording_coordinator {
            for track_id in &old_track_ids {
                coordinator
                    .lock()
                    .send_command(crate::core::RecordingCommand::DisarmTrack {
                        track_id: track_id.clone(),
                    });
            }
            coordinator
                .lock()
                .send_command(crate::core::RecordingCommand::SetTempo(project.bpm));
        }

        // Arming and monitoring are runtime state. Do not display persisted
        // values that are no longer active in the coordinator.
        for track in &mut project.tracks {
            track.is_armed = false;
            track.input_monitoring = false;
        }

        self.state.project = project;
        self.state.playing = false;
        self.state.recording = false;
        self.state.current_time = 0.0;
        self.state.last_update = None;
        self.state.selected_track = None;
        self.state.selected_clip = None;
        self.state.current_view = EditorView::Arrangement;
        self.state.loop_enabled = false;
        self.state.loop_start = 0.0;
        self.state.loop_end = 4.0;
        self.state.metronome = false;
        self.state.track_scroll_y = 0.0;
        self.state.count_in_active = false;
        self.state.count_in_start_time = None;
        self.state.count_in_elapsed = 0.0;
        self.state.pending_recording_session = None;

        self.command_manager.clear();
        self.pending_midi_routes.clear();
        self.last_bpm_sent = Some(self.state.project.bpm);
        self.reset_scheduling_watermark();

        let mut timeline = Timeline::default();
        timeline.update_midi_ports(
            self.midi_output_ports
                .iter()
                .map(|(name, _)| name.clone())
                .collect(),
        );
        timeline.update_midi_input_ports(
            self.midi_input_ports
                .iter()
                .map(|(name, _)| name.clone())
                .collect(),
        );
        self.timeline = timeline;
        self.piano_roll = PianoRoll::default();
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
                if self.state.playing {
                    self.schedule_midi_events();
                }
            }
            KeyAction::SaveProject => {
                self.save_or_open_save_as();
            }
            KeyAction::Undo => {
                let had_undo = self.command_manager.can_undo();
                if let Err(e) = self.command_manager.undo(&mut self.state) {
                    eprintln!("Undo failed: {}", e);
                    self.state.status.error(format!("Undo failed: {}", e));
                } else if had_undo {
                    self.rebuild_playback_schedule();
                }
            }
            KeyAction::Redo => {
                let had_redo = self.command_manager.can_redo();
                if let Err(e) = self.command_manager.redo(&mut self.state) {
                    eprintln!("Redo failed: {}", e);
                    self.state.status.error(format!("Redo failed: {}", e));
                } else if had_redo {
                    self.rebuild_playback_schedule();
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

    fn connect_midi_output_port(
        &mut self,
        port_name: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Find the port index
        if let Some((_, port_index)) = self
            .midi_output_ports
            .iter()
            .find(|(name, _)| name == port_name)
        {
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
        timeline.update_midi_ports(
            midi_output_ports
                .iter()
                .map(|(name, _)| name.clone())
                .collect(),
        );
        timeline.update_midi_input_ports(
            midi_input_ports
                .iter()
                .map(|(name, _)| name.clone())
                .collect(),
        );

        let mut app = Self {
            state: DawState::new(),
            midi_output_ports,
            midi_input_ports,
            file_dialog: None,
            save_as_name: "Untitled".to_string(),
            save_as_name_needs_focus: false,
            pending_project_action: None,
            last_bpm_sent: None,
            scheduled_through_beat: None,
            pending_midi_routes: HashMap::new(),
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
        // Keep a time-based horizon so high tempos do not reduce the amount of
        // real time already queued in the engine.
        let lookahead_beats = (self.state.project.bpm / 60.0) * 4.0;

        let schedule_from = self
            .scheduled_through_beat
            .map_or(current_beats, |watermark| watermark.max(current_beats));
        let schedule_to = current_beats + lookahead_beats;

        if schedule_from >= schedule_to {
            return;
        }

        for track in &self.state.project.tracks {
            if track.is_muted {
                continue;
            }

            // Check solo status
            let any_soloed = self.state.project.tracks.iter().any(|t| t.is_soloed);
            if any_soloed && !track.is_soloed {
                continue;
            }

            let TrackType::Midi {
                channel,
                device_name,
                ..
            } = &track.track_type;
            let port_id = device_name.clone().unwrap_or_default();

            for clip in &track.clips {
                let crate::core::Clip::Midi {
                    id: clip_id,
                    start_time,
                    length,
                    midi_data,
                    ..
                } = clip;
                if let Some(take) = track.takes.iter().find(|take| take.clip_id == *clip_id) {
                    let is_active = track.active_take.as_ref() == Some(&take.id);
                    if !is_active || take.is_muted {
                        continue;
                    }
                }
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
                        let mut end_time_seconds = relative_end * 60.0 / self.state.project.bpm;
                        if schedule_to >= clip_end_beats {
                            // MidiEventStore uses a half-open range. Extend the final
                            // window so a note-off exactly at the clip end is included.
                            end_time_seconds += 1.0e-9;
                        }

                        let events_in_range =
                            events.get_events_in_range(start_time_seconds, end_time_seconds);

                        // Schedule each event with the MIDI engine
                        if let Some(engine) = &self.state.midi_engine {
                            for event in events_in_range {
                                let absolute_time_beats =
                                    clip_start_beats + (event.time * self.state.project.bpm / 60.0);

                                if absolute_time_beats >= schedule_from {
                                    let message = event
                                        .message
                                        .clone()
                                        .with_channel(midi_channel_index(*channel));
                                    engine
                                        .lock()
                                        .send_command(MidiEngineCommand::ScheduleEvent {
                                            time_in_beats: absolute_time_beats,
                                            port_id: port_id.clone(),
                                            message,
                                            track_id: track.id.clone(),
                                        });
                                }
                            }
                        }
                    }
                }
            }
        }

        // Update watermark
        self.scheduled_through_beat = Some(schedule_to);
    }

    fn draw_transport(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.set_min_height(32.0);

            // ===== ESSENTIAL CONTROLS =====

            // Play/Pause button
            if ui
                .add_sized(
                    TRANSPORT_ICON_BUTTON_SIZE,
                    egui::Button::new(if self.state.playing { "⏸" } else { "▶" }),
                )
                .on_hover_text(if self.state.playing { "Pause" } else { "Play" })
                .clicked()
            {
                self.handle_key_action(KeyAction::TogglePlay);
            }

            // Return to start
            if ui
                .add_sized(TRANSPORT_ICON_BUTTON_SIZE, egui::Button::new("⏮"))
                .on_hover_text("Return to start")
                .clicked()
            {
                if let Err(error) = self
                    .command_manager
                    .execute(DawCommand::SeekTime { time: 0.0 }, &mut self.state)
                {
                    self.state.status.error(format!("Failed to seek: {error}"));
                }
                self.reset_scheduling_watermark();
            }

            // Recording button with color state
            let rec_button = if self.state.count_in_active {
                ui.add_sized(
                    TRANSPORT_ICON_BUTTON_SIZE,
                    egui::Button::new("⏺").fill(egui::Color32::YELLOW),
                )
            } else if self.state.recording_track.is_some() {
                ui.add_sized(
                    TRANSPORT_ICON_BUTTON_SIZE,
                    egui::Button::new("⏺").fill(egui::Color32::RED),
                )
            } else {
                ui.add_sized(TRANSPORT_ICON_BUTTON_SIZE, egui::Button::new("⏺"))
            };

            if rec_button.on_hover_text("Record MIDI").clicked() {
                if let Some(track_id) = self.state.recording_track.clone() {
                    if let Err(e) = self.command_manager.execute(
                        DawCommand::StopMidiRecording {
                            track_id,
                            create_take: true,
                        },
                        &mut self.state,
                    ) {
                        self.state
                            .status
                            .error(format!("Failed to stop recording: {}", e));
                    }
                } else if let Some(track_id) = self.state.selected_track.clone() {
                    if let Err(e) = self.command_manager.execute(
                        DawCommand::StartMidiRecording {
                            track_id: track_id.clone(),
                            mode: self.state.recording_mode,
                        },
                        &mut self.state,
                    ) {
                        self.state
                            .status
                            .error(format!("Failed to start recording: {}", e));
                    }
                } else {
                    self.state
                        .status
                        .warning("Select a track to record on".to_string());
                }
            }

            // Metronome toggle with visual state
            let metro_btn = if self.state.metronome {
                ui.add_sized(
                    TRANSPORT_ICON_BUTTON_SIZE,
                    egui::Button::new("M").fill(egui::Color32::from_rgb(80, 120, 200)),
                )
            } else {
                ui.add_sized(TRANSPORT_ICON_BUTTON_SIZE, egui::Button::new("M"))
            };
            if metro_btn.on_hover_text("Toggle metronome").clicked() {
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

            ui.separator();

            // BPM display and controls
            ui.label(format!("{:.0}", self.state.project.bpm));
            for (label, delta) in [("−", -1.0), ("+", 1.0)] {
                if ui
                    .add_sized(TRANSPORT_ICON_BUTTON_SIZE, egui::Button::new(label).small())
                    .clicked()
                {
                    let new_bpm = (self.state.project.bpm + delta).clamp(20.0, 400.0);
                    if let Err(e) = self
                        .command_manager
                        .execute(DawCommand::SetBpm { bpm: new_bpm }, &mut self.state)
                    {
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
                if let Some(count_in_duration) =
                    count_in_duration_seconds(self.state.count_in_bars, self.state.project.bpm)
                {
                    let remaining_beats = ((count_in_duration - self.state.count_in_elapsed)
                        .max(0.0)
                        * self.state.project.bpm
                        / 60.0)
                        .ceil() as u32;
                    ui.colored_label(egui::Color32::YELLOW, format!("⏱ {}", remaining_beats));
                }
            }

            ui.separator();

            // Loop toggle
            let loop_btn = if self.state.loop_enabled {
                ui.add_sized(
                    TRANSPORT_ICON_BUTTON_SIZE,
                    egui::Button::new("⟲").fill(egui::Color32::from_rgb(80, 160, 80)),
                )
            } else {
                ui.add_sized(TRANSPORT_ICON_BUTTON_SIZE, egui::Button::new("⟲"))
            };
            if loop_btn.on_hover_text("Toggle loop playback").clicked() {
                self.state.loop_enabled = !self.state.loop_enabled;
            }

            ui.separator();

            // ===== SETTINGS DROPDOWN =====
            egui::menu::menu_custom_button(
                ui,
                egui::Button::new("⚙").min_size(TRANSPORT_ICON_BUTTON_SIZE),
                |ui| {
                    ui.set_min_width(220.0);

                    // Snap Mode
                    ui.horizontal(|ui| {
                        ui.label("Snap:");
                        egui::ComboBox::from_id_salt("snap_settings")
                            .selected_text(self.state.snap_mode.display_name())
                            .show_ui(ui, |ui| {
                                for snap_mode in [
                                    SnapMode::None,
                                    SnapMode::Bar,
                                    SnapMode::Beat,
                                    SnapMode::Halfbeat,
                                    SnapMode::Quarter,
                                    SnapMode::Eighth,
                                    SnapMode::Sixteenth,
                                    SnapMode::Triplet,
                                    SnapMode::SixteenthTriplet,
                                    SnapMode::ThirtySecond,
                                ] {
                                    if ui
                                        .selectable_value(
                                            &mut self.state.snap_mode,
                                            snap_mode,
                                            snap_mode.display_name(),
                                        )
                                        .clicked()
                                    {
                                        let _ = self.command_manager.execute(
                                            DawCommand::SetSnapMode { snap_mode },
                                            &mut self.state,
                                        );
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
                                if ui
                                    .selectable_label(
                                        matches!(self.state.recording_mode, RecordingMode::Overdub),
                                        "Overdub",
                                    )
                                    .clicked()
                                {
                                    self.state.recording_mode = RecordingMode::Overdub;
                                }
                                if ui
                                    .selectable_label(
                                        matches!(self.state.recording_mode, RecordingMode::Replace),
                                        "Replace",
                                    )
                                    .clicked()
                                {
                                    self.state.recording_mode = RecordingMode::Replace;
                                }
                                if ui
                                    .selectable_label(
                                        matches!(
                                            self.state.recording_mode,
                                            RecordingMode::PunchInOut
                                        ),
                                        "Punch In/Out",
                                    )
                                    .clicked()
                                {
                                    self.state.recording_mode = RecordingMode::PunchInOut;
                                }
                            });
                    });

                    // Count-in
                    ui.horizontal(|ui| {
                        ui.label("Count-in:");
                        egui::ComboBox::from_id_salt("countin_settings")
                            .selected_text(if self.state.count_in_bars == 0 {
                                "Off".to_string()
                            } else {
                                format!("{} bars", self.state.count_in_bars)
                            })
                            .show_ui(ui, |ui| {
                                for bars in [0u32, 1, 2, 4] {
                                    let label = if bars == 0 {
                                        "Off".to_string()
                                    } else {
                                        format!("{} bars", bars)
                                    };
                                    if ui
                                        .selectable_label(self.state.count_in_bars == bars, label)
                                        .clicked()
                                    {
                                        let _ = self.command_manager.execute(
                                            DawCommand::SetCountInBars { bars },
                                            &mut self.state,
                                        );
                                    }
                                }
                            });
                    });

                    // Quantize on record
                    let mut quantize_on_record = self
                        .state
                        .recording_coordinator
                        .as_ref()
                        .map(|rc| rc.lock().get_config().quantize_on_record)
                        .unwrap_or(false);
                    if ui
                        .checkbox(&mut quantize_on_record, "Quantize on record")
                        .changed()
                    {
                        if let Some(rc) = &self.state.recording_coordinator {
                            let mut config = rc.lock().get_config();
                            config.quantize_on_record = quantize_on_record;
                            rc.lock().update_config(config);
                        }
                    }

                    ui.separator();

                    // Loop Range
                    ui.menu_button(
                        format!(
                            "Loop: {:.1}s - {:.1}s",
                            self.state.loop_start, self.state.loop_end
                        ),
                        |ui| {
                            if ui.button("Set Start to Playhead").clicked() {
                                self.state.loop_start = self.state.current_time;
                                ui.close_menu();
                            }
                            if ui.button("Set End to Playhead").clicked() {
                                self.state.loop_end = self.state.current_time;
                                ui.close_menu();
                            }
                        },
                    );

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
                                DawCommand::SetPunchPoints {
                                    punch_in: Some(self.state.current_time),
                                    punch_out: self.state.punch_out,
                                },
                                &mut self.state,
                            );
                            ui.close_menu();
                        }
                        if ui.button("Set Punch Out").clicked() {
                            let _ = self.command_manager.execute(
                                DawCommand::SetPunchPoints {
                                    punch_in: self.state.punch_in,
                                    punch_out: Some(self.state.current_time),
                                },
                                &mut self.state,
                            );
                            ui.close_menu();
                        }
                        if ui.button("Clear").clicked() {
                            let _ = self.command_manager.execute(
                                DawCommand::SetPunchPoints {
                                    punch_in: None,
                                    punch_out: None,
                                },
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
                            self.timeline.update_midi_ports(
                                self.midi_output_ports
                                    .iter()
                                    .map(|(name, _)| name.clone())
                                    .collect(),
                            );
                            self.timeline.update_midi_input_ports(
                                self.midi_input_ports
                                    .iter()
                                    .map(|(name, _)| name.clone())
                                    .collect(),
                            );
                            self.state
                                .status
                                .success("MIDI ports refreshed".to_string());
                            ui.close_menu();
                        }
                        ui.separator();
                        if self.midi_input_ports.is_empty() {
                            ui.label("No input ports");
                        } else {
                            for (port_name, port_index) in &self.midi_input_ports {
                                if ui.button(format!("+ {}", port_name)).clicked() {
                                    if let Some(engine) = &self.state.midi_engine {
                                        engine.lock().send_command(
                                            MidiEngineCommand::AddInputPort(
                                                port_name.clone(),
                                                *port_index,
                                            ),
                                        );
                                        self.state
                                            .status
                                            .success(format!("Enabled: {}", port_name));
                                    }
                                    ui.close_menu();
                                }
                            }
                        }
                    });
                },
            );
        });
    }

    fn import_midi_file(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.pause_for_modal_dialog();
        if let Some(file_path) = rfd::FileDialog::new()
            .set_title("Select MIDI File")
            .add_filter("MIDI Files", &["mid", "midi"])
            .pick_file()
        {
            let track_id = self
                .state
                .project
                .create_midi_track_from_file_path(&file_path)?;
            self.command_manager.mark_project_dirty();

            // Select the newly created track
            self.state.selected_track = Some(track_id);

            self.state.status.success(format!(
                "Imported MIDI file: {}",
                file_path.file_name().unwrap_or_default().to_string_lossy()
            ));
        }

        Ok(())
    }

    fn commit_recording(
        &mut self,
        session: RecordingSessionContext,
        events: Vec<crate::core::RecordedEvent>,
    ) {
        if events.is_empty() {
            return;
        }

        let quantize_config = self
            .state
            .recording_coordinator
            .as_ref()
            .map(|coordinator| coordinator.lock().get_config())
            .unwrap_or_default();
        let source = recorded_events_to_midi(
            &events,
            self.state.project.ppq,
            self.state.project.bpm,
            self.state.snap_mode,
            quantize_config.quantize_on_record,
            quantize_config.quantize_strength,
            session
                .punch_range
                .filter(|_| session.mode == RecordingMode::PunchInOut)
                .map(|(_, punch_end)| (punch_end - session.transport_start_seconds).max(0.0)),
        );
        if source.get_events().next().is_none() {
            return;
        }

        let track_id = session.track_id.clone();
        let project_path = self.state.project.project_path.clone();
        let ppq = self.state.project.ppq;
        let Some(track) = self
            .state
            .project
            .tracks
            .iter_mut()
            .find(|track| track.id == track_id)
        else {
            self.state.status.error(format!(
                "Recorded MIDI target track no longer exists: {track_id}"
            ));
            return;
        };

        // A selected target is edited in place for non-loop recording. Loop
        // recording always creates stacked take clips for each captured pass.
        if session.loop_range.is_none() {
            if let Some(target_clip_id) = session.target_clip_id.as_ref() {
                if let Some(target_clip) = track.clips.iter_mut().find(
                    |clip| matches!(clip, crate::core::Clip::Midi { id, .. } if id == target_clip_id),
                ) {
                    if let Err(error) = target_clip.load_midi() {
                        self.state.status.error(format!(
                            "Failed to load the target MIDI clip before recording commit: {error}"
                        ));
                        return;
                    }

                    let crate::core::Clip::Midi {
                        start_time,
                        length,
                        file_path,
                        midi_data,
                        ..
                    } = target_clip;
                    let target = midi_data.get_or_insert_with(|| MidiEventStore::new(ppq));

                    apply_recording_to_target(target, *start_time, &session, &source);

                    if let Some(last_time) = target.get_last_event_time() {
                        *length = length.max(last_time);
                    }
                    let save_result = target.save_to_file(file_path);
                    self.state.selected_clip = Some(target_clip_id.clone());
                    self.command_manager.mark_project_dirty();
                    match save_result {
                        Ok(()) => self.state.status.success(match session.mode {
                            RecordingMode::Overdub => "MIDI overdubbed successfully",
                            RecordingMode::Replace => "MIDI interval replaced successfully",
                            RecordingMode::PunchInOut => "MIDI punch recorded successfully",
                        }),
                        Err(error) => self
                            .state
                            .status
                            .error(format!("Failed to save recorded MIDI: {error}")),
                    }
                    return;
                }
            }
        }

        let passes = build_recording_passes(&source, &session, ppq);
        if passes.is_empty() {
            return;
        }

        let is_loop_recording = session.loop_range.is_some();
        let mut newest_take = None;
        let mut newest_completed_take = None;
        let mut newest_clip = None;
        let mut save_error = None;
        for pass in passes {
            let clip_id = Uuid::new_v4().to_string();
            let file_path = recording_asset_path(project_path.as_ref(), &clip_id);
            if let Err(error) = pass.midi_data.save_to_file(&file_path) {
                save_error.get_or_insert_with(|| error.to_string());
            }

            track.clips.push(crate::core::Clip::Midi {
                id: clip_id.clone(),
                start_time: pass.start_time,
                length: pass.length,
                file_path,
                midi_data: Some(pass.midi_data),
                loaded: true,
                automation_lanes: vec![crate::core::AutomationLane::new(
                    crate::core::AutomationParameter::Velocity,
                )],
            });

            let take = crate::core::Take {
                id: Uuid::new_v4().to_string(),
                track_id: track_id.clone(),
                clip_id: clip_id.clone(),
                name: format!("Take {}", track.takes.len() + 1),
                timestamp: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
                is_muted: false,
            };
            newest_take = Some(take.id.clone());
            if pass.completed {
                newest_completed_take = Some(take.id.clone());
            }
            track.takes.push(take);
            newest_clip = Some(clip_id);
        }

        // Prefer a pass that reached the loop boundary. If the first pass was
        // stopped early, it is still the only useful take and remains active.
        track.active_take = newest_completed_take.or(newest_take);
        self.state.selected_clip = newest_clip;
        self.command_manager.mark_project_dirty();
        if let Some(error) = save_error {
            self.state
                .status
                .error(format!("Failed to save recorded MIDI: {error}"));
        } else {
            self.state.status.success(if is_loop_recording {
                "MIDI loop passes recorded as stacked takes"
            } else {
                "MIDI recorded successfully"
            });
        }
    }
}

enum KeyAction {
    TogglePlay,
    SaveProject,
    Undo,
    Redo,
}

impl eframe::App for SupersawApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.state.update_playhead();

        // Check count-in completion
        if let Some(session) = take_completed_count_in_session(&mut self.state) {
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
                    engine
                        .lock()
                        .send_command(crate::core::MidiEngineCommand::SetMetronomeEnabled(false));
                }
            }

            if let Some(recording_coordinator) = &self.state.recording_coordinator {
                recording_coordinator
                    .lock()
                    .start_recording_session(session);
            }
            self.state.status.info("Recording started after count-in");
        }

        // Update MIDI engine and recording coordinator with tempo changes (only when BPM changes)
        if Some(self.state.project.bpm) != self.last_bpm_sent {
            if let Some(engine) = &self.state.midi_engine {
                let engine = engine.lock();
                engine.send_command(MidiEngineCommand::SetTempo(self.state.project.bpm));
            }

            // Also update recording coordinator
            if let Some(coordinator) = &self.state.recording_coordinator {
                coordinator
                    .lock()
                    .send_command(crate::core::RecordingCommand::SetTempo(
                        self.state.project.bpm,
                    ));
            }

            self.last_bpm_sent = Some(self.state.project.bpm);
        }

        let mut loop_seeked = false;
        if let Some(engine) = &self.state.midi_engine {
            let engine = engine.lock();

            // Process any messages from the MIDI engine
            while let Some(message) = engine.try_recv_message() {
                match message {
                    crate::core::MidiEngineMessage::PositionUpdate(beats) => {
                        // Convert beats to seconds for display
                        let seconds = (beats / self.state.project.bpm) * 60.0;
                        if self.state.playing && !loop_seeked {
                            let loop_length = self.state.loop_end - self.state.loop_start;
                            if self.state.loop_enabled && loop_length > f64::EPSILON {
                                if seconds >= self.state.loop_end {
                                    let loop_time = self.state.loop_start
                                        + (seconds - self.state.loop_start).rem_euclid(loop_length);
                                    let loop_beat = loop_time * self.state.project.bpm / 60.0;
                                    engine.send_command(MidiEngineCommand::SetPosition(loop_beat));
                                    self.state.current_time = loop_time;
                                    loop_seeked = true;
                                } else {
                                    self.state.current_time = seconds;
                                }
                            } else {
                                if self.state.loop_enabled {
                                    self.state.loop_enabled = false;
                                    self.state
                                        .status
                                        .warning("Loop range must have a positive length");
                                }
                                self.state.current_time = seconds;
                            }
                        }
                    }
                    crate::core::MidiEngineMessage::MidiInput(port_id, midi_message, timestamp) => {
                        // Forward to recording coordinator via channel
                        if let Some(sender) = &self.state.midi_input_sender {
                            let _ = sender.try_send((port_id, midi_message, timestamp));
                        }
                    }
                    crate::core::MidiEngineMessage::PortStatusChanged(port_name, true) => {
                        if let Some(track_ids) = self.pending_midi_routes.remove(&port_name) {
                            for track_id in track_ids {
                                if let Some(track) = self
                                    .state
                                    .project
                                    .tracks
                                    .iter_mut()
                                    .find(|track| track.id == track_id)
                                {
                                    let TrackType::Midi { device_name, .. } = &mut track.track_type;
                                    *device_name = Some(port_name.clone());
                                    self.command_manager.mark_project_dirty();
                                }
                                engine.send_command(MidiEngineCommand::SetPortRouting(
                                    track_id,
                                    port_name.clone(),
                                ));
                            }
                            self.state
                                .status
                                .success(format!("Connected to MIDI port: {port_name}"));
                        }
                    }
                    crate::core::MidiEngineMessage::PortConnectionFailed(port_name, error) => {
                        self.pending_midi_routes.remove(&port_name);
                        self.state.status.error(format!(
                            "Failed to connect to MIDI port {port_name}: {error}"
                        ));
                    }
                    _ => {} // Handle other messages as needed
                }
            }
        }

        if loop_seeked {
            self.reset_scheduling_watermark();
        }

        // Drain first so event handling can safely query the coordinator config
        // without attempting to lock the same non-reentrant mutex twice.
        let recording_events: Vec<_> = self
            .state
            .recording_coordinator
            .as_ref()
            .map(|recording_coordinator| {
                let coordinator = recording_coordinator.lock();
                std::iter::from_fn(|| coordinator.try_recv_event()).collect()
            })
            .unwrap_or_default();

        for event in recording_events {
            use crate::core::RecordingEvent;
            match event {
                RecordingEvent::RecordingStarted { track_id, .. } => {
                    self.state
                        .status
                        .info(format!("Recording started on track {}", track_id));
                }
                RecordingEvent::RecordingStopped {
                    track_id,
                    events_recorded,
                } => {
                    self.state.status.info(format!(
                        "Recording stopped on track {}: {} events",
                        track_id, events_recorded
                    ));
                }
                RecordingEvent::EventsRecorded { session, events } => {
                    self.commit_recording(session, events);
                }
                RecordingEvent::BufferOverflow {
                    track_id,
                    dropped_events,
                } => {
                    self.state.status.error(format!(
                        "Recording buffer overflow on track {}: {} events dropped",
                        track_id, dropped_events
                    ));
                }
                RecordingEvent::MonitoringEvent { track_id, message } => {
                    // Forward monitored MIDI to the track's output
                    if let Some(track) = self.state.project.tracks.iter().find(|t| t.id == track_id)
                    {
                        let crate::core::TrackType::Midi {
                            device_name,
                            channel,
                            ..
                        } = &track.track_type;
                        let port_id = device_name.clone().unwrap_or_default();

                        // Send immediately via MIDI engine
                        if let Some(engine) = &self.state.midi_engine {
                            // Adjust channel if needed
                            let msg = message.clone().with_channel(midi_channel_index(*channel));

                            engine
                                .lock()
                                .send_command(MidiEngineCommand::ScheduleEvent {
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

        // Schedule MIDI events if playing (moved to a separate method for clarity)
        if self.state.playing {
            self.schedule_midi_events();
        }

        // Keyboard shortcuts
        // SAVE -  Ctrl + S
        // REDO -  Shift + Ctrl + Z
        // UNDO -  Ctrl + Z
        if self.pending_project_action.is_none() && !ctx.wants_keyboard_input() {
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

                // View switching shortcuts (Bitwig-style)
                if i.modifiers.command || i.modifiers.ctrl {
                    if i.key_pressed(Key::Num1) {
                        // Cmd+1 = Arrangement view
                        self.state.current_view = EditorView::Arrangement;
                    }

                    if i.key_pressed(Key::Num2) {
                        // Cmd+2 = Piano Roll (if clip selected)
                        if let (Some(clip_id), Some(track_id)) =
                            (&self.state.selected_clip, &self.state.selected_track)
                        {
                            self.state.current_view = EditorView::PianoRoll {
                                clip_id: clip_id.clone(),
                                track_id: track_id.clone(),
                                scroll_position: 0.0,
                                vertical_zoom: 1.0,
                            };
                        }
                    }
                }
            });
        }

        egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
            if self.pending_project_action.is_some() {
                ui.disable();
            }
            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("New Project").clicked() {
                        self.request_project_action(ProjectAction::New);
                        ui.close_menu();
                    }
                    if ui.button("Save Project").clicked() {
                        self.save_or_open_save_as();
                        ui.close_menu();
                    }
                    if ui.button("Save Project As...").clicked() {
                        self.open_save_as_dialog();
                        ui.close_menu();
                    }
                    if ui.button("Load Project").clicked() {
                        self.request_project_action(ProjectAction::Load);
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
                    let shortcut_modifier = if cfg!(target_os = "macos") {
                        "⌘"
                    } else {
                        "Ctrl+"
                    };

                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(can_undo, egui::Button::new("Undo"))
                            .clicked()
                        {
                            self.handle_key_action(KeyAction::Undo);
                            ui.close_menu();
                        }
                        ui.label(
                            egui::RichText::new(format!("{shortcut_modifier}Z"))
                                .small()
                                .weak(),
                        );
                    });

                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(can_redo, egui::Button::new("Redo"))
                            .clicked()
                        {
                            self.handle_key_action(KeyAction::Redo);
                            ui.close_menu();
                        }
                        ui.label(
                            egui::RichText::new(format!("{shortcut_modifier}Shift+Z"))
                                .small()
                                .weak(),
                        );
                    });
                });

                ui.menu_button("View", |ui| {
                    let is_arrangement = matches!(self.state.current_view, EditorView::Arrangement);
                    let is_piano_roll =
                        matches!(self.state.current_view, EditorView::PianoRoll { .. });

                    // Arrangement
                    ui.horizontal(|ui| {
                        if ui
                            .add(egui::Button::new("Arrangement").selected(is_arrangement))
                            .clicked()
                        {
                            self.state.current_view = EditorView::Arrangement;
                            ui.close_menu();
                        }
                        ui.label(egui::RichText::new("⌘1").weak());
                    });

                    // Piano Roll - only enabled if clip selected
                    let piano_roll_enabled =
                        self.state.selected_clip.is_some() && self.state.selected_track.is_some();

                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(
                                piano_roll_enabled,
                                egui::Button::new("Piano Roll").selected(is_piano_roll),
                            )
                            .clicked()
                        {
                            if let (Some(clip_id), Some(track_id)) =
                                (&self.state.selected_clip, &self.state.selected_track)
                            {
                                self.state.current_view = EditorView::PianoRoll {
                                    clip_id: clip_id.clone(),
                                    track_id: track_id.clone(),
                                    scroll_position: 0.0,
                                    vertical_zoom: 1.0,
                                };
                                ui.close_menu();
                            }
                        }
                        ui.label(egui::RichText::new("⌘2").weak());
                    });

                    ui.separator();

                    // Close Editor (back to Arrangement)
                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(!is_arrangement, egui::Button::new("Close Editor"))
                            .clicked()
                        {
                            self.state.current_view = EditorView::Arrangement;
                            ui.close_menu();
                        }
                        ui.label(egui::RichText::new("⌘1").weak());
                    });
                });
            });
        });

        let mut project_action_decision = None;
        if self.file_dialog.is_none() {
            if let Some(action) = self.pending_project_action {
                let action_name = match action {
                    ProjectAction::New => "create a new project",
                    ProjectAction::Load => "load another project",
                };
                let modal_response =
                    egui::Modal::new(egui::Id::new("unsaved_changes")).show(ctx, |ui| {
                        ui.heading("Unsaved changes");
                        ui.label(format!("Save your changes before you {action_name}?"));
                        ui.horizontal(|ui| {
                            if ui.button("Save").clicked() {
                                project_action_decision = Some(ProjectActionDecision::Save);
                            }
                            if ui.button("Discard").clicked() {
                                project_action_decision = Some(ProjectActionDecision::Discard);
                            }
                            if ui.button("Cancel").clicked() {
                                project_action_decision = Some(ProjectActionDecision::Cancel);
                            }
                        });
                    });
                if modal_response.should_close() {
                    project_action_decision = Some(ProjectActionDecision::Cancel);
                }
            }
        }

        if let Some(decision) = project_action_decision {
            match decision {
                ProjectActionDecision::Save => self.save_or_open_save_as(),
                ProjectActionDecision::Discard => {
                    if let Some(action) = self.pending_project_action.take() {
                        self.continue_project_action(action);
                    }
                }
                ProjectActionDecision::Cancel => {
                    self.pending_project_action = None;
                }
            }
        }

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
            if self.pending_project_action.is_some() {
                ui.disable();
            }
            self.draw_transport(ui);
        });

        // Update timeline with current MIDI ports
        self.timeline.update_midi_ports(
            self.midi_output_ports
                .iter()
                .map(|(name, _)| name.clone())
                .collect(),
        );

        // Do not run editor-local raw input handlers behind a modal.
        if self.pending_project_action.is_some() {
            egui::CentralPanel::default().show(ctx, |_ui| {});
        } else {
            // Draw the main content area
            egui::CentralPanel::default().show(ctx, |ui| match &self.state.current_view {
                EditorView::Arrangement => {
                    let commands = self.timeline.show(ui, &mut self.state);
                    for command in commands {
                        // Check if this is a command that affects scheduling
                        let affects_transport = matches!(
                            command,
                            DawCommand::SeekTime { .. }
                                | DawCommand::StartPlayback
                                | DawCommand::PausePlayback
                                | DawCommand::StopPlayback
                        );
                        let affects_midi_schedule = command.affects_midi_schedule();

                        match self.command_manager.execute(command, &mut self.state) {
                            Ok(()) if affects_midi_schedule => self.rebuild_playback_schedule(),
                            Ok(()) if affects_transport => {
                                self.reset_scheduling_watermark();
                                if self.state.playing {
                                    self.schedule_midi_events();
                                }
                            }
                            Ok(()) => {}
                            Err(error) => {
                                eprintln!("timeline: Command failed: {error}");
                                self.state.status.error(format!("Command failed: {error}"));
                            }
                        }
                    }

                    if self.timeline.take_playback_schedule_dirty() {
                        self.rebuild_playback_schedule();
                    }

                    // Handle pending MIDI connections from timeline
                    let pending_connections = self.timeline.take_pending_midi_connections();
                    for (track_id, device_name) in pending_connections {
                        let requested_device =
                            (!device_name.is_empty()).then_some(device_name.as_str());
                        let current_device = self
                            .state
                            .project
                            .tracks
                            .iter()
                            .find(|track| track.id == track_id)
                            .and_then(|track| {
                                let TrackType::Midi { device_name, .. } = &track.track_type;
                                device_name.as_deref()
                            });
                        if current_device == requested_device {
                            continue;
                        }

                        if device_name.is_empty() {
                            // Disconnect - remove port from engine
                            if let Some(track) =
                                self.state.project.tracks.iter().find(|t| t.id == track_id)
                            {
                                let TrackType::Midi {
                                    device_name: current_device,
                                    ..
                                } = &track.track_type;
                                if let Some(current) = current_device {
                                    if let Some(engine) = &self.state.midi_engine {
                                        let used_by_another_track =
                                        self.state.project.tracks.iter().any(|other| {
                                            other.id != track_id
                                                && matches!(
                                                    &other.track_type,
                                                    TrackType::Midi { device_name: Some(name), .. }
                                                        if name == current
                                                )
                                        });
                                        let engine = engine.lock();
                                        engine.send_command(MidiEngineCommand::ClearPortRouting(
                                            track_id.clone(),
                                        ));
                                        if !used_by_another_track {
                                            engine.send_command(
                                                MidiEngineCommand::RemoveOutputPort(
                                                    current.clone(),
                                                ),
                                            );
                                        }
                                    }
                                }
                            }

                            self.state
                                .status
                                .info("MIDI output disconnected".to_string());

                            // Update track device name
                            if let Some(track) = self
                                .state
                                .project
                                .tracks
                                .iter_mut()
                                .find(|t| t.id == track_id)
                            {
                                let TrackType::Midi {
                                    device_name: dev_name,
                                    ..
                                } = &mut track.track_type;
                                *dev_name = None;
                                self.command_manager.mark_project_dirty();
                            }
                        } else {
                            // Connect to the port
                            self.pending_midi_routes
                                .entry(device_name.clone())
                                .or_default()
                                .push(track_id.clone());
                            if let Err(e) = self.connect_midi_output_port(&device_name) {
                                self.pending_midi_routes.remove(&device_name);
                                self.state
                                    .status
                                    .error(format!("Failed to connect to MIDI port: {}", e));
                            } else {
                                self.state
                                    .status
                                    .info(format!("Connecting to MIDI port: {device_name}"));
                            }
                        }
                    }
                }
                EditorView::PianoRoll { .. } => {
                    let commands = self.piano_roll.show(ui, &mut self.state);
                    for command in commands {
                        println!("command: {:?}", command);
                        let affects_midi_schedule = command.affects_midi_schedule();

                        match self.command_manager.execute(command, &mut self.state) {
                            Ok(()) if affects_midi_schedule => self.rebuild_playback_schedule(),
                            Ok(()) => {}
                            Err(error) => {
                                eprintln!("piano_roll: Command failed: {error}");
                                self.state.status.error(format!("Command failed: {error}"));
                            }
                        }
                    }
                }
                EditorView::SampleEditor { .. } => {
                    ui.label("Sample Editor (Not Implemented)");
                }
            });
        }

        // MIDI editor functionality is now integrated into the piano roll

        if matches!(self.file_dialog, Some(FileDialog::SaveAsName)) {
            let mut begin_folder_selection = false;
            let mut cancel_save_as = false;
            let modal_response =
                egui::Modal::new(egui::Id::new("save_as_project")).show(ctx, |ui| {
                    ui.heading("Save Project As");
                    ui.label("Project name");
                    let name_response = ui
                        .push_id("save_as_project_name", |ui| {
                            ui.add(
                                egui::TextEdit::singleline(&mut self.save_as_name)
                                    .desired_width(260.0),
                            )
                        })
                        .inner;
                    if self.save_as_name_needs_focus {
                        name_response.request_focus();
                        self.save_as_name_needs_focus = false;
                    }
                    let submit_with_enter = name_response.has_focus()
                        && ui.input(|input| input.key_pressed(egui::Key::Enter));
                    ui.label(
                        "Choose a parent folder; a new project folder will be created inside it.",
                    );
                    ui.horizontal(|ui| {
                        if ui.button("Choose Folder...").clicked() || submit_with_enter {
                            match Project::validate_name(&self.save_as_name) {
                                Ok(name) => {
                                    self.save_as_name = name;
                                    begin_folder_selection = true;
                                }
                                Err(error) => self.state.status.error(error.to_string()),
                            }
                        }
                        if ui.button("Cancel").clicked() {
                            cancel_save_as = true;
                        }
                    });
                });

            if begin_folder_selection {
                self.file_dialog = Some(FileDialog::SaveAsDirectory);
            } else if cancel_save_as || modal_response.should_close() {
                self.file_dialog = None;
                self.pending_project_action = None;
            }
        }

        // Handle file dialogs
        if let Some(dialog_type) = self.file_dialog {
            match dialog_type {
                FileDialog::SaveAsName => {}
                FileDialog::SaveAsDirectory => {
                    self.file_dialog = None;
                    self.pause_for_modal_dialog();
                    if let Some(parent_dir) = rfd::FileDialog::new()
                        .set_title("Choose Parent Folder for New Project")
                        .pick_folder()
                    {
                        match self.state.project.save_as(&parent_dir, &self.save_as_name) {
                            Ok(path) => {
                                self.command_manager.mark_project_saved();
                                self.report_project_saved(path);
                            }
                            Err(error) => {
                                self.state
                                    .status
                                    .error(format!("Failed to save project: {error}"));
                                self.save_as_name_needs_focus = true;
                                self.file_dialog = Some(FileDialog::SaveAsName);
                            }
                        }
                    } else {
                        self.pending_project_action = None;
                    }
                }
                FileDialog::LoadProject => {
                    self.pause_for_modal_dialog();
                    // Use a file dialog to allow the user to select a project file
                    if let Some(file_path) = rfd::FileDialog::new()
                        .set_title("Select Project File")
                        .add_filter("Supersaw Project", &["supersaw"])
                        .pick_file()
                    {
                        println!("Selected project file: {}", file_path.display());

                        match Project::load(&file_path) {
                            Ok(project) => {
                                self.install_project(project);
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

        // Playback requires animation frames. Recording and asynchronous MIDI
        // connection work also need a short poll interval because their events
        // arrive without an egui input event to wake the UI.
        let awaiting_midi_input = self
            .state
            .project
            .tracks
            .iter()
            .any(|track| track.is_armed || track.input_monitoring);
        if self.state.playing || self.state.count_in_active {
            ctx.request_repaint();
        } else if self.state.recording_track.is_some()
            || !self.pending_midi_routes.is_empty()
            || awaiting_midi_input
        {
            ctx.request_repaint_after(Duration::from_millis(16));
        } else if self.state.status.get_message().is_some() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{Note, RecordedEvent};

    fn recorded_event(timestamp_beats: f64, message: MidiMessage) -> RecordedEvent {
        RecordedEvent {
            timestamp_samples: 0,
            timestamp_beats,
            port_id: "input".to_string(),
            message,
        }
    }

    fn note(id: &str, key: u8, start_time: f64, duration: f64) -> Note {
        Note {
            id: id.to_string(),
            channel: 0,
            key,
            velocity: 100,
            start_time,
            duration,
            start_tick: (start_time * 960.0) as u32,
            duration_ticks: (duration * 960.0) as u32,
        }
    }

    fn recorded_note(key: u8, start_beat: f64, end_beat: f64) -> Vec<RecordedEvent> {
        vec![
            recorded_event(
                start_beat,
                MidiMessage::NoteOn {
                    channel: 0,
                    key,
                    velocity: 100,
                },
            ),
            recorded_event(
                end_beat,
                MidiMessage::NoteOff {
                    channel: 0,
                    key,
                    velocity: 0,
                },
            ),
        ]
    }

    #[test]
    fn count_in_completion_uses_elapsed_time_across_a_transport_loop() {
        let mut state = DawState::new();
        state.project.bpm = 120.0;
        state.count_in_bars = 1;
        state.count_in_active = true;
        state.count_in_start_time = Some(3.5);
        state.count_in_elapsed = 2.0;
        state.current_time = 0.25;
        state.recording_track = Some("track".to_string());
        state.pending_recording_session = Some(RecordingSessionContext::new(
            "track".to_string(),
            Some("clip".to_string()),
            RecordingMode::Replace,
            3.5,
            None,
            Some((0.0, 4.0)),
        ));

        let completed = take_completed_count_in_session(&mut state).expect("completed count-in");

        assert!(!state.count_in_active);
        assert_eq!(state.count_in_elapsed, 0.0);
        assert_eq!(completed.mode, RecordingMode::Replace);
        assert_eq!(completed.target_clip_id.as_deref(), Some("clip"));
        assert_eq!(completed.transport_start_seconds, 0.25);
    }

    #[test]
    fn unquantized_recording_builds_notes_for_the_piano_roll() {
        let events = vec![
            recorded_event(
                0.0,
                MidiMessage::NoteOn {
                    channel: 0,
                    key: 60,
                    velocity: 96,
                },
            ),
            recorded_event(
                1.0,
                MidiMessage::NoteOff {
                    channel: 0,
                    key: 60,
                    velocity: 0,
                },
            ),
        ];

        let store = recorded_events_to_midi(&events, 480, 120.0, SnapMode::None, false, 1.0, None);
        let note = store.get_notes().next().expect("recorded note");
        assert_eq!(note.start_time, 0.0);
        assert_eq!(note.duration, 0.5);
        assert_eq!(note.velocity, 96);
    }

    #[test]
    fn quantize_with_snap_none_keeps_recorded_notes() {
        let events = vec![
            recorded_event(
                0.1,
                MidiMessage::NoteOn {
                    channel: 0,
                    key: 64,
                    velocity: 100,
                },
            ),
            recorded_event(
                0.6,
                MidiMessage::NoteOff {
                    channel: 0,
                    key: 64,
                    velocity: 0,
                },
            ),
        ];

        let store = recorded_events_to_midi(&events, 480, 120.0, SnapMode::None, true, 1.0, None);
        assert_eq!(store.get_notes().count(), 1);
    }

    #[test]
    fn recording_workflow_quantizes_to_the_selected_grid() {
        let events = recorded_note(64, 0.42, 0.92);

        let store =
            recorded_events_to_midi(&events, 480, 120.0, SnapMode::Halfbeat, true, 1.0, None);

        let note = store.get_notes().next().expect("quantized note");
        assert_eq!(note.start_time, 0.25);
        assert_eq!(note.duration, 0.25);
    }

    #[test]
    fn overlapping_same_pitch_recording_pairs_note_offs_fifo() {
        let events = vec![
            recorded_event(
                0.0,
                MidiMessage::NoteOn {
                    channel: 0,
                    key: 60,
                    velocity: 80,
                },
            ),
            recorded_event(
                0.25,
                MidiMessage::NoteOn {
                    channel: 0,
                    key: 60,
                    velocity: 100,
                },
            ),
            recorded_event(
                0.5,
                MidiMessage::NoteOff {
                    channel: 0,
                    key: 60,
                    velocity: 0,
                },
            ),
            recorded_event(
                1.0,
                MidiMessage::NoteOff {
                    channel: 0,
                    key: 60,
                    velocity: 0,
                },
            ),
        ];

        let store = recorded_events_to_midi(&events, 480, 60.0, SnapMode::None, false, 1.0, None);
        let mut notes: Vec<_> = store.get_notes().cloned().collect();
        notes.sort_by(|left, right| left.start_time.total_cmp(&right.start_time));
        assert_eq!(notes[0].duration, 0.5);
        assert_eq!(notes[1].duration, 0.75);
    }

    #[test]
    fn recording_conversion_preserves_silence_before_the_first_event() {
        let events = vec![
            recorded_event(
                0.5,
                MidiMessage::NoteOn {
                    channel: 0,
                    key: 60,
                    velocity: 100,
                },
            ),
            recorded_event(
                1.0,
                MidiMessage::NoteOff {
                    channel: 0,
                    key: 60,
                    velocity: 0,
                },
            ),
        ];

        let store = recorded_events_to_midi(&events, 480, 120.0, SnapMode::None, false, 1.0, None);
        let note = store.get_notes().next().unwrap();
        assert_eq!(note.start_time, 0.25);
        assert_eq!(note.duration, 0.25);
    }

    #[test]
    fn held_notes_are_closed_when_recording_stops() {
        let events = vec![recorded_event(
            0.5,
            MidiMessage::NoteOn {
                channel: 0,
                key: 60,
                velocity: 100,
            },
        )];

        let store = recorded_events_to_midi(&events, 480, 120.0, SnapMode::None, false, 1.0, None);
        let note = store.get_notes().next().unwrap();
        assert_eq!(note.start_time, 0.25);
        assert_eq!(note.duration, 1.0 / 1000.0);
    }

    #[test]
    fn loop_passes_split_crossing_notes_and_stack_at_the_loop_start() {
        let mut source = MidiEventStore::new(480);
        source.add_note(crate::core::Note {
            id: "crossing".to_string(),
            channel: 0,
            key: 60,
            velocity: 100,
            start_time: 2.5,
            duration: 1.0,
            start_tick: 2400,
            duration_ticks: 960,
        });
        let session = RecordingSessionContext::new(
            "track".to_string(),
            None,
            RecordingMode::Overdub,
            1.0,
            None,
            Some((0.0, 4.0)),
        );

        let passes = build_recording_passes(&source, &session, 480);

        assert_eq!(passes.len(), 2);
        assert!(passes[0].completed);
        assert!(!passes[1].completed);
        assert_eq!((passes[0].start_time, passes[0].length), (0.0, 4.0));
        let first_note = passes[0].midi_data.get_notes().next().unwrap();
        let second_note = passes[1].midi_data.get_notes().next().unwrap();
        assert_eq!((first_note.start_time, first_note.duration), (3.5, 0.5));
        assert_eq!((second_note.start_time, second_note.duration), (0.0, 0.5));
    }

    #[test]
    fn event_on_loop_boundary_belongs_to_the_next_pass() {
        let mut source = MidiEventStore::new(480);
        source.add_event(crate::core::MidiEvent {
            id: "boundary".to_string(),
            time: 3.0,
            tick: 2880,
            message: MidiMessage::MidiClock,
        });
        let session = RecordingSessionContext::new(
            "track".to_string(),
            None,
            RecordingMode::Overdub,
            1.0,
            None,
            Some((0.0, 4.0)),
        );

        let passes = build_recording_passes(&source, &session, 480);

        assert_eq!(passes.len(), 1);
        let event = passes[0].midi_data.get_events().next().unwrap();
        assert_eq!(event.time, 0.0);
    }

    #[test]
    fn recording_started_before_the_loop_keeps_its_pre_loop_placement() {
        let mut source = MidiEventStore::new(480);
        source.add_note(note("pre-loop", 60, 1.0, 0.5));
        let session = RecordingSessionContext::new(
            "track".to_string(),
            None,
            RecordingMode::Overdub,
            2.0,
            None,
            Some((4.0, 8.0)),
        );

        let passes = build_recording_passes(&source, &session, 480);

        assert_eq!(passes.len(), 1);
        assert_eq!((passes[0].start_time, passes[0].length), (2.0, 6.0));
        assert_eq!(
            passes[0].midi_data.get_notes().next().unwrap().start_time,
            1.0
        );
    }

    #[test]
    fn overdub_uses_the_captured_transport_and_target_clip_offset() {
        let mut target = MidiEventStore::new(480);
        target.add_note(note("existing", 60, 0.0, 0.5));
        let mut source = MidiEventStore::new(480);
        source.add_note(note("recorded", 64, 0.25, 0.5));
        let session = RecordingSessionContext::new(
            "track".to_string(),
            Some("clip".to_string()),
            RecordingMode::Overdub,
            2.0,
            None,
            None,
        );

        apply_recording_to_target(&mut target, 1.0, &session, &source);

        let mut notes: Vec<_> = target.get_notes().collect();
        notes.sort_by(|left, right| left.start_time.total_cmp(&right.start_time));
        assert_eq!(notes.len(), 2);
        assert_eq!(notes[0].start_time, 0.0);
        assert_eq!(notes[1].start_time, 1.25);
    }

    #[test]
    fn recording_workflow_overdub_converts_and_merges_captured_events() {
        let mut target = MidiEventStore::new(480);
        target.add_note(note("existing", 60, 0.0, 0.5));
        let source = recorded_events_to_midi(
            &recorded_note(64, 0.5, 1.0),
            480,
            60.0,
            SnapMode::None,
            false,
            1.0,
            None,
        );
        let session = RecordingSessionContext::new(
            "track".to_string(),
            Some("clip".to_string()),
            RecordingMode::Overdub,
            2.0,
            None,
            None,
        );

        apply_recording_to_target(&mut target, 1.0, &session, &source);

        let mut notes: Vec<_> = target.get_notes().collect();
        notes.sort_by(|left, right| left.start_time.total_cmp(&right.start_time));
        assert_eq!(notes.len(), 2);
        assert_eq!((notes[0].key, notes[0].start_time), (60, 0.0));
        assert_eq!((notes[1].key, notes[1].start_time), (64, 1.5));
    }

    #[test]
    fn recording_workflow_replace_preserves_material_outside_the_capture() {
        let mut target = MidiEventStore::new(480);
        target.add_note(note("crossing", 60, 0.0, 4.0));
        let source = recorded_events_to_midi(
            &recorded_note(72, 0.0, 1.0),
            480,
            60.0,
            SnapMode::None,
            false,
            1.0,
            None,
        );
        let session = RecordingSessionContext::new(
            "track".to_string(),
            Some("clip".to_string()),
            RecordingMode::Replace,
            1.0,
            None,
            None,
        );

        apply_recording_to_target(&mut target, 0.0, &session, &source);

        let mut notes: Vec<_> = target.get_notes().collect();
        notes.sort_by(|left, right| left.start_time.total_cmp(&right.start_time));
        assert_eq!(notes.len(), 3);
        assert_eq!(
            (notes[0].key, notes[0].start_time, notes[0].duration),
            (60, 0.0, 1.0)
        );
        assert_eq!(
            (notes[1].key, notes[1].start_time, notes[1].duration),
            (72, 1.0, 1.0)
        );
        assert_eq!(
            (notes[2].key, notes[2].start_time, notes[2].duration),
            (60, 2.0, 2.0)
        );
    }

    #[test]
    fn recording_workflow_punch_filters_and_replaces_the_punch_interval() {
        let mut target = MidiEventStore::new(480);
        target.add_note(note("crossing", 60, 0.0, 4.0));
        let source = recorded_events_to_midi(
            &recorded_note(72, 1.25, 1.75),
            480,
            60.0,
            SnapMode::None,
            false,
            1.0,
            Some(2.0),
        );
        let session = RecordingSessionContext::new(
            "track".to_string(),
            Some("clip".to_string()),
            RecordingMode::PunchInOut,
            0.0,
            Some((1.0, 2.0)),
            None,
        );

        apply_recording_to_target(&mut target, 0.0, &session, &source);

        let mut notes: Vec<_> = target.get_notes().collect();
        notes.sort_by(|left, right| left.start_time.total_cmp(&right.start_time));
        assert_eq!(notes.len(), 3);
        assert_eq!(
            (notes[0].key, notes[0].start_time, notes[0].duration),
            (60, 0.0, 1.0)
        );
        assert_eq!(
            (notes[1].key, notes[1].start_time, notes[1].duration),
            (72, 1.25, 0.5)
        );
        assert_eq!(
            (notes[2].key, notes[2].start_time, notes[2].duration),
            (60, 2.0, 2.0)
        );
    }

    #[test]
    fn recording_workflow_loop_stacks_passes_from_captured_events() {
        let source = recorded_events_to_midi(
            &recorded_note(60, 2.5, 3.5),
            480,
            60.0,
            SnapMode::None,
            false,
            1.0,
            None,
        );
        let session = RecordingSessionContext::new(
            "track".to_string(),
            None,
            RecordingMode::Overdub,
            1.0,
            None,
            Some((0.0, 4.0)),
        );

        let passes = build_recording_passes(&source, &session, 480);

        assert_eq!(passes.len(), 2);
        assert!(passes[0].completed);
        assert!(!passes[1].completed);
        let first_note = passes[0].midi_data.get_notes().next().unwrap();
        let second_note = passes[1].midi_data.get_notes().next().unwrap();
        assert_eq!((first_note.start_time, first_note.duration), (3.5, 0.5));
        assert_eq!((second_note.start_time, second_note.duration), (0.0, 0.5));
    }

    #[test]
    fn punch_replaces_only_the_captured_interval() {
        let mut target = MidiEventStore::new(480);
        target.add_note(note("crossing", 60, 0.0, 4.0));
        let mut source = MidiEventStore::new(480);
        source.add_note(note("recorded", 72, 1.25, 0.5));
        let session = RecordingSessionContext::new(
            "track".to_string(),
            Some("clip".to_string()),
            RecordingMode::PunchInOut,
            0.0,
            Some((1.0, 2.0)),
            None,
        );

        apply_recording_to_target(&mut target, 0.0, &session, &source);

        let mut notes: Vec<_> = target.get_notes().collect();
        notes.sort_by(|left, right| left.start_time.total_cmp(&right.start_time));
        assert_eq!(notes.len(), 3);
        assert_eq!(
            (notes[0].key, notes[0].start_time, notes[0].duration),
            (60, 0.0, 1.0)
        );
        assert_eq!(
            (notes[1].key, notes[1].start_time, notes[1].duration),
            (72, 1.25, 0.5)
        );
        assert_eq!(
            (notes[2].key, notes[2].start_time, notes[2].duration),
            (60, 2.0, 2.0)
        );
    }

    #[test]
    fn punch_closes_a_held_note_at_the_punch_out_boundary() {
        let events = vec![recorded_event(
            1.0,
            MidiMessage::NoteOn {
                channel: 0,
                key: 60,
                velocity: 100,
            },
        )];

        let store =
            recorded_events_to_midi(&events, 480, 60.0, SnapMode::None, false, 1.0, Some(2.0));
        let note = store.get_notes().next().unwrap();
        assert_eq!(note.start_time, 1.0);
        assert_eq!(note.duration, 1.0);
    }
}
