use std::collections::HashMap;
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc,
};

use crossbeam::channel::{bounded, Receiver, Sender, TrySendError};
use parking_lot::RwLock;

use crate::core::MidiMessage;

/// Recording modes
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RecordingMode {
    Overdub,    // Add to existing MIDI data
    Replace,    // Overwrite in recording range
    PunchInOut, // Record only between punch points
}

/// Immutable transport and editing context captured when a recording begins.
///
/// Keeping this with the committed event batch prevents UI changes made while
/// recording from changing how the completed session is applied.
#[derive(Debug, Clone, PartialEq)]
pub struct RecordingSessionContext {
    pub track_id: String,
    pub target_clip_id: Option<String>,
    pub mode: RecordingMode,
    pub transport_start_seconds: f64,
    pub punch_range: Option<(f64, f64)>,
    pub loop_range: Option<(f64, f64)>,
}

impl RecordingSessionContext {
    pub fn new(
        track_id: String,
        target_clip_id: Option<String>,
        mode: RecordingMode,
        transport_start_seconds: f64,
        punch_range: Option<(f64, f64)>,
        loop_range: Option<(f64, f64)>,
    ) -> Self {
        Self {
            track_id,
            target_clip_id,
            mode,
            transport_start_seconds,
            punch_range: punch_range.filter(|(start, end)| end > start),
            loop_range: loop_range.filter(|(start, end)| end - start > f64::EPSILON),
        }
    }
}

/// Monitoring modes for input
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MonitoringMode {
    Off,      // No monitoring
    Auto,     // Monitor when track armed
    Input,    // Always monitor input
    Playback, // Monitor playback only
}

/// Recording state for a track
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RecordingState {
    Idle,
    Armed,
    Recording,
    Stopping,
}

/// Configuration for recording
#[derive(Debug, Clone)]
pub struct RecordingConfig {
    pub count_in_bars: u32,
    pub pre_roll_ms: u32,
    pub quantize_on_record: bool,
    pub quantize_strength: f32,
    pub monitoring_mode: MonitoringMode,
    pub metronome_during_record: bool,
    pub metronome_during_count_in: bool,
}

impl Default for RecordingConfig {
    fn default() -> Self {
        Self {
            count_in_bars: 1,
            pre_roll_ms: 200,
            quantize_on_record: false,
            quantize_strength: 1.0,
            monitoring_mode: MonitoringMode::Auto,
            metronome_during_record: false,
            metronome_during_count_in: true,
        }
    }
}

/// Commands sent to the recording thread
#[derive(Debug, Clone)]
pub enum RecordingCommand {
    StartRecordingSession {
        session: RecordingSessionContext,
        capture_start_sample: u64,
    },
    StopRecording {
        track_id: String,
        commit: bool,
    },
    ArmTrack {
        track_id: String,
        input_port: String,
        channel_filter: Option<u8>,
    },
    DisarmTrack {
        track_id: String,
    },
    SetInputMonitoring {
        track_id: String,
        enabled: bool,
    },
    UpdateConfig(RecordingConfig),
    SetTempo(f64),
}

/// Events received from the recording thread
#[derive(Debug, Clone)]
pub enum RecordingEvent {
    RecordingStarted {
        track_id: String,
    },
    RecordingStopped {
        track_id: String,
        events_recorded: usize,
    },
    EventsRecorded {
        session: RecordingSessionContext,
        events: Vec<RecordedEvent>,
    },
    BufferOverflow {
        track_id: String,
        dropped_events: usize,
    },
    MonitoringEvent {
        track_id: String,
        message: MidiMessage,
    },
}

/// A timestamped MIDI event from recording
#[derive(Debug, Clone)]
pub struct RecordedEvent {
    pub timestamp_samples: u64,
    pub timestamp_beats: f64,
    pub port_id: String,
    pub message: MidiMessage,
}

/// Active recording session for a track
struct ActiveRecording {
    session: RecordingSessionContext,
    start_sample: u64,
    punch_in_sample: Option<u64>,
    punch_out_sample: Option<u64>,
    events: Vec<RecordedEvent>,
    input_port: String,
    channel_filter: Option<u8>,
}

/// Pre-roll buffer for retroactive recording
struct PreRollBuffer {
    buffer: Vec<RecordedEvent>,
    capacity: usize,
    write_index: usize,
}

impl PreRollBuffer {
    fn new(capacity: usize) -> Self {
        Self {
            buffer: Vec::with_capacity(capacity),
            capacity,
            write_index: 0,
        }
    }

    fn push(&mut self, event: RecordedEvent) {
        if self.capacity == 0 {
            return;
        }
        if self.buffer.len() < self.capacity {
            self.buffer.push(event);
        } else {
            self.buffer[self.write_index] = event;
            self.write_index = (self.write_index + 1) % self.capacity;
        }
    }

    fn get_events_since(&self, timestamp: u64) -> Vec<RecordedEvent> {
        let oldest_index = if self.buffer.len() < self.capacity {
            0
        } else {
            self.write_index
        };
        (0..self.buffer.len())
            .map(|offset| (oldest_index + offset) % self.buffer.len())
            .map(|index| &self.buffer[index])
            // Timestamps from separate MIDI connections can have different
            // origins, so do not assume one older value makes the rest old.
            .filter(|event| event.timestamp_samples >= timestamp)
            .cloned()
            .collect()
    }
}

/// Main recording coordinator
pub struct RecordingCoordinator {
    // Communication with recording thread
    command_tx: Sender<RecordingCommand>,
    event_rx: Receiver<RecordingEvent>,

    // Recording thread handle
    thread_handle: Option<std::thread::JoinHandle<()>>,
    shutdown: Arc<AtomicBool>,
    capture_sample_clock: Arc<AtomicU64>,

    // Shared configuration
    config: Arc<RwLock<RecordingConfig>>,
}

#[derive(Clone)]
struct ArmedTrackInfo {
    input_port: String,
    channel_filter: Option<u8>,
    monitoring: bool,
}

impl RecordingCoordinator {
    pub fn new(
        sample_rate: u32,
        midi_input_rx: Receiver<(String, MidiMessage, u64)>,
        capture_sample_clock: Arc<AtomicU64>,
    ) -> Self {
        let (command_tx, command_rx) = bounded(256);
        let (event_tx, event_rx) = bounded(1024);

        let config = Arc::new(RwLock::new(RecordingConfig::default()));
        let armed_tracks = Arc::new(RwLock::new(HashMap::new()));

        // Spawn recording thread
        let thread_config = config.clone();
        let thread_armed_tracks = armed_tracks.clone();
        let shutdown = Arc::new(AtomicBool::new(false));
        let thread_shutdown = shutdown.clone();
        let thread_handle = std::thread::Builder::new()
            .name("midi_recording".to_string())
            .spawn(move || {
                let mut recorder = MidiRecorder::new(
                    sample_rate,
                    command_rx,
                    event_tx,
                    midi_input_rx,
                    thread_config,
                    thread_armed_tracks,
                    thread_shutdown,
                );
                recorder.run();
            })
            .expect("Failed to spawn recording thread");

        Self {
            command_tx,
            event_rx,
            thread_handle: Some(thread_handle),
            shutdown,
            capture_sample_clock,
            config,
        }
    }

    /// Send a command to the recording thread
    pub fn send_command(&self, command: RecordingCommand) {
        let _ = self.command_tx.send(command);
    }

    /// Try to receive events from the recording thread
    pub fn try_recv_event(&self) -> Option<RecordingEvent> {
        self.event_rx.try_recv().ok()
    }

    /// Update recording configuration
    pub fn update_config(&self, config: RecordingConfig) {
        *self.config.write() = config.clone();
        self.send_command(RecordingCommand::UpdateConfig(config));
    }

    /// Get current recording configuration
    pub fn get_config(&self) -> RecordingConfig {
        self.config.read().clone()
    }

    /// Start recording with immutable project/transport context.
    pub fn start_recording_session(&mut self, session: RecordingSessionContext) {
        self.send_command(RecordingCommand::StartRecordingSession {
            session,
            capture_start_sample: self.capture_sample_clock.load(Ordering::Acquire),
        });
    }

    /// Stop recording on a track
    pub fn stop_recording(&mut self, track_id: &str, commit: bool) {
        self.send_command(RecordingCommand::StopRecording {
            track_id: track_id.to_string(),
            commit,
        });
    }

    /// Set input monitoring for a track
    pub fn set_input_monitoring(&mut self, track_id: &str, enabled: bool) {
        self.send_command(RecordingCommand::SetInputMonitoring {
            track_id: track_id.to_string(),
            enabled,
        });
    }
}

impl Drop for RecordingCoordinator {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Release);
        if let Some(thread) = self.thread_handle.take() {
            let _ = thread.join();
        }
    }
}

/// The actual recording thread implementation
struct MidiRecorder {
    sample_rate: u32,

    // Communication
    command_rx: Receiver<RecordingCommand>,
    event_tx: Sender<RecordingEvent>,
    midi_input_rx: Receiver<(String, MidiMessage, u64)>,

    // State
    config: Arc<RwLock<RecordingConfig>>,
    armed_tracks: Arc<RwLock<HashMap<String, ArmedTrackInfo>>>,
    active_recordings: HashMap<String, ActiveRecording>,

    // Pre-roll buffer
    pre_roll_buffer: PreRollBuffer,

    // Timing
    tempo: f64,
    beats_per_sample: f64,
    shutdown: Arc<AtomicBool>,
}

impl MidiRecorder {
    fn new(
        sample_rate: u32,
        command_rx: Receiver<RecordingCommand>,
        event_tx: Sender<RecordingEvent>,
        midi_input_rx: Receiver<(String, MidiMessage, u64)>,
        config: Arc<RwLock<RecordingConfig>>,
        armed_tracks: Arc<RwLock<HashMap<String, ArmedTrackInfo>>>,
        shutdown: Arc<AtomicBool>,
    ) -> Self {
        let pre_roll_capacity = (sample_rate as f64 * 0.2) as usize; // 200ms buffer

        Self {
            sample_rate,
            command_rx,
            event_tx,
            midi_input_rx,
            config,
            armed_tracks,
            active_recordings: HashMap::new(),
            pre_roll_buffer: PreRollBuffer::new(pre_roll_capacity),
            tempo: 120.0, // Default, will be updated
            beats_per_sample: 120.0 / 60.0 / sample_rate as f64,
            shutdown,
        }
    }

    fn run(&mut self) {
        while !self.shutdown.load(Ordering::Acquire) {
            // Process commands
            while let Ok(command) = self.command_rx.try_recv() {
                self.handle_command(command);
            }

            // Process incoming MIDI events
            while let Ok((port_id, message, timestamp)) = self.midi_input_rx.try_recv() {
                self.handle_midi_event(port_id, message, timestamp);
            }

            // Sleep briefly to avoid busy waiting
            std::thread::sleep(std::time::Duration::from_micros(100));
        }
    }

    fn handle_command(&mut self, command: RecordingCommand) {
        match command {
            RecordingCommand::StartRecordingSession {
                session,
                capture_start_sample,
            } => self.start_recording(session, capture_start_sample),
            RecordingCommand::StopRecording { track_id, commit } => {
                self.stop_recording(track_id, commit);
            }
            RecordingCommand::ArmTrack {
                track_id,
                input_port,
                channel_filter,
            } => {
                self.armed_tracks.write().insert(
                    track_id.clone(),
                    ArmedTrackInfo {
                        input_port,
                        channel_filter,
                        monitoring: false,
                    },
                );
            }
            RecordingCommand::DisarmTrack { track_id } => {
                self.armed_tracks.write().remove(&track_id);
                // Also stop any active recording
                self.stop_recording(track_id, false);
            }
            RecordingCommand::SetInputMonitoring { track_id, enabled } => {
                if let Some(info) = self.armed_tracks.write().get_mut(&track_id) {
                    info.monitoring = enabled;
                }
            }
            RecordingCommand::UpdateConfig(new_config) => {
                *self.config.write() = new_config;
            }
            RecordingCommand::SetTempo(bpm) => {
                self.tempo = bpm;
                // Update beats_per_sample with new tempo
                self.beats_per_sample = bpm / 60.0 / self.sample_rate as f64;
            }
        }
    }

    fn handle_midi_event(&mut self, port_id: String, message: MidiMessage, timestamp: u64) {
        let timestamp_beats = timestamp as f64 * self.beats_per_sample;

        let recorded_event = RecordedEvent {
            timestamp_samples: timestamp,
            timestamp_beats,
            port_id: port_id.clone(),
            message: message.clone(),
        };

        // Always add to pre-roll buffer
        self.pre_roll_buffer.push(recorded_event.clone());

        for recording in self.active_recordings.values_mut() {
            // Check if this event is for this track's input
            if !input_port_matches(&recording.input_port, &port_id) {
                continue;
            }

            // System messages are not channel-scoped and pass every channel filter.
            if recording
                .channel_filter
                .zip(message.channel())
                .is_some_and(|(filter, channel)| filter != channel)
            {
                continue;
            }

            // Check punch in/out
            let Some(relative_samples) = timestamp.checked_sub(recording.start_sample) else {
                // Events queued before the start command belong only to the
                // explicit pre-roll path; never collapse them onto time zero.
                continue;
            };

            if let Some(punch_in) = recording.punch_in_sample {
                if relative_samples < punch_in {
                    continue;
                }
            }

            if let Some(punch_out) = recording.punch_out_sample {
                if relative_samples >= punch_out {
                    continue;
                }
            }

            recording.events.push(RecordedEvent {
                timestamp_samples: relative_samples,
                timestamp_beats: relative_samples as f64 * self.beats_per_sample,
                port_id: recorded_event.port_id.clone(),
                message: recorded_event.message.clone(),
            });
        }

        // Handle monitoring for armed tracks
        let armed_tracks = self.armed_tracks.read();
        for (track_id, info) in armed_tracks.iter() {
            if !info.monitoring {
                continue;
            }

            // Check if this event is for this track's input
            if !input_port_matches(&info.input_port, &port_id) {
                continue;
            }

            if info
                .channel_filter
                .zip(message.channel())
                .is_some_and(|(filter, channel)| filter != channel)
            {
                continue;
            }

            // Send monitoring event
            let _ = self.event_tx.try_send(RecordingEvent::MonitoringEvent {
                track_id: track_id.clone(),
                message: message.clone(),
            });
        }
    }

    fn start_recording(&mut self, mut session: RecordingSessionContext, current_sample: u64) {
        let track_id = session.track_id.clone();
        // Get armed track info
        let armed_info = match self.armed_tracks.read().get(&track_id) {
            Some(info) => info.clone(),
            None => return, // Track not armed
        };

        // Get pre-roll events if configured
        let config = self.config.read();
        let mut pre_roll_events = Vec::new();

        if config.pre_roll_ms > 0 && session.mode != RecordingMode::PunchInOut {
            let pre_roll_samples =
                (self.sample_rate as f64 * config.pre_roll_ms as f64 / 1000.0) as u64;
            let pre_roll_start = current_sample.saturating_sub(pre_roll_samples);
            pre_roll_events = self
                .pre_roll_buffer
                .get_events_since(pre_roll_start)
                .into_iter()
                .filter(|event| input_port_matches(&armed_info.input_port, &event.port_id))
                .filter(|event| {
                    !armed_info
                        .channel_filter
                        .zip(event.message.channel())
                        .is_some_and(|(filter, channel)| filter != channel)
                })
                .collect::<Vec<_>>();
        }
        drop(config);

        let recording_origin = pre_roll_events
            .first()
            .map(|event| event.timestamp_samples.min(current_sample))
            .unwrap_or(current_sample);
        let pre_roll_seconds =
            current_sample.saturating_sub(recording_origin) as f64 / self.sample_rate as f64;
        session.transport_start_seconds -= pre_roll_seconds;
        let initial_events = pre_roll_events
            .into_iter()
            .map(|event| RecordedEvent {
                timestamp_samples: event.timestamp_samples.saturating_sub(recording_origin),
                timestamp_beats: event.timestamp_samples.saturating_sub(recording_origin) as f64
                    * self.beats_per_sample,
                port_id: event.port_id,
                message: event.message,
            })
            .collect();

        // Convert absolute project punch points to this session's sample clock.
        let (punch_in_sample, punch_out_sample) = session
            .punch_range
            .filter(|_| session.mode == RecordingMode::PunchInOut)
            .map(|(punch_in, punch_out)| {
                let relative_in = (punch_in - session.transport_start_seconds).max(0.0);
                let relative_out = (punch_out - session.transport_start_seconds).max(0.0);
                (
                    Some((relative_in * self.sample_rate as f64) as u64),
                    Some((relative_out * self.sample_rate as f64) as u64),
                )
            })
            .unwrap_or((None, None));

        // Create active recording
        let recording = ActiveRecording {
            session,
            start_sample: recording_origin,
            punch_in_sample,
            punch_out_sample,
            events: initial_events,
            input_port: armed_info.input_port,
            channel_filter: armed_info.channel_filter,
        };

        self.active_recordings.insert(track_id.clone(), recording);

        // Notify recording started
        let _ = self
            .event_tx
            .try_send(RecordingEvent::RecordingStarted { track_id });
    }

    fn stop_recording(&mut self, track_id: String, commit: bool) {
        if let Some(recording) = self.active_recordings.remove(&track_id) {
            let events_count = recording.events.len();

            if commit && events_count > 0 {
                self.send_recorded_batch(RecordingEvent::EventsRecorded {
                    session: recording.session,
                    events: recording.events,
                });
            }

            // Notify recording stopped
            let _ = self.event_tx.try_send(RecordingEvent::RecordingStopped {
                track_id,
                events_recorded: events_count,
            });
        }
    }

    /// Preserve the one-stop/one-batch contract without making shutdown wait
    /// forever on a full UI event queue.
    fn send_recorded_batch(&self, mut event: RecordingEvent) {
        loop {
            match self.event_tx.try_send(event) {
                Ok(()) | Err(TrySendError::Disconnected(_)) => return,
                Err(TrySendError::Full(returned_event)) => {
                    if self.shutdown.load(Ordering::Acquire) {
                        return;
                    }
                    event = returned_event;
                    std::thread::sleep(std::time::Duration::from_micros(100));
                }
            }
        }
    }
}

fn input_port_matches(configured_port: &str, received_port: &str) -> bool {
    configured_port.is_empty() || configured_port == "default" || configured_port == received_port
}

#[cfg(test)]
mod tests {
    use super::*;

    fn recorder() -> (MidiRecorder, Receiver<RecordingEvent>) {
        recorder_with_pre_roll(0)
    }

    fn recorder_with_pre_roll(pre_roll_ms: u32) -> (MidiRecorder, Receiver<RecordingEvent>) {
        let (_command_tx, command_rx) = bounded(1);
        let (_input_tx, midi_input_rx) = bounded(1);
        let (event_tx, event_rx) = bounded(8);
        let config = Arc::new(RwLock::new(RecordingConfig {
            pre_roll_ms,
            ..RecordingConfig::default()
        }));
        let armed_tracks = Arc::new(RwLock::new(HashMap::new()));

        (
            MidiRecorder::new(
                48_000,
                command_rx,
                event_tx,
                midi_input_rx,
                config,
                armed_tracks,
                Arc::new(AtomicBool::new(false)),
            ),
            event_rx,
        )
    }

    fn note_on() -> MidiMessage {
        MidiMessage::NoteOn {
            channel: 0,
            key: 60,
            velocity: 100,
        }
    }

    #[test]
    fn monitoring_follows_the_armed_port_and_stops_when_disabled() {
        let (mut recorder, event_rx) = recorder();
        recorder.handle_command(RecordingCommand::ArmTrack {
            track_id: "track".to_string(),
            input_port: "Keyboard".to_string(),
            channel_filter: None,
        });
        recorder.handle_command(RecordingCommand::SetInputMonitoring {
            track_id: "track".to_string(),
            enabled: true,
        });

        recorder.handle_midi_event("Other".to_string(), note_on(), 0);
        assert!(event_rx.try_recv().is_err());

        recorder.handle_midi_event("Keyboard".to_string(), note_on(), 1);
        assert!(matches!(
            event_rx.try_recv(),
            Ok(RecordingEvent::MonitoringEvent { track_id, .. }) if track_id == "track"
        ));

        recorder.handle_command(RecordingCommand::SetInputMonitoring {
            track_id: "track".to_string(),
            enabled: false,
        });
        recorder.handle_midi_event("Keyboard".to_string(), note_on(), 2);
        assert!(event_rx.try_recv().is_err());
    }

    #[test]
    fn default_armed_port_accepts_events_from_connected_input() {
        let (mut recorder, event_rx) = recorder();
        recorder.handle_command(RecordingCommand::ArmTrack {
            track_id: "track".to_string(),
            input_port: "default".to_string(),
            channel_filter: None,
        });
        recorder.start_recording(
            RecordingSessionContext::new(
                "track".to_string(),
                None,
                RecordingMode::Overdub,
                0.0,
                None,
                None,
            ),
            48_000,
        );
        let _ = event_rx.try_recv(); // RecordingStarted

        recorder.handle_midi_event("Keyboard".to_string(), note_on(), 48_000);
        recorder.stop_recording("track".to_string(), true);

        match event_rx.try_recv().unwrap() {
            RecordingEvent::EventsRecorded { events, .. } => {
                assert_eq!(events.len(), 1);
                assert_eq!(events[0].timestamp_samples, 0);
            }
            event => panic!("expected recorded batch, got {event:?}"),
        }
    }

    #[test]
    fn recording_emits_one_coherent_batch_when_stopped() {
        let (mut recorder, event_rx) = recorder();
        recorder.handle_command(RecordingCommand::ArmTrack {
            track_id: "track".to_string(),
            input_port: "Keyboard".to_string(),
            channel_filter: None,
        });
        let session = RecordingSessionContext::new(
            "track".to_string(),
            Some("clip".to_string()),
            RecordingMode::Overdub,
            12.0,
            None,
            Some((8.0, 12.0)),
        );
        recorder.start_recording(session.clone(), 48_000);
        let _ = event_rx.try_recv(); // RecordingStarted

        recorder.handle_midi_event("Keyboard".to_string(), note_on(), 48_000);
        recorder.handle_midi_event(
            "Keyboard".to_string(),
            MidiMessage::NoteOff {
                channel: 0,
                key: 60,
                velocity: 0,
            },
            72_000,
        );
        assert!(event_rx.try_recv().is_err());

        recorder.stop_recording("track".to_string(), true);
        match event_rx.try_recv().unwrap() {
            RecordingEvent::EventsRecorded {
                session: committed_session,
                events,
            } => {
                assert_eq!(committed_session, session);
                assert_eq!(events.len(), 2);
                assert_eq!(events[0].timestamp_samples, 0);
                assert_eq!(events[1].timestamp_samples, 24_000);
                assert_eq!(events[1].timestamp_beats, 1.0);
            }
            event => panic!("expected recorded batch, got {event:?}"),
        }
    }

    #[test]
    fn channel_filter_applies_to_every_channel_message() {
        let (mut recorder, event_rx) = recorder();
        recorder.handle_command(RecordingCommand::ArmTrack {
            track_id: "track".to_string(),
            input_port: "Keyboard".to_string(),
            channel_filter: Some(2),
        });
        recorder.start_recording(
            RecordingSessionContext::new(
                "track".to_string(),
                None,
                RecordingMode::Overdub,
                0.0,
                None,
                None,
            ),
            0,
        );
        let _ = event_rx.try_recv();

        recorder.handle_midi_event(
            "Keyboard".to_string(),
            MidiMessage::ProgramChange {
                channel: 1,
                program: 4,
            },
            0,
        );
        recorder.handle_midi_event(
            "Keyboard".to_string(),
            MidiMessage::Aftertouch {
                channel: 2,
                key: 60,
                pressure: 80,
            },
            1,
        );
        recorder.stop_recording("track".to_string(), true);

        match event_rx.try_recv().unwrap() {
            RecordingEvent::EventsRecorded { events, .. } => {
                assert_eq!(events.len(), 1);
                assert!(matches!(events[0].message, MidiMessage::Aftertouch { .. }));
            }
            event => panic!("expected recorded batch, got {event:?}"),
        }
    }

    #[test]
    fn punch_range_is_half_open() {
        let (mut recorder, event_rx) = recorder();
        recorder.handle_command(RecordingCommand::ArmTrack {
            track_id: "track".to_string(),
            input_port: "default".to_string(),
            channel_filter: None,
        });
        recorder.start_recording(
            RecordingSessionContext::new(
                "track".to_string(),
                None,
                RecordingMode::PunchInOut,
                0.0,
                Some((0.5, 1.0)),
                None,
            ),
            0,
        );
        let _ = event_rx.try_recv();

        // The first played note arrives after punch-in. Its timestamp remains
        // relative to recording start instead of becoming the clock origin.
        recorder.handle_midi_event("Keyboard".to_string(), note_on(), 36_000);
        recorder.handle_midi_event("Keyboard".to_string(), note_on(), 48_000);
        recorder.stop_recording("track".to_string(), true);

        match event_rx.try_recv().unwrap() {
            RecordingEvent::EventsRecorded { events, .. } => {
                assert_eq!(events.len(), 1);
                assert_eq!(events[0].timestamp_samples, 36_000);
            }
            event => panic!("expected recorded batch, got {event:?}"),
        }
    }

    #[test]
    fn partially_filled_pre_roll_buffer_is_read_in_timestamp_order() {
        let mut buffer = PreRollBuffer::new(3);
        for timestamp_samples in [1, 2] {
            buffer.push(RecordedEvent {
                timestamp_samples,
                timestamp_beats: 0.0,
                port_id: "Keyboard".to_string(),
                message: note_on(),
            });
        }
        assert_eq!(
            buffer
                .get_events_since(0)
                .iter()
                .map(|event| event.timestamp_samples)
                .collect::<Vec<_>>(),
            [1, 2]
        );

        for timestamp_samples in [3, 4] {
            buffer.push(RecordedEvent {
                timestamp_samples,
                timestamp_beats: 0.0,
                port_id: "Keyboard".to_string(),
                message: note_on(),
            });
        }
        assert_eq!(
            buffer
                .get_events_since(0)
                .iter()
                .map(|event| event.timestamp_samples)
                .collect::<Vec<_>>(),
            [2, 3, 4]
        );
    }

    #[test]
    fn pre_roll_respects_the_armed_port_and_channel_filter() {
        let (mut recorder, event_rx) = recorder_with_pre_roll(1_000);
        recorder.handle_command(RecordingCommand::ArmTrack {
            track_id: "track".to_string(),
            input_port: "Keyboard".to_string(),
            channel_filter: Some(2),
        });
        recorder.handle_midi_event("Other".to_string(), note_on(), 1);
        recorder.handle_midi_event(
            "Keyboard".to_string(),
            MidiMessage::ProgramChange {
                channel: 1,
                program: 10,
            },
            2,
        );
        recorder.handle_midi_event(
            "Keyboard".to_string(),
            MidiMessage::ProgramChange {
                channel: 2,
                program: 11,
            },
            3,
        );

        recorder.start_recording(
            RecordingSessionContext::new(
                "track".to_string(),
                None,
                RecordingMode::Overdub,
                0.0,
                None,
                None,
            ),
            3,
        );
        let _ = event_rx.try_recv();
        recorder.stop_recording("track".to_string(), true);

        match event_rx.try_recv().unwrap() {
            RecordingEvent::EventsRecorded { events, .. } => {
                assert_eq!(events.len(), 1);
                assert!(matches!(
                    events[0].message,
                    MidiMessage::ProgramChange { program: 11, .. }
                ));
            }
            event => panic!("expected recorded batch, got {event:?}"),
        }
    }

    #[test]
    fn queued_event_before_capture_start_is_not_rebased_to_zero() {
        let (mut recorder, event_rx) = recorder();
        recorder.handle_command(RecordingCommand::ArmTrack {
            track_id: "track".to_string(),
            input_port: "Keyboard".to_string(),
            channel_filter: None,
        });
        recorder.start_recording(
            RecordingSessionContext::new(
                "track".to_string(),
                None,
                RecordingMode::Overdub,
                0.0,
                None,
                None,
            ),
            100,
        );
        let _ = event_rx.try_recv();

        recorder.handle_midi_event("Keyboard".to_string(), note_on(), 99);
        recorder.handle_midi_event("Keyboard".to_string(), note_on(), 101);
        recorder.stop_recording("track".to_string(), true);

        match event_rx.try_recv().unwrap() {
            RecordingEvent::EventsRecorded { events, .. } => {
                assert_eq!(events.len(), 1);
                assert_eq!(events[0].timestamp_samples, 1);
            }
            event => panic!("expected recorded batch, got {event:?}"),
        }
    }
}
