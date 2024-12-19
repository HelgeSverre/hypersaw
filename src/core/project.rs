#![allow(unused_variables)]
#![allow(unused_imports)]

use serde::{Deserialize, Serialize};
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub name: String,
    pub bpm: f32,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Clip {
    Midi {
        id: String,
        start_time: f64,
        length: f64,
        file_path: PathBuf, // Relative to project directory
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

impl Project {
    pub fn new(name: String) -> Self {
        Self {
            name,
            bpm: 140.0, // Default BPM for trance
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

        // Copy all referenced files to project directory and update paths
        let mut project = self.clone();
        for track in &mut project.tracks {
            match &mut track.track_type {
                TrackType::DrumRack { samples } => {
                    for pad in samples {
                        let new_path = copy_to_project_dir(&pad.sample_path, &samples_dir)?;
                        pad.sample_path = new_path;
                    }
                }
                _ => {}
            }

            for clip in &mut track.clips {
                match clip {
                    Clip::Audio { file_path, .. } => {
                        let new_path = copy_to_project_dir(file_path, &samples_dir)?;
                        *file_path = new_path;
                    }
                    Clip::Midi { file_path, .. } => {
                        let new_path = copy_to_project_dir(file_path, &midi_dir)?;
                        *file_path = new_path;
                    }
                }
            }
        }

        // Save project file
        let project_file = path.join(format!("{}.supersaw", self.name));
        let json = serde_json::to_string_pretty(&project)?;
        fs::write(project_file, json)?;

        Ok(())
    }

    pub fn load(path: &Path) -> Result<Self, Box<dyn Error>> {
        let content = fs::read_to_string(path)?;
        let mut project: Project = serde_json::from_str(&content)?;
        project.project_path = Some(path.parent().unwrap().to_path_buf());
        Ok(project)
    }
}

// Helper function to copy a file to the project directory and return the relative path
fn copy_to_project_dir(source_path: &Path, target_dir: &Path) -> Result<PathBuf, Box<dyn Error>> {
    let file_name = source_path
        .file_name()
        .ok_or("Invalid source path")?
        .to_str()
        .ok_or("Invalid file name")?;

    // Generate unique filename to avoid conflicts
    let unique_name = format!(
        "{}_{}.{}",
        source_path.file_stem().unwrap().to_str().unwrap(),
        Uuid::new_v4().to_string().split('-').next().unwrap(),
        source_path
            .extension()
            .unwrap_or_default()
            .to_str()
            .unwrap()
    );

    let target_path = target_dir.join(unique_name);
    fs::copy(source_path, &target_path)?;

    // Convert to relative path
    Ok(PathBuf::from("samples").join(target_path.file_name().unwrap()))
}

// State management for the DAW
#[derive(Debug)]
pub struct DawState {
    pub project: Project,
    pub playing: bool,
    pub recording: bool,
    pub current_time: f64,
    pub selected_track: Option<String>,
    pub selected_clip: Option<String>,
}

impl DawState {
    pub fn new() -> Self {
        Self {
            project: Project::new("Untitled".to_string()),
            playing: false,
            recording: false,
            current_time: 0.0,
            selected_track: None,
            selected_clip: None,
        }
    }
}
