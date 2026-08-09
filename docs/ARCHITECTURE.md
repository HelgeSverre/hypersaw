# Hypersaw Architecture

**Status:** Active development on `feature/midi-recording`

**Last reviewed:** 2026-08-09

**Scope:** Hardware-first MIDI sequencing; full audio editing is deferred

## System Overview

Hypersaw is a native Rust desktop application built with egui/eframe. The application owns
project and editor state on the UI thread and communicates with dedicated MIDI playback and
recording threads through bounded Crossbeam channels.

```text
┌──────────────────────────────────────────────────────────┐
│ UI thread                                                │
│ SupersawApp                                              │
│ ├── DawState: project, editor session, runtime handles   │
│ ├── CommandManager: implemented undo/redo operations     │
│ ├── Timeline                                             │
│ └── PianoRoll                                            │
└───────────────┬───────────────────────┬──────────────────┘
                │ commands/messages     │ recording events
        ┌───────▼────────┐      ┌───────▼────────────────┐
        │ MIDI engine    │      │ Recording coordinator │
        │ thread         │      │ thread                │
        │ ├── transport  │      │ ├── armed tracks      │
        │ ├── event heap │      │ ├── pre-roll buffer   │
        │ ├── routing    │      │ └── committed batches │
        │ └── metronome  │      └────────────────────────┘
        └───────┬────────┘
                │ midir
        ┌───────▼────────┐
        │ MIDI hardware  │
        └────────────────┘
```

## Module Responsibilities

### Application and UI

- `src/ui/app.rs` owns the eframe lifecycle, menus, transport, dialogs, MIDI port polling,
  recording-result integration, and playback lookahead scheduling.
- `src/ui/timeline.rs` draws the arrangement, track headers, loop/punch regions, clips, takes,
  routing controls, and the MIDI device panel.
- `src/ui/piano_roll.rs` draws and edits notes, selections, velocity, and MIDI automation lanes.

The UI currently performs more orchestration than is desirable. Scheduling, recording-result
conversion, and project dialog workflows should move into framework-neutral controllers before
another UI framework is considered.

### Project and commands

- `src/core/project.rs` defines serializable projects, tracks, clips, takes, editor views, and
  project asset persistence.
- `src/core/commands.rs` defines application commands and their execution behavior.
- `src/core/command_manager.rs` tracks the implemented undo/redo subset and project dirty state.
- `src/core/automation.rs` defines automation lanes, points, parameters, and interpolation.

The command enum covers most actions, but undo is intentionally enabled only where enough
original data is stored to restore state correctly. Note edits and track mute are covered;
most track, clip, take, automation, routing, and tempo mutations still need undo data.

### MIDI model and runtime

- `src/core/midi.rs` owns MIDI events, notes, tempo-aware SMF import/export, and note editing.
- `src/core/midi_engine.rs` owns the playback thread, sample-clock transport, scheduled-event
  queue, hardware connections, routing, mute/solo filtering, and metronome output.
- `src/core/midi_recording.rs` owns arming, monitoring, pre-roll, punch filtering, and the
  one-stop/one-committed-batch recording contract.
- `src/core/state.rs` currently constructs the runtime and combines persistent project state,
  transient editor state, and runtime handles.

## Time and Scheduling Model

- Project clip positions and UI loop/punch positions are stored in seconds.
- MIDI notes contain seconds plus PPQ tick data.
- The engine transport operates in samples and reports positions in beats.
- The app converts between seconds and beats using the project BPM and maintains a four-second
  scheduling horizon.
- Seeking and loop wrap send all-notes-off, clear queued events, reset the scheduling watermark,
  and refill the lookahead range.

Current limitations:

- The engine and recorder are constructed with a hardcoded 44.1 kHz sample rate.
- The application, rather than the engine, scans project clips and owns the scheduling horizon.
- The project has one global BPM; imported clip tempo maps are used during MIDI conversion but
  there is no editable project tempo map or meter lane.

## Recording Model

Each armed track has an input-port selector and optional channel filter in the recording
runtime. The current command/UI path arms tracks with the wildcard `default` port and no channel
filter, so the configuration is not yet persisted per track.

The recorder:

1. receives sample-timestamped MIDI input;
2. rebases timestamps to the recording session;
3. applies input, channel, and punch filters;
4. buffers events until a committed stop; and
5. delivers one coherent `EventsRecorded` batch to the UI.

The UI pairs note-on/off messages, optionally quantizes them, writes a MIDI asset, creates a clip,
and creates a take. Overdub, Replace, Punch, and loop-pass semantics still need a stricter session
result contract and integration tests; see `TODOS.md`.

## Persistence

A project is stored as a directory:

```text
<project>/
├── <project-name>.supersaw
├── midi/
│   └── <clip-id>.mid
└── samples/
```

MIDI asset paths serialized into the project are relative and stable. Loading resolves them
against the project-file directory. Repeated saves reuse the clip ID rather than creating
duplicate assets, and empty MIDI clips produce valid empty MIDI files.

Remaining project-management work includes project naming, Save As, saving loaded projects back
to their existing location, recent projects, and autosave/recovery.

## egui Interaction Rules

The custom arrangement and piano-roll canvases rely on explicit egui gesture state.

- `Response::drag_delta()` is treated according to egui 0.31's per-frame behavior.
- Text input and inline rename own keyboard focus; global shortcuts yield while egui wants
  keyboard input.
- Escape is editor-local for cancellation/deselection. Cmd/Ctrl+1 returns to Arrangement.
- Unsaved-project confirmation uses `egui::Modal` and suppresses background editor input.
- Symbol-based transport controls reserve a fixed 28×28 point footprint.
- Repaints are continuous only while required by playback, recording, routing, or status expiry.

## Verification Baseline

The repository currently has 23 unit tests covering selected MIDI conversions, scheduling queue
behavior, recording batching, project asset persistence, recorded-note conversion, piano-roll
gesture math, and timeline scroll limits.

Important gaps:

- no CI configuration;
- no command-level undo matrix;
- no full recording-mode or loop-pass integration tests;
- no fake MIDI-port disconnect/reconnect tests;
- no project workflow or UI interaction tests; and
- no cross-platform release/packaging matrix.

## Known Technical Debt

- `SupersawApp::update` remains a large orchestration boundary.
- `DawState` combines project, session, and runtime responsibilities.
- `midi_editing`, `undo_data`, and `keymap` are unused or prototype modules.
- The compiler and Clippy report a substantial dead-code/style warning backlog.
- `block 0.1.6`, pulled in transitively, has a future-Rust compatibility warning.
- MIDI message I/O is incomplete for aftertouch, SysEx, and transport/clock messages, and live
  pitch-bend conversion needs correction.

## Implementation Order

1. Complete MIDI I/O correctness and Recording v1 semantics.
2. Complete destructive-operation undo and project Save/Save As safety.
3. Add CI and integration coverage around recording, scheduling, undo, and persistence.
4. Extract framework-neutral transport, scheduling, recording, and project controllers.
5. Add higher-level MIDI editing, effects, and step-sequencer workflows.
6. Revisit a GPUI vertical-slice experiment only after the controller extraction.

The detailed, checkbox-level roadmap lives in [`../TODOS.md`](../TODOS.md).
