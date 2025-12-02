# Hypersaw - TODO List

> **Note**: Hypersaw is a MIDI-focused DAW. Audio support is secondary.
>
> See `docs/changelog/` for completed work history.

## Current Focus: MIDI Recording Branch

The `feature/midi-recording` branch has +1,885 uncommitted lines. Priority is completing and merging this work.

---

## High Priority

### Undo/Redo System
- [x] Basic commands (Mute/Solo/Arm, Add/Move notes, Playback)
- [x] Redo functionality in CommandManager
- [x] Keyboard shortcuts (Ctrl+Z/Ctrl+Shift+Z)
- [x] Visual feedback in Edit menu
- [x] DeleteNotes undo
- [x] ResizeNote undo
- [x] UpdateNoteVelocity undo
- [ ] Create undo history UI panel
- [ ] Undo grouping for related MIDI edits

### Keyboard Shortcuts
- [x] Quantize (Q)
- [x] Duplicate (Ctrl+D)
- [x] Select all (Ctrl+A)
- [x] Copy/Paste (Ctrl+C/V)
- [x] Delete (Del/Backspace)
- [ ] Transpose up/down (Shift+Up/Down)
- [ ] Cut (Ctrl+X)
- [ ] Split notes at cursor (S)
- [ ] Join selected notes (J)
- [ ] Velocity shortcuts (increase/decrease)
- [ ] Customizable shortcuts configuration
- [ ] Shortcuts editor UI

### MIDI Editing Tools

#### Selection
- [ ] Fix selection box dragging in piano roll
- [ ] Lasso selection tool
- [ ] Double-click to select same pitch
- [ ] Select by velocity/beat/length filters
- [ ] Add to selection (Shift+click)
- [ ] Remove from selection (Ctrl+click)

#### Batch Transformations
- [ ] Transpose (semitones/octaves)
- [ ] Legato / Staccato
- [ ] Remove overlaps
- [ ] Time stretch/compress
- [ ] Reverse notes
- [ ] Invert pitch

#### Velocity Editing
- [ ] Visual velocity lane
- [ ] Draw velocity ramps/curves
- [ ] Randomize / compress dynamics
- [ ] Velocity presets (pp, p, mf, f, ff)

#### Smart Quantization
- [ ] Quantize dialog (strength %, swing)
- [ ] Preserve note lengths option
- [ ] Groove template extraction
- [ ] MPC-style swing

### MIDI Loop Recording
- [ ] Merge mode (layer passes)
- [ ] Replace mode (last pass only)
- [ ] Stack takes mode
- [ ] Visual loop region markers
- [ ] Loop length adjustment while playing

---

## Medium Priority

### MIDI Effects
- [ ] Effect chain architecture per track
- [ ] Arpeggiator (patterns, rate sync, gate)
- [ ] Chord generator
- [ ] Scale snap
- [ ] Note repeater (ratcheting)
- [ ] Velocity processor

### Step Sequencer
- [ ] Step sequencer view (16/32/64 steps)
- [ ] Per-step velocity/gate
- [ ] Pattern chaining
- [ ] Probability per step
- [ ] Pattern library

### Project Management
- [ ] Fix file path issues in save/load
- [ ] MIDI clip library
- [ ] Template system

---

## Lower Priority

### Advanced MIDI Routing
- [ ] MIDI channel matrix
- [ ] Note/velocity range filtering
- [ ] MIDI learn for parameters
- [ ] Virtual MIDI cables between tracks

### MPE Support
- [ ] MPE recording
- [ ] MPE editing in piano roll
- [ ] Per-note automation lanes

### Analysis Tools
- [ ] MIDI statistics view
- [ ] Chord/key detection
- [ ] Performance analyzer

---

## Architectural Improvements
- [ ] Move scheduling to engine (engine owns timeline, not UI)
- [ ] Add automated tests (unit + integration)
- [ ] Resolve 49 compiler warnings
- [ ] Document public interfaces (rustdoc)

---

## UI/UX Polish
- [ ] Ghost notes from other tracks
- [ ] Fold piano roll to used notes
- [ ] MIDI activity indicators
- [ ] Custom note colors by velocity/channel

---

*Last updated: 2025-12-02*
