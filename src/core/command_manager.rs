use super::commands::*;
use super::DawState;

#[derive(Default)]
pub struct CommandManager {
    undo_stack: Vec<DawCommand>,
    redo_stack: Vec<DawCommand>,
    project_dirty: bool,
}

impl CommandManager {
    pub fn execute(
        &mut self,
        command: DawCommand,
        state: &mut DawState,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let is_undoable = command.is_undoable();
        let invalidates_redo = command.invalidates_redo();
        let changes_project = command.changes_project();

        command.execute(state)?;

        self.project_dirty |= changes_project;

        if is_undoable {
            self.undo_stack.push(command);
        }

        if is_undoable || invalidates_redo {
            self.redo_stack.clear();
        }

        Ok(())
    }

    pub fn undo(&mut self, state: &mut DawState) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(command) = self.undo_stack.pop() {
            let changes_project = command.changes_project();
            if let Err(error) = command.undo(state) {
                self.undo_stack.push(command);
                return Err(error);
            }
            self.project_dirty |= changes_project;

            // Log the undo action
            state.status.info(format!("Undo: {}", command.name()));

            self.redo_stack.push(command);
        } else {
            state.status.info("Nothing to undo".to_string());
        }
        Ok(())
    }

    pub fn redo(&mut self, state: &mut DawState) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(command) = self.redo_stack.pop() {
            let changes_project = command.changes_project();
            if let Err(error) = command.execute(state) {
                self.redo_stack.push(command);
                return Err(error);
            }
            self.project_dirty |= changes_project;

            // Log the redo action
            state.status.info(format!("Redo: {}", command.name()));

            self.undo_stack.push(command);
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

    pub fn clear(&mut self) {
        self.undo_stack.clear();
        self.redo_stack.clear();
        self.project_dirty = false;
    }

    pub fn is_project_dirty(&self) -> bool {
        self.project_dirty
    }

    pub fn mark_project_dirty(&mut self) {
        self.project_dirty = true;
    }

    pub fn mark_project_saved(&mut self) {
        self.project_dirty = false;
    }
}
