use crate::note::NoteConfig;
use std::fs;

pub fn load_note_map(path: &str) -> Vec<NoteConfig> {
    let content = fs::read_to_string(path).expect("Failed to read config");
    serde_json::from_str(&content).expect("Invalid JSON format")
}
