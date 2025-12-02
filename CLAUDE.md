# Hypersaw

MIDI-focused DAW built in Rust with egui. Emphasizes sample-accurate timing, undo/redo via command pattern, and immediate-mode UI.

## Build & Run

```bash
cargo check    # Fast type checking
cargo build    # Debug build
cargo run      # Run application
```

## Code Organization

```
src/
├── core/           # Business logic
│   ├── commands.rs     # Command pattern (undo/redo)
│   ├── midi.rs         # MIDI data structures
│   ├── midi_engine.rs  # Sample-accurate playback thread
│   ├── project.rs      # Project state
│   └── state.rs        # Application state
└── ui/             # egui components
    ├── app.rs          # Main application
    ├── timeline.rs     # Track/clip timeline
    └── piano_roll.rs   # Note editor
```

## Key Patterns

- **Command Pattern**: All MIDI operations go through `CommandManager` for undo/redo
- **MidiEngine Thread**: Dedicated thread for sample-accurate timing; communicate via channels
- **Immediate-Mode UI**: egui redraws each frame; keep UI code simple and stateless where possible

## Documentation Workflow

### Active Work

| File | Purpose |
|------|---------|
| `TODOS.md` | Bugs, tasks, and work items discovered during development (not feature requests) |
| `docs/features/FEATURE_NAME_PLAN.md` | Active feature plans with context, design, steps, and acceptance criteria |

### Archive

| File | Purpose |
|------|---------|
| `docs/changelog/YYYY-MM-DD.md` | Completed work archived by date |

### Workflow

1. **Starting a feature**: Create `docs/features/FEATURE_NAME_PLAN.md` first
2. **During development**: Keep the plan updated as design evolves
3. **Completing work**: Move to `docs/changelog/YYYY-MM-DD.md`

### Feature Plan Template

```markdown
# Feature: [Name]

## Context
Why this feature? What problem does it solve?

## Design Decisions
Key architectural choices and tradeoffs.

## Implementation Steps
- [ ] Step 1
- [ ] Step 2

## Acceptance Criteria
- [ ] Criterion 1
- [ ] Criterion 2
```
