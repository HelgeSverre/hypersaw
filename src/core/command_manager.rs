use super::commands::*;
use super::undo_data::{UndoData, UndoDataStore};
use super::DawState;
use std::time::{Duration, Instant};

pub struct CommandManager {
    undo_stack: Vec<(DawCommand, usize)>, // (command, undo_data_id)
    redo_stack: Vec<(DawCommand, usize)>,
    undo_data_store: UndoDataStore,
    state_snapshots: Vec<StateSnapshot>,
    max_snapshot_count: usize,
    last_snapshot_time: Instant,
    time_between_snapshots: Duration,
}

pub struct StateSnapshot {
    timestamp: u64,
    state: DawState,
    command: DawCommand,
}

impl StateSnapshot {
    pub fn from_state(state: DawState) -> Self {
        Self {
            timestamp: 0,
            state,
            command: DawCommand::NoOp,
        }
    }

    pub fn new(state: DawState, command: DawCommand) -> Self {
        Self {
            timestamp: 0,
            state,
            command,
        }
    }
}

impl CommandManager {
    pub fn default() -> Self {
        Self {
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            undo_data_store: UndoDataStore::new(),
            state_snapshots: Vec::new(),
            last_snapshot_time: Instant::now(),
            max_snapshot_count: 50,
            time_between_snapshots: Duration::from_millis(120),
        }
    }

    pub fn new(max_snapshot_count: usize, time_between_snapshots: Duration) -> Self {
        Self {
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            undo_data_store: UndoDataStore::new(),
            state_snapshots: Vec::new(),
            last_snapshot_time: Instant::now(),
            max_snapshot_count,
            time_between_snapshots,
        }
    }

    pub fn execute(
        &mut self,
        command: DawCommand,
        state: &mut DawState,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let name = command.name();

        // Save current state before executing the command
        self.save_snapshot(state);

        // Execute the command
        command.execute(state)?;

        // Add to undo stack with a dummy undo data id (we don't use it yet)
        self.undo_stack.push((command, 0));

        // Clear redo stack as we have a new command
        self.redo_stack.clear();

        Ok(())
    }

    pub fn undo(&mut self, state: &mut DawState) -> Result<(), Box<dyn std::error::Error>> {
        if let Some((command, _undo_data_id)) = self.undo_stack.pop() {
            // Call the command's undo method
            command.undo(state)?;
            
            // Log the undo action
            state.status.info(format!("Undo: {}", command.name()));
            
            // Add to redo stack (with invalid undo data id since we'll recalculate on redo)
            self.redo_stack.push((command, 0));
        } else {
            state.status.info("Nothing to undo".to_string());
        }
        Ok(())
    }

    pub fn redo(&mut self, state: &mut DawState) -> Result<(), Box<dyn std::error::Error>> {
        if let Some((command, _)) = self.redo_stack.pop() {
            // Save current state before re-executing the command
            self.save_snapshot(state);

            // Re-execute the command
            command.execute(state)?;

            // Log the redo action
            state.status.info(format!("Redo: {}", command.name()));

            self.undo_stack.push((command, 0));
        } else {
            state.status.info("Nothing to redo".to_string());
        }
        Ok(())
    }

    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    fn save_snapshot(&mut self, state: &DawState) {
        let now = Instant::now();

        if now.duration_since(self.last_snapshot_time) >= self.time_between_snapshots {
            if self.state_snapshots.len() >= self.max_snapshot_count {
                self.state_snapshots.remove(0);
            }

            // TODO: Implement state snapshots without cloning the entire DawState
            // For now, we'll skip snapshots since DawState contains the MIDI engine handle
            // which can't be cloned. We should serialize only the necessary parts.
            self.last_snapshot_time = now;
        }
    }
}
