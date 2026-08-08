#![allow(unused_variables)]
#![allow(unused_imports)]

use crate::core::{AutomationLane, AutomationParameter, MidiEvent, MidiEventStore};
use serde::{Deserialize, Serialize};
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

type TrackId = String; // Uuid
type ClipId = String; // Uuid

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum SnapMode {
    None,
    Bar,
    Beat,
    Halfbeat,         // 1/2 beat (8th note)
    Quarter,          // 1/4 beat (16th note)
    Eighth,           // 1/8 beat (32nd note)
    Sixteenth,        // 1/16 beat (64th note)
    Triplet,          // 1/3 of a beat (8th-note triplet)
    SixteenthTriplet, // 1/6 of a beat (16th-note triplet)
    ThirtySecond,     // 1/32 beat (128th note)
}

impl SnapMode {
    pub fn get_division(&self, bpm: f64) -> f64 {
        let beat_duration = 60.0 / bpm; // Duration of one beat in seconds
        match self {
            SnapMode::None => 0.0,                             // No snapping
            SnapMode::Bar => beat_duration * 4.0,              // Full measure
            SnapMode::Beat => beat_duration,                   // Quarter note
            SnapMode::Halfbeat => beat_duration / 2.0,         // Eighth note
            SnapMode::Quarter => beat_duration / 4.0,          // Sixteenth note
            SnapMode::Eighth => beat_duration / 8.0,           // 32nd note
            SnapMode::Sixteenth => beat_duration / 16.0,       // 64th note
            SnapMode::Triplet => beat_duration / 3.0,          // Eighth-note triplet
            SnapMode::SixteenthTriplet => beat_duration / 6.0, // 16th-note triplet
            SnapMode::ThirtySecond => beat_duration / 32.0,    // 128th note
        }
    }

    pub fn display_name(&self) -> &str {
        match self {
            SnapMode::None => "None",
            SnapMode::Bar => "Bar",
            SnapMode::Beat => "Beat (1/4)",
            SnapMode::Halfbeat => "1/8",
            SnapMode::Quarter => "1/16",
            SnapMode::Eighth => "1/32",
            SnapMode::Sixteenth => "1/64",
            SnapMode::Triplet => "Triplet (1/3)",
            SnapMode::SixteenthTriplet => "Triplet (1/6)",
            SnapMode::ThirtySecond => "1/128",
        }
    }
}

#[derive(Debug, Clone)]
pub enum EditorView {
    Arrangement,
    PianoRoll {
        clip_id: ClipId,
        track_id: TrackId,
        scroll_position: f32,
        vertical_zoom: f32,
    },
    SampleEditor {
        clip_id: ClipId,
        track_id: TrackId,
        zoom_level: f32,
    },
}

impl Default for EditorView {
    fn default() -> Self {
        Self::Arrangement
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub name: String,
    pub bpm: f64,
    pub ppq: u32,
    pub tracks: Vec<Track>,
    #[serde(skip)]
    pub project_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Track {
    pub id: String,
    pub name: String,
    pub track_type: TrackType,
    pub clips: Vec<Clip>,
    pub is_muted: bool,
    pub is_soloed: bool,
    pub is_armed: bool,
    pub input_monitoring: bool,
    pub color: String,               // Hex color like "#fde047"
    pub takes: Vec<Take>,            // Recording takes
    pub active_take: Option<String>, // Currently active take ID
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Take {
    pub id: String,
    pub track_id: String,
    pub clip_id: String,
    pub name: String,
    pub timestamp: u64, // Unix timestamp
    pub is_muted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum TrackType {
    Midi {
        channel: u8,
        device_name: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type")]
pub enum Clip {
    // TODO: color
    Midi {
        id: ClipId,
        start_time: f64,
        length: f64,
        file_path: PathBuf,
        midi_data: Option<MidiEventStore>,
        loaded: bool,
        #[serde(default)]
        automation_lanes: Vec<AutomationLane>,
    },
}

impl Clip {
    pub fn load_midi(&mut self) -> Result<(), Box<dyn Error>> {
        let Clip::Midi {
            file_path,
            midi_data,
            loaded,
            length,
            ..
        } = self;
        if !*loaded {
            let store = MidiEventStore::load_from_file(file_path)?;

            // Update clip length based on actual MIDI content
            if let Some(last_time) = store.get_last_event_time() {
                *length = last_time;
            }

            *midi_data = Some(store);
            *loaded = true;
        }
        Ok(())
    }

    pub fn get_events_in_time_range(&self, start: f64, end: f64) -> Vec<MidiEvent> {
        let Clip::Midi {
            midi_data,
            start_time,
            ..
        } = self;

        if let Some(store) = midi_data {
            // Adjust time range for clip position
            let clip_start = start - start_time;
            let clip_end = end - start_time;

            store
                .get_events_in_range(clip_start, clip_end)
                .into_iter()
                .map(|event| MidiEvent {
                    time: event.time + start_time,
                    ..event.clone()
                })
                .collect()
        } else {
            Vec::new()
        }
    }
}

// Track-level MIDI handling
impl Track {
    pub fn get_events_in_time_range(&self, start: f64, end: f64) -> Vec<MidiEvent> {
        let TrackType::Midi { .. } = &self.track_type;
        self.clips
            .iter()
            .flat_map(|clip| clip.get_events_in_time_range(start, end))
            .collect()
    }
}

// Project-level MIDI handling
impl Project {
    pub fn get_all_events_in_time_range(&self, start: f64, end: f64) -> Vec<(String, MidiEvent)> {
        self.tracks
            .iter()
            .flat_map(|track| {
                track
                    .get_events_in_time_range(start, end)
                    .into_iter()
                    .map(move |event| (track.id.clone(), event))
            })
            .collect()
    }

    pub fn ticks_per_second(&self) -> f64 {
        (self.bpm / 60.0) * self.ppq as f64
    }

    pub fn beats_per_second(&self) -> f64 {
        self.bpm / 60.0
    }

    pub fn ticks_to_seconds(&self, ticks: u32) -> f64 {
        ticks as f64 / self.ticks_per_second()
    }

    pub fn seconds_to_ticks(&self, seconds: f64) -> u32 {
        (seconds * self.ticks_per_second()) as u32
    }

    pub fn new(name: String) -> Self {
        Self {
            name,
            bpm: 120.0,
            ppq: 480,
            tracks: Vec::new(),
            project_path: None,
        }
    }

    pub fn save(&mut self, path: &Path) -> Result<(), Box<dyn Error>> {
        // Create project directory if it doesn't exist
        fs::create_dir_all(path)?;

        // Create subdirectories for different asset types
        let samples_dir = path.join("samples");
        let midi_dir = path.join("midi");
        fs::create_dir_all(&samples_dir)?;
        fs::create_dir_all(&midi_dir)?;

        println!("After creating folders at: {}", path.display());

        // Persist all MIDI assets under a stable clip-based filename. Using the
        // clip id avoids creating another duplicate asset on every save.
        let mut project = self.clone();
        println!("Saving tracks...");
        for track in &mut project.tracks {
            println!("Saving track: {}", track.name);

            let TrackType::Midi { .. } = &track.track_type;
            println!("MIDI track detected");

            println!("Saving clips...");
            for clip in &mut track.clips {
                let Clip::Midi {
                    id,
                    file_path,
                    midi_data,
                    ..
                } = clip;
                println!("MIDI clip file path: {:?}", file_path);
                let relative_path = PathBuf::from("midi").join(format!("{id}.mid"));
                let target_path = path.join(&relative_path);

                if let Some(midi_data) = midi_data {
                    midi_data.save_to_file(&target_path)?;
                } else if file_path.as_os_str().is_empty() {
                    MidiEventStore::new(project.ppq).save_to_file(&target_path)?;
                } else {
                    copy_midi_asset(file_path, &target_path)?;
                }

                *file_path = relative_path;
            }
        }

        // Save project file
        println!("Finalizing save...");
        let project_file = path.join(format!("{}.supersaw", self.name));
        println!("Saving project to: {}", project_file.display());

        let json = serde_json::to_string_pretty(&project)
            .map_err(|e| format!("Failed to serialize project: {}", e))?;
        fs::write(&project_file, json)
            .map_err(|e| format!("Failed to write project file: {}", e))?;

        self.project_path = Some(path.to_path_buf());
        println!("Project saved successfully.");
        Ok(())
    }

    pub fn load(path: &Path) -> Result<Self, Box<dyn Error>> {
        println!("Loading project from: {}", path.display());
        let content = fs::read_to_string(path)?;
        let mut project: Project = serde_json::from_str(&content)
            .map_err(|e| format!("Failed to deserialize project: {}", e))?;
        let project_dir = path.parent().unwrap_or_else(|| Path::new("."));
        for track in &mut project.tracks {
            for clip in &mut track.clips {
                let Clip::Midi { file_path, .. } = clip;
                if file_path.is_relative() {
                    *file_path = project_dir.join(&*file_path);
                }
            }
        }
        project.project_path = Some(project_dir.to_path_buf());
        println!("Project loaded successfully.");
        Ok(project)
    }

    pub fn create_midi_track_from_file_path(
        &mut self,
        file_path: &Path,
    ) -> Result<TrackId, Box<dyn Error>> {
        let mut clip = Clip::Midi {
            id: Uuid::new_v4().to_string(),
            start_time: 0.0,
            length: 0.0,
            file_path: file_path.to_path_buf(),
            midi_data: None,
            loaded: false,
            automation_lanes: vec![AutomationLane::new(AutomationParameter::Velocity)],
        };

        // Load the MIDI data
        if let Err(e) = clip.load_midi() {
            return Err(format!("Failed to load MIDI file {}: {}", file_path.display(), e).into());
        }

        // Extract file name for track name
        let mid_name = file_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "Unnamed MIDI".to_string());

        // Create track with the loaded clip
        let track_id = Uuid::new_v4().to_string();
        let track = Track {
            id: track_id.clone(),
            name: format!(
                "{} - {}",
                self.tracks.len() + 1,
                mid_name.trim_end_matches(".mid")
            ),
            track_type: TrackType::Midi {
                channel: 1,
                device_name: None,
            },
            clips: vec![clip],
            is_muted: false,
            is_soloed: false,
            is_armed: false,
            input_monitoring: false,
            color: "#fde047".to_string(), // Default yellow
            takes: Vec::new(),
            active_take: None,
        };

        // Add the track to the project
        self.tracks.push(track);

        // Return the new track ID
        Ok(track_id)
    }
}

fn copy_midi_asset(source_path: &Path, target_path: &Path) -> Result<(), Box<dyn Error>> {
    if !source_path.exists() {
        return Err(format!("Source file does not exist: {:?}", source_path).into());
    }

    if source_path.canonicalize().ok() == target_path.canonicalize().ok() && target_path.exists() {
        return Ok(());
    }

    println!("Copying file from {:?} to {:?}", source_path, target_path);

    fs::copy(source_path, &target_path).map_err(|e| {
        format!(
            "Failed to copy {:?} to {:?}: {}",
            source_path, target_path, e
        )
    })?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saving_empty_clip_creates_stable_midi_asset() -> Result<(), Box<dyn Error>> {
        let test_dir = std::env::temp_dir().join(format!("hypersaw-project-{}", Uuid::new_v4()));
        let clip_id = Uuid::new_v4().to_string();
        let mut project = Project::new("Empty clip".to_string());
        project.tracks.push(Track {
            id: Uuid::new_v4().to_string(),
            name: "Track 1".to_string(),
            track_type: TrackType::Midi {
                channel: 1,
                device_name: None,
            },
            clips: vec![Clip::Midi {
                id: clip_id.clone(),
                start_time: 0.0,
                length: 1.0,
                file_path: PathBuf::new(),
                midi_data: Some(MidiEventStore::new(project.ppq)),
                loaded: true,
                automation_lanes: Vec::new(),
            }],
            is_muted: false,
            is_soloed: false,
            is_armed: false,
            input_monitoring: false,
            color: "#fde047".to_string(),
            takes: Vec::new(),
            active_take: None,
        });

        project.save(&test_dir)?;
        let asset_path = test_dir.join("midi").join(format!("{clip_id}.mid"));
        assert!(asset_path.exists());
        assert!(test_dir.join("Empty clip.supersaw").exists());

        let loaded = Project::load(&test_dir.join("Empty clip.supersaw"))?;
        let Clip::Midi { file_path, .. } = &loaded.tracks[0].clips[0];
        assert_eq!(file_path, &asset_path);

        project.save(&test_dir)?;
        let asset_count = fs::read_dir(test_dir.join("midi"))?.count();
        assert_eq!(asset_count, 1);

        fs::remove_dir_all(test_dir)?;
        Ok(())
    }
}
