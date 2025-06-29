use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AutomationPoint {
    pub id: String,
    pub time: f64,    // Time in seconds
    pub value: f64,   // Normalized 0.0 to 1.0
    pub curve_type: CurveType,
    #[serde(default)]
    pub tension: f32, // For bezier curves
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum CurveType {
    Linear,
    Bezier,
    Step,
    Exponential,
    Logarithmic,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomationLane {
    pub id: String,
    pub parameter: AutomationParameter,
    pub points: Vec<AutomationPoint>,
    pub visible: bool,
    pub height: f32,
    pub color: [f32; 3],
    pub min_value: f64,
    pub max_value: f64,
    pub default_value: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AutomationParameter {
    // MIDI CC parameters
    MidiCC { cc_number: u8, name: String },
    // Note parameters
    Velocity,
    PitchBend,
    // Track parameters
    Volume,
    Pan,
    // Plugin parameters (future)
    PluginParam { plugin_id: String, param_id: String, name: String },
}

impl AutomationParameter {
    pub fn display_name(&self) -> String {
        match self {
            AutomationParameter::MidiCC { cc_number, name } => {
                format!("CC{} - {}", cc_number, name)
            }
            AutomationParameter::Velocity => "Velocity".to_string(),
            AutomationParameter::PitchBend => "Pitch Bend".to_string(),
            AutomationParameter::Volume => "Volume".to_string(),
            AutomationParameter::Pan => "Pan".to_string(),
            AutomationParameter::PluginParam { name, .. } => name.clone(),
        }
    }

    pub fn default_color(&self) -> [f32; 3] {
        match self {
            AutomationParameter::MidiCC { cc_number, .. } => {
                // Generate color based on CC number
                let hue = (*cc_number as f32 / 127.0) * 360.0;
                hsv_to_rgb(hue, 0.7, 0.8)
            }
            AutomationParameter::Velocity => [0.8, 0.2, 0.2],
            AutomationParameter::PitchBend => [0.2, 0.8, 0.2],
            AutomationParameter::Volume => [0.2, 0.2, 0.8],
            AutomationParameter::Pan => [0.8, 0.8, 0.2],
            AutomationParameter::PluginParam { .. } => [0.5, 0.5, 0.8],
        }
    }
}

impl AutomationLane {
    pub fn new(parameter: AutomationParameter) -> Self {
        let (min, max, default) = match &parameter {
            AutomationParameter::MidiCC { .. } => (0.0, 127.0, 64.0),
            AutomationParameter::Velocity => (0.0, 127.0, 80.0),
            AutomationParameter::PitchBend => (-8192.0, 8191.0, 0.0),
            AutomationParameter::Volume => (0.0, 1.0, 0.8),
            AutomationParameter::Pan => (-1.0, 1.0, 0.0),
            AutomationParameter::PluginParam { .. } => (0.0, 1.0, 0.5),
        };

        Self {
            id: Uuid::new_v4().to_string(),
            parameter: parameter.clone(),
            points: Vec::new(),
            visible: true,
            height: 80.0,
            color: parameter.default_color(),
            min_value: min,
            max_value: max,
            default_value: default,
        }
    }

    pub fn add_point(&mut self, time: f64, value: f64) -> String {
        let point = AutomationPoint {
            id: Uuid::new_v4().to_string(),
            time,
            value: value.clamp(self.min_value, self.max_value),
            curve_type: CurveType::Linear,
            tension: 0.5,
        };
        let id = point.id.clone();
        self.points.push(point);
        self.sort_points();
        id
    }

    pub fn remove_point(&mut self, point_id: &str) {
        self.points.retain(|p| p.id != point_id);
    }

    pub fn update_point(&mut self, point_id: &str, time: Option<f64>, value: Option<f64>) {
        if let Some(point) = self.points.iter_mut().find(|p| p.id == point_id) {
            if let Some(t) = time {
                point.time = t;
            }
            if let Some(v) = value {
                point.value = v.clamp(self.min_value, self.max_value);
            }
        }
        self.sort_points();
    }

    pub fn get_value_at_time(&self, time: f64) -> f64 {
        if self.points.is_empty() {
            return self.default_value;
        }

        // Find surrounding points
        let mut prev_point = None;
        let mut next_point = None;

        for point in &self.points {
            if point.time <= time {
                prev_point = Some(point);
            } else {
                next_point = Some(point);
                break;
            }
        }

        match (prev_point, next_point) {
            (None, Some(next)) => next.value,
            (Some(prev), None) => prev.value,
            (Some(prev), Some(next)) => {
                self.interpolate_value(prev, next, time)
            }
            (None, None) => self.default_value,
        }
    }

    fn interpolate_value(&self, prev: &AutomationPoint, next: &AutomationPoint, time: f64) -> f64 {
        let t = (time - prev.time) / (next.time - prev.time);
        
        match prev.curve_type {
            CurveType::Linear => {
                prev.value + (next.value - prev.value) * t
            }
            CurveType::Step => {
                prev.value
            }
            CurveType::Bezier => {
                // Simple bezier interpolation
                let t2 = t * t;
                let t3 = t2 * t;
                let mt = 1.0 - t;
                let mt2 = mt * mt;
                let mt3 = mt2 * mt;
                
                // Using tension to control the curve
                let p1 = prev.value;
                let p2 = prev.value + (next.value - prev.value) * prev.tension as f64;
                let p3 = next.value - (next.value - prev.value) * prev.tension as f64;
                let p4 = next.value;
                
                mt3 * p1 + 3.0 * mt2 * t * p2 + 3.0 * mt * t2 * p3 + t3 * p4
            }
            CurveType::Exponential => {
                prev.value + (next.value - prev.value) * (t * t)
            }
            CurveType::Logarithmic => {
                prev.value + (next.value - prev.value) * t.sqrt()
            }
        }
    }

    fn sort_points(&mut self) {
        self.points.sort_by(|a, b| a.time.partial_cmp(&b.time).unwrap());
    }

    pub fn clear_range(&mut self, start_time: f64, end_time: f64) {
        self.points.retain(|p| p.time < start_time || p.time > end_time);
    }

    pub fn get_points_in_range(&self, start_time: f64, end_time: f64) -> Vec<&AutomationPoint> {
        self.points
            .iter()
            .filter(|p| p.time >= start_time && p.time <= end_time)
            .collect()
    }
}

// Helper function to convert HSV to RGB
fn hsv_to_rgb(h: f32, s: f32, v: f32) -> [f32; 3] {
    let h = h / 60.0;
    let c = v * s;
    let x = c * (1.0 - ((h % 2.0) - 1.0).abs());
    let m = v - c;

    let (r, g, b) = if h < 1.0 {
        (c, x, 0.0)
    } else if h < 2.0 {
        (x, c, 0.0)
    } else if h < 3.0 {
        (0.0, c, x)
    } else if h < 4.0 {
        (0.0, x, c)
    } else if h < 5.0 {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };

    [r + m, g + m, b + m]
}

// Common MIDI CC definitions
pub fn common_midi_cc() -> Vec<(u8, &'static str)> {
    vec![
        (1, "Mod Wheel"),
        (2, "Breath"),
        (4, "Foot Controller"),
        (5, "Portamento Time"),
        (7, "Volume"),
        (10, "Pan"),
        (11, "Expression"),
        (64, "Sustain Pedal"),
        (65, "Portamento On/Off"),
        (66, "Sostenuto"),
        (67, "Soft Pedal"),
        (71, "Filter Resonance"),
        (74, "Filter Cutoff"),
        (91, "Reverb"),
        (93, "Chorus"),
        (94, "Delay"),
        (95, "Phaser"),
    ]
}