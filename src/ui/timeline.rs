#![allow(unused_variables)]
#![allow(unused_imports)]

use crate::core::utils::SnapHandler;
use crate::core::*;
use eframe::egui;
use eframe::epaint::StrokeKind;

const ADD_TRACK_AREA_HEIGHT: f32 = 50.0;

pub struct Timeline {
    pixels_per_second: f32,
    scroll_offset: f32,
    scroll_y: f32,
    snap_enabled: bool,
    track_height: f32,
    track_header_width: f32,
    drag_start: Option<(egui::Pos2, f32)>, // (pointer_pos, clip_start_time)
    command_collector: CommandCollector,
    midi_ports: Vec<String>,
    midi_input_ports: Vec<String>,
    pending_midi_connections: Vec<(String, String)>, // (track_id, device_name)
    // Resize state
    resize_snap_handler: SnapHandler,
    resize_initial_values: Option<(f32, f32)>, // (start_time, length)
    // Track reordering state
    dragging_track: Option<(usize, f32)>, // (track_index, y_offset)
    // Device panel state
    device_panel_height: f32,
    show_device_panel: bool,
    // Track name editing state
    editing_track_name: Option<(String, String)>, // (track_id, current_text)
    track_name_needs_focus: bool,
    playback_schedule_dirty: bool,
}

impl Default for Timeline {
    fn default() -> Self {
        Self {
            pixels_per_second: 100.0,
            scroll_offset: 0.0,
            scroll_y: 0.0,
            snap_enabled: true,
            track_height: 48.0, // Reduced from 80 - MIDI config moved to device panel
            track_header_width: 200.0,
            drag_start: None,
            command_collector: CommandCollector::new(),
            midi_ports: Vec::new(),
            midi_input_ports: Vec::new(),
            pending_midi_connections: Vec::new(),
            resize_snap_handler: SnapHandler::new(10.0),
            resize_initial_values: None,
            dragging_track: None,
            device_panel_height: 100.0,
            show_device_panel: true,
            editing_track_name: None,
            track_name_needs_focus: false,
            playback_schedule_dirty: false,
        }
    }
}

impl Timeline {
    pub fn update_midi_ports(&mut self, ports: Vec<String>) {
        self.midi_ports = ports;
    }

    /// Updates the MIDI input ports shown by the recording-routing controls.
    /// Output ports continue to be provided through `update_midi_ports`.
    pub fn update_midi_input_ports(&mut self, ports: Vec<String>) {
        self.midi_input_ports = ports;
    }

    fn get_clip_id(&self, clip: &Clip) -> String {
        let Clip::Midi { id, .. } = clip;
        id.clone()
    }

    fn get_color_palette() -> Vec<(&'static str, &'static str)> {
        vec![
            ("White", "#ffffff"),
            ("Black", "#000000"),
            ("Gray", "#3F3F46"),
            ("Red", "#fca5a5"),
            ("Orange", "#fdba74"),
            ("Amber", "#fcd34d"),
            ("Yellow", "#fde047"),
            ("Lime", "#bef264"),
            ("Green", "#86efac"),
            ("Emerald", "#6ee7b7"),
            ("Teal", "#5eead4"),
            ("Cyan", "#67e8f9"),
            ("Sky", "#7dd3fc"),
            ("Blue", "#93c5fd"),
            ("Indigo", "#a5b4fc"),
            ("Violet", "#c4b5fd"),
            ("Purple", "#d8b4fe"),
            ("Fuchsia", "#f0abfc"),
            ("Pink", "#f9a8d4"),
            ("Rose", "#fda4af"),
        ]
    }

    pub fn take_pending_midi_connections(&mut self) -> Vec<(String, String)> {
        std::mem::take(&mut self.pending_midi_connections)
    }

    pub fn take_playback_schedule_dirty(&mut self) -> bool {
        std::mem::take(&mut self.playback_schedule_dirty)
    }
    pub fn show(&mut self, ui: &mut egui::Ui, state: &mut DawState) -> Vec<DawCommand> {
        let (full_rect, response) =
            ui.allocate_exact_size(ui.available_size(), egui::Sense::drag());

        let ruler_height = 20.0;
        let divider_height = 4.0;

        // Calculate device panel height (only show if track selected)
        let device_panel_height = if self.show_device_panel && state.selected_track.is_some() {
            self.device_panel_height.clamp(80.0, 200.0)
        } else {
            0.0
        };

        // Main content area (excluding device panel)
        let main_height = full_rect.height()
            - device_panel_height
            - if device_panel_height > 0.0 {
                divider_height
            } else {
                0.0
            };

        // Split into regions
        let header_width = self.track_header_width;

        // Header area (left side, below ruler)
        let header_rect = egui::Rect::from_min_size(
            egui::pos2(full_rect.left(), full_rect.top() + ruler_height),
            egui::vec2(header_width, main_height - ruler_height),
        );

        // Timeline area (right side, including ruler)
        let timeline_rect = egui::Rect::from_min_size(
            egui::pos2(full_rect.left() + header_width, full_rect.top()),
            egui::vec2(full_rect.width() - header_width, main_height),
        );

        // Tracks area (timeline minus ruler)
        let tracks_rect = egui::Rect::from_min_size(
            egui::pos2(timeline_rect.left(), timeline_rect.top() + ruler_height),
            egui::vec2(timeline_rect.width(), timeline_rect.height() - ruler_height),
        );

        // Ruler area (only above timeline)
        let ruler_rect = egui::Rect::from_min_size(
            timeline_rect.min,
            egui::vec2(timeline_rect.width(), ruler_height),
        );

        // Device panel areas
        let divider_rect = egui::Rect::from_min_size(
            egui::pos2(full_rect.left(), full_rect.top() + main_height),
            egui::vec2(full_rect.width(), divider_height),
        );
        let device_panel_rect = egui::Rect::from_min_size(
            egui::pos2(full_rect.left(), divider_rect.bottom()),
            egui::vec2(full_rect.width(), device_panel_height),
        );

        // Draw timeline background and grid
        self.draw_background(ui, tracks_rect);
        self.draw_grid(ui, tracks_rect, state);

        // Handle interactions
        self.handle_zooming(ui, timeline_rect);
        self.handle_scrolling(ui, &response, state.project.tracks.len(), tracks_rect);
        self.handle_file_drops(ui, state);
        self.handle_delete_clip(ui, state);
        self.handle_escape_key(ui);

        // Draw components
        self.draw_track_headers(ui, header_rect, state);
        self.draw_tracks(ui, tracks_rect, state);
        self.draw_ruler(ui, ruler_rect, state);
        self.handle_loop_region(ui, tracks_rect, state);
        self.draw_playhead(ui, timeline_rect, state);
        self.draw_punch_points(ui, tracks_rect, state);

        // Draw device panel if visible
        if device_panel_height > 0.0 {
            self.draw_device_panel_divider(ui, divider_rect);
            self.draw_device_panel(ui, device_panel_rect, state);
        }

        self.command_collector.take_commands()
    }

    fn draw_background(&self, ui: &mut egui::Ui, rect: egui::Rect) {
        ui.painter()
            .rect_filled(rect, 0.0, ui.visuals().extreme_bg_color);
    }

    fn draw_grid(&self, ui: &mut egui::Ui, rect: egui::Rect, state: &DawState) {
        let bpm = state.project.bpm;
        let beat_duration = 60.0 / bpm;
        let bar_duration = beat_duration * 4.0;

        let pixels_per_beat = self.pixels_per_second * beat_duration as f32;
        let pixels_per_bar = pixels_per_beat * 4.0;

        let start_time = self.scroll_offset / self.pixels_per_second;
        let end_time = (self.scroll_offset + rect.width()) / self.pixels_per_second;

        let start_bar = ((start_time as f64) / bar_duration).floor() as i32;
        let end_bar = ((end_time as f64) / bar_duration).ceil() as i32;

        let division = state.snap_mode.get_division(bpm);
        let subdivisions_per_beat = if division > 0.0 {
            (beat_duration / division).round() as i32
        } else {
            1
        };
        let pixels_per_division = pixels_per_beat / subdivisions_per_beat.max(1) as f32;

        // Grid colors - explicit for consistent visibility
        let bar_shade_even = egui::Color32::from_rgba_unmultiplied(45, 45, 50, 255);
        let bar_shade_odd = egui::Color32::from_rgba_unmultiplied(40, 40, 45, 255);
        let bar_line_color = egui::Color32::from_rgba_unmultiplied(100, 100, 110, 200);
        let beat_line_color = egui::Color32::from_rgba_unmultiplied(70, 70, 80, 150);
        let subdivision_color = egui::Color32::from_rgba_unmultiplied(60, 60, 65, 80);

        // Draw alternating bar backgrounds
        for bar in start_bar..=end_bar {
            let x = rect.left() + (bar as f32 * pixels_per_bar) - self.scroll_offset;
            let bar_rect = egui::Rect::from_min_size(
                egui::pos2(x, rect.top()),
                egui::vec2(pixels_per_bar, rect.height()),
            );
            let shade = if bar % 2 == 0 {
                bar_shade_even
            } else {
                bar_shade_odd
            };
            ui.painter().rect_filled(bar_rect, 0.0, shade);
        }

        // Draw grid lines
        for bar in start_bar..=end_bar {
            let x = rect.left() + (bar as f32 * pixels_per_bar) - self.scroll_offset;

            // Bar line (strongest)
            ui.painter().line_segment(
                [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
                egui::Stroke::new(1.5_f32, bar_line_color),
            );

            // Beat lines (skip beat 0 - it's the bar line)
            for beat in 1..4 {
                let beat_x = x + (beat as f32 * pixels_per_beat);
                if beat_x > rect.left() && beat_x < rect.right() {
                    ui.painter().line_segment(
                        [
                            egui::pos2(beat_x, rect.top()),
                            egui::pos2(beat_x, rect.bottom()),
                        ],
                        egui::Stroke::new(1.0_f32, beat_line_color),
                    );
                }
            }

            // Subdivision lines (only when zoomed in enough)
            if pixels_per_beat > 40.0 && subdivisions_per_beat > 1 {
                for beat in 0..4 {
                    for sub in 1..subdivisions_per_beat {
                        let sub_x = x
                            + (beat as f32 + sub as f32 / subdivisions_per_beat as f32)
                                * pixels_per_beat;
                        if sub_x > rect.left() && sub_x < rect.right() {
                            ui.painter().line_segment(
                                [
                                    egui::pos2(sub_x, rect.top()),
                                    egui::pos2(sub_x, rect.bottom()),
                                ],
                                egui::Stroke::new(0.5_f32, subdivision_color),
                            );
                        }
                    }
                }
            }
        }
    }

    fn handle_zooming(&mut self, ui: &mut egui::Ui, rect: egui::Rect) {
        if ui.input(|i| i.modifiers.ctrl) {
            ui.input(|i| {
                if let Some(mouse_pos) = i.pointer.hover_pos() {
                    if !rect.contains(mouse_pos) {
                        return;
                    }
                    let zoom_delta = i.raw_scroll_delta.y * 0.01;

                    // Calculate the exact time at mouse position before zooming
                    let mouse_offset = mouse_pos.x - rect.left();
                    let time_at_mouse =
                        (mouse_offset + self.scroll_offset) / self.pixels_per_second;

                    // Calculate and apply new zoom level
                    self.pixels_per_second = (self.pixels_per_second * (1.0 + zoom_delta))
                        .max(10.0)
                        .min(500.0);

                    // Calculate new scroll offset to maintain mouse position
                    let new_pixel_offset = time_at_mouse * self.pixels_per_second;
                    self.scroll_offset = new_pixel_offset - mouse_offset;
                }
            });
        }
    }

    fn handle_scrolling(
        &mut self,
        ui: &egui::Ui,
        response: &egui::Response,
        track_count: usize,
        scroll_rect: egui::Rect,
    ) {
        if response.dragged() {
            let invert = -1.0; // Make dragging intuitive
            let delta = response.drag_delta();
            self.scroll_offset = (self.scroll_offset + delta.x * invert).max(0.0);
        }

        let pointer_over_scroll_area = ui.input(|input| {
            input
                .pointer
                .hover_pos()
                .is_some_and(|position| scroll_rect.contains(position))
        });
        if pointer_over_scroll_area {
            ui.input(|input| {
                if input.modifiers.shift {
                    let horizontal = input.raw_scroll_delta.x;
                    let scroll_delta = if horizontal.abs() > f32::EPSILON {
                        horizontal
                    } else {
                        -input.raw_scroll_delta.y
                    };
                    self.scroll_offset = (self.scroll_offset + scroll_delta).max(0.0);
                } else if !input.modifiers.ctrl {
                    self.scroll_y -= input.raw_scroll_delta.y;
                }
            });
        }

        self.scroll_y = self.scroll_y.clamp(
            0.0,
            vertical_scroll_limit(track_count, self.track_height, scroll_rect.height()),
        );
    }

    fn handle_file_drops(&mut self, ui: &mut egui::Ui, state: &mut DawState) {
        let mut files = ui.input(|i| i.raw.dropped_files.clone());
        if let Some(file) = files.pop() {
            println!("Dropping files");
            if let Some(path) = file.path {
                println!("Dropping file: {:?}", path);

                if let Some(pos) = ui.input(|i| i.pointer.hover_pos()) {
                    // TODO: Wrong, use util and cleanup
                    let time = (pos.x + self.scroll_offset) / self.pixels_per_second;

                    println!("Dropping file at time: {}", time);

                    if let Some(track_id) = &state.selected_track {
                        println!("Dropping file on track: {}", track_id);

                        let extension = path
                            .extension()
                            .and_then(|e| e.to_str())
                            .unwrap_or("")
                            .to_lowercase();
                        let is_midi = extension == "mid" || extension == "midi";

                        println!(
                            "name : {}, extension: {}, is_midi: {}",
                            path.display(),
                            extension,
                            is_midi
                        );

                        if let Some(track) = state.project.tracks.iter().find(|t| &t.id == track_id)
                        {
                            let TrackType::Midi { .. } = &track.track_type;
                            if is_midi {
                                self.command_collector.add_command(DawCommand::AddClip {
                                    track_id: track_id.clone(),
                                    start_time: time as f64,
                                    length: 10.0,
                                    file_path: path,
                                });
                            }
                        }
                    }
                }
            }
        }
    }

    fn handle_delete_clip(&mut self, ui: &mut egui::Ui, state: &mut DawState) {
        if self.editing_track_name.is_none()
            && !ui.ctx().wants_keyboard_input()
            && ui.input(|i| i.key_pressed(egui::Key::Delete))
        {
            if let Some(clip_id) = &state.selected_clip {
                for track in &state.project.tracks {
                    if let Some(_clip) = track.clips.iter().find(|c| {
                        let Clip::Midi { id, .. } = c;
                        id == clip_id
                    }) {
                        self.command_collector.add_command(DawCommand::DeleteClip {
                            track_id: track.id.clone(),
                            clip_id: clip_id.clone(),
                        });
                        break;
                    }
                }
            }
        }
    }

    fn handle_escape_key(&mut self, ui: &mut egui::Ui) {
        if self.editing_track_name.is_none()
            && !ui.ctx().wants_keyboard_input()
            && ui.input(|i| i.key_pressed(egui::Key::Escape))
        {
            self.command_collector.add_command(DawCommand::DeselectAll);
        }
    }

    fn handle_loop_region(&mut self, ui: &mut egui::Ui, rect: egui::Rect, state: &mut DawState) {
        if state.loop_enabled {
            let loop_start_x =
                rect.left() + state.loop_start as f32 * self.pixels_per_second - self.scroll_offset;
            let loop_end_x =
                rect.left() + state.loop_end as f32 * self.pixels_per_second - self.scroll_offset;

            let loop_rect = egui::Rect::from_min_max(
                egui::pos2(loop_start_x, rect.top()),
                egui::pos2(loop_end_x, rect.bottom()),
            );

            ui.painter().rect_filled(
                loop_rect,
                0.0,
                ui.visuals().selection.bg_fill.linear_multiply(0.2),
            );

            let marker_height = 10.0;
            let marker_width = 2.0;

            ui.painter().rect_filled(
                egui::Rect::from_min_max(
                    egui::pos2(loop_start_x - marker_width / 2.0, rect.top()),
                    egui::pos2(
                        loop_start_x + marker_width / 2.0,
                        rect.top() + marker_height,
                    ),
                ),
                0.0,
                ui.visuals().selection.stroke.color,
            );

            ui.painter().rect_filled(
                egui::Rect::from_min_max(
                    egui::pos2(loop_end_x - marker_width / 2.0, rect.top()),
                    egui::pos2(loop_end_x + marker_width / 2.0, rect.top() + marker_height),
                ),
                0.0,
                ui.visuals().selection.stroke.color,
            );

            let start_handle = egui::Rect::from_min_max(
                egui::pos2(loop_start_x - 5.0, rect.top()),
                egui::pos2(loop_start_x + 5.0, rect.top() + marker_height),
            );
            let end_handle = egui::Rect::from_min_max(
                egui::pos2(loop_end_x - 5.0, rect.top()),
                egui::pos2(loop_end_x + 5.0, rect.top() + marker_height),
            );

            let start_response = ui.allocate_rect(start_handle, egui::Sense::drag());
            let end_response = ui.allocate_rect(end_handle, egui::Sense::drag());

            // Handle start handle dragging
            if start_response.dragged() {
                let delta = start_response.drag_delta().x / self.pixels_per_second;

                let new_start_snap = if self.snap_enabled {
                    TimeUtils::snap_time(
                        (state.loop_start + delta as f64).max(0.0),
                        state.project.bpm,
                        state.snap_mode,
                    )
                } else {
                    (state.loop_start + delta as f64).max(0.0)
                };

                state.loop_start = new_start_snap;
            }

            // Handle end handle dragging
            if end_response.dragged() {
                let delta = end_response.drag_delta().x / self.pixels_per_second;
                let new_end_snap = if self.snap_enabled {
                    TimeUtils::snap_time(
                        (state.loop_end + delta as f64).max(state.loop_start + 0.1),
                        state.project.bpm,
                        state.snap_mode,
                    )
                } else {
                    (state.loop_end + delta as f64).max(state.loop_start + 0.1)
                };

                state.loop_end = new_end_snap;
            }

            // Show cursor change when hovering over loop handles
            if start_response.hovered() || end_response.hovered() {
                ui.output_mut(|o| o.cursor_icon = egui::CursorIcon::ResizeHorizontal);
            }
        }
    }

    fn draw_ruler(&mut self, ui: &mut egui::Ui, rect: egui::Rect, state: &DawState) {
        // Store and set the clip rect for ruler area
        let original_clip_rect = ui.clip_rect();
        ui.set_clip_rect(rect);

        // Fill the ruler background to prevent grid line bleeding
        let ruler_bg_color = ui.visuals().extreme_bg_color.linear_multiply(1.2);
        ui.painter().rect_filled(rect, 0.0, ruler_bg_color);

        let response = ui.allocate_rect(rect, egui::Sense::click_and_drag());

        const EDGE_SCROLL_MARGIN: f32 = 50.0; // Pixels from edge where scrolling starts
        const EDGE_SCROLL_SPEED: f32 = 10.0; // Pixels per frame when scrolling

        if response.dragged() {
            if let Some(pos) = response.hover_pos() {
                // todo: cleanup this so we dont get accelleration and jumping when seeking
                if !state.playing {
                    if pos.x < rect.left() + EDGE_SCROLL_MARGIN {
                        self.scroll_offset = self.scroll_offset - EDGE_SCROLL_SPEED;
                    } else if pos.x > rect.right() - EDGE_SCROLL_MARGIN {
                        self.scroll_offset += EDGE_SCROLL_SPEED;
                    }
                }

                // Convert viewport position to time
                let viewport_x = pos.x - rect.left();
                let viewport_time = viewport_x / self.pixels_per_second;
                let absolute_time = viewport_time + (self.scroll_offset / self.pixels_per_second);

                self.command_collector.add_command(DawCommand::SeekTime {
                    time: absolute_time as f64,
                });
            }
        } else if response.clicked() {
            if let Some(pos) = response.hover_pos() {
                let viewport_x = pos.x - rect.left();
                let viewport_time = viewport_x / self.pixels_per_second;
                let absolute_time = viewport_time + (self.scroll_offset / self.pixels_per_second);

                self.command_collector.add_command(DawCommand::SeekTime {
                    time: absolute_time as f64,
                });
            }
        }

        if response.hovered() {
            ui.output_mut(|o| o.cursor_icon = egui::CursorIcon::PointingHand);
        }

        // Draw bar markers instead of time
        let bpm = state.project.bpm;
        let beat_duration = 60.0 / bpm;
        let bar_duration = (beat_duration * 4.0) as f32;
        let pixels_per_bar = self.pixels_per_second * bar_duration;

        let start_bar = (self.scroll_offset / pixels_per_bar).floor() as i32;
        let end_bar = ((self.scroll_offset + rect.width()) / pixels_per_bar).ceil() as i32;

        // Ruler colors
        let ruler_text = egui::Color32::from_rgba_unmultiplied(180, 180, 185, 255);
        let tick_color = egui::Color32::from_rgba_unmultiplied(120, 120, 130, 200);

        for bar in start_bar..=end_bar {
            let x = rect.left() + (bar as f32 * pixels_per_bar) - self.scroll_offset;

            // Tick mark
            ui.painter().line_segment(
                [
                    egui::pos2(x, rect.bottom() - 8.0),
                    egui::pos2(x, rect.bottom()),
                ],
                egui::Stroke::new(1.0_f32, tick_color),
            );

            // Bar number (1-indexed for musicians)
            ui.painter().text(
                egui::pos2(x + 4.0, rect.top() + 3.0),
                egui::Align2::LEFT_TOP,
                format!("{}", bar + 1),
                egui::FontId::monospace(11.0),
                ruler_text,
            );
        }

        // Bottom border
        ui.painter().line_segment(
            [rect.left_bottom(), rect.right_bottom()],
            egui::Stroke::new(1.0_f32, tick_color),
        );

        // Restore original clip rect
        ui.set_clip_rect(original_clip_rect);
    }

    fn draw_track_headers(&mut self, ui: &mut egui::Ui, rect: egui::Rect, state: &mut DawState) {
        // Draw header background
        ui.painter()
            .rect_filled(rect, 0.0, ui.visuals().window_fill);

        // Store original clip rect and set header clip rect
        let original_clip_rect = ui.clip_rect();
        ui.set_clip_rect(rect);

        // Draw track headers manually with scroll offset
        for (track_idx, track) in state.project.tracks.iter().enumerate() {
            let track_top = rect.top() + (track_idx as f32 * self.track_height) - self.scroll_y;
            let track_rect = egui::Rect::from_min_size(
                egui::pos2(rect.left(), track_top),
                egui::vec2(rect.width(), self.track_height),
            );

            // Skip if not visible
            if track_rect.bottom() < rect.top() || track_rect.top() > rect.bottom() {
                continue;
            }

            // Draw track header
            self.draw_track_header(ui, track_rect, track, track_idx, state);
        }

        // Draw "Add Track" button at the bottom
        let total_height = state.project.tracks.len() as f32 * self.track_height;
        let add_track_y = rect.top() + total_height - self.scroll_y;

        // Always show the button, even when there are no tracks
        let button_rect = egui::Rect::from_min_size(
            egui::pos2(rect.left() + 10.0, add_track_y.max(rect.top()) + 10.0),
            egui::vec2(rect.width() - 20.0, 30.0),
        );

        // Only draw if button is within visible area
        if button_rect.bottom() > rect.top() && button_rect.top() < rect.bottom() {
            let response = ui.allocate_rect(button_rect, egui::Sense::click());
            if response.clicked() {
                self.command_collector.add_command(DawCommand::AddTrack {
                    track_type: TrackType::Midi {
                        channel: 1,
                        device_name: None,
                        input_device_name: None,
                        input_channel: None,
                    },
                    name: format!("Track {}", state.project.tracks.len() + 1),
                });
            }

            // Draw button
            let style = if response.hovered() {
                ui.visuals().widgets.hovered
            } else {
                ui.visuals().widgets.inactive
            };

            ui.painter()
                .rect_filled(button_rect, 4.0, style.weak_bg_fill);
            ui.painter().text(
                button_rect.center(),
                egui::Align2::CENTER_CENTER,
                "➕ Add Track",
                egui::FontId::proportional(12.0),
                style.text_color(),
            );
        }

        // Draw drop indicator when dragging
        if let Some((from_index, offset)) = self.dragging_track {
            let tracks_moved = (offset / self.track_height).round() as i32;
            let target_index = (from_index as i32 + tracks_moved).max(0) as usize;
            let target_index = target_index.min(state.project.tracks.len().saturating_sub(1));

            if target_index != from_index {
                let indicator_y =
                    rect.top() + (target_index as f32 * self.track_height) - self.scroll_y;
                let indicator_y = if target_index > from_index {
                    indicator_y + self.track_height // Show below the target track
                } else {
                    indicator_y // Show above the target track
                };

                // Draw insertion line
                ui.painter().line_segment(
                    [
                        egui::pos2(rect.left() + 10.0, indicator_y),
                        egui::pos2(rect.right() - 10.0, indicator_y),
                    ],
                    (3.0, ui.visuals().selection.stroke.color),
                );
            }
        }

        // Restore original clip rect
        ui.set_clip_rect(original_clip_rect);
    }

    fn draw_track_header(
        &mut self,
        ui: &mut egui::Ui,
        rect: egui::Rect,
        track: &Track,
        index: usize,
        state: &DawState,
    ) {
        let is_selected = state.selected_track == Some(track.id.clone());

        // Draw background
        let mut bg_color = if is_selected {
            ui.visuals().selection.bg_fill
        } else if index % 2 == 0 {
            ui.visuals().faint_bg_color
        } else {
            ui.visuals().extreme_bg_color
        };

        // Make dragged track semi-transparent
        if let Some((drag_index, _)) = self.dragging_track {
            if drag_index == index {
                bg_color = bg_color.gamma_multiply(0.5);
            }
        }

        ui.painter().rect_filled(rect, 0.0, bg_color);

        // Split header into drag zone (left) and content zone
        const DRAG_HANDLE_WIDTH: f32 = 16.0;

        let drag_zone =
            egui::Rect::from_min_size(rect.min, egui::vec2(DRAG_HANDLE_WIDTH, rect.height()));

        let content_zone = egui::Rect::from_min_max(
            egui::pos2(rect.min.x + DRAG_HANDLE_WIDTH, rect.min.y),
            rect.max,
        );

        // 1. Drag handle zone - only responds to drag
        let drag_response = ui.allocate_rect(drag_zone, egui::Sense::drag());

        // Draw grip dots (⋮⋮ pattern)
        let grip_color = if drag_response.hovered() || drag_response.dragged() {
            ui.visuals().strong_text_color()
        } else {
            ui.visuals().weak_text_color()
        };
        let center_x = drag_zone.center().x;
        for i in 0..3 {
            let y = drag_zone.center().y + (i as f32 - 1.0) * 4.0;
            ui.painter()
                .circle_filled(egui::pos2(center_x - 2.0, y), 1.0, grip_color);
            ui.painter()
                .circle_filled(egui::pos2(center_x + 2.0, y), 1.0, grip_color);
        }

        // Handle drag start
        if drag_response.drag_started() {
            self.dragging_track = Some((index, 0.0));
        }

        // Handle dragging
        if let Some((drag_index, _)) = self.dragging_track {
            if drag_index == index && drag_response.dragged() {
                let delta_y = drag_response.drag_delta().y;
                if let Some((_, ref mut offset)) = self.dragging_track {
                    *offset += delta_y;
                }

                // Change cursor to indicate dragging
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
            }
        }

        // Handle drag end
        if drag_response.drag_stopped() {
            if let Some((from_index, offset)) = self.dragging_track {
                // Calculate target index based on drag offset
                let tracks_moved = (offset / self.track_height).round() as i32;
                let to_index = (from_index as i32 + tracks_moved).max(0) as usize;
                let to_index = to_index.min(state.project.tracks.len().saturating_sub(1));

                if from_index != to_index {
                    self.command_collector
                        .add_command(DawCommand::ReorderTracks {
                            from_index,
                            to_index,
                        });
                }

                self.dragging_track = None;
            }
        }

        // Show grab cursor on drag handle hover
        if drag_response.hovered() && self.dragging_track.is_none() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
        }

        // 2. Content zone - responds to click for selection
        let content_response = ui.allocate_rect(content_zone, egui::Sense::click());
        if content_response.clicked() {
            self.command_collector.add_command(DawCommand::SelectTrack {
                track_id: track.id.clone(),
            });
        }

        // Draw track color stripe (after drag handle)
        let stripe_rect = egui::Rect::from_min_size(
            egui::pos2(rect.min.x + DRAG_HANDLE_WIDTH, rect.min.y),
            egui::vec2(4.0, rect.height()),
        );
        let track_color =
            hex_to_color32(&track.color).unwrap_or(egui::Color32::from_rgb(253, 224, 71)); // Default yellow
        ui.painter().rect_filled(stripe_rect, 0.0, track_color);

        // Draw separator line at bottom
        ui.painter().line_segment(
            [rect.left_bottom(), rect.right_bottom()],
            (1.0, ui.visuals().widgets.noninteractive.bg_stroke.color),
        );

        // Draw right border to separate from timeline
        ui.painter().line_segment(
            [rect.right_top(), rect.right_bottom()],
            (1.0, ui.visuals().widgets.noninteractive.bg_stroke.color),
        );

        // Content area with padding (accounting for drag handle and color stripe)
        let content_rect = egui::Rect::from_min_size(
            rect.min + egui::vec2(DRAG_HANDLE_WIDTH + 8.0, 6.0),
            egui::vec2(
                rect.width() - DRAG_HANDLE_WIDTH - 14.0,
                rect.height() - 12.0,
            ),
        );

        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(content_rect), |ui| {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 4.0;

                // First row: Track name and controls
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;

                    // Track name (double-click to edit, single-click to select)
                    let is_editing_this_track = self
                        .editing_track_name
                        .as_ref()
                        .map(|(id, _)| id == &track.id)
                        .unwrap_or(false);

                    if is_editing_this_track {
                        // Show text input for editing
                        if let Some((_, ref mut edit_text)) = &mut self.editing_track_name {
                            let text_edit = egui::TextEdit::singleline(edit_text)
                                .desired_width(120.0)
                                .font(egui::TextStyle::Body);

                            let response = ui.add(text_edit);

                            if self.track_name_needs_focus {
                                response.request_focus();
                                self.track_name_needs_focus = false;
                            }

                            // Commit on Enter or focus lost
                            if response.lost_focus()
                                || ui.input(|i| i.key_pressed(egui::Key::Enter))
                            {
                                let new_name = edit_text.clone();
                                let track_id = track.id.clone();

                                // Clear editing state
                                self.editing_track_name = None;
                                self.track_name_needs_focus = false;

                                if !new_name.is_empty() && new_name != track.name {
                                    self.command_collector.add_command(DawCommand::RenameTrack {
                                        track_id,
                                        new_name,
                                    });
                                }
                            }

                            // Cancel on Escape
                            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                                self.editing_track_name = None;
                                self.track_name_needs_focus = false;
                            }
                        }
                    } else {
                        // Show label with click and double-click handling
                        let name_response = ui.add(
                            egui::Label::new(egui::RichText::new(&track.name).size(13.0))
                                .sense(egui::Sense::click()),
                        );

                        if name_response.double_clicked() {
                            // Start editing
                            self.editing_track_name = Some((track.id.clone(), track.name.clone()));
                            self.track_name_needs_focus = true;
                        } else if name_response.clicked() {
                            // Select track
                            self.command_collector.add_command(DawCommand::SelectTrack {
                                track_id: track.id.clone(),
                            });
                        }
                    }

                    // Push buttons to the right
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 3.0;

                        // Menu button
                        ui.push_id(format!("track_menu_{}", track.id), |ui| {
                            ui.menu_button("☰", |ui| {
                                if ui.button("Delete Track").clicked() {
                                    self.command_collector.add_command(DawCommand::DeleteTrack {
                                        track_id: track.id.clone(),
                                    });
                                    ui.close_menu();
                                }
                            })
                            .response
                            .on_hover_text("Track Menu");
                        });

                        // Mute button
                        ui.push_id(format!("mute_{}", track.id), |ui| {
                            let mute_button = egui::Button::new("M").small();
                            let mute_button = if track.is_muted {
                                mute_button.fill(egui::Color32::from_rgb(180, 60, 60))
                            } else {
                                mute_button
                            };

                            if ui.add(mute_button).on_hover_text("Mute").clicked() {
                                if track.is_muted {
                                    self.command_collector.add_command(DawCommand::UnmuteTrack {
                                        track_id: track.id.clone(),
                                    });
                                } else {
                                    self.command_collector.add_command(DawCommand::MuteTrack {
                                        track_id: track.id.clone(),
                                    });
                                }
                            }
                        });

                        // Solo button
                        ui.push_id(format!("solo_{}", track.id), |ui| {
                            let solo_button = egui::Button::new("S").small();
                            let solo_button = if track.is_soloed {
                                solo_button.fill(egui::Color32::from_rgb(180, 180, 60))
                            } else {
                                solo_button
                            };

                            if ui.add(solo_button).on_hover_text("Solo").clicked() {
                                if track.is_soloed {
                                    self.command_collector.add_command(DawCommand::UnsoloTrack {
                                        track_id: track.id.clone(),
                                    });
                                } else {
                                    self.command_collector.add_command(DawCommand::SoloTrack {
                                        track_id: track.id.clone(),
                                    });
                                }
                            }
                        });

                        // Record/Arm button
                        ui.push_id(format!("arm_{}", track.id), |ui| {
                            let arm_button = egui::Button::new("R").small();
                            let arm_button = if track.is_armed {
                                arm_button.fill(egui::Color32::from_rgb(255, 60, 60))
                            } else {
                                arm_button
                            };

                            if ui.add(arm_button).on_hover_text("Record Arm").clicked() {
                                if track.is_armed {
                                    self.command_collector.add_command(DawCommand::UnarmTrack {
                                        track_id: track.id.clone(),
                                    });
                                } else {
                                    self.command_collector.add_command(DawCommand::ArmTrack {
                                        track_id: track.id.clone(),
                                    });
                                }
                            }
                        });

                        // Input monitoring button (only for armed tracks)
                        if track.is_armed {
                            ui.push_id(format!("monitor_{}", track.id), |ui| {
                                let monitor_button = egui::Button::new("I").small();
                                let monitor_button = if track.input_monitoring {
                                    monitor_button.fill(egui::Color32::from_rgb(60, 180, 255))
                                } else {
                                    monitor_button
                                };

                                if ui
                                    .add(monitor_button)
                                    .on_hover_text("Input Monitoring")
                                    .clicked()
                                {
                                    self.command_collector.add_command(
                                        DawCommand::ToggleInputMonitoring {
                                            track_id: track.id.clone(),
                                        },
                                    );
                                }
                            });
                        }

                        // Color picker - use menu_button with space label
                        let current_color = hex_to_color32(&track.color)
                            .unwrap_or(egui::Color32::from_rgb(253, 224, 71));

                        ui.push_id(format!("color_menu_{}", track.id), |ui| {
                            let button_response = ui.menu_button(" ", |ui| {
                                ui.set_min_width(50.0);

                                let palette = Self::get_color_palette();
                                for (name, hex) in palette {
                                    let color = hex_to_color32(hex).unwrap();
                                    ui.horizontal(|ui| {
                                        // Color preview
                                        let (rect, _) = ui.allocate_exact_size(
                                            egui::vec2(16.0, 16.0),
                                            egui::Sense::hover(),
                                        );
                                        ui.painter().rect_filled(rect, 2.0, color);
                                        ui.painter().rect_stroke(
                                            rect,
                                            2.0,
                                            (
                                                1.0,
                                                ui.visuals().widgets.noninteractive.bg_stroke.color,
                                            ),
                                            StrokeKind::Middle,
                                        );

                                        let is_selected = track.color == hex;
                                        if ui.selectable_label(is_selected, name).clicked() {
                                            self.command_collector.add_command(
                                                DawCommand::SetTrackColor {
                                                    track_id: track.id.clone(),
                                                    color: hex.to_string(),
                                                },
                                            );
                                            ui.close_menu();
                                        }
                                    });
                                }
                            });

                            // Draw color indicator on the menu button
                            let button_rect = button_response.response.rect;

                            let color_rect = button_rect.shrink(3.0);
                            ui.painter().rect_filled(color_rect, 2.0, current_color);

                            button_response.response.on_hover_text("Track Color");
                        });
                    });
                });

                // Second row for MIDI settings if track is tall enough
                if self.track_height > 70.0 {
                    let TrackType::Midi {
                        channel,
                        device_name,
                        ..
                    } = &track.track_type;

                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;

                        // MIDI port dropdown
                        let display_text = match device_name {
                            Some(dev) if !dev.is_empty() => dev.as_str(),
                            _ => "No Device",
                        };

                        egui::ComboBox::new(format!("midi_port_{}", track.id), "")
                            .width(ui.available_width() - 60.0)
                            .selected_text(display_text)
                            .show_ui(ui, |ui| {
                                if ui
                                    .selectable_label(device_name.is_none(), "No Device")
                                    .clicked()
                                {
                                    self.pending_midi_connections
                                        .push((track.id.clone(), String::new()));
                                }

                                for port in &self.midi_ports {
                                    let is_selected = device_name.as_ref() == Some(port);
                                    if ui.selectable_label(is_selected, port).clicked() {
                                        self.pending_midi_connections
                                            .push((track.id.clone(), port.clone()));
                                    }
                                }
                            });

                        // MIDI channel selector
                        let mut channel_changed = false;
                        let mut new_channel = *channel;

                        egui::ComboBox::new(format!("midi_channel_{}", track.id), "")
                            .width(45.0)
                            .selected_text(format!("Ch{}", channel))
                            .show_ui(ui, |ui| {
                                for ch in 1..=16 {
                                    if ui
                                        .selectable_value(&mut new_channel, ch, format!("Ch{}", ch))
                                        .clicked()
                                    {
                                        channel_changed = true;
                                    }
                                }
                            });

                        if channel_changed {
                            self.command_collector
                                .add_command(DawCommand::SetTrackMidiChannel {
                                    track_id: track.id.clone(),
                                    channel: new_channel,
                                });
                        }
                    });
                }

                // Right-click context menu for takes
                drag_response.context_menu(|ui| {
                    ui.label("Take Management");
                    ui.separator();

                    if !track.takes.is_empty() {
                        ui.menu_button("Select Take", |ui| {
                            for take in &track.takes {
                                let is_active = track.active_take.as_ref() == Some(&take.id);
                                if ui.selectable_label(is_active, &take.name).clicked() {
                                    self.command_collector.add_command(DawCommand::SelectTake {
                                        track_id: track.id.clone(),
                                        take_id: take.id.clone(),
                                    });
                                    ui.close_menu();
                                }
                            }
                        });

                        ui.menu_button("Delete Take", |ui| {
                            for take in &track.takes {
                                if ui.button(&take.name).clicked() {
                                    self.command_collector.add_command(DawCommand::DeleteTake {
                                        track_id: track.id.clone(),
                                        take_id: take.id.clone(),
                                    });
                                    ui.close_menu();
                                }
                            }
                        });

                        ui.separator();
                    }

                    if ui.button("Clear All Takes").clicked() {
                        // Delete all takes
                        for take in track.takes.clone() {
                            self.command_collector.add_command(DawCommand::DeleteTake {
                                track_id: track.id.clone(),
                                take_id: take.id,
                            });
                        }
                        ui.close_menu();
                    }
                });

                // Show takes if any exist
                if !track.takes.is_empty() {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;

                        ui.label("Takes:");

                        // Takes dropdown
                        let active_take_name = track
                            .active_take
                            .as_ref()
                            .and_then(|take_id| track.takes.iter().find(|t| t.id == *take_id))
                            .map(|t| t.name.clone())
                            .unwrap_or_else(|| "None".to_string());

                        egui::ComboBox::new(format!("takes_{}", track.id), "")
                            .width(ui.available_width())
                            .selected_text(&active_take_name)
                            .show_ui(ui, |ui| {
                                // Option to have no active take
                                if ui
                                    .selectable_label(track.active_take.is_none(), "None")
                                    .clicked()
                                {
                                    self.command_collector.add_command(DawCommand::SelectTake {
                                        track_id: track.id.clone(),
                                        take_id: String::new(),
                                    });
                                }

                                ui.separator();

                                // List all takes
                                for take in &track.takes {
                                    let is_active = track.active_take.as_ref() == Some(&take.id);
                                    let label = if take.is_muted {
                                        format!("{} (muted)", take.name)
                                    } else {
                                        take.name.clone()
                                    };

                                    if ui.selectable_label(is_active, &label).clicked() {
                                        self.command_collector.add_command(
                                            DawCommand::SelectTake {
                                                track_id: track.id.clone(),
                                                take_id: take.id.clone(),
                                            },
                                        );
                                    }
                                }
                            });
                    });
                }
            });
        });
    }

    fn draw_tracks(&mut self, ui: &mut egui::Ui, rect: egui::Rect, state: &mut DawState) {
        // Store and set the clip rect for tracks area
        let original_clip_rect = ui.clip_rect();
        ui.set_clip_rect(rect);

        let start_time = self.scroll_offset / self.pixels_per_second;
        let end_time = (self.scroll_offset + rect.width()) / self.pixels_per_second;

        // Apply vertical scroll offset to tracks
        for (track_idx, track) in state.project.tracks.iter().enumerate() {
            let track_top = rect.top() + track_idx as f32 * self.track_height - self.scroll_y;
            let track_rect = egui::Rect::from_min_max(
                egui::pos2(rect.left(), track_top),
                egui::pos2(rect.right(), track_top + self.track_height),
            );

            // Skip if track is not visible
            if track_rect.bottom() < rect.top() || track_rect.top() > rect.bottom() {
                continue;
            }

            // Draw track background
            if state.selected_track == Some(track.id.clone()) {
                // Highlight selected track
                ui.painter()
                    .rect_filled(track_rect, 0.0, ui.visuals().selection.bg_fill);
            } else if track_idx % 2 == 0 {
                // Odd rows
                ui.painter()
                    .rect_filled(track_rect, 0.0, ui.visuals().faint_bg_color);
            } else {
                // Don't draw anything for even rows
            }

            // Draw track separator
            ui.painter().line_segment(
                [track_rect.left_bottom(), track_rect.right_bottom()],
                (1.0, ui.visuals().window_stroke.color),
            );

            // Handle click on empty track area for deselection and double-click for new clip
            let response = ui.interact(
                track_rect,
                ui.id().with(format!("track_{}", track_idx)),
                egui::Sense::click(),
            );

            // Check if click was on empty area (not on a clip)
            let click_pos = response.hover_pos().unwrap_or_default();
            let click_time =
                (click_pos.x - track_rect.left() + self.scroll_offset) / self.pixels_per_second;

            let clicked_on_clip = track.clips.iter().any(|clip| {
                let Clip::Midi {
                    start_time, length, ..
                } = clip;
                let (start, len) = (*start_time as f32, *length as f32);
                click_time >= start && click_time <= start + len
            });

            // Double-click on empty space creates a new clip
            if response.double_clicked() && !clicked_on_clip {
                let bpm = state.project.bpm;
                let beat_duration = 60.0 / bpm;
                let bar_duration = beat_duration * 4.0; // 4 beats per bar

                // Snap to grid if enabled
                let snapped_time = TimeUtils::snap_time(click_time as f64, bpm, state.snap_mode);

                // Create a new empty MIDI clip (1 bar long)
                self.command_collector.add_command(DawCommand::AddClip {
                    track_id: track.id.clone(),
                    start_time: snapped_time,
                    length: bar_duration,
                    file_path: std::path::PathBuf::new(), // Empty path = new empty clip
                });
            }
            // Single click on empty area deselects
            else if response.clicked() && !clicked_on_clip {
                self.command_collector.add_command(DawCommand::DeselectAll);
            }

            // Draw clips
            for clip in &track.clips {
                // Check if this clip is from the active take
                let is_active_take = track.active_take.is_none()
                    || track
                        .takes
                        .iter()
                        .find(|t| &t.id == track.active_take.as_ref().unwrap_or(&String::new()))
                        .map(|t| t.clip_id == self.get_clip_id(clip))
                        .unwrap_or(true);
                // Draw clip (will handle active/inactive state internally)
                self.draw_clip(ui, track_rect, clip, state, is_active_take);
            }
        }

        // Restore original clip rect
        ui.set_clip_rect(original_clip_rect);
    }

    fn draw_clip(
        &mut self,
        ui: &mut egui::Ui,
        track_rect: egui::Rect,
        clip: &Clip,
        state: &DawState,
        is_active_take: bool,
    ) {
        let Clip::Midi {
            start_time,
            length,
            id: clip_id,
            ..
        } = clip;
        let (start_time, length) = (*start_time as f32, *length as f32);

        let viewport_pos =
            ViewportPosition::new(self.pixels_per_second, self.scroll_offset, track_rect);
        let clip_left = viewport_pos.time_to_x(start_time as f64);
        let clip_width = viewport_pos.duration_to_width(length as f64);

        let clip_rect = egui::Rect::from_min_size(
            egui::pos2(clip_left, track_rect.top() + 2.0),
            egui::vec2(clip_width, track_rect.height() - 4.0),
        );

        // Add interaction handling
        let response = ui.allocate_rect(clip_rect, egui::Sense::click_and_drag());

        // Handle dragging with proper start position tracking
        if response.drag_started() {
            // Store the initial drag position and clip start time
            // Use if-let to gracefully handle focus loss during drag
            if let Some(pos) = response.hover_pos() {
                self.drag_start = Some((pos, start_time));
            }
        }

        if response.dragged() {
            if let Some((drag_start_pos, clip_start_time)) = self.drag_start {
                // Gracefully handle focus loss - skip frame if hover_pos is None
                if let Some(current_pos) = response.hover_pos() {
                    let delta_x = current_pos.x - drag_start_pos.x;
                    let time_delta = delta_x / self.pixels_per_second;

                    let new_start_time = (clip_start_time + time_delta).max(0.0);

                    // Snap to grid if enabled (disable with Shift key)
                    let snap = self.snap_enabled && !ui.input(|i| i.modifiers.shift);
                    let snapped_time = if snap {
                        TimeUtils::snap_time(
                            new_start_time as f64,
                            state.project.bpm,
                            state.snap_mode,
                        ) as f32
                    } else {
                        new_start_time
                    };

                    self.command_collector.add_command(DawCommand::MoveClip {
                        clip_id: clip_id.clone(),
                        track_id: state
                            .project
                            .tracks
                            .iter()
                            .find(|t| t.clips.contains(clip))
                            .map(|t| t.id.clone())
                            .unwrap_or_default(),
                        new_start_time: snapped_time as f64,
                    });
                }
            }
        }

        if response.drag_stopped() {
            self.playback_schedule_dirty |= self.drag_start.is_some();
            self.drag_start = None;
        }

        if response.double_clicked() {
            if let Some(track_id) = state
                .project
                .tracks
                .iter()
                .find(|t| {
                    t.clips.iter().any(|c| {
                        let Clip::Midi { id, .. } = c;
                        id == clip_id
                    })
                })
                .map(|t| t.id.clone())
            {
                self.command_collector
                    .add_command(DawCommand::OpenPianoRoll {
                        clip_id: clip_id.clone(),
                        track_id: track_id.to_string(),
                    });
            }
        }

        // Handle single clicks for selection
        if response.clicked() {
            self.command_collector.add_command(DawCommand::SelectClip {
                clip_id: clip_id.clone(),
            });
        }

        // Draw clip background
        let base_color = egui::Color32::from_rgb(64, 128, 255);

        // Apply opacity for inactive takes
        let clip_color = if is_active_take {
            base_color
        } else {
            base_color.linear_multiply(0.3)
        };

        ui.painter().rect_filled(clip_rect, 2.0, clip_color);

        // Draw clip border
        let is_selected = state.selected_clip == Some(clip_id.clone());

        // Make selection visible
        if is_selected || response.hovered() {
            ui.painter().rect_stroke(
                clip_rect,
                2.0,
                egui::Stroke::new(1.5_f32, ui.visuals().selection.stroke.color),
                StrokeKind::Inside,
            );
        }

        let Clip::Midi { file_path, .. } = clip;
        let clip_name = file_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("New Clip");

        // Draw clip name
        ui.painter().text(
            clip_rect.left_top() + egui::vec2(4.0, 4.0),
            egui::Align2::LEFT_TOP,
            clip_name,
            egui::FontId::proportional(12.0),
            ui.visuals().extreme_bg_color,
        );

        // Draw MIDI preview for MIDI clips
        let Clip::Midi {
            midi_data,
            start_time: clip_start,
            length: clip_length,
            ..
        } = clip;
        if let Some(midi_store) = midi_data {
            self.draw_midi_preview(ui, clip_rect, midi_store, *clip_start, *clip_length);
        }

        // Handle clip resize - reuse start_time and length from earlier extraction

        // Add resize handles on the edges
        let handle_width = 5.0;
        let left_handle = egui::Rect::from_min_size(
            clip_rect.left_top(),
            egui::vec2(handle_width, clip_rect.height()),
        );
        let right_handle = egui::Rect::from_min_size(
            egui::pos2(clip_rect.right() - handle_width, clip_rect.top()),
            egui::vec2(handle_width, clip_rect.height()),
        );

        // Draw resize handles when hovered
        if response.hovered() {
            ui.painter()
                .rect_filled(left_handle, 0.0, ui.visuals().selection.stroke.color);
            ui.painter()
                .rect_filled(right_handle, 0.0, ui.visuals().selection.stroke.color);
        }

        // Handle resizing from left edge
        let left_response = ui.allocate_rect(left_handle, egui::Sense::drag());

        if left_response.drag_started() {
            self.resize_initial_values = Some((start_time, length));
            self.resize_snap_handler.reset();
        }

        if left_response.dragged() {
            if let Some((initial_start, initial_length)) = self.resize_initial_values {
                // Accumulate drag delta
                self.resize_snap_handler
                    .add_delta(left_response.drag_delta().x);
                let accumulated_time_delta =
                    self.resize_snap_handler.get_accumulated() / self.pixels_per_second;

                // Apply snapping if enabled (disable with Shift key)
                let snap = self.snap_enabled && !ui.input(|i| i.modifiers.shift);
                let new_start = self.resize_snap_handler.snap_time_accumulated(
                    initial_start as f64,
                    accumulated_time_delta as f64,
                    state.project.bpm,
                    state.snap_mode,
                    snap,
                ) as f32;

                let new_length = (initial_length + (initial_start - new_start)).max(0.1);

                // Move the clip
                self.command_collector.add_command(DawCommand::MoveClip {
                    clip_id: clip_id.clone(),
                    track_id: state
                        .project
                        .tracks
                        .iter()
                        .find(|t| t.clips.contains(clip))
                        .map(|t| t.id.clone())
                        .unwrap_or_default(),
                    new_start_time: new_start as f64,
                });

                // Resize the clip (shrink from left = move start time and change length)
                self.command_collector.add_command(DawCommand::ResizeClip {
                    clip_id: clip_id.clone(),
                    new_length: new_length as f64,
                });
            }
        }

        if left_response.drag_stopped() {
            self.playback_schedule_dirty |= self.resize_initial_values.is_some();
            self.resize_initial_values = None;
            self.resize_snap_handler.reset();
        }

        // Handle resizing from right edge (only change length as clip doesn't move)
        let right_response = ui.allocate_rect(right_handle, egui::Sense::drag());

        if right_response.drag_started() {
            self.resize_initial_values = Some((start_time, length));
            self.resize_snap_handler.reset();
        }

        if right_response.dragged() {
            if let Some((initial_start, initial_length)) = self.resize_initial_values {
                // Accumulate drag delta
                self.resize_snap_handler
                    .add_delta(right_response.drag_delta().x);
                let accumulated_time_delta =
                    self.resize_snap_handler.get_accumulated() / self.pixels_per_second;
                let proposed_length = (initial_length + accumulated_time_delta).max(0.1);

                // Apply snapping if enabled (disable with Shift key)
                let snap = self.snap_enabled && !ui.input(|i| i.modifiers.shift);
                let new_length = if snap && self.resize_snap_handler.should_snap() {
                    let end_time = initial_start + proposed_length;
                    let snapped_end =
                        TimeUtils::snap_time(end_time as f64, state.project.bpm, state.snap_mode)
                            as f32;
                    (snapped_end - initial_start).max(0.1)
                } else {
                    proposed_length
                };

                self.command_collector.add_command(DawCommand::ResizeClip {
                    clip_id: clip_id.clone(),
                    new_length: new_length as f64,
                });
            }
        }

        if right_response.drag_stopped() {
            self.playback_schedule_dirty |= self.resize_initial_values.is_some();
            self.resize_initial_values = None;
            self.resize_snap_handler.reset();
        }

        // Change cursor when hovering over resize handles
        if left_response.hovered() || right_response.hovered() {
            ui.output_mut(|o| o.cursor_icon = egui::CursorIcon::ResizeHorizontal);
        }
    }

    fn draw_midi_preview(
        &self,
        ui: &mut egui::Ui,
        clip_rect: egui::Rect,
        midi_store: &MidiEventStore,
        clip_start_time: f64,
        clip_length: f64,
    ) {
        // Create a content area below the clip name with padding
        let vertical_padding = 3.0;
        let preview_rect = egui::Rect::from_min_size(
            clip_rect.left_top() + egui::vec2(0.0, 20.0),
            egui::vec2(clip_rect.width(), clip_rect.height() - 20.0),
        )
        .shrink2(egui::vec2(2.0, vertical_padding));

        // Only draw if we have enough space
        if preview_rect.height() < 10.0 {
            return;
        }

        // Draw a subtle background for the preview area (before padding)
        let preview_bg_rect = egui::Rect::from_min_size(
            clip_rect.left_top() + egui::vec2(0.0, 20.0),
            egui::vec2(clip_rect.width(), clip_rect.height() - 20.0),
        );
        ui.painter().rect_filled(
            preview_bg_rect,
            2.0,
            egui::Color32::from_rgba_unmultiplied(0, 0, 0, 30),
        );

        // Get all notes
        let notes: Vec<_> = midi_store.get_notes().collect();
        if notes.is_empty() {
            // Draw "Empty" text if no notes
            ui.painter().text(
                preview_rect.center(),
                egui::Align2::CENTER_CENTER,
                "Empty",
                egui::FontId::proportional(10.0),
                egui::Color32::from_rgba_unmultiplied(255, 255, 255, 60),
            );
            return;
        }

        // Find the pitch range
        let min_pitch = notes.iter().map(|n| n.key).min().unwrap_or(60);
        let max_pitch = notes.iter().map(|n| n.key).max().unwrap_or(72);
        let pitch_range = (max_pitch - min_pitch).max(12) as f32;

        // Draw notes as small rectangles
        // Use a lighter color that contrasts with the clip background
        let note_color = egui::Color32::from_rgba_unmultiplied(255, 255, 255, 100);
        let pixels_per_second = clip_rect.width() as f64 / clip_length;

        for note in notes {
            // Calculate note position within the clip
            let note_x = preview_rect.left() + (note.start_time * pixels_per_second) as f32;
            let note_width = (note.duration * pixels_per_second) as f32;

            // Skip notes outside the visible clip area
            if note_x + note_width < preview_rect.left() || note_x > preview_rect.right() {
                continue;
            }

            // Calculate vertical position (inverted so higher pitches are at top)
            let pitch_normalized = (note.key - min_pitch) as f32 / pitch_range;
            let available_height = preview_rect.height();
            let note_y = preview_rect.bottom() - (pitch_normalized * available_height);
            let note_height = (available_height / pitch_range).max(1.0).min(3.0);

            let note_rect = egui::Rect::from_min_size(
                egui::pos2(note_x.max(preview_rect.left()), note_y - note_height / 2.0),
                egui::vec2(
                    note_width.min(preview_rect.right() - note_x).max(1.0),
                    note_height,
                ),
            );

            // Only draw if the note rect is within the preview area
            if note_rect.intersects(preview_rect) {
                // Draw note with velocity-based opacity
                let opacity = (note.velocity as f32 / 127.0 * 150.0 + 50.0) as u8;
                let velocity_color = egui::Color32::from_rgba_unmultiplied(255, 255, 255, opacity);

                ui.painter().rect_filled(note_rect, 0.5, velocity_color);
            }
        }
    }

    fn draw_playhead(&mut self, ui: &mut egui::Ui, rect: egui::Rect, state: &DawState) {
        // Store and set the clip rect to prevent overflow
        let original_clip_rect = ui.clip_rect();
        ui.set_clip_rect(rect);

        let playhead_x = state.current_time * self.pixels_per_second as f64;
        let visible_width = rect.width() as f64;
        let visible_width_threshold = visible_width * 0.8;

        let playhead_position = playhead_x - self.scroll_offset as f64;

        if state.playing {
            if playhead_position > visible_width * 0.8 {
                self.scroll_offset = (playhead_x - visible_width_threshold) as f32;
            } else if playhead_position < visible_width_threshold {
                self.scroll_offset = (playhead_x - visible_width_threshold).max(0.0) as f32;
            }
        }

        let playhead_x = rect.left() as f64 + playhead_x - self.scroll_offset as f64;

        // Use a soft red color for the playhead
        let playhead_color = egui::Color32::from_rgb(220, 80, 80);

        ui.painter().line_segment(
            [
                egui::pos2(playhead_x as f32, rect.top()),
                egui::pos2(playhead_x as f32, rect.bottom()),
            ],
            (2.0, playhead_color),
        );

        // Restore original clip rect
        ui.set_clip_rect(original_clip_rect);
    }

    fn draw_punch_points(&mut self, ui: &mut egui::Ui, rect: egui::Rect, state: &DawState) {
        // Store and set the clip rect to prevent overflow
        let original_clip_rect = ui.clip_rect();
        ui.set_clip_rect(rect);

        // Draw punch in point
        if let Some(punch_in) = state.punch_in {
            let punch_in_x = rect.left() + (punch_in * self.pixels_per_second as f64) as f32
                - self.scroll_offset;

            // Draw punch in marker (green)
            ui.painter().line_segment(
                [
                    egui::pos2(punch_in_x, rect.top()),
                    egui::pos2(punch_in_x, rect.bottom()),
                ],
                (2.0, egui::Color32::from_rgb(80, 220, 80)),
            );

            // Draw punch in flag
            let flag_points = vec![
                egui::pos2(punch_in_x, rect.top()),
                egui::pos2(punch_in_x + 10.0, rect.top()),
                egui::pos2(punch_in_x + 10.0, rect.top() + 15.0),
                egui::pos2(punch_in_x, rect.top() + 10.0),
            ];
            ui.painter().add(egui::Shape::convex_polygon(
                flag_points,
                egui::Color32::from_rgb(80, 220, 80),
                egui::Stroke::NONE,
            ));
        }

        // Draw punch out point
        if let Some(punch_out) = state.punch_out {
            let punch_out_x = rect.left() + (punch_out * self.pixels_per_second as f64) as f32
                - self.scroll_offset;

            // Draw punch out marker (red)
            ui.painter().line_segment(
                [
                    egui::pos2(punch_out_x, rect.top()),
                    egui::pos2(punch_out_x, rect.bottom()),
                ],
                (2.0, egui::Color32::from_rgb(220, 80, 80)),
            );

            // Draw punch out flag
            let flag_points = vec![
                egui::pos2(punch_out_x, rect.top()),
                egui::pos2(punch_out_x - 10.0, rect.top()),
                egui::pos2(punch_out_x - 10.0, rect.top() + 15.0),
                egui::pos2(punch_out_x, rect.top() + 10.0),
            ];
            ui.painter().add(egui::Shape::convex_polygon(
                flag_points,
                egui::Color32::from_rgb(220, 80, 80),
                egui::Stroke::NONE,
            ));
        }

        // Draw shaded area between punch points
        if let (Some(punch_in), Some(punch_out)) = (state.punch_in, state.punch_out) {
            let punch_in_x = rect.left() + (punch_in * self.pixels_per_second as f64) as f32
                - self.scroll_offset;
            let punch_out_x = rect.left() + (punch_out * self.pixels_per_second as f64) as f32
                - self.scroll_offset;

            if punch_out > punch_in {
                let punch_rect = egui::Rect::from_min_max(
                    egui::pos2(punch_in_x, rect.top()),
                    egui::pos2(punch_out_x, rect.bottom()),
                );

                // Draw semi-transparent yellow area
                ui.painter().rect_filled(
                    punch_rect,
                    0.0,
                    egui::Color32::from_rgba_unmultiplied(255, 255, 100, 30),
                );
            }
        }

        // Restore original clip rect
        ui.set_clip_rect(original_clip_rect);
    }

    // ===== DEVICE PANEL =====

    fn draw_device_panel_divider(&mut self, ui: &mut egui::Ui, rect: egui::Rect) {
        let response = ui.allocate_rect(rect, egui::Sense::drag());

        // Visual - highlight on hover/drag
        let color = if response.hovered() || response.dragged() {
            ui.visuals().widgets.active.bg_fill
        } else {
            ui.visuals().widgets.noninteractive.bg_fill
        };
        ui.painter().rect_filled(rect, 0.0, color);

        // Resize on drag
        if response.dragged() {
            self.device_panel_height =
                (self.device_panel_height - response.drag_delta().y).clamp(80.0, 200.0);
        }

        // Cursor change
        if response.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
        }
    }

    fn draw_device_panel(&mut self, ui: &mut egui::Ui, rect: egui::Rect, state: &mut DawState) {
        // Background
        ui.painter().rect_filled(rect, 0.0, ui.visuals().panel_fill);

        // Top border
        ui.painter().line_segment(
            [rect.left_top(), rect.right_top()],
            egui::Stroke::new(1.0_f32, ui.visuals().widgets.noninteractive.bg_stroke.color),
        );

        // Create UI for panel content
        let content_rect = rect.shrink2(egui::vec2(12.0, 8.0));
        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(content_rect), |ui| {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.x = 16.0;

                if let Some(track_id) = &state.selected_track.clone() {
                    if let Some(track) = state.project.tracks.iter_mut().find(|t| &t.id == track_id)
                    {
                        // Track name heading
                        ui.horizontal(|ui| {
                            ui.heading(&track.name);
                            ui.separator();
                        });

                        let TrackType::Midi {
                            device_name,
                            channel,
                            input_device_name,
                            input_channel,
                        } = &mut track.track_type;

                        ui.horizontal(|ui| {
                            // MIDI Output Device
                            ui.label("Output:");
                            let display_text = device_name.as_deref().unwrap_or("No Device");
                            egui::ComboBox::from_id_salt("device_panel_output")
                                .selected_text(display_text)
                                .width(180.0)
                                .show_ui(ui, |ui| {
                                    if ui
                                        .selectable_label(device_name.is_none(), "No Device")
                                        .clicked()
                                    {
                                        self.pending_midi_connections
                                            .push((track_id.clone(), String::new()));
                                    }
                                    for port in &self.midi_ports {
                                        let is_selected = device_name.as_ref() == Some(port);
                                        if ui.selectable_label(is_selected, port).clicked() {
                                            self.pending_midi_connections
                                                .push((track_id.clone(), port.clone()));
                                        }
                                    }
                                });

                            ui.add_space(8.0);

                            // MIDI Channel
                            ui.label("Channel:");
                            let mut new_channel = *channel;
                            let mut channel_changed = false;
                            egui::ComboBox::from_id_salt("device_panel_channel")
                                .selected_text(format!("Ch {}", channel))
                                .width(70.0)
                                .show_ui(ui, |ui| {
                                    for ch in 1..=16u8 {
                                        if ui
                                            .selectable_value(
                                                &mut new_channel,
                                                ch,
                                                format!("Ch {}", ch),
                                            )
                                            .clicked()
                                        {
                                            channel_changed = true;
                                        }
                                    }
                                });
                            if channel_changed {
                                self.command_collector.add_command(
                                    DawCommand::SetTrackMidiChannel {
                                        track_id: track_id.clone(),
                                        channel: new_channel,
                                    },
                                );
                            }
                        });

                        ui.horizontal(|ui| {
                            ui.add_space(8.0);

                            // MIDI Input Device
                            ui.label("Input:");
                            let input_display_text =
                                input_device_name.as_deref().unwrap_or("Default");
                            egui::ComboBox::from_id_salt("device_panel_input")
                                .selected_text(input_display_text)
                                .width(180.0)
                                .show_ui(ui, |ui| {
                                    if ui
                                        .selectable_label(input_device_name.is_none(), "Default")
                                        .clicked()
                                    {
                                        self.command_collector.add_command(
                                            DawCommand::SetTrackMidiInputPort {
                                                track_id: track_id.clone(),
                                                input_port: None,
                                            },
                                        );
                                    }
                                    for port in &self.midi_input_ports {
                                        let is_selected = input_device_name.as_ref() == Some(port);
                                        if ui.selectable_label(is_selected, port).clicked() {
                                            self.command_collector.add_command(
                                                DawCommand::SetTrackMidiInputPort {
                                                    track_id: track_id.clone(),
                                                    input_port: Some(port.clone()),
                                                },
                                            );
                                        }
                                    }
                                });

                            ui.add_space(8.0);

                            // MIDI Input Channel
                            ui.label("Input Ch:");
                            let mut new_input_channel = *input_channel;
                            let mut input_channel_changed = false;
                            egui::ComboBox::from_id_salt("device_panel_input_channel")
                                .selected_text(
                                    input_channel
                                        .map(|channel| format!("Ch {channel}"))
                                        .unwrap_or_else(|| "All".to_string()),
                                )
                                .width(70.0)
                                .show_ui(ui, |ui| {
                                    if ui
                                        .selectable_value(&mut new_input_channel, None, "All")
                                        .clicked()
                                    {
                                        input_channel_changed = true;
                                    }
                                    for input_channel in 1..=16u8 {
                                        if ui
                                            .selectable_value(
                                                &mut new_input_channel,
                                                Some(input_channel),
                                                format!("Ch {input_channel}"),
                                            )
                                            .clicked()
                                        {
                                            input_channel_changed = true;
                                        }
                                    }
                                });
                            if input_channel_changed {
                                self.command_collector.add_command(
                                    DawCommand::SetTrackMidiInputChannel {
                                        track_id: track_id.clone(),
                                        channel: new_input_channel,
                                    },
                                );
                            }

                            ui.add_space(8.0);

                            // Input Monitoring
                            let mut input_monitoring = track.input_monitoring;
                            if ui
                                .checkbox(&mut input_monitoring, "Input Monitor")
                                .changed()
                            {
                                self.command_collector.add_command(
                                    DawCommand::ToggleInputMonitoring {
                                        track_id: track_id.clone(),
                                    },
                                );
                            }
                        });
                    }
                } else {
                    ui.centered_and_justified(|ui| {
                        ui.label("Select a track to configure MIDI settings");
                    });
                }
            });
        });
    }
}

fn vertical_scroll_limit(track_count: usize, track_height: f32, viewport_height: f32) -> f32 {
    let content_height = track_count as f32 * track_height + ADD_TRACK_AREA_HEIGHT;
    (content_height - viewport_height).max(0.0)
}

#[cfg(test)]
mod tests {
    use super::vertical_scroll_limit;

    #[test]
    fn one_track_does_not_scroll_when_content_fits() {
        assert_eq!(vertical_scroll_limit(1, 80.0, 500.0), 0.0);
        assert_eq!(vertical_scroll_limit(1, 80.0, 40.0), 90.0);
    }

    #[test]
    fn overflowing_tracks_scroll_only_to_the_content_end() {
        assert_eq!(vertical_scroll_limit(10, 80.0, 500.0), 350.0);
    }
}
