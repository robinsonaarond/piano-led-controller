use crate::note::{ActiveNote, NoteType};
// use env_logger::fmt::Color;
// Ended up rolling our own
pub struct Color;
impl Color {
    pub fn new(r: u8, g: u8, b: u8) -> [u8; 4] {
        [b, g, r, 0] // RGBW
    }
}

pub struct LedState {
    pub controller: rs_ws281x::Controller,
}

impl LedState {
    pub fn new() -> Self {
        let led_count_1 = 1200;
        let led_count_2 = 1500;

        let channel1 = rs_ws281x::ChannelBuilder::new()
            .pin(18)
            .count(led_count_1)
            .brightness(255)
            .strip_type(rs_ws281x::StripType::Ws2812)
            .build();

        let channel2 = rs_ws281x::ChannelBuilder::new()
            .pin(13)
            .count(led_count_2)
            .brightness(255)
            .strip_type(rs_ws281x::StripType::Ws2812)
            .build();

        let controller = rs_ws281x::ControllerBuilder::new()
            .dma(10)
            .channel(0, channel1)
            .channel(1, channel2)
            .build()
            .expect("Failed to build LED controller");

        Self { controller }
    }

    pub fn update_from_notes(&mut self, notes: &[ActiveNote]) {
        let (strip1_notes, strip2_notes): (Vec<_>, Vec<_>) =
            notes.iter().partition(|note| note.config.midi <= 59);

        {
            let leds1 = self.controller.leds_mut(0);
            Self::update_strip(leds1, &strip1_notes);
        }

        {
            let leds2 = self.controller.leds_mut(1);
            Self::update_strip(leds2, &strip2_notes);
        }

        self.controller.render().expect("Failed to render LEDs");
    }

    fn update_strip(leds: &mut [[u8; 4]], notes: &[&ActiveNote]) {
        // Clear strip
        for led in leds.iter_mut() {
            *led = Color::new(0, 0, 0);
        }

        // Draw notes
        for note in notes {
            let color = match note.config.note_type {
                NoteType::White => [255, 255, 255],
                NoteType::Black => [150, 150, 255],
            };
            println!(
                "Playing note {:?} ({:?})",
                note.config.name, note.config.midi
            );

            let intensity = (note.intensity as f32 / 128.0).clamp(0.0, 1.0);
            let final_color = Color::new(
                (color[0] as f32 * intensity) as u8,
                (color[1] as f32 * intensity) as u8,
                (color[2] as f32 * intensity) as u8,
            );

            let (start, end) = note.config.led_range;
            for i in start..end {
                if i < leds.len() {
                    leds[i] = final_color;
                }
            }
        }
    }
}
