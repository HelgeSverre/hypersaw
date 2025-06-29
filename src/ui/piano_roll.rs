use crate::core::*;
use eframe::egui;
use egui::{FontId, StrokeKind};

const MIDDLE_C: i32 = 60; // MIDI note number for middle C
const DEFAULT_OCTAVES: i32 = 8; // Number of octaves to show
const NOTES_PER_OCTAVE: i32 = 12;

pub struct PianoRoll {
    key_width: f32,
    key_height: f32,
    grid_snap: bool,
    zoom: f32,
    scroll_x: f32,
    scroll_y: f32,
    viewport_height: f32,
    selected_notes: Vec<EventID>,
    dragging: Option<DragOperation>,
    command_collector: CommandCollector,
    // Automation panel
    automation_panel_height: f32,
    automation_lanes: Vec<AutomationLane>,
    selected_automation_points: Vec<(String, String)>, // (lane_id, point_id)
    automation_scroll_y: f32,
    resizing_divider: bool,
    // UI state
    show_automation: bool,
    // Drag state
    drag_accumulator: f32,
    resize_initial_values: Option<(f64, f64)>, // (start_time, duration)
    drag_initial_positions: Option<Vec<(String, f64, u8)>>, // Vec<(note_id, start_time, pitch)>
    drag_accumulator_x: f32,
    drag_accumulator_y: f32,
    last_applied_delta_time: f64,
    last_applied_delta_pitch: i8,
}

#[derive(Debug)]
enum DragOperation {
    MovingNotes { start_x: f32, start_y: f32 },
    ResizingNotes { edge: ResizeEdge, start_x: f32 },
    Drawing { start_x: f32, start_y: f32 },
    MovingAutomationPoint { lane_id: String, point_id: String, start_x: f32, start_y: f32 },
    DrawingAutomation { lane_id: String, start_x: f32, start_y: f32 },
}

#[derive(Debug, Clone, Copy)]
enum ResizeEdge {
    Left,
    Right,
}

// Helper struct for note positioning calculations
struct NotePositioning {
    zoom: f32,
    key_height: f32,
    scroll_x: f32,
    scroll_y: f32,
    note_area: egui::Rect,
}

impl NotePositioning {
    fn new(zoom: f32, key_height: f32, scroll_x: f32, scroll_y: f32, note_area: egui::Rect) -> Self {
        Self { zoom, key_height, scroll_x, scroll_y, note_area }
    }
    
    fn note_to_rect(&self, start_time: f64, key: u8, duration: f64) -> egui::Rect {
        let x_start = self.note_area.left() + (start_time as f32 * self.zoom) - self.scroll_x;
        let x_end = self.note_area.left() + ((start_time + duration) as f32 * self.zoom) - self.scroll_x;
        let y = self.note_area.bottom() - (key as f32 + 1.0) * self.key_height + self.scroll_y;
        
        egui::Rect::from_min_max(
            egui::pos2(x_start, y),
            egui::pos2(x_end, y + self.key_height),
        )
    }
    
    fn is_note_visible(&self, start_time: f64, key: u8, duration: f64) -> bool {
        let note_rect = self.note_to_rect(start_time, key, duration);
        note_rect.intersects(self.note_area)
    }
}

impl PianoRoll {
    pub fn default() -> Self {
        let mut automation_lanes = Vec::new();
        
        // Add default velocity lane
        let mut velocity_lane = AutomationLane::new(AutomationParameter::Velocity);
        velocity_lane.visible = true;
        automation_lanes.push(velocity_lane);
        
        // Add common MIDI CC lanes (hidden by default)
        for (cc, name) in common_midi_cc().into_iter().take(4) {
            let mut lane = AutomationLane::new(AutomationParameter::MidiCC {
                cc_number: cc,
                name: name.to_string(),
            });
            lane.visible = false;
            automation_lanes.push(lane);
        }
        
        Self {
            key_width: 80.0,
            key_height: 20.0,
            grid_snap: true,
            zoom: 100.0,
            scroll_x: 0.0,
            scroll_y: 0.0,
            viewport_height: 0.0,
            selected_notes: Vec::new(),
            dragging: None,
            command_collector: CommandCollector::new(),
            automation_panel_height: 200.0,
            automation_lanes,
            selected_automation_points: Vec::new(),
            automation_scroll_y: 0.0,
            resizing_divider: false,
            show_automation: true,
            drag_accumulator: 0.0,
            resize_initial_values: None,
            drag_initial_positions: None,
            drag_accumulator_x: 0.0,
            drag_accumulator_y: 0.0,
            last_applied_delta_time: 0.0,
            last_applied_delta_pitch: 0,
        }
    }
    fn get_active_notes(
        &self,
        state: &DawState,
        clip_id: &str,
        track_id: &str,
        current_time: f64,
    ) -> Vec<u8> {
        let mut active_notes = Vec::new();

        if let Some(track) = state.project.tracks.iter().find(|t| &t.id == track_id) {
            if let Some(Clip::Midi {
                midi_data,
                start_time,
                ..
            }) = track
                .clips
                .iter()
                .find(|c| matches!(c, Clip::Midi { id, .. } if id == clip_id))
            {
                if let Some(store) = midi_data {
                    // Get relative time within the clip
                    let clip_time = current_time - start_time;

                    // Find all notes that contain the current time point
                    for note in store.get_notes() {
                        let note_end = note.start_time + note.duration;
                        if clip_time >= note.start_time && clip_time < note_end {
                            active_notes.push(note.key);
                        }
                    }
                }
            }
        }

        active_notes
    }
    pub fn show(&mut self, ui: &mut egui::Ui, state: &mut DawState) -> Vec<DawCommand> {
        if let EditorView::PianoRoll {
            clip_id, track_id, ..
        } = &state.current_view
        {
            // TODO: move into the project.rs - track struct
            // Load MIDI data if needed
            if let Some(track) = state.project.tracks.iter_mut().find(|t| &t.id == track_id) {
                if let Some(clip @ Clip::Midi { loaded: false, .. }) = track
                    .clips
                    .iter_mut()
                    .find(|c| matches!(c, Clip::Midi { id, .. } if id == clip_id))
                {
                    if let Err(e) = clip.load_midi() {
                        state
                            .status
                            .error(format!("Failed to load MIDI data: {}", e));
                    }
                }
            }

            // Get clip start time
            let clip_start =
                if let Some(track) = state.project.tracks.iter().find(|t| &t.id == track_id) {
                    if let Some(Clip::Midi { start_time, .. }) = track
                        .clips
                        .iter()
                        .find(|c| matches!(c, Clip::Midi { id, .. } if id == clip_id))
                    {
                        *start_time
                    } else {
                        0.0
                    }
                } else {
                    0.0
                };

            let full_rect = ui.available_rect_before_wrap();
            
            // Calculate rects for piano roll and automation
            let divider_height = 4.0;
            let min_panel_height = 50.0;
            
            let effective_automation_height = if self.show_automation {
                self.automation_panel_height.clamp(min_panel_height, full_rect.height() - min_panel_height - divider_height)
            } else {
                0.0
            };
            
            let piano_roll_rect = egui::Rect::from_min_size(
                full_rect.min,
                egui::vec2(full_rect.width(), full_rect.height() - effective_automation_height - (if self.show_automation { divider_height } else { 0.0 })),
            );
            
            let divider_rect = if self.show_automation {
                egui::Rect::from_min_size(
                    egui::pos2(full_rect.left(), piano_roll_rect.bottom()),
                    egui::vec2(full_rect.width(), divider_height),
                )
            } else {
                egui::Rect::NOTHING
            };
            
            let automation_rect = if self.show_automation {
                egui::Rect::from_min_size(
                    egui::pos2(full_rect.left(), divider_rect.bottom()),
                    egui::vec2(full_rect.width(), effective_automation_height),
                )
            } else {
                egui::Rect::NOTHING
            };

            // Draw piano roll in its rect
            ui.allocate_new_ui(egui::UiBuilder::new().max_rect(piano_roll_rect), |ui| {
                let (rect, response) =
                    ui.allocate_exact_size(ui.available_size(), egui::Sense::click_and_drag());

                self.center_on_middle_c(rect.height());
                self.draw_grid(ui, rect, state);
                
                // Handle note area interactions before drawing notes
                self.handle_note_area_interaction(ui, rect, clip_id, track_id, state, &response);
                
                self.draw_notes(ui, rect, clip_id, track_id, state);
                self.draw_piano_keys(ui, rect, state, clip_id, track_id);

                // Draw playhead after everything else
                self.draw_playhead(ui, rect, clip_start, state.current_time);

                // Handle zoom and scrolling
                self.handle_zoom(ui, rect);

                // Handle middle-button dragging for panning
                if response.dragged() && !self.resizing_divider {
                    // Only pan if we're not drawing or have another drag operation
                    if self.dragging.is_none() {
                        // Horizontal scroll
                        let invert = -1.0;
                        let delta = response.drag_delta();
                        self.scroll_x = (self.scroll_x + delta.x * invert).max(0.0);

                        // Vertical scroll
                        let new_scroll_y = (self.scroll_y + delta.y).max(0.0);
                        self.scroll_y =
                            new_scroll_y.clamp(0.0, self.get_total_height() - self.viewport_height);
                    }
                }
            });

            // Draw resizable divider
            if self.show_automation {
                self.draw_divider(ui, divider_rect);
            }

            // Draw automation panel
            if self.show_automation {
                ui.allocate_new_ui(egui::UiBuilder::new().max_rect(automation_rect), |ui| {
                    self.draw_automation_panel(ui, automation_rect, clip_id, track_id, state);
                });
            }

            // Handle keyboard shortcuts
            ui.input(|i| {
                // Delete key - delete selected notes
                if i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace) {
                    if !self.selected_notes.is_empty() {
                        self.command_collector.add_command(DawCommand::DeleteNotes {
                            clip_id: clip_id.to_string(),
                            note_ids: self.selected_notes.clone(),
                        });
                        self.selected_notes.clear();
                    }
                    
                    // TODO: Also handle deleting automation points
                }
                
                // Ctrl+A - Select all notes
                if i.key_pressed(egui::Key::A) && (i.modifiers.ctrl || i.modifiers.command) {
                    self.selected_notes.clear();
                    // Get all notes in the clip
                    if let Some(track) = state.project.tracks.iter().find(|t| &t.id == track_id) {
                        if let Some(Clip::Midi { midi_data, .. }) = track.clips.iter()
                            .find(|c| matches!(c, Clip::Midi { id, .. } if id == clip_id))
                        {
                            if let Some(store) = midi_data {
                                for note in store.get_notes() {
                                    self.selected_notes.push(note.id.clone());
                                }
                            }
                        }
                    }
                }
                
                // Escape - Clear selection
                if i.key_pressed(egui::Key::Escape) {
                    self.selected_notes.clear();
                    self.selected_automation_points.clear();
                }
            });

            // Auto-scroll to follow playhead if it's outside view
            // self.handle_playhead_autoscroll(rect, clip_start, state.current_time);
        }

        self.command_collector.take_commands()
    }

    fn handle_scrolling(&mut self, ui: &egui::Ui, rect: egui::Rect) {
        ui.input(|i| {
            if i.modifiers.shift {
                // Horizontal scroll with shift
                let scroll_delta = i.raw_scroll_delta.x;
                println!("Horizontal scroll with shift {}", scroll_delta);
                self.scroll_x = (self.scroll_x + scroll_delta).max(0.0);
            } else {
                // Vertical scroll without shift
                let scroll_delta = i.raw_scroll_delta.y;
                println!("Vertical scroll without shift {}", scroll_delta);
                let new_scroll_y = self.scroll_y + scroll_delta;
                self.scroll_y =
                    new_scroll_y.clamp(0.0, self.get_total_height() - self.viewport_height);
            }
        });
    }

    fn handle_zoom(&mut self, ui: &egui::Ui, rect: egui::Rect) {
        ui.input(|i| {
            if i.modifiers.ctrl || i.modifiers.command {
                if let Some(mouse_pos) = i.pointer.hover_pos() {
                    // Calculate time at mouse position before zoom
                    let time_at_mouse =
                        (mouse_pos.x - rect.left() - self.key_width + self.scroll_x) / self.zoom;
                    let pitch_at_mouse =
                        ((rect.bottom() - mouse_pos.y + self.scroll_y) / self.key_height).floor();

                    let zoom_delta = i.raw_scroll_delta.y / 100.0;
                    self.zoom = (self.zoom * (1.0 + zoom_delta)).clamp(20.0, 500.0);

                    // Adjust scroll to maintain mouse position
                    let new_mouse_x = time_at_mouse * self.zoom;
                    self.scroll_x = new_mouse_x - (mouse_pos.x - rect.left() - self.key_width);
                }
            }
        });
    }

    fn handle_note_drag(
        &mut self,
        response: &egui::Response,
        note: &Note,
        clip_id: &str,
        state: &DawState,
    ) {
        const DRAG_THRESHOLD: f32 = 3.0;  // Pixels before drag starts
        const PITCH_DRAG_THRESHOLD: f32 = 0.5;  // Half a key height before pitch changes

        // Initialize drag state when starting
        if response.drag_started() {
            // Ensure the note being dragged is selected
            if !self.selected_notes.contains(&note.id) {
                self.selected_notes.push(note.id.clone());
            }

            // Store initial positions of all selected notes
            let mut initial_positions = Vec::new();
            if let Some(track) = state.project.tracks.iter().find(|t| 
                t.clips.iter().any(|c| matches!(c, Clip::Midi { id, .. } if id == clip_id))
            ) {
                if let Some(Clip::Midi { midi_data, .. }) = track.clips.iter()
                    .find(|c| matches!(c, Clip::Midi { id, .. } if id == clip_id))
                {
                    if let Some(store) = midi_data {
                        for selected_id in &self.selected_notes {
                            // Find the note by iterating through all notes
                            for note in store.get_notes() {
                                if &note.id == selected_id {
                                    initial_positions.push((
                                        selected_id.clone(),
                                        note.start_time,
                                        note.key,
                                    ));
                                    break;
                                }
                            }
                        }
                    }
                }
            }
            
            self.drag_initial_positions = Some(initial_positions);
            self.drag_accumulator_x = 0.0;
            self.drag_accumulator_y = 0.0;
            self.last_applied_delta_time = 0.0;
            self.last_applied_delta_pitch = 0;
            self.dragging = Some(DragOperation::MovingNotes {
                start_x: response.interact_pointer_pos().unwrap_or_default().x,
                start_y: response.interact_pointer_pos().unwrap_or_default().y,
            });
        }

        // Handle dragging
        if response.dragged() {
            if let Some(ref initial_positions) = self.drag_initial_positions {
                // Accumulate drag deltas
                self.drag_accumulator_x += response.drag_delta().x;
                self.drag_accumulator_y += response.drag_delta().y;

                // Only process if we've exceeded the threshold
                if self.drag_accumulator_x.abs() >= DRAG_THRESHOLD || 
                   (self.drag_accumulator_y.abs() / self.key_height) >= PITCH_DRAG_THRESHOLD {
                    
                    // Convert accumulated pixel delta to time and pitch deltas from initial position
                    let accumulated_time_delta = self.drag_accumulator_x / self.zoom;
                    let accumulated_pitch_delta = -(self.drag_accumulator_y / self.key_height).round() as i8;

                    // Apply snapping less aggressively (only when accumulated drag is significant)
                    let total_delta_time = if self.grid_snap && self.drag_accumulator_x.abs() > 10.0 {
                        // Find the first note's initial position to use as reference
                        if let Some((_, initial_time, _)) = initial_positions.first() {
                            let new_time = TimeUtils::snap_time(
                                initial_time + accumulated_time_delta as f64,
                                state.project.bpm,
                                state.snap_mode,
                            );
                            new_time - initial_time
                        } else {
                            accumulated_time_delta as f64
                        }
                    } else {
                        accumulated_time_delta as f64
                    };

                    // Calculate incremental delta from last applied position
                    let incremental_delta_time = total_delta_time - self.last_applied_delta_time;
                    let incremental_delta_pitch = accumulated_pitch_delta - self.last_applied_delta_pitch;

                    // Only send command if there's an actual change
                    if incremental_delta_time.abs() > 0.001 || incremental_delta_pitch != 0 {
                        self.command_collector.add_command(DawCommand::MoveNotes {
                            clip_id: clip_id.to_string(),
                            note_ids: self.selected_notes.clone(),
                            delta_time: incremental_delta_time,
                            delta_pitch: incremental_delta_pitch,
                        });

                        // Update last applied deltas
                        self.last_applied_delta_time = total_delta_time;
                        self.last_applied_delta_pitch = accumulated_pitch_delta;
                    }
                }
            }
        }

        // Clean up when drag ends
        if response.drag_stopped() {
            self.drag_initial_positions = None;
            self.drag_accumulator_x = 0.0;
            self.drag_accumulator_y = 0.0;
            self.last_applied_delta_time = 0.0;
            self.last_applied_delta_pitch = 0;
            self.dragging = None;
        }
    }

    fn draw_piano_keys(
        &self,
        ui: &mut egui::Ui,
        rect: egui::Rect,
        state: &DawState,
        clip_id: &str,
        track_id: &str,
    ) {
        let keys_rect = rect.intersect(egui::Rect::from_min_size(
            rect.min,
            egui::vec2(self.key_width, rect.height()),
        ));

        let start_note = (self.scroll_y / self.key_height).floor() as i32;
        let end_note = ((self.scroll_y + rect.height()) / self.key_height).ceil() as i32;
        let visible_notes = start_note..=end_note;

        // Get currently active notes
        let active_notes = self.get_active_notes(state, clip_id, track_id, state.current_time);

        // Draw background for piano keys
        ui.painter()
            .rect_filled(keys_rect, 0.0, ui.visuals().window_fill);

        // Draw white keys first
        for note_number in visible_notes.clone() {
            let note = note_number % 12;
            if [0, 2, 4, 5, 7, 9, 11].contains(&note) {
                self.draw_key(ui, note_number as u8, false, keys_rect, &active_notes);
            }
        }

        // Draw black keys on top
        for note_number in visible_notes {
            let note = note_number % 12;
            if [1, 3, 6, 8, 10].contains(&note) {
                self.draw_key(ui, note_number as u8, true, keys_rect, &active_notes);
            }
        }
    }

    fn draw_key(
        &self,
        ui: &mut egui::Ui,
        note_number: u8,
        is_black: bool,
        rect: egui::Rect,
        active_notes: &[u8],
    ) {
        let y = rect.bottom() - (note_number as f32 + 1.0) * self.key_height + self.scroll_y;

        // Simply adjust width for black keys, always start from left
        let key_width = if is_black {
            self.key_width * 0.6
        } else {
            self.key_width
        };

        let key_rect = egui::Rect::from_min_max(
            egui::pos2(rect.left(), y),
            egui::pos2(rect.left() + key_width, y + self.key_height),
        );

        // Check if note is currently active
        let is_active = active_notes.contains(&note_number);

        // Draw key background with active state
        let base_color = if is_black {
            ui.visuals().extreme_bg_color
        } else {
            ui.visuals().window_fill
        };

        let color = if is_active {
            // Create a highlighted version of the key color
            let highlight_color = egui::Color32::from_rgb(64, 128, 255);
            if is_black {
                highlight_color.linear_multiply(0.7)
            } else {
                highlight_color
            }
        } else {
            base_color
        };

        ui.painter().rect_filled(key_rect, 0.0, color);
        ui.painter().rect_stroke(
            key_rect,
            0.0,
            egui::Stroke::new(1.0, ui.visuals().window_stroke.color),
            StrokeKind::Outside,
        );

        let response = ui.allocate_rect(key_rect, egui::Sense::click());

        // Draw note name
        let note = note_number % 12;
        let octave = (note_number / 12) - 1;
        let note_names = [
            "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
        ];
        let note_name = format!("{}{}", note_names[note as usize], octave);

        if note == 0 || response.hovered() || is_active {
            let text_color = if is_active {
                if is_black {
                    ui.visuals().text_color()
                } else {
                    ui.visuals().strong_text_color()
                }
            } else if is_black {
                ui.visuals().strong_text_color()
            } else {
                ui.visuals().text_color()
            };

            let text_pos = key_rect.center();
            ui.painter().text(
                text_pos,
                egui::Align2::CENTER_CENTER,
                &note_name,
                FontId::monospace(10.0),
                text_color,
            );
        }
    }

    fn draw_grid(&self, ui: &mut egui::Ui, rect: egui::Rect, state: &DawState) {
        let grid_rect = egui::Rect::from_min_max(
            egui::pos2(rect.left() + self.key_width, rect.top()),
            rect.max,
        );

        let bpm = state.project.bpm;
        let beat_duration = 60.0 / bpm;
        let bar_duration = beat_duration * 4.0;

        let pixels_per_beat = self.zoom;
        let pixels_per_bar = pixels_per_beat * 4.0;

        let start_bar = (self.scroll_x / pixels_per_bar).floor() as i32;
        let end_bar = ((self.scroll_x + grid_rect.width()) / pixels_per_bar).ceil() as i32;

        let division = state.snap_mode.get_division(bpm);
        let subdivisions_per_beat = (beat_duration / division).round() as i32;
        let pixels_per_division = pixels_per_beat / subdivisions_per_beat as f32;

        for bar in start_bar..=end_bar {
            let x = grid_rect.left() + bar as f32 * pixels_per_bar - self.scroll_x;

            // **Ensure shading is properly aligned**
            if bar % 8 < 4 {
                let bar_rect = egui::Rect::from_min_size(
                    egui::pos2(x, grid_rect.top()),
                    egui::vec2(pixels_per_bar * 4.0, grid_rect.height()),
                );

                let bg_color = ui.visuals().extreme_bg_color.linear_multiply(1.08);
                ui.painter().rect_filled(bar_rect, 0.0, bg_color);
            }

            // **Draw bar lines**
            let bar_line_color = ui.visuals().window_stroke.color.linear_multiply(2.0);
            ui.painter().line_segment(
                [
                    egui::pos2(x, grid_rect.top()),
                    egui::pos2(x, grid_rect.bottom()),
                ],
                (1.5, bar_line_color),
            );

            // **Draw beat and subdivision lines**
            for beat in 0..4 {
                let beat_x = x + (beat as f32 * pixels_per_beat);
                let beat_line_color = ui.visuals().window_stroke.color.linear_multiply(0.8);
                ui.painter().line_segment(
                    [
                        egui::pos2(beat_x, grid_rect.top()),
                        egui::pos2(beat_x, grid_rect.bottom()),
                    ],
                    (1.0, beat_line_color),
                );

                for sub in 1..subdivisions_per_beat {
                    let sub_x = beat_x + (sub as f32 * pixels_per_division);
                    if sub_x > grid_rect.right() {
                        break;
                    }
                    let sub_line_color = ui.visuals().window_stroke.color.linear_multiply(0.5);
                    ui.painter().line_segment(
                        [
                            egui::pos2(sub_x, grid_rect.top()),
                            egui::pos2(sub_x, grid_rect.bottom()),
                        ],
                        (0.5, sub_line_color),
                    );
                }
            }
        }

        // **Draw horizontal note grid (per pitch)**
        let note_height = self.key_height;
        let start_note = (self.scroll_y / note_height).floor() as i32;
        let end_note = ((self.scroll_y + grid_rect.height()) / note_height).ceil() as i32;

        for note in start_note..=end_note {
            let y = grid_rect.bottom() - (note as f32 + 1.0) * note_height + self.scroll_y;
            let is_c = note % 12 == 0;

            ui.painter().line_segment(
                [
                    egui::pos2(grid_rect.left(), y),
                    egui::pos2(grid_rect.right(), y),
                ],
                (
                    if is_c { 1.0 } else { 0.5 },
                    ui.visuals().window_stroke.color,
                ),
            );
        }
    }

    fn draw_notes(
        &mut self,
        ui: &mut egui::Ui,
        rect: egui::Rect,
        clip_id: &str,
        track_id: &str,
        state: &DawState,
    ) {
        let note_area = egui::Rect::from_min_max(
            egui::pos2(rect.left() + self.key_width, rect.top()),
            rect.max,
        );

        let note_position = NotePositioning::new(
            self.zoom,
            self.key_height,
            self.scroll_x,
            self.scroll_y,
            note_area,
        );

        // Get visible notes
        let visible_notes = self.get_visible_notes(note_area, track_id, clip_id, state);

        // First pass: Draw note bodies
        for note in &visible_notes {
            if !note_position.is_note_visible(note.start_time, note.key, note.duration) {
                continue;
            }

            let note_rect = note_position.note_to_rect(note.start_time, note.key, note.duration);

            // Draw base note shape
            let is_selected = self.selected_notes.contains(&note.id);
            let color = if is_selected {
                ui.visuals().selection.bg_fill
            } else {
                egui::Color32::from_rgb(64, 128, 255)
            };

            ui.painter().rect_filled(note_rect, 4.0, color);

            // Draw velocity indicator
            self.draw_velocity_indicator(ui, note_rect, note.velocity);
        }

        // Second pass: Handle interactions and overlays
        // Only handle note interactions if we're not currently drawing
        if !matches!(self.dragging, Some(DragOperation::Drawing { .. })) {
            for note in &visible_notes {
                if !note_position.is_note_visible(note.start_time, note.key, note.duration) {
                    continue;
                }

                let note_rect =
                    note_position.note_to_rect(note.start_time, note.key, note.duration);

                // Handle note interactions
                self.handle_note_interaction(ui, note_rect, note, clip_id, state);
            }
        }
    }

    fn handle_resize_controls(
        &mut self,
        ui: &mut egui::Ui,
        note_rect: egui::Rect,
        note: &Note,
        clip_id: &str,
        state: &DawState,
        note_response: &egui::Response,
    ) {
        let handle_width = 6.0;  // Made wider for easier grabbing
        const DRAG_THRESHOLD: f32 = 3.0;  // Pixels before resize starts

        // Create resize handles
        let left_handle = egui::Rect::from_min_size(
            note_rect.left_top(),
            egui::vec2(handle_width, note_rect.height()),
        );
        let right_handle = egui::Rect::from_min_size(
            egui::pos2(note_rect.right() - handle_width, note_rect.top()),
            egui::vec2(handle_width, note_rect.height()),
        );

        // Draw handles when note is hovered or selected
        if note_response.hovered() || self.selected_notes.contains(&note.id) {
            ui.painter()
                .rect_filled(left_handle, 0.0, ui.visuals().selection.stroke.color);
            ui.painter()
                .rect_filled(right_handle, 0.0, ui.visuals().selection.stroke.color);
        }

        // Handle resizing
        let left_response = ui.allocate_rect(left_handle, egui::Sense::drag());
        let right_response = ui.allocate_rect(right_handle, egui::Sense::drag());

        // Initialize resize state when drag starts
        if left_response.drag_started() || right_response.drag_started() {
            self.resize_initial_values = Some((note.start_time, note.duration));
            self.drag_accumulator = 0.0;
        }

        // Reset state when drag stops
        if left_response.drag_stopped() || right_response.drag_stopped() {
            self.resize_initial_values = None;
            self.drag_accumulator = 0.0;
        }

        if (left_response.dragged() || right_response.dragged())
            && !matches!(self.dragging, Some(DragOperation::MovingNotes { .. }))
        {
            let (edge, delta) = if left_response.dragged() {
                (ResizeEdge::Left, -left_response.drag_delta().x)
            } else {
                (ResizeEdge::Right, right_response.drag_delta().x)
            };

            // Accumulate drag delta
            self.drag_accumulator += delta;

            // Only process if we've exceeded the threshold
            if self.drag_accumulator.abs() >= DRAG_THRESHOLD {
                if let Some((initial_start, initial_duration)) = self.resize_initial_values {
                    // Convert accumulated pixel delta to time delta
                    let accumulated_time_delta = self.drag_accumulator / self.zoom;

                    // Calculate new times based on the accumulated delta
                    let (new_start_time, new_duration) = match edge {
                        ResizeEdge::Left => {
                            let note_end = initial_start + initial_duration;
                            let proposed_start = initial_start - accumulated_time_delta as f64;

                            // Apply snapping less aggressively
                            let new_start = if self.grid_snap && self.drag_accumulator.abs() > 10.0 {
                                TimeUtils::snap_time(
                                    proposed_start.max(0.0).min(note_end - 0.1),
                                    state.project.bpm,
                                    state.snap_mode,
                                )
                            } else {
                                proposed_start.max(0.0).min(note_end - 0.1)
                            };

                            let new_duration = note_end - new_start;
                            (new_start, new_duration)
                        }
                        ResizeEdge::Right => {
                            let proposed_duration = initial_duration + accumulated_time_delta as f64;

                            // Apply snapping less aggressively
                            let new_duration = if self.grid_snap && self.drag_accumulator.abs() > 10.0 {
                                let end_time = initial_start + proposed_duration;
                                let snapped_end = TimeUtils::snap_time(
                                    end_time.max(initial_start + 0.1),
                                    state.project.bpm,
                                    state.snap_mode,
                                );
                                snapped_end - initial_start
                            } else {
                                proposed_duration.max(0.1)
                            };

                            (initial_start, new_duration)
                        }
                    };

                    self.command_collector.add_command(DawCommand::ResizeNote {
                        clip_id: clip_id.to_string(),
                        note_id: note.id.clone(),
                        new_start_time,
                        new_duration,
                    });
                }
            }
        }

        // Update cursor
        if left_response.hovered() || right_response.hovered() {
            ui.output_mut(|o| o.cursor_icon = egui::CursorIcon::ResizeHorizontal);
        }
    }

    fn handle_note_interaction(
        &mut self,
        ui: &mut egui::Ui,
        note_rect: egui::Rect,
        note: &Note,
        clip_id: &str,
        state: &DawState,
    ) {
        let response = ui.allocate_rect(note_rect, egui::Sense::click_and_drag());

        // Handle selection
        if response.clicked() {
            if ui.input(|i| i.modifiers.ctrl || i.modifiers.command) {
                // Ctrl+Click: Toggle selection
                if self.selected_notes.contains(&note.id) {
                    self.selected_notes.retain(|id| id != &note.id);
                } else {
                    self.selected_notes.push(note.id.clone());
                }
            } else if ui.input(|i| i.modifiers.shift) && !self.selected_notes.is_empty() {
                // Shift+Click: Range selection (to be implemented later)
                // For now, just add to selection
                if !self.selected_notes.contains(&note.id) {
                    self.selected_notes.push(note.id.clone());
                }
            } else {
                // Regular click: Single selection
                self.selected_notes.clear();
                self.selected_notes.push(note.id.clone());
            }
        }

        // Draw resize handles and handle resizing
        self.handle_resize_controls(ui, note_rect, note, clip_id, state, &response);

        // Handle dragging
        if matches!(
            self.dragging,
            None | Some(DragOperation::MovingNotes { .. })
        ) {
            self.handle_note_drag(&response, note, clip_id, state);
        }
    }

    fn draw_velocity_indicator(&self, ui: &mut egui::Ui, note_rect: egui::Rect, velocity: u8) {
        let velocity_height = (velocity as f32 / 127.0) * note_rect.height();
        let velocity_rect = egui::Rect::from_min_size(
            note_rect.left_bottom() - egui::vec2(0.0, velocity_height),
            egui::vec2(3.0, velocity_height),
        );
        ui.painter()
            .rect_filled(velocity_rect, 0.0, ui.visuals().text_color());
    }

    // Add this method to draw the playhead
    fn draw_playhead(
        &self,
        ui: &mut egui::Ui,
        rect: egui::Rect,
        clip_start: f64,
        current_time: f64,
    ) {
        let grid_rect = egui::Rect::from_min_max(
            egui::pos2(rect.left() + self.key_width, rect.top()),
            rect.max,
        );

        // Calculate relative time within the clip
        let relative_time = current_time - clip_start;

        // Convert time to x-coordinate
        let playhead_x = grid_rect.left() + (relative_time as f32 * self.zoom) - self.scroll_x;

        // Only draw if playhead is within view
        if playhead_x >= grid_rect.left() && playhead_x <= grid_rect.right() {
            // Draw playhead line
            let playhead_color = ui.visuals().selection.stroke.color;
            ui.painter().line_segment(
                [
                    egui::pos2(playhead_x, grid_rect.top()),
                    egui::pos2(playhead_x, grid_rect.bottom()),
                ],
                (2.0, playhead_color),
            );

            // Draw playhead head (triangle)
            let triangle_size = 8.0;
            let points = vec![
                egui::pos2(playhead_x - triangle_size / 2.0, grid_rect.top()),
                egui::pos2(playhead_x + triangle_size / 2.0, grid_rect.top()),
                egui::pos2(playhead_x, grid_rect.top() + triangle_size),
            ];
            ui.painter().add(egui::Shape::convex_polygon(
                points,
                playhead_color,
                (1.0, playhead_color),
            ));
        }
    }

    // Add auto-scroll functionality to follow playhead
    fn handle_playhead_autoscroll(&mut self, rect: egui::Rect, clip_start: f64, current_time: f64) {
        let grid_rect = egui::Rect::from_min_max(
            egui::pos2(rect.left() + self.key_width, rect.top()),
            rect.max,
        );

        // Calculate relative time within the clip
        let relative_time = current_time - clip_start;

        // Convert time to x-coordinate
        let playhead_x = grid_rect.left() + (relative_time as f32 * self.zoom) - self.scroll_x;

        // Define margins for auto-scroll (e.g., 100 pixels from edge)
        let margin = 100.0;

        // Auto-scroll if playhead is outside view or too close to edges
        if playhead_x > grid_rect.right() - margin {
            self.scroll_x += playhead_x - (grid_rect.right() - margin);
        } else if playhead_x < grid_rect.left() + margin {
            self.scroll_x = (self.scroll_x - ((grid_rect.left() + margin) - playhead_x)).max(0.0);
        }
    }

    fn get_visible_notes(
        &self,
        note_area: egui::Rect,
        track_id: &str,
        clip_id: &str,
        state: &DawState,
    ) -> Vec<Note> {
        let start_time = self.scroll_x / self.zoom;
        let end_time = (self.scroll_x + note_area.width()) / self.zoom;

        if let Some(track) = state.project.tracks.iter().find(|t| &t.id == track_id) {
            if let Some(Clip::Midi { midi_data, .. }) = track
                .clips
                .iter()
                .find(|c| matches!(c, Clip::Midi { id, .. } if id == clip_id))
            {
                if let Some(store) = midi_data {
                    // Clone the notes to get owned values
                    return store
                        .get_notes_in_range(start_time as f64, end_time as f64)
                        .into_iter()
                        .cloned()
                        .collect();
                }
            }
        }

        Vec::new()
    }

    fn get_total_height(&self) -> f32 {
        (DEFAULT_OCTAVES * NOTES_PER_OCTAVE) as f32 * self.key_height
    }

    fn handle_note_area_interaction(
        &mut self,
        ui: &mut egui::Ui,
        rect: egui::Rect,
        clip_id: &str,
        track_id: &str,
        state: &DawState,
        response: &egui::Response,
    ) {
        let note_area = egui::Rect::from_min_max(
            egui::pos2(rect.left() + self.key_width, rect.top()),
            rect.max,
        );

        // Only handle clicks in the note area (not on piano keys)
        if let Some(pos) = response.interact_pointer_pos() {
            if pos.x > rect.left() + self.key_width {
                // Start drawing operation on click
                if response.clicked() && !ui.input(|i| i.modifiers.ctrl || i.modifiers.command || i.modifiers.shift) {
                    // Check if we clicked on empty space (not on a note)
                    let clicked_on_note = self.get_visible_notes(note_area, track_id, clip_id, state)
                        .iter()
                        .any(|note| {
                            let note_rect = NotePositioning::new(
                                self.zoom,
                                self.key_height,
                                self.scroll_x,
                                self.scroll_y,
                                note_area,
                            ).note_to_rect(note.start_time, note.key, note.duration);
                            note_rect.contains(pos)
                        });

                    if !clicked_on_note {
                        // Clear selection when clicking empty space
                        self.selected_notes.clear();
                        
                        // Calculate note position from click
                        let time = ((pos.x - note_area.left() + self.scroll_x) / self.zoom) as f64;
                        let pitch_float = (rect.bottom() - pos.y + self.scroll_y) / self.key_height;
                        let pitch = pitch_float.floor() as u8;
                        
                        // Snap time to grid if enabled
                        let snapped_time = if self.grid_snap {
                            TimeUtils::snap_time(time, state.project.bpm, state.snap_mode)
                        } else {
                            time
                        };
                        
                        // Calculate default duration (1 beat)
                        let beat_duration = 60.0 / state.project.bpm;
                        let default_duration = if self.grid_snap {
                            state.snap_mode.get_division(state.project.bpm)
                        } else {
                            beat_duration
                        };
                        
                        // Create the note
                        self.command_collector.add_command(DawCommand::AddNote {
                            clip_id: clip_id.to_string(),
                            start_time: snapped_time,
                            duration: default_duration,
                            pitch,
                            velocity: 100, // Default velocity
                        });
                        
                        // Start drawing operation for potential drag-to-extend
                        self.dragging = Some(DragOperation::Drawing { 
                            start_x: pos.x,
                            start_y: pos.y,
                        });
                    }
                }
                
                // Handle drag to extend note duration
                if response.dragged() {
                    if let Some(DragOperation::Drawing { start_x, start_y }) = self.dragging {
                        // Visual feedback could be added here
                        // For now, we'll handle the duration on release
                    }
                }
                
                // Complete drawing operation on release
                if response.drag_stopped() {
                    if let Some(DragOperation::Drawing { start_x, start_y }) = self.dragging {
                        if let Some(end_pos) = response.interact_pointer_pos() {
                            let drag_distance = (end_pos.x - start_x).abs();
                            
                            // Only extend duration if we dragged significantly
                            if drag_distance > 5.0 {
                                // Calculate the duration from drag
                                let start_time = ((start_x - note_area.left() + self.scroll_x) / self.zoom) as f64;
                                let end_time = ((end_pos.x - note_area.left() + self.scroll_x) / self.zoom) as f64;
                                
                                if end_time > start_time {
                                    let duration = end_time - start_time;
                                    let snapped_duration = if self.grid_snap {
                                        TimeUtils::snap_time(duration, state.project.bpm, state.snap_mode)
                                    } else {
                                        duration
                                    };
                                    
                                    // We already created the note with default duration,
                                    // so we'd need to update it here. For now, this is a TODO.
                                    // TODO: Track the created note ID and update its duration
                                }
                            }
                        }
                        self.dragging = None;
                    }
                }
            }
        }
    }

    //todo move into utils/midi module
    fn get_note_name(note_number: i32) -> String {
        let note_names = [
            "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
        ];
        let octave = (note_number / 12) - 1;
        let note = note_number % 12;
        format!("{}{}", note_names[note as usize], octave)
    }

    fn center_on_middle_c(&mut self, viewport_height: f32) {
        // Only center if we haven't initialized the scroll position yet
        if self.viewport_height != viewport_height {
            self.viewport_height = viewport_height;
            let total_height = self.get_total_height();
            let middle_c_position = (MIDDLE_C as f32) * self.key_height;
            self.scroll_y = middle_c_position - (viewport_height / 2.0);

            // Clamp scroll position to keep piano roll in view
            self.scroll_y = self.scroll_y.clamp(0.0, total_height - viewport_height);
        }
    }

    fn draw_divider(&mut self, ui: &mut egui::Ui, rect: egui::Rect) {
        let response = ui.allocate_rect(rect, egui::Sense::click_and_drag());
        
        // Draw divider line
        let color = if response.hovered() || self.resizing_divider {
            ui.visuals().selection.stroke.color
        } else {
            ui.visuals().widgets.noninteractive.bg_stroke.color
        };
        
        ui.painter().rect_filled(rect, 0.0, color);
        
        // Update cursor
        if response.hovered() || self.resizing_divider {
            ui.output_mut(|o| o.cursor_icon = egui::CursorIcon::ResizeVertical);
        }
        
        // Handle dragging
        if response.drag_started() {
            self.resizing_divider = true;
        }
        
        if self.resizing_divider {
            let delta = response.drag_delta().y;
            self.automation_panel_height = (self.automation_panel_height - delta).clamp(50.0, 500.0);
        }
        
        if response.drag_stopped() {
            self.resizing_divider = false;
        }
    }

    fn draw_automation_panel(&mut self, ui: &mut egui::Ui, rect: egui::Rect, clip_id: &str, track_id: &str, state: &DawState) {
        let header_height = 30.0;
        let lane_gap = 2.0;
        
        // Header with lane selection
        let header_rect = egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), header_height));
        
        // Draw background for the piano key area equivalent
        let key_area_rect = egui::Rect::from_min_size(
            header_rect.min,
            egui::vec2(self.key_width, header_height),
        );
        ui.painter().rect_filled(key_area_rect, 0.0, ui.visuals().window_fill);
        
        // Draw the header content offset by key_width
        let header_content_rect = egui::Rect::from_min_size(
            egui::pos2(header_rect.left() + self.key_width, header_rect.top()),
            egui::vec2(header_rect.width() - self.key_width, header_height),
        );
        
        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(header_content_rect), |ui| {
            ui.horizontal(|ui| {
                ui.label("Automation:");
                
                // Toggle automation visibility button
                if ui.button("➕ Add Lane").clicked() {
                    // TODO: Show lane selection popup
                }
                
                ui.separator();
                
                // Quick toggle buttons for existing lanes
                for lane in &mut self.automation_lanes {
                    let label = format!("{} {}", 
                        if lane.visible { "👁" } else { "👁‍🗨" },
                        lane.parameter.display_name()
                    );
                    
                    if ui.selectable_label(lane.visible, label).clicked() {
                        lane.visible = !lane.visible;
                    }
                }
            });
        });
        
        // Calculate content area
        let content_rect = egui::Rect::from_min_size(
            egui::pos2(rect.left(), header_rect.bottom()),
            egui::vec2(rect.width(), rect.height() - header_height),
        );
        
        // Draw visible lanes
        let visible_lanes: Vec<_> = self.automation_lanes
            .iter()
            .filter(|lane| lane.visible)
            .collect();
        
        if visible_lanes.is_empty() {
            ui.allocate_new_ui(egui::UiBuilder::new().max_rect(content_rect), |ui| {
                ui.centered_and_justified(|ui| {
                    ui.label("No automation lanes visible. Click '➕ Add Lane' to add automation.");
                });
            });
            return;
        }
        
        // Calculate lane heights
        let total_gaps = (visible_lanes.len() - 1) as f32 * lane_gap;
        let available_height = content_rect.height() - total_gaps - self.automation_scroll_y;
        let default_lane_height = (available_height / visible_lanes.len() as f32).max(60.0);
        
        // Draw each visible lane
        let mut current_y = content_rect.top() - self.automation_scroll_y;
        
        for i in 0..self.automation_lanes.len() {
            if !self.automation_lanes[i].visible {
                continue;
            }
            
            let lane_height = self.automation_lanes[i].height;
            let lane_rect = egui::Rect::from_min_size(
                egui::pos2(content_rect.left(), current_y),
                egui::vec2(content_rect.width(), lane_height),
            );
            
            if lane_rect.bottom() > content_rect.top() && lane_rect.top() < content_rect.bottom() {
                let lane_id = self.automation_lanes[i].id.clone();
                ui.allocate_new_ui(egui::UiBuilder::new().max_rect(lane_rect.intersect(content_rect)), |ui| {
                    self.draw_automation_lane(ui, lane_rect, lane_id, clip_id, state);
                });
            }
            
            current_y += lane_height + lane_gap;
        }
        
        // Handle scrolling
        let total_height = self.automation_lanes
            .iter()
            .filter(|l| l.visible)
            .map(|l| l.height)
            .sum::<f32>() + total_gaps;
        
        if total_height > content_rect.height() {
            // TODO: Add scroll bar
        }
    }

    fn draw_automation_lane(&mut self, ui: &mut egui::Ui, rect: egui::Rect, lane_id: String, clip_id: &str, state: &DawState) {
        let label_width = self.key_width;
        let margin = 4.0;
        
        // Background
        ui.painter().rect_filled(
            rect,
            4.0,
            ui.visuals().extreme_bg_color,
        );
        
        // Label area
        let label_rect = egui::Rect::from_min_size(
            egui::pos2(rect.left() + margin, rect.top() + margin),
            egui::vec2(label_width - margin * 2.0, rect.height() - margin * 2.0),
        );
        
        // Get lane info
        let lane = self.automation_lanes.iter().find(|l| l.id == lane_id).unwrap();
        let param_name = lane.parameter.display_name();
        let current_value = lane.get_value_at_time(state.current_time);
        
        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(label_rect), |ui| {
            ui.vertical(|ui| {
                ui.label(&param_name);
                
                // Value display
                ui.small(format!("{:.1}", current_value));
            });
        });
        
        // Automation curve area
        let curve_rect = egui::Rect::from_min_size(
            egui::pos2(rect.left() + label_width, rect.top()),
            egui::vec2(rect.width() - label_width, rect.height()),
        );
        
        self.draw_automation_curve(ui, curve_rect, &lane_id, state);
    }

    fn draw_automation_curve(&mut self, ui: &mut egui::Ui, rect: egui::Rect, lane_id: &str, state: &DawState) {
        let response = ui.allocate_rect(rect, egui::Sense::click_and_drag());
        
        // Get lane data for drawing
        let lane = match self.automation_lanes.iter().find(|l| l.id == lane_id) {
            Some(l) => l.clone(),
            None => return,
        };
        
        // Check if this is a velocity lane
        let is_velocity_lane = matches!(lane.parameter, AutomationParameter::Velocity);
        
        // Grid alignment with piano roll
        let grid_rect = rect;
        
        // Draw grid lines (aligned with piano roll)
        let bpm = state.project.bpm;
        let beat_duration = 60.0 / bpm;
        let pixels_per_beat = self.zoom;
        let pixels_per_bar = pixels_per_beat * 4.0;
        
        let start_bar = (self.scroll_x / pixels_per_bar).floor() as i32;
        let end_bar = ((self.scroll_x + grid_rect.width()) / pixels_per_bar).ceil() as i32;
        
        // Draw vertical grid lines
        for bar in start_bar..=end_bar {
            let x = grid_rect.left() + bar as f32 * pixels_per_bar - self.scroll_x;
            
            if x >= grid_rect.left() && x <= grid_rect.right() {
                let is_bar_line = true;
                let color = ui.visuals().widgets.noninteractive.bg_stroke.color;
                ui.painter().line_segment(
                    [egui::pos2(x, grid_rect.top()), egui::pos2(x, grid_rect.bottom())],
                    (0.5, color),
                );
            }
        }
        
        // Draw velocity bars or automation curve
        if is_velocity_lane {
            self.draw_velocity_bars(ui, rect, lane_id, state);
        } else if !lane.points.is_empty() {
            let mut path = Vec::new();
            
            // Calculate visible time range
            let start_time = self.scroll_x / self.zoom;
            let end_time = (self.scroll_x + rect.width()) / self.zoom;
            
            // Get points to draw (including one before and after visible range for continuity)
            let mut points_to_draw = Vec::new();
            let mut last_before = None;
            let mut first_after = None;
            
            for point in &lane.points {
                if point.time < start_time as f64 {
                    last_before = Some(point);
                } else if point.time > end_time as f64 && first_after.is_none() {
                    first_after = Some(point);
                    break;
                } else {
                    points_to_draw.push(point);
                }
            }
            
            // Add boundary points if they exist
            if let Some(point) = last_before {
                points_to_draw.insert(0, point);
            }
            if let Some(point) = first_after {
                points_to_draw.push(point);
            }
            
            // Generate curve path
            for i in 0..points_to_draw.len() {
                let point = points_to_draw[i];
                let x = rect.left() + (point.time as f32 * self.zoom) - self.scroll_x;
                let normalized_value = (point.value - lane.min_value) / (lane.max_value - lane.min_value);
                let y = rect.bottom() - (normalized_value as f32 * rect.height());
                
                if i == 0 {
                    path.push(egui::pos2(x, y));
                } else {
                    // Interpolate between points based on curve type
                    let prev_point = points_to_draw[i - 1];
                    let steps = ((point.time - prev_point.time) * self.zoom as f64 / 2.0).ceil() as usize;
                    
                    for step in 1..=steps {
                        let t = step as f64 / steps as f64;
                        let time = prev_point.time + (point.time - prev_point.time) * t;
                        let value = lane.get_value_at_time(time);
                        
                        let x = rect.left() + (time as f32 * self.zoom) - self.scroll_x;
                        let normalized_value = (value - lane.min_value) / (lane.max_value - lane.min_value);
                        let y = rect.bottom() - (normalized_value as f32 * rect.height());
                        
                        if x >= rect.left() && x <= rect.right() {
                            path.push(egui::pos2(x, y));
                        }
                    }
                }
            }
            
            // Draw the curve
            if path.len() > 1 {
                let color = egui::Color32::from_rgb(
                    (lane.color[0] * 255.0) as u8,
                    (lane.color[1] * 255.0) as u8,
                    (lane.color[2] * 255.0) as u8,
                );
                
                ui.painter().add(egui::Shape::line(
                    path,
                    egui::Stroke::new(2.0, color),
                ));
            }
            
            // Draw points
            let points_to_draw: Vec<_> = lane.points.iter().enumerate().collect();
            
            for (point_idx, point) in points_to_draw {
                let x = rect.left() + (point.time as f32 * self.zoom) - self.scroll_x;
                
                if x >= rect.left() - 10.0 && x <= rect.right() + 10.0 {
                    let normalized_value = (point.value - lane.min_value) / (lane.max_value - lane.min_value);
                    let y = rect.bottom() - (normalized_value as f32 * rect.height());
                    
                    let point_rect = egui::Rect::from_center_size(
                        egui::pos2(x, y),
                        egui::vec2(8.0, 8.0),
                    );
                    
                    let is_selected = self.selected_automation_points.iter()
                        .any(|(lid, pid)| lid == &lane.id && pid == &point.id);
                    
                    let color = if is_selected {
                        ui.visuals().selection.bg_fill
                    } else {
                        egui::Color32::from_rgb(
                            (lane.color[0] * 255.0) as u8,
                            (lane.color[1] * 255.0) as u8,
                            (lane.color[2] * 255.0) as u8,
                        )
                    };
                    
                    ui.painter().circle_filled(
                        point_rect.center(),
                        4.0,
                        color,
                    );
                    
                    // Handle point interaction
                    let point_response = ui.allocate_rect(point_rect, egui::Sense::click_and_drag());
                    let point_id = point.id.clone();
                    let lane_id = lane.id.clone();
                    
                    if point_response.clicked() {
                        if ui.input(|i| i.modifiers.ctrl || i.modifiers.command) {
                            // Multi-select
                            let selection = (lane_id.clone(), point_id.clone());
                            if is_selected {
                                self.selected_automation_points.retain(|s| s != &selection);
                            } else {
                                self.selected_automation_points.push(selection);
                            }
                        } else {
                            // Single select
                            self.selected_automation_points.clear();
                            self.selected_automation_points.push((lane_id.clone(), point_id.clone()));
                        }
                    }
                    
                    // Handle dragging
                    if point_response.dragged() && is_selected {
                        let delta_x = point_response.drag_delta().x / self.zoom;
                        let delta_y = -point_response.drag_delta().y / rect.height();
                        
                        let new_time = (point.time + delta_x as f64).max(0.0);
                        let delta_value = delta_y as f64 * (lane.max_value - lane.min_value);
                        let new_value = (point.value + delta_value).clamp(lane.min_value, lane.max_value);
                        
                        // Update the point
                        if let Some(lane) = self.automation_lanes.iter_mut().find(|l| l.id == lane_id) {
                            lane.update_point(&point_id, Some(new_time), Some(new_value));
                        }
                    }
                }
            }
        }
        
        // Handle creating new points
        if response.clicked() && !response.dragged() {
            let click_pos = response.interact_pointer_pos().unwrap();
            let time = ((click_pos.x - rect.left() + self.scroll_x) / self.zoom) as f64;
            let normalized_value = (rect.bottom() - click_pos.y) / rect.height();
            
            if let Some(lane) = self.automation_lanes.iter_mut().find(|l| l.id == lane_id) {
                let value = lane.min_value + normalized_value as f64 * (lane.max_value - lane.min_value);
                
                if time >= 0.0 {
                    let point_id = lane.add_point(time, value);
                    self.selected_automation_points.clear();
                    self.selected_automation_points.push((lane_id.to_string(), point_id));
                }
            }
        }
        
        // Draw playhead
        self.draw_automation_playhead(ui, rect, state.current_time);
    }

    fn draw_velocity_bars(&mut self, ui: &mut egui::Ui, rect: egui::Rect, lane_id: &str, state: &DawState) {
        // Get the current clip's MIDI data
        if let EditorView::PianoRoll { clip_id, track_id, .. } = &state.current_view {
            if let Some(track) = state.project.tracks.iter().find(|t| &t.id == track_id) {
                if let Some(Clip::Midi { midi_data, .. }) = track.clips.iter()
                    .find(|c| matches!(c, Clip::Midi { id, .. } if id == clip_id))
                {
                    if let Some(store) = midi_data {
                        // Get visible notes
                        let start_time = self.scroll_x / self.zoom;
                        let end_time = (self.scroll_x + rect.width()) / self.zoom;
                        let notes = store.get_notes_in_range(start_time as f64, end_time as f64);
                        
                        // Draw velocity bar for each note
                        for note in notes {
                            let x_start = rect.left() + (note.start_time as f32 * self.zoom) - self.scroll_x;
                            let x_end = rect.left() + ((note.start_time + note.duration) as f32 * self.zoom) - self.scroll_x;
                            let bar_width = (x_end - x_start).max(2.0);
                            
                            // Calculate bar height based on velocity
                            let velocity_normalized = note.velocity as f32 / 127.0;
                            let bar_height = velocity_normalized * rect.height();
                            
                            // Draw the velocity bar
                            let bar_rect = egui::Rect::from_min_size(
                                egui::pos2(x_start, rect.bottom() - bar_height),
                                egui::vec2(bar_width, bar_height),
                            );
                            
                            // Color based on velocity
                            let color = egui::Color32::from_rgb(
                                (255.0 * velocity_normalized) as u8,
                                (100.0 + 100.0 * (1.0 - velocity_normalized)) as u8,
                                (255.0 * (1.0 - velocity_normalized)) as u8,
                            );
                            
                            ui.painter().rect_filled(bar_rect, 2.0, color);
                            
                            // Draw outline
                            ui.painter().rect_stroke(
                                bar_rect,
                                2.0,
                                egui::Stroke::new(1.0, ui.visuals().window_stroke.color),
                                egui::epaint::StrokeKind::Outside,
                            );
                            
                            // Handle interaction
                            let bar_response = ui.allocate_rect(bar_rect, egui::Sense::drag());
                            if bar_response.dragged() {
                                let delta_y = -bar_response.drag_delta().y;
                                let new_velocity_normalized = ((bar_height + delta_y) / rect.height()).clamp(0.0, 1.0);
                                let new_velocity = (new_velocity_normalized * 127.0).max(1.0) as u8;
                                
                                // Update note velocity through command system
                                self.command_collector.add_command(DawCommand::UpdateNoteVelocity {
                                    clip_id: clip_id.clone(),
                                    note_id: note.id.clone(),
                                    velocity: new_velocity,
                                });
                            }
                            
                            // Show velocity value on hover
                            if bar_response.hovered() {
                                ui.painter().text(
                                    egui::pos2(x_start + bar_width / 2.0, rect.bottom() - bar_height - 10.0),
                                    egui::Align2::CENTER_BOTTOM,
                                    format!("{}", note.velocity),
                                    egui::FontId::proportional(10.0),
                                    ui.visuals().text_color(),
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    fn draw_automation_playhead(&self, ui: &mut egui::Ui, rect: egui::Rect, current_time: f64) {
        // Use the same calculation as the piano roll playhead
        let playhead_x = rect.left() + (current_time as f32 * self.zoom) - self.scroll_x;
        
        if playhead_x >= rect.left() && playhead_x <= rect.right() {
            ui.painter().line_segment(
                [egui::pos2(playhead_x, rect.top()), egui::pos2(playhead_x, rect.bottom())],
                (1.0, ui.visuals().selection.stroke.color),
            );
        }
    }
}
