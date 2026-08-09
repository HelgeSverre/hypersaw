use std::collections::{BinaryHeap, HashMap};
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc,
};
use std::thread;
use std::time::{Duration, Instant};

use crossbeam::channel::{bounded, unbounded, Receiver, Sender};
use parking_lot::Mutex;

use crate::core::MidiMessage;

/// Commands from UI to MIDI engine
#[derive(Debug, Clone)]
pub enum MidiEngineCommand {
    Start,
    Stop,
    Continue,
    SetTempo(f64),
    SetPosition(f64), // Position in beats
    ScheduleEvent {
        time_in_beats: f64,
        port_id: String,
        message: MidiMessage,
        track_id: String,
    },
    SetTrackMute(String, bool),
    SetTrackSolo(String, bool),
    SetPortRouting(String, String), // (track_id, port_id)
    ClearPortRouting(String),
    AddOutputPort(String, usize), // (name, midir port number)
    RemoveOutputPort(String),
    AddInputPort(String, usize), // (name, midir port number)
    RemoveInputPort(String),
    SetMetronomeEnabled(bool),
    SetMetronomeVolume(u8),
    SetMetronomeSound {
        accent_note: u8,
        regular_note: u8,
    },
    SetMetronomePort(Option<String>), // Set dedicated metronome output port
}

/// Messages from MIDI engine to UI
#[derive(Debug, Clone)]
pub enum MidiEngineMessage {
    PositionUpdate(f64),                  // Current position in beats
    PortStatusChanged(String, bool),      // (port_id, connected)
    PortConnectionFailed(String, String), // (port_id, error)
    MidiInput(String, MidiMessage, u64),  // (port_id, message, timestamp)
}

/// A scheduled MIDI event
#[derive(Debug, Clone)]
struct ScheduledEvent {
    time_in_samples: u64,
    port_id: String,
    message: MidiMessage,
    track_id: String,
}

impl PartialEq for ScheduledEvent {
    fn eq(&self, other: &Self) -> bool {
        self.time_in_samples == other.time_in_samples
    }
}

impl Eq for ScheduledEvent {}

impl PartialOrd for ScheduledEvent {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ScheduledEvent {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // Reverse order for min-heap behavior
        other.time_in_samples.cmp(&self.time_in_samples)
    }
}

/// MIDI output port wrapper
struct MidiOutputPort {
    connection: Box<dyn MidiOutputConnection>,
}

/// The small boundary between scheduling/routing and the platform MIDI backend.
///
/// Keeping this boundary local lets the engine's delivery behavior be verified
/// without requiring a physical MIDI device.
trait MidiOutputConnection: Send {
    fn send_message(&mut self, message: &MidiMessage) -> Result<(), String>;
}

impl MidiOutputConnection for midir::MidiOutputConnection {
    fn send_message(&mut self, message: &MidiMessage) -> Result<(), String> {
        self.send(&encode_midi_message(message))
            .map_err(|error| error.to_string())
    }
}

/// The main MIDI engine
pub struct MidiEngine {
    // Thread communication
    command_rx: Receiver<MidiEngineCommand>,
    message_tx: Sender<MidiEngineMessage>,

    // Timing
    sample_rate: u32,
    current_sample: AtomicU64,
    tempo: f64,

    // Transport
    is_playing: AtomicBool,

    // Event scheduling
    event_queue: Mutex<BinaryHeap<ScheduledEvent>>,

    // Port management
    output_ports: Mutex<HashMap<String, MidiOutputPort>>,
    input_ports: Mutex<HashMap<String, MidiInputPort>>,
    track_routing: Mutex<HashMap<String, String>>, // track_id -> port_id

    // Track state
    muted_tracks: Mutex<HashMap<String, bool>>,
    soloed_tracks: Mutex<HashMap<String, bool>>,

    // Metronome
    metronome_enabled: AtomicBool,
    metronome_volume: AtomicU64, // Store as u64 for atomic, but use as u8
    metronome_accent_note: AtomicU64, // MIDI note for accent (downbeat)
    metronome_regular_note: AtomicU64, // MIDI note for regular beats
    metronome_port: Mutex<Option<String>>, // Dedicated metronome output port
    time_signature: Mutex<(u32, u32)>, // (numerator, denominator)
    last_metronome_beat: AtomicU64, // Last beat where metronome clicked
    capture_sample_clock: Arc<AtomicU64>,
    shutdown: Arc<AtomicBool>,
}

/// MIDI input port wrapper
struct MidiInputPort {
    _connection: midir::MidiInputConnection<()>, // Keep connection alive
}

impl MidiEngine {
    pub fn new(
        sample_rate: u32,
        command_rx: Receiver<MidiEngineCommand>,
        message_tx: Sender<MidiEngineMessage>,
    ) -> Self {
        Self::new_with_shutdown(
            sample_rate,
            command_rx,
            message_tx,
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicU64::new(0)),
        )
    }

    fn new_with_shutdown(
        sample_rate: u32,
        command_rx: Receiver<MidiEngineCommand>,
        message_tx: Sender<MidiEngineMessage>,
        shutdown: Arc<AtomicBool>,
        capture_sample_clock: Arc<AtomicU64>,
    ) -> Self {
        Self {
            command_rx,
            message_tx,
            sample_rate,
            current_sample: AtomicU64::new(0),
            tempo: 120.0,
            is_playing: AtomicBool::new(false),
            event_queue: Mutex::new(BinaryHeap::new()),
            output_ports: Mutex::new(HashMap::new()),
            input_ports: Mutex::new(HashMap::new()),
            track_routing: Mutex::new(HashMap::new()),
            muted_tracks: Mutex::new(HashMap::new()),
            soloed_tracks: Mutex::new(HashMap::new()),
            metronome_enabled: AtomicBool::new(false),
            metronome_volume: AtomicU64::new(100), // Default volume
            metronome_accent_note: AtomicU64::new(76), // High wood block
            metronome_regular_note: AtomicU64::new(77), // Low wood block
            metronome_port: Mutex::new(None),      // No default metronome port
            time_signature: Mutex::new((4, 4)),    // Default 4/4
            last_metronome_beat: AtomicU64::new(0),
            capture_sample_clock,
            shutdown,
        }
    }

    /// Convert beats to samples based on current tempo
    fn beats_to_samples(&self, beats: f64) -> u64 {
        let seconds = (beats / self.tempo) * 60.0;
        (seconds * self.sample_rate as f64) as u64
    }

    /// Convert samples to beats based on current tempo
    fn samples_to_beats(&self, samples: u64) -> f64 {
        let seconds = samples as f64 / self.sample_rate as f64;
        (seconds / 60.0) * self.tempo
    }

    /// Process incoming commands from UI
    fn process_commands(&mut self) {
        while let Ok(command) = self.command_rx.try_recv() {
            match command {
                MidiEngineCommand::Start => {
                    self.is_playing.store(true, Ordering::SeqCst);
                }
                MidiEngineCommand::Stop => {
                    self.is_playing.store(false, Ordering::SeqCst);
                    self.event_queue.lock().clear();
                    self.send_all_notes_off();
                }
                MidiEngineCommand::SetTempo(tempo) => {
                    self.tempo = tempo;
                }
                MidiEngineCommand::SetPosition(beats) => {
                    self.send_all_notes_off();
                    self.event_queue.lock().clear();
                    let samples = self.beats_to_samples(beats);
                    self.current_sample.store(samples, Ordering::SeqCst);
                }
                MidiEngineCommand::ScheduleEvent {
                    time_in_beats,
                    port_id,
                    message,
                    track_id,
                } => {
                    let event = ScheduledEvent {
                        time_in_samples: self.beats_to_samples(time_in_beats),
                        port_id,
                        message,
                        track_id,
                    };
                    self.event_queue.lock().push(event);
                }
                MidiEngineCommand::SetTrackMute(track_id, muted) => {
                    self.muted_tracks.lock().insert(track_id, muted);
                }
                MidiEngineCommand::SetTrackSolo(track_id, soloed) => {
                    self.soloed_tracks.lock().insert(track_id, soloed);
                }
                MidiEngineCommand::SetPortRouting(track_id, port_id) => {
                    self.track_routing.lock().insert(track_id, port_id);
                }
                MidiEngineCommand::ClearPortRouting(track_id) => {
                    self.track_routing.lock().remove(&track_id);
                }
                MidiEngineCommand::AddOutputPort(name, port_number) => {
                    let result = midir::MidiOutput::new("Hypersaw")
                        .map_err(|error| error.to_string())
                        .and_then(|midi_out| {
                            let ports = midi_out.ports();
                            let port = ports.get(port_number).ok_or_else(|| {
                                format!("MIDI output index {port_number} is no longer available")
                            })?;
                            midi_out
                                .connect(port, &name)
                                .map_err(|error| error.to_string())
                        });
                    match result {
                        Ok(connection) => {
                            self.add_output_connection(name, Box::new(connection));
                        }
                        Err(error) => {
                            let _ = self
                                .message_tx
                                .try_send(MidiEngineMessage::PortConnectionFailed(name, error));
                        }
                    }
                }
                MidiEngineCommand::RemoveOutputPort(name) => {
                    self.output_ports.lock().remove(&name);
                    let _ = self
                        .message_tx
                        .try_send(MidiEngineMessage::PortStatusChanged(name, false));
                }
                MidiEngineCommand::AddInputPort(name, port_number) => {
                    if let Ok(midi_in) = midir::MidiInput::new("Hypersaw") {
                        let ports = midi_in.ports();
                        if port_number < ports.len() {
                            let port_name = name.clone();
                            let message_tx = self.message_tx.clone();
                            let sample_rate = self.sample_rate;
                            let capture_sample_clock = self.capture_sample_clock.clone();
                            let mut timestamp_origin = None;

                            if let Ok(connection) = midi_in.connect(
                                &ports[port_number],
                                &name,
                                move |timestamp, message, _| {
                                    if let Some(msg) = parse_midi_message(message) {
                                        // Align this connection's arbitrary `midir` origin to
                                        // the recorder's shared monotonic sample clock while
                                        // preserving device-provided timing between messages.
                                        let sample_timestamp = align_midir_timestamp(
                                            timestamp,
                                            capture_sample_clock.load(Ordering::Acquire),
                                            sample_rate,
                                            &mut timestamp_origin,
                                        );
                                        let _ = message_tx.try_send(MidiEngineMessage::MidiInput(
                                            port_name.clone(),
                                            msg,
                                            sample_timestamp,
                                        ));
                                    }
                                },
                                (),
                            ) {
                                self.input_ports.lock().insert(
                                    name.clone(),
                                    MidiInputPort {
                                        _connection: connection,
                                    },
                                );
                                let _ = self
                                    .message_tx
                                    .try_send(MidiEngineMessage::PortStatusChanged(name, true));
                            }
                        }
                    }
                }
                MidiEngineCommand::RemoveInputPort(name) => {
                    self.input_ports.lock().remove(&name);
                    let _ = self
                        .message_tx
                        .try_send(MidiEngineMessage::PortStatusChanged(name, false));
                }
                MidiEngineCommand::SetMetronomeEnabled(enabled) => {
                    self.metronome_enabled.store(enabled, Ordering::SeqCst);
                }
                MidiEngineCommand::SetMetronomeVolume(volume) => {
                    self.metronome_volume.store(volume as u64, Ordering::SeqCst);
                }
                MidiEngineCommand::SetMetronomeSound {
                    accent_note,
                    regular_note,
                } => {
                    self.metronome_accent_note
                        .store(accent_note as u64, Ordering::SeqCst);
                    self.metronome_regular_note
                        .store(regular_note as u64, Ordering::SeqCst);
                }
                MidiEngineCommand::SetMetronomePort(port) => {
                    *self.metronome_port.lock() = port;
                }
                _ => {} // Continue is not implemented yet
            }
        }
    }

    /// Process and send scheduled events
    fn process_events(&mut self, current_sample: u64) {
        // Step 1: Collect due events (minimize queue lock time)
        let mut due_events = Vec::new();
        {
            let mut queue = self.event_queue.lock();
            while let Some(event) = queue.peek() {
                if event.time_in_samples > current_sample {
                    break;
                }
                due_events.push(queue.pop().unwrap());
            }
        } // queue lock released

        // Step 2: Filter events by mute/solo (minimize state lock time)
        let (has_soloed, mute_solo_state): (bool, Vec<_>) = {
            let muted = self.muted_tracks.lock();
            let soloed = self.soloed_tracks.lock();
            let has_solo = soloed.values().any(|&s| s);

            let state = due_events
                .iter()
                .map(|event| {
                    if event.track_id == "metronome" {
                        true // always play metronome
                    } else {
                        let is_muted = muted.get(&event.track_id).copied().unwrap_or(false);
                        let is_soloed = soloed.get(&event.track_id).copied().unwrap_or(false);
                        !is_muted && (!has_solo || is_soloed)
                    }
                })
                .collect();
            (has_solo, state)
        }; // muted/soloed locks released

        // Step 3: Resolve routing (minimize routing lock time)
        let port_ids: Vec<_> = {
            let routing = self.track_routing.lock();
            let metronome_port = self.metronome_port.lock();
            let ports_lock = self.output_ports.lock();

            // Use dedicated metronome port, or fall back to first available
            let metro_port = metronome_port
                .as_ref()
                .cloned()
                .or_else(|| ports_lock.keys().next().cloned())
                .unwrap_or_default();

            due_events
                .iter()
                .map(|event| {
                    if event.track_id == "metronome" {
                        metro_port.clone()
                    } else {
                        routing
                            .get(&event.track_id)
                            .cloned()
                            .unwrap_or_else(|| event.port_id.clone())
                    }
                })
                .collect()
        }; // routing lock released

        // Step 4: Send MIDI messages (only lock output_ports when sending)
        for ((event, should_play), port_id) in due_events
            .into_iter()
            .zip(mute_solo_state.into_iter())
            .zip(port_ids.into_iter())
        {
            if !should_play {
                continue;
            }

            let mut ports = self.output_ports.lock();
            if let Some(port) = ports.get_mut(&port_id) {
                if let Err(e) = port.connection.send_message(&event.message) {
                    eprintln!("Failed to send MIDI message: {}", e);
                }
            }
        } // ports lock released per iteration
    }

    fn add_output_connection(&self, name: String, connection: Box<dyn MidiOutputConnection>) {
        self.output_ports
            .lock()
            .insert(name.clone(), MidiOutputPort { connection });
        let _ = self
            .message_tx
            .try_send(MidiEngineMessage::PortStatusChanged(name, true));
    }

    /// Send all notes off to all ports using CC 123 (All Notes Off)
    fn send_all_notes_off(&mut self) {
        let mut ports = self.output_ports.lock();
        for port in ports.values_mut() {
            for channel in 0..16 {
                // Send CC 123 (All Notes Off) - much more efficient than 128 individual NoteOff messages
                let msg = MidiMessage::ControlChange {
                    channel,
                    controller: 123,
                    value: 0,
                };
                let _ = port.connection.send_message(&msg);
            }
        }
    }

    /// Process metronome clicks
    fn process_metronome(&mut self, current_sample: u64, lookahead_samples: u64) {
        if !self.metronome_enabled.load(Ordering::SeqCst) {
            return;
        }

        let current_beat = self.samples_to_beats(current_sample);
        let lookahead_beat = self.samples_to_beats(current_sample + lookahead_samples);
        let last_beat = self.last_metronome_beat.load(Ordering::SeqCst) as f64;

        // Get time signature
        let (numerator, _denominator) = *self.time_signature.lock();

        // Find all beats in the lookahead window
        let start_beat = last_beat.max(current_beat);
        let end_beat = lookahead_beat;

        let mut next_beat = start_beat.ceil();
        while next_beat <= end_beat {
            // Determine if this is an accent beat (downbeat)
            let beat_in_bar = (next_beat as i32) % (numerator as i32);
            let is_accent = beat_in_bar == 0;

            // Get metronome settings
            let volume = self.metronome_volume.load(Ordering::SeqCst) as u8;
            let note = if is_accent {
                self.metronome_accent_note.load(Ordering::SeqCst) as u8
            } else {
                self.metronome_regular_note.load(Ordering::SeqCst) as u8
            };

            // Schedule metronome click
            let click_sample = self.beats_to_samples(next_beat);

            // Use a dedicated metronome port or the first available port
            let event = ScheduledEvent {
                time_in_samples: click_sample,
                port_id: "metronome".to_string(), // Special port ID for metronome
                message: MidiMessage::NoteOn {
                    channel: 9, // Use channel 10 (drums)
                    key: note,
                    velocity: volume,
                },
                track_id: "metronome".to_string(),
            };
            self.event_queue.lock().push(event);

            // Schedule note off
            let off_event = ScheduledEvent {
                time_in_samples: click_sample + (self.sample_rate as u64 / 20), // 50ms duration
                port_id: "metronome".to_string(),
                message: MidiMessage::NoteOff {
                    channel: 9,
                    key: note,
                    velocity: 0,
                },
                track_id: "metronome".to_string(),
            };
            self.event_queue.lock().push(off_event);

            // Update last beat
            self.last_metronome_beat
                .store(next_beat as u64, Ordering::SeqCst);

            next_beat += 1.0;
        }
    }

    /// Main engine loop
    pub fn run(mut self) {
        let period_ns = 1_000_000; // 1ms period for ~1000Hz update rate
        let lookahead_samples = (self.sample_rate as f64 * 0.005) as u64; // 5ms lookahead

        let mut last_time = Instant::now();
        let mut last_position_update = Instant::now();
        let capture_clock_origin = Instant::now();

        while !self.shutdown.load(Ordering::Acquire) {
            let start = Instant::now();

            // Process commands from UI
            self.process_commands();

            // The capture clock advances even while transport is stopped, so
            // recording silence and punch windows have a stable origin.
            let now = Instant::now();
            let elapsed = now.duration_since(last_time);
            let elapsed_samples = (elapsed.as_secs_f64() * self.sample_rate as f64) as u64;
            self.capture_sample_clock.store(
                (capture_clock_origin.elapsed().as_secs_f64() * self.sample_rate as f64) as u64,
                Ordering::Release,
            );

            // Update transport timing
            if self.is_playing.load(Ordering::SeqCst) {
                let current = self
                    .current_sample
                    .fetch_add(elapsed_samples, Ordering::SeqCst)
                    .saturating_add(elapsed_samples);

                // Process metronome
                self.process_metronome(current, lookahead_samples);

                // Process events
                self.process_events(current);

                if last_position_update.elapsed() >= Duration::from_millis(16) {
                    let beats = self.samples_to_beats(current);
                    let _ = self
                        .message_tx
                        .try_send(MidiEngineMessage::PositionUpdate(beats));
                    last_position_update = now;
                }
            }
            last_time = now;

            // Sleep for remainder of period
            let elapsed = start.elapsed();
            if elapsed < Duration::from_nanos(period_ns) {
                thread::sleep(Duration::from_nanos(period_ns) - elapsed);
            }
        }
    }
}

/// Handle for communicating with the MIDI engine
pub struct MidiEngineHandle {
    command_tx: Sender<MidiEngineCommand>,
    message_rx: Receiver<MidiEngineMessage>,
    thread: Option<thread::JoinHandle<()>>,
    shutdown: Arc<AtomicBool>,
}

impl MidiEngineHandle {
    /// Start a new MIDI engine
    pub fn start(sample_rate: u32, capture_sample_clock: Arc<AtomicU64>) -> Self {
        // Use bounded channels to prevent unbounded memory growth
        // Command buffer: up to 1000 pending commands
        let (command_tx, command_rx) = bounded(1000);
        // Message buffer: up to 500 pending messages
        let (message_tx, message_rx) = bounded(500);

        let shutdown = Arc::new(AtomicBool::new(false));
        let engine = MidiEngine::new_with_shutdown(
            sample_rate,
            command_rx,
            message_tx,
            shutdown.clone(),
            capture_sample_clock,
        );

        let thread = thread::Builder::new()
            .name("midi_engine".to_string())
            .spawn(move || {
                // TODO: Set thread priority to high/realtime
                engine.run();
            })
            .expect("Failed to spawn MIDI engine thread");

        Self {
            command_tx,
            message_rx,
            thread: Some(thread),
            shutdown,
        }
    }

    /// Send a command to the engine
    pub fn send_command(&self, command: MidiEngineCommand) {
        let _ = self.command_tx.send(command);
    }

    /// Try to receive a message from the engine
    pub fn try_recv_message(&self) -> Option<MidiEngineMessage> {
        self.message_rx.try_recv().ok()
    }

    /// Scan available MIDI output ports
    pub fn scan_midi_output_ports() -> Vec<(String, usize)> {
        let mut ports = Vec::new();

        if let Ok(midi_out) = midir::MidiOutput::new("Hypersaw Scanner") {
            let midi_ports = midi_out.ports();
            for (i, port) in midi_ports.iter().enumerate() {
                if let Ok(name) = midi_out.port_name(port) {
                    ports.push((name, i));
                }
            }
        }

        ports
    }

    /// Scan available MIDI input ports
    pub fn scan_midi_input_ports() -> Vec<(String, usize)> {
        let mut ports = Vec::new();

        if let Ok(midi_in) = midir::MidiInput::new("Hypersaw Scanner") {
            let midi_ports = midi_in.ports();
            for (i, port) in midi_ports.iter().enumerate() {
                if let Ok(name) = midi_in.port_name(port) {
                    ports.push((name, i));
                }
            }
        }

        ports
    }
}

impl Drop for MidiEngineHandle {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Converts a complete MIDI wire message into the application's message model.
///
/// Messages with invalid lengths, data bytes, or unsupported status bytes are ignored. MIDI
/// input callbacks deliver complete messages, so this intentionally does not implement running
/// status or assemble fragmented SysEx packets.
fn parse_midi_message(bytes: &[u8]) -> Option<MidiMessage> {
    let (&status, data) = bytes.split_first()?;

    match status {
        0xF0 => parse_sysex(data),
        0xF8 if data.is_empty() => Some(MidiMessage::MidiClock),
        0xFA if data.is_empty() => Some(MidiMessage::MidiStart),
        0xFC if data.is_empty() => Some(MidiMessage::MidiStop),
        0xFB if data.is_empty() => Some(MidiMessage::MidiContinue),
        status if status < 0xF0 => parse_channel_message(status, data),
        _ => None,
    }
}

fn parse_channel_message(status: u8, data: &[u8]) -> Option<MidiMessage> {
    let channel = status & 0x0F;
    let message_type = status & 0xF0;

    match message_type {
        0x80 if valid_data_bytes(data, 2) => Some(MidiMessage::NoteOff {
            channel,
            key: data[0],
            velocity: data[1],
        }),
        0x90 if valid_data_bytes(data, 2) => {
            let key = data[0];
            let velocity = data[1];
            if velocity == 0 {
                Some(MidiMessage::NoteOff {
                    channel,
                    key,
                    velocity,
                })
            } else {
                Some(MidiMessage::NoteOn {
                    channel,
                    key,
                    velocity,
                })
            }
        }
        0xA0 if valid_data_bytes(data, 2) => Some(MidiMessage::Aftertouch {
            channel,
            key: data[0],
            pressure: data[1],
        }),
        0xB0 if valid_data_bytes(data, 2) => Some(MidiMessage::ControlChange {
            channel,
            controller: data[0],
            value: data[1],
        }),
        0xC0 if valid_data_bytes(data, 1) => Some(MidiMessage::ProgramChange {
            channel,
            program: data[0],
        }),
        0xE0 if valid_data_bytes(data, 2) => {
            let raw_value = u16::from(data[0]) | (u16::from(data[1]) << 7);
            Some(MidiMessage::PitchBend {
                channel,
                value: raw_value as i16 - 8_192,
            })
        }
        _ => None,
    }
}

fn valid_data_bytes(data: &[u8], expected_len: usize) -> bool {
    data.len() == expected_len && data.iter().all(|byte| *byte <= 0x7F)
}

fn parse_sysex(data: &[u8]) -> Option<MidiMessage> {
    let (end, payload) = data.split_last()?;
    if *end != 0xF7 || payload.iter().any(|byte| *byte > 0x7F) {
        return None;
    }

    Some(MidiMessage::SysEx(payload.to_vec()))
}

/// Converts an application MIDI message into a complete wire message.
///
/// The model uses wider unsigned fields than the MIDI wire format. Values outside the wire range
/// are truncated to their low 7 bits, channels to their low 4 bits, and pitch bend is clamped to
/// its signed 14-bit range before being re-centered for transmission.
fn encode_midi_message(message: &MidiMessage) -> Vec<u8> {
    match message {
        MidiMessage::NoteOn {
            channel,
            key,
            velocity,
        } => vec![
            channel_status(0x90, *channel),
            data_byte(*key),
            data_byte(*velocity),
        ],
        MidiMessage::NoteOff {
            channel,
            key,
            velocity,
        } => vec![
            channel_status(0x80, *channel),
            data_byte(*key),
            data_byte(*velocity),
        ],
        MidiMessage::ControlChange {
            channel,
            controller,
            value,
        } => vec![
            channel_status(0xB0, *channel),
            data_byte(*controller),
            data_byte(*value),
        ],
        MidiMessage::ProgramChange { channel, program } => {
            vec![channel_status(0xC0, *channel), data_byte(*program)]
        }
        MidiMessage::PitchBend { channel, value } => {
            let raw_value = (i32::from(*value).clamp(-8_192, 8_191) + 8_192) as u16;
            vec![
                channel_status(0xE0, *channel),
                (raw_value & 0x7F) as u8,
                ((raw_value >> 7) & 0x7F) as u8,
            ]
        }
        MidiMessage::Aftertouch {
            channel,
            key,
            pressure,
        } => vec![
            channel_status(0xA0, *channel),
            data_byte(*key),
            data_byte(*pressure),
        ],
        MidiMessage::SysEx(data) => {
            let mut bytes = Vec::with_capacity(data.len() + 2);
            bytes.push(0xF0);
            bytes.extend(data.iter().map(|byte| data_byte(*byte)));
            bytes.push(0xF7);
            bytes
        }
        MidiMessage::MidiClock => vec![0xF8],
        MidiMessage::MidiStart => vec![0xFA],
        MidiMessage::MidiStop => vec![0xFC],
        MidiMessage::MidiContinue => vec![0xFB],
    }
}

fn channel_status(message_type: u8, channel: u8) -> u8 {
    message_type | (channel & 0x0F)
}

fn data_byte(value: u8) -> u8 {
    value & 0x7F
}

fn midir_timestamp_to_samples(timestamp_microseconds: u64, sample_rate: u32) -> u64 {
    let sample_rate = u64::from(sample_rate);
    (timestamp_microseconds / 1_000_000) * sample_rate
        + (timestamp_microseconds % 1_000_000) * sample_rate / 1_000_000
}

fn align_midir_timestamp(
    timestamp_microseconds: u64,
    capture_sample_now: u64,
    sample_rate: u32,
    origin: &mut Option<(u64, u64)>,
) -> u64 {
    let (midir_origin, sample_origin) =
        *origin.get_or_insert((timestamp_microseconds, capture_sample_now));
    sample_origin.saturating_add(midir_timestamp_to_samples(
        timestamp_microseconds.saturating_sub(midir_origin),
        sample_rate,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Default)]
    struct RecordingMidiOutput {
        messages: Arc<Mutex<Vec<MidiMessage>>>,
    }

    impl RecordingMidiOutput {
        fn messages(&self) -> Vec<MidiMessage> {
            self.messages.lock().clone()
        }
    }

    impl MidiOutputConnection for RecordingMidiOutput {
        fn send_message(&mut self, message: &MidiMessage) -> Result<(), String> {
            self.messages.lock().push(message.clone());
            Ok(())
        }
    }

    fn engine_with_sender() -> (MidiEngine, Sender<MidiEngineCommand>) {
        let (command_tx, command_rx) = unbounded();
        let (message_tx, _message_rx) = unbounded();
        (MidiEngine::new(48_000, command_rx, message_tx), command_tx)
    }

    fn engine_with_channels() -> (
        MidiEngine,
        Sender<MidiEngineCommand>,
        Receiver<MidiEngineMessage>,
    ) {
        let (command_tx, command_rx) = unbounded();
        let (message_tx, message_rx) = unbounded();
        (
            MidiEngine::new(48_000, command_rx, message_tx),
            command_tx,
            message_rx,
        )
    }

    fn schedule_note(
        command_tx: &Sender<MidiEngineCommand>,
        track_id: &str,
        port_id: &str,
        key: u8,
    ) {
        command_tx
            .send(MidiEngineCommand::ScheduleEvent {
                time_in_beats: 0.0,
                port_id: port_id.to_string(),
                message: MidiMessage::NoteOn {
                    channel: 0,
                    key,
                    velocity: 100,
                },
                track_id: track_id.to_string(),
            })
            .unwrap();
    }

    #[test]
    fn disconnected_port_drops_events_and_reconnect_uses_existing_track_routing() {
        let (mut engine, command_tx, message_rx) = engine_with_channels();
        let first_connection = RecordingMidiOutput::default();
        engine.add_output_connection("device".to_string(), Box::new(first_connection.clone()));

        command_tx
            .send(MidiEngineCommand::SetPortRouting(
                "track".to_string(),
                "device".to_string(),
            ))
            .unwrap();
        schedule_note(&command_tx, "track", "unused", 60);
        engine.process_commands();
        engine.process_events(0);
        assert_eq!(first_connection.messages().len(), 1);

        command_tx
            .send(MidiEngineCommand::RemoveOutputPort("device".to_string()))
            .unwrap();
        schedule_note(&command_tx, "track", "unused", 61);
        engine.process_commands();
        engine.process_events(0);
        assert_eq!(first_connection.messages().len(), 1);

        let reconnected = RecordingMidiOutput::default();
        engine.add_output_connection("device".to_string(), Box::new(reconnected.clone()));
        schedule_note(&command_tx, "track", "unused", 62);
        engine.process_commands();
        engine.process_events(0);

        assert_eq!(
            reconnected.messages(),
            [MidiMessage::NoteOn {
                channel: 0,
                key: 62,
                velocity: 100,
            }]
        );
        let statuses: Vec<_> = message_rx
            .try_iter()
            .filter_map(|message| match message {
                MidiEngineMessage::PortStatusChanged(name, connected) => Some((name, connected)),
                _ => None,
            })
            .collect();
        assert_eq!(
            statuses,
            [
                ("device".to_string(), true),
                ("device".to_string(), false),
                ("device".to_string(), true),
            ]
        );
    }

    #[test]
    fn track_routing_targets_the_configured_output_and_can_be_cleared() {
        let (mut engine, command_tx, _message_rx) = engine_with_channels();
        let scheduled_port = RecordingMidiOutput::default();
        let routed_port = RecordingMidiOutput::default();
        engine.add_output_connection("scheduled".to_string(), Box::new(scheduled_port.clone()));
        engine.add_output_connection("routed".to_string(), Box::new(routed_port.clone()));

        command_tx
            .send(MidiEngineCommand::SetPortRouting(
                "track".to_string(),
                "routed".to_string(),
            ))
            .unwrap();
        schedule_note(&command_tx, "track", "scheduled", 60);
        engine.process_commands();
        engine.process_events(0);
        assert!(scheduled_port.messages().is_empty());
        assert_eq!(routed_port.messages().len(), 1);

        command_tx
            .send(MidiEngineCommand::ClearPortRouting("track".to_string()))
            .unwrap();
        schedule_note(&command_tx, "track", "scheduled", 61);
        engine.process_commands();
        engine.process_events(0);
        assert_eq!(scheduled_port.messages().len(), 1);
        assert_eq!(routed_port.messages().len(), 1);
    }

    #[test]
    fn muting_a_track_prevents_delivery_until_it_is_unmuted() {
        let (mut engine, command_tx, _message_rx) = engine_with_channels();
        let output = RecordingMidiOutput::default();
        engine.add_output_connection("port".to_string(), Box::new(output.clone()));

        command_tx
            .send(MidiEngineCommand::SetTrackMute("track".to_string(), true))
            .unwrap();
        schedule_note(&command_tx, "track", "port", 60);
        engine.process_commands();
        engine.process_events(0);
        assert!(output.messages().is_empty());

        command_tx
            .send(MidiEngineCommand::SetTrackMute("track".to_string(), false))
            .unwrap();
        schedule_note(&command_tx, "track", "port", 61);
        engine.process_commands();
        engine.process_events(0);
        assert_eq!(output.messages().len(), 1);
    }

    #[test]
    fn soloing_a_track_filters_other_tracks_and_respects_mute() {
        let (mut engine, command_tx, _message_rx) = engine_with_channels();
        let output = RecordingMidiOutput::default();
        engine.add_output_connection("port".to_string(), Box::new(output.clone()));

        command_tx
            .send(MidiEngineCommand::SetTrackSolo("solo".to_string(), true))
            .unwrap();
        schedule_note(&command_tx, "other", "port", 60);
        schedule_note(&command_tx, "solo", "port", 61);
        engine.process_commands();
        engine.process_events(0);
        assert_eq!(
            output.messages(),
            [MidiMessage::NoteOn {
                channel: 0,
                key: 61,
                velocity: 100,
            }]
        );

        command_tx
            .send(MidiEngineCommand::SetTrackMute("solo".to_string(), true))
            .unwrap();
        schedule_note(&command_tx, "solo", "port", 62);
        engine.process_commands();
        engine.process_events(0);
        assert_eq!(output.messages().len(), 1);
    }

    #[test]
    fn stop_discards_scheduled_events() {
        let (mut engine, command_tx) = engine_with_sender();
        command_tx
            .send(MidiEngineCommand::ScheduleEvent {
                time_in_beats: 4.0,
                port_id: "port".to_string(),
                message: MidiMessage::MidiClock,
                track_id: "track".to_string(),
            })
            .unwrap();
        command_tx.send(MidiEngineCommand::Stop).unwrap();
        engine.process_commands();
        assert!(engine.event_queue.lock().is_empty());
    }

    #[test]
    fn seeking_discards_scheduled_events() {
        let (mut engine, command_tx) = engine_with_sender();
        command_tx
            .send(MidiEngineCommand::ScheduleEvent {
                time_in_beats: 4.0,
                port_id: "port".to_string(),
                message: MidiMessage::MidiClock,
                track_id: "track".to_string(),
            })
            .unwrap();
        command_tx
            .send(MidiEngineCommand::SetPosition(2.0))
            .unwrap();
        engine.process_commands();
        assert!(engine.event_queue.lock().is_empty());
        assert_eq!(engine.current_sample.load(Ordering::SeqCst), 48_000);
    }

    #[test]
    fn scheduled_events_are_not_dispatched_early() {
        let (mut engine, command_tx) = engine_with_sender();
        command_tx
            .send(MidiEngineCommand::ScheduleEvent {
                time_in_beats: 1.0,
                port_id: "port".to_string(),
                message: MidiMessage::MidiClock,
                track_id: "track".to_string(),
            })
            .unwrap();
        engine.process_commands();

        engine.process_events(23_999);
        assert_eq!(engine.event_queue.lock().len(), 1);

        engine.process_events(24_000);
        assert!(engine.event_queue.lock().is_empty());
    }

    #[test]
    fn midir_microseconds_are_converted_to_samples() {
        assert_eq!(midir_timestamp_to_samples(1_000_000, 48_000), 48_000);
        assert_eq!(midir_timestamp_to_samples(500_000, 48_000), 24_000);
    }

    #[test]
    fn each_midir_connection_is_aligned_to_the_shared_capture_clock() {
        let mut origin = None;
        assert_eq!(
            align_midir_timestamp(5_000_000, 10_000, 48_000, &mut origin),
            10_000
        );
        assert_eq!(
            align_midir_timestamp(5_500_000, 99_999, 48_000, &mut origin),
            34_000
        );
    }

    #[test]
    fn pitch_bend_round_trips_at_its_full_signed_range() {
        for value in [-8_192, 0, 8_191] {
            let message = MidiMessage::PitchBend { channel: 12, value };
            let bytes = encode_midi_message(&message);

            assert_eq!(bytes[0], 0xEC);
            assert_eq!(parse_midi_message(&bytes), Some(message));
        }

        assert_eq!(
            encode_midi_message(&MidiMessage::PitchBend {
                channel: 0,
                value: -8_192,
            }),
            [0xE0, 0, 0]
        );
        assert_eq!(
            encode_midi_message(&MidiMessage::PitchBend {
                channel: 0,
                value: 0,
            }),
            [0xE0, 0, 64]
        );
        assert_eq!(
            encode_midi_message(&MidiMessage::PitchBend {
                channel: 0,
                value: 8_191,
            }),
            [0xE0, 127, 127]
        );
    }

    #[test]
    fn parses_and_encodes_each_supported_channel_message() {
        let cases = [
            (
                [0x92, 60, 100].as_slice(),
                MidiMessage::NoteOn {
                    channel: 2,
                    key: 60,
                    velocity: 100,
                },
            ),
            (
                [0x82, 60, 64].as_slice(),
                MidiMessage::NoteOff {
                    channel: 2,
                    key: 60,
                    velocity: 64,
                },
            ),
            (
                [0xA2, 60, 72].as_slice(),
                MidiMessage::Aftertouch {
                    channel: 2,
                    key: 60,
                    pressure: 72,
                },
            ),
            (
                [0xB2, 74, 99].as_slice(),
                MidiMessage::ControlChange {
                    channel: 2,
                    controller: 74,
                    value: 99,
                },
            ),
            (
                [0xC2, 10].as_slice(),
                MidiMessage::ProgramChange {
                    channel: 2,
                    program: 10,
                },
            ),
        ];

        for (bytes, message) in cases {
            assert_eq!(parse_midi_message(bytes), Some(message.clone()));
            assert_eq!(encode_midi_message(&message), bytes);
        }
    }

    #[test]
    fn zero_velocity_note_on_is_parsed_as_note_off() {
        assert_eq!(
            parse_midi_message(&[0x9A, 60, 0]),
            Some(MidiMessage::NoteOff {
                channel: 10,
                key: 60,
                velocity: 0,
            })
        );
    }

    #[test]
    fn parses_and_encodes_sysex_and_realtime_messages() {
        let sysex = MidiMessage::SysEx(vec![0x7D, 1, 2, 3]);
        assert_eq!(encode_midi_message(&sysex), [0xF0, 0x7D, 1, 2, 3, 0xF7]);
        assert_eq!(
            parse_midi_message(&[0xF0, 0x7D, 1, 2, 3, 0xF7]),
            Some(sysex)
        );

        for (status, message) in [
            (0xF8, MidiMessage::MidiClock),
            (0xFA, MidiMessage::MidiStart),
            (0xFC, MidiMessage::MidiStop),
            (0xFB, MidiMessage::MidiContinue),
        ] {
            assert_eq!(parse_midi_message(&[status]), Some(message.clone()));
            assert_eq!(encode_midi_message(&message), [status]);
        }
    }

    #[test]
    fn rejects_malformed_midi_input_without_panicking() {
        for bytes in [
            [].as_slice(),
            [0x90, 60].as_slice(),
            [0xC0].as_slice(),
            [0xB0, 1, 0x80].as_slice(),
            [0xF8, 0].as_slice(),
            [0xF0, 1, 2].as_slice(),
            [0xF0, 1, 0xF8, 0xF7].as_slice(),
            [0xF7].as_slice(),
        ] {
            assert_eq!(parse_midi_message(bytes), None, "{bytes:?}");
        }
    }

    #[test]
    fn output_truncates_non_wire_values_and_clamps_pitch_bend() {
        assert_eq!(
            encode_midi_message(&MidiMessage::NoteOn {
                channel: 31,
                key: 200,
                velocity: 255,
            }),
            [0x9F, 72, 127]
        );
        assert_eq!(
            encode_midi_message(&MidiMessage::PitchBend {
                channel: 0,
                value: 20_000,
            }),
            [0xE0, 127, 127]
        );
    }
}
