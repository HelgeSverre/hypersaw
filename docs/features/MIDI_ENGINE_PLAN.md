# MIDI Engine Design for Hypersaw

## Overview
A rock-solid MIDI sequencer engine focused on hardware gear integration with sample-accurate timing and multi-port support.

## Core Requirements
1. **Tight MIDI timing** - Sample-accurate scheduling for hardware gear
2. **Multi-port I/O** - Multiple MIDI interfaces simultaneously
3. **MIDI recording** - Capture performances from hardware
4. **Low latency** - Minimal delay for live performance
5. **MIDI clock sync** - Master clock output for hardware sync

## Architecture

### 1. MIDI Engine Thread
- Dedicated high-priority thread for MIDI I/O
- Separate from UI thread
- Real-time safe (no allocations, no blocking)
- Uses lock-free queues for communication

### 2. Event Scheduler
```rust
struct MidiScheduler {
    // Priority queue of scheduled events
    event_queue: BinaryHeap<ScheduledEvent>,
    // Current transport position in samples
    current_sample: u64,
    // Sample rate for timing calculations
    sample_rate: u32,
    // Lookahead buffer in samples
    lookahead_samples: u32,
}

struct ScheduledEvent {
    time_in_samples: u64,
    port_id: MidiPortId,
    message: MidiMessage,
    track_id: TrackId,
}
```

### 3. Port Management
```rust
struct MidiPortManager {
    // Multiple output ports
    output_ports: HashMap<MidiPortId, MidiOutputConnection>,
    // Multiple input ports for recording
    input_ports: HashMap<MidiPortId, MidiInputConnection>,
    // Port assignments per track
    track_routing: HashMap<TrackId, MidiPortId>,
}
```

### 4. MIDI Clock Generator
```rust
struct MidiClock {
    // PPQ (Pulses Per Quarter note) - typically 24
    ppq: u32,
    // Last clock pulse time
    last_pulse_sample: u64,
    // Current tempo in BPM
    tempo: f64,
    // Enabled output ports for clock
    clock_outputs: Vec<MidiPortId>,
}
```

### 5. Recording System
```rust
struct MidiRecorder {
    // Currently recording tracks
    recording_tracks: HashSet<TrackId>,
    // Buffer for incoming events
    input_buffer: RingBuffer<RecordedEvent>,
    // Quantization settings
    quantize_on_record: bool,
    quantize_strength: f32,
}
```

## Implementation Plan

### Phase 1: Core Engine
1. Create dedicated MIDI thread with high priority
2. Implement lock-free communication between UI and MIDI thread
3. Add sample-accurate event scheduling
4. Basic play/stop/record functionality

### Phase 2: Multi-Port Support
1. Scan and manage multiple MIDI devices
2. Per-track MIDI port routing
3. Port hot-plug detection
4. Save/restore port configurations

### Phase 3: MIDI Recording
1. Input port monitoring
2. Recording with count-in and metronome
3. Punch in/out recording
4. Loop recording with takes

### Phase 4: Advanced Features
1. MIDI clock output
2. MTC (MIDI Time Code) support
3. MMC (MIDI Machine Control)
4. SysEx support for hardware editors

## Thread Communication

### UI → MIDI Engine
- Transport commands (play/stop/record)
- Track mute/solo changes
- MIDI port routing changes
- Tempo/time signature changes

### MIDI Engine → UI
- Current playhead position
- Recording status
- MIDI activity meters
- Port connection status

## Timing Precision

### Sample-Accurate Scheduling
- All events scheduled in samples, not milliseconds
- Conversion from musical time (bars/beats) to samples
- Compensation for MIDI interface latency
- Jitter buffer for consistent timing

### Example Timing Calculation
```rust
fn beats_to_samples(beats: f64, tempo: f64, sample_rate: u32) -> u64 {
    let seconds = (beats / tempo) * 60.0;
    (seconds * sample_rate as f64) as u64
}
```

## Benefits Over Current System
1. **Timing**: Sample-accurate vs 10ms frame-based
2. **Performance**: Dedicated thread vs UI thread processing
3. **Scalability**: Multi-port support vs single port
4. **Recording**: Full MIDI input vs playback only
5. **Sync**: MIDI clock output for hardware sync

## Dependencies
- `midir`: MIDI I/O (already in use)
- `crossbeam`: Lock-free queues
- `parking_lot`: Real-time safe mutexes
- Standard library threads with platform-specific priority setting

## Next Steps
1. Create the basic MIDI engine thread structure
2. Implement event scheduler with sample-accurate timing
3. Add multi-port support
4. Implement MIDI recording
5. Add MIDI clock output