# Hypersaw Roadmap

> Hypersaw is a hardware-first, MIDI-focused DAW. Audio editing remains explicitly deferred.
>
> Status reviewed against `feature/midi-recording` on 2026-08-09, after commit `fef2523`.

## Current Status

The arrangement, piano roll, MIDI playback, recording, automation lanes, takes, project
persistence, and basic undo/redo are functional. The next milestone is not another editor
surface; it is making recording and hardware I/O reliable enough for regular use.

Known constraints:

- The MIDI engine and recorder use a hardcoded 44.1 kHz internal sample clock.
- Per-track recording input and channel filters are not persisted; arming uses the default
  wildcard input.
- Playback scheduling is still coordinated by the egui application layer.
- Undo covers note edits and track mute, but not most track, clip, take, automation, or tempo
  mutations.
- There are 23 unit tests, but no CI or end-to-end recording/project tests.

## P0: Recording v1

### MIDI I/O correctness

- [ ] Fix live pitch-bend decoding/encoding around the MIDI center value (-8192..8191).
- [ ] Support the message types already represented by the model: aftertouch, SysEx, MIDI
  clock, start, stop, and continue.
- [ ] Persist a per-track MIDI input port and optional channel filter.
- [ ] Use the persisted input configuration when arming and monitoring a track.
- [ ] Add codec tests for every supported channel and system message.
- [ ] Add fake-port tests for disconnect, reconnect, monitoring, mute, and solo behavior.

### Recording semantics

- [ ] Carry the recording mode, transport start, and punch range in the committed recording
  result instead of reading mutable UI state after the session ends.
- [ ] Make Overdub merge into the intended clip instead of always creating a separate clip.
- [ ] Make Replace overwrite only the recorded interval instead of deleting whole overlapping
  clips.
- [ ] Define and test note behavior at punch and loop boundaries.
- [ ] Record loop passes as stacked takes and select the newest completed pass.
- [ ] Expose take rename, mute, delete, and active-take selection consistently in the UI.
- [ ] Add integration tests for count-in, quantize-on-record, Overdub, Replace, Punch, and loop
  recording.

## P1: Project and Undo Safety

### Project management

- [x] Store clip assets under stable, relative `midi/<clip-id>.mid` paths.
- [x] Save empty clips and avoid duplicating MIDI assets on repeated saves.
- [x] Warn before New or Load when the project is dirty.
- [ ] Add project naming and Save As.
- [ ] Save a loaded project back to its existing location by default.
- [ ] Add recent-project handling.
- [ ] Add autosave and crash recovery after Save/Save As semantics are settled.

### Undo/redo

- [x] Undo/redo for note add, delete, move, resize, velocity, and track mute.
- [x] Keyboard shortcuts and truthful Edit-menu enabled state.
- [ ] Add undo data for destructive track, clip, take, automation, routing, and BPM changes.
- [ ] Group a continuous drag or paint gesture into one undo entry.
- [ ] Add command-level undo/redo regression tests.
- [ ] Add an undo history panel after command coverage and grouping are complete.

## P1: Architecture and Verification

- [ ] Move transport lookahead and event scheduling out of `SupersawApp::update` and into a
  framework-neutral controller or engine-owned timeline.
- [ ] Replace the hardcoded 44.1 kHz value with one shared clock configuration.
- [ ] Split project data, editor session state, and MIDI runtime handles out of `DawState`.
- [ ] Extract recording conversion and project I/O orchestration from the egui update loop.
- [ ] Add CI for formatting, tests, and Clippy on supported platforms.
- [ ] Expand tests around loop/seek scheduling, project round trips, recording, and undo.
- [ ] Remove or integrate dead prototype modules (`midi_editing`, `undo_data`, and `keymap`).
- [ ] Resolve the existing Rust/Clippy warning backlog and the `block 0.1.6`
  future-compatibility warning.
- [ ] Document stable public interfaces with rustdoc.

## P2: MIDI Editing

### Selection and shortcuts

- [x] Lasso selection using visual intersection.
- [x] Ctrl/Cmd-drag duplication.
- [x] Multi-note resize with collective clamping.
- [x] Grid/semitone/octave keyboard nudging.
- [ ] Cut selected notes (Ctrl/Cmd+X).
- [ ] Split notes at the cursor.
- [ ] Join selected notes.
- [ ] Shift-click to add to selection and modifier-click to remove.
- [ ] Double-click to select notes of the same pitch.
- [ ] Select by velocity, beat, and length filters.
- [ ] Replace hardcoded shortcuts with a real keymap and shortcuts editor.

### Transformations and quantization

- [ ] Transpose dialog for semitone/octave operations.
- [ ] Legato and staccato transforms.
- [ ] Remove overlaps.
- [ ] Time stretch/compress.
- [ ] Reverse time and invert pitch.
- [x] Visual velocity lane with direct editing.
- [ ] Velocity ramps, curves, randomization, compression, and presets.
- [ ] Quantize dialog for strength, swing, and note-length preservation.
- [ ] Groove template extraction and application.

## P2: Workflow Features

- [ ] Ghost notes from other tracks.
- [ ] Fold the piano roll to used notes or a selected scale.
- [ ] MIDI activity indicators.
- [ ] Note coloring by velocity or channel.
- [ ] MIDI clip library and project templates.
- [ ] Tempo-map and time-signature editing at the project level.

## P3: Larger Product Features

### MIDI effects

- [ ] Per-track MIDI effect-chain architecture.
- [ ] Arpeggiator, chord generator, scale snap, note repeater, and velocity processor.

### Step sequencer

- [ ] 16/32/64-step view with velocity, gate, probability, pattern chaining, and a pattern
  library.

### Advanced routing and expression

- [ ] MIDI channel matrix and note/velocity range filters.
- [ ] MIDI learn and virtual MIDI cables between tracks.
- [ ] MPE recording, editing, and per-note automation.
- [ ] MIDI statistics, chord/key detection, and performance analysis.

## Deferred

- Full audio recording and sample editing.
- A full GPUI port. Revisit only after controller/scheduler extraction; start with a measured
  vertical-slice prototype rather than a framework-wide rewrite.

## Recently Completed

- Stable scheduling lookahead, loop seek/rescheduling, and all-notes-off on reposition.
- MIDI timestamp conversion and coherent committed recording batches.
- Active/muted take-aware playback.
- Portable project MIDI assets and empty-clip saving.
- egui focus, modal, drag, scrolling, snapping, and fixed-size transport control fixes.
- Piano-roll paste/duplicate timing, full 0-127 pitch range, automation scrolling, and velocity
  editing.
