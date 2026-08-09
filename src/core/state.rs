use crate::core::{
    EditorView, MidiEngineHandle, Project, RecordingCoordinator, RecordingMode,
    RecordingSessionContext, SnapMode, StatusManager,
};
use parking_lot::Mutex;
use std::sync::{atomic::AtomicU64, Arc};

pub struct DawState {
    pub project: Project,
    pub snap_mode: SnapMode,
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
    // pub plugin_manager: PluginManager,

    // Shared UI state
    pub track_scroll_y: f32,

    // MIDI engine
    pub midi_engine: Option<Arc<Mutex<MidiEngineHandle>>>,

    // Recording
    pub recording_coordinator: Option<Arc<Mutex<RecordingCoordinator>>>,
    pub midi_input_sender:
        Option<crossbeam::channel::Sender<(String, crate::core::MidiMessage, u64)>>,
    pub recording_track: Option<String>,
    pub recording_mode: RecordingMode,
    pub punch_in: Option<f64>,
    pub punch_out: Option<f64>,
    pub count_in_bars: u32,
    pub count_in_active: bool,
    pub count_in_start_time: Option<f64>,
    pub pending_recording_session: Option<RecordingSessionContext>,
}

impl DawState {
    pub fn new() -> Self {
        let capture_sample_clock = Arc::new(AtomicU64::new(0));
        // Start MIDI engine
        let midi_engine = MidiEngineHandle::start(44100, capture_sample_clock.clone()); // TODO: Get actual sample rate
        let midi_engine_arc = Arc::new(Mutex::new(midi_engine));

        // Create bounded channel for MIDI input from engine to recording (prevent unbounded growth)
        // Buffer up to 2000 MIDI events
        let (midi_input_tx, midi_input_rx) = crossbeam::channel::bounded(2000);

        // Create recording coordinator
        let recording_coordinator =
            RecordingCoordinator::new(44100, midi_input_rx, capture_sample_clock);

        Self {
            project: Project::new("Untitled".to_string()),
            snap_mode: SnapMode::Eighth,
            metronome: false,
            playing: false,
            recording: false,
            current_time: 0.0,
            last_update: None,
            selected_track: None,
            selected_clip: None,
            loop_enabled: false,
            loop_start: 3.0,
            loop_end: 4.0,
            current_view: EditorView::default(),
            status: StatusManager::new(),
            track_scroll_y: 0.0,
            midi_engine: Some(midi_engine_arc),
            recording_coordinator: Some(Arc::new(Mutex::new(recording_coordinator))),
            midi_input_sender: Some(midi_input_tx),
            recording_track: None,
            recording_mode: RecordingMode::Overdub,
            punch_in: None,
            punch_out: None,
            count_in_bars: 1,
            count_in_active: false,
            count_in_start_time: None,
            pending_recording_session: None,
        }
    }

    pub fn update_playhead(&mut self) {
        let now = std::time::Instant::now();

        if self.playing {
            // When engine is active AND playing, it's the source of truth for playhead time
            // The engine sends PositionUpdate messages that update current_time
            // So we skip local increment to avoid dual time sources
            if self.midi_engine.is_some() {
                // Still update last_update for timing consistency
                self.last_update = Some(now);
                return;
            }

            // Fallback: update time locally if no engine
            if let Some(last_update) = self.last_update {
                let delta_time = now.duration_since(last_update).as_secs_f64();

                self.current_time += delta_time;

                let loop_length = self.loop_end - self.loop_start;
                if self.loop_enabled
                    && loop_length > f64::EPSILON
                    && self.current_time >= self.loop_end
                {
                    self.current_time = self.loop_start
                        + (self.current_time - self.loop_start).rem_euclid(loop_length);
                }
            }
        }

        self.last_update = Some(now);
    }
}
