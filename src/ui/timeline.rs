#![allow(unused_variables)]
#![allow(unused_imports)]

use crate::core::*;
use crate::core::utils::SnapHandler;
use eframe::egui;
use eframe::epaint::StrokeKind;

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
    pending_midi_connections: Vec<(String, String)>, // (track_id, device_name)
    // Resize state
    resize_snap_handler: SnapHandler,
    resize_initial_values: Option<(f32, f32)>, // (start_time, length)
}

impl Default for Timeline {
    fn default() -> Self {
        Self {
            pixels_per_second: 100.0,
            scroll_offset: 0.0,
            scroll_y: 0.0,
            snap_enabled: true, // TODO: add toggle in UI
            track_height: 80.0,
            track_header_width: 200.0,
            drag_start: None,
            command_collector: CommandCollector::new(),
            midi_ports: Vec::new(),
            pending_midi_connections: Vec::new(),
            resize_snap_handler: SnapHandler::new(10.0),
            resize_initial_values: None,
        }
    }
}

impl Timeline {
    pub fn update_midi_ports(&mut self, ports: Vec<String>) {
        self.midi_ports = ports;
    }
    
    pub fn take_pending_midi_connections(&mut self) -> Vec<(String, String)> {
        std::mem::take(&mut self.pending_midi_connections)
    }
    pub fn show(&mut self, ui: &mut egui::Ui, state: &mut DawState) -> Vec<DawCommand> {
        let (full_rect, response) = ui.allocate_exact_size(ui.available_size(), egui::Sense::drag());
        
        let ruler_height = 20.0;
        
        // Split into regions
        let header_width = self.track_header_width;
        
        // Header area (left side, below ruler)
        let header_rect = egui::Rect::from_min_size(
            egui::pos2(full_rect.left(), full_rect.top() + ruler_height),
            egui::vec2(header_width, full_rect.height() - ruler_height),
        );
        
        // Timeline area (right side, including ruler)
        let timeline_rect = egui::Rect::from_min_size(
            egui::pos2(full_rect.left() + header_width, full_rect.top()),
            egui::vec2(full_rect.width() - header_width, full_rect.height()),
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

        // Draw timeline background and grid
        self.draw_background(ui, tracks_rect);
        self.draw_grid(ui, tracks_rect, state);
        
        // Handle interactions
        self.handle_zooming(ui, timeline_rect);
        self.handle_scrolling(ui, &response);
        self.handle_file_drops(ui, state);
        self.handle_delete_clip(ui, state);
        self.handle_escape_key(ui);

        // Draw components
        self.draw_track_headers(ui, header_rect, state);
        self.draw_tracks(ui, tracks_rect, state);
        self.draw_ruler(ui, ruler_rect, state);
        self.handle_loop_region(ui, tracks_rect, state);
        self.draw_playhead(ui, tracks_rect, state);

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

            // Alternate background shading every 4 bars
            if bar % 8 < 4 {
                let bar_rect = egui::Rect::from_min_size(
                    egui::pos2(x, rect.top()),
                    egui::vec2(pixels_per_bar * 4.0, rect.height()),
                );

                let bg_color = ui.visuals().extreme_bg_color.linear_multiply(1.05);
                ui.painter().rect_filled(bar_rect, 0.0, bg_color);
            }

            // Draw bar lines (stronger)
            let bar_line_color = ui.visuals().window_stroke.color.linear_multiply(2.0);
            ui.painter().line_segment(
                [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
                (1.5, bar_line_color),
            );

            // Draw beat and subdivision lines
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

                // Draw correct number of subdivisions per beat
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

    fn handle_zooming(&mut self, ui: &mut egui::Ui, rect: egui::Rect) {
        if ui.input(|i| i.modifiers.ctrl) {
            ui.input(|i| {
                if let Some(mouse_pos) = i.pointer.hover_pos() {
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

    fn handle_scrolling(&mut self, ui: &egui::Ui, response: &egui::Response) {
        if response.dragged() {
            let invert = -1.0; // Make dragging intuitive
            let delta = response.drag_delta();
            self.scroll_offset = (self.scroll_offset + delta.x * invert).max(0.0);
        }

        // Support mouse wheel scrolling
        ui.input(|i| {
            if i.modifiers.shift {
                // Horizontal scroll with shift
                let scroll_delta = i.raw_scroll_delta.x;
                self.scroll_offset = (self.scroll_offset + scroll_delta).max(0.0);
            } else if !i.modifiers.ctrl {
                // Vertical scroll (when not zooming)
                let scroll_delta = i.raw_scroll_delta.y;
                self.scroll_y = (self.scroll_y - scroll_delta).max(0.0);
            }
        });
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
    
    fn handle_escape_key(&mut self, ui: &mut egui::Ui) {
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
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

        // Draw time markers
        let start_time = (self.scroll_offset / self.pixels_per_second).floor() as i32;
        let end_time = ((self.scroll_offset + rect.width()) / self.pixels_per_second).ceil() as i32;

        for time in start_time..=end_time {
            let x = rect.left() + (time as f32 * self.pixels_per_second) - self.scroll_offset;

            // Draw major marker
            ui.painter().line_segment(
                [
                    egui::pos2(x, rect.top()),
                    egui::pos2(x, rect.bottom()),
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
                egui::pos2(x + 5.0, rect.top() + 5.0),
                egui::Align2::LEFT_TOP,
                time_str,
                egui::FontId::monospace(10.0),
                ui.visuals().text_color(),
            );
        }
        
        // Restore original clip rect
        ui.set_clip_rect(original_clip_rect);
    }

    fn draw_track_headers(&mut self, ui: &mut egui::Ui, rect: egui::Rect, state: &DawState) {
        // Draw header background
        ui.painter().rect_filled(rect, 0.0, ui.visuals().window_fill);
        
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
        if add_track_y > rect.top() && add_track_y < rect.bottom() {
            let button_rect = egui::Rect::from_min_size(
                egui::pos2(rect.left() + 10.0, add_track_y + 10.0),
                egui::vec2(rect.width() - 20.0, 30.0),
            );
            
            let response = ui.allocate_rect(button_rect, egui::Sense::click());
            if response.clicked() {
                self.command_collector.add_command(DawCommand::AddTrack {
                    track_type: TrackType::Midi { channel: 1, device_name: None },
                    name: format!("Track {}", state.project.tracks.len() + 1),
                });
            }
            
            // Draw button
            let style = if response.hovered() {
                ui.visuals().widgets.hovered
            } else {
                ui.visuals().widgets.inactive
            };
            
            ui.painter().rect_filled(button_rect, 4.0, style.weak_bg_fill);
            ui.painter().text(
                button_rect.center(),
                egui::Align2::CENTER_CENTER,
                "➕ Add Track",
                egui::FontId::proportional(12.0),
                style.text_color(),
            );
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
        let bg_color = if is_selected {
            ui.visuals().selection.bg_fill
        } else if index % 2 == 0 {
            ui.visuals().faint_bg_color
        } else {
            ui.visuals().extreme_bg_color
        };
        
        ui.painter().rect_filled(rect, 0.0, bg_color);
        
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
        
        // Content area with padding
        let content_rect = rect.shrink(4.0);
        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(content_rect), |ui| {
            ui.vertical(|ui| {
                // First row: Track info and controls
                ui.horizontal(|ui| {
                    // Track number with icon
                    let track_icon = match &track.track_type {
                        TrackType::Midi { .. } => "🎹",
                        TrackType::Audio => "🎵",
                    };
                    
                    ui.label(
                        egui::RichText::new(format!("{:02} {}", index + 1, track_icon))
                            .monospace()
                            .size(11.0)
                            .color(ui.visuals().text_color().linear_multiply(0.6))
                    );
                    
                    // Mute and Solo buttons
                    let mute_button = egui::Button::new("M").small();
                    let mute_button = if track.is_muted {
                        mute_button.fill(egui::Color32::from_rgb(180, 60, 60))
                    } else {
                        mute_button
                    };
                    
                    if ui.add(mute_button).clicked() {
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
                    
                    let solo_button = egui::Button::new("S").small();
                    let solo_button = if track.is_soloed {
                        solo_button.fill(egui::Color32::from_rgb(180, 180, 60))
                    } else {
                        solo_button
                    };
                    
                    if ui.add(solo_button).clicked() {
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
                
                // Second row: Track name
                ui.horizontal(|ui| {
                    let name_response = ui.add(
                        egui::Label::new(
                            egui::RichText::new(&track.name)
                                .size(12.0)
                        )
                        .sense(egui::Sense::click())
                    );
                    
                    if name_response.clicked() {
                        self.command_collector.add_command(DawCommand::SelectTrack {
                            track_id: track.id.clone(),
                        });
                    }
                });
                
                // Third row: Track-specific controls
                if self.track_height > 60.0 {
                    match &track.track_type {
                        TrackType::Midi { channel, device_name } => {
                            ui.horizontal(|ui| {
                                // MIDI channel dropdown
                                let mut channel_changed = false;
                                let mut new_channel = *channel;
                                
                                egui::ComboBox::new(
                                    format!("midi_channel_{}", track.id),
                                    "",
                                )
                                .width(40.0)
                                .selected_text(format!("Ch{}", channel))
                                .show_ui(ui, |ui| {
                                    for ch in 1..=16 {
                                        if ui.selectable_value(&mut new_channel, ch, format!("Ch{}", ch)).clicked() {
                                            channel_changed = true;
                                        }
                                    }
                                });
                                
                                if channel_changed {
                                    self.command_collector.add_command(DawCommand::SetTrackMidiChannel {
                                        track_id: track.id.clone(),
                                        channel: new_channel,
                                    });
                                }
                                
                                // MIDI port dropdown
                                let display_text = match device_name {
                                    Some(dev) if !dev.is_empty() => dev.as_str(),
                                    _ => "None",
                                };
                                
                                egui::ComboBox::new(
                                    format!("midi_port_{}", track.id),
                                    "",
                                )
                                .width(100.0)
                                .selected_text(display_text)
                                .show_ui(ui, |ui| {
                                    if ui.selectable_label(device_name.is_none(), "None").clicked() {
                                        self.pending_midi_connections.push((track.id.clone(), String::new()));
                                    }
                                    
                                    for port in &self.midi_ports {
                                        let is_selected = device_name.as_ref() == Some(port);
                                        if ui.selectable_label(is_selected, port).clicked() {
                                            self.pending_midi_connections.push((track.id.clone(), port.clone()));
                                        }
                                    }
                                });
                            });
                        }
                        TrackType::Audio => {
                            ui.label(
                                egui::RichText::new("Audio Track")
                                    .size(10.0)
                                    .color(ui.visuals().text_color().linear_multiply(0.7))
                            );
                        }
                    }
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
            
            // Handle click on empty track area for deselection
            let response = ui.interact(track_rect, ui.id().with(format!("track_{}", track_idx)), egui::Sense::click());
            if response.clicked() {
                // Check if click was on empty area (not on a clip)
                let click_pos = response.hover_pos().unwrap_or_default();
                let click_time = (click_pos.x - track_rect.left() + self.scroll_offset) / self.pixels_per_second;
                
                let clicked_on_clip = track.clips.iter().any(|clip| {
                    let (start, length) = match clip {
                        Clip::Midi { start_time, length, .. } => (*start_time as f32, *length as f32),
                        Clip::Audio { start_time, length, .. } => (*start_time as f32, *length as f32),
                    };
                    click_time >= start && click_time <= start + length
                });
                
                if !clicked_on_clip {
                    self.command_collector.add_command(DawCommand::DeselectAll);
                }
            }

            // Draw clips
            for clip in &track.clips {
                self.draw_clip(ui, track_rect, clip, state);
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
    ) {
        let (start_time, length) = match clip {
            Clip::Midi {
                start_time, length, ..
            } => (*start_time as f32, *length as f32),
            Clip::Audio {
                start_time, length, ..
            } => (*start_time as f32, *length as f32),
        };

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
            self.drag_start = Some((response.hover_pos().unwrap(), start_time));
        }

        if response.dragged() {
            if let Some((drag_start_pos, clip_start_time)) = self.drag_start {
                let current_pos = response.hover_pos().unwrap();
                let delta_x = current_pos.x - drag_start_pos.x;
                let time_delta = delta_x / self.pixels_per_second;

                let new_start_time = (clip_start_time + time_delta).max(0.0);

                // Snap to grid if enabled (disable with Shift key)
                let snap = self.snap_enabled && !ui.input(|i| i.modifiers.shift);
                let snapped_time = if snap {
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

        ui.painter().rect_filled(clip_rect, 2.0, clip_color);

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
                2.0,
                egui::Stroke::new(1.5, ui.visuals().selection.stroke.color),
                StrokeKind::Inside,
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
        
        // Draw MIDI preview for MIDI clips
        if let Clip::Midi { midi_data, start_time: clip_start, length: clip_length, .. } = clip {
            if let Some(midi_store) = midi_data {
                self.draw_midi_preview(ui, clip_rect, midi_store, *clip_start, *clip_length);
            }
        }

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
        
        if left_response.drag_started() {
            self.resize_initial_values = Some((start_time, length));
            self.resize_snap_handler.reset();
        }
        
        if left_response.dragged() {
            if let Some((initial_start, initial_length)) = self.resize_initial_values {
                // Accumulate drag delta
                self.resize_snap_handler.add_delta(left_response.drag_delta().x);
                let accumulated_time_delta = self.resize_snap_handler.get_accumulated() / self.pixels_per_second;
                
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
        }
        
        if left_response.drag_stopped() {
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
                self.resize_snap_handler.add_delta(right_response.drag_delta().x);
                let accumulated_time_delta = self.resize_snap_handler.get_accumulated() / self.pixels_per_second;
                let proposed_length = (initial_length + accumulated_time_delta).max(0.1);
                
                // Apply snapping if enabled (disable with Shift key)
                let snap = self.snap_enabled && !ui.input(|i| i.modifiers.shift);
                let new_length = if snap && self.resize_snap_handler.should_snap() {
                    let end_time = initial_start + proposed_length;
                    let snapped_end = TimeUtils::snap_time(
                        end_time as f64,
                        state.project.bpm,
                        state.snap_mode,
                    ) as f32;
                    (snapped_end - initial_start).max(0.1)
                } else {
                    proposed_length
                };

                self.command_collector.add_command(DawCommand::ResizeClip {
                    clip_id: match clip {
                        Clip::Midi { id, .. } | Clip::Audio { id, .. } => id.clone(),
                    },
                    new_length: new_length as f64,
                });
            }
        }
        
        if right_response.drag_stopped() {
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
        ).shrink2(egui::vec2(2.0, vertical_padding));
        
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
                    note_height
                ),
            );
            
            // Only draw if the note rect is within the preview area
            if note_rect.intersects(preview_rect) {
                // Draw note with velocity-based opacity
                let opacity = (note.velocity as f32 / 127.0 * 150.0 + 50.0) as u8;
                let velocity_color = egui::Color32::from_rgba_unmultiplied(255, 255, 255, opacity);
                
                ui.painter().rect_filled(
                    note_rect,
                    0.5,
                    velocity_color,
                );
            }
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
