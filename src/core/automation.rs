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

// All MIDI CC definitions (0-127)
pub fn get_all_midi_cc() -> Vec<(u8, &'static str)> {
    vec![
        (0, "Bank Select MSB"),
        (1, "Mod Wheel"),
        (2, "Breath Controller"),
        (3, "Controller 3"),
        (4, "Foot Controller"),
        (5, "Portamento Time"),
        (6, "Data Entry MSB"),
        (7, "Volume"),
        (8, "Balance"),
        (9, "Controller 9"),
        (10, "Pan"),
        (11, "Expression"),
        (12, "Effect Control 1"),
        (13, "Effect Control 2"),
        (14, "Controller 14"),
        (15, "Controller 15"),
        (16, "General Purpose 1"),
        (17, "General Purpose 2"),
        (18, "General Purpose 3"),
        (19, "General Purpose 4"),
        (20, "Controller 20"),
        (21, "Controller 21"),
        (22, "Controller 22"),
        (23, "Controller 23"),
        (24, "Controller 24"),
        (25, "Controller 25"),
        (26, "Controller 26"),
        (27, "Controller 27"),
        (28, "Controller 28"),
        (29, "Controller 29"),
        (30, "Controller 30"),
        (31, "Controller 31"),
        (32, "Bank Select LSB"),
        (33, "Mod Wheel LSB"),
        (34, "Breath LSB"),
        (35, "Controller 3 LSB"),
        (36, "Foot Controller LSB"),
        (37, "Portamento Time LSB"),
        (38, "Data Entry LSB"),
        (39, "Volume LSB"),
        (40, "Balance LSB"),
        (41, "Controller 9 LSB"),
        (42, "Pan LSB"),
        (43, "Expression LSB"),
        (44, "Effect Control 1 LSB"),
        (45, "Effect Control 2 LSB"),
        (46, "Controller 14 LSB"),
        (47, "Controller 15 LSB"),
        (48, "General Purpose 1 LSB"),
        (49, "General Purpose 2 LSB"),
        (50, "General Purpose 3 LSB"),
        (51, "General Purpose 4 LSB"),
        (52, "Controller 20 LSB"),
        (53, "Controller 21 LSB"),
        (54, "Controller 22 LSB"),
        (55, "Controller 23 LSB"),
        (56, "Controller 24 LSB"),
        (57, "Controller 25 LSB"),
        (58, "Controller 26 LSB"),
        (59, "Controller 27 LSB"),
        (60, "Controller 28 LSB"),
        (61, "Controller 29 LSB"),
        (62, "Controller 30 LSB"),
        (63, "Controller 31 LSB"),
        (64, "Sustain Pedal"),
        (65, "Portamento On/Off"),
        (66, "Sostenuto"),
        (67, "Soft Pedal"),
        (68, "Legato Footswitch"),
        (69, "Hold 2"),
        (70, "Sound Controller 1"),
        (71, "Sound Controller 2 (Filter Resonance)"),
        (72, "Sound Controller 3 (Release Time)"),
        (73, "Sound Controller 4 (Attack Time)"),
        (74, "Sound Controller 5 (Filter Cutoff)"),
        (75, "Sound Controller 6 (Decay Time)"),
        (76, "Sound Controller 7 (Vibrato Rate)"),
        (77, "Sound Controller 8 (Vibrato Depth)"),
        (78, "Sound Controller 9 (Vibrato Delay)"),
        (79, "Sound Controller 10"),
        (80, "General Purpose 5"),
        (81, "General Purpose 6"),
        (82, "General Purpose 7"),
        (83, "General Purpose 8"),
        (84, "Portamento Control"),
        (85, "Controller 85"),
        (86, "Controller 86"),
        (87, "Controller 87"),
        (88, "High Resolution Velocity Prefix"),
        (89, "Controller 89"),
        (90, "Controller 90"),
        (91, "Reverb Send"),
        (92, "Effects 2 (Tremolo)"),
        (93, "Chorus Send"),
        (94, "Effects 4 (Delay)"),
        (95, "Effects 5 (Phaser)"),
        (96, "Data Increment"),
        (97, "Data Decrement"),
        (98, "NRPN LSB"),
        (99, "NRPN MSB"),
        (100, "RPN LSB"),
        (101, "RPN MSB"),
        (102, "Controller 102"),
        (103, "Controller 103"),
        (104, "Controller 104"),
        (105, "Controller 105"),
        (106, "Controller 106"),
        (107, "Controller 107"),
        (108, "Controller 108"),
        (109, "Controller 109"),
        (110, "Controller 110"),
        (111, "Controller 111"),
        (112, "Controller 112"),
        (113, "Controller 113"),
        (114, "Controller 114"),
        (115, "Controller 115"),
        (116, "Controller 116"),
        (117, "Controller 117"),
        (118, "Controller 118"),
        (119, "Controller 119"),
        (120, "All Sound Off"),
        (121, "Reset All Controllers"),
        (122, "Local Control On/Off"),
        (123, "All Notes Off"),
        (124, "Omni Mode Off"),
        (125, "Omni Mode On"),
        (126, "Mono Mode On"),
        (127, "Poly Mode On"),
    ]
}