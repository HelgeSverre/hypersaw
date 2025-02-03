// // src/keymap.rs
// use eframe::egui::Key;
// use serde::{Deserialize, Serialize};
// use std::collections::HashMap;
// use std::fs::File;
// use std::path::Path;
//
// #[derive(Serialize, Deserialize, Debug)]
// pub enum KeyAction {
//     LoadProject,
//     SaveProject,
//     Undo,
//     Redo,
// }
//
// struct Keymap {
//     keymap: HashMap<Vec<Key>, KeyAction>,
// }
//
// impl Keymap {
//     pub fn initialize_keymap() -> HashMap<Vec<Key>, KeyAction> {
//         use KeyAction::*;
//         let mut keymap = HashMap::new();
//
//         // Add key bindings
//         keymap.insert(vec![Key::O], LoadProject);
//         keymap.insert(vec![Key::S], SaveProject);
//         keymap.insert(vec![Key::Z], Undo);
//         keymap.insert(vec![Key::R], Redo);
//         keymap.insert(vec![Key::G, "Shift", "Ctrl"]);
//
//         keymap
//     }
//
//     pub fn load_keymap(
//         path: &Path,
//     ) -> Result<HashMap<Vec<Key>, KeyAction>, Box<dyn std::error::Error>> {
//         let file = File::open(path)?;
//         let keymap = serde_json::from_reader(file)?;
//         Ok(keymap)
//     }
// }
