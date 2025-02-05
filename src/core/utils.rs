use crate::core::SnapMode;

pub struct TimeUtils {}

impl TimeUtils {
    pub fn snap_time(time: f64, bpm: f64, snap_mode: SnapMode) -> f64 {
        let division = snap_mode.get_division(bpm);
        if division == 0.0 {
            return time;
        }
        (time / division).round() * division
    }

    pub fn beats_to_seconds(beats: f64, bpm: f64) -> f64 {
        beats * 60.0 / bpm
    }

    pub fn seconds_to_beats(seconds: f64, bpm: f64) -> f64 {
        seconds * bpm / 60.0
    }
}

pub struct NotePositioning {
    /// Pixels per second for time axis
    time_zoom: f32,
    /// Pixels per semitone for pitch axis
    pitch_zoom: f32,
    /// Time scroll offset in seconds
    time_offset: f32,
    /// Pitch scroll offset in semitones (from the lowest visible note)
    pitch_offset: f32,
    /// Viewport rectangle for clipping
    viewport: egui::Rect,
}

impl NotePositioning {
    pub fn new(
        time_zoom: f32,
        pitch_zoom: f32,
        time_offset: f32,
        pitch_offset: f32,
        viewport: egui::Rect,
    ) -> Self {
        Self {
            time_zoom,
            pitch_zoom,
            time_offset,
            pitch_offset,
            viewport,
        }
    }

    /// Convert note properties to screen rect
    pub fn note_to_rect(&self, time: f64, pitch: u8, duration: f64) -> egui::Rect {
        let x = self.viewport.left() + (time as f32 * self.time_zoom - self.time_offset);
        let y = self.viewport.bottom() - (pitch as f32 * self.pitch_zoom - self.pitch_offset);
        let width = duration as f32 * self.time_zoom;
        let height = self.pitch_zoom;

        egui::Rect::from_min_size(
            egui::pos2(x, y - height), // Subtract height since we render from top-down
            egui::vec2(width, height),
        )
    }

    /// Convert screen coordinates back to musical time
    pub fn pos_to_time(&self, pos: egui::Pos2) -> f64 {
        ((pos.x - self.viewport.left() + self.time_offset) / self.time_zoom) as f64
    }

    /// Convert screen coordinates to MIDI pitch
    pub fn pos_to_pitch(&self, pos: egui::Pos2) -> u8 {
        ((self.viewport.bottom() - pos.y + self.pitch_offset) / self.pitch_zoom) as u8
    }

    /// Check if note would be visible in viewport
    pub fn is_note_visible(&self, time: f64, pitch: u8, duration: f64) -> bool {
        let rect = self.note_to_rect(time, pitch, duration);
        self.viewport.intersects(rect)
    }
}
