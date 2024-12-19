// src/core/commands.rs
use super::*;
use std::path::PathBuf;
use egui::debug_text::print;
use uuid::Uuid;

pub trait Command {
    fn execute(&self, state: &mut DawState) -> Result<(), Box<dyn std::error::Error>>;
    fn undo(&self, state: &mut DawState) -> Result<(), Box<dyn std::error::Error>>;
    fn name(&self) -> &'static str;
}

#[derive(Debug)]
pub enum DawCommand {
    OpenPianoRoll {
        clip_id: String,
        track_id: String,
    },
    SelectClip {
        clip_id: String,
    },
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
}

impl Command for DawCommand {
    fn execute(&self, state: &mut DawState) -> Result<(), Box<dyn std::error::Error>> {
        match self {
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
                print!("Selected clip: {}", clip_id);
                state.status.info(format!("Selected clip: {}", clip_id));
                state.selected_clip = Some(clip_id.clone());
                Ok(())
            }

            DawCommand::SelectTrack { track_id } => {
                state.selected_track = Some(track_id.clone());
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
                        },
                        TrackType::Audio => Clip::Audio {
                            id: Uuid::new_v4().to_string(),
                            start_time: *start_time,
                            length: *length,
                            file_path: file_path.clone(),
                            start_offset: 0.0,
                            end_offset: *length,
                        },
                        _ => return Err("Invalid track type for clip".into()),
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
        }
    }

    fn undo(&self, state: &mut DawState) -> Result<(), Box<dyn std::error::Error>> {
        // TODO: Implement undo for each command
        // Will need to store previous state information
        Ok(())
    }

    fn name(&self) -> &'static str {
        match self {
            DawCommand::OpenPianoRoll { .. } => "Open Piano Roll",
            DawCommand::SelectClip { .. } => "Select Clip",
            DawCommand::SelectTrack { .. } => "Select Track",
            DawCommand::AddTrack { .. } => "Add Track",
            DawCommand::DeleteTrack { .. } => "Delete Track",
            DawCommand::AddClip { .. } => "Add Clip",
            DawCommand::DeleteClip { .. } => "Delete Clip",
            DawCommand::MoveClip { .. } => "Move Clip",
            DawCommand::ResizeClip { .. } => "Resize Clip",
        }
    }
}
