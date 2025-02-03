use crate::core::*;
use eframe::egui;

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
            let (rect, response) =
                ui.allocate_exact_size(ui.available_size(), egui::Sense::click_and_drag());

            // Draw background
            ui.painter()
                .rect_filled(rect, 0.0, ui.visuals().extreme_bg_color);

            // Draw piano keys on the left
            self.draw_piano_keys(ui, rect);

            // Draw grid
            self.draw_grid(ui, rect);

            // Draw notes
            if let Some(track) = state.project.tracks.iter().find(|t| &t.id == track_id) {
                if let Some(Clip::Midi { .. }) = track.clips.iter().find(|c| match c {
                    Clip::Midi { id, .. } => id == clip_id,
                    _ => false,
                }) {
                    // TODO: Draw actual MIDI notes
                    // For now just draw a test note
                    let note_rect = egui::Rect::from_min_size(
                        egui::pos2(rect.left() + 100.0, rect.top() + 200.0),
                        egui::vec2(100.0, self.key_height),
                    );
                    ui.painter()
                        .rect_filled(note_rect, 2.0, egui::Color32::from_rgb(64, 128, 255));
                }
            }

            // Handle input
            if response.dragged_by(egui::PointerButton::Middle) {
                self.scroll_x -= response.drag_delta().x;
                self.scroll_y -= response.drag_delta().y;
            }

            // Handle zoom
            if response.hovered() {
                ui.input(|i| {
                    if i.modifiers.ctrl {
                        let zoom_delta = i.smooth_scroll_delta.y * 0.001;
                        self.zoom = (self.zoom * (1.0 + zoom_delta))
                            .max(20.0) // Minimum zoom
                            .min(500.0); // Maximum zoom
                    }
                });
            }
        }

        self.command_collector.take_commands()
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
}
