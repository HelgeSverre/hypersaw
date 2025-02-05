use crate::core::*;
use eframe::egui;
use egui::FontId;

pub struct PianoRoll {
    key_width: f32,
    key_height: f32,
    grid_snap: f32, // In beats
    zoom: f32,      // Pixels per beat
    scroll_x: f32,
    scroll_y: f32,
    selected_notes: Vec<usize>,
    dragging: Option<DragOperation>,
    command_collector: CommandCollector,
}

enum DragOperation {
    MovingNotes { start_x: f32, start_y: f32 },
    ResizingNotes { edge: ResizeEdge, start_x: f32 },
    Drawing { start_x: f32, start_y: f32 },
}

enum ResizeEdge {
    Left,
    Right,
}

impl Default for PianoRoll {
    fn default() -> Self {
        Self {
            key_width: 80.0,
            key_height: 20.0,
            grid_snap: 0.25,
            zoom: 100.0,
            scroll_x: 0.0,
            scroll_y: 0.0,
            selected_notes: Vec::new(),
            dragging: None,
            command_collector: CommandCollector::new(),
        }
    }
}

impl PianoRoll {
    pub fn show(&mut self, ui: &mut egui::Ui, state: &mut DawState) -> Vec<DawCommand> {
        if let EditorView::PianoRoll {
            clip_id, track_id, ..
        } = &state.current_view
        {
            // Try to load the MIDI data if not already loaded
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

            let (rect, response) =
                ui.allocate_exact_size(ui.available_size(), egui::Sense::click_and_drag());

            self.draw_background(ui, rect);
            self.draw_piano_keys(ui, rect);
            self.draw_grid(ui, rect);
            self.draw_notes(ui, rect, clip_id, track_id, state);

            if response.dragged() {
                self.scroll_x -= response.drag_delta().x;
                self.scroll_y += response.drag_delta().y;
            }

            self.handle_zoom(ui);
        }

        self.command_collector.take_commands()
    }

    fn handle_zoom(&mut self, ui: &egui::Ui) {
        ui.input(|i| {
            if i.modifiers.ctrl || i.modifiers.command {
                let zoom_delta = i.raw_scroll_delta.y / 100.0;
                self.zoom = (self.zoom * (1.0 + zoom_delta)).max(20.0).min(500.0);
            }
        });
    }

    fn draw_background(&self, ui: &mut egui::Ui, rect: egui::Rect) {
        ui.painter()
            .rect_filled(rect, 0.0, ui.visuals().extreme_bg_color);
    }

    fn draw_piano_keys(&self, ui: &mut egui::Ui, rect: egui::Rect) {
        let keys_rect = rect.intersect(egui::Rect::from_min_size(
            rect.min,
            egui::vec2(self.key_width, rect.height()),
        ));

        // Draw white keys first
        for octave in 0..8 {
            for white_key in [0, 2, 4, 5, 7, 9, 11] {
                let note = octave * 12 + white_key;
                let y = rect.bottom() - (note as f32 + 1.0) * self.key_height;
                let key_rect = egui::Rect::from_min_max(
                    egui::pos2(rect.left(), y),
                    egui::pos2(rect.left() + self.key_width, y + self.key_height),
                );
                ui.painter()
                    .rect_filled(key_rect, 0.0, ui.visuals().window_fill);

                // Draw key label
                let note_name = format!("{:?}", note);
                let text_pos = key_rect.center() - egui::vec2(0.0, 8.0);
                ui.painter().text(
                    text_pos,
                    egui::Align2::CENTER_CENTER,
                    note_name,
                    FontId::monospace(8.0),
                    ui.visuals().text_color(),
                );
            }
        }

        // Draw black keys on top
        for octave in 0..8 {
            for black_key in [1, 3, 6, 8, 10] {
                let note = octave * 12 + black_key;
                let y = rect.bottom() - (note as f32 + 1.0) * self.key_height;
                let key_rect = egui::Rect::from_min_max(
                    egui::pos2(rect.left(), y),
                    egui::pos2(rect.left() + self.key_width * 0.6, y + self.key_height),
                );
                ui.painter()
                    .rect_filled(key_rect, 0.0, ui.visuals().extreme_bg_color);

                // Draw key label
                let note_name = format!("{:?}", note);
                let text_pos = key_rect.center() - egui::vec2(0.0, 8.0);
                ui.painter().text(
                    text_pos,
                    egui::Align2::CENTER_CENTER,
                    note_name,
                    FontId::monospace(8.0),
                    ui.visuals().strong_text_color(),
                );
            }
        }
    }

    fn draw_grid(&self, ui: &mut egui::Ui, rect: egui::Rect) {
        let grid_rect = rect.translate(egui::vec2(self.key_width, 0.0));

        // Draw vertical grid lines (beats)
        let start_beat = (self.scroll_x / self.zoom).floor() as i32;
        let end_beat = ((self.scroll_x + rect.width()) / self.zoom).ceil() as i32;

        for beat in start_beat..=end_beat {
            let x = grid_rect.left() + beat as f32 * self.zoom - self.scroll_x;
            let is_bar = beat % 4 == 0;

            ui.painter().line_segment(
                [
                    egui::pos2(x, grid_rect.top()),
                    egui::pos2(x, grid_rect.bottom()),
                ],
                (
                    if is_bar { 1.0 } else { 0.5 },
                    ui.visuals().window_stroke.color,
                ),
            );
        }

        // Draw horizontal grid lines (notes)
        for note in 0..128 {
            let y = grid_rect.bottom() - (note as f32 + 1.0) * self.key_height;
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
        let viewport = ui.clip_rect();
        let grid_rect = rect.translate(egui::vec2(self.key_width, 0.0));

        let note_position = NotePositioning::new(
            self.zoom,
            self.key_height,
            self.scroll_x,
            self.scroll_y,
            viewport,
        );

        // Find the clip and ensure it's loaded
        if let Some(track) = state.project.tracks.iter().find(|t| &t.id == track_id) {
            if let Some(Clip::Midi { midi_data, .. }) = track.clips.iter().find(|c| match c {
                Clip::Midi { id, .. } => id == clip_id,
                _ => false,
            }) {
                if let Some(midi_data) = midi_data {
                    for note in &midi_data.notes {
                        // Skip notes outside viewport for performance
                        if !note_position.is_note_visible(
                            note.start_time,
                            note.pitch,
                            note.duration,
                        ) {
                            continue;
                        }

                        let note_rect =
                            note_position.note_to_rect(note.start_time, note.pitch, note.duration);

                        // Add interaction handling
                        let response = ui.allocate_rect(note_rect, egui::Sense::click_and_drag());

                        // Draw base note shape
                        let is_selected = self.selected_notes.contains(&note.id.parse().unwrap());
                        let color = if is_selected {
                            ui.visuals().selection.bg_fill
                        } else {
                            egui::Color32::from_rgb(64, 128, 255)
                        };

                        ui.painter().rect_filled(note_rect, 4.0, color);

                        // Draw velocity indicator
                        let velocity_height = (note.velocity as f32 / 127.0) * note_rect.height();
                        let velocity_rect = egui::Rect::from_min_size(
                            note_rect.left_bottom() - egui::vec2(0.0, velocity_height),
                            egui::vec2(3.0, velocity_height),
                        );
                        ui.painter()
                            .rect_filled(velocity_rect, 0.0, ui.visuals().text_color());

                        // Handle interactions
                        if response.clicked() {
                            if !ui.input(|i| i.modifiers.shift) {
                                self.selected_notes.clear();
                            }
                            self.selected_notes.push(note.id.parse().unwrap());
                        }

                        if response.dragged() {
                            if let None = self.dragging {
                                self.dragging = Some(DragOperation::MovingNotes {
                                    start_x: response.hover_pos().unwrap().x,
                                    start_y: response.hover_pos().unwrap().y,
                                });
                            }

                            if let Some(DragOperation::MovingNotes { start_x, start_y }) =
                                self.dragging
                            {
                                let current_pos = response.hover_pos().unwrap();
                                let delta_x = current_pos.x - start_x;
                                let delta_y = current_pos.y - start_y;

                                // TODO: Add command for moving notes
                                //             self.command_collector.add_command(DawCommand::MoveNotes {
                                //     clip_id: clip_id.to_string(),
                                //     note_ids: self.selected_notes.clone(),
                                //     delta_time: delta_x / self.zoom,
                                //     delta_pitch: -(delta_y / self.key_height) as i8,
                                //             });
                            }
                        }

                        if response.drag_stopped() {
                            self.dragging = None;
                        }
                    }
                }
            }
        }
    }
}
