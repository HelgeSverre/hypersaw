#![allow(unused_variables)]
#![allow(unused_imports)]

use crate::core::{MidiEvent, MidiEventStore};
use serde::{Deserialize, Serialize};
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

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
            SnapMode::None => 0.0,
            SnapMode::Bar => beat_duration * 4.0, // Full measure
            SnapMode::Beat => beat_duration,      // Quarter note
            SnapMode::Halfbeat => beat_duration / 2.0, // Eighth note
            SnapMode::Quarter => beat_duration / 4.0, // Sixteenth note
            SnapMode::Eighth => beat_duration / 8.0, // 32nd note
            SnapMode::Sixteenth => beat_duration / 16.0, // 64th note
            SnapMode::Triplet => beat_duration / 3.0, // Eighth-note triplet
            SnapMode::SixteenthTriplet => beat_duration / 6.0, // 16th-note triplet
            SnapMode::ThirtySecond => beat_duration / 32.0, // 128th note
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
        clip_id: String,
        track_id: String,
        scroll_position: f32,
        vertical_zoom: f32,
    },
    SampleEditor {
        clip_id: String,
        track_id: String,
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum TrackType {
    Midi { channel: u8, device_name: String },
    DrumRack { samples: Vec<DrumPad> },
    Audio,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DrumPad {
    pub note: u8,
    pub name: String,
    pub sample_path: PathBuf, // Relative to project directory
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type")]
pub enum Clip {
    Midi {
        id: String,
        start_time: f64,
        length: f64,
        file_path: PathBuf,
        midi_data: Option<MidiEventStore>,
        loaded: bool,
    },
    Audio {
        id: String,
        start_time: f64,
        length: f64,
        file_path: PathBuf, // Relative to project directory
        start_offset: f64,  // Start point within audio file
        end_offset: f64,    // End point within audio file
    },
}

impl Clip {
    pub fn load_midi(&mut self) -> Result<(), Box<dyn Error>> {
        if let Clip::Midi {
            file_path,
            midi_data,
            loaded,
            length,
            ..
        } = self
        {
            if !*loaded {
                let store = MidiEventStore::load_from_file(file_path)?;

                // Update clip length based on actual MIDI content
                if let Some(last_time) = store.get_last_event_time() {
                    *length = last_time;
                }

                *midi_data = Some(store);
                *loaded = true;
            }
        }
        Ok(())
    }

    pub fn get_events_in_time_range(&self, start: f64, end: f64) -> Vec<MidiEvent> {
        match self {
            Clip::Midi {
                midi_data,
                start_time,
                ..
            } => {
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
            _ => Vec::new(),
        }
    }
}

// Track-level MIDI handling
impl Track {
    pub fn get_events_in_time_range(&self, start: f64, end: f64) -> Vec<MidiEvent> {
        match &self.track_type {
            TrackType::Midi { .. } => self
                .clips
                .iter()
                .flat_map(|clip| clip.get_events_in_time_range(start, end))
                .collect(),
            _ => Vec::new(),
        }
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

    pub fn save(&self, path: &Path) -> Result<(), Box<dyn Error>> {
        // Create project directory if it doesn't exist
        fs::create_dir_all(path)?;

        // Create subdirectories for different asset types
        let samples_dir = path.join("samples");
        let midi_dir = path.join("midi");
        fs::create_dir_all(&samples_dir)?;
        fs::create_dir_all(&midi_dir)?;

        println!("After creating folders at: {}", path.display());

        // Copy all referenced files to project directory and update paths
        let mut project = self.clone();
        println!("Saving tracks...");
        for track in &mut project.tracks {
            println!("Saving track: {}", track.name);

            match &mut track.track_type {
                TrackType::DrumRack { samples } => {
                    for pad in samples {
                        println!("Drum rack sample path: {:?}", pad.sample_path);
                        let new_path = copy_to_project_dir(&pad.sample_path, &samples_dir)?;
                        pad.sample_path = new_path;
                    }
                }

                TrackType::Midi { .. } => {
                    println!("MIDI track detected");
                }

                TrackType::Audio => {
                    println!("Audio track detected");
                }
            }

            println!("Saving clips...");
            for clip in &mut track.clips {
                match clip {
                    Clip::Audio { file_path, .. } => {
                        println!("Audio clip file path: {:?}", file_path);
                        let new_path = copy_to_project_dir(file_path, &samples_dir)?;
                        *file_path = new_path;
                    }
                    Clip::Midi { file_path, .. } => {
                        println!("MIDI clip file path: {:?}", file_path);
                        let new_path = copy_to_project_dir(file_path, &midi_dir)?;
                        *file_path = new_path;
                    }
                }
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

        println!("Project saved successfully.");
        Ok(())
    }

    pub fn load(path: &Path) -> Result<Self, Box<dyn Error>> {
        println!("Loading project from: {}", path.display());
        let content = fs::read_to_string(path)?;
        let mut project: Project = serde_json::from_str(&content)
            .map_err(|e| format!("Failed to deserialize project: {}", e))?;
        project.project_path = Some(path.parent().unwrap().to_path_buf());
        println!("Project loaded successfully.");
        Ok(project)
    }
}

// Helper function to copy a file to the project directory and return the relative path
fn copy_to_project_dir(source_path: &Path, target_dir: &Path) -> Result<PathBuf, Box<dyn Error>> {
    if !source_path.exists() {
        return Err(format!("Source file does not exist: {:?}", source_path).into());
    }

    let file_name = source_path
        .file_name()
        .ok_or_else(|| "Invalid source path: Missing file name")?;

    // Generate unique filename to avoid conflicts
    let unique_name = format!(
        "{}_{}.{}",
        source_path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy(),
        Uuid::new_v4().to_string().split('-').next().unwrap(),
        source_path
            .extension()
            .unwrap_or_default()
            .to_string_lossy()
    );

    let target_path = target_dir.join(unique_name);
    println!("Copying file from {:?} to {:?}", source_path, target_path);

    fs::copy(source_path, &target_path).map_err(|e| {
        format!(
            "Failed to copy {:?} to {:?}: {}",
            source_path, target_path, e
        )
    })?;

    Ok(target_path)
}
