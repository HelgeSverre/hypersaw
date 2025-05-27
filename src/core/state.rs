use crate::core::{
    EditorView, MidiMessage, MidiScheduler, MidiSchedulerListener, Project, SnapMode,
    StatusManager, TrackType, Transport, TransportListener,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt::{Debug, Formatter};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

struct MidiThread {
    handle: JoinHandle<()>,
    running: Arc<AtomicBool>,
}

pub struct DawState {
    pub project: Project,
    pub snap_mode: SnapMode,
    pub metronome: bool,
    pub recording: bool,

    // UI state
    pub selected_track: Option<String>,
    pub selected_clip: Option<String>,
    pub current_view: EditorView,

    pub status: StatusManager,
    pub transport: Transport,

    midi_output: Arc<midir::MidiOutput>,
    midi_port: Option<midir::MidiOutputPort>, // Store the port to reconnect easily

    midi_thread: Option<MidiThread>,
}

impl Debug for DawState {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DawState")
            .field("project", &self.project)
            .field("snap_mode", &self.snap_mode)
            .field("metronome", &self.metronome)
            .field("recording", &self.recording)
            .field("selected_track", &self.selected_track)
            .field("selected_clip", &self.selected_clip)
            .field("current_view", &self.current_view)
            .field("status", &self.status)
            .finish()
    }
}

impl DawState {
    pub fn new() -> Self {
        Self {
            project: Project::new("Untitled".to_string()),
            snap_mode: SnapMode::Eighth,
            metronome: false,
            recording: false,
            transport: Transport::new(120.0),

            midi_output: Arc::new(midir::MidiOutput::new("Supersaw").unwrap()), // wrapped here
            midi_port: None,
            midi_thread: None,

            selected_track: None,
            selected_clip: None,
            current_view: EditorView::default(),
            status: StatusManager::new(),
        }
    }

    pub fn connect_midi_port(&mut self, port_name: &str) -> Result<(), Box<dyn std::error::Error>> {
        let ports = self.midi_output.ports();

        for port in ports {
            if self.midi_output.port_name(&port)? == port_name {
                self.midi_port = Some(port);
                return Ok(());
            }
        }

        Err("MIDI port not found".into())
    }

    pub fn start_playback(&mut self) {
        self.transport.play();

        // If existing thread, stop it first
        if let Some(thread) = self.midi_thread.take() {
            thread.running.store(false, Ordering::SeqCst);
            let _ = thread.handle.join();
        }

        let project_clone = self.project.clone();
        let running = Arc::new(AtomicBool::new(true));
        let running_clone = running.clone();
        let start_position = self.transport.get_position();

        let midi_output = Arc::clone(&self.midi_output);
        let port = self.midi_port.clone().expect("MIDI port not connected");

        let handle = std::thread::spawn(move || {
            let mut midi_out = (*midi_output)
                .connect(&port, "Playback Thread")
                .expect("Failed to connect MIDI port");

            let mut current_pos = start_position;
            let mut last_time = Instant::now();

            while running_clone.load(Ordering::SeqCst) {
                let now = Instant::now();
                let delta = now.duration_since(last_time).as_secs_f64();
                last_time = now;

                current_pos += delta;
                let window_end = current_pos + 0.05;

                let events = project_clone.get_all_events_in_time_range(current_pos, window_end);

                if !events.is_empty() {
                    let mut sorted_events = events;
                    sorted_events.sort_by(|(_, a), (_, b)| a.time.partial_cmp(&b.time).unwrap());

                    for (_, event) in sorted_events {
                        let wait_duration = (event.time - current_pos).max(0.0);
                        if wait_duration > 0.0 {
                            std::thread::sleep(Duration::from_secs_f64(wait_duration));
                        }

                        DawState::send_midi_message(0xB0 | 0, &event.message, &mut midi_out);
                    }
                }

                current_pos = window_end;
                std::thread::sleep(Duration::from_millis(1));
            }

            // All notes off when stopping
            for channel in 0..16 {
                let _ = midi_out.send(&[0xB0 | channel, 123, 0]);
            }
        });

        self.midi_thread = Some(MidiThread { handle, running });
    }

    pub fn stop_playback(&mut self) {
        self.transport.stop();

        if let Some(thread) = self.midi_thread.take() {
            thread.running.store(false, Ordering::SeqCst);
            let _ = thread.handle.join();
        }
    }
    fn send_midi_message(
        channel: u8,
        message: &MidiMessage,
        midi_out: &mut midir::MidiOutputConnection,
    ) {
        match message {
            MidiMessage::NoteOn { key, velocity, .. } => {
                let _ = midi_out.send(&[0x90 | channel, *key, *velocity]);
            }
            MidiMessage::NoteOff { key, velocity, .. } => {
                let _ = midi_out.send(&[0x80 | channel, *key, *velocity]);
            }
            _ => {}
        }
    }
}
