use super::{Note, Clip, Track, AutomationLane, AutomationPoint, Take};
use std::collections::HashMap;

/// Stores the minimal data needed to undo each command type
#[derive(Debug, Clone)]
pub enum UndoData {
    // Note operations
    AddedNote { note_id: String },
    DeletedNotes { notes: Vec<Note> },
    MovedNotes { original_positions: HashMap<String, (f64, u8)> }, // note_id -> (start_time, pitch)
    ResizedNote { original_start: f64, original_duration: f64 },
    UpdatedNoteVelocity { original_velocity: u8 },
    
    // Track operations
    AddedTrack { track_id: String },
    DeletedTrack { track: Track, index: usize },
    TrackMidiChannel { previous_channel: u8 },
    TrackMuted { was_muted: bool },
    TrackSoloed { was_soloed: bool },
    TrackArmed { was_armed: bool },
    TrackColor { previous_color: String },
    ReorderedTracks { from_index: usize, to_index: usize },
    
    // Clip operations
    AddedClip { clip_id: String },
    DeletedClip { clip: Clip, track_id: String },
    MovedClip { original_position: f64 },
    ResizedClip { original_start: f64, original_length: f64 },
    
    // Selection
    PreviousSelection { 
        selected_track: Option<String>,
        selected_clip: Option<String>,
    },
    
    // Transport
    PlaybackState { was_playing: bool },
    PreviousTime { time: f64 },
    PreviousBpm { bpm: f64 },
    MetronomeState { was_enabled: bool },
    
    // Automation
    AddedAutomationLane { lane_id: String },
    RemovedAutomationLane { lane: AutomationLane },
    AutomationVisibility { was_visible: bool },
    AddedAutomationPoint { point_id: String },
    DeletedAutomationPoints { points: Vec<(String, AutomationPoint)> }, // (lane_id, point)
    UpdatedAutomationPoint { original_time: f64, original_value: f64 },
    
    // Recording
    RecordingState { was_recording: bool, track_id: Option<String> },
    InputMonitoring { was_monitoring: bool },
    PunchPoints { previous_in: Option<f64>, previous_out: Option<f64> },
    CountInBars { previous_bars: u32 },
    
    // Takes
    CreatedTake { take_id: String },
    DeletedTake { take: Take },
    TakeMuted { was_muted: bool },
    TakeRenamed { previous_name: String },
    SelectedTake { previous_take_id: Option<String> },
    
    // Quantization
    QuantizedNotes { original_timings: HashMap<String, (f64, u32)> }, // note_id -> (start_time, start_tick)
    
    // No undo data needed
    None,
}

/// Manager for storing undo data alongside commands
pub struct UndoDataStore {
    data: HashMap<usize, UndoData>, // command_id -> undo_data
    next_id: usize,
}

impl UndoDataStore {
    pub fn new() -> Self {
        Self {
            data: HashMap::new(),
            next_id: 0,
        }
    }
    
    pub fn store(&mut self, data: UndoData) -> usize {
        let id = self.next_id;
        self.next_id += 1;
        self.data.insert(id, data);
        id
    }
    
    pub fn retrieve(&mut self, id: usize) -> Option<UndoData> {
        self.data.remove(&id)
    }
    
    pub fn clear(&mut self) {
        self.data.clear();
    }
}