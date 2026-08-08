use crate::core::*;
use eframe::egui;
use egui::{FontId, StrokeKind};

const MIDDLE_C: i32 = 60; // MIDI note number for middle C
const MIDI_NOTE_COUNT: i32 = 128;

pub struct PianoRoll {
    key_width: f32,
    key_height: f32,
    grid_snap: bool,
    zoom: f32,
    scroll_x: f32,
    scroll_y: f32,
    viewport_height: f32,
    selected_notes: Vec<EventID>,
    clipboard: Vec<Note>, // Clipboard for copy/paste
    dragging: Option<DragOperation>,
    command_collector: CommandCollector,
    // Automation panel
    automation_panel_height: f32,
    selected_automation_points: Vec<(String, String)>, // (lane_id, point_id)
    automation_scroll_y: f32,
    resizing_divider: bool,
    // UI state
    show_automation: bool,
    note_gesture: Option<NoteGesture>,
    pan_origin: Option<(f32, f32, egui::Pos2)>,
    // CC search
    cc_search_query: String,
}

#[derive(Debug, Clone)]
enum NoteGesture {
    Move {
        initial_notes: Vec<Note>,
        delta_time: f64,
        delta_pitch: i8,
        duplicate: bool,
    },
    Resize {
        initial_notes: Vec<Note>,
        edge: ResizeEdge,
        delta: f64,
    },
}

#[derive(Debug)]
enum DragOperation {
    MovingNotes {
        start_x: f32,
        start_y: f32,
    },
    ResizingNotes {
        edge: ResizeEdge,
        start_x: f32,
    },
    Drawing {
        start_x: f32,
        start_y: f32,
    },
    SelectionBox {
        start_x: f32,
        start_y: f32,
    },
    MovingAutomationPoint {
        lane_id: String,
        point_id: String,
        start_x: f32,
        start_y: f32,
    },
    DrawingAutomation {
        lane_id: String,
        start_x: f32,
        start_y: f32,
    },
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
    fn new(
        zoom: f32,
        key_height: f32,
        scroll_x: f32,
        scroll_y: f32,
        note_area: egui::Rect,
    ) -> Self {
        Self {
            zoom,
            key_height,
            scroll_x,
            scroll_y,
            note_area,
        }
    }

    fn note_to_rect(&self, start_time: f64, key: u8, duration: f64) -> egui::Rect {
        let x_start = self.note_area.left() + (start_time as f32 * self.zoom) - self.scroll_x;
        let x_end =
            self.note_area.left() + ((start_time + duration) as f32 * self.zoom) - self.scroll_x;
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
        Self {
            key_width: 80.0,
            key_height: 20.0,
            grid_snap: true,
            zoom: 100.0,
            scroll_x: 0.0,
            scroll_y: 0.0,
            viewport_height: 0.0,
            selected_notes: Vec::new(),
            clipboard: Vec::new(),
            dragging: None,
            command_collector: CommandCollector::new(),
            automation_panel_height: 200.0,
            selected_automation_points: Vec::new(),
            automation_scroll_y: 0.0,
            resizing_divider: false,
            show_automation: true,
            note_gesture: None,
            pan_origin: None,
            cc_search_query: String::new(),
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
        let (clip_id, track_id) = if let EditorView::PianoRoll {
            clip_id, track_id, ..
        } = &state.current_view
        {
            (clip_id.clone(), track_id.clone())
        } else {
            return Vec::new();
        };

        // TODO: move into the project.rs - track struct
        // Load MIDI data if needed
        if let Some(track) = state.project.tracks.iter_mut().find(|t| &t.id == &track_id) {
            if let Some(clip @ Clip::Midi { loaded: false, .. }) = track
                .clips
                .iter_mut()
                .find(|c| matches!(c, Clip::Midi { id, .. } if id == &clip_id))
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
            if let Some(track) = state.project.tracks.iter().find(|t| &t.id == &track_id) {
                if let Some(Clip::Midi { start_time, .. }) = track
                    .clips
                    .iter()
                    .find(|c| matches!(c, Clip::Midi { id, .. } if id == &clip_id))
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
            self.automation_panel_height.clamp(
                min_panel_height,
                full_rect.height() - min_panel_height - divider_height,
            )
        } else {
            0.0
        };

        let piano_roll_rect = egui::Rect::from_min_size(
            full_rect.min,
            egui::vec2(
                full_rect.width(),
                full_rect.height()
                    - effective_automation_height
                    - (if self.show_automation {
                        divider_height
                    } else {
                        0.0
                    }),
            ),
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
            self.handle_note_area_interaction(ui, rect, &clip_id, &track_id, state, &response);

            self.draw_notes(ui, rect, &clip_id, &track_id, state);
            self.draw_piano_keys(ui, rect, state, &clip_id, &track_id);

            // Draw playhead after everything else
            self.draw_playhead(ui, rect, clip_start, state.current_time);

            // Draw selection box on top of everything
            if let Some(DragOperation::SelectionBox { start_x, start_y }) = self.dragging {
                if let Some(current_pos) = response.interact_pointer_pos() {
                    let selection_rect =
                        egui::Rect::from_two_pos(egui::pos2(start_x, start_y), current_pos);

                    // Draw the selection box
                    ui.painter().rect_stroke(
                        selection_rect,
                        0.0,
                        egui::Stroke::new(1.0_f32, ui.visuals().selection.stroke.color),
                        StrokeKind::Outside,
                    );
                }
            }

            // Handle zoom and scrolling
            self.handle_zoom(ui, rect);
            self.handle_scrolling(ui, rect);

            // Handle middle-button dragging for panning
            if response.drag_started() && self.dragging.is_none() && !self.resizing_divider {
                if let Some(pointer_pos) = response.interact_pointer_pos() {
                    self.pan_origin = Some((self.scroll_x, self.scroll_y, pointer_pos));
                }
            }
            if response.dragged() && !self.resizing_divider {
                // Only pan if we're not drawing or have another drag operation
                if self.dragging.is_none() {
                    let (origin_x, origin_y, pointer_origin) =
                        self.pan_origin
                            .unwrap_or((self.scroll_x, self.scroll_y, egui::Pos2::ZERO));
                    let delta = response
                        .interact_pointer_pos()
                        .map_or(egui::Vec2::ZERO, |pos| pos - pointer_origin);
                    self.scroll_x = (origin_x - delta.x).max(0.0);
                    self.scroll_y = (origin_y + delta.y).clamp(
                        0.0,
                        (self.get_total_height() - self.viewport_height).max(0.0),
                    );
                }
            }
            if response.drag_stopped() {
                self.pan_origin = None;
            }
        });

        // Draw resizable divider
        if self.show_automation {
            self.draw_divider(ui, divider_rect);
        }

        // Draw automation panel
        if self.show_automation {
            // Fill background to prevent bleed-through
            ui.painter()
                .rect_filled(automation_rect, 0.0, ui.visuals().window_fill);

            let clip_id_clone = clip_id.clone();
            let track_id_clone = track_id.clone();
            ui.allocate_new_ui(egui::UiBuilder::new().max_rect(automation_rect), |ui| {
                self.draw_automation_panel(
                    ui,
                    automation_rect,
                    &clip_id_clone,
                    &track_id_clone,
                    clip_start,
                    state,
                );
            });
        }

        // Handle keyboard shortcuts
        let deleted_notes: Vec<Note> = if !self.selected_notes.is_empty() {
            // Collect notes for undo before deleting
            state
                .project
                .tracks
                .iter()
                .flat_map(|track| &track.clips)
                .find_map(|c| {
                    let Clip::Midi { id, midi_data, .. } = c;
                    if id == &clip_id {
                        return midi_data.as_ref().map(|store| {
                            self.selected_notes
                                .iter()
                                .filter_map(|note_id| store.get_note(note_id).cloned())
                                .collect()
                        });
                    }
                    None
                })
                .unwrap_or_default()
        } else {
            Vec::new()
        };

        ui.input(|i| {
            if ui.ctx().wants_keyboard_input() {
                return;
            }

            // Delete key - delete selected notes and automation points
            if i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace) {
                if !self.selected_notes.is_empty() {
                    self.command_collector.add_command(DawCommand::DeleteNotes {
                        clip_id: clip_id.to_string(),
                        note_ids: self.selected_notes.clone(),
                        deleted_notes: Some(deleted_notes),
                    });
                    self.selected_notes.clear();
                }

                // Handle deleting automation points
                if !self.selected_automation_points.is_empty() {
                    self.command_collector
                        .add_command(DawCommand::DeleteAutomationPoints {
                            clip_id: clip_id.to_string(),
                            points: self.selected_automation_points.clone(),
                        });
                    self.selected_automation_points.clear();
                }
            }

            // Ctrl+A - Select all notes
            if i.key_pressed(egui::Key::A) && (i.modifiers.ctrl || i.modifiers.command) {
                self.selected_notes.clear();
                // Get all notes in the clip
                if let Some(track) = state.project.tracks.iter().find(|t| &t.id == &track_id) {
                    if let Some(Clip::Midi { midi_data, .. }) = track
                        .clips
                        .iter()
                        .find(|c| matches!(c, Clip::Midi { id, .. } if id == &clip_id))
                    {
                        if let Some(store) = midi_data {
                            for note in store.get_notes() {
                                self.selected_notes.push(note.id.clone());
                            }
                        }
                    }
                }
            }

            // Ctrl+C - Copy selected notes
            if i.key_pressed(egui::Key::C) && (i.modifiers.ctrl || i.modifiers.command) {
                if !self.selected_notes.is_empty() {
                    self.clipboard.clear();
                    // Copy selected notes to clipboard
                    if let Some(track) = state.project.tracks.iter().find(|t| &t.id == &track_id) {
                        if let Some(Clip::Midi { midi_data, .. }) = track
                            .clips
                            .iter()
                            .find(|c| matches!(c, Clip::Midi { id, .. } if id == &clip_id))
                        {
                            if let Some(store) = midi_data {
                                for note_id in &self.selected_notes {
                                    if let Some(note) = store.get_note(note_id) {
                                        self.clipboard.push(note.clone());
                                    }
                                }
                                state
                                    .status
                                    .info(format!("Copied {} notes", self.clipboard.len()));
                            }
                        }
                    }
                }
            }

            // Ctrl+V - Paste notes
            if i.key_pressed(egui::Key::V) && (i.modifiers.ctrl || i.modifiers.command) {
                if !self.clipboard.is_empty() {
                    // Find the earliest note in clipboard to use as reference
                    let min_time = self
                        .clipboard
                        .iter()
                        .map(|n| n.start_time)
                        .min_by(|a, b| a.partial_cmp(b).unwrap())
                        .unwrap_or(0.0);

                    // Notes are stored relative to their clip, while playback time is global.
                    let paste_time = (state.current_time - clip_start).max(0.0);
                    let time_offset = paste_time - min_time;

                    // Create AddNote commands for each clipboard note
                    for note in &self.clipboard {
                        self.command_collector.add_command(DawCommand::AddNote {
                            clip_id: clip_id.to_string(),
                            start_time: note.start_time + time_offset,
                            duration: note.duration,
                            pitch: note.key,
                            velocity: note.velocity,
                        });
                    }
                    state
                        .status
                        .success(format!("Pasted {} notes", self.clipboard.len()));
                }
            }

            // Ctrl+D - Duplicate selected notes
            if i.key_pressed(egui::Key::D) && (i.modifiers.ctrl || i.modifiers.command) {
                if !self.selected_notes.is_empty() {
                    // Collect notes to duplicate
                    let mut notes_to_duplicate = Vec::new();
                    if let Some(track) = state.project.tracks.iter().find(|t| &t.id == &track_id) {
                        if let Some(Clip::Midi { midi_data, .. }) = track
                            .clips
                            .iter()
                            .find(|c| matches!(c, Clip::Midi { id, .. } if id == &clip_id))
                        {
                            if let Some(store) = midi_data {
                                for note_id in &self.selected_notes {
                                    if let Some(note) = store.get_note(note_id) {
                                        notes_to_duplicate.push(note.clone());
                                    }
                                }
                            }
                        }
                    }

                    // Place the duplicated group after the selected group while retaining the
                    // timing relationships between its notes.
                    let count = notes_to_duplicate.len();
                    let time_offset = duplicate_time_offset(&notes_to_duplicate);

                    // Duplicate notes immediately after the originals
                    for note in notes_to_duplicate {
                        self.command_collector.add_command(DawCommand::AddNote {
                            clip_id: clip_id.to_string(),
                            start_time: note.start_time + time_offset,
                            duration: note.duration,
                            pitch: note.key,
                            velocity: note.velocity,
                        });
                    }
                    state.status.success(format!("Duplicated {} notes", count));
                }
            }

            // Escape - Clear selection
            if i.key_pressed(egui::Key::Escape) {
                self.selected_notes.clear();
                self.selected_automation_points.clear();
            }

            // Q - Quantize selected notes
            if i.key_pressed(egui::Key::Q) && !self.selected_notes.is_empty() {
                self.command_collector
                    .add_command(DawCommand::QuantizeNotes {
                        clip_id: clip_id.to_string(),
                        note_ids: self.selected_notes.clone(),
                        strength: 1.0, // Full quantization
                        grid: state.snap_mode,
                    });
            }

            // Arrow key nudging for selected notes
            if !self.selected_notes.is_empty() {
                let shift = i.modifiers.shift;

                // Horizontal nudge (time)
                let time_nudge = if i.key_pressed(egui::Key::ArrowLeft) {
                    Some(-1.0)
                } else if i.key_pressed(egui::Key::ArrowRight) {
                    Some(1.0)
                } else {
                    None
                };

                if let Some(direction) = time_nudge {
                    let delta_time = if shift {
                        // One beat
                        60.0 / state.project.bpm
                    } else {
                        // One grid unit (fallback to beat if grid is None)
                        let division = state.snap_mode.get_division(state.project.bpm);
                        if division == 0.0 {
                            60.0 / state.project.bpm
                        } else {
                            division
                        }
                    };

                    // Clamp to prevent negative times (check all notes)
                    let clamped_delta = clamp_time_delta_for_notes(
                        &self.selected_notes,
                        direction * delta_time,
                        &clip_id,
                        state,
                    );

                    if clamped_delta.abs() > 0.0001 {
                        self.command_collector.add_command(DawCommand::MoveNotes {
                            clip_id: clip_id.to_string(),
                            note_ids: self.selected_notes.clone(),
                            delta_time: clamped_delta,
                            delta_pitch: 0,
                        });
                    }
                }

                // Vertical nudge (pitch)
                let pitch_nudge = if i.key_pressed(egui::Key::ArrowUp) {
                    Some(1)
                } else if i.key_pressed(egui::Key::ArrowDown) {
                    Some(-1)
                } else {
                    None
                };

                if let Some(direction) = pitch_nudge {
                    let delta_pitch = if shift { direction * 12 } else { direction };

                    // Clamp to MIDI range 0-127
                    let clamped_pitch = clamp_pitch_delta_for_notes(
                        &self.selected_notes,
                        delta_pitch,
                        &clip_id,
                        state,
                    );

                    if clamped_pitch != 0 {
                        self.command_collector.add_command(DawCommand::MoveNotes {
                            clip_id: clip_id.to_string(),
                            note_ids: self.selected_notes.clone(),
                            delta_time: 0.0,
                            delta_pitch: clamped_pitch as i8,
                        });
                    }
                }
            }
        });

        // Auto-scroll to follow playhead if it's outside view
        // self.handle_playhead_autoscroll(rect, clip_start, state.current_time);

        self.command_collector.take_commands()
    }

    fn handle_scrolling(&mut self, ui: &egui::Ui, rect: egui::Rect) {
        ui.input(|i| {
            let pointer_over_roll = i.pointer.hover_pos().is_some_and(|pos| rect.contains(pos));
            if !pointer_over_roll || i.modifiers.ctrl || i.modifiers.command {
                return;
            }

            if i.modifiers.shift {
                // Horizontal scroll with shift
                let scroll_delta = if i.raw_scroll_delta.x.abs() > f32::EPSILON {
                    i.raw_scroll_delta.x
                } else {
                    i.raw_scroll_delta.y
                };
                self.scroll_x = (self.scroll_x - scroll_delta).max(0.0);
            } else {
                // Vertical scroll without shift
                let scroll_delta = i.raw_scroll_delta.y;
                let new_scroll_y = self.scroll_y - scroll_delta;
                self.scroll_y = new_scroll_y.clamp(
                    0.0,
                    (self.get_total_height() - self.viewport_height).max(0.0),
                );
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
        ui: &egui::Ui,
        response: &egui::Response,
        note: &Note,
        clip_id: &str,
        state: &DawState,
    ) {
        const DRAG_THRESHOLD: f32 = 3.0;

        if response.drag_started() {
            let additive = ui.input(|input| input.modifiers.shift);
            if !self.selected_notes.contains(&note.id) {
                if !additive {
                    self.selected_notes.clear();
                }
                self.selected_notes.push(note.id.clone());
            }

            self.note_gesture = Some(NoteGesture::Move {
                initial_notes: notes_for_ids(state, clip_id, &self.selected_notes),
                delta_time: 0.0,
                delta_pitch: 0,
                duplicate: ui.input(|input| input.modifiers.ctrl || input.modifiers.command),
            });
            self.dragging = Some(DragOperation::MovingNotes {
                start_x: response.interact_pointer_pos().unwrap_or_default().x,
                start_y: response.interact_pointer_pos().unwrap_or_default().y,
            });
        }

        let drag_delta = match (&self.dragging, response.interact_pointer_pos()) {
            (Some(DragOperation::MovingNotes { start_x, start_y }), Some(pointer_position)) => {
                pointer_position - egui::pos2(*start_x, *start_y)
            }
            _ => egui::Vec2::ZERO,
        };

        if response.dragged() && drag_delta.length() >= DRAG_THRESHOLD {
            if let Some(NoteGesture::Move {
                initial_notes,
                delta_time,
                delta_pitch,
                ..
            }) = &mut self.note_gesture
            {
                let raw_delta = f64::from(drag_delta.x / self.zoom);
                let anchor = initial_notes
                    .iter()
                    .map(|initial| initial.start_time)
                    .reduce(f64::min)
                    .unwrap_or(0.0);
                let proposed = if self.grid_snap {
                    TimeUtils::snap_time(anchor + raw_delta, state.project.bpm, state.snap_mode)
                        - anchor
                } else {
                    raw_delta
                };
                *delta_time = proposed.max(-anchor);
                let raw_pitch = -(drag_delta.y / self.key_height).round() as i32;
                *delta_pitch = clamp_pitch_delta(initial_notes, raw_pitch) as i8;
            }
        }

        if response.drag_stopped() {
            if let Some(NoteGesture::Move {
                initial_notes,
                delta_time,
                delta_pitch,
                duplicate,
            }) = self.note_gesture.take()
            {
                if duplicate {
                    let notes = initial_notes
                        .into_iter()
                        .map(|mut initial| {
                            initial.id = uuid::Uuid::new_v4().to_string();
                            initial.start_time += delta_time;
                            initial.key = (i16::from(initial.key) + i16::from(delta_pitch))
                                .clamp(0, 127) as u8;
                            initial
                        })
                        .collect();
                    self.command_collector.add_command(DawCommand::AddNotes {
                        clip_id: clip_id.to_string(),
                        notes,
                    });
                } else if delta_time.abs() > f64::EPSILON || delta_pitch != 0 {
                    self.command_collector.add_command(DawCommand::MoveNotes {
                        clip_id: clip_id.to_string(),
                        note_ids: initial_notes
                            .into_iter()
                            .map(|initial| initial.id)
                            .collect(),
                        delta_time,
                        delta_pitch,
                    });
                }
            }
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
        let start_note = start_note.clamp(0, MIDI_NOTE_COUNT - 1);
        let end_note = end_note.clamp(0, MIDI_NOTE_COUNT - 1);
        let visible_notes = start_note..=end_note;

        // Get currently active notes
        let active_notes = self.get_active_notes(state, clip_id, track_id, state.current_time);

        // Draw subtle background for piano keys area
        let piano_bg_color = egui::Color32::from_gray(25); // Very dark background
        ui.painter().rect_filled(keys_rect, 0.0, piano_bg_color);

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

        // Draw key background with proper piano colors
        let base_color = if is_black {
            egui::Color32::from_gray(40) // Dark gray for black keys
        } else {
            egui::Color32::from_gray(240) // Light gray (almost white) for white keys
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

        // Draw key border
        let border_color = if is_black {
            egui::Color32::from_gray(20)
        } else {
            egui::Color32::from_gray(100)
        };
        ui.painter().rect_stroke(
            key_rect,
            0.0,
            egui::Stroke::new(1.0_f32, border_color),
            StrokeKind::Outside,
        );

        let response = ui.allocate_rect(key_rect, egui::Sense::click());

        // Draw note name and MIDI number side by side
        let note = note_number % 12;
        let octave = (note_number / 12) - 1;
        let note_names = [
            "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
        ];
        let note_name = format!("{}{}", note_names[note as usize], octave);

        // Always show labels for white keys, show black key labels on hover or when active
        let show_label = if is_black {
            response.hovered() || is_active
        } else {
            true
        };

        if show_label {
            // Note name color
            let note_color = if is_active {
                egui::Color32::WHITE
            } else if is_black {
                egui::Color32::from_gray(200)
            } else {
                egui::Color32::from_gray(40)
            };

            // MIDI number color (fainter)
            let midi_color = if is_active {
                egui::Color32::from_gray(220)
            } else if is_black {
                egui::Color32::from_gray(160)
            } else {
                egui::Color32::from_gray(100)
            };

            let font_size = if is_black { 8.0 } else { 9.0 };

            // Draw note name
            let note_pos = egui::pos2(key_rect.center().x - 8.0, key_rect.center().y);
            ui.painter().text(
                note_pos,
                egui::Align2::RIGHT_CENTER,
                &note_name,
                FontId::monospace(font_size),
                note_color,
            );

            // Draw MIDI number
            let midi_pos = egui::pos2(key_rect.center().x + 8.0, key_rect.center().y);
            ui.painter().text(
                midi_pos,
                egui::Align2::LEFT_CENTER,
                &note_number.to_string(),
                FontId::monospace(font_size * 0.9),
                midi_color,
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

        let pixels_per_beat = self.zoom * beat_duration as f32;
        let pixels_per_bar = self.zoom * bar_duration as f32;

        let start_bar = (self.scroll_x / pixels_per_bar).floor() as i32;
        let end_bar = ((self.scroll_x + grid_rect.width()) / pixels_per_bar).ceil() as i32;

        let subdivisions_per_beat = grid_subdivisions_per_beat(state.snap_mode, bpm);

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

                if let Some(subdivisions_per_beat) = subdivisions_per_beat {
                    let pixels_per_division = pixels_per_beat / subdivisions_per_beat as f32;
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
        }

        // **Draw horizontal note grid (per pitch)**
        let note_height = self.key_height;
        let start_note = (self.scroll_y / note_height).floor() as i32;
        let end_note = ((self.scroll_y + grid_rect.height()) / note_height).ceil() as i32;
        let start_note = start_note.clamp(0, MIDI_NOTE_COUNT - 1);
        let end_note = end_note.clamp(0, MIDI_NOTE_COUNT - 1);

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
        let mut visible_notes = self.get_visible_notes(note_area, track_id, clip_id, state);
        if let Some(gesture) = &self.note_gesture {
            for note in &mut visible_notes {
                apply_note_gesture_preview(note, gesture);
            }
        }

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
                self.handle_note_interaction(ui, note_rect, note, clip_id, state, &visible_notes);
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
        all_notes: &[Note],
    ) {
        let handle_width = 6.0; // Made wider for easier grabbing
        const DRAG_THRESHOLD: f32 = 3.0; // Pixels before resize starts

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

        if left_response.drag_started() || right_response.drag_started() {
            if !self.selected_notes.contains(&note.id) {
                self.selected_notes.clear();
                self.selected_notes.push(note.id.clone());
            }
            let edge = if left_response.drag_started() {
                ResizeEdge::Left
            } else {
                ResizeEdge::Right
            };
            self.note_gesture = Some(NoteGesture::Resize {
                initial_notes: all_notes
                    .iter()
                    .filter(|candidate| self.selected_notes.contains(&candidate.id))
                    .cloned()
                    .collect(),
                edge,
                delta: 0.0,
            });
            self.dragging = Some(DragOperation::ResizingNotes {
                edge,
                start_x: if left_response.drag_started() {
                    left_response.interact_pointer_pos().unwrap_or_default().x
                } else {
                    right_response.interact_pointer_pos().unwrap_or_default().x
                },
            });
        }

        let resize_drag = match (
            &self.dragging,
            ui.input(|input| input.pointer.interact_pos()),
        ) {
            (Some(DragOperation::ResizingNotes { edge, start_x }), Some(pointer_position)) => {
                Some((*edge, pointer_position.x - *start_x))
            }
            _ => None,
        };

        if let Some((edge, pixel_delta)) = resize_drag {
            if pixel_delta.abs() >= DRAG_THRESHOLD {
                if let Some(NoteGesture::Resize {
                    initial_notes,
                    delta,
                    ..
                }) = &mut self.note_gesture
                {
                    let first = initial_notes.first();
                    let raw_edge_delta = f64::from(pixel_delta / self.zoom);
                    let proposed = match (edge, first) {
                        (ResizeEdge::Left, Some(initial)) => {
                            let edge_time = initial.start_time + raw_edge_delta;
                            let target = if self.grid_snap {
                                TimeUtils::snap_time(edge_time, state.project.bpm, state.snap_mode)
                            } else {
                                edge_time
                            };
                            initial.start_time - target
                        }
                        (ResizeEdge::Right, Some(initial)) => {
                            let edge_time = initial.start_time + initial.duration + raw_edge_delta;
                            let target = if self.grid_snap {
                                TimeUtils::snap_time(edge_time, state.project.bpm, state.snap_mode)
                            } else {
                                edge_time
                            };
                            target - (initial.start_time + initial.duration)
                        }
                        (_, None) => 0.0,
                    };
                    *delta = calculate_clamped_resize_delta(initial_notes, proposed, edge, 0.1);
                }
            }
        }

        if left_response.drag_stopped() || right_response.drag_stopped() {
            if let Some(NoteGesture::Resize {
                initial_notes,
                edge,
                delta,
            }) = self.note_gesture.take()
            {
                if delta.abs() > f64::EPSILON {
                    let note_ids = initial_notes
                        .iter()
                        .map(|initial| initial.id.clone())
                        .collect();
                    let old_times = initial_notes
                        .iter()
                        .map(|initial| (initial.start_time, initial.duration))
                        .collect();
                    let new_times = initial_notes
                        .iter()
                        .map(|initial| resized_note_times(initial, edge, delta, 0.1))
                        .collect();
                    self.command_collector.add_command(DawCommand::ResizeNotes {
                        clip_id: clip_id.to_string(),
                        note_ids,
                        new_times,
                        old_times: Some(old_times),
                    });
                }
            }
            self.dragging = None;
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
        all_notes: &[Note],
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
                // Shift+Click: Range selection
                // Find the bounds of current selection and clicked note
                if let Some(track) = state.project.tracks.iter().find(|t| {
                    t.clips
                        .iter()
                        .any(|c| matches!(c, Clip::Midi { id, .. } if id == clip_id))
                }) {
                    if let Some(Clip::Midi { midi_data, .. }) = track
                        .clips
                        .iter()
                        .find(|c| matches!(c, Clip::Midi { id, .. } if id == clip_id))
                    {
                        if let Some(store) = midi_data {
                            // Get all notes as a vec
                            let all_notes: Vec<_> = store.get_notes().collect();

                            // Find min/max time of current selection
                            let mut min_time = f64::MAX;
                            let mut max_time = f64::MIN;
                            let mut min_pitch = u8::MAX;
                            let mut max_pitch = u8::MIN;

                            for selected_id in &self.selected_notes {
                                if let Some(selected_note) =
                                    all_notes.iter().find(|n| &n.id == selected_id)
                                {
                                    min_time = min_time.min(selected_note.start_time);
                                    max_time = max_time.max(selected_note.start_time);
                                    min_pitch = min_pitch.min(selected_note.key);
                                    max_pitch = max_pitch.max(selected_note.key);
                                }
                            }

                            // Extend range to include clicked note
                            min_time = min_time.min(note.start_time);
                            max_time = max_time.max(note.start_time);
                            min_pitch = min_pitch.min(note.key);
                            max_pitch = max_pitch.max(note.key);

                            // Clear and select all notes in range
                            self.selected_notes.clear();
                            for n in &all_notes {
                                if n.start_time >= min_time
                                    && n.start_time <= max_time
                                    && n.key >= min_pitch
                                    && n.key <= max_pitch
                                {
                                    self.selected_notes.push(n.id.clone());
                                }
                            }
                        }
                    }
                }
            } else {
                // Regular click: Single selection
                self.selected_notes.clear();
                self.selected_notes.push(note.id.clone());
            }
        }

        // Draw resize handles and handle resizing
        self.handle_resize_controls(ui, note_rect, note, clip_id, state, &response, all_notes);

        // Handle dragging
        if matches!(
            self.dragging,
            None | Some(DragOperation::MovingNotes { .. })
        ) {
            self.handle_note_drag(ui, &response, note, clip_id, state);
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
            // Draw playhead line - use same soft red color as timeline
            let playhead_color = egui::Color32::from_rgb(220, 80, 80);
            ui.painter().line_segment(
                [
                    egui::pos2(playhead_x, grid_rect.top()),
                    egui::pos2(playhead_x, grid_rect.bottom()),
                ],
                (2.0, playhead_color),
            );
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
        MIDI_NOTE_COUNT as f32 * self.key_height
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
                // Handle clicks
                if response.clicked() {
                    // Check if we clicked on empty space (not on a note)
                    let clicked_on_note = self
                        .get_visible_notes(note_area, track_id, clip_id, state)
                        .iter()
                        .any(|note| {
                            let note_rect = NotePositioning::new(
                                self.zoom,
                                self.key_height,
                                self.scroll_x,
                                self.scroll_y,
                                note_area,
                            )
                            .note_to_rect(
                                note.start_time,
                                note.key,
                                note.duration,
                            );
                            note_rect.contains(pos)
                        });

                    if !clicked_on_note {
                        // Clear selection when clicking empty space (unless Ctrl/Shift is held)
                        if !ui
                            .input(|i| i.modifiers.ctrl || i.modifiers.command || i.modifiers.shift)
                        {
                            // If we have selected notes, just clear selection
                            if !self.selected_notes.is_empty() {
                                self.selected_notes.clear();
                            } else {
                                // Only create a note if nothing was selected
                                // Calculate note position from click
                                let time =
                                    ((pos.x - note_area.left() + self.scroll_x) / self.zoom) as f64;
                                let pitch_float =
                                    (rect.bottom() - pos.y + self.scroll_y) / self.key_height;
                                let pitch = pitch_float.floor() as u8;

                                // Snap time to grid if enabled
                                let snapped_time = if self.grid_snap {
                                    TimeUtils::snap_time(time, state.project.bpm, state.snap_mode)
                                } else {
                                    time
                                };

                                // Calculate default duration (quarter note = 1 beat)
                                let beat_duration = 60.0 / state.project.bpm;
                                let default_duration = beat_duration; // Quarter note

                                // Create the note
                                self.command_collector.add_command(DawCommand::AddNote {
                                    clip_id: clip_id.to_string(),
                                    start_time: snapped_time,
                                    duration: default_duration,
                                    pitch,
                                    velocity: 100, // Default velocity
                                });
                            }
                        }
                    }
                }

                // Start selection box on drag with shift
                if response.drag_started() && ui.input(|i| i.modifiers.shift) {
                    if self.dragging.is_none() {
                        self.dragging = Some(DragOperation::SelectionBox {
                            start_x: pos.x,
                            start_y: pos.y,
                        });
                    }
                }

                // Handle drag operations
                if response.dragged() {
                    match self.dragging {
                        Some(DragOperation::SelectionBox { start_x, start_y }) => {
                            // Selection box visual is drawn in the main draw code
                        }
                        _ => {}
                    }
                }

                // Complete drag operations on release
                if response.drag_stopped() {
                    match self.dragging {
                        Some(DragOperation::SelectionBox { start_x, start_y }) => {
                            if let Some(end_pos) = response.interact_pointer_pos() {
                                let selection_rect =
                                    egui::Rect::from_two_pos(egui::pos2(start_x, start_y), end_pos);

                                // Clear selection if not holding Ctrl
                                if !ui.input(|i| i.modifiers.ctrl || i.modifiers.command) {
                                    self.selected_notes.clear();
                                }

                                // Get all visible notes and check for visual intersection
                                let visible_notes =
                                    self.get_visible_notes(note_area, track_id, clip_id, state);

                                for note in visible_notes {
                                    // Calculate note's visual rect
                                    let note_x = note_area.left()
                                        + (note.start_time as f32 * self.zoom)
                                        - self.scroll_x;
                                    let note_width = note.duration as f32 * self.zoom;
                                    let note_y = rect.bottom()
                                        - ((note.key as f32 + 1.0) * self.key_height)
                                        + self.scroll_y;
                                    let note_height = self.key_height;

                                    let note_rect = egui::Rect::from_min_size(
                                        egui::pos2(note_x, note_y),
                                        egui::vec2(note_width, note_height),
                                    );

                                    // Visual intersection - partial overlap counts
                                    if selection_rect.intersects(note_rect) {
                                        if !self.selected_notes.contains(&note.id) {
                                            self.selected_notes.push(note.id.clone());
                                        }
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                    self.dragging = None;
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
        // Preserve the current view across layout changes; only center the initial view.
        if self.viewport_height <= f32::EPSILON {
            let total_height = self.get_total_height();
            let middle_c_position = (MIDDLE_C as f32 + 0.5) * self.key_height;
            self.scroll_y = middle_c_position - (viewport_height / 2.0);
            self.scroll_y = self
                .scroll_y
                .clamp(0.0, (total_height - viewport_height).max(0.0));
        }
        self.viewport_height = viewport_height;
        self.scroll_y = self
            .scroll_y
            .clamp(0.0, (self.get_total_height() - viewport_height).max(0.0));
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
            self.automation_panel_height =
                (self.automation_panel_height - delta).clamp(50.0, 500.0);
        }

        if response.drag_stopped() {
            self.resizing_divider = false;
        }
    }

    fn draw_automation_panel(
        &mut self,
        ui: &mut egui::Ui,
        rect: egui::Rect,
        clip_id: &str,
        track_id: &str,
        clip_start: f64,
        state: &mut DawState,
    ) {
        let header_height = 30.0;
        let lane_gap = 2.0;

        // Get the clip's automation lanes
        let (has_velocity_lane, automation_lanes) =
            if let Some(track) = state.project.tracks.iter().find(|t| &t.id == track_id) {
                if let Some(Clip::Midi {
                    automation_lanes, ..
                }) = track
                    .clips
                    .iter()
                    .find(|c| matches!(c, Clip::Midi { id, .. } if id == clip_id))
                {
                    let has_velocity = automation_lanes
                        .iter()
                        .any(|l| matches!(l.parameter, AutomationParameter::Velocity));
                    (has_velocity, Some(automation_lanes))
                } else {
                    (false, None)
                }
            } else {
                (false, None)
            };

        // Header with lane selection
        let header_rect =
            egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), header_height));

        // Draw opaque background for the entire header to prevent bleed-through
        ui.painter()
            .rect_filled(header_rect, 0.0, ui.visuals().window_fill);

        // Draw slightly different background for the piano key area equivalent
        let key_area_rect =
            egui::Rect::from_min_size(header_rect.min, egui::vec2(self.key_width, header_height));
        ui.painter()
            .rect_filled(key_area_rect, 0.0, ui.visuals().extreme_bg_color);

        // Draw the header content offset by key_width
        let header_content_rect = egui::Rect::from_min_size(
            egui::pos2(header_rect.left() + self.key_width, header_rect.top()),
            egui::vec2(header_rect.width() - self.key_width, header_height),
        );

        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(header_content_rect), |ui| {
            ui.horizontal(|ui| {
                ui.label("Automation:");

                // Add lane button with popup menu
                ui.menu_button("➕ Add Lane", |ui| {
                    // Show available automation parameters
                    if ui.button("Velocity").clicked() {
                        if !has_velocity_lane {
                            self.command_collector
                                .add_command(DawCommand::AddAutomationLane {
                                    clip_id: clip_id.to_string(),
                                    parameter: AutomationParameter::Velocity,
                                });
                        } else {
                            // Find velocity lane and toggle visibility
                            if let Some(lanes) = automation_lanes {
                                if let Some(lane) = lanes
                                    .iter()
                                    .find(|l| matches!(l.parameter, AutomationParameter::Velocity))
                                {
                                    self.command_collector.add_command(
                                        DawCommand::SetAutomationLaneVisibility {
                                            clip_id: clip_id.to_string(),
                                            lane_id: lane.id.clone(),
                                            visible: !lane.visible,
                                        },
                                    );
                                }
                            }
                        }
                        ui.close_menu();
                    }

                    ui.separator();
                    ui.label("MIDI CC:");

                    // Search field
                    ui.horizontal(|ui| {
                        ui.label("🔍");
                        ui.text_edit_singleline(&mut self.cc_search_query);
                        if ui.button("✕").clicked() {
                            self.cc_search_query.clear();
                        }
                    });

                    ui.separator();

                    // Scrollable area for CC list
                    egui::ScrollArea::vertical()
                        .max_height(300.0)
                        .show(ui, |ui| {
                            // Get all CC definitions
                            let all_ccs = get_all_midi_cc();

                            // Filter based on search query
                            let search_lower = self.cc_search_query.to_lowercase();
                            let filtered_ccs: Vec<_> = all_ccs
                                .iter()
                                .filter(|(cc, name)| {
                                    if search_lower.is_empty() {
                                        true
                                    } else {
                                        cc.to_string().contains(&search_lower)
                                            || name.to_lowercase().contains(&search_lower)
                                    }
                                })
                                .collect();

                            // Show common CCs first if no search
                            if search_lower.is_empty() {
                                ui.label("Common:");
                                for (cc, name) in common_midi_cc() {
                                    if ui.button(format!("CC{} - {}", cc, name)).clicked() {
                                        self.add_or_show_cc_lane(
                                            clip_id,
                                            cc,
                                            name,
                                            automation_lanes,
                                        );
                                        self.cc_search_query.clear();
                                        ui.close_menu();
                                    }
                                }
                                ui.separator();
                                ui.label("All CC:");
                            }

                            // Show filtered CCs
                            for (cc, name) in filtered_ccs {
                                if ui.button(format!("CC{} - {}", cc, name)).clicked() {
                                    self.add_or_show_cc_lane(clip_id, *cc, name, automation_lanes);
                                    self.cc_search_query.clear();
                                    ui.close_menu();
                                }
                            }
                        });
                });

                ui.separator();

                // Quick toggle buttons for existing lanes
                if let Some(lanes) = automation_lanes {
                    for lane in lanes {
                        let label = format!(
                            "{} {}",
                            if lane.visible { "👁" } else { "👁‍🗨" },
                            lane.parameter.display_name()
                        );

                        if ui.selectable_label(lane.visible, label).clicked() {
                            self.command_collector.add_command(
                                DawCommand::SetAutomationLaneVisibility {
                                    clip_id: clip_id.to_string(),
                                    lane_id: lane.id.clone(),
                                    visible: !lane.visible,
                                },
                            );
                        }
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
        let visible_lanes: Vec<_> = if let Some(lanes) = automation_lanes {
            lanes.iter().filter(|lane| lane.visible).collect()
        } else {
            Vec::new()
        };

        if visible_lanes.is_empty() {
            ui.allocate_new_ui(egui::UiBuilder::new().max_rect(content_rect), |ui| {
                ui.centered_and_justified(|ui| {
                    ui.label("No automation lanes visible. Click '➕ Add Lane' to add automation.");
                });
            });
            return;
        }

        // Keep lane scrolling independent of the lane layout. The offset used to reduce the
        // available height as well, which made lower lanes inaccessible.
        let total_gaps = (visible_lanes.len() - 1) as f32 * lane_gap;
        let total_height = visible_lanes.iter().map(|lane| lane.height).sum::<f32>() + total_gaps;
        let max_scroll = (total_height - content_rect.height()).max(0.0);
        self.automation_scroll_y = self.automation_scroll_y.clamp(0.0, max_scroll);
        ui.input(|input| {
            let pointer_over_content = input
                .pointer
                .hover_pos()
                .is_some_and(|position| content_rect.contains(position));
            if pointer_over_content && input.raw_scroll_delta.y.abs() > f32::EPSILON {
                self.automation_scroll_y =
                    (self.automation_scroll_y - input.raw_scroll_delta.y).clamp(0.0, max_scroll);
            }
        });

        // Draw each visible lane
        let mut current_y = content_rect.top() - self.automation_scroll_y;

        for lane in &visible_lanes {
            let lane_height = lane.height;
            let lane_rect = egui::Rect::from_min_size(
                egui::pos2(content_rect.left(), current_y),
                egui::vec2(content_rect.width(), lane_height),
            );

            if lane_rect.bottom() > content_rect.top() && lane_rect.top() < content_rect.bottom() {
                let lane_id = lane.id.clone();
                ui.allocate_new_ui(
                    egui::UiBuilder::new().max_rect(lane_rect.intersect(content_rect)),
                    |ui| {
                        self.draw_automation_lane(
                            ui, lane_rect, lane_id, clip_id, clip_start, state,
                        );
                    },
                );
            }

            current_y += lane_height + lane_gap;
        }

        if max_scroll > 0.0 {
            let track = egui::Rect::from_min_size(
                egui::pos2(content_rect.right() - 4.0, content_rect.top()),
                egui::vec2(4.0, content_rect.height()),
            );
            let thumb_height = (content_rect.height() * content_rect.height() / total_height)
                .clamp(16.0, content_rect.height());
            let thumb_top = track.top()
                + (track.height() - thumb_height) * (self.automation_scroll_y / max_scroll);
            ui.painter()
                .rect_filled(track, 2.0, ui.visuals().widgets.noninteractive.bg_fill);
            ui.painter().rect_filled(
                egui::Rect::from_min_size(
                    egui::pos2(track.left(), thumb_top),
                    egui::vec2(track.width(), thumb_height),
                ),
                2.0,
                ui.visuals().widgets.inactive.bg_fill,
            );
        }
    }

    fn draw_automation_lane(
        &mut self,
        ui: &mut egui::Ui,
        rect: egui::Rect,
        lane_id: String,
        clip_id: &str,
        clip_start: f64,
        state: &DawState,
    ) {
        let label_width = self.key_width;
        let margin = 4.0;

        // Background
        ui.painter()
            .rect_filled(rect, 4.0, ui.visuals().extreme_bg_color);

        // Label area
        let label_rect = egui::Rect::from_min_size(
            egui::pos2(rect.left() + margin, rect.top() + margin),
            egui::vec2(label_width - margin * 2.0, rect.height() - margin * 2.0),
        );

        // Get lane info from clip
        let (param_name, current_value) = if let Some(track) =
            state.project.tracks.iter().find(|t| {
                t.clips
                    .iter()
                    .any(|c| matches!(c, Clip::Midi { id, .. } if id == clip_id))
            }) {
            if let Some(Clip::Midi {
                automation_lanes, ..
            }) = track
                .clips
                .iter()
                .find(|c| matches!(c, Clip::Midi { id, .. } if id == clip_id))
            {
                if let Some(lane) = automation_lanes.iter().find(|l| l.id == lane_id) {
                    (
                        lane.parameter.display_name(),
                        lane.get_value_at_time(state.current_time),
                    )
                } else {
                    ("Unknown".to_string(), 0.0)
                }
            } else {
                ("Unknown".to_string(), 0.0)
            }
        } else {
            ("Unknown".to_string(), 0.0)
        };

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

        self.draw_automation_curve(ui, curve_rect, &lane_id, clip_id, clip_start, state);
    }

    fn draw_automation_curve(
        &mut self,
        ui: &mut egui::Ui,
        rect: egui::Rect,
        lane_id: &str,
        clip_id: &str,
        clip_start: f64,
        state: &DawState,
    ) {
        let response = ui.allocate_rect(rect, egui::Sense::click_and_drag());

        // Get lane data from clip
        let lane = if let Some(track) = state.project.tracks.iter().find(|t| {
            t.clips
                .iter()
                .any(|c| matches!(c, Clip::Midi { id, .. } if id == clip_id))
        }) {
            if let Some(Clip::Midi {
                automation_lanes, ..
            }) = track
                .clips
                .iter()
                .find(|c| matches!(c, Clip::Midi { id, .. } if id == clip_id))
            {
                if let Some(lane) = automation_lanes.iter().find(|l| l.id == lane_id) {
                    lane.clone()
                } else {
                    return;
                }
            } else {
                return;
            }
        } else {
            return;
        };

        // Check if this is a velocity lane
        let is_velocity_lane = matches!(lane.parameter, AutomationParameter::Velocity);

        // Grid alignment with piano roll
        let grid_rect = rect;

        // Draw grid lines (aligned with piano roll)
        let bpm = state.project.bpm;
        let beat_duration = 60.0 / bpm;
        let pixels_per_bar = self.zoom * (beat_duration * 4.0) as f32;

        let start_bar = (self.scroll_x / pixels_per_bar).floor() as i32;
        let end_bar = ((self.scroll_x + grid_rect.width()) / pixels_per_bar).ceil() as i32;

        // Draw vertical grid lines
        for bar in start_bar..=end_bar {
            let x = grid_rect.left() + bar as f32 * pixels_per_bar - self.scroll_x;

            if x >= grid_rect.left() && x <= grid_rect.right() {
                let is_bar_line = true;
                let color = ui.visuals().widgets.noninteractive.bg_stroke.color;
                ui.painter().line_segment(
                    [
                        egui::pos2(x, grid_rect.top()),
                        egui::pos2(x, grid_rect.bottom()),
                    ],
                    (0.5, color),
                );
            }
        }

        // Draw velocity bars or automation curve
        if is_velocity_lane {
            self.draw_velocity_bars(ui, rect, lane_id, clip_id, state);
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
                let normalized_value =
                    (point.value - lane.min_value) / (lane.max_value - lane.min_value);
                let y = rect.bottom() - (normalized_value as f32 * rect.height());

                if i == 0 {
                    path.push(egui::pos2(x, y));
                } else {
                    // Interpolate between points based on curve type
                    let prev_point = points_to_draw[i - 1];
                    let steps =
                        ((point.time - prev_point.time) * self.zoom as f64 / 2.0).ceil() as usize;

                    for step in 1..=steps {
                        let t = step as f64 / steps as f64;
                        let time = prev_point.time + (point.time - prev_point.time) * t;
                        let value = lane.get_value_at_time(time);

                        let x = rect.left() + (time as f32 * self.zoom) - self.scroll_x;
                        let normalized_value =
                            (value - lane.min_value) / (lane.max_value - lane.min_value);
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

                ui.painter()
                    .add(egui::Shape::line(path, egui::Stroke::new(2.0_f32, color)));
            }

            // Draw points
            let points_to_draw: Vec<_> = lane.points.iter().enumerate().collect();

            for (point_idx, point) in points_to_draw {
                let x = rect.left() + (point.time as f32 * self.zoom) - self.scroll_x;

                if x >= rect.left() - 10.0 && x <= rect.right() + 10.0 {
                    let normalized_value =
                        (point.value - lane.min_value) / (lane.max_value - lane.min_value);
                    let y = rect.bottom() - (normalized_value as f32 * rect.height());

                    let point_rect =
                        egui::Rect::from_center_size(egui::pos2(x, y), egui::vec2(8.0, 8.0));

                    let is_selected = self
                        .selected_automation_points
                        .iter()
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

                    ui.painter().circle_filled(point_rect.center(), 4.0, color);

                    // Handle point interaction
                    let point_response =
                        ui.allocate_rect(point_rect, egui::Sense::click_and_drag());
                    let point_id = point.id.clone();
                    let lane_id = lane.id.clone();

                    // Handle drag start - select immediately on mouse down if not already selected
                    if point_response.drag_started() {
                        if !is_selected {
                            if ui.input(|i| i.modifiers.ctrl || i.modifiers.command) {
                                // Add to selection
                                self.selected_automation_points
                                    .push((lane_id.clone(), point_id.clone()));
                            } else {
                                // Single select
                                self.selected_automation_points.clear();
                                self.selected_automation_points
                                    .push((lane_id.clone(), point_id.clone()));
                            }
                        }
                    }

                    // Handle click without drag (for toggle selection)
                    if point_response.clicked() && !point_response.dragged() {
                        if ui.input(|i| i.modifiers.ctrl || i.modifiers.command) {
                            // Toggle selection
                            let selection = (lane_id.clone(), point_id.clone());
                            if is_selected {
                                self.selected_automation_points.retain(|s| s != &selection);
                            } else {
                                self.selected_automation_points.push(selection);
                            }
                        } else if !is_selected {
                            // Single select if not already selected
                            self.selected_automation_points.clear();
                            self.selected_automation_points
                                .push((lane_id.clone(), point_id.clone()));
                        }
                    }

                    // Handle dragging - now works immediately since we select on drag_started
                    if point_response.dragged() {
                        // Check if this point is selected (it should be after drag_started)
                        let is_selected_now = self
                            .selected_automation_points
                            .iter()
                            .any(|(lid, pid)| lid == &lane_id && pid == &point_id);

                        if is_selected_now {
                            let delta_x = point_response.drag_delta().x / self.zoom;
                            let delta_y = -point_response.drag_delta().y / rect.height();

                            let new_time = (point.time + delta_x as f64).max(0.0);
                            let delta_value = delta_y as f64 * (lane.max_value - lane.min_value);
                            let new_value =
                                (point.value + delta_value).clamp(lane.min_value, lane.max_value);

                            // Update the point using command
                            self.command_collector
                                .add_command(DawCommand::UpdateAutomationPoint {
                                    clip_id: clip_id.to_string(),
                                    lane_id: lane_id.clone(),
                                    point_id: point_id.clone(),
                                    time: Some(new_time),
                                    value: Some(new_value),
                                });
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

            let value =
                lane.min_value + normalized_value as f64 * (lane.max_value - lane.min_value);

            if time >= 0.0 {
                self.command_collector
                    .add_command(DawCommand::AddAutomationPoint {
                        clip_id: clip_id.to_string(),
                        lane_id: lane_id.to_string(),
                        time,
                        value,
                    });
                self.selected_automation_points.clear();
                // Note: We can't immediately add to selection since we don't know the new point's ID
                // This would need to be handled by the command response system
            }
        }

        // Draw playhead
        self.draw_automation_playhead(ui, rect, clip_start, state.current_time);
    }

    fn draw_velocity_bars(
        &mut self,
        ui: &mut egui::Ui,
        rect: egui::Rect,
        lane_id: &str,
        clip_id: &str,
        state: &DawState,
    ) {
        // Get the current clip's MIDI data
        if let EditorView::PianoRoll {
            clip_id, track_id, ..
        } = &state.current_view
        {
            if let Some(track) = state.project.tracks.iter().find(|t| &t.id == track_id) {
                if let Some(Clip::Midi { midi_data, .. }) = track
                    .clips
                    .iter()
                    .find(|c| matches!(c, Clip::Midi { id, .. } if id == clip_id))
                {
                    if let Some(store) = midi_data {
                        // Get visible notes
                        let start_time = self.scroll_x / self.zoom;
                        let end_time = (self.scroll_x + rect.width()) / self.zoom;
                        let notes = store.get_notes_in_range(start_time as f64, end_time as f64);

                        // Draw velocity bar for each note
                        for note in notes {
                            let x_start =
                                rect.left() + (note.start_time as f32 * self.zoom) - self.scroll_x;
                            let x_end = rect.left()
                                + ((note.start_time + note.duration) as f32 * self.zoom)
                                - self.scroll_x;
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
                                egui::Stroke::new(1.0_f32, ui.visuals().window_stroke.color),
                                egui::epaint::StrokeKind::Outside,
                            );

                            // Handle interaction
                            let bar_response = ui.allocate_rect(bar_rect, egui::Sense::drag());
                            if bar_response.dragged() {
                                let delta_y = -bar_response.drag_delta().y;
                                let new_velocity_normalized =
                                    ((bar_height + delta_y) / rect.height()).clamp(0.0, 1.0);
                                let new_velocity = (new_velocity_normalized * 127.0).max(1.0) as u8;

                                // Update note velocity through command system
                                self.command_collector.add_command(
                                    DawCommand::UpdateNoteVelocity {
                                        clip_id: clip_id.clone(),
                                        note_id: note.id.clone(),
                                        velocity: new_velocity,
                                        old_velocity: Some(note.velocity),
                                    },
                                );
                            }

                            // Show velocity value on hover
                            if bar_response.hovered() {
                                ui.painter().text(
                                    egui::pos2(
                                        x_start + bar_width / 2.0,
                                        rect.bottom() - bar_height - 10.0,
                                    ),
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

    fn draw_automation_playhead(
        &self,
        ui: &mut egui::Ui,
        rect: egui::Rect,
        clip_start: f64,
        current_time: f64,
    ) {
        // Calculate relative time within the clip (same as piano roll playhead)
        let relative_time = current_time - clip_start;
        let playhead_x = rect.left() + (relative_time as f32 * self.zoom) - self.scroll_x;

        if playhead_x >= rect.left() && playhead_x <= rect.right() {
            // Use same soft red color as timeline and piano roll
            let playhead_color = egui::Color32::from_rgb(220, 80, 80);
            ui.painter().line_segment(
                [
                    egui::pos2(playhead_x, rect.top()),
                    egui::pos2(playhead_x, rect.bottom()),
                ],
                (2.0, playhead_color),
            );
        }
    }

    fn add_or_show_cc_lane(
        &mut self,
        clip_id: &str,
        cc: u8,
        name: &str,
        automation_lanes: Option<&Vec<AutomationLane>>,
    ) {
        // Check if lane already exists
        let mut found = false;
        let mut lane_id = String::new();

        if let Some(lanes) = automation_lanes {
            for lane in lanes {
                if let AutomationParameter::MidiCC { cc_number, .. } = &lane.parameter {
                    if *cc_number == cc {
                        found = true;
                        lane_id = lane.id.clone();
                        break;
                    }
                }
            }
        }

        if found {
            // Toggle visibility of existing lane
            self.command_collector
                .add_command(DawCommand::SetAutomationLaneVisibility {
                    clip_id: clip_id.to_string(),
                    lane_id,
                    visible: true,
                });
        } else {
            // Create new lane
            self.command_collector
                .add_command(DawCommand::AddAutomationLane {
                    clip_id: clip_id.to_string(),
                    parameter: AutomationParameter::MidiCC {
                        cc_number: cc,
                        name: name.to_string(),
                    },
                });
        }
    }
}

fn notes_for_ids(state: &DawState, clip_id: &str, note_ids: &[String]) -> Vec<Note> {
    state
        .project
        .tracks
        .iter()
        .flat_map(|track| &track.clips)
        .find_map(|clip| match clip {
            Clip::Midi {
                id,
                midi_data: Some(store),
                ..
            } if id == clip_id => Some(
                note_ids
                    .iter()
                    .filter_map(|note_id| store.get_note(note_id).cloned())
                    .collect(),
            ),
            _ => None,
        })
        .unwrap_or_default()
}

fn clamp_pitch_delta(notes: &[Note], proposed_delta: i32) -> i32 {
    let min_pitch = notes
        .iter()
        .map(|note| i32::from(note.key))
        .min()
        .unwrap_or(0);
    let max_pitch = notes
        .iter()
        .map(|note| i32::from(note.key))
        .max()
        .unwrap_or(127);
    proposed_delta.clamp(-min_pitch, 127 - max_pitch)
}

fn calculate_clamped_resize_delta(
    notes: &[Note],
    proposed_delta: f64,
    edge: ResizeEdge,
    min_duration: f64,
) -> f64 {
    let shrink_limit = notes
        .iter()
        .map(|note| note.duration - min_duration)
        .reduce(f64::min)
        .unwrap_or(0.0);
    match edge {
        ResizeEdge::Left => {
            if proposed_delta >= 0.0 {
                let expand_limit = notes
                    .iter()
                    .map(|note| note.start_time)
                    .reduce(f64::min)
                    .unwrap_or(0.0);
                proposed_delta.min(expand_limit)
            } else {
                proposed_delta.max(-shrink_limit)
            }
        }
        ResizeEdge::Right => proposed_delta.max(-shrink_limit),
    }
}

fn resized_note_times(note: &Note, edge: ResizeEdge, delta: f64, min_duration: f64) -> (f64, f64) {
    match edge {
        ResizeEdge::Left => (
            (note.start_time - delta).max(0.0),
            (note.duration + delta).max(min_duration),
        ),
        ResizeEdge::Right => (note.start_time, (note.duration + delta).max(min_duration)),
    }
}

fn apply_note_gesture_preview(note: &mut Note, gesture: &NoteGesture) {
    match gesture {
        NoteGesture::Move {
            initial_notes,
            delta_time,
            delta_pitch,
            ..
        } => {
            if let Some(initial) = initial_notes.iter().find(|initial| initial.id == note.id) {
                note.start_time = initial.start_time + delta_time;
                note.key = (i16::from(initial.key) + i16::from(*delta_pitch)).clamp(0, 127) as u8;
            }
        }
        NoteGesture::Resize {
            initial_notes,
            edge,
            delta,
        } => {
            if let Some(initial) = initial_notes.iter().find(|initial| initial.id == note.id) {
                (note.start_time, note.duration) = resized_note_times(initial, *edge, *delta, 0.1);
            }
        }
    }
}

fn duplicate_time_offset(notes: &[Note]) -> f64 {
    let min_start = notes
        .iter()
        .map(|note| note.start_time)
        .min_by(|left, right| left.partial_cmp(right).unwrap())
        .unwrap_or(0.0);
    let max_end = notes
        .iter()
        .map(|note| note.start_time + note.duration)
        .max_by(|left, right| left.partial_cmp(right).unwrap())
        .unwrap_or(0.0);

    max_end - min_start
}

fn grid_subdivisions_per_beat(snap_mode: SnapMode, bpm: f64) -> Option<i32> {
    let division = snap_mode.get_division(bpm);
    division
        .is_normal()
        .then(|| ((60.0 / bpm) / division).round() as i32)
        .filter(|subdivisions| *subdivisions > 0)
}

// Helper functions for arrow key nudging

fn clamp_time_delta_for_notes(
    note_ids: &[String],
    proposed_delta: f64,
    clip_id: &str,
    state: &DawState,
) -> f64 {
    if proposed_delta >= 0.0 {
        return proposed_delta; // Moving forward always OK
    }

    // Find earliest note start time
    let mut min_start = f64::MAX;
    for track in &state.project.tracks {
        if let Some(Clip::Midi {
            id,
            midi_data: Some(store),
            ..
        }) = track
            .clips
            .iter()
            .find(|c| matches!(c, Clip::Midi { id, .. } if id == clip_id))
        {
            for note_id in note_ids {
                if let Some(note) = store.get_note(note_id) {
                    min_start = min_start.min(note.start_time);
                }
            }
        }
    }

    // Don't allow moving before time 0
    proposed_delta.max(-min_start)
}

fn clamp_pitch_delta_for_notes(
    note_ids: &[String],
    proposed_delta: i32,
    clip_id: &str,
    state: &DawState,
) -> i32 {
    let mut min_pitch = 127i32;
    let mut max_pitch = 0i32;

    for track in &state.project.tracks {
        if let Some(Clip::Midi {
            id,
            midi_data: Some(store),
            ..
        }) = track
            .clips
            .iter()
            .find(|c| matches!(c, Clip::Midi { id, .. } if id == clip_id))
        {
            for note_id in note_ids {
                if let Some(note) = store.get_note(note_id) {
                    min_pitch = min_pitch.min(note.key as i32);
                    max_pitch = max_pitch.max(note.key as i32);
                }
            }
        }
    }

    // Clamp to keep all notes within 0-127
    if proposed_delta > 0 {
        proposed_delta.min(127 - max_pitch)
    } else {
        proposed_delta.max(-min_pitch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(id: &str, start_time: f64, duration: f64, key: u8) -> Note {
        Note {
            id: id.to_string(),
            channel: 0,
            key,
            velocity: 100,
            start_time,
            duration,
            start_tick: 0,
            duration_ticks: 0,
        }
    }

    #[test]
    fn pitch_delta_is_clamped_for_the_whole_selection() {
        let notes = vec![note("low", 0.0, 1.0, 2), note("high", 0.0, 1.0, 126)];

        assert_eq!(clamp_pitch_delta(&notes, -12), -2);
        assert_eq!(clamp_pitch_delta(&notes, 12), 1);
    }

    #[test]
    fn left_resize_preserves_note_end() {
        let initial = note("note", 2.0, 1.0, 60);

        let (start, duration) = resized_note_times(&initial, ResizeEdge::Left, 0.5, 0.1);

        assert_eq!(start, 1.5);
        assert_eq!(duration, 1.5);
        assert_eq!(start + duration, 3.0);
    }

    #[test]
    fn group_resize_respects_shortest_note() {
        let notes = vec![note("short", 1.0, 0.2, 60), note("long", 1.0, 2.0, 64)];

        let delta = calculate_clamped_resize_delta(&notes, -1.0, ResizeEdge::Right, 0.1);

        assert!((delta + 0.1).abs() < f64::EPSILON);
    }

    #[test]
    fn grid_omits_subdivisions_when_snapping_is_disabled() {
        assert_eq!(grid_subdivisions_per_beat(SnapMode::None, 120.0), None);
    }

    #[test]
    fn duplicate_offset_preserves_the_selected_rhythm() {
        let notes = vec![note("first", 1.0, 0.25, 60), note("second", 1.5, 1.0, 64)];
        let offset = duplicate_time_offset(&notes);

        assert_eq!(offset, 1.5);
        assert_eq!(notes[0].start_time + offset, 2.5);
        assert_eq!(notes[1].start_time + offset, 3.0);
    }
}
