#[cfg(test)]
mod tests {
    use std::path::Path;
    use crate::core::{Project, Track, TrackType, Clip};
    use uuid::Uuid;

    #[test]
    fn test_dawproject_export() -> Result<(), Box<dyn std::error::Error>> {
        // Create a simple test project
        let mut project = Project::new("Test Project".to_string());
        project.bpm = 140.0;
        
        // Add a test MIDI track
        let track = Track {
            id: Uuid::new_v4().to_string(),
            name: "Test MIDI Track".to_string(),
            track_type: TrackType::Midi { 
                channel: 1, 
                device_name: None 
            },
            clips: vec![],
            is_muted: false,
            is_soloed: false,
            is_armed: false,
            color: "#ff0000".to_string(),
        };
        
        project.tracks.push(track);
        
        // Export to DAWproject
        let export_path = Path::new("/tmp/test_export.dawproject");
        project.export_dawproject(export_path)?;
        
        // Verify the file was created
        assert!(export_path.exists());
        println!("DAWproject exported successfully to: {}", export_path.display());
        
        Ok(())
    }
}