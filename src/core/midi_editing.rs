use crate::core::{MidiEvent, MidiMessage};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuantizeSettings {
    pub grid: QuantizeGrid,
    pub strength: f32,     // 0.0 to 1.0
    pub swing: f32,        // -1.0 to 1.0
    pub humanize: f32,     // 0.0 to 1.0 - adds random timing variation
    pub preserve_flams: bool, // Don't quantize notes very close together
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum QuantizeGrid {
    Quarter,      // 1/4 note
    Eighth,       // 1/8 note
    Sixteenth,    // 1/16 note
    ThirtySecond, // 1/32 note
    EighthTriplet,    // 1/8 triplet
    SixteenthTriplet, // 1/16 triplet
    Dotted8th,    // Dotted 1/8 note
    Dotted16th,   // Dotted 1/16 note
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VelocityEditSettings {
    pub mode: VelocityEditMode,
    pub amount: f32,
    pub curve: VelocityCurve,
    pub randomize: f32, // 0.0 to 1.0 - adds random velocity variation
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum VelocityEditMode {
    Set,        // Set to fixed value
    Add,        // Add/subtract amount
    Scale,      // Multiply by factor
    Compress,   // Compress dynamic range
    Expand,     // Expand dynamic range
    Ramp,       // Linear ramp from start to end
    Curve,      // Apply curve shape
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum VelocityCurve {
    Linear,
    Exponential,
    Logarithmic,
    Sine,
    Cosine,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ControllerLane {
    pub controller: u8,     // CC number (0-127)
    pub name: String,       // Display name
    pub events: Vec<ControllerEvent>,
    pub visible: bool,
    pub height: f32,        // Lane height in pixels
    pub color: [f32; 3],    // RGB color
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ControllerEvent {
    pub id: String,
    pub time: f64,          // Time in seconds
    pub value: u8,          // Controller value (0-127)
    pub selected: bool,
}

#[derive(Debug, Clone)]
pub struct MidiEditor {
    pub velocity_settings: VelocityEditSettings,
    pub quantize_settings: QuantizeSettings,
    pub controller_lanes: HashMap<u8, ControllerLane>,
    pub show_velocity_lane: bool,
    pub velocity_lane_height: f32,
    pub selected_notes: Vec<String>, // Note IDs
    pub selected_controllers: Vec<String>, // Controller event IDs
}

impl Default for QuantizeSettings {
    fn default() -> Self {
        Self {
            grid: QuantizeGrid::Sixteenth,
            strength: 1.0,
            swing: 0.0,
            humanize: 0.0,
            preserve_flams: true,
        }
    }
}

impl Default for VelocityEditSettings {
    fn default() -> Self {
        Self {
            mode: VelocityEditMode::Set,
            amount: 80.0,
            curve: VelocityCurve::Linear,
            randomize: 0.0,
        }
    }
}

impl Default for MidiEditor {
    fn default() -> Self {
        let mut controller_lanes = HashMap::new();
        
        // Add common controller lanes
        controller_lanes.insert(1, ControllerLane {
            controller: 1,
            name: "Mod Wheel".to_string(),
            events: Vec::new(),
            visible: false,
            height: 60.0,
            color: [0.2, 0.8, 0.2],
        });
        
        controller_lanes.insert(7, ControllerLane {
            controller: 7,
            name: "Volume".to_string(),
            events: Vec::new(),
            visible: false,
            height: 60.0,
            color: [0.8, 0.2, 0.2],
        });
        
        controller_lanes.insert(10, ControllerLane {
            controller: 10,
            name: "Pan".to_string(),
            events: Vec::new(),
            visible: false,
            height: 60.0,
            color: [0.2, 0.2, 0.8],
        });
        
        controller_lanes.insert(11, ControllerLane {
            controller: 11,
            name: "Expression".to_string(),
            events: Vec::new(),
            visible: false,
            height: 60.0,
            color: [0.8, 0.8, 0.2],
        });

        Self {
            velocity_settings: VelocityEditSettings::default(),
            quantize_settings: QuantizeSettings::default(),
            controller_lanes,
            show_velocity_lane: true,
            velocity_lane_height: 80.0,
            selected_notes: Vec::new(),
            selected_controllers: Vec::new(),
        }
    }
}

impl MidiEditor {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn quantize_events(&self, events: &mut [MidiEvent], bpm: f64) {
        let grid_size = self.get_grid_size(bpm);
        let swing_offset = grid_size * self.quantize_settings.swing as f64 * 0.1;
        
        for event in events.iter_mut() {
            if let MidiMessage::NoteOn { .. } = event.message {
                // Calculate quantized time
                let beat_position = event.time / grid_size;
                let quantized_beat = beat_position.round();
                let quantized_time = quantized_beat * grid_size;
                
                // Apply swing on off-beats
                let swing_time = if (quantized_beat as i32) % 2 == 1 {
                    quantized_time + swing_offset
                } else {
                    quantized_time
                };
                
                // Apply humanization (random timing variation)
                let humanize_offset = if self.quantize_settings.humanize > 0.0 {
                    let max_offset = grid_size * 0.1 * self.quantize_settings.humanize as f64;
                    (rand::random::<f64>() - 0.5) * 2.0 * max_offset
                } else {
                    0.0
                };
                
                // Blend between original and quantized time based on strength
                let target_time = swing_time + humanize_offset;
                event.time = event.time + (target_time - event.time) * self.quantize_settings.strength as f64;
            }
        }
    }

    pub fn edit_velocities(&self, events: &mut [MidiEvent], selection_start: f64, selection_end: f64) {
        let selected_events: Vec<_> = events.iter_mut()
            .filter(|e| {
                if let MidiMessage::NoteOn { .. } = e.message {
                    e.time >= selection_start && e.time <= selection_end
                } else {
                    false
                }
            })
            .collect();

        let event_count = selected_events.len();
        if event_count == 0 {
            return;
        }

        for (index, event) in selected_events.into_iter().enumerate() {
            if let MidiMessage::NoteOn { velocity, .. } = &mut event.message {
                let new_velocity = match self.velocity_settings.mode {
                    VelocityEditMode::Set => {
                        self.velocity_settings.amount as u8
                    }
                    VelocityEditMode::Add => {
                        (*velocity as f32 + self.velocity_settings.amount).clamp(1.0, 127.0) as u8
                    }
                    VelocityEditMode::Scale => {
                        (*velocity as f32 * self.velocity_settings.amount / 100.0).clamp(1.0, 127.0) as u8
                    }
                    VelocityEditMode::Compress => {
                        let center = 64.0;
                        let diff = *velocity as f32 - center;
                        (center + diff * (1.0 - self.velocity_settings.amount / 100.0)).clamp(1.0, 127.0) as u8
                    }
                    VelocityEditMode::Expand => {
                        let center = 64.0;
                        let diff = *velocity as f32 - center;
                        (center + diff * (1.0 + self.velocity_settings.amount / 100.0)).clamp(1.0, 127.0) as u8
                    }
                    VelocityEditMode::Ramp => {
                        let progress = if event_count > 1 {
                            index as f32 / (event_count - 1) as f32
                        } else {
                            0.0
                        };
                        let start_vel = 1.0;
                        let end_vel = self.velocity_settings.amount;
                        (start_vel + (end_vel - start_vel) * progress).clamp(1.0, 127.0) as u8
                    }
                    VelocityEditMode::Curve => {
                        let progress = if event_count > 1 {
                            index as f32 / (event_count - 1) as f32
                        } else {
                            0.0
                        };
                        let curve_value = self.apply_curve(progress);
                        (curve_value * self.velocity_settings.amount).clamp(1.0, 127.0) as u8
                    }
                };

                // Apply randomization
                let final_velocity = if self.velocity_settings.randomize > 0.0 {
                    let random_offset = (rand::random::<f32>() - 0.5) * 2.0 * 
                        self.velocity_settings.randomize * 20.0; // Max ±20 velocity units
                    (new_velocity as f32 + random_offset).clamp(1.0, 127.0) as u8
                } else {
                    new_velocity
                };

                *velocity = final_velocity;
            }
        }
    }

    pub fn add_controller_event(&mut self, controller: u8, time: f64, value: u8) {
        if let Some(lane) = self.controller_lanes.get_mut(&controller) {
            let event = ControllerEvent {
                id: Uuid::new_v4().to_string(),
                time,
                value,
                selected: false,
            };
            lane.events.push(event);
            lane.events.sort_by(|a, b| a.time.partial_cmp(&b.time).unwrap());
        }
    }

    pub fn remove_controller_event(&mut self, controller: u8, event_id: &str) {
        if let Some(lane) = self.controller_lanes.get_mut(&controller) {
            lane.events.retain(|e| e.id != event_id);
        }
    }

    pub fn get_controller_value_at_time(&self, controller: u8, time: f64) -> Option<u8> {
        if let Some(lane) = self.controller_lanes.get(&controller) {
            // Find the most recent event before or at the given time
            lane.events.iter()
                .filter(|e| e.time <= time)
                .last()
                .map(|e| e.value)
        } else {
            None
        }
    }

    pub fn interpolate_controller_values(&mut self, controller: u8, start_time: f64, end_time: f64, start_value: u8, end_value: u8, steps: usize) {
        if let Some(lane) = self.controller_lanes.get_mut(&controller) {
            // Remove existing events in the range
            lane.events.retain(|e| e.time < start_time || e.time > end_time);
            
            // Add interpolated events
            for i in 0..=steps {
                let progress = if steps > 0 { i as f64 / steps as f64 } else { 0.0 };
                let time = start_time + (end_time - start_time) * progress;
                let value = (start_value as f64 + (end_value as f64 - start_value as f64) * progress) as u8;
                
                let event = ControllerEvent {
                    id: Uuid::new_v4().to_string(),
                    time,
                    value,
                    selected: false,
                };
                lane.events.push(event);
            }
            
            lane.events.sort_by(|a, b| a.time.partial_cmp(&b.time).unwrap());
        }
    }

    pub fn toggle_controller_lane(&mut self, controller: u8) {
        if let Some(lane) = self.controller_lanes.get_mut(&controller) {
            lane.visible = !lane.visible;
        }
    }

    pub fn add_custom_controller_lane(&mut self, controller: u8, name: String, color: [f32; 3]) {
        let lane = ControllerLane {
            controller,
            name,
            events: Vec::new(),
            visible: true,
            height: 60.0,
            color,
        };
        self.controller_lanes.insert(controller, lane);
    }

    fn get_grid_size(&self, bpm: f64) -> f64 {
        let beat_duration = 60.0 / bpm;
        match self.quantize_settings.grid {
            QuantizeGrid::Quarter => beat_duration,
            QuantizeGrid::Eighth => beat_duration * 0.5,
            QuantizeGrid::Sixteenth => beat_duration * 0.25,
            QuantizeGrid::ThirtySecond => beat_duration * 0.125,
            QuantizeGrid::EighthTriplet => beat_duration / 3.0,
            QuantizeGrid::SixteenthTriplet => beat_duration / 6.0,
            QuantizeGrid::Dotted8th => beat_duration * 0.75,
            QuantizeGrid::Dotted16th => beat_duration * 0.375,
        }
    }

    fn apply_curve(&self, progress: f32) -> f32 {
        match self.velocity_settings.curve {
            VelocityCurve::Linear => progress,
            VelocityCurve::Exponential => progress * progress,
            VelocityCurve::Logarithmic => progress.sqrt(),
            VelocityCurve::Sine => (progress * std::f32::consts::PI * 0.5).sin(),
            VelocityCurve::Cosine => 1.0 - (progress * std::f32::consts::PI * 0.5).cos(),
        }
    }
}

impl QuantizeGrid {
    pub fn display_name(&self) -> &'static str {
        match self {
            QuantizeGrid::Quarter => "1/4",
            QuantizeGrid::Eighth => "1/8",
            QuantizeGrid::Sixteenth => "1/16",
            QuantizeGrid::ThirtySecond => "1/32",
            QuantizeGrid::EighthTriplet => "1/8T",
            QuantizeGrid::SixteenthTriplet => "1/16T",
            QuantizeGrid::Dotted8th => "1/8.",
            QuantizeGrid::Dotted16th => "1/16.",
        }
    }
}

impl VelocityEditMode {
    pub fn display_name(&self) -> &'static str {
        match self {
            VelocityEditMode::Set => "Set",
            VelocityEditMode::Add => "Add",
            VelocityEditMode::Scale => "Scale",
            VelocityEditMode::Compress => "Compress",
            VelocityEditMode::Expand => "Expand",
            VelocityEditMode::Ramp => "Ramp",
            VelocityEditMode::Curve => "Curve",
        }
    }
}

impl VelocityCurve {
    pub fn display_name(&self) -> &'static str {
        match self {
            VelocityCurve::Linear => "Linear",
            VelocityCurve::Exponential => "Exponential",
            VelocityCurve::Logarithmic => "Logarithmic",
            VelocityCurve::Sine => "Sine",
            VelocityCurve::Cosine => "Cosine",
        }
    }
} 