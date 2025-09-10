use std::path::Path;
use crate::core::{Project, Track, TrackType, Clip, MidiEventStore, MidiEvent, MidiMessage};
use uuid::Uuid;

/// Create a sample project with MIDI data for testing DAWproject export
pub fn create_sample_project() -> Project {
    let mut project = Project::new("Sample Project with MIDI".to_string());
    project.bpm = 120.0;
    project.ppq = 480;

    // Create a simple MIDI event store with some notes
    let mut midi_store = MidiEventStore::new(480);
    
    // Add some notes using the Note struct
    let notes = vec![
        // C4 quarter note at beat 0
        crate::core::Note {
            id: Uuid::new_v4().to_string(),
            channel: 0,
            key: 60, // C4
            velocity: 100,
            start_time: 0.0,
            duration: 0.5, // Half a second
            start_tick: 0,
            duration_ticks: 240,
        },
        // E4 quarter note at beat 1
        crate::core::Note {
            id: Uuid::new_v4().to_string(),
            channel: 0,
            key: 64, // E4
            velocity: 90,
            start_time: 1.0,
            duration: 0.5,
            start_tick: 480,
            duration_ticks: 240,
        },
        // G4 quarter note at beat 2
        crate::core::Note {
            id: Uuid::new_v4().to_string(),
            channel: 0,
            key: 67, // G4
            velocity: 85,
            start_time: 2.0,
            duration: 0.5,
            start_tick: 960,
            duration_ticks: 240,
        },
    ];

    for note in notes {
        midi_store.add_note(note);
    }

    // Create a MIDI clip with the events
    let clip = Clip::Midi {
        id: Uuid::new_v4().to_string(),
        start_time: 0.0,
        length: 4.0, // 4 beats
        file_path: std::path::PathBuf::from("generated_notes.mid"),
        midi_data: Some(midi_store),
        loaded: true,
        automation_lanes: Vec::new(),
    };

    // Create a MIDI track
    let track = Track {
        id: Uuid::new_v4().to_string(),
        name: "Piano".to_string(),
        track_type: TrackType::Midi {
            channel: 1,
            device_name: Some("Virtual Piano".to_string()),
        },
        clips: vec![clip],
        is_muted: false,
        is_soloed: false,
        is_armed: true,
        color: "#4a90e2".to_string(),
    };

    project.tracks.push(track);
    project
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sample_project_dawproject_export() -> Result<(), Box<dyn std::error::Error>> {
        let project = create_sample_project();
        
        // Export to DAWproject
        let export_path = Path::new("/tmp/sample_project.dawproject");
        project.export_dawproject(export_path)?;
        
        // Verify the file was created
        assert!(export_path.exists());
        println!("Sample project exported successfully to: {}", export_path.display());
        
        Ok(())
    }
}