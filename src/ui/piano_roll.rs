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
    selected_notes: Vec<String>, // Change to store UUIDs as strings
    dragging: Option<DragOperation>,
    command_collector: CommandCollector,
}

#[derive(Debug)]
enum DragOperation {
    MovingNotes { start_x: f32, start_y: f32 },
    ResizingNotes { edge: ResizeEdge, start_x: f32 },
    Drawing { start_x: f32, start_y: f32 },
}

#[derive(Debug)]
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

    pub fn show(&mut self, ui: &mut egui::Ui, state: &mut DawState) -> Vec<DawCommand> {
        if let EditorView::PianoRoll {
            clip_id, track_id, ..
        } = &state.current_view
        {
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

            let (rect, response) =
                ui.allocate_exact_size(ui.available_size(), egui::Sense::click_and_drag());

            self.center_on_middle_c(rect.height());
            self.draw_grid(ui, rect, state);
            self.draw_notes(ui, rect, clip_id, track_id, state);
            self.draw_piano_keys(ui, rect);

            // self.handle_scrolling(ui, rect);
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
        note: &MidiNote,
        clip_id: &str,
        state: &DawState,
    ) {
        if response.dragged() {
            println!("Dragging note: {:?}, dragging: {:?}", note, self.dragging);
            if let None = self.dragging {
                self.dragging = Some(DragOperation::MovingNotes {
                    start_x: response.hover_pos().unwrap().x,
                    start_y: response.hover_pos().unwrap().y,
                });
            }

            if let Some(DragOperation::MovingNotes { start_x, start_y }) = self.dragging {
                let current_pos = response.hover_pos().unwrap();
                let delta_x = current_pos.x - start_x;
                let delta_y = current_pos.y - start_y;

                let delta_time = delta_x / self.zoom;
                let delta_pitch = -(delta_y / self.key_height) as i8;

                // Calculate new time with snapping
                let time = note.start_time + delta_time as f64;

                // TODO: If holding "command/alt" ignore snapping
                let new_time = if self.grid_snap {
                    TimeUtils::snap_time(time, state.project.bpm, state.snap_mode)
                } else {
                    time
                };

                let actual_delta_time = new_time - note.start_time;

                self.command_collector.add_command(DawCommand::MoveNotes {
                    clip_id: clip_id.to_string(),
                    note_ids: self.selected_notes.clone(),
                    delta_time: actual_delta_time,
                    delta_pitch,
                });
            }
        }

        if response.drag_stopped() {
            self.dragging = None;
        }
    }

    fn draw_piano_keys(&self, ui: &mut egui::Ui, rect: egui::Rect) {
        let keys_rect = rect.intersect(egui::Rect::from_min_size(
            rect.min,
            egui::vec2(self.key_width, rect.height()),
        ));

        let start_note = (self.scroll_y / self.key_height).floor() as i32;
        let end_note = ((self.scroll_y + rect.height()) / self.key_height).ceil() as i32;
        let visible_notes = start_note..=end_note;

        // Draw background for piano keys
        ui.painter()
            .rect_filled(keys_rect, 0.0, ui.visuals().extreme_bg_color);

        // Draw white keys first
        for note_number in visible_notes.clone() {
            let note = note_number % 12;
            if [0, 2, 4, 5, 7, 9, 11].contains(&note) {
                self.draw_key(ui, note_number, false, keys_rect);
            }
        }

        // Draw black keys on top
        for note_number in visible_notes {
            let note = note_number % 12;
            if [1, 3, 6, 8, 10].contains(&note) {
                self.draw_key(ui, note_number, true, keys_rect);
            }
        }
    }

    fn draw_key(&self, ui: &mut egui::Ui, note_number: i32, is_black: bool, rect: egui::Rect) {
        let y = rect.bottom() - (note_number as f32 + 1.0) * self.key_height + self.scroll_y;

        // Calculate key dimensions
        let key_width = if is_black {
            self.key_width * 0.6
        } else {
            self.key_width
        };

        // Offset black keys to overlap whites
        let x_offset = if is_black {
            -self.key_width * 0.15
        } else {
            0.0
        };

        let key_rect = egui::Rect::from_min_max(
            egui::pos2(rect.left() + x_offset, y),
            egui::pos2(rect.left() + x_offset + key_width, y + self.key_height),
        );

        // Draw key background
        let color = if is_black {
            ui.visuals().extreme_bg_color
        } else {
            ui.visuals().window_fill
        };

        // Add key border
        ui.painter().rect_stroke(
            key_rect,
            0.0,
            egui::Stroke::new(1.0, ui.visuals().window_stroke.color),
            StrokeKind::Outside,
        );

        ui.painter().rect_filled(key_rect, 0.0, color);
        // Add key response for potential MIDI preview
        let response = ui.allocate_rect(key_rect, egui::Sense::click());

        // Draw note name
        let note = note_number % 12;
        let octave = (note_number / 12) - 1;
        let note_names = [
            "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
        ];
        let note_name = format!("{}{}", note_names[note as usize], octave);

        // Only show full note name for C notes or when hovering
        if note == 0 || response.hovered() {
            let text_pos = key_rect.left_center() + egui::vec2(4.0, 0.0);
            ui.painter().text(
                text_pos,
                egui::Align2::CENTER_CENTER,
                &note_name,
                FontId::monospace(10.0),
                if is_black {
                    ui.visuals().strong_text_color()
                } else {
                    ui.visuals().text_color()
                },
            );
        }

        if response.hovered() {
            // TODO: Preview MIDI note
        }
    }

    fn draw_grid(&self, ui: &mut egui::Ui, rect: egui::Rect, state: &DawState) {
        let grid_rect = egui::Rect::from_min_max(
            egui::pos2(rect.left() + self.key_width, rect.top()),
            rect.max,
        );

        let bpm = state.project.bpm;
        let division = state.snap_mode.get_division(bpm); // Get division based on snap mode

        let pixels_per_beat = self.zoom;
        let pixels_per_bar = pixels_per_beat * 4.0;
        let pixels_per_division = pixels_per_beat * (division / (60.0 / bpm)) as f32;

        let start_beat = (self.scroll_x / pixels_per_beat).floor() as i32;
        let end_beat = ((self.scroll_x + grid_rect.width()) / pixels_per_beat).ceil() as i32;

        for beat in start_beat..=end_beat {
            let x = grid_rect.left() + beat as f32 * pixels_per_beat - self.scroll_x;
            let is_bar = beat % 4 == 0;

            // Alternate background shading every bar
            if is_bar {
                let bar_rect = egui::Rect::from_min_size(
                    egui::pos2(x, grid_rect.top()),
                    egui::vec2(pixels_per_bar, grid_rect.height()),
                );

                let bg_color = if (beat / 4) % 2 == 0 {
                    ui.visuals().extreme_bg_color.linear_multiply(1.1)
                } else {
                    ui.visuals().extreme_bg_color.linear_multiply(0.9)
                };

                ui.painter().rect_filled(bar_rect, 0.0, bg_color);
            }

            // Draw major grid lines (bars and beats)
            let line_color = if is_bar {
                ui.visuals().window_stroke.color.linear_multiply(2.0)
            } else {
                ui.visuals().window_stroke.color
            };

            ui.painter().line_segment(
                [
                    egui::pos2(x, grid_rect.top()),
                    egui::pos2(x, grid_rect.bottom()),
                ],
                (1.0, line_color),
            );

            // **Subdivisions**
            for i in 1..4 {
                let sub_x = x + (i as f32 * pixels_per_division);
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

        // Draw horizontal note grid (per pitch)
        for note in 0..128 {
            let y = grid_rect.bottom() - (note as f32 + 1.0) * self.key_height + self.scroll_y;
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
        // Calculate note area (to the right of piano keys)
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

        // Find the clip and ensure it's loaded
        if let Some(track) = state.project.tracks.iter().find(|t| &t.id == track_id) {
            if let Some(Clip::Midi { midi_data, .. }) = track.clips.iter().find(|c| match c {
                Clip::Midi { id, .. } => id == clip_id,
                _ => false,
            }) {
                if let Some(midi_data) = midi_data {
                    for note in &midi_data.notes {
                        if !note_position.is_note_visible(
                            note.start_time,
                            note.pitch,
                            note.duration,
                        ) {
                            continue;
                        }

                        let note_rect =
                            note_position.note_to_rect(note.start_time, note.pitch, note.duration);

                        let response = ui.allocate_rect(note_rect, egui::Sense::click_and_drag());

                        // Draw base note shape
                        let is_selected = self.selected_notes.contains(&note.id);
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

                        // Handle note selection
                        if response.clicked() {
                            if !ui.input(|i| i.modifiers.shift) {
                                self.selected_notes.clear();
                            }
                            self.selected_notes.push(note.id.clone());
                        }

                        // Draw resize handles
                        let handle_width = 4.0;
                        let left_handle = egui::Rect::from_min_size(
                            note_rect.left_top(),
                            egui::vec2(handle_width, note_rect.height()),
                        );
                        let right_handle = egui::Rect::from_min_size(
                            egui::pos2(note_rect.right() - handle_width, note_rect.top()),
                            egui::vec2(handle_width, note_rect.height()),
                        );

                        let left_response = ui.allocate_rect(left_handle, egui::Sense::drag());
                        let right_response = ui.allocate_rect(right_handle, egui::Sense::drag());

                        // Handle dragging (resizing)
                        if left_response.dragged() {
                            let delta = (left_response.drag_delta().x / self.zoom) as f64;
                            let new_start_time = if self.grid_snap {
                                TimeUtils::snap_time(
                                    (note.start_time - delta).max(0.0),
                                    state.project.bpm,
                                    state.snap_mode,
                                )
                            } else {
                                (note.start_time - delta).max(0.0)
                            };
                            let new_duration = if self.grid_snap {
                                TimeUtils::snap_time(
                                    (note.duration + delta).max(0.1),
                                    state.project.bpm,
                                    state.snap_mode,
                                )
                            } else {
                                (note.duration - delta).max(0.1)
                            };

                            self.command_collector.add_command(DawCommand::ResizeNote {
                                clip_id: clip_id.to_string(),
                                note_id: note.id.clone(),
                                new_start_time,
                                new_duration,
                            });
                        }

                        if right_response.dragged() {
                            let delta = (right_response.drag_delta().x / self.zoom) as f64;
                            let new_duration = if self.grid_snap {
                                TimeUtils::snap_time(
                                    (note.duration + delta).max(0.1),
                                    state.project.bpm,
                                    state.snap_mode,
                                )
                            } else {
                                (note.duration + delta).max(0.1)
                            };

                            self.command_collector.add_command(DawCommand::ResizeNote {
                                clip_id: clip_id.to_string(),
                                note_id: note.id.clone(),
                                new_start_time: note.start_time,
                                new_duration,
                            });
                        }

                        if left_response.hovered() || right_response.hovered() {
                            ui.output_mut(|o| o.cursor_icon = egui::CursorIcon::ResizeHorizontal);
                        }

                        // Handle note dragging with snapping
                        self.handle_note_drag(&response, note, clip_id, state);
                    }
                }
            }
        }
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
