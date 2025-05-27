// // src/core/midi_scheduler.rs
// use crate::core::{MidiMessage, Project, TransportEvent, TransportListener};
// use std::collections::{BinaryHeap, HashMap};
// use std::error::Error;
// use std::fmt;
// use std::sync::atomic::{AtomicBool, Ordering};
// use std::sync::{Arc, Mutex};
// use std::thread::{self, JoinHandle};
// use std::time::{Duration, Instant};
//
// // Timed MIDI event for the scheduler
// #[derive(Debug, Clone)]
// struct ScheduledMidiEvent {
//     timestamp: Instant, // When the event should be triggered
//     track_id: String,
//     channel: u8,
//     message: MidiMessage,
// }
//
// // We need custom ordering for the priority queue
// impl Ord for ScheduledMidiEvent {
//     fn cmp(&self, other: &Self) -> std::cmp::Ordering {
//         // Reverse order (earlier timestamps are "greater" in priority)
//         other.timestamp.cmp(&self.timestamp)
//     }
// }
//
// impl PartialOrd for ScheduledMidiEvent {
//     fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
//         Some(self.cmp(other))
//     }
// }
//
// impl PartialEq for ScheduledMidiEvent {
//     fn eq(&self, other: &Self) -> bool {
//         self.timestamp == other.timestamp
//     }
// }
//
// impl Eq for ScheduledMidiEvent {}
//
// struct SchedulerThread {
//     handle: JoinHandle<()>,
//     running: Arc<AtomicBool>,
// }
//
// pub struct MidiSchedulerListener(pub Arc<MidiScheduler>);
//
// impl TransportListener for MidiSchedulerListener {
//     fn on_transport_event(&self, event: TransportEvent) {
//         self.0.on_transport_event(event);
//     }
// }
//
// pub struct MidiScheduler {
//     project: Arc<Mutex<Project>>,
//     event_queue: Arc<Mutex<BinaryHeap<ScheduledMidiEvent>>>,
//     scheduler_thread: Arc<Mutex<Option<SchedulerThread>>>,
//     midi_output: Arc<Mutex<Option<midir::MidiOutputConnection>>>,
//     lookahead_ms: u64,
//     current_position: Arc<Mutex<f64>>,
// }
//
// impl fmt::Debug for MidiScheduler {
//     fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
//         f.debug_struct("MidiScheduler")
//             .field("lookahead_ms", &self.lookahead_ms)
//             .field("current_position", &self.current_position)
//             .field("running", &self.is_running())
//             .finish()
//     }
// }
//
// impl MidiScheduler {
//     pub fn new(project: Project) -> Self {
//         Self {
//             project: Arc::new(Mutex::new(project)),
//             event_queue: Arc::new(Mutex::new(BinaryHeap::new())),
//             scheduler_thread: Arc::new(Mutex::new(None)),
//             midi_output: Arc::new(Mutex::new(None)),
//             lookahead_ms: 100, // Default 100ms lookahead
//             current_position: Arc::new(Mutex::new(0.0)),
//         }
//     }
//
//     pub fn connect_output(&self, port_name: &str) -> Result<(), Box<dyn Error>> {
//         let midi_out = midir::MidiOutput::new("Supersaw")?;
//         let ports = midi_out.ports();
//
//         for port in ports {
//             if midi_out.port_name(&port)? == port_name {
//                 let mut output = self.midi_output.lock().unwrap();
//                 *output = Some(midi_out.connect(&port, "Supersaw")?);
//                 return Ok(());
//             }
//         }
//
//         Err("MIDI port not found".into())
//     }
//
//     pub fn disconnect_output(&self) {
//         let mut output = self.midi_output.lock().unwrap();
//         *output = None;
//     }
//
//     pub fn is_running(&self) -> bool {
//         self.scheduler_thread.lock().unwrap().is_some()
//     }
//
//     pub fn schedule_events(
//         &self,
//         from_time: f64,
//         duration: Duration,
//     ) -> Result<(), Box<dyn Error>> {
//         let to_time = from_time + duration.as_secs_f64();
//
//         // Get events from project
//         let events = {
//             let project = self.project.lock().unwrap();
//             project.get_all_events_in_time_range(from_time, to_time)
//         };
//
//         if !events.is_empty() {
//             let base_time = Instant::now();
//             let mut queue = self.event_queue.lock().unwrap();
//
//             // Get track channel mapping
//             let channel_map = self.get_track_channel_map();
//
//             for (track_id, event) in events {
//                 // Calculate when to trigger this event
//                 let offset = event.time - from_time;
//                 let event_time = base_time + Duration::from_secs_f64(offset);
//
//                 // Get channel from track information
//                 let channel = *channel_map.get(&track_id).unwrap_or(&1); // Default to channel 1
//
//                 queue.push(ScheduledMidiEvent {
//                     timestamp: event_time,
//                     track_id,
//                     channel,
//                     message: event.message,
//                 });
//             }
//         } else {
//             print!(" No events to schedule");
//         }
//
//         Ok(())
//     }
//
//     // TODO: extract into separate struct with more routing/filtering capabilities
//     fn get_track_channel_map(&self) -> HashMap<String, u8> {
//         let mut channel_map = HashMap::new();
//         let project = self.project.lock().unwrap();
//
//         for track in &project.tracks {
//             if let crate::core::TrackType::Midi { channel, .. } = track.track_type {
//                 channel_map.insert(track.id.clone(), channel);
//             }
//         }
//
//         channel_map
//     }
//
//     pub fn start_scheduler_thread(&self, initial_position: f64) {
//         // Don't start if already running
//         eprintln!(
//             " Starting scheduler thread at position: {}",
//             initial_position
//         );
//
//         if self.is_running() {
//             eprintln!("Scheduler thread is already running.");
//             return;
//         }
//
//         // Clear any old events
//         {
//             let mut queue = self.event_queue.lock().unwrap();
//             queue.clear();
//         }
//
//         // Set initial position
//         {
//             let mut pos = self.current_position.lock().unwrap();
//             *pos = initial_position;
//         }
//
//         // Schedule initial batch of events
//         let _ = self.schedule_events(initial_position, Duration::from_millis(self.lookahead_ms));
//
//         // Create thread-safe clones for the thread
//         let queue = Arc::clone(&self.event_queue);
//         let midi_output = Arc::clone(&self.midi_output);
//         let project = Arc::clone(&self.project);
//         let current_position = Arc::clone(&self.current_position);
//         let lookahead_ms = self.lookahead_ms;
//
//         let running = Arc::new(AtomicBool::new(true));
//         let running_clone = running.clone();
//
//         // Start scheduler thread
//         let handle = thread::spawn(move || {
//             // Set high priority if possible
//             // #[cfg(target_os = "windows")]
//             // unsafe {
//             //     use winapi::um::processthreadsapi::*;
//             //     use winapi::um::winbase::*;
//             //     SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_TIME_CRITICAL);
//             // }
//
//             let mut last_schedule_time = Instant::now();
//             let schedule_interval = Duration::from_millis(lookahead_ms / 2);
//
//             while running_clone.load(Ordering::SeqCst) {
//                 // Process events due in the next few milliseconds
//                 let now = Instant::now();
//                 let process_until = now + Duration::from_millis(10);
//
//                 // Get events to process
//                 let mut events_to_process = Vec::new();
//                 {
//                     let mut queue = queue.lock().unwrap();
//                     while let Some(event) = queue.peek() {
//                         if event.timestamp <= process_until {
//                             events_to_process.push(queue.pop().unwrap());
//                         } else {
//                             break;
//                         }
//                     }
//                 }
//
//                 // Process events with precise timing
//                 for event in &events_to_process {
//                     let sleep_duration = event.timestamp.saturating_duration_since(Instant::now());
//                     if !sleep_duration.is_zero() {
//                         spin_sleep::sleep(sleep_duration);
//                     }
//
//                     // Send MIDI message if we have a connection
//                     let mut output_guard = midi_output.lock().unwrap();
//                     if let Some(midi_out) = output_guard.as_mut() {
//                         Self::send_midi_message(midi_out, event.channel, &event.message)
//                             .unwrap_or_else(|e| eprintln!("Error sending MIDI: {}", e));
//                     }
//                 }
//
//                 // Schedule more events periodically
//                 if now.duration_since(last_schedule_time) >= schedule_interval {
//                     let current_time = {
//                         let pos = current_position.lock().unwrap();
//                         *pos
//                     };
//
//                     // Schedule next batch of events
//                     let _ = Self::schedule_events_static(
//                         &queue,
//                         &project,
//                         current_time,
//                         Duration::from_millis(lookahead_ms),
//                     );
//
//                     last_schedule_time = now;
//                 }
//
//                 // Sleep a bit if nothing to do
//                 if events_to_process.is_empty() {
//                     thread::sleep(Duration::from_millis(1));
//                 }
//             }
//         });
//
//         // Store the thread and running flag
//         let mut thread_guard = self.scheduler_thread.lock().unwrap();
//         *thread_guard = Some(SchedulerThread { handle, running });
//     }
//
//     // Static version of schedule_events for use in the thread
//     fn schedule_events_static(
//         queue: &Arc<Mutex<BinaryHeap<ScheduledMidiEvent>>>,
//         project: &Arc<Mutex<Project>>,
//         from_time: f64,
//         duration: Duration,
//     ) -> Result<(), Box<dyn Error>> {
//         let to_time = from_time + duration.as_secs_f64();
//
//         // Get events from project
//         let events = {
//             let project = project.lock().unwrap();
//             project.get_all_events_in_time_range(from_time, to_time)
//         };
//
//         if !events.is_empty() {
//             let base_time = Instant::now();
//             let mut queue = queue.lock().unwrap();
//
//             // Get track channel mapping
//             let channel_map = Self::get_track_channel_map_static(project);
//
//             for (track_id, event) in events {
//                 // Calculate when to trigger this event
//                 let offset = event.time - from_time;
//                 let event_time = base_time + Duration::from_secs_f64(offset);
//
//                 // Get channel from track information
//                 let channel = *channel_map.get(&track_id).unwrap_or(&1); // Default to channel 1
//
//                 queue.push(ScheduledMidiEvent {
//                     timestamp: event_time,
//                     track_id,
//                     channel,
//                     message: event.message,
//                 });
//             }
//         }
//
//         Ok(())
//     }
//
//     fn get_track_channel_map_static(project: &Arc<Mutex<Project>>) -> HashMap<String, u8> {
//         let mut channel_map = HashMap::new();
//         let project = project.lock().unwrap();
//
//         for track in &project.tracks {
//             if let crate::core::TrackType::Midi { channel, .. } = track.track_type {
//                 channel_map.insert(track.id.clone(), channel);
//             }
//         }
//
//         channel_map
//     }
//
//     fn send_midi_message(
//         midi_out: &mut midir::MidiOutputConnection,
//         channel: u8,
//         message: &MidiMessage,
//     ) -> Result<(), Box<dyn Error>> {
//         match message {
//             MidiMessage::NoteOn { key, velocity, .. } => {
//                 let midi_message = [0x90 | (channel - 1), *key, *velocity];
//                 midi_out.send(&midi_message)?;
//             }
//             MidiMessage::NoteOff { key, velocity, .. } => {
//                 let midi_message = [0x80 | (channel - 1), *key, *velocity];
//                 midi_out.send(&midi_message)?;
//             }
//             MidiMessage::ControlChange {
//                 controller, value, ..
//             } => {
//                 let midi_message = [0xB0 | (channel - 1), *controller, *value];
//                 midi_out.send(&midi_message)?;
//             }
//             MidiMessage::ProgramChange { program, .. } => {
//                 let midi_message = [0xC0 | (channel - 1), *program];
//                 midi_out.send(&midi_message)?;
//             }
//             MidiMessage::PitchBend { value, .. } => {
//                 let lsb = (*value & 0x7F) as u8;
//                 let msb = ((*value >> 7) & 0x7F) as u8;
//                 let midi_message = [0xE0 | (channel - 1), lsb, msb];
//                 midi_out.send(&midi_message)?;
//             }
//             // Handle other message types as needed
//             _ => {}
//         }
//         Ok(())
//     }
//
//     pub fn stop(&self) {
//         let mut thread_guard = self.scheduler_thread.lock().unwrap();
//
//         if let Some(thread) = thread_guard.take() {
//             // Signal thread to stop
//             thread.running.store(false, Ordering::SeqCst);
//
//             // Wait for thread to finish (optional, can cause blocking)
//             if let Ok(_) = thread.handle.join() {
//                 // Thread stopped successfully
//             }
//
//             // Send all notes off to prevent hanging notes
//             let mut output = self.midi_output.lock().unwrap();
//             if let Some(midi_out) = output.as_mut() {
//                 for channel in 0..16 {
//                     let _ = midi_out.send(&[0xB0 | channel, 123, 0]); // All Notes Off
//                 }
//             }
//         }
//     }
//
//     pub fn update_project(&self, project: Project) {
//         let mut proj = self.project.lock().unwrap();
//         *proj = project;
//     }
//
//     pub fn update_position(&self, position: f64) {
//         let mut pos = self.current_position.lock().unwrap();
//         *pos = position;
//     }
// }
//
// impl TransportListener for MidiScheduler {
//     fn on_transport_event(&self, event: TransportEvent) {
//         match event {
//             // Start scheduler thread with current position
//             TransportEvent::Started { position } => {
//                 print!("Starting scheduler thread at position: {}", position);
//                 self.update_position(position);
//                 self.start_scheduler_thread(position);
//             }
//             // Stop the scheduler
//             TransportEvent::Stopped | TransportEvent::Paused => {
//                 self.stop();
//             }
//             // Update position
//             TransportEvent::PositionChanged { position } => {
//                 self.update_position(position);
//             }
//             _ => {} // Ignore other events for now
//         }
//     }
// }

// src/core/midi_scheduler.rs - simplified version
use crate::core::{MidiMessage, Project, TransportEvent, TransportListener};
use std::collections::HashMap;
use std::error::Error;
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

pub struct MidiSchedulerListener(pub Arc<MidiScheduler>);

impl TransportListener for MidiSchedulerListener {
    fn on_transport_event(&self, event: TransportEvent) {
        match event {
            TransportEvent::Started { position } => {
                println!(
                    "MidiScheduler received Started event at position: {}",
                    position
                );
                self.0.start_playback(position);
            }
            TransportEvent::Stopped | TransportEvent::Paused => {
                println!("MidiScheduler received Stop/Pause event");
                self.0.stop_playback();
            }
            _ => {} // Ignore other events
        }
    }
}

pub struct MidiScheduler {
    project: Arc<Mutex<Project>>,
    midi_output: Arc<Mutex<Option<midir::MidiOutputConnection>>>,
    playing: Arc<AtomicBool>,
    current_position: Arc<Mutex<f64>>,
}

impl fmt::Debug for MidiScheduler {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        f.debug_struct("MidiScheduler")
            .field("playing", &self.playing.load(Ordering::SeqCst))
            .field("current_position", &self.current_position)
            .finish()
    }
}

impl MidiScheduler {
    pub fn new(project: Project) -> Self {
        Self {
            project: Arc::new(Mutex::new(project)),
            midi_output: Arc::new(Mutex::new(None)),
            playing: Arc::new(AtomicBool::new(false)),
            current_position: Arc::new(Mutex::new(0.0)),
        }
    }

    pub fn connect_output(&self, port_name: &str) -> Result<(), Box<dyn Error>> {
        let midi_out = midir::MidiOutput::new("Supersaw")?;
        let ports = midi_out.ports();

        for port in ports {
            if midi_out.port_name(&port)? == port_name {
                let mut output = self.midi_output.lock().unwrap();
                *output = Some(midi_out.connect(&port, "Supersaw")?);
                println!("Connected to MIDI port: {}", port_name);
                return Ok(());
            }
        }

        Err("MIDI port not found".into())
    }

    pub fn disconnect_output(&self) {
        let mut output = self.midi_output.lock().unwrap();
        *output = None;
        println!("Disconnected MIDI output");
    }

    pub fn is_playing(&self) -> bool {
        self.playing.load(Ordering::SeqCst)
    }

    pub fn start_playback(&self, position: f64) {
        if self.is_playing() {
            return; // Already playing
        }

        // Set position
        {
            let mut pos = self.current_position.lock().unwrap();
            *pos = position;
        }

        // Set playing flag
        self.playing.store(true, Ordering::SeqCst);

        // Start playback thread
        let project = Arc::clone(&self.project);
        let midi_output = Arc::clone(&self.midi_output);
        let playing = Arc::clone(&self.playing);
        let current_position = Arc::clone(&self.current_position);

        thread::spawn(move || {
            println!("MIDI playback thread started at position: {}", position);

            // Set high priority if supported by OS

            let mut last_pos = position;
            let mut last_check_time = Instant::now();

            while playing.load(Ordering::SeqCst) {
                // Calculate current position based on elapsed time
                let now = Instant::now();
                let elapsed = now.duration_since(last_check_time).as_secs_f64();
                last_check_time = now;

                // Get updated position
                let current_pos = {
                    let mut pos = current_position.lock().unwrap();
                    *pos += elapsed;
                    *pos
                };

                // Look ahead a small window
                let window_end = current_pos + 10.0; // 100ms lookahead

                // Get events in this window
                let events = {
                    let project_guard = project.lock().unwrap();
                    project_guard.get_all_events_in_time_range(0.0, 100.0)
                };

                if !events.is_empty() {
                    println!(
                        "Found {} events between {} and {}",
                        events.len(),
                        last_pos,
                        window_end
                    );

                    // Sort events by time
                    let mut sorted_events = events;
                    sorted_events.sort_by(|(_, a), (_, b)| a.time.partial_cmp(&b.time).unwrap());

                    // Process events
                    for (track_id, event) in sorted_events {
                        // Get channel for track
                        let channel = {
                            let project_guard = project.lock().unwrap();
                            let track = project_guard.tracks.iter().find(|t| t.id == track_id);

                            if let Some(track) = track {
                                if let crate::core::TrackType::Midi { channel, .. } =
                                    track.track_type
                                {
                                    channel
                                } else {
                                    1 // Default
                                }
                            } else {
                                1 // Default
                            }
                        };

                        // Calculate when to play this event
                        let wait_time = (event.time - current_pos).max(0.0);
                        if wait_time > 0.0 {
                            thread::sleep(Duration::from_secs_f64(wait_time));
                        }

                        // Send the MIDI message
                        let mut output_guard = midi_output.lock().unwrap();
                        if let Some(midi_out) = output_guard.as_mut() {
                            Self::send_midi_message(midi_out, channel, &event.message)
                                .unwrap_or_else(|e| eprintln!("Error sending MIDI: {}", e));
                        }
                    }
                }

                // Update last position for next iteration
                last_pos = window_end;

                // Sleep a bit to avoid high CPU usage
                thread::sleep(Duration::from_millis(10));
            }

            println!("MIDI playback thread stopped");

            // Send all notes off on stop
            let mut output = midi_output.lock().unwrap();
            if let Some(midi_out) = output.as_mut() {
                for channel in 0..16 {
                    let _ = midi_out.send(&[0xB0 | channel, 123, 0]); // All Notes Off
                }
            }
        });
    }

    fn send_midi_message(
        midi_out: &mut midir::MidiOutputConnection,
        channel: u8,
        message: &MidiMessage,
    ) -> Result<(), Box<dyn Error>> {
        println!("Sending MIDI message on channel {}: {:?}", channel, message);

        match message {
            MidiMessage::NoteOn { key, velocity, .. } => {
                let midi_message = [0x90 | (channel - 1), *key, *velocity];
                midi_out.send(&midi_message)?;
            }
            MidiMessage::NoteOff { key, velocity, .. } => {
                let midi_message = [0x80 | (channel - 1), *key, *velocity];
                midi_out.send(&midi_message)?;
            }
            MidiMessage::ControlChange {
                controller, value, ..
            } => {
                let midi_message = [0xB0 | (channel - 1), *controller, *value];
                midi_out.send(&midi_message)?;
            }
            MidiMessage::ProgramChange { program, .. } => {
                let midi_message = [0xC0 | (channel - 1), *program];
                midi_out.send(&midi_message)?;
            }
            MidiMessage::PitchBend { value, .. } => {
                let lsb = (*value & 0x7F) as u8;
                let msb = ((*value >> 7) & 0x7F) as u8;
                let midi_message = [0xE0 | (channel - 1), lsb, msb];
                midi_out.send(&midi_message)?;
            }
            // Handle other message types as needed
            _ => {}
        }
        Ok(())
    }

    pub fn stop_playback(&self) {
        self.playing.store(false, Ordering::SeqCst);
    }

    pub fn update_project(&self, project: Project) {
        let mut proj = self.project.lock().unwrap();
        *proj = project;
    }

    pub fn update_position(&self, position: f64) {
        let mut pos = self.current_position.lock().unwrap();
        *pos = position;
    }
}
