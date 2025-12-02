use std::sync::{Arc, atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering}};
use std::collections::HashMap;
use parking_lot::{Mutex, RwLock};
use crossbeam::channel::{Sender, Receiver, bounded};
use crate::core::{MidiMessage, MidiEvent, MidiEventStore, MidiEngineMessage};

/// Recording modes
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RecordingMode {
    Overdub,    // Add to existing MIDI data
    Replace,    // Overwrite in recording range  
    PunchInOut, // Record only between punch points
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
    StartRecording {
        track_id: String,
        clip_id: Option<String>,
        mode: RecordingMode,
        punch_in: Option<f64>,
        punch_out: Option<f64>,
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
        timestamp: f64,
    },
    RecordingStopped {
        track_id: String,
        events_recorded: usize,
    },
    EventsRecorded {
        track_id: String,
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
    track_id: String,
    clip_id: Option<String>,
    mode: RecordingMode,
    start_sample: u64,
    start_beat: f64,
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
        if self.buffer.len() < self.capacity {
            self.buffer.push(event);
        } else {
            self.buffer[self.write_index] = event;
            self.write_index = (self.write_index + 1) % self.capacity;
        }
    }
    
    fn get_events_since(&self, timestamp: u64) -> Vec<RecordedEvent> {
        let mut result = Vec::new();
        
        // Collect events newer than timestamp
        for i in 0..self.buffer.len() {
            let idx = (self.write_index + self.capacity - i - 1) % self.capacity;
            let event = &self.buffer[idx];
            if event.timestamp_samples >= timestamp {
                result.push(event.clone());
            } else {
                break;
            }
        }
        
        result.reverse();
        result
    }
}

/// Main recording coordinator
pub struct RecordingCoordinator {
    // Communication with recording thread
    command_tx: Sender<RecordingCommand>,
    event_rx: Receiver<RecordingEvent>,
    
    // Recording thread handle
    thread_handle: Option<std::thread::JoinHandle<()>>,
    
    // Shared state
    config: Arc<RwLock<RecordingConfig>>,
    armed_tracks: Arc<RwLock<HashMap<String, ArmedTrackInfo>>>,
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
    ) -> Self {
        let (command_tx, command_rx) = bounded(256);
        let (event_tx, event_rx) = bounded(1024);
        
        let config = Arc::new(RwLock::new(RecordingConfig::default()));
        let armed_tracks = Arc::new(RwLock::new(HashMap::new()));
        
        // Spawn recording thread
        let thread_config = config.clone();
        let thread_armed_tracks = armed_tracks.clone();
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
                );
                recorder.run();
            })
            .expect("Failed to spawn recording thread");
        
        Self {
            command_tx,
            event_rx,
            thread_handle: Some(thread_handle),
            config,
            armed_tracks,
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
    
    /// Get current armed tracks
    pub fn get_armed_tracks(&self) -> Vec<String> {
        self.armed_tracks.read().keys().cloned().collect()
    }
    
    /// Get current recording configuration
    pub fn get_config(&self) -> RecordingConfig {
        self.config.read().clone()
    }
    
    /// Start recording on a track
    pub fn start_recording(
        &mut self,
        track_id: String,
        clip_id: Option<String>,
        mode: RecordingMode,
        punch_in: Option<f64>,
        punch_out: Option<f64>,
    ) {
        self.send_command(RecordingCommand::StartRecording {
            track_id,
            clip_id,
            mode,
            punch_in,
            punch_out,
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
    
    /// Handle incoming MIDI input
    pub fn handle_midi_input(&mut self, port_id: String, message: MidiMessage, timestamp: u64) {
        // The recording thread will receive this through midi_input_rx
        // which is already connected to the MIDI engine
    }
}

/// The actual recording thread implementation
struct MidiRecorder {
    sample_rate: u32,
    current_sample: AtomicU64,
    
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
}

impl MidiRecorder {
    fn new(
        sample_rate: u32,
        command_rx: Receiver<RecordingCommand>,
        event_tx: Sender<RecordingEvent>,
        midi_input_rx: Receiver<(String, MidiMessage, u64)>,
        config: Arc<RwLock<RecordingConfig>>,
        armed_tracks: Arc<RwLock<HashMap<String, ArmedTrackInfo>>>,
    ) -> Self {
        let pre_roll_capacity = (sample_rate as f64 * 0.2) as usize; // 200ms buffer
        
        Self {
            sample_rate,
            current_sample: AtomicU64::new(0),
            command_rx,
            event_tx,
            midi_input_rx,
            config,
            armed_tracks,
            active_recordings: HashMap::new(),
            pre_roll_buffer: PreRollBuffer::new(pre_roll_capacity),
            tempo: 120.0, // Default, will be updated
            beats_per_sample: 120.0 / 60.0 / sample_rate as f64,
        }
    }
    
    fn run(&mut self) {
        loop {
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
            RecordingCommand::StartRecording { track_id, clip_id, mode, punch_in, punch_out } => {
                self.start_recording(track_id, clip_id, mode, punch_in, punch_out);
            }
            RecordingCommand::StopRecording { track_id, commit } => {
                self.stop_recording(track_id, commit);
            }
            RecordingCommand::ArmTrack { track_id, input_port, channel_filter } => {
                self.armed_tracks.write().insert(track_id.clone(), ArmedTrackInfo {
                    input_port,
                    channel_filter,
                    monitoring: false,
                });
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
        
        // Check active recordings
        let mut events_to_send = Vec::new();
        
        for (track_id, recording) in &mut self.active_recordings {
            // Check if this event is for this track's input
            if recording.input_port != port_id {
                continue;
            }
            
            // Check channel filter
            if let Some(filter_channel) = recording.channel_filter {
                match &message {
                    MidiMessage::NoteOn { channel, .. } |
                    MidiMessage::NoteOff { channel, .. } |
                    MidiMessage::ControlChange { channel, .. } => {
                        if *channel != filter_channel {
                            continue;
                        }
                    }
                    _ => {}
                }
            }
            
            // Check punch in/out
            if let Some(punch_in) = recording.punch_in_sample {
                if timestamp < punch_in {
                    continue;
                }
            }
            
            if let Some(punch_out) = recording.punch_out_sample {
                if timestamp > punch_out {
                    continue;
                }
            }
            
            // Record the event
            recording.events.push(recorded_event.clone());
            events_to_send.push((track_id.clone(), recorded_event.clone()));
        }
        
        // Send recorded events
        for (track_id, event) in events_to_send {
            let _ = self.event_tx.send(RecordingEvent::EventsRecorded {
                track_id,
                events: vec![event],
            });
        }
        
        // Handle monitoring for armed tracks
        let armed_tracks = self.armed_tracks.read();
        for (track_id, info) in armed_tracks.iter() {
            if !info.monitoring {
                continue;
            }
            
            // Check if this event is for this track's input
            if info.input_port != port_id {
                continue;
            }
            
            // Check channel filter
            if let Some(filter_channel) = info.channel_filter {
                match &message {
                    MidiMessage::NoteOn { channel, .. } |
                    MidiMessage::NoteOff { channel, .. } |
                    MidiMessage::ControlChange { channel, .. } => {
                        if *channel != filter_channel {
                            continue;
                        }
                    }
                    _ => {}
                }
            }
            
            // Send monitoring event
            let _ = self.event_tx.send(RecordingEvent::MonitoringEvent {
                track_id: track_id.clone(),
                message: message.clone(),
            });
        }
    }
    
    fn start_recording(
        &mut self,
        track_id: String,
        clip_id: Option<String>,
        mode: RecordingMode,
        punch_in: Option<f64>,
        punch_out: Option<f64>,
    ) {
        // Get armed track info
        let armed_info = match self.armed_tracks.read().get(&track_id) {
            Some(info) => info.clone(),
            None => return, // Track not armed
        };
        
        let current_sample = self.current_sample.load(Ordering::Relaxed);
        let current_beat = current_sample as f64 * self.beats_per_sample;
        
        // Convert punch points to samples
        let punch_in_sample = punch_in.map(|beats| (beats / self.beats_per_sample) as u64);
        let punch_out_sample = punch_out.map(|beats| (beats / self.beats_per_sample) as u64);
        
        // Get pre-roll events if configured
        let config = self.config.read();
        let mut initial_events = Vec::new();
        
        if config.pre_roll_ms > 0 {
            let pre_roll_samples = (self.sample_rate as f64 * config.pre_roll_ms as f64 / 1000.0) as u64;
            let pre_roll_start = current_sample.saturating_sub(pre_roll_samples);
            initial_events = self.pre_roll_buffer.get_events_since(pre_roll_start);
        }
        
        // Create active recording
        let recording = ActiveRecording {
            track_id: track_id.clone(),
            clip_id,
            mode,
            start_sample: current_sample,
            start_beat: current_beat,
            punch_in_sample,
            punch_out_sample,
            events: initial_events,
            input_port: armed_info.input_port,
            channel_filter: armed_info.channel_filter,
        };
        
        self.active_recordings.insert(track_id.clone(), recording);
        
        // Notify recording started
        let _ = self.event_tx.send(RecordingEvent::RecordingStarted {
            track_id,
            timestamp: current_beat,
        });
    }
    
    fn stop_recording(&mut self, track_id: String, commit: bool) {
        if let Some(recording) = self.active_recordings.remove(&track_id) {
            let events_count = recording.events.len();
            
            if commit && events_count > 0 {
                // Send all recorded events
                let _ = self.event_tx.send(RecordingEvent::EventsRecorded {
                    track_id: track_id.clone(),
                    events: recording.events,
                });
            }
            
            // Notify recording stopped
            let _ = self.event_tx.send(RecordingEvent::RecordingStopped {
                track_id,
                events_recorded: events_count,
            });
        }
    }
}