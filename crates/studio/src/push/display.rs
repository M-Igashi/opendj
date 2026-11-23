//! Push 2's screen: 960 x 160 pixels sent over USB. Drawn with
//! embedded-graphics into a frame buffer, then encoded as the manual's
//! "Display Interface" chapter asks: a 16-byte header, then 160 lines of
//! 1920 bytes of pixels and 128 filler bytes, XORed with a fixed pattern.

use std::time::Duration;

use embedded_graphics::mono_font::ascii::{FONT_10X20, FONT_6X10, FONT_7X13, FONT_9X15_BOLD};
use embedded_graphics::mono_font::{MonoFont, MonoTextStyle};
use embedded_graphics::pixelcolor::{Bgr565, RgbColor};
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{PrimitiveStyle, Rectangle};
use embedded_graphics::text::{Alignment, Baseline, Text, TextStyleBuilder};

use super::surface::{Encoders, Mode};
use crate::control::{fader_text, pan_text, Controls, Mix};
use crate::sound::PARAMS;

pub const WIDTH: usize = 960;
pub const HEIGHT: usize = 160;
const LINE_BYTES: usize = 2048;
const COLUMN: i32 = 120;

const VENDOR: u16 = 0x2982;
const PRODUCT: u16 = 0x1967;
const ENDPOINT: u8 = 0x01;
const HEADER: [u8; 16] = [0xFF, 0xCC, 0xAA, 0x88, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
const MASK: [u8; 4] = [0xE7, 0xF3, 0xE7, 0xFF];

/// One screen's pixels, top line first.
pub struct Frame {
    pixels: Vec<u16>,
}

impl Default for Frame {
    fn default() -> Self {
        Frame::new()
    }
}

impl Frame {
    pub fn new() -> Frame {
        Frame {
            pixels: vec![0; WIDTH * HEIGHT],
        }
    }

    /// The bytes Push wants after the header.
    pub fn encode(&self) -> Vec<u8> {
        let mut out = vec![0u8; LINE_BYTES * HEIGHT];
        for (y, line) in self.pixels.chunks(WIDTH).enumerate() {
            let row = &mut out[y * LINE_BYTES..(y + 1) * LINE_BYTES];
            for (x, p) in line.iter().enumerate() {
                row[2 * x..2 * x + 2].copy_from_slice(&p.to_le_bytes());
            }
        }
        for (i, b) in out.iter_mut().enumerate() {
            *b ^= MASK[i % 4];
        }
        out
    }

    /// The frame as 8-bit RGB, for a preview image.
    pub fn rgb(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(WIDTH * HEIGHT * 3);
        for &p in &self.pixels {
            let c = Bgr565::from(embedded_graphics::pixelcolor::raw::RawU16::new(p));
            out.push(c.r() << 3 | c.r() >> 2);
            out.push(c.g() << 2 | c.g() >> 4);
            out.push(c.b() << 3 | c.b() >> 2);
        }
        out
    }
}

impl OriginDimensions for Frame {
    fn size(&self) -> Size {
        Size::new(WIDTH as u32, HEIGHT as u32)
    }
}

impl DrawTarget for Frame {
    type Color = Bgr565;
    type Error = std::convert::Infallible;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        for Pixel(p, c) in pixels {
            if p.x >= 0 && p.y >= 0 && (p.x as usize) < WIDTH && (p.y as usize) < HEIGHT {
                self.pixels[p.y as usize * WIDTH + p.x as usize] = c.into_storage();
            }
        }
        Ok(())
    }
}

fn color(rgb: u32) -> Bgr565 {
    Bgr565::new(
        ((rgb >> 16) as u8) >> 3,
        ((rgb >> 8) as u8) >> 2,
        (rgb as u8) >> 3,
    )
}

const TEXT: u32 = 0xe8e8ea;
const DIM: u32 = 0x8b8d91;
const FAINT: u32 = 0x3a3d40;
const ACCENT: u32 = 0xb69cff;
const RED: u32 = 0xff5040;

fn text(frame: &mut Frame, s: &str, x: i32, y: i32, font: &MonoFont, rgb: u32, align: Alignment) {
    let style = MonoTextStyle::new(font, color(rgb));
    let layout = TextStyleBuilder::new()
        .alignment(align)
        .baseline(Baseline::Top)
        .build();
    let _ = Text::with_text_style(s, Point::new(x, y), style, layout).draw(frame);
}

fn rect(frame: &mut Frame, x: i32, y: i32, w: u32, h: u32, rgb: u32) {
    let _ = Rectangle::new(Point::new(x, y), Size::new(w, h))
        .into_styled(PrimitiveStyle::with_fill(color(rgb)))
        .draw(frame);
}

/// Cuts `s` to `chars` characters.
fn fit(s: &str, chars: usize) -> String {
    s.chars().take(chars).collect()
}

/// One encoder's column: name, value and a bar.
fn knob(frame: &mut Frame, i: i32, name: &str, value: &str, amount: f32, bipolar: bool, rgb: u32) {
    let x = i * COLUMN;
    let cx = x + COLUMN / 2;
    text(
        frame,
        &fit(name, 16),
        cx,
        34,
        &FONT_7X13,
        DIM,
        Alignment::Center,
    );
    text(
        frame,
        &fit(value, 11),
        cx,
        56,
        &FONT_10X20,
        TEXT,
        Alignment::Center,
    );
    let (bx, bw) = (x + 12, (COLUMN - 24) as u32);
    rect(frame, bx, 92, bw, 6, FAINT);
    let a = amount.clamp(0.0, 1.0);
    if bipolar {
        let mid = bx + bw as i32 / 2;
        let end = bx + (a * bw as f32) as i32;
        let (lo, hi) = if end < mid { (end, mid) } else { (mid, end) };
        rect(frame, lo, 92, (hi - lo).max(2) as u32, 6, rgb);
    } else {
        rect(frame, bx, 92, (a * bw as f32) as u32, 6, rgb);
    }
}

/// Draws what Push shows: the encoders' knobs in the middle, Stop labels
/// over the upper buttons, track names over the lower ones.
pub fn draw(c: &Controls, mode: Mode, encoders: Encoders) -> Frame {
    let mut f = Frame::new();
    let m = &c.monitor;
    let tracks = c.tracks.len();
    let selected = c.selected_track();

    // Upper row: Stop over each playing track.
    for t in 0..tracks.min(8) {
        if m.progress(t) >= 0.0 {
            let rgb = if m.stopping(t) { RED } else { DIM };
            text(
                &mut f,
                "STOP",
                t as i32 * COLUMN + COLUMN / 2,
                4,
                &FONT_6X10,
                rgb,
                Alignment::Center,
            );
        }
    }
    // Where we are, top right.
    let title = match (mode, encoders) {
        (Mode::Note, _) => format!("Note | {}", c.tracks[selected].name),
        (_, Encoders::Device) => {
            let name = &c.tracks[selected].name;
            format!("Device | {name}")
        }
        (_, Encoders::Mix(mix)) => format!("Mix | {}", mix.label()),
    };
    let beat = m.beat();
    let bar = (beat / 4.0).floor() as i64 + 1;
    let in_bar = beat.rem_euclid(4.0).floor() as i64 + 1;
    let status = format!(
        "{title}   {:.1} BPM   {bar}.{in_bar}{}",
        m.tempo(),
        if m.playing() { "" } else { "  stopped" }
    );
    text(
        &mut f,
        &status,
        WIDTH as i32 - 8,
        4,
        &FONT_6X10,
        ACCENT,
        Alignment::Right,
    );
    rect(&mut f, 0, 18, WIDTH as u32, 1, FAINT);

    // Encoders.
    match encoders {
        Encoders::Device => {
            let rgb = c.tracks[selected].color;
            for i in 0..PARAMS {
                knob(
                    &mut f,
                    i as i32,
                    c.param_name(selected, i),
                    &c.param_text(selected, i),
                    c.param(selected, i),
                    false,
                    rgb,
                );
            }
        }
        Encoders::Mix(mix) => {
            for (t, info) in c.tracks.iter().enumerate().take(8) {
                let v = c.mix(t, mix);
                let value = match mix {
                    Mix::Volume => format!("{} dB", fader_text(v)),
                    Mix::Pan => pan_text(v),
                    Mix::SendA | Mix::SendB => format!("{:.0}%", v * 100.0),
                };
                knob(
                    &mut f,
                    t as i32,
                    &info.name,
                    &value,
                    v,
                    mix == Mix::Pan,
                    info.color,
                );
            }
            if tracks < 8 {
                let v = c.main_volume();
                knob(
                    &mut f,
                    7,
                    "Main",
                    &format!("{} dB", fader_text(v)),
                    v,
                    false,
                    ACCENT,
                );
            }
        }
    }

    // Lower row: the tracks, the selected one filled.
    rect(&mut f, 0, 133, WIDTH as u32, 1, FAINT);
    for (t, info) in c.tracks.iter().enumerate().take(8) {
        let x = t as i32 * COLUMN;
        let playing = m.progress(t) >= 0.0;
        let label = format!("{}{}", if playing { "> " } else { "" }, fit(&info.name, 10));
        if t == selected {
            rect(&mut f, x + 4, 138, (COLUMN - 8) as u32, 20, info.color);
            text(
                &mut f,
                &label,
                x + COLUMN / 2,
                141,
                &FONT_9X15_BOLD,
                0x101112,
                Alignment::Center,
            );
        } else {
            text(
                &mut f,
                &label,
                x + COLUMN / 2,
                141,
                &FONT_9X15_BOLD,
                info.color,
                Alignment::Center,
            );
        }
    }
    f
}

/// The open screen.
pub struct Screen {
    handle: rusb::DeviceHandle<rusb::GlobalContext>,
}

impl Screen {
    /// Opens Push 2's screen, if one is plugged in.
    pub fn open() -> Option<Screen> {
        let handle = rusb::open_device_with_vid_pid(VENDOR, PRODUCT)?;
        handle.claim_interface(0).ok()?;
        Some(Screen { handle })
    }

    pub fn send(&self, frame: &Frame) -> rusb::Result<()> {
        let timeout = Duration::from_millis(1000);
        self.handle.write_bulk(ENDPOINT, &HEADER, timeout)?;
        let data = frame.encode();
        // Larger transfers are faster; 16 KB as in the manual's example.
        for chunk in data.chunks(16 * 1024) {
            self.handle.write_bulk(ENDPOINT, chunk, timeout)?;
        }
        Ok(())
    }
}

impl Drop for Screen {
    fn drop(&mut self) {
        let _ = self.handle.release_interface(0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_encode_as_push_expects() {
        let mut f = Frame::new();
        // Top-left pixel pure red: red is the low 5 bits.
        f.pixels[0] = color(0xff0000).into_storage();
        assert_eq!(f.pixels[0], 0x001F);
        let data = f.encode();
        assert_eq!(data.len(), 160 * 2048);
        // 0x1F ^ 0xE7, 0x00 ^ 0xF3; black elsewhere is the mask itself.
        assert_eq!(&data[..6], &[0x1F ^ 0xE7, 0xF3, 0xE7, 0xFF, 0xE7, 0xF3]);
        // Lines start on a fresh 2 KB boundary.
        assert_eq!(&data[2048..2052], &MASK);
    }

    #[test]
    fn draws_the_tracks_and_knobs() {
        let set = crate::clip::demo_set();
        let (tx, _rx) = rtrb::RingBuffer::new(8);
        let monitor = crate::engine::Studio::new(&set, 48_000.0, 120.0).1.monitor;
        let c = Controls::new(&set, &[0xff8a65, 0x4fc3f7, 0xb69cff, 0xf48fb1], tx, monitor);
        let f = draw(&c, Mode::Session, Encoders::Device);
        // The selected track's name sits on a fill of its colour.
        assert_eq!(f.pixels[150 * WIDTH + 8], color(0xff8a65).into_storage());
        assert!(f.pixels.iter().filter(|p| **p != 0).count() > 2000);
    }
}
