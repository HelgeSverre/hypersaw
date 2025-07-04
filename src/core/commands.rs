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
    },
    UpdateNoteVelocity {
        clip_id: String,
        note_id: EventID,
        velocity: u8,
    },

    ResizeNote {
        clip_id: String,
        note_id: String,
        new_start_time: f64,
        new_duration: f64,
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
                }
                Ok(())
            }
            
            DawCommand::UnmuteTrack { track_id } => {
                if let Some(track) = state.project.tracks.iter_mut().find(|t| t.id == *track_id) {
                    track.is_muted = false;
                }
                Ok(())
            }
            
            DawCommand::SoloTrack { track_id } => {
                // First, unsolo all tracks
                for track in &mut state.project.tracks {
                    track.is_soloed = false;
                }
                // Then solo the specified track
                if let Some(track) = state.project.tracks.iter_mut().find(|t| t.id == *track_id) {
                    track.is_soloed = true;
                }
                Ok(())
            }
            
            DawCommand::UnsoloTrack { track_id } => {
                if let Some(track) = state.project.tracks.iter_mut().find(|t| t.id == *track_id) {
                    track.is_soloed = false;
                }
                Ok(())
            }
            
            DawCommand::ArmTrack { track_id } => {
                if let Some(track) = state.project.tracks.iter_mut().find(|t| t.id == *track_id) {
                    track.is_armed = true;
                }
                Ok(())
            }
            
            DawCommand::UnarmTrack { track_id } => {
                if let Some(track) = state.project.tracks.iter_mut().find(|t| t.id == *track_id) {
                    track.is_armed = false;
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
                    color: "#fde047".to_string(), // Default yellow
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
                    let clip = match track.track_type {
                        TrackType::Midi { .. } => Clip::Midi {
                            id: Uuid::new_v4().to_string(),
                            start_time: *start_time,
                            length: *length,
                            file_path: file_path.clone(),
                            midi_data: None,
                            loaded: false,
                            automation_lanes: Vec::new(),
                        },
                        TrackType::Audio => Clip::Audio {
                            id: Uuid::new_v4().to_string(),
                            start_time: *start_time,
                            length: *length,
                            file_path: file_path.clone(),
                            start_offset: 0.0,
                            end_offset: *length,
                        },
                    };
                    track.clips.push(clip);
                }
                Ok(())
            }

            DawCommand::DeleteClip { track_id, clip_id } => {
                if let Some(track) = state.project.tracks.iter_mut().find(|t| t.id == *track_id) {
                    if let Some(index) = track.clips.iter().position(|c| match c {
                        Clip::Midi { id, .. } | Clip::Audio { id, .. } => id == clip_id,
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
                    if let Some(clip) = track.clips.iter_mut().find(|c| match c {
                        Clip::Midi { id, .. } | Clip::Audio { id, .. } => id == clip_id,
                    }) {
                        match clip {
                            Clip::Midi { start_time, .. } => *start_time = *new_start_time,
                            Clip::Audio { start_time, .. } => *start_time = *new_start_time,
                        }
                    }
                }
                Ok(())
            }

            DawCommand::ResizeClip {
                clip_id,
                new_length,
            } => {
                for track in &mut state.project.tracks {
                    if let Some(clip) = track.clips.iter_mut().find(|c| match c {
                        Clip::Midi { id, .. } | Clip::Audio { id, .. } => id == clip_id,
                    }) {
                        match clip {
                            Clip::Midi { length, .. } => *length = *new_length,
                            Clip::Audio { length, .. } => *length = *new_length,
                        }
                    }
                }
                Ok(())
            }

            // Do nothing.
            DawCommand::NoOp => Ok(()),
            DawCommand::EnableMetronome {} => {
                state.metronome = true;
                state.status.info("Metronome enabled".to_string());
                Ok(())
            }
            DawCommand::DisableMetronome => {
                state.metronome = false;
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

                Ok(())
            }

            DawCommand::PausePlayback => {
                state.playing = false;

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

            DawCommand::DeleteNotes { clip_id, note_ids } => {
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
            
            DawCommand::UpdateNoteVelocity { clip_id, note_id, velocity } => {
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
        }
    }

    fn undo(&self, state: &mut DawState) -> Result<(), Box<dyn std::error::Error>> {
        // TODO: Implement undo for each command
        // Will need to store previous state information
        Ok(())
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
