#![allow(unused_variables)]
#![allow(unused_imports)]

use crate::core::*;
use eframe::egui;
use eframe::epaint::StrokeKind;

pub struct Timeline {
    pixels_per_second: f32,
    scroll_offset: f32,
    grid_size: f32,
    snap_enabled: bool,
    track_height: f32,
    drag_start: Option<(egui::Pos2, f32)>, // (pointer_pos, clip_start_time)
    command_collector: CommandCollector,
}

impl Default for Timeline {
    fn default() -> Self {
        Self {
            pixels_per_second: 100.0,
            scroll_offset: 0.0,
            grid_size: 1.0,     // todo: remove
            snap_enabled: true, // TODO: add toggle in UI
            track_height: 80.0,
            drag_start: None,
            command_collector: CommandCollector::new(),
        }
    }
}

impl Timeline {
    pub fn show(&mut self, ui: &mut egui::Ui, state: &mut DawState) -> Vec<DawCommand> {
        let (rect, response) = ui.allocate_exact_size(ui.available_size(), egui::Sense::drag());

        self.draw_background(ui, rect);
        self.draw_grid(ui, rect, state);
        self.handle_zooming(ui);
        self.handle_scrolling(ui);
        self.handle_file_drops(ui, state);
        self.handle_delete_clip(ui, state);

        self.draw_ruler(ui, rect, state);
        self.draw_tracks(ui, rect.shrink2(egui::vec2(0.0, 20.0)), state);
        self.handle_loop_region(ui, rect, state);
        self.draw_playhead(ui, rect, state);

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
        let subdivisions_per_beat = (beat_duration / division).round() as i32; // How many subdivision lines per beat
        let pixels_per_division = pixels_per_beat / subdivisions_per_beat as f32;

        for bar in start_bar..=end_bar {
            let x = rect.left() + (bar as f32 * pixels_per_bar) - self.scroll_offset;

            // **Alternate background shading every 4 bars**
            if bar % 8 < 4 {
                let bar_rect = egui::Rect::from_min_size(
                    egui::pos2(x, rect.top()),
                    egui::vec2(pixels_per_bar * 4.0, rect.height()),
                );

                let bg_color = ui.visuals().extreme_bg_color.linear_multiply(1.05);
                ui.painter().rect_filled(bar_rect, 0.0, bg_color);
            }

            // **Draw bar lines (stronger)**
            let bar_line_color = ui.visuals().window_stroke.color.linear_multiply(2.0);
            ui.painter().line_segment(
                [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
                (1.5, bar_line_color),
            );

            // **Draw beat and subdivision lines**
            for beat in 0..4 {
                let beat_x = x + (beat as f32 * pixels_per_beat);
                let beat_line_color = ui.visuals().window_stroke.color.linear_multiply(0.8);
                ui.painter().line_segment(
                    [
                        egui::pos2(beat_x, rect.top()),
                        egui::pos2(beat_x, rect.bottom()),
                    ],
                    (1.0, beat_line_color),
                );

                // **Draw correct number of subdivisions per beat**
                for sub in 1..subdivisions_per_beat {
                    let sub_x = beat_x + (sub as f32 * pixels_per_division);
                    if sub_x > rect.right() {
                        break;
                    }
                    let sub_line_color = ui.visuals().window_stroke.color.linear_multiply(0.5);
                    ui.painter().line_segment(
                        [
                            egui::pos2(sub_x, rect.top()),
                            egui::pos2(sub_x, rect.bottom()),
                        ],
                        (0.5, sub_line_color),
                    );
                }
            }
        }
    }

    fn handle_zooming(&mut self, ui: &mut egui::Ui) {
        if ui.input(|i| i.modifiers.ctrl) {
            ui.input(|i| {
                let zoom_delta = i.raw_scroll_delta.y * 0.01;
                if let Some(mouse_pos) = i.pointer.hover_pos() {
                    let time_at_mouse = (mouse_pos.x + self.scroll_offset) / self.pixels_per_second;
                    self.pixels_per_second = (self.pixels_per_second * (1.0 + zoom_delta))
                        .max(10.0)
                        .min(500.0);
                    let new_mouse_x = time_at_mouse * self.pixels_per_second;

                    self.scroll_offset = new_mouse_x - mouse_pos.x;
                }
            });
        }
    }

    fn handle_scrolling(&mut self, ui: &mut egui::Ui) {
        if ui.input(|i| i.modifiers.shift) {
            ui.input(|i| {
                let scroll_delta = i.raw_scroll_delta.x;
                self.scroll_offset = self.scroll_offset + scroll_delta;
            });
        }
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
                        let is_audio = extension == "wav" || extension == "mp3";

                        println!(
                            "name : {}, extension: {}, is_midi: {}, is_audio: {}",
                            path.display(),
                            extension,
                            is_midi,
                            is_audio
                        );

                        if let Some(track) = state.project.tracks.iter().find(|t| &t.id == track_id)
                        {
                            let can_add = match &track.track_type {
                                TrackType::Midi { .. } => is_midi,
                                // TODO: Handle audio tracks
                                _ => false,
                            };
                            if can_add {
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
        if ui.input(|i| i.key_pressed(egui::Key::Delete)) {
            if let Some(clip_id) = &state.selected_clip {
                for track in &state.project.tracks {
                    if let Some(clip) = track.clips.iter().find(|c| match c {
                        Clip::Midi { id, .. } | Clip::Audio { id, .. } => id == clip_id,
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

            if start_response.dragged() {
                let delta = start_response.drag_delta().x / self.pixels_per_second;
                state.loop_start = (state.loop_start + delta as f64)
                    .max(0.0)
                    .min(state.loop_end - 0.1);
            }

            if end_response.dragged() {
                let delta = end_response.drag_delta().x / self.pixels_per_second;
                state.loop_end = (state.loop_end + delta as f64).max(state.loop_start + 0.1);
            }

            if start_response.hovered() || end_response.hovered() {
                ui.output_mut(|o| o.cursor_icon = egui::CursorIcon::ResizeHorizontal);
            }
        }
    }

    fn draw_ruler(&mut self, ui: &mut egui::Ui, rect: egui::Rect, state: &DawState) {
        let ruler_height = 20.0;
        let ruler_rect =
            egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), ruler_height));

        let response = ui.allocate_rect(ruler_rect, egui::Sense::click_and_drag());

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

        // Draw time markers
        let start_time = (self.scroll_offset / self.pixels_per_second).floor() as i32;
        let end_time = ((self.scroll_offset + rect.width()) / self.pixels_per_second).ceil() as i32;

        for time in start_time..=end_time {
            let x = rect.left() + (time as f32 * self.pixels_per_second) - self.scroll_offset;

            // Draw major marker
            ui.painter().line_segment(
                [
                    egui::pos2(x, ruler_rect.top()),
                    egui::pos2(x, ruler_rect.bottom()),
                ],
                (1.0, ui.visuals().text_color()),
            );

            // Draw time label
            let time_str = format!(
                "{}:{}",
                (time / 60).abs(),
                format!("{:02}", (time % 60).abs())
            );

            ui.painter().text(
                egui::pos2(x + 2.0, ruler_rect.top() + 2.0),
                egui::Align2::LEFT_TOP,
                time_str,
                egui::FontId::monospace(10.0),
                ui.visuals().text_color(),
            );
        }
    }

    fn draw_tracks(&mut self, ui: &mut egui::Ui, rect: egui::Rect, state: &mut DawState) {
        let clip_rect = ui.clip_rect();
        let start_time = self.scroll_offset / self.pixels_per_second;
        let end_time = (self.scroll_offset + rect.width()) / self.pixels_per_second;

        for (track_idx, track) in state.project.tracks.iter().enumerate() {
            let track_top = rect.top() + track_idx as f32 * self.track_height;
            let track_rect = egui::Rect::from_min_max(
                egui::pos2(rect.left(), track_top),
                egui::pos2(rect.right(), track_top + self.track_height),
            );

            // Skip if track is not visible
            if track_rect.bottom() < clip_rect.top() || track_rect.top() > clip_rect.bottom() {
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

            // Draw clips
            for clip in &track.clips {
                self.draw_clip(ui, track_rect, clip, state);
            }
        }
    }

    fn draw_clip(
        &mut self,
        ui: &mut egui::Ui,
        track_rect: egui::Rect,
        clip: &Clip,
        state: &DawState,
    ) {
        let (start_time, length) = match clip {
            Clip::Midi {
                start_time, length, ..
            } => (*start_time as f32, *length as f32),
            Clip::Audio {
                start_time, length, ..
            } => (*start_time as f32, *length as f32),
        };

        let clip_left =
            track_rect.left() + start_time * self.pixels_per_second - self.scroll_offset;
        let clip_width = length * self.pixels_per_second;

        let clip_rect = egui::Rect::from_min_size(
            egui::pos2(clip_left, track_rect.top() + 2.0),
            egui::vec2(clip_width, track_rect.height() - 4.0),
        );

        // Add interaction handling
        let response = ui.allocate_rect(clip_rect, egui::Sense::click_and_drag());

        // Handle dragging with proper start position tracking
        if response.drag_started() {
            // Store the initial drag position and clip start time
            self.drag_start = Some((response.hover_pos().unwrap(), start_time));
        }

        if response.dragged() {
            if let Some((drag_start_pos, clip_start_time)) = self.drag_start {
                let current_pos = response.hover_pos().unwrap();
                let delta_x = current_pos.x - drag_start_pos.x;
                let time_delta = delta_x / self.pixels_per_second;

                let new_start_time = (clip_start_time + time_delta).max(0.0);

                // Snap to grid if enabled
                let snapped_time = if self.snap_enabled {
                    TimeUtils::snap_time(new_start_time as f64, state.project.bpm, state.snap_mode)
                        as f32
                } else {
                    new_start_time
                };

                self.command_collector.add_command(DawCommand::MoveClip {
                    clip_id: match clip {
                        Clip::Midi { id, .. } | Clip::Audio { id, .. } => id.clone(),
                    },
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

        if response.drag_stopped() {
            self.drag_start = None;
        }

        if response.double_clicked() {
            if let Clip::Midi { id, .. } = clip {
                if let Some(track_id) = state
                    .project
                    .tracks
                    .iter()
                    .find(|t| {
                        t.clips.iter().any(|c| match c {
                            Clip::Midi { id: clip_id, .. } => clip_id == id,
                            _ => false,
                        })
                    })
                    .map(|t| t.id.clone())
                {
                    self.command_collector
                        .add_command(DawCommand::OpenPianoRoll {
                            clip_id: id.clone(),
                            track_id: track_id.to_string(),
                        });
                }
            }
        }

        // Handle single clicks for selection
        if response.clicked() {
            match clip {
                Clip::Midi { id, .. } | Clip::Audio { id, .. } => {
                    self.command_collector.add_command(DawCommand::SelectClip {
                        clip_id: id.clone(),
                    });
                }
            };
        }

        // Draw clip background
        let clip_color = match clip {
            Clip::Midi { .. } => egui::Color32::from_rgb(64, 128, 255),
            Clip::Audio { .. } => egui::Color32::from_rgb(128, 255, 64),
        };

        ui.painter().rect_filled(clip_rect, 4.0, clip_color);

        // Draw clip border
        let is_selected = match clip {
            Clip::Midi { id, .. } | Clip::Audio { id, .. } => {
                state.selected_clip == Some(id.clone())
            }
        };

        // Make selection visible
        if is_selected || response.hovered() {
            ui.painter().rect_stroke(
                clip_rect,
                4.0,
                egui::Stroke::new(2.0, ui.visuals().selection.stroke.color),
                StrokeKind::Outside,
            );
        }

        let clip_name = match clip {
            Clip::Midi { file_path, .. } => file_path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("Unnamed MIDI"),
            Clip::Audio { file_path, .. } => file_path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("Unnamed Audio"),
        };

        // Draw clip name
        ui.painter().text(
            clip_rect.left_top() + egui::vec2(4.0, 4.0),
            egui::Align2::LEFT_TOP,
            clip_name,
            egui::FontId::proportional(12.0),
            ui.visuals().extreme_bg_color,
        );

        // Handle clip dragging
        let (start_time, length) = match clip {
            Clip::Midi {
                start_time, length, ..
            } => (*start_time as f32, *length as f32),
            Clip::Audio {
                start_time, length, ..
            } => (*start_time as f32, *length as f32),
        };

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
        if left_response.dragged() {
            let delta = left_response.drag_delta().x / self.pixels_per_second;
            let new_start = (start_time + delta).max(0.0);
            let new_length = (length + (start_time - new_start)).max(0.1);

            // TODO: snapping support when holding key-combo

            // Move the clip
            self.command_collector.add_command(DawCommand::MoveClip {
                clip_id: match clip {
                    Clip::Midi { id, .. } | Clip::Audio { id, .. } => id.clone(),
                },
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
                clip_id: match clip {
                    Clip::Midi { id, .. } | Clip::Audio { id, .. } => id.clone(),
                },
                new_length: new_length as f64,
            });
        }

        // Handle resizing from right edge (only change length as clip doesn't move)
        let right_response = ui.allocate_rect(right_handle, egui::Sense::drag());
        if right_response.dragged() {
            let delta = right_response.drag_delta().x / self.pixels_per_second;
            let new_length = (length + delta).max(0.1);

            // TODO: snapping support when holding key-combo

            self.command_collector.add_command(DawCommand::ResizeClip {
                clip_id: match clip {
                    Clip::Midi { id, .. } | Clip::Audio { id, .. } => id.clone(),
                },
                new_length: new_length as f64,
            });
        }

        // Change cursor when hovering over resize handles
        if left_response.hovered() || right_response.hovered() {
            ui.output_mut(|o| o.cursor_icon = egui::CursorIcon::ResizeHorizontal);
        }
    }

    fn draw_playhead(&mut self, ui: &mut egui::Ui, rect: egui::Rect, state: &DawState) {
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
        ui.painter().line_segment(
            [
                egui::pos2(playhead_x as f32, rect.top()),
                egui::pos2(playhead_x as f32, rect.bottom()),
            ],
            (1.0, ui.visuals().text_color()),
        );
    }
}
