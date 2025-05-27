use super::*;
use std::path::PathBuf;
use uuid::Uuid;

pub trait Command {
    fn execute(&self, state: &mut DawState) -> Result<(), Box<dyn std::error::Error>>;
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

    // Clips
    SelectClip {
        clip_id: String,
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

    // Transport
    EnableMetronome,
    DisableMetronome,
    ToggleMetronome,
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
                state.transport.seek_to(*time);

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
                            midi_data: None,
                            loaded: false,
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
            DawCommand::ToggleMetronome => {
                if state.metronome {
                    state.metronome = false;
                    state.status.info("Metronome disabled".to_string());
                } else {
                    state.metronome = true;
                    state.status.info("Metronome enabled".to_string());
                }

                Ok(())
            }
            DawCommand::DisableMetronome => {
                state.metronome = false;
                state.status.info("Metronome disabled".to_string());
                Ok({})
            }
            DawCommand::SetBpm { bpm } => {
                state.project.bpm = *bpm;
                state.transport.set_bpm(*bpm);
                state.status.info(format!("BPM set to: {}", bpm));
                Ok(())
            }
            DawCommand::StopPlayback => {
                state.stop_playback();
                Ok(())
            }

            DawCommand::StartPlayback => {
                state.start_playback();

                Ok(())
            }

            DawCommand::PausePlayback => {
                state.stop_playback();

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
        }
    }

    fn name(&self) -> &'static str {
        match self {
            DawCommand::ResizeNote { .. } => "Resize Note",
            DawCommand::MoveNotes { .. } => "Move Notes",
            DawCommand::DeleteNotes { .. } => "Delete Notes",
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
            DawCommand::ToggleMetronome { .. } => "Toggle Metronome",
            DawCommand::EnableMetronome { .. } => "Enable Metronome",
            DawCommand::DisableMetronome => "Disable Metronome",
            DawCommand::SetBpm { .. } => "Set BPM",
            DawCommand::StopPlayback => "Stop Playback",
            DawCommand::StartPlayback => "Start Playback",
            DawCommand::PausePlayback => "Pause Playback",
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
