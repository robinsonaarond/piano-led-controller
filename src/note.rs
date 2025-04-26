use serde::Deserialize;
use tokio::time::Instant;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize)]
pub enum NoteType {
    White,
    Black,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NoteConfig {
    pub name: String,
    pub midi: u8,
    pub led_range: (usize, usize),
    pub note_type: NoteType,
}

#[derive(Debug, Clone)]
pub struct ActiveNote {
    pub config: NoteConfig,
    pub intensity: u8,
    pub birth: Instant,
    pub int_birth: u8,
}

#[derive(Debug, Clone)]
pub enum NoteEvent {
    NoteOn(u8, u8),        // (midi number, velocity)
    NoteOff(u8),           // (midi number)
    ControlChange(u8, u8), // (controller number, value)
}
