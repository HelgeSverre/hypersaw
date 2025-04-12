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
}

#[derive(Debug)]
enum DragOperation {
    MovingNotes { start_x: f32, start_y: f32 },
    ResizingNotes { edge: ResizeEdge, start_x: f32 },
    Drawing { start_x: f32, start_y: f32 },
}

#[derive(Debug, Clone, Copy)]
enum ResizeEdge {
    Left,
    Right,
}

impl PianoRoll {
    pub fn default() -> Self {
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

            let (rect, response) =
                ui.allocate_exact_size(ui.available_size(), egui::Sense::click_and_drag());

            self.center_on_middle_c(rect.height());
            self.draw_grid(ui, rect, state);
            self.draw_notes(ui, rect, clip_id, track_id, state);
            self.draw_piano_keys(ui, rect, state, clip_id, track_id);

            // Draw playhead after everything else
            self.draw_playhead(ui, rect, clip_start, state.current_time);

            // Handle zoom and scrolling
            self.handle_zoom(ui, rect);

            // Handle middle-button dragging for panning
            if response.dragged() {
                // Horizontal scroll
                let invert = -1.0;
                let delta = response.drag_delta();
                self.scroll_x = (self.scroll_x + delta.x * invert).max(0.0);

                // Vertical scroll
                let new_scroll_y = (self.scroll_y + delta.y).max(0.0);
                self.scroll_y =
                    new_scroll_y.clamp(0.0, self.get_total_height() - self.viewport_height);
            }

            // If pressing delete, delete selected notes

            if ui.input(|i| i.key_pressed(egui::Key::Delete)) {
                self.command_collector.add_command(DawCommand::DeleteNotes {
                    clip_id: clip_id.to_string(),
                    note_ids: self.selected_notes.clone(),
                });
                self.selected_notes.clear();
            }

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
        if response.dragged() {
            // Ensure the note being dragged is selected
            if !self.selected_notes.contains(&note.id) {
                self.selected_notes.push(note.id.clone());
            }

            // Calculate deltas from last frame
            let delta_x = response.drag_delta().x / self.zoom;
            let delta_y = -(response.drag_delta().y / self.key_height) as i8;

            // Apply snapping if enabled
            let actual_delta_time = if self.grid_snap {
                let new_time = TimeUtils::snap_time(
                    note.start_time + delta_x as f64,
                    state.project.bpm,
                    state.snap_mode,
                );
                new_time - note.start_time
            } else {
                delta_x as f64
            };

            self.command_collector.add_command(DawCommand::MoveNotes {
                clip_id: clip_id.to_string(),
                note_ids: self.selected_notes.clone(),
                delta_time: actual_delta_time,
                delta_pitch: delta_y,
            });
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
        let handle_width = 4.0;

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

        if (left_response.dragged() || right_response.dragged())
            && !matches!(self.dragging, Some(DragOperation::MovingNotes { .. }))
        {
            let (edge, delta) = if left_response.dragged() {
                (ResizeEdge::Left, -left_response.drag_delta().x)
            } else {
                (ResizeEdge::Right, right_response.drag_delta().x)
            };

            // Convert pixel delta to time delta
            let delta_time = delta / self.zoom;

            // Calculate new times based on the delta
            let (new_start_time, new_duration) = match edge {
                ResizeEdge::Left => {
                    let note_end = note.start_time + note.duration;
                    let proposed_start = note.start_time - delta_time as f64;

                    // Clamp the start time to not go beyond the note end
                    let new_start = if self.grid_snap {
                        TimeUtils::snap_time(
                            proposed_start.min(note_end - 0.1),
                            state.project.bpm,
                            state.snap_mode,
                        )
                    } else {
                        proposed_start.min(note_end - 0.1)
                    };

                    // Duration is always end - start
                    let new_duration = note_end - new_start;
                    (new_start, new_duration)
                }
                ResizeEdge::Right => {
                    let proposed_duration = note.duration + delta_time as f64;

                    // Clamp the duration to be positive
                    let new_duration = if self.grid_snap {
                        TimeUtils::snap_time(
                            proposed_duration.max(0.1),
                            state.project.bpm,
                            state.snap_mode,
                        )
                    } else {
                        proposed_duration.max(0.1)
                    };

                    (note.start_time, new_duration)
                }
            };

            self.command_collector.add_command(DawCommand::ResizeNote {
                clip_id: clip_id.to_string(),
                note_id: note.id.clone(),
                new_start_time,
                new_duration,
            });
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
            if self.selected_notes.contains(&note.id) {
                // Deselect note if already selected
                self.selected_notes.retain(|id| id != &note.id);
            } else {
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
}
