# Hypersaw - Developer Quick Reference

**Quick links for developers getting started with the Hypersaw codebase.**

---

## 📁 File Locations

### Entry Points
- `/Users/helge/code/hypersaw/src/main.rs` - Application entry (27 lines)
- `/Users/helge/code/hypersaw/src/ui/app.rs` - Main UI loop (56KB)

### Core Modules
```
src/core/
├── state.rs              # Global DAW state (DawState struct)
├── project.rs            # Project data model (Track, Clip, Project)
├── commands.rs           # Command pattern (56KB - all user actions)
├── command_manager.rs    # Undo/redo system
├── midi.rs               # MIDI data structures (Note, MidiEvent, MidiEventStore)
├── midi_engine.rs        # Real-time playback thread (28KB)
├── midi_recording.rs     # Recording coordinator thread (18KB)
├── midi_editing.rs       # MIDI manipulation utilities
├── automation.rs         # Automation lanes and points
└── utils.rs              # Time conversion, grid snapping
```

### UI Modules
```
src/ui/
├── app.rs                # Main application window (56KB)
├── timeline.rs           # Timeline/arrangement view (69KB)
├── piano_roll.rs         # MIDI note editor (91KB)
└── plugin_browser.rs     # Plugin scanning (disabled)
```

---

## 🏗️ Key Data Structures

### DawState (Global State)
```rust
// Location: src/core/state.rs
pub struct DawState {
    pub project: Project,                    // All project data
    pub playing/recording: bool,             // Transport state
    pub current_time: f64,                   // Playhead position (seconds)
    pub midi_engine: Option<Arc<Mutex<...>>>, // Real-time engine
    pub recording_coordinator: Option<...>,  // Recording thread
    pub selected_track/clip: Option<String>, // Current selection
    pub current_view: EditorView,            // Arrangement/PianoRoll
    pub status: StatusManager,               # Toast messages
}
```

### Project Structure
```rust
// Location: src/core/project.rs
Project {
    name: String,
    bpm: f64,
    ppq: u32,               // 480 default
    tracks: Vec<Track>,
}

Track {
    id: String,             // UUID
    clips: Vec<Clip>,
    is_muted/soloed/armed: bool,
    track_type: TrackType,  // Midi | Audio
}

Clip::Midi {
    start_time: f64,
    length: f64,
    file_path: PathBuf,     // Path to .mid file
    midi_data: Option<MidiEventStore>,
    automation_lanes: Vec<AutomationLane>,
}
```

### Command Pattern
```rust
// Location: src/core/commands.rs
pub enum DawCommand {
    AddNote { clip_id, start_time, duration, pitch, velocity },
    MoveNotes { clip_id, note_ids, delta_time, delta_pitch },
    DeleteNotes { clip_id, note_ids, deleted_notes: Option<Vec<Note>> },
    // ... 40+ command variants
}

pub trait Command {
    fn execute(&self, state: &mut DawState) -> Result<(), Error>;
    fn undo(&self, state: &mut DawState) -> Result<(), Error>;
    fn name(&self) -> &'static str;
}
```

---

## 🔧 Common Operations

### Adding a New Command

1. **Define in `DawCommand` enum** (`src/core/commands.rs`):
```rust
pub enum DawCommand {
    MyNewCommand {
        param1: String,
        param2: f64,
        old_value: Option<OldData>, // For undo
    },
}
```

2. **Implement `Command` trait**:
```rust
impl Command for DawCommand {
    fn execute(&self, state: &mut DawState) -> Result<(), Error> {
        match self {
            DawCommand::MyNewCommand { param1, param2, .. } => {
                // Perform the action
                // Store undo data in old_value
                Ok(())
            }
        }
    }

    fn undo(&self, state: &mut DawState) -> Result<(), Error> {
        match self {
            DawCommand::MyNewCommand { old_value, .. } => {
                if let Some(old) = old_value {
                    // Restore old state
                }
                Ok(())
            }
        }
    }

    fn name(&self) -> &'static str {
        match self {
            DawCommand::MyNewCommand { .. } => "My New Command",
        }
    }
}
```

3. **Execute from UI**:
```rust
// In app.rs, timeline.rs, or piano_roll.rs
self.command_manager.execute(
    DawCommand::MyNewCommand {
        param1: value1,
        param2: value2,
        old_value: None, // Capture in execute()
    },
    &mut self.state,
)?;
```

### Time Conversions

```rust
// Location: src/core/utils.rs (TimeUtils struct)

// Seconds ↔ Beats
let beats = TimeUtils::seconds_to_beats(seconds, bpm);
let seconds = TimeUtils::beats_to_seconds(beats, bpm);

// Location: src/core/project.rs (Project methods)

// Seconds ↔ Ticks
let ticks = project.seconds_to_ticks(seconds);
let seconds = project.ticks_to_seconds(ticks);

// Beats ↔ Samples (engine)
fn beats_to_samples(beats: f64, tempo: f64, sample_rate: u32) -> u64 {
    let seconds = (beats / tempo) * 60.0;
    (seconds * sample_rate as f64) as u64
}
```

### Snap to Grid

```rust
// Location: src/core/utils.rs (TimeUtils struct)
impl TimeUtils {
    pub fn snap_time(time: f64, bpm: f64, snap_mode: SnapMode) -> f64 {
        let grid_size = snap_mode.get_division(bpm);
        if grid_size <= 0.0 {
            return time; // No snapping
        }
        (time / grid_size).round() * grid_size
    }
}

// Usage
let snapped_time = TimeUtils::snap_time(cursor_time, state.project.bpm, state.snap_mode);

// For drag operations with accumulator (prevents micro-movements)
let snapped = SnapHandler::snap_time_accumulated(time, bpm, snap_mode, &mut accumulator);
```

### Accessing MIDI Engine

```rust
// Send command to engine
if let Some(engine) = &state.midi_engine {
    engine.lock().send_command(MidiEngineCommand::SetTempo(120.0));
}

// Read messages from engine
if let Some(engine) = &state.midi_engine {
    for msg in engine.lock().read_messages() {
        match msg {
            MidiEngineMessage::PositionUpdate(beats) => {
                state.current_time = beats_to_seconds(beats, state.project.bpm);
            }
            MidiEngineMessage::MidiInput(port, message, timestamp) => {
                // Handle incoming MIDI
            }
        }
    }
}
```

---

## 🧪 Testing Locally

### Run the Application
```bash
cd /Users/helge/code/hypersaw
cargo run
```

### Build Release
```bash
cargo build --release
./target/release/supersaw
```

### Check for Errors
```bash
cargo check
```

### Format Code
```bash
cargo fmt
```

### Run Clippy (Linting)
```bash
cargo clippy
```

---

## 🎹 MIDI Testing

### Test Files
- `/Users/helge/code/hypersaw/data/4bars.mid` - Auto-loaded on startup

### Connect MIDI Hardware
1. Connect MIDI keyboard/controller via USB
2. Launch Hypersaw
3. MIDI ports auto-detected and listed in timeline controls
4. Click port dropdown to select output device
5. Arm track (red record button) to enable recording

### Virtual MIDI (macOS)
```bash
# Open Audio MIDI Setup
open "/Applications/Utilities/Audio MIDI Setup.app"

# Create IAC Driver (virtual MIDI bus)
# Window > Show MIDI Studio > IAC Driver > Enable "Device is online"
```

---

## 🐛 Debugging Tips

### Enable Debug Logging
```rust
// Add to main.rs
println!("Debug: Current time = {}", state.current_time);
eprintln!("Error: Failed to load MIDI file");
```

### Inspect State
```rust
// Print entire project structure
println!("{:#?}", state.project);

// Print specific track
if let Some(track) = state.project.tracks.get(0) {
    println!("Track 0: {:#?}", track);
}
```

### Monitor MIDI Events
```rust
// In midi_engine.rs process_events()
println!("Sending MIDI: {:?} at sample {}", event.message, event.time_in_samples);
```

### UI Inspection
- egui has built-in inspection: `Ctrl+Shift+I` (if enabled in app)
- Use `egui::show_tooltip_text()` to debug hover values
- `ui.label(format!("Debug: {}", value))` for inline debugging

---

## 📚 Documentation Locations

### Project Documentation
- `/Users/helge/code/hypersaw/README.md` - Basic setup
- `/Users/helge/code/hypersaw/docs/CODEBASE_ANALYSIS.md` - **Comprehensive analysis**
- `/Users/helge/code/hypersaw/docs/EXECUTIVE_SUMMARY.md` - High-level overview
- `/Users/helge/code/hypersaw/docs/midi_engine_design.md` - Engine architecture

### Task Lists
- `/Users/helge/code/hypersaw/TODOS.md` - Feature roadmap (302 lines)
- `/Users/helge/code/hypersaw/CODE_REVIEW_FIXES.md` - Completed bug fixes
- `/Users/helge/code/hypersaw/UNDO_IMPLEMENTATION.md` - Undo system details
- `/Users/helge/code/hypersaw/KEYBOARD_SHORTCUTS.md` - Shortcut reference

### External Documentation
- [egui documentation](https://docs.rs/egui)
- [midir documentation](https://docs.rs/midir)
- [Rust book](https://doc.rust-lang.org/book/)

---

## 🔍 Code Patterns to Follow

### Pattern: Bounded Channels for Thread Communication
```rust
// Don't use unbounded()
let (tx, rx) = crossbeam::channel::unbounded(); // ❌ Risk of memory growth

// Use bounded() instead
let (tx, rx) = crossbeam::channel::bounded(1000); // ✅ Bounded queue
```

### Pattern: Arc<Mutex<T>> for Shared Mutable State
```rust
// Share across threads
let engine = Arc::new(Mutex::new(MidiEngine::new()));
let engine_clone = Arc::clone(&engine);

// Lock only when needed
{
    let mut engine = engine.lock();
    engine.send_command(cmd);
} // Lock released immediately
```

### Pattern: Option<T> for Undo Data
```rust
// Command stores undo data as Option
DawCommand::DeleteNotes {
    note_ids: vec![id1, id2],
    deleted_notes: None, // Created from UI
}

// After execute(), populate for undo
fn execute(&self, state: &mut DawState) -> Result<(), Error> {
    // Capture notes before deleting
    let deleted = note_ids.iter()
        .filter_map(|id| store.get_note(id))
        .collect();

    // Store in command (via mutation or new command)
    // Next call to undo() will have data
}
```

### Pattern: Time Representation
```rust
// UI uses seconds (f64)
let clip_start_time: f64 = 2.5; // 2.5 seconds

// Engine uses samples (u64) for sample-accurate timing
let event_time_samples: u64 = beats_to_samples(beats, tempo, 44100);

// Musical time uses beats (f64)
let position_beats: f64 = seconds_to_beats(current_time, bpm);
```

---

## ⚠️ Common Pitfalls

### Pitfall: Forgetting to Update Engine on State Changes
```rust
// ❌ Wrong - Only updates UI state
state.project.bpm = 140.0;

// ✅ Correct - Updates both UI and engine
state.project.bpm = 140.0;
if let Some(engine) = &state.midi_engine {
    engine.lock().send_command(MidiEngineCommand::SetTempo(140.0));
}
```

### Pitfall: Holding Locks During I/O
```rust
// ❌ Wrong - Lock held during MIDI send
let mut ports = output_ports.lock();
for event in events {
    send_midi(&mut ports, event); // Lock held for entire loop
}

// ✅ Correct - Staged processing
let events_to_send = {
    let queue = event_queue.lock();
    queue.drain_due_events() // Short lock
}; // Lock released

for event in events_to_send {
    let mut ports = output_ports.lock();
    send_midi(&mut ports, event); // Lock per iteration
}
```

### Pitfall: Frame-based Timing for MIDI
```rust
// ❌ Wrong - Frame-based (imprecise)
let lookahead_ms = 50.0;
if event.time < current_time + lookahead_ms { /* ... */ }

// ✅ Correct - Sample-accurate
let lookahead_samples = 2205; // 50ms at 44.1kHz
if event.time_in_samples <= current_sample + lookahead_samples { /* ... */ }
```

---

## 🚀 Quick Start for New Features

### 1. Check TODOS.md for Priority
Find your feature in `/Users/helge/code/hypersaw/TODOS.md` to understand scope and priority.

### 2. Find Related Code
Use `grep` or IDE search to find existing implementations:
```bash
# Find all references to "quantize"
grep -r "quantize" src/

# Find struct definitions
grep -r "pub struct" src/core/

# Find command definitions
grep -A 5 "pub enum DawCommand" src/core/commands.rs
```

### 3. Create Command (if needed)
Add to `DawCommand` enum, implement `Command` trait, add undo data.

### 4. Update UI
Add button/menu item in `app.rs`, `timeline.rs`, or `piano_roll.rs`.

### 5. Execute Command
Call `command_manager.execute(DawCommand::YourCommand { ... }, &mut state)?;`

### 6. Test Manually
Run `cargo run`, test feature, verify undo/redo works.

### 7. Document
Update TODOS.md to mark feature complete, add comments to code.

---

## 📞 Getting Help

- **Code questions:** Check architecture docs in `docs/`
- **Feature priority:** See `TODOS.md`
- **Bug reports:** File GitHub issue
- **Architecture questions:** Review `docs/CODEBASE_ANALYSIS.md`

---

**Last Updated:** December 2, 2025
**Maintainer:** HelgeSverre
**Repository:** github.com/HelgeSverre/hypersaw
