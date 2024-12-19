// src/core/command_manager.rs
use super::commands::*;
use super::DawState;

pub struct CommandManager {
    undo_stack: Vec<DawCommand>,
    redo_stack: Vec<DawCommand>,
}

impl CommandManager {
    pub fn new() -> Self {
        Self {
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
        }
    }

    pub fn execute(
        &mut self,
        command: DawCommand,
        state: &mut DawState,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Execute the command
        command.execute(state)?;

        // Add to undo stack
        self.undo_stack.push(command);

        // Clear redo stack as we have a new command
        self.redo_stack.clear();

        Ok(())
    }

    pub fn undo(&mut self, state: &mut DawState) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(command) = self.undo_stack.pop() {
            command.undo(state)?;
            self.redo_stack.push(command);
        }
        Ok(())
    }

    pub fn redo(&mut self, state: &mut DawState) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(command) = self.redo_stack.pop() {
            command.execute(state)?;
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
}
