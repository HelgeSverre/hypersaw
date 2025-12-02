use std::sync::{Arc, atomic::{AtomicBool, AtomicU64, Ordering}};
use std::thread;
use std::time::{Duration, Instant};
use crossbeam::channel::{Sender, Receiver, bounded, unbounded};
use parking_lot::Mutex;
use std::collections::{HashMap, BinaryHeap};
use crate::core::{MidiMessage, MidiEvent};

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
    AddOutputPort(String, usize), // (name, midir port number)
    RemoveOutputPort(String),
    AddInputPort(String, usize), // (name, midir port number)
    RemoveInputPort(String),
    SetMetronomeEnabled(bool),
    SetMetronomeVolume(u8),
    SetMetronomeSound { accent_note: u8, regular_note: u8 },
    SetMetronomePort(Option<String>), // Set dedicated metronome output port
}

/// Messages from MIDI engine to UI
#[derive(Debug, Clone)]
pub enum MidiEngineMessage {
    PositionUpdate(f64), // Current position in beats
    PortStatusChanged(String, bool), // (port_id, connected)
    MidiInput(String, MidiMessage, u64), // (port_id, message, timestamp)
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
        // Reverse order for min-heap behavior (earliest events first)
        other.time_in_samples.partial_cmp(&self.time_in_samples)
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
    name: String,
    connection: midir::MidiOutputConnection,
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
}

/// MIDI input port wrapper
struct MidiInputPort {
    name: String,
    _connection: midir::MidiInputConnection<()>, // Keep connection alive
}

impl MidiEngine {
    pub fn new(
        sample_rate: u32,
        command_rx: Receiver<MidiEngineCommand>,
        message_tx: Sender<MidiEngineMessage>,
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
            metronome_port: Mutex::new(None), // No default metronome port
            time_signature: Mutex::new((4, 4)), // Default 4/4
            last_metronome_beat: AtomicU64::new(0),
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
                    self.send_all_notes_off();
                }
                MidiEngineCommand::SetTempo(tempo) => {
                    self.tempo = tempo;
                }
                MidiEngineCommand::SetPosition(beats) => {
                    let samples = self.beats_to_samples(beats);
                    self.current_sample.store(samples, Ordering::SeqCst);
                }
                MidiEngineCommand::ScheduleEvent { time_in_beats, port_id, message, track_id } => {
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
                MidiEngineCommand::AddOutputPort(name, port_number) => {
                    if let Ok(midi_out) = midir::MidiOutput::new("Hypersaw") {
                        let ports = midi_out.ports();
                        if port_number < ports.len() {
                            if let Ok(connection) = midi_out.connect(&ports[port_number], &name) {
                                self.output_ports.lock().insert(name.clone(), MidiOutputPort {
                                    name: name.clone(),
                                    connection,
                                });
                                let _ = self.message_tx.send(MidiEngineMessage::PortStatusChanged(name, true));
                            }
                        }
                    }
                }
                MidiEngineCommand::RemoveOutputPort(name) => {
                    self.output_ports.lock().remove(&name);
                    let _ = self.message_tx.send(MidiEngineMessage::PortStatusChanged(name, false));
                }
                MidiEngineCommand::AddInputPort(name, port_number) => {
                    if let Ok(midi_in) = midir::MidiInput::new("Hypersaw") {
                        let ports = midi_in.ports();
                        if port_number < ports.len() {
                            let port_name = name.clone();
                            let message_tx = self.message_tx.clone();
                            let sample_rate = self.sample_rate;
                            
                            if let Ok(connection) = midi_in.connect(
                                &ports[port_number],
                                &name,
                                move |timestamp, message, _| {
                                    // Parse MIDI message
                                    if message.len() >= 2 {
                                        let status = message[0];
                                        let channel = status & 0x0F;
                                        let msg_type = status & 0xF0;
                                        
                                        let midi_msg = match msg_type {
                                            0x80 => { // Note off
                                                if message.len() >= 3 {
                                                    Some(MidiMessage::NoteOff {
                                                        channel,
                                                        key: message[1],
                                                        velocity: message[2],
                                                    })
                                                } else { None }
                                            }
                                            0x90 => { // Note on
                                                if message.len() >= 3 {
                                                    // NoteOn with velocity 0 should be treated as NoteOff
                                                    if message[2] == 0 {
                                                        Some(MidiMessage::NoteOff {
                                                            channel,
                                                            key: message[1],
                                                            velocity: 0,
                                                        })
                                                    } else {
                                                        Some(MidiMessage::NoteOn {
                                                            channel,
                                                            key: message[1],
                                                            velocity: message[2],
                                                        })
                                                    }
                                                } else { None }
                                            }
                                            0xB0 => { // Control change
                                                if message.len() >= 3 {
                                                    Some(MidiMessage::ControlChange {
                                                        channel,
                                                        controller: message[1],
                                                        value: message[2],
                                                    })
                                                } else { None }
                                            }
                                            0xE0 => { // Pitch bend
                                                if message.len() >= 3 {
                                                    let value = ((message[2] as u16) << 7) | (message[1] as u16);
                                                    Some(MidiMessage::PitchBend {
                                                        channel,
                                                        value: value as i16,
                                                    })
                                                } else { None }
                                            }
                                            _ => None,
                                        };
                                        
                                        if let Some(msg) = midi_msg {
                                            // Send to UI with timestamp
                                            let sample_timestamp = ((timestamp as f64 / 1000.0) * sample_rate as f64) as u64;
                                            let _ = message_tx.send(MidiEngineMessage::MidiInput(
                                                port_name.clone(),
                                                msg,
                                                sample_timestamp
                                            ));
                                        }
                                    }
                                },
                                (),
                            ) {
                                self.input_ports.lock().insert(name.clone(), MidiInputPort {
                                    name: name.clone(),
                                    _connection: connection,
                                });
                                let _ = self.message_tx.send(MidiEngineMessage::PortStatusChanged(name, true));
                            }
                        }
                    }
                }
                MidiEngineCommand::RemoveInputPort(name) => {
                    self.input_ports.lock().remove(&name);
                    let _ = self.message_tx.send(MidiEngineMessage::PortStatusChanged(name, false));
                }
                MidiEngineCommand::SetMetronomeEnabled(enabled) => {
                    self.metronome_enabled.store(enabled, Ordering::SeqCst);
                }
                MidiEngineCommand::SetMetronomeVolume(volume) => {
                    self.metronome_volume.store(volume as u64, Ordering::SeqCst);
                }
                MidiEngineCommand::SetMetronomeSound { accent_note, regular_note } => {
                    self.metronome_accent_note.store(accent_note as u64, Ordering::SeqCst);
                    self.metronome_regular_note.store(regular_note as u64, Ordering::SeqCst);
                }
                MidiEngineCommand::SetMetronomePort(port) => {
                    *self.metronome_port.lock() = port;
                }
                _ => {} // Continue is not implemented yet
            }
        }
    }
    
    /// Process and send scheduled events
    fn process_events(&mut self, current_sample: u64, lookahead_samples: u64) {
        // Step 1: Collect due events (minimize queue lock time)
        let mut due_events = Vec::new();
        {
            let mut queue = self.event_queue.lock();
            while let Some(event) = queue.peek() {
                if event.time_in_samples > current_sample + lookahead_samples {
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
            
            let state = due_events.iter().map(|event| {
                if event.track_id == "metronome" {
                    true // always play metronome
                } else {
                    let is_muted = muted.get(&event.track_id).copied().unwrap_or(false);
                    let is_soloed = soloed.get(&event.track_id).copied().unwrap_or(false);
                    !is_muted && (!has_solo || is_soloed)
                }
            }).collect();
            (has_solo, state)
        }; // muted/soloed locks released
        
        // Step 3: Resolve routing (minimize routing lock time)
        let port_ids: Vec<_> = {
            let routing = self.track_routing.lock();
            let metronome_port = self.metronome_port.lock();
            let ports_lock = self.output_ports.lock();
            
            // Use dedicated metronome port, or fall back to first available
            let metro_port = metronome_port.as_ref()
                .cloned()
                .or_else(|| ports_lock.keys().next().cloned())
                .unwrap_or_default();
            
            due_events.iter().map(|event| {
                if event.track_id == "metronome" {
                    metro_port.clone()
                } else {
                    routing.get(&event.track_id)
                        .cloned()
                        .unwrap_or_else(|| event.port_id.clone())
                }
            }).collect()
        }; // routing lock released
        
        // Step 4: Send MIDI messages (only lock output_ports when sending)
        for ((event, should_play), port_id) in due_events.into_iter().zip(mute_solo_state.into_iter()).zip(port_ids.into_iter()) {
            if !should_play {
                continue;
            }
            
            let mut ports = self.output_ports.lock();
            if let Some(port) = ports.get_mut(&port_id) {
                if let Err(e) = Self::send_midi_message(&mut port.connection, &event.message) {
                    eprintln!("Failed to send MIDI message: {}", e);
                }
            }
        } // ports lock released per iteration
    }
    
    /// Send a MIDI message to a port
    fn send_midi_message(
        conn: &mut midir::MidiOutputConnection,
        message: &MidiMessage,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let bytes = match message {
            MidiMessage::NoteOn { channel, key, velocity } => {
                vec![0x90 | channel, *key, *velocity]
            }
            MidiMessage::NoteOff { channel, key, velocity } => {
                vec![0x80 | channel, *key, *velocity]
            }
            MidiMessage::ControlChange { channel, controller, value } => {
                vec![0xB0 | channel, *controller, *value]
            }
            MidiMessage::ProgramChange { channel, program } => {
                vec![0xC0 | channel, *program]
            }
            MidiMessage::PitchBend { channel, value } => {
                let value = *value as u16;
                vec![0xE0 | channel, (value & 0x7F) as u8, ((value >> 7) & 0x7F) as u8]
            }
            _ => return Ok(()), // TODO: Implement other message types
        };
        
        conn.send(&bytes)?;
        Ok(())
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
                let _ = Self::send_midi_message(&mut port.connection, &msg);
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
            self.last_metronome_beat.store(next_beat as u64, Ordering::SeqCst);
            
            next_beat += 1.0;
        }
    }
    
    /// Main engine loop
    pub fn run(mut self) {
        let period_ns = 1_000_000; // 1ms period for ~1000Hz update rate
        let lookahead_samples = (self.sample_rate as f64 * 0.005) as u64; // 5ms lookahead
        
        let mut last_time = Instant::now();
        
        loop {
            let start = Instant::now();
            
            // Process commands from UI
            self.process_commands();
            
            // Update timing
            if self.is_playing.load(Ordering::SeqCst) {
                let now = Instant::now();
                let elapsed = now.duration_since(last_time);
                let elapsed_samples = (elapsed.as_secs_f64() * self.sample_rate as f64) as u64;
                
                let current = self.current_sample.fetch_add(elapsed_samples, Ordering::SeqCst);
                
                // Process metronome
                self.process_metronome(current, lookahead_samples);
                
                // Process events
                self.process_events(current, lookahead_samples);
                
                // Send position update
                let beats = self.samples_to_beats(current);
                let _ = self.message_tx.send(MidiEngineMessage::PositionUpdate(beats));
                
                last_time = now;
            } else {
                last_time = Instant::now();
            }
            
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
}

impl MidiEngineHandle {
    /// Start a new MIDI engine
    pub fn start(sample_rate: u32) -> Self {
        // Use bounded channels to prevent unbounded memory growth
        // Command buffer: up to 1000 pending commands
        let (command_tx, command_rx) = bounded(1000);
        // Message buffer: up to 500 pending messages
        let (message_tx, message_rx) = bounded(500);
        
        let engine = MidiEngine::new(sample_rate, command_rx, message_tx);
        
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