pub mod automation;
pub mod command_manager;
pub mod commands;
pub mod dawproject;
pub mod midi;
pub mod midi_editing;
pub mod project;
pub mod sample_project;
pub mod state;
pub mod status;
pub mod utils;

#[cfg(test)]
mod test_dawproject;

pub use automation::*;
pub use command_manager::*;
pub use commands::*;
pub use midi::*;
pub use midi_editing::*;
pub use project::*;
pub use state::*;
pub use status::*;
pub use utils::*;

// Import dawproject types with explicit naming to avoid conflicts
pub use dawproject::{DawProject, MetaData as DawMetaData};
