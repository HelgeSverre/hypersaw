// src/core/commands.rs
use super::*;
use crate::core::{AutomationParameter, AutomationLane};
use std::path::PathBuf;
use uuid::Uuid;

pub trait Command {
    fn execute(&self, state: &mut DawState) -> Result<(), Box<dyn std::error::Error>>;
    fn undo(&self, state: &mut DawState) -> Result<(), Box<dyn std::error::Error>>;
    fn name(&self) -> &'static str;
}

#[derive(Debug)]
pub enum DawCommand {
    // Editor
    OpenPianoRoll {
        clip_id: String,
        track_id: String,
    },

    // Notes
    MoveNotes {
        clip_id: String,
        note_ids: Vec<EventID>,
        delta_time: f64,
        delta_pitch: i8,
    },

    DeleteNotes {
        clip_id: String,
        note_ids: Vec<EventID>,
        deleted_notes: Option<Vec<Note>>, // Store deleted notes for undo
    },
    UpdateNoteVelocity {
        clip_id: String,
        note_id: EventID,
        velocity: u8,
        old_velocity: Option<u8>, // Store for undo
    },

    ResizeNote {
        clip_id: String,
        note_id: String,
        new_start_time: f64,
        new_duration: f64,
        old_start_time: Option<f64>, // Store for undo
        old_duration: Option<f64>,    // Store for undo
    },
    AddNote {
        clip_id: String,
        start_time: f64,
        duration: f64,
        pitch: u8,
        velocity: u8,
    },

    // Track
    SelectTrack {
        track_id: String,
    },
    AddTrack {
        track_type: TrackType,
        name: String,
    },
    DeleteTrack {
        track_id: String,
    },
    SetTrackMidiChannel {
        track_id: String,
        channel: u8,
    },
    MuteTrack {
        track_id: String,
    },
    UnmuteTrack {
        track_id: String,
    },
    SoloTrack {
        track_id: String,
    },
    UnsoloTrack {
        track_id: String,
    },
    ArmTrack {
        track_id: String,
    },
    UnarmTrack {
        track_id: String,
    },
    SetTrackColor {
        track_id: String,
        color: String,
    },
    ReorderTracks {
        from_index: usize,
        to_index: usize,
    },

    // Clips
    SelectClip {
        clip_id: String,
    },
    DeselectAll,
    AddClip {
        track_id: String,
        start_time: f64,
        length: f64,
        file_path: PathBuf,
    },
    DeleteClip {
        track_id: String,
        clip_id: String,
    },
    MoveClip {
        clip_id: String,
        track_id: String,
        new_start_time: f64,
    },
    ResizeClip {
        clip_id: String,
        new_length: f64,
    },

    // Automation
    AddAutomationLane {
        clip_id: String,
        parameter: AutomationParameter,
    },
    RemoveAutomationLane {
        clip_id: String,
        lane_id: String,
    },
    SetAutomationLaneVisibility {
        clip_id: String,
        lane_id: String,
        visible: bool,
    },
    AddAutomationPoint {
        clip_id: String,
        lane_id: String,
        time: f64,
        value: f64,
    },
    DeleteAutomationPoints {
        clip_id: String,
        points: Vec<(String, String)>, // (lane_id, point_id)
    },
    UpdateAutomationPoint {
        clip_id: String,
        lane_id: String,
        point_id: String,
        time: Option<f64>,
        value: Option<f64>,
    },
    
    // Transport
    EnableMetronome,
    DisableMetronome,
    SetBpm {
        bpm: f64,
    },
    SeekTime {
        time: f64,
    },

    // Playback
    StopPlayback,
    StartPlayback,
    PausePlayback,

    // MIDI Recording
    StartMidiRecording {
        track_id: String,
        mode: RecordingMode,
    },
    StopMidiRecording {
        track_id: String,
        create_take: bool,
    },
    SetRecordingMode {
        mode: RecordingMode,
    },
    ToggleInputMonitoring {
        track_id: String,
    },
    SetPunchPoints {
        punch_in: Option<f64>,
        punch_out: Option<f64>,
    },
    SetCountInBars {
        bars: u32,
    },
    QuantizeNotes {
        clip_id: String,
        note_ids: Vec<EventID>,
        strength: f32, // 0.0 to 1.0
        grid: SnapMode,
    },
    
    // Take management
    CreateTake {
        track_id: String,
        clip_id: String,
        name: String,
    },
    SelectTake {
        track_id: String,
        take_id: String,
    },
    DeleteTake {
        track_id: String,
        take_id: String,
    },
    MuteTake {
        track_id: String,
        take_id: String,
        muted: bool,
    },
    RenameTake {
        track_id: String,
        take_id: String,
        new_name: String,
    },

    // Does nothing, used for testing and such
    NoOp,
    SetSnapMode {
        snap_mode: SnapMode,
    },
}

impl Command for DawCommand {
    fn execute(&self, state: &mut DawState) -> Result<(), Box<dyn std::error::Error>> {
        match self {
            DawCommand::SetSnapMode { snap_mode } => {
                state.snap_mode = *snap_mode;
                Ok(())
            }
            DawCommand::SeekTime { time } => {
                if state.loop_enabled {
                    // If we seeked outside the loop, disable the loop
                    if *time < state.loop_start || *time > state.loop_end {
                        state.loop_enabled = false;
                    }
                }

                state.current_time = *time;
                
                // Inform the engine of the new position
                if let Some(engine) = &state.midi_engine {
                    let beats = crate::core::TimeUtils::seconds_to_beats(*time, state.project.bpm);
                    engine.lock().send_command(crate::core::MidiEngineCommand::SetPosition(beats));
                }
                
                Ok(())
            }
            DawCommand::OpenPianoRoll { clip_id, track_id } => {
                state.selected_clip = Some(clip_id.clone());
                state.current_view = EditorView::PianoRoll {
                    clip_id: clip_id.clone(),
                    track_id: track_id.clone(),
                    scroll_position: 0.0,
                    vertical_zoom: 1.0,
                };
                Ok(())
            }

            DawCommand::SelectClip { clip_id } => {
                state.selected_clip = Some(clip_id.clone());
                Ok(())
            }
            
            DawCommand::DeselectAll => {
                state.selected_clip = None;
                state.selected_track = None;
                Ok(())
            }

            DawCommand::SelectTrack { track_id } => {
                state.selected_track = Some(track_id.clone());
                Ok(())
            }

            DawCommand::SetTrackMidiChannel { track_id, channel } => {
                if let Some(track) = state.project.tracks.iter_mut().find(|t| t.id == *track_id) {
                    if let TrackType::Midi { channel: ch, .. } = &mut track.track_type {
                        *ch = *channel;
                    }
                }
                Ok(())
            }
            
            DawCommand::MuteTrack { track_id } => {
                if let Some(track) = state.project.tracks.iter_mut().find(|t| t.id == *track_id) {
                    track.is_muted = true;
                    // Update MIDI engine
                    if let Some(engine) = &state.midi_engine {
                        engine.lock().send_command(crate::core::MidiEngineCommand::SetTrackMute(
                            track_id.clone(),
                            true
                        ));
                    }
                }
                Ok(())
            }
            
            DawCommand::UnmuteTrack { track_id } => {
                if let Some(track) = state.project.tracks.iter_mut().find(|t| t.id == *track_id) {
                    track.is_muted = false;
                    // Update MIDI engine
                    if let Some(engine) = &state.midi_engine {
                        engine.lock().send_command(crate::core::MidiEngineCommand::SetTrackMute(
                            track_id.clone(),
                            false
                        ));
                    }
                }
                Ok(())
            }
            
            DawCommand::SoloTrack { track_id } => {
                // First, unsolo all tracks
                for track in &mut state.project.tracks {
                    if track.is_soloed {
                        track.is_soloed = false;
                        // Update MIDI engine
                        if let Some(engine) = &state.midi_engine {
                            engine.lock().send_command(crate::core::MidiEngineCommand::SetTrackSolo(
                                track.id.clone(),
                                false
                            ));
                        }
                    }
                }
                // Then solo the specified track
                if let Some(track) = state.project.tracks.iter_mut().find(|t| t.id == *track_id) {
                    track.is_soloed = true;
                    // Update MIDI engine
                    if let Some(engine) = &state.midi_engine {
                        engine.lock().send_command(crate::core::MidiEngineCommand::SetTrackSolo(
                            track_id.clone(),
                            true
                        ));
                    }
                }
                Ok(())
            }
            
            DawCommand::UnsoloTrack { track_id } => {
                if let Some(track) = state.project.tracks.iter_mut().find(|t| t.id == *track_id) {
                    track.is_soloed = false;
                    // Update MIDI engine
                    if let Some(engine) = &state.midi_engine {
                        engine.lock().send_command(crate::core::MidiEngineCommand::SetTrackSolo(
                            track_id.clone(),
                            false
                        ));
                    }
                }
                Ok(())
            }
            
            DawCommand::ArmTrack { track_id } => {
                if let Some(track) = state.project.tracks.iter_mut().find(|t| t.id == *track_id) {
                    track.is_armed = true;
                    track.input_monitoring = true; // Auto-enable monitoring
                    
                    // Arm the track in recording coordinator
                    if let Some(recording_coordinator) = &state.recording_coordinator {
                        let coordinator = recording_coordinator.lock();
                        coordinator.send_command(
                            crate::core::RecordingCommand::ArmTrack {
                                track_id: track_id.clone(),
                                input_port: "default".to_string(), // TODO: Get from track settings
                                channel_filter: None, // TODO: Get from track settings
                            }
                        );
                        // Enable monitoring
                        coordinator.send_command(
                            crate::core::RecordingCommand::SetInputMonitoring {
                                track_id: track_id.clone(),
                                enabled: true,
                            }
                        );
                    }
                }
                Ok(())
            }
            
            DawCommand::UnarmTrack { track_id } => {
                if let Some(track) = state.project.tracks.iter_mut().find(|t| t.id == *track_id) {
                    track.is_armed = false;
                    track.input_monitoring = false; // Also disable monitoring
                    
                    // Disarm the track in recording coordinator
                    if let Some(recording_coordinator) = &state.recording_coordinator {
                        recording_coordinator.lock().send_command(
                            crate::core::RecordingCommand::DisarmTrack {
                                track_id: track_id.clone(),
                            }
                        );
                    }
                }
                Ok(())
            }
            
            DawCommand::SetTrackColor { track_id, color } => {
                if let Some(track) = state.project.tracks.iter_mut().find(|t| t.id == *track_id) {
                    track.color = color.clone();
                }
                Ok(())
            }
            
            DawCommand::ReorderTracks { from_index, to_index } => {
                let len = state.project.tracks.len();
                if *from_index < len && *to_index < len {
                    let track = state.project.tracks.remove(*from_index);
                    state.project.tracks.insert(*to_index, track);
                }
                Ok(())
            }
            
            DawCommand::AddTrack { track_type, name } => {
                let track = Track {
                    id: Uuid::new_v4().to_string(),
                    name: name.clone(),
                    track_type: track_type.clone(),
                    clips: Vec::new(),
                    is_muted: false,
                    is_soloed: false,
                    is_armed: false,
                    input_monitoring: false,
                    color: "#fde047".to_string(), // Default yellow
                    takes: Vec::new(),
                    active_take: None,
                };
                state.project.tracks.push(track);
                Ok(())
            }

            DawCommand::DeleteTrack { track_id } => {
                if let Some(index) = state.project.tracks.iter().position(|t| t.id == *track_id) {
                    state.project.tracks.remove(index);
                    if state.selected_track == Some(track_id.clone()) {
                        state.selected_track = None;
                    }
                }
                Ok(())
            }

            DawCommand::AddClip {
                track_id,
                start_time,
                length,
                file_path,
            } => {
                if let Some(track) = state.project.tracks.iter_mut().find(|t| t.id == *track_id) {
                    // For empty file paths, create an empty MIDI clip (for new clips)
                    let (midi_data, loaded) = if file_path.as_os_str().is_empty() {
                        (Some(MidiEventStore::new(state.project.ppq)), true)
                    } else {
                        (None, false)
                    };

                    let clip = Clip::Midi {
                        id: Uuid::new_v4().to_string(),
                        start_time: *start_time,
                        length: *length,
                        file_path: file_path.clone(),
                        midi_data,
                        loaded,
                        automation_lanes: Vec::new(),
                    };
                    track.clips.push(clip);
                }
                Ok(())
            }

            DawCommand::DeleteClip { track_id, clip_id } => {
                if let Some(track) = state.project.tracks.iter_mut().find(|t| t.id == *track_id) {
                    if let Some(index) = track.clips.iter().position(|c| {
                        let Clip::Midi { id, .. } = c;
                        id == clip_id
                    }) {
                        track.clips.remove(index);
                        if state.selected_clip == Some(clip_id.clone()) {
                            state.selected_clip = None;
                        }
                    }
                }
                Ok(())
            }

            DawCommand::MoveClip {
                clip_id,
                track_id,
                new_start_time,
            } => {
                if let Some(track) = state.project.tracks.iter_mut().find(|t| t.id == *track_id) {
                    if let Some(Clip::Midi { start_time, id, .. }) = track.clips.iter_mut().find(|c| {
                        let Clip::Midi { id, .. } = c;
                        id == clip_id
                    }) {
                        *start_time = *new_start_time;
                    }
                }
                Ok(())
            }

            DawCommand::ResizeClip {
                clip_id,
                new_length,
            } => {
                for track in &mut state.project.tracks {
                    if let Some(Clip::Midi { length, id, .. }) = track.clips.iter_mut().find(|c| {
                        let Clip::Midi { id, .. } = c;
                        id == clip_id
                    }) {
                        *length = *new_length;
                    }
                }
                Ok(())
            }

            // Do nothing.
            DawCommand::NoOp => Ok(()),
            DawCommand::EnableMetronome {} => {
                state.metronome = true;
                // Enable metronome in MIDI engine
                if let Some(engine) = &state.midi_engine {
                    engine.lock().send_command(crate::core::MidiEngineCommand::SetMetronomeEnabled(true));
                }
                state.status.info("Metronome enabled".to_string());
                Ok(())
            }
            DawCommand::DisableMetronome => {
                state.metronome = false;
                // Disable metronome in MIDI engine
                if let Some(engine) = &state.midi_engine {
                    engine.lock().send_command(crate::core::MidiEngineCommand::SetMetronomeEnabled(false));
                }
                state.status.info("Metronome disabled".to_string());
                Ok({})
            }
            DawCommand::SetBpm { bpm } => {
                state.project.bpm = *bpm;
                state.status.info(format!("BPM set to: {}", bpm));
                Ok(())
            }
            DawCommand::StopPlayback => {
                state.playing = false;
                state.current_time = 0.0;
                Ok(())
            }

            DawCommand::StartPlayback => {
                state.playing = true;
                state.last_update = Some(std::time::Instant::now());
                
                // Start MIDI engine playback
                if let Some(engine) = &state.midi_engine {
                    engine.lock().send_command(MidiEngineCommand::Start);
                }

                Ok(())
            }

            DawCommand::PausePlayback => {
                state.playing = false;
                
                // Stop MIDI engine playback
                if let Some(engine) = &state.midi_engine {
                    engine.lock().send_command(MidiEngineCommand::Stop);
                }

                Ok(())
            }

            DawCommand::AddNote {
                clip_id,
                start_time,
                duration,
                pitch,
                velocity,
            } => {
                // Find the clip and add the note
                for track in &mut state.project.tracks {
                    if let Some(Clip::Midi { midi_data, .. }) = track
                        .clips
                        .iter_mut()
                        .find(|c| matches!(c, Clip::Midi { id, .. } if id == clip_id))
                    {
                        if let Some(store) = midi_data {
                            let note = Note {
                                id: Uuid::new_v4().to_string(),
                                channel: 0, // TODO: Get from track settings
                                key: *pitch,
                                velocity: *velocity,
                                start_time: *start_time,
                                duration: *duration,
                                start_tick: store.time_to_tick(*start_time),
                                duration_ticks: store.time_to_tick(*duration),
                            };
                            store.add_note(note);
                        }
                    }
                }
                Ok(())
            }

            DawCommand::DeleteNotes { clip_id, note_ids, .. } => {
                // Find the clip and delete the notes
                for track in &mut state.project.tracks {
                    if let Some(Clip::Midi { midi_data, .. }) = track
                        .clips
                        .iter_mut()
                        .find(|c| matches!(c, Clip::Midi { id, .. } if id == clip_id))
                    {
                        if let Some(store) = midi_data {
                            for note_id in note_ids {
                                store.delete_note(note_id);
                            }
                        }
                    }
                }
                Ok(())
            }

            DawCommand::MoveNotes {
                clip_id,
                note_ids,
                delta_time,
                delta_pitch,
            } => {
                // Find the clip and move the notes
                for track in &mut state.project.tracks {
                    if let Some(Clip::Midi { midi_data, .. }) = track
                        .clips
                        .iter_mut()
                        .find(|c| matches!(c, Clip::Midi { id, .. } if id == clip_id))
                    {
                        if let Some(store) = midi_data {
                            for note_id in note_ids {
                                store.move_note(note_id, *delta_time, *delta_pitch);
                            }
                        }
                    }
                }
                Ok(())
            }

            DawCommand::ResizeNote {
                clip_id,
                note_id,
                new_start_time,
                new_duration,
                ..
            } => {
                // Find the clip and resize the note
                for track in &mut state.project.tracks {
                    if let Some(Clip::Midi { midi_data, .. }) = track
                        .clips
                        .iter_mut()
                        .find(|c| matches!(c, Clip::Midi { id, .. } if id == clip_id))
                    {
                        if let Some(store) = midi_data {
                            store.update_note(note_id, *new_start_time, *new_duration);
                        }
                    }
                }
                Ok(())
            }
            
            DawCommand::UpdateNoteVelocity { clip_id, note_id, velocity, .. } => {
                // Find the clip and update note velocity
                for track in &mut state.project.tracks {
                    if let Some(Clip::Midi { midi_data, .. }) = track
                        .clips
                        .iter_mut()
                        .find(|c| matches!(c, Clip::Midi { id, .. } if id == clip_id))
                    {
                        if let Some(store) = midi_data {
                            store.update_note_velocity(note_id, *velocity);
                        }
                    }
                }
                Ok(())
            }
            
            // Automation commands
            DawCommand::AddAutomationLane { clip_id, parameter } => {
                for track in &mut state.project.tracks {
                    if let Some(Clip::Midi { automation_lanes, .. }) = track
                        .clips
                        .iter_mut()
                        .find(|c| matches!(c, Clip::Midi { id, .. } if id == clip_id))
                    {
                        let mut lane = AutomationLane::new(parameter.clone());
                        lane.visible = true;
                        automation_lanes.push(lane);
                    }
                }
                Ok(())
            }
            
            DawCommand::RemoveAutomationLane { clip_id, lane_id } => {
                for track in &mut state.project.tracks {
                    if let Some(Clip::Midi { automation_lanes, .. }) = track
                        .clips
                        .iter_mut()
                        .find(|c| matches!(c, Clip::Midi { id, .. } if id == clip_id))
                    {
                        automation_lanes.retain(|lane| lane.id != *lane_id);
                    }
                }
                Ok(())
            }
            
            DawCommand::SetAutomationLaneVisibility { clip_id, lane_id, visible } => {
                for track in &mut state.project.tracks {
                    if let Some(Clip::Midi { automation_lanes, .. }) = track
                        .clips
                        .iter_mut()
                        .find(|c| matches!(c, Clip::Midi { id, .. } if id == clip_id))
                    {
                        if let Some(lane) = automation_lanes.iter_mut().find(|l| l.id == *lane_id) {
                            lane.visible = *visible;
                        }
                    }
                }
                Ok(())
            }
            
            DawCommand::AddAutomationPoint { clip_id, lane_id, time, value } => {
                for track in &mut state.project.tracks {
                    if let Some(Clip::Midi { automation_lanes, .. }) = track
                        .clips
                        .iter_mut()
                        .find(|c| matches!(c, Clip::Midi { id, .. } if id == clip_id))
                    {
                        if let Some(lane) = automation_lanes.iter_mut().find(|l| l.id == *lane_id) {
                            lane.add_point(*time, *value);
                        }
                    }
                }
                Ok(())
            }
            
            DawCommand::DeleteAutomationPoints { clip_id, points } => {
                for track in &mut state.project.tracks {
                    if let Some(Clip::Midi { automation_lanes, .. }) = track
                        .clips
                        .iter_mut()
                        .find(|c| matches!(c, Clip::Midi { id, .. } if id == clip_id))
                    {
                        for (lane_id, point_id) in points {
                            if let Some(lane) = automation_lanes.iter_mut().find(|l| &l.id == lane_id) {
                                lane.remove_point(point_id);
                            }
                        }
                    }
                }
                Ok(())
            }
            
            DawCommand::UpdateAutomationPoint { clip_id, lane_id, point_id, time, value } => {
                for track in &mut state.project.tracks {
                    if let Some(Clip::Midi { automation_lanes, .. }) = track
                        .clips
                        .iter_mut()
                        .find(|c| matches!(c, Clip::Midi { id, .. } if id == clip_id))
                    {
                        if let Some(lane) = automation_lanes.iter_mut().find(|l| l.id == *lane_id) {
                            lane.update_point(point_id, *time, *value);
                        }
                    }
                }
                Ok(())
            }
            
            // MIDI Recording commands
            DawCommand::StartMidiRecording { track_id, mode } => {
                if let Some(track) = state.project.tracks.iter_mut().find(|t| t.id == *track_id) {
                    // Arm the track if not already armed
                    if !track.is_armed {
                        track.is_armed = true;
                        
                        // Arm the track in recording coordinator
                        if let Some(recording_coordinator) = &state.recording_coordinator {
                            recording_coordinator.lock().send_command(
                                crate::core::RecordingCommand::ArmTrack {
                                    track_id: track_id.clone(),
                                    input_port: "default".to_string(), // TODO: Get from track settings
                                    channel_filter: None, // TODO: Get from track settings
                                }
                            );
                        }
                    }
                    
                    // Check if count-in is enabled
                    if state.count_in_bars > 0 && !state.playing {
                        // Start count-in
                        state.count_in_active = true;
                        state.count_in_start_time = Some(state.current_time);
                        
                        // Enable metronome during count-in
                        if let Some(engine) = &state.midi_engine {
                            engine.lock().send_command(crate::core::MidiEngineCommand::SetMetronomeEnabled(true));
                        }
                        
                        // Start playback for count-in
                        state.playing = true;
                        state.last_update = Some(std::time::Instant::now());
                        if let Some(engine) = &state.midi_engine {
                            engine.lock().send_command(crate::core::MidiEngineCommand::Start);
                        }
                        
                        state.status.info(format!("Count-in: {} bars", state.count_in_bars));
                    } else {
                        // Start recording immediately
                        if let Some(recording_coordinator) = &state.recording_coordinator {
                            recording_coordinator.lock().start_recording(
                                track_id.clone(),
                                None, // No specific clip yet
                                *mode,
                                state.punch_in,
                                state.punch_out,
                            );
                        }
                        
                        state.status.info(format!("Started MIDI recording on track: {}", track.name));
                    }
                    
                    state.recording_track = Some(track_id.clone());
                    state.recording_mode = *mode;
                }
                Ok(())
            }
            
            DawCommand::StopMidiRecording { track_id, create_take } => {
                // Check if we're in count-in phase
                if state.count_in_active {
                    // Cancel count-in
                    state.count_in_active = false;
                    state.count_in_start_time = None;
                    
                    // Disable metronome if it was only for count-in
                    if !state.metronome {
                        if let Some(engine) = &state.midi_engine {
                            engine.lock().send_command(crate::core::MidiEngineCommand::SetMetronomeEnabled(false));
                        }
                    }
                    
                    state.status.info("Count-in cancelled".to_string());
                } else {
                    // Normal recording stop
                    if let Some(recording_coordinator) = &state.recording_coordinator {
                        recording_coordinator.lock().stop_recording(track_id, *create_take);
                    }
                    
                    state.status.info("Stopped MIDI recording".to_string());
                }
                
                if state.recording_track == Some(track_id.clone()) {
                    state.recording_track = None;
                }
                Ok(())
            }
            
            DawCommand::SetRecordingMode { mode } => {
                state.recording_mode = *mode;
                state.status.info(format!("Recording mode: {:?}", mode));
                Ok(())
            }
            
            DawCommand::ToggleInputMonitoring { track_id } => {
                if let Some(track) = state.project.tracks.iter_mut().find(|t| t.id == *track_id) {
                    track.input_monitoring = !track.input_monitoring;
                    
                    // Update monitoring in recording system
                    if let Some(recording_coordinator) = &state.recording_coordinator {
                        recording_coordinator.lock().set_input_monitoring(track_id, track.input_monitoring);
                    }
                    
                    let status = if track.input_monitoring { "enabled" } else { "disabled" };
                    state.status.info(format!("Input monitoring {} for track: {}", status, track.name));
                }
                Ok(())
            }
            
            DawCommand::SetPunchPoints { punch_in, punch_out } => {
                state.punch_in = *punch_in;
                state.punch_out = *punch_out;
                
                let msg = match (punch_in, punch_out) {
                    (Some(i), Some(o)) => format!("Punch in: {:.2}, Punch out: {:.2}", i, o),
                    (Some(i), None) => format!("Punch in: {:.2}", i),
                    (None, Some(o)) => format!("Punch out: {:.2}", o),
                    (None, None) => "Punch points cleared".to_string(),
                };
                state.status.info(msg);
                Ok(())
            }
            
            DawCommand::SetCountInBars { bars } => {
                state.count_in_bars = *bars;
                state.status.info(format!("Count-in: {} bars", bars));
                Ok(())
            }
            
            DawCommand::QuantizeNotes { clip_id, note_ids, strength, grid } => {
                // Find the clip and quantize the notes
                for track in &mut state.project.tracks {
                    if let Some(Clip::Midi { midi_data, .. }) = track
                        .clips
                        .iter_mut()
                        .find(|c| matches!(c, Clip::Midi { id, .. } if id == clip_id))
                    {
                        if let Some(store) = midi_data {
                            // Get the grid interval in seconds
                            let grid_interval = match grid {
                                SnapMode::None => return Ok(()), // No quantization
                                SnapMode::Bar => 4.0 * 60.0 / state.project.bpm,
                                SnapMode::Beat => 60.0 / state.project.bpm,
                                SnapMode::Halfbeat => 30.0 / state.project.bpm,
                                SnapMode::Quarter => 15.0 / state.project.bpm,
                                SnapMode::Eighth => 7.5 / state.project.bpm,
                                SnapMode::Sixteenth => 3.75 / state.project.bpm,
                                SnapMode::Triplet => 20.0 / state.project.bpm,
                                SnapMode::SixteenthTriplet => 10.0 / state.project.bpm,
                                SnapMode::ThirtySecond => 1.875 / state.project.bpm,
                            };
                            
                            // Quantize each note
                            let mut updates = Vec::new();
                            
                            for note_id in note_ids {
                                if let Some(note) = store.get_note_mut(note_id) {
                                    // Calculate the nearest grid position
                                    let nearest_grid = (note.start_time / grid_interval).round() * grid_interval;
                                    
                                    // Apply quantization with strength
                                    let quantized_time = note.start_time + (nearest_grid - note.start_time) * *strength as f64;
                                    
                                    // Store the update for later
                                    updates.push((note_id.clone(), quantized_time));
                                }
                            }
                            
                            // Apply the updates
                            for (note_id, quantized_time) in updates {
                                let quantized_tick = store.time_to_tick(quantized_time);
                                if let Some(note) = store.get_note_mut(&note_id) {
                                    note.start_time = quantized_time;
                                    note.start_tick = quantized_tick;
                                }
                            }
                            
                            store.rebuild_note_maps();
                        }
                    }
                }
                
                state.status.info(format!("Quantized {} notes", note_ids.len()));
                Ok(())
            }
            
            // Take management commands
            DawCommand::CreateTake { track_id, clip_id, name } => {
                use std::time::{SystemTime, UNIX_EPOCH};
                
                if let Some(track) = state.project.tracks.iter_mut().find(|t| t.id == *track_id) {
                    let take = crate::core::Take {
                        id: Uuid::new_v4().to_string(),
                        track_id: track_id.clone(),
                        clip_id: clip_id.clone(),
                        name: name.clone(),
                        timestamp: SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .unwrap()
                            .as_secs(),
                        is_muted: false,
                    };
                    
                    let take_id = take.id.clone();
                    track.takes.push(take);
                    track.active_take = Some(take_id);
                    
                    state.status.info(format!("Created take: {}", name));
                }
                Ok(())
            }
            
            DawCommand::SelectTake { track_id, take_id } => {
                if let Some(track) = state.project.tracks.iter_mut().find(|t| t.id == *track_id) {
                    if track.takes.iter().any(|t| t.id == *take_id) {
                        track.active_take = Some(take_id.clone());
                        state.status.info(format!("Selected take: {}", take_id));
                    }
                }
                Ok(())
            }
            
            DawCommand::DeleteTake { track_id, take_id } => {
                if let Some(track) = state.project.tracks.iter_mut().find(|t| t.id == *track_id) {
                    let take_name = track.takes.iter()
                        .find(|t| t.id == *take_id)
                        .map(|t| t.name.clone())
                        .unwrap_or_default();
                    
                    track.takes.retain(|t| t.id != *take_id);
                    
                    // If this was the active take, clear it
                    if track.active_take == Some(take_id.clone()) {
                        track.active_take = None;
                    }
                    
                    state.status.info(format!("Deleted take: {}", take_name));
                }
                Ok(())
            }
            
            DawCommand::MuteTake { track_id, take_id, muted } => {
                if let Some(track) = state.project.tracks.iter_mut().find(|t| t.id == *track_id) {
                    if let Some(take) = track.takes.iter_mut().find(|t| t.id == *take_id) {
                        take.is_muted = *muted;
                        let status = if *muted { "Muted" } else { "Unmuted" };
                        state.status.info(format!("{} take: {}", status, take.name));
                    }
                }
                Ok(())
            }
            
            DawCommand::RenameTake { track_id, take_id, new_name } => {
                if let Some(track) = state.project.tracks.iter_mut().find(|t| t.id == *track_id) {
                    if let Some(take) = track.takes.iter_mut().find(|t| t.id == *take_id) {
                        take.name = new_name.clone();
                        state.status.info(format!("Renamed take to: {}", new_name));
                    }
                }
                Ok(())
            }
        }
    }

    fn undo(&self, state: &mut DawState) -> Result<(), Box<dyn std::error::Error>> {
        match self {
            // Note operations
            DawCommand::AddNote { clip_id, start_time, duration, pitch, velocity } => {
                // Find the note we just added and delete it
                for track in &mut state.project.tracks {
                    if let Some(Clip::Midi { midi_data, .. }) = track.clips.iter_mut()
                        .find(|c| matches!(c, Clip::Midi { id, .. } if id == clip_id))
                    {
                        if let Some(store) = midi_data {
                            // Find the most recently added note matching our parameters
                            let note_id_to_delete = {
                                let notes: Vec<_> = store.get_notes().collect();
                                notes.iter().rev().find(|n| 
                                    n.key == *pitch && 
                                    n.velocity == *velocity &&
                                    (n.start_time - start_time).abs() < 0.001 &&
                                    (n.duration - duration).abs() < 0.001
                                ).map(|n| n.id.clone())
                            };
                            
                            if let Some(note_id) = note_id_to_delete {
                                store.delete_note(&note_id);
                            }
                        }
                    }
                }
                Ok(())
            }
            
            DawCommand::DeleteNotes { clip_id, deleted_notes, .. } => {
                // Restore the deleted notes
                if let Some(notes) = deleted_notes {
                    for track in &mut state.project.tracks {
                        if let Some(Clip::Midi { midi_data, .. }) = track.clips.iter_mut()
                            .find(|c| matches!(c, Clip::Midi { id, .. } if id == clip_id))
                        {
                            if let Some(store) = midi_data {
                                for note in notes {
                                    store.add_note(note.clone());
                                }
                            }
                        }
                    }
                } else {
                    state.status.error("Cannot undo: note data not available".to_string());
                }
                Ok(())
            }
            
            DawCommand::MoveNotes { clip_id, note_ids, delta_time, delta_pitch } => {
                // Move the notes back
                for track in &mut state.project.tracks {
                    if let Some(Clip::Midi { midi_data, .. }) = track.clips.iter_mut()
                        .find(|c| matches!(c, Clip::Midi { id, .. } if id == clip_id))
                    {
                        if let Some(store) = midi_data {
                            for note_id in note_ids {
                                store.move_note(note_id, -delta_time, -delta_pitch);
                            }
                        }
                    }
                }
                Ok(())
            }
            
            DawCommand::ResizeNote { clip_id, note_id, old_start_time, old_duration, .. } => {
                // Restore the original size
                if let (Some(start), Some(duration)) = (old_start_time, old_duration) {
                    for track in &mut state.project.tracks {
                        if let Some(Clip::Midi { midi_data, .. }) = track.clips.iter_mut()
                            .find(|c| matches!(c, Clip::Midi { id, .. } if id == clip_id))
                        {
                            if let Some(store) = midi_data {
                                store.update_note(note_id, *start, *duration);
                            }
                        }
                    }
                } else {
                    state.status.error("Cannot undo: original size not available".to_string());
                }
                Ok(())
            }
            
            DawCommand::UpdateNoteVelocity { clip_id, note_id, old_velocity, .. } => {
                // Restore the original velocity
                if let Some(velocity) = old_velocity {
                    for track in &mut state.project.tracks {
                        if let Some(Clip::Midi { midi_data, .. }) = track.clips.iter_mut()
                            .find(|c| matches!(c, Clip::Midi { id, .. } if id == clip_id))
                        {
                            if let Some(store) = midi_data {
                                store.update_note_velocity(note_id, *velocity);
                            }
                        }
                    }
                } else {
                    state.status.error("Cannot undo: original velocity not available".to_string());
                }
                Ok(())
            }
            
            // Track operations
            DawCommand::MuteTrack { track_id } => {
                // Unmute the track
                if let Some(track) = state.project.tracks.iter_mut().find(|t| t.id == *track_id) {
                    track.is_muted = false;
                }
                Ok(())
            }
            
            DawCommand::UnmuteTrack { track_id } => {
                // Mute the track
                if let Some(track) = state.project.tracks.iter_mut().find(|t| t.id == *track_id) {
                    track.is_muted = true;
                }
                Ok(())
            }
            
            DawCommand::SoloTrack { track_id } => {
                // Unsolo the track
                if let Some(track) = state.project.tracks.iter_mut().find(|t| t.id == *track_id) {
                    track.is_soloed = false;
                }
                Ok(())
            }
            
            DawCommand::UnsoloTrack { track_id } => {
                // Solo the track
                if let Some(track) = state.project.tracks.iter_mut().find(|t| t.id == *track_id) {
                    track.is_soloed = true;
                }
                Ok(())
            }
            
            DawCommand::ArmTrack { track_id } => {
                // Unarm the track
                if let Some(track) = state.project.tracks.iter_mut().find(|t| t.id == *track_id) {
                    track.is_armed = false;
                    
                    // Also unarm in recording coordinator
                    if let Some(recording_coordinator) = &state.recording_coordinator {
                        recording_coordinator.lock().send_command(
                            RecordingCommand::DisarmTrack {
                                track_id: track_id.clone(),
                            }
                        );
                    }
                }
                Ok(())
            }
            
            DawCommand::UnarmTrack { track_id } => {
                // Arm the track
                if let Some(track) = state.project.tracks.iter_mut().find(|t| t.id == *track_id) {
                    track.is_armed = true;
                    
                    // Also arm in recording coordinator
                    if let Some(recording_coordinator) = &state.recording_coordinator {
                        recording_coordinator.lock().send_command(
                            RecordingCommand::ArmTrack {
                                track_id: track_id.clone(),
                                input_port: "default".to_string(),
                                channel_filter: None,
                            }
                        );
                    }
                }
                Ok(())
            }
            
            DawCommand::SetTrackColor { track_id, color } => {
                // We can't restore the original color without storing it
                state.status.error("Undo for SetTrackColor not yet implemented".to_string());
                Ok(())
            }
            
            // Playback operations
            DawCommand::StartPlayback => {
                // Stop playback
                state.playing = false;
                state.last_update = None;
                if let Some(engine) = &state.midi_engine {
                    engine.lock().send_command(MidiEngineCommand::Stop);
                }
                Ok(())
            }
            
            DawCommand::StopPlayback => {
                // Can't undo stop - it resets position
                state.status.error("Cannot undo stop playback".to_string());
                Ok(())
            }
            
            DawCommand::PausePlayback => {
                // Resume playback
                state.playing = true;
                state.last_update = Some(std::time::Instant::now());
                if let Some(engine) = &state.midi_engine {
                    engine.lock().send_command(MidiEngineCommand::Start);
                }
                Ok(())
            }
            
            DawCommand::EnableMetronome => {
                // Disable metronome
                state.metronome = false;
                if let Some(engine) = &state.midi_engine {
                    engine.lock().send_command(MidiEngineCommand::SetMetronomeEnabled(false));
                }
                Ok(())
            }
            
            DawCommand::DisableMetronome => {
                // Enable metronome
                state.metronome = true;
                if let Some(engine) = &state.midi_engine {
                    engine.lock().send_command(MidiEngineCommand::SetMetronomeEnabled(true));
                }
                Ok(())
            }
            
            DawCommand::SetBpm { bpm } => {
                // We can't restore the original BPM without storing it
                state.status.error("Undo for SetBpm not yet implemented".to_string());
                Ok(())
            }
            
            DawCommand::SeekTime { time } => {
                // We can't restore the original time without storing it
                state.status.error("Undo for SeekTime not yet implemented".to_string());
                Ok(())
            }
            
            // Selection operations
            DawCommand::SelectTrack { track_id } => {
                // We can't restore the previous selection without storing it
                state.selected_track = None;
                Ok(())
            }
            
            DawCommand::SelectClip { clip_id } => {
                // We can't restore the previous selection without storing it
                state.selected_clip = None;
                Ok(())
            }
            
            DawCommand::DeselectAll => {
                // We can't restore the previous selection without storing it
                state.status.error("Undo for DeselectAll not yet implemented".to_string());
                Ok(())
            }
            
            // Default for unimplemented commands
            _ => {
                state.status.error(format!("Undo not implemented for: {}", self.name()));
                Ok(())
            }
        }
    }

    fn name(&self) -> &'static str {
        match self {
            DawCommand::ResizeNote { .. } => "Resize Note",
            DawCommand::MoveNotes { .. } => "Move Notes",
            DawCommand::DeleteNotes { .. } => "Delete Notes",
            DawCommand::UpdateNoteVelocity { .. } => "Update Note Velocity",
            DawCommand::AddNote { .. } => "Add Note",
            DawCommand::SetSnapMode { .. } => "Set Snap Mode",
            DawCommand::SeekTime { .. } => "Seek Time",
            DawCommand::OpenPianoRoll { .. } => "Open Piano Roll",
            DawCommand::SelectClip { .. } => "Select Clip",
            DawCommand::SelectTrack { .. } => "Select Track",
            DawCommand::AddTrack { .. } => "Add Track",
            DawCommand::DeleteTrack { .. } => "Delete Track",
            DawCommand::AddClip { .. } => "Add Clip",
            DawCommand::DeleteClip { .. } => "Delete Clip",
            DawCommand::MoveClip { .. } => "Move Clip",
            DawCommand::ResizeClip { .. } => "Resize Clip",
            DawCommand::NoOp => "NoOp",
            DawCommand::EnableMetronome { .. } => "Enable Metronome",
            DawCommand::DisableMetronome => "Disable Metronome",
            DawCommand::SetBpm { .. } => "Set BPM",
            DawCommand::StopPlayback => "Stop Playback",
            DawCommand::StartPlayback => "Start Playback",
            DawCommand::PausePlayback => "Pause Playback",
            DawCommand::SetTrackMidiChannel { .. } => "Set Track MIDI Channel",
            DawCommand::MuteTrack { .. } => "Mute Track",
            DawCommand::UnmuteTrack { .. } => "Unmute Track",
            DawCommand::SoloTrack { .. } => "Solo Track",
            DawCommand::UnsoloTrack { .. } => "Unsolo Track",
            DawCommand::ArmTrack { .. } => "Arm Track",
            DawCommand::UnarmTrack { .. } => "Unarm Track",
            DawCommand::SetTrackColor { .. } => "Set Track Color",
            DawCommand::ReorderTracks { .. } => "Reorder Tracks",
            DawCommand::DeselectAll => "Deselect All",
            DawCommand::AddAutomationLane { .. } => "Add Automation Lane",
            DawCommand::RemoveAutomationLane { .. } => "Remove Automation Lane",
            DawCommand::SetAutomationLaneVisibility { .. } => "Set Automation Lane Visibility",
            DawCommand::AddAutomationPoint { .. } => "Add Automation Point",
            DawCommand::DeleteAutomationPoints { .. } => "Delete Automation Points",
            DawCommand::UpdateAutomationPoint { .. } => "Update Automation Point",
            DawCommand::StartMidiRecording { .. } => "Start MIDI Recording",
            DawCommand::StopMidiRecording { .. } => "Stop MIDI Recording",
            DawCommand::SetRecordingMode { .. } => "Set Recording Mode",
            DawCommand::ToggleInputMonitoring { .. } => "Toggle Input Monitoring",
            DawCommand::SetPunchPoints { .. } => "Set Punch Points",
            DawCommand::SetCountInBars { .. } => "Set Count-In Bars",
            DawCommand::QuantizeNotes { .. } => "Quantize Notes",
            DawCommand::CreateTake { .. } => "Create Take",
            DawCommand::SelectTake { .. } => "Select Take",
            DawCommand::DeleteTake { .. } => "Delete Take",
            DawCommand::MuteTake { .. } => "Mute/Unmute Take",
            DawCommand::RenameTake { .. } => "Rename Take",
        }
    }
}

#[derive(Default)]
pub struct CommandCollector {
    commands: Vec<DawCommand>,
}

impl CommandCollector {
    pub fn new() -> Self {
        Self {
            commands: Vec::new(),
        }
    }

    pub fn add_command(&mut self, command: DawCommand) {
        self.commands.push(command);
    }

    pub fn take_commands(&mut self) -> Vec<DawCommand> {
        std::mem::take(&mut self.commands)
    }
}
