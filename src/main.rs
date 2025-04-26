use midir::{Ignore, MidiInput};
use std::str;
use std::sync::Arc;
use tokio::net::UdpSocket;
use tokio::sync::mpsc;
use tokio::sync::Mutex;
use tokio::time::{sleep, Duration, Instant};

mod config;
mod led;
mod note;

use config::load_note_map;
use led::LedState;
use note::ActiveNote;

const FADE_INTERVAL_MS: u64 = 20;

async fn start_udp_listener(note_tx: mpsc::Sender<note::NoteEvent>) -> std::io::Result<()> {
    let socket = UdpSocket::bind("0.0.0.0:10000").await?;
    println!("Listening for UDP messages on port 10000...");

    let mut buf = [0u8; 64];

    loop {
        let (len, _addr) = socket.recv_from(&mut buf).await?;
        if let Ok(msg) = str::from_utf8(&buf[..len]) {
            // Expected format: "55 125;"
            let parts: Vec<&str> = msg
                .trim()
                .trim_end_matches(';')
                .split_whitespace()
                .collect();

            if parts.len() == 2 {
                let note = parts[0].parse::<u8>();
                let velocity = parts[1].parse::<u8>();

                if let (Ok(note), Ok(velocity)) = (note, velocity) {
                    if (22..=108).contains(&note) {
                        if velocity == 0 {
                            let _ = note_tx.send(note::NoteEvent::NoteOff(note)).await;
                        } else {
                            let _ = note_tx.send(note::NoteEvent::NoteOn(note, velocity)).await;
                        };
                    } else {
                        println!("Note out of range: {}", note);
                    }
                } else {
                    let control_velocity = (parts[0].parse::<u16>().unwrap() - 300) as u8;
                    let control_message = parts[1].parse::<u8>().unwrap();
                    if control_message == 64 {
                        let _ = note_tx
                            .send(note::NoteEvent::ControlChange(
                                control_message,
                                control_velocity,
                            ))
                            .await;
                    } else {
                        println!("Invalid number in message: {}", msg);
                        println!("Parts: 0 {:?}, 1 {:?}", parts[0], parts[1]);
                    }
                }
            }
        }
    }
}

fn start_midi_listener(tx: mpsc::Sender<note::NoteEvent>) {
    std::thread::spawn(move || {
        let mut midi_in = MidiInput::new("piano-listener").expect("Couldn't create MIDI input");
        midi_in.ignore(Ignore::None);

        let ports = midi_in.ports();

        let port = ports.iter().find(|p| {
            if let Ok(name) = midi_in.port_name(p) {
                name.contains("Digital Piano")
            } else {
                false
            }
        });

        let Some(port) = port else {
            eprintln!("No matching MIDI port found for 'Digital Piano'");
            return;
        };

        let port_name = midi_in.port_name(port).unwrap_or("<unknown>".to_string());
        println!("Connecting to MIDI device: {port_name}");

        let _conn = midi_in.connect(
            port,
            "midir-read",
            move |_, message, _| {
                if message.len() >= 3 {
                    let status = message[0] & 0xF0;
                    let note = message[1];
                    let velocity = message[2];

                    match status {
                        0x90 => {
                            if velocity > 0 {
                                let _ = tx.blocking_send(note::NoteEvent::NoteOn(note, velocity));
                            } else {
                                let _ = tx.blocking_send(note::NoteEvent::NoteOff(note));
                            }
                        }
                        0x80 => {
                            let _ = tx.blocking_send(note::NoteEvent::NoteOff(note));
                        }
                        0xB0 => {
                            let _ =
                                tx.blocking_send(note::NoteEvent::ControlChange(note, velocity));
                        }
                        _ => {}
                    }
                }
            },
            (),
        );

        println!("Listening for MIDI input from {port_name}...");

        loop {
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
    });
}

#[tokio::main]
async fn main() {
    env_logger::init();

    let notes_config = load_note_map("notes_config_safe.json");
    let shared_notes: Arc<Mutex<Vec<ActiveNote>>> = Arc::new(Mutex::new(Vec::new()));
    let led_state = Arc::new(Mutex::new(LedState::new()));
    let pending_note_offs = Arc::new(Mutex::new(Vec::<u8>::new()));
    let sustain_active = Arc::new(Mutex::new(false));

    // Channel for all the notes
    let (tx, mut rx) = mpsc::channel(100);
    start_midi_listener(tx.clone());

    tokio::spawn(async move {
        if let Err(e) = start_udp_listener(tx).await {
            eprintln!("UDP listener error: {:?}", e);
        }
    });

    println!("Now looping through the fade timer.");
    loop {
        // Handle incoming MIDI messages
        while let Ok(event) = rx.try_recv() {
            match event {
                note::NoteEvent::NoteOn(midi_note, velocity) => {
                    let mut notes = shared_notes.lock().await;
                    if let Some(config) = notes_config
                        .clone()
                        .into_iter()
                        .find(|c| c.midi == midi_note)
                    {
                        // Add the note to the list of notes to play
                        notes.push(note::ActiveNote {
                            config,
                            intensity: velocity,
                            birth: Instant::now(),
                            int_birth: velocity,
                        });
                        // This should also negate any notes off in the sustain pedal list
                        // because you are now playing the note again
                        let mut pending = pending_note_offs.lock().await;
                        pending.retain(|n| n != &midi_note);
                    }
                }
                note::NoteEvent::NoteOff(midi_note) => {
                    let sustain = sustain_active.lock().await;
                    if *sustain {
                        let mut pending = pending_note_offs.lock().await;
                        pending.push(midi_note);
                    } else {
                        let mut notes = shared_notes.lock().await;
                        notes.retain(|n| n.config.midi != midi_note);
                    }
                }
                note::NoteEvent::ControlChange(64, value) => {
                    // Sustain Pedal
                    let mut sustain = sustain_active.lock().await;
                    if value >= 80 {
                        *sustain = true;
                    } else if value <= 40 {
                        *sustain = false;

                        // Sustain was just released: flush pending note offs
                        let mut pending = pending_note_offs.lock().await;
                        let mut notes = shared_notes.lock().await;

                        // TODO: I think I just want to take one out, not all of them
                        for midi_note in pending.drain(..) {
                            notes.retain(|n| n.config.midi != midi_note);
                        }
                    }
                }

                _ => {}
            }
        }

        {
            // Prevent too many notes from nuking the power.
            let mut notes = shared_notes.lock().await;

            // If too many notes, drop oldest first
            if notes.len() > 20 {
                // Sort by birth (oldest first)
                println!("Found too many.");
                notes.sort_by_key(|note| note.birth);
                let overflow = notes.len() - 20;
                notes.drain(0..overflow);
            }
        }

        // Fade note intensity over time

        sleep(Duration::from_millis(FADE_INTERVAL_MS)).await;

        let now = Instant::now();
        let mut notes = shared_notes.lock().await;
        // const SECONDS_TO_HOLD: f32 = 2.0;
        const SECONDS_TO_HOLD: f32 = 10.0;

        notes.retain_mut(|note| {
            let age = now.duration_since(note.birth).as_secs_f32();
            // Time to do this manually.  Let's say time is percentages
            // Up to 5% - Intensity = 100%
            // 5 - 15%  - Intensity decays down to 50%
            // from 15% - 100% - Intensity decays to 0%
            // That's basically an ASDR envelope lol
            const ATTACK: f32 = 0.015;
            const SUSTAIN: f32 = 0.15;
            const DECAY: f32 = 1.0;
            let current_percent = age / SECONDS_TO_HOLD;
            let decayed_intensity: f32 = note.int_birth as f32 * (1.0 - &current_percent);
            if current_percent <= ATTACK {
                // 100% -> 95%
                note.intensity = decayed_intensity as u8;
            } else if (ATTACK..=SUSTAIN).contains(&current_percent) {
                // 100% -> 50%
                note.intensity = (decayed_intensity * 0.3) as u8;
            } else if (SUSTAIN..=DECAY).contains(&current_percent) {
                // 50% -> 0%
                note.intensity = (decayed_intensity * 0.1) as u8;
            } else {
                note.intensity = 0_u8;
            }

            //note.intensity = (note.intensity as f32 * (0.9_f32).powf(age * 1.0)).round() as u8;
            note.intensity > 0
        });

        let mut led = led_state.lock().await;
        led.update_from_notes(&notes);
    }
}
