use crate::core::SnapMode;
use eframe::egui;

pub struct TimeUtils {}

impl TimeUtils {}

pub fn hex_to_color32(hex: &str) -> Option<egui::Color32> {
    let hex = hex.trim_start_matches('#');
    if hex.len() != 6 {
        return None;
    }
    
    let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
    let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
    let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
    
    Some(egui::Color32::from_rgb(r, g, b))
}

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

/// Handles smooth snapping with accumulator to prevent jumpiness
pub struct SnapHandler {
    accumulator: f32,
    threshold: f32,
}

impl SnapHandler {
    pub fn new(threshold: f32) -> Self {
        Self {
            accumulator: 0.0,
            threshold,
        }
    }
    
    /// Reset the accumulator (call on drag start)
    pub fn reset(&mut self) {
        self.accumulator = 0.0;
    }
    
    /// Add delta to accumulator
    pub fn add_delta(&mut self, delta: f32) {
        self.accumulator += delta;
    }
    
    /// Get accumulated value
    pub fn get_accumulated(&self) -> f32 {
        self.accumulator
    }
    
    /// Check if we should apply snapping based on threshold
    pub fn should_snap(&self) -> bool {
        self.accumulator.abs() > self.threshold
    }
    
    /// Apply snapping to a time value with accumulator logic
    pub fn snap_time_accumulated(
        &self,
        initial_time: f64,
        delta_time: f64,
        bpm: f64,
        snap_mode: SnapMode,
        snap_enabled: bool,
    ) -> f64 {
        let proposed_time = initial_time + delta_time;
        
        if snap_enabled && self.should_snap() {
            TimeUtils::snap_time(proposed_time, bpm, snap_mode)
        } else {
            proposed_time
        }
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



pub struct ViewportPosition {
    /// Pixels per second for time axis
    pixels_per_second: f32,
    /// Horizontal scroll offset in seconds
    scroll_offset: f32,
    /// Viewport rectangle for clipping
    viewport: egui::Rect,
}

impl ViewportPosition {
    pub fn new(pixels_per_second: f32, scroll_offset: f32, viewport: egui::Rect) -> Self {
        Self {
            pixels_per_second,
            scroll_offset,
            viewport,
        }
    }

    /// Convert time to screen X coordinate
    pub fn time_to_x(&self, time: f64) -> f32 {
        self.viewport.left() + (time as f32 * self.pixels_per_second - self.scroll_offset)
    }

    /// Convert screen X coordinate to time
    pub fn x_to_time(&self, x: f32) -> f64 {
        ((x - self.viewport.left() + self.scroll_offset) / self.pixels_per_second) as f64
    }

    /// Convert time duration to screen width
    pub fn duration_to_width(&self, duration: f64) -> f32 {
        duration as f32 * self.pixels_per_second
    }

    /// Check if time range would be visible in viewport
    pub fn is_time_visible(&self, start_time: f64, duration: f64) -> bool {
        let start_x = self.time_to_x(start_time);
        let width = self.duration_to_width(duration);
        let rect = egui::Rect::from_min_size(
            egui::pos2(start_x, self.viewport.top()),
            egui::vec2(width, self.viewport.height()),
        );
        self.viewport.intersects(rect)
    }

    /// Get visible time range
    pub fn visible_time_range(&self) -> (f64, f64) {
        let start_time = self.x_to_time(self.viewport.left());
        let end_time = self.x_to_time(self.viewport.right());
        (start_time, end_time)
    }

    /// Get zoom level
    pub fn get_pixels_per_second(&self) -> f32 {
        self.pixels_per_second
    }

    /// Get scroll offset
    pub fn get_scroll_offset(&self) -> f32 {
        self.scroll_offset
    }
}