#![allow(unused_variables)]
#![allow(unused_imports)]

use crate::core::{Project, Track, Clip, TrackType};
use quick_xml::se::to_string;
use quick_xml::de::from_str;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::error::Error;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use uuid::Uuid;
use zip::{ZipArchive, ZipWriter};

/// DAWproject XML structure for serialization
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename = "Project")]
pub struct DawProject {
    #[serde(rename = "@version")]
    pub version: String,
    #[serde(rename = "Application")]
    pub application: Application,
    #[serde(rename = "Transport", skip_serializing_if = "Option::is_none")]
    pub transport: Option<Transport>,
    #[serde(rename = "Structure", skip_serializing_if = "Option::is_none")]
    pub structure: Option<Structure>,
    #[serde(rename = "Arrangement", skip_serializing_if = "Option::is_none")]
    pub arrangement: Option<Arrangement>,
    #[serde(rename = "Scenes", skip_serializing_if = "Option::is_none")]
    pub scenes: Option<Scenes>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Application {
    #[serde(rename = "@name")]
    pub name: String,
    #[serde(rename = "@version")]
    pub version: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Transport {
    #[serde(rename = "Tempo", skip_serializing_if = "Option::is_none")]
    pub tempo: Option<RealParameter>,
    #[serde(rename = "TimeSignature", skip_serializing_if = "Option::is_none")]
    pub time_signature: Option<TimeSignatureParameter>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RealParameter {
    #[serde(rename = "@max", skip_serializing_if = "Option::is_none")]
    pub max: Option<String>,
    #[serde(rename = "@min", skip_serializing_if = "Option::is_none")]
    pub min: Option<String>,
    #[serde(rename = "@unit")]
    pub unit: String,
    #[serde(rename = "@value", skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    #[serde(rename = "@id")]
    pub id: String,
    #[serde(rename = "@name")]
    pub name: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TimeSignatureParameter {
    #[serde(rename = "@denominator")]
    pub denominator: i32,
    #[serde(rename = "@numerator")]
    pub numerator: i32,
    #[serde(rename = "@id")]
    pub id: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Structure {
    #[serde(rename = "Track")]
    pub tracks: Vec<DawTrack>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DawTrack {
    #[serde(rename = "@contentType")]
    pub content_type: String,
    #[serde(rename = "@loaded")]
    pub loaded: bool,
    #[serde(rename = "@id")]
    pub id: String,
    #[serde(rename = "@name")]
    pub name: String,
    #[serde(rename = "@color", skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(rename = "Channel")]
    pub channel: Channel,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Channel {
    #[serde(rename = "@audioChannels")]
    pub audio_channels: i32,
    #[serde(rename = "@destination", skip_serializing_if = "Option::is_none")]
    pub destination: Option<String>,
    #[serde(rename = "@role")]
    pub role: String,
    #[serde(rename = "@solo")]
    pub solo: bool,
    #[serde(rename = "@id")]
    pub id: String,
    #[serde(rename = "Mute", skip_serializing_if = "Option::is_none")]
    pub mute: Option<BoolParameter>,
    #[serde(rename = "Pan", skip_serializing_if = "Option::is_none")]
    pub pan: Option<RealParameter>,
    #[serde(rename = "Volume", skip_serializing_if = "Option::is_none")]
    pub volume: Option<RealParameter>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BoolParameter {
    #[serde(rename = "@value")]
    pub value: bool,
    #[serde(rename = "@id")]
    pub id: String,
    #[serde(rename = "@name")]
    pub name: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Arrangement {
    #[serde(rename = "@id")]
    pub id: String,
    #[serde(rename = "Lanes")]
    pub lanes: Lanes,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Lanes {
    #[serde(rename = "@timeUnit")]
    pub time_unit: String,
    #[serde(rename = "@id")]
    pub id: String,
    #[serde(rename = "Lanes")]
    pub track_lanes: Vec<TrackLanes>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TrackLanes {
    #[serde(rename = "@track")]
    pub track: String,
    #[serde(rename = "@id")]
    pub id: String,
    #[serde(rename = "Clips")]
    pub clips: DawClips,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DawClips {
    #[serde(rename = "@id")]
    pub id: String,
    #[serde(rename = "Clip")]
    pub clips: Vec<DawClip>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DawClip {
    #[serde(rename = "@time")]
    pub time: String,
    #[serde(rename = "@duration")]
    pub duration: String,
    #[serde(rename = "@playStart")]
    pub play_start: String,
    #[serde(rename = "@name", skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(rename = "Notes", skip_serializing_if = "Option::is_none")]
    pub notes: Option<Notes>,
    #[serde(rename = "Clips", skip_serializing_if = "Option::is_none")]
    pub clips: Option<DawClips>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Notes {
    #[serde(rename = "@id")]
    pub id: String,
    #[serde(rename = "Note")]
    pub notes: Vec<DawNote>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DawNote {
    #[serde(rename = "@time")]
    pub time: String,
    #[serde(rename = "@duration")]
    pub duration: String,
    #[serde(rename = "@channel")]
    pub channel: i32,
    #[serde(rename = "@key")]
    pub key: i32,
    #[serde(rename = "@vel")]
    pub vel: String,
    #[serde(rename = "@rel")]
    pub rel: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Scenes {
    // Empty for now, can be expanded later
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename = "MetaData")]
pub struct MetaData {
    #[serde(rename = "Title", skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(rename = "Artist", skip_serializing_if = "Option::is_none")]
    pub artist: Option<String>,
    #[serde(rename = "Comment", skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
}

impl Project {
    /// Export project to DAWproject format
    pub fn export_dawproject(&self, path: &Path) -> Result<(), Box<dyn Error>> {
        // Create the ZIP file
        let file = fs::File::create(path)?;
        let mut zip = ZipWriter::new(file);

        // Convert to DAWproject format
        let daw_project = self.to_daw_project();
        let metadata = self.to_metadata();

        // Write project.xml
        let project_xml = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n{}",
            to_string(&daw_project)?
        );
        zip.start_file("project.xml", zip::write::FileOptions::default())?;
        zip.write_all(project_xml.as_bytes())?;

        // Write metadata.xml
        let metadata_xml = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n{}",
            to_string(&metadata)?
        );
        zip.start_file("metadata.xml", zip::write::FileOptions::default())?;
        zip.write_all(metadata_xml.as_bytes())?;

        // Copy referenced files (audio, MIDI) to the archive
        self.copy_media_files_to_zip(&mut zip)?;

        zip.finish()?;
        Ok(())
    }

    /// Import project from DAWproject format
    pub fn import_dawproject(path: &Path) -> Result<Self, Box<dyn Error>> {
        let file = fs::File::open(path)?;
        let mut archive = ZipArchive::new(file)?;

        // Read project.xml
        let project_content = {
            let mut project_file = archive.by_name("project.xml")?;
            let mut content = String::new();
            project_file.read_to_string(&mut content)?;
            content
        };
        
        let daw_project: DawProject = from_str(&project_content)?;

        // Read metadata.xml if present
        let metadata = {
            if let Ok(mut metadata_file) = archive.by_name("metadata.xml") {
                let mut metadata_content = String::new();
                metadata_file.read_to_string(&mut metadata_content)?;
                Some(from_str::<MetaData>(&metadata_content)?)
            } else {
                None
            }
        };

        // Convert to Project format
        let mut project = Self::from_daw_project(daw_project, metadata)?;
        
        // Set project path for relative file references
        if let Some(parent) = path.parent() {
            project.project_path = Some(parent.to_path_buf());
        }

        Ok(project)
    }

    fn to_daw_project(&self) -> DawProject {
        let mut id_counter = 0;
        let mut generate_id = || {
            id_counter += 1;
            format!("id{}", id_counter)
        };

        // Create master track for mixer destination
        let master_id = generate_id();

        DawProject {
            version: "1.0".to_string(),
            application: Application {
                name: "HyperSaw".to_string(),
                version: "0.1.0".to_string(),
            },
            transport: Some(Transport {
                tempo: Some(RealParameter {
                    max: Some("300.0".to_string()),
                    min: Some("60.0".to_string()),
                    unit: "bpm".to_string(),
                    value: Some(self.bpm.to_string()),
                    id: generate_id(),
                    name: "Tempo".to_string(),
                }),
                time_signature: Some(TimeSignatureParameter {
                    denominator: 4,
                    numerator: 4,
                    id: generate_id(),
                }),
            }),
            structure: Some(Structure {
                tracks: self.convert_tracks_to_daw(&mut generate_id, &master_id),
            }),
            arrangement: Some(self.convert_arrangement_to_daw(&mut generate_id)),
            scenes: Some(Scenes {}),
        }
    }

    fn to_metadata(&self) -> MetaData {
        MetaData {
            title: Some(self.name.clone()),
            artist: None,
            comment: Some("Exported from HyperSaw DAW".to_string()),
        }
    }

    fn convert_tracks_to_daw(&self, generate_id: &mut impl FnMut() -> String, master_id: &str) -> Vec<DawTrack> {
        let mut daw_tracks = Vec::new();

        // Add regular tracks
        for track in &self.tracks {
            let track_id = track.id.clone();
            let channel_id = generate_id();
            
            let content_type = match track.track_type {
                TrackType::Midi { .. } => "notes",
                TrackType::Audio => "audio",
            };

            daw_tracks.push(DawTrack {
                content_type: content_type.to_string(),
                loaded: true,
                id: track_id,
                name: track.name.clone(),
                color: Some(track.color.clone()),
                channel: Channel {
                    audio_channels: 2,
                    destination: Some(master_id.to_string()),
                    role: "regular".to_string(),
                    solo: track.is_soloed,
                    id: channel_id.clone(),
                    mute: Some(BoolParameter {
                        value: track.is_muted,
                        id: generate_id(),
                        name: "Mute".to_string(),
                    }),
                    pan: Some(RealParameter {
                        max: Some("1.0".to_string()),
                        min: Some("0.0".to_string()),
                        unit: "normalized".to_string(),
                        value: Some("0.5".to_string()),
                        id: generate_id(),
                        name: "Pan".to_string(),
                    }),
                    volume: Some(RealParameter {
                        max: Some("2.0".to_string()),
                        min: Some("0.0".to_string()),
                        unit: "linear".to_string(),
                        value: Some("1.0".to_string()),
                        id: generate_id(),
                        name: "Volume".to_string(),
                    }),
                },
            });
        }

        // Add master track
        daw_tracks.push(DawTrack {
            content_type: "audio notes".to_string(),
            loaded: true,
            id: master_id.to_string(),
            name: "Master".to_string(),
            color: None,
            channel: Channel {
                audio_channels: 2,
                destination: None,
                role: "master".to_string(),
                solo: false,
                id: generate_id(),
                mute: Some(BoolParameter {
                    value: false,
                    id: generate_id(),
                    name: "Mute".to_string(),
                }),
                pan: Some(RealParameter {
                    max: Some("1.0".to_string()),
                    min: Some("0.0".to_string()),
                    unit: "normalized".to_string(),
                    value: Some("0.5".to_string()),
                    id: generate_id(),
                    name: "Pan".to_string(),
                }),
                volume: Some(RealParameter {
                    max: Some("2.0".to_string()),
                    min: Some("0.0".to_string()),
                    unit: "linear".to_string(),
                    value: Some("1.0".to_string()),
                    id: generate_id(),
                    name: "Volume".to_string(),
                }),
            },
        });

        daw_tracks
    }

    fn convert_arrangement_to_daw(&self, generate_id: &mut impl FnMut() -> String) -> Arrangement {
        let arrangement_id = generate_id();
        let lanes_id = generate_id();

        let track_lanes: Vec<TrackLanes> = self.tracks.iter().map(|track| {
            let track_lanes_id = generate_id();
            let clips_id = generate_id();

            let daw_clips: Vec<DawClip> = track.clips.iter().map(|clip| {
                match clip {
                    Clip::Midi { start_time, length, midi_data, .. } => {
                        let notes_id = generate_id();
                        let notes = if let Some(store) = midi_data {
                            let midi_notes: Vec<DawNote> = store.get_notes_in_range(0.0, f64::MAX)
                                .iter()
                                .map(|note| {
                                    DawNote {
                                        time: note.start_time.to_string(),
                                        duration: note.duration.to_string(),
                                        channel: note.channel as i32,
                                        key: note.key as i32,
                                        vel: (note.velocity as f32 / 127.0).to_string(),
                                        rel: (note.velocity as f32 / 127.0).to_string(),
                                    }
                                })
                                .collect();
                            
                            Some(Notes {
                                id: notes_id,
                                notes: midi_notes,
                            })
                        } else {
                            None
                        };

                        DawClip {
                            time: start_time.to_string(),
                            duration: length.to_string(),
                            play_start: "0.0".to_string(),
                            name: None,
                            notes,
                            clips: None,
                        }
                    }
                    Clip::Audio { start_time, length, .. } => {
                        DawClip {
                            time: start_time.to_string(),
                            duration: length.to_string(),
                            play_start: "0.0".to_string(),
                            name: None,
                            notes: None,
                            clips: None,
                        }
                    }
                }
            }).collect();

            TrackLanes {
                track: track.id.clone(),
                id: track_lanes_id,
                clips: DawClips {
                    id: clips_id,
                    clips: daw_clips,
                },
            }
        }).collect();

        Arrangement {
            id: arrangement_id,
            lanes: Lanes {
                time_unit: "beats".to_string(),
                id: lanes_id,
                track_lanes,
            },
        }
    }

    fn copy_media_files_to_zip(&self, zip: &mut ZipWriter<fs::File>) -> Result<(), Box<dyn Error>> {
        // Create directories for media files
        let audio_dir = "audio/";
        let midi_dir = "midi/";

        for track in &self.tracks {
            for clip in &track.clips {
                match clip {
                    Clip::Audio { file_path, .. } => {
                        if file_path.exists() {
                            let filename = file_path.file_name()
                                .ok_or("Invalid audio file path")?
                                .to_string_lossy();
                            let zip_path = format!("{}{}", audio_dir, filename);
                            
                            zip.start_file(&zip_path, zip::write::FileOptions::default())?;
                            let data = fs::read(file_path)?;
                            zip.write_all(&data)?;
                        }
                    }
                    Clip::Midi { file_path, .. } => {
                        if file_path.exists() {
                            let filename = file_path.file_name()
                                .ok_or("Invalid MIDI file path")?
                                .to_string_lossy();
                            let zip_path = format!("{}{}", midi_dir, filename);
                            
                            zip.start_file(&zip_path, zip::write::FileOptions::default())?;
                            let data = fs::read(file_path)?;
                            zip.write_all(&data)?;
                        }
                    }
                }
            }
        }

        Ok(())
    }

    fn from_daw_project(daw_project: DawProject, _metadata: Option<MetaData>) -> Result<Self, Box<dyn Error>> {
        let mut project = Project::new("Imported Project".to_string());

        // Set BPM from transport
        if let Some(transport) = &daw_project.transport {
            if let Some(tempo) = &transport.tempo {
                if let Some(value) = &tempo.value {
                    project.bpm = value.parse().unwrap_or(120.0);
                }
            }
        }

        // Convert tracks
        if let Some(structure) = &daw_project.structure {
            for daw_track in &structure.tracks {
                // Skip master track
                if daw_track.channel.role == "master" {
                    continue;
                }

                let track_type = match daw_track.content_type.as_str() {
                    "notes" => TrackType::Midi { 
                        channel: 1, 
                        device_name: None 
                    },
                    "audio" => TrackType::Audio,
                    _ => TrackType::Audio, // Default to audio
                };

                let track = Track {
                    id: daw_track.id.clone(),
                    name: daw_track.name.clone(),
                    track_type,
                    clips: Vec::new(), // Will be populated from arrangement
                    is_muted: daw_track.channel.mute.as_ref().map(|m| m.value).unwrap_or(false),
                    is_soloed: daw_track.channel.solo,
                    is_armed: false,
                    color: daw_track.color.clone().unwrap_or("#fde047".to_string()),
                };

                project.tracks.push(track);
            }
        }

        // TODO: Convert arrangement clips to track clips
        // This would require more complex logic to map clips back to tracks

        Ok(project)
    }
}