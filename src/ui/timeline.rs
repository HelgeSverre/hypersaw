#![allow(unused_variables)]
#![allow(unused_imports)]

use crate::core::*;
use eframe::egui;
use std::time::Duration;

pub struct Timeline {
    pixels_per_second: f32,
    scroll_offset: f32,
    grid_size: f32,    // In seconds
    visible_time: f32, // Visible time range in seconds
    snap_enabled: bool,
    track_height: f32,
}

impl Default for Timeline {
    fn default() -> Self {
        Self {
            pixels_per_second: 100.0, // Start with 100 pixels per second
            scroll_offset: 0.0,
            grid_size: 1.0,     // 1 second grid
            visible_time: 60.0, // Show 1 minute by default
            snap_enabled: true,
            track_height: 80.0,
        }
    }
}

impl Timeline {
    pub fn show(&mut self, ui: &mut egui::Ui, state: &mut DawState) {
        let (rect, response) = ui.allocate_exact_size(ui.available_size(), egui::Sense::drag());

        // Draw background
        ui.painter()
            .rect_filled(rect, 0.0, ui.visuals().extreme_bg_color);

        // Handle scrolling
        if response.dragged_by(egui::PointerButton::Middle) {
            self.scroll_offset = (self.scroll_offset - response.drag_delta().x)
                .max(0.0)
                .min(state.project.tracks.len() as f32 * self.track_height - rect.height());
        }

        // Draw time ruler
        self.draw_ruler(ui, rect);

        // Draw tracks
        let track_rect = rect.shrink2(egui::vec2(0.0, 20.0)); // Space for ruler
        self.draw_tracks(ui, track_rect, state);

        // Draw playhead

        let playhead_x = rect.left() as f64 + state.current_time * self.pixels_per_second as f64
            - self.scroll_offset as f64;
        ui.painter().line_segment(
            [
                egui::pos2(playhead_x as f32, rect.top()),
                egui::pos2(playhead_x as f32, rect.bottom()),
            ],
            (1.0, ui.visuals().text_color()),
        );

        // Handle zoom with Ctrl + Mouse wheel
        if response.hovered() {
            ui.input(|i| {
                if i.modifiers.ctrl {
                    let zoom_delta = i.raw_scroll_delta.y * 0.001;
                    self.pixels_per_second = (self.pixels_per_second * (1.0 + zoom_delta))
                        .max(10.0) // Minimum zoom
                        .min(500.0); // Maximum zoom
                }
            });
        }
    }

    fn draw_ruler(&self, ui: &mut egui::Ui, rect: egui::Rect) {
        let ruler_height = 20.0;
        let ruler_rect =
            egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), ruler_height));

        // Draw ruler background
        ui.painter()
            .rect_filled(ruler_rect, 0.0, ui.visuals().window_fill);

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

    fn draw_tracks(&self, ui: &mut egui::Ui, rect: egui::Rect, state: &mut DawState) {
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
            ui.painter().rect_filled(
                track_rect,
                0.0,
                if track_idx % 2 == 0 {
                    ui.visuals().faint_bg_color
                } else {
                    ui.visuals().window_fill
                },
            );

            // Draw track separator
            ui.painter().line_segment(
                [track_rect.left_top(), track_rect.right_top()],
                (1.0, ui.visuals().window_stroke.color),
            );

            // Draw clips
            for clip in &track.clips {
                self.draw_clip(ui, track_rect, clip, state);
            }
        }
    }

    fn draw_clip(&self, ui: &mut egui::Ui, track_rect: egui::Rect, clip: &Clip, state: &DawState) {
        let (start_time, length) = match clip {
            Clip::Midi {
                start_time, length, ..
            } => (*start_time as f32, *length as f32),
            Clip::Audio {
                start_time, length, ..
            } => (*start_time as f32, *length as f32),
        };

        let clip_left = track_rect.left() + start_time * self.pixels_per_second;
        let clip_width = length * self.pixels_per_second;

        let clip_rect = egui::Rect::from_min_size(
            egui::pos2(clip_left, track_rect.top() + 2.0),
            egui::vec2(clip_width, track_rect.height() - 4.0),
        );

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

        if is_selected {
            ui.painter().rect_stroke(
                clip_rect,
                4.0,
                egui::Stroke::new(2.0, ui.visuals().selection.stroke.color),
            );
        }

        // Draw clip name
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

        ui.painter().text(
            clip_rect.left_top() + egui::vec2(4.0, 4.0),
            egui::Align2::LEFT_TOP,
            clip_name,
            egui::FontId::proportional(12.0),
            ui.visuals().text_color(),
        );
    }
}
