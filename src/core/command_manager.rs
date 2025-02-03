use super::commands::*;
use super::DawState;
use std::time::{Duration, Instant};

pub struct CommandManager {
    undo_stack: Vec<DawCommand>,
    redo_stack: Vec<DawCommand>,
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
    pub fn new() -> Self {
        Self {
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            state_snapshots: Vec::new(),
            max_snapshot_count: 50, // todo: configurable
            last_snapshot_time: Instant::now(),
            time_between_snapshots: Duration::from_millis(120), // todo: configurable
        }
    }

    pub fn execute(
        &mut self,
        command: DawCommand,
        state: &mut DawState,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let name = command.name();

        println!("Executing command: {}", name);

        // Save current state before executing the command
        self.save_snapshot(state);

        // Execute the command
        command.execute(state)?;

        // Add to undo stack
        self.undo_stack.push(command);

        // Clear redo stack as we have a new command
        self.redo_stack.clear();

        println!("Executing command: {} - DONE", name);

        Ok(())
    }

    pub fn undo(&mut self, state: &mut DawState) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(command) = self.undo_stack.pop() {
            // Restore the previous state
            if let Some(snapshot) = self.state_snapshots.pop() {
                *state = snapshot.state;
            }

            // Log the undo action
            println!("Undo: {}", command.name());

            self.redo_stack.push(command);
        }
        Ok(())
    }

    pub fn redo(&mut self, state: &mut DawState) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(command) = self.redo_stack.pop() {
            // Save current state before re-executing the command
            self.save_snapshot(state);

            // Re-execute the command
            command.execute(state)?;

            // Log the redo action
            println!("Redo: {}", command.name());

            self.undo_stack.push(command);
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

            let snapshot = StateSnapshot::from_state(state.clone());
            self.last_snapshot_time = now;
            self.state_snapshots.push(snapshot);
        }
    }
}
