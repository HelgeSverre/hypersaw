use crate::core::{EditorView, Project, StatusManager};

#[derive(Clone, Debug)]
pub struct DawState {
    pub project: Project,
    pub metronome: bool,
    pub playing: bool,
    pub recording: bool,
    pub current_time: f64,
    pub loop_enabled: bool,
    pub loop_start: f64,
    pub loop_end: f64,

    pub last_update: Option<std::time::Instant>,
    pub selected_track: Option<String>,
    pub selected_clip: Option<String>,
    pub current_view: EditorView,
    pub status: StatusManager,
}

impl DawState {
    pub fn new() -> Self {
        Self {
            project: Project::new("Untitled".to_string()),
            metronome: false,
            playing: false,
            recording: false,
            current_time: 0.0,
            last_update: None,
            selected_track: None,
            selected_clip: None,
            loop_enabled: true,
            loop_start: 3.0,
            loop_end: 4.0,
            current_view: EditorView::default(),
            status: StatusManager::new(),
        }
    }

    pub fn update_playhead(&mut self) {
        let now = std::time::Instant::now();

        if self.playing {
            if let Some(last_update) = self.last_update {
                let elapsed = now.duration_since(last_update).as_secs_f64();
                let seconds_per_beat = 60.0 / self.project.bpm;
                let beats_elapsed = elapsed / seconds_per_beat;
                let ticks_elapsed = beats_elapsed * self.project.ppq as f64;
                let ticks_per_second = self.project.bpm / 60.0 * self.project.ppq as f64;

                self.current_time += elapsed;

                // Handle looping
                if self.loop_enabled && self.current_time >= self.loop_end {
                    let minimum_loop_length = 5.0;
                    if (self.loop_end - self.loop_start) > minimum_loop_length {
                        self.current_time = self.loop_start + (self.current_time - self.loop_end);
                    } else {
                        self.current_time = self.loop_start;
                    }
                }
            }
        }

        self.last_update = Some(now);
    }
}
