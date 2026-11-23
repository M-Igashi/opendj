//! Push 2 as a control surface: what each pad, button and encoder does, and
//! what each light should show. No I/O here: messages come in as bytes and
//! go out as bytes, so all of it can be tested without the hardware.
//!
//! Numbers from Ableton's "Push 2 MIDI and Display Interface Manual".

use crate::clip::{Sound, SCENES};
use crate::control::{Controls, Mix};
use crate::engine::SlotState;
use crate::sound::{DRUM_NOTES, PARAMS};
use crate::steps::{self, Layer, Stage, ROOT, SCALE};

/// Ableton's SysEx header for Push 2.
pub const SYSEX: [u8; 6] = [0xF0, 0x00, 0x21, 0x1D, 0x01, 0x01];

/// Takes Push 2 out of Live's hands: pads and buttons talk on the User port.
pub fn user_mode() -> Vec<u8> {
    sysex(&[0x0A, 0x01])
}

/// Gives Push 2 back to Live.
pub fn live_mode() -> Vec<u8> {
    sysex(&[0x0A, 0x00])
}

pub fn sysex(body: &[u8]) -> Vec<u8> {
    let mut m = SYSEX.to_vec();
    m.extend_from_slice(body);
    m.push(0xF7);
    m
}

// Pads: note 36 is the bottom left, 99 the top right.
pub const PAD_FIRST: u8 = 36;
pub const PAD_LAST: u8 = 99;

// Buttons (control changes).
pub const PLAY: u8 = 85;
pub const STOP: u8 = 29;
pub const SHIFT: u8 = 49;
pub const SESSION: u8 = 51;
pub const NOTE: u8 = 50;
pub const DEVICE: u8 = 110;
pub const MIX: u8 = 112;
pub const PAGE_LEFT: u8 = 62;
pub const PAGE_RIGHT: u8 = 63;
pub const LEFT: u8 = 44;
pub const RIGHT: u8 = 45;
pub const UP: u8 = 46;
pub const DOWN: u8 = 47;
pub const OCTAVE_DOWN: u8 = 54;
pub const OCTAVE_UP: u8 = 55;
pub const QUANTIZE: u8 = 116;
pub const SELECT: u8 = 48;
pub const DELETE: u8 = 118;
pub const LAYOUT: u8 = 31;
/// Scene buttons right of the pads, top (1/4) to bottom (1/32t).
pub const SCENE_FIRST: u8 = 36;
/// Buttons under the display, left to right.
pub const LOWER_FIRST: u8 = 20;
/// Buttons over the display, left to right.
pub const UPPER_FIRST: u8 = 102;
/// Encoders over the display, left to right.
pub const ENCODER_FIRST: u8 = 71;
pub const MASTER_ENCODER: u8 = 79;
pub const TEMPO_ENCODER: u8 = 14;

// Colour palette entries this surface writes, clear of the white
// palette's usual values.
pub const fn track_color(t: usize) -> u8 {
    64 + (t % 8) as u8
}
pub const fn track_dim(t: usize) -> u8 {
    72 + (t % 8) as u8
}
pub const WHITE: u8 = 80;
pub const GREY: u8 = 81;
pub const AMBER: u8 = 82;
pub const GREEN: u8 = 83;
pub const RED: u8 = 84;
pub const RED_DIM: u8 = 85;
pub const SOFT: u8 = 86;

/// White-light brightness.
const W_OFF: u8 = 0;
const W_DIM: u8 = 16;
const W_ON: u8 = 127;

/// LED animation, as the MIDI channel that asks for it.
const PULSE_QUARTER: u8 = 9;
const BLINK_EIGHTH: u8 = 13;

/// SysEx messages writing this surface's palette, then reapplying it.
pub fn palette(track_colors: &[u32]) -> Vec<Vec<u8>> {
    let rgb = |c: u32| ((c >> 16) as u8, (c >> 8) as u8, c as u8);
    let scale = |(r, g, b): (u8, u8, u8), k: f32| {
        let f = |x: u8| (x as f32 * k) as u8;
        (f(r), f(g), f(b))
    };
    let mut entries: Vec<(u8, (u8, u8, u8))> = Vec::new();
    for t in 0..8 {
        let c = rgb(track_colors.get(t).copied().unwrap_or(0x808080));
        entries.push((track_color(t), c));
        entries.push((track_dim(t), scale(c, 0.16)));
    }
    entries.extend([
        (WHITE, (230, 230, 230)),
        (GREY, (40, 40, 40)),
        (AMBER, (255, 160, 0)),
        (GREEN, (60, 255, 90)),
        (RED, (255, 40, 30)),
        (RED_DIM, (60, 8, 6)),
        (SOFT, (110, 110, 110)),
    ]);
    let mut out: Vec<Vec<u8>> = entries
        .into_iter()
        .map(|(i, (r, g, b))| {
            // The white LEDs' level for the entry: its brightness.
            let w = ((r as u32 + g as u32 + b as u32) / 3) as u8;
            sysex(&[
                0x03,
                i,
                r & 0x7F,
                r >> 7,
                g & 0x7F,
                g >> 7,
                b & 0x7F,
                b >> 7,
                w & 0x7F,
                w >> 7,
            ])
        })
        .collect();
    out.push(sysex(&[0x05]));
    out
}

/// What the pads do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// 8 tracks by 8 scenes of clips.
    Session,
    /// The selected track's instrument: drum pads or a keyboard in key.
    Note,
    /// The selected stage clip: a column per stage, the scene buttons
    /// pick what the pads set.
    Steps,
}

/// What the eight encoders over the display turn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Encoders {
    /// The selected track's instrument knobs.
    Device,
    /// One mixer control across the tracks.
    Mix(Mix),
}

/// One light: a colour, and optionally an animation towards a second one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Led {
    color: u8,
    /// MIDI channel of the animation, 0 for none.
    anim: u8,
    to: u8,
}

impl Led {
    const OFF: Led = Led {
        color: 0,
        anim: 0,
        to: 0,
    };

    fn solid(color: u8) -> Led {
        Led {
            color,
            anim: 0,
            to: 0,
        }
    }

    fn animate(color: u8, anim: u8, to: u8) -> Led {
        Led { color, anim, to }
    }
}

pub struct Surface {
    pub mode: Mode,
    pub encoders: Encoders,
    shift: bool,
    /// Octaves up or down from each sound's home range in Note mode.
    octave: i32,
    /// Pitch held by each pad in Note mode, to end it on release.
    held: [Option<(usize, u8)>; 64],
    /// What each light was last told; None until first sent.
    pads: [Option<Led>; 64],
    buttons: [Option<Led>; 128],
    /// The encoder a finger rests on.
    touched: Option<usize>,
    pub layer: Layer,
    select: bool,
    /// Steps mode pads held down, by pad index.
    down: u64,
    /// Held pads already used (for locks or a semitone): their release
    /// sets nothing.
    used: u64,
}

impl Default for Surface {
    fn default() -> Self {
        Surface::new()
    }
}

impl Surface {
    pub fn new() -> Surface {
        Surface {
            mode: Mode::Session,
            encoders: Encoders::Device,
            shift: false,
            octave: 0,
            held: [None; 64],
            pads: [None; 64],
            buttons: [None; 128],
            touched: None,
            layer: Layer::Gate,
            select: false,
            down: 0,
            used: 0,
        }
    }

    /// The stages (columns) held down in Steps mode, lowest first.
    pub fn held_stages(&self) -> Vec<usize> {
        (0..8)
            .filter(|col| (0..8).any(|row| self.down & (1 << (row * 8 + col)) != 0))
            .collect()
    }

    /// The encoder (0..8) a finger rests on, if any.
    pub fn touched(&self) -> Option<usize> {
        self.touched
    }

    /// Forgets what the lights show, so the next [`Surface::lights`] sends
    /// them all (after a reconnect).
    pub fn reset_lights(&mut self) {
        self.pads = [None; 64];
        self.buttons = [None; 128];
    }

    /// Handles one message from Push.
    pub fn input(&mut self, msg: &[u8], c: &Controls) {
        let [status, data1, data2] = match msg {
            [s, a, b, ..] => [*s, *a, *b],
            _ => return,
        };
        match status & 0xF0 {
            0x90 | 0x80 if (PAD_FIRST..=PAD_LAST).contains(&data1) => {
                let velocity = if status & 0xF0 == 0x80 { 0 } else { data2 };
                self.pad((data1 - PAD_FIRST) as usize, velocity, c);
            }
            // Encoder touch: notes 0-7.
            0x90 | 0x80 if data1 < 8 => {
                let i = data1 as usize;
                if status & 0xF0 == 0x90 && data2 > 0 {
                    self.touched = Some(i);
                } else if self.touched == Some(i) {
                    self.touched = None;
                }
            }
            0xB0 => {
                if (ENCODER_FIRST..ENCODER_FIRST + 8).contains(&data1)
                    || data1 == MASTER_ENCODER
                    || data1 == TEMPO_ENCODER
                {
                    // Relative: 1..63 right, 127..64 left.
                    let delta = if data2 < 64 {
                        data2 as i32
                    } else {
                        data2 as i32 - 128
                    };
                    self.encoder(data1, delta, c);
                } else {
                    self.button(data1, data2 > 0, c);
                }
            }
            _ => {}
        }
    }

    fn pad(&mut self, index: usize, velocity: u8, c: &Controls) {
        let (row, col) = (index / 8, index % 8);
        match self.mode {
            Mode::Session => {
                if velocity == 0 {
                    return;
                }
                let (track, scene) = (col, 7 - row);
                if track >= c.tracks.len() || scene >= SCENES {
                    return;
                }
                if self.shift {
                    c.select(track, Some(scene));
                } else {
                    c.launch(track, scene);
                }
            }
            Mode::Note => {
                if velocity == 0 {
                    if let Some((track, pitch)) = self.held[index].take() {
                        c.note(track, pitch, 0.0);
                    }
                    return;
                }
                let track = c.selected_track();
                if let Some(pitch) = self.note_pitch(c, track, row, col) {
                    if let Some((t, p)) = self.held[index].take() {
                        c.note(t, p, 0.0);
                    }
                    self.held[index] = Some((track, pitch));
                    c.note(track, pitch, velocity as f32 / 127.0);
                }
            }
            Mode::Steps => self.step_pad(index, velocity > 0, c),
        }
    }

    fn step_pad(&mut self, index: usize, down: bool, c: &Controls) {
        let (row, col) = (index / 8, index % 8);
        let bit = 1u64 << index;
        let (t, s) = c.steps_slot();
        if down {
            self.down |= bit;
            // Two neighbouring pitch pads: the semitone between.
            if self.layer == Layer::Pitch {
                let below = row > 0 && self.down & (bit >> 8) != 0;
                let above = row < 7 && self.down & (bit << 8) != 0;
                if below || above {
                    let low = if below { row - 1 } else { row };
                    c.edit_stages(t, s, |st| {
                        let stage = &mut st.stages[col];
                        stage.degree = low as u8;
                        stage.sharp = true;
                    });
                    self.used |= bit | if below { bit >> 8 } else { bit << 8 };
                }
            }
            return;
        }
        self.down &= !bit;
        if self.used & bit != 0 {
            self.used &= !bit;
            return;
        }
        let (layer, select, shift) = (self.layer, self.select, self.shift);
        c.edit_stages(t, s, |st| {
            steps::press(&mut st.stages[col], layer, row as u8, select, shift)
        });
    }

    /// The note pad (`row` from the bottom, `col`) plays on `track`.
    fn note_pitch(&self, c: &Controls, track: usize, row: usize, col: usize) -> Option<u8> {
        let sound = c.tracks.get(track)?.sound;
        if let Sound::Drums(_) = sound {
            // A 4x4 block in the bottom left.
            if row >= 4 || col >= 4 {
                return None;
            }
            return DRUM_NOTES.get(row * 4 + col).copied();
        }
        let home: i32 = match sound {
            Sound::Bass => 33,
            Sound::Pad => 45,
            _ => 57,
        };
        // In key: a scale step to the right, a fourth (three steps) up.
        let degree = row * 3 + col;
        let pitch = home + self.octave * 12 + SCALE[degree % 7] as i32 + 12 * (degree / 7) as i32;
        u8::try_from(pitch).ok().filter(|p| *p < 128)
    }

    fn button(&mut self, cc: u8, down: bool, c: &Controls) {
        if cc == SHIFT {
            self.shift = down;
            return;
        }
        if cc == SELECT {
            self.select = down;
            return;
        }
        if !down {
            return;
        }
        let tracks = c.tracks.len();
        match cc {
            PLAY => c.toggle_play(),
            STOP => c.stop_all(),
            SESSION => self.set_mode(Mode::Session, c),
            // Again (or Layout): between the keyboard and the stages.
            NOTE | LAYOUT => {
                let next = match (cc, self.mode) {
                    (_, Mode::Note) => Mode::Steps,
                    (LAYOUT, Mode::Session) => Mode::Session,
                    _ => Mode::Note,
                };
                self.set_mode(next, c);
            }
            DELETE => {
                // Clears the held stages' locks.
                let held = self.held_stages();
                if !held.is_empty() {
                    let (t, s) = c.steps_slot();
                    c.edit_stages(t, s, |st| {
                        for &i in &held {
                            st.stages[i].locks = [None; PARAMS];
                        }
                    });
                    self.used |= self.down;
                }
            }
            DEVICE => self.encoders = Encoders::Device,
            MIX => {
                // Again: the next mixer control.
                self.encoders = match self.encoders {
                    Encoders::Mix(m) => Encoders::Mix(Mix::ALL[(m as usize + 1) % 4]),
                    Encoders::Device => Encoders::Mix(Mix::Volume),
                };
            }
            PAGE_LEFT | PAGE_RIGHT => {
                if let Encoders::Mix(m) = self.encoders {
                    let step = if cc == PAGE_RIGHT { 1 } else { 3 };
                    self.encoders = Encoders::Mix(Mix::ALL[(m as usize + step) % 4]);
                }
            }
            LEFT | RIGHT => {
                let t = c.selected_track();
                let next = if cc == RIGHT {
                    (t + 1).min(tracks.saturating_sub(1))
                } else {
                    t.saturating_sub(1)
                };
                self.set_track(next, c);
            }
            UP | DOWN => {
                let next = match (c.selected_scene(), cc) {
                    (None, _) => 0,
                    (Some(s), DOWN) => (s + 1).min(SCENES - 1),
                    (Some(s), _) => s.saturating_sub(1),
                };
                c.select(c.selected_track(), Some(next));
            }
            OCTAVE_UP => self.octave = (self.octave + 1).min(3),
            OCTAVE_DOWN => self.octave = (self.octave - 1).max(-3),
            QUANTIZE => {
                c.next_quantize();
            }
            cc if (SCENE_FIRST..SCENE_FIRST + 8).contains(&cc) => {
                let scene = (cc - SCENE_FIRST) as usize;
                if self.mode == Mode::Steps && !self.shift {
                    if let Some(&l) = Layer::ALL.get(scene) {
                        self.layer = l;
                    }
                } else if self.shift {
                    c.select(c.selected_track(), Some(scene));
                } else {
                    c.launch_scene(scene);
                }
            }
            cc if (LOWER_FIRST..LOWER_FIRST + 8).contains(&cc) => {
                let t = (cc - LOWER_FIRST) as usize;
                if t < tracks {
                    self.set_track(t, c);
                }
            }
            cc if (UPPER_FIRST..UPPER_FIRST + 8).contains(&cc) => {
                let t = (cc - UPPER_FIRST) as usize;
                if t < tracks {
                    c.stop(t);
                }
            }
            _ => {}
        }
    }

    fn set_mode(&mut self, mode: Mode, c: &Controls) {
        self.end_notes(c);
        self.down = 0;
        self.used = 0;
        self.mode = mode;
    }

    fn set_track(&mut self, track: usize, c: &Controls) {
        self.end_notes(c);
        self.down = 0;
        self.used = 0;
        c.select_track(track);
    }

    /// Ends notes held from Note mode.
    pub fn end_notes(&mut self, c: &Controls) {
        for h in self.held.iter_mut() {
            if let Some((t, p)) = h.take() {
                c.note(t, p, 0.0);
            }
        }
    }

    fn encoder(&mut self, cc: u8, delta: i32, c: &Controls) {
        // About 210 steps a turn: a full sweep in one turn, finer with Shift.
        let step = if self.shift { 0.001 } else { 0.005 } * delta as f32;
        match cc {
            TEMPO_ENCODER => c.tempo_step(if self.shift { 0.1 } else { 1.0 } * delta as f64),
            MASTER_ENCODER => c.set_main_volume(c.main_volume() + step),
            _ => {
                let i = (cc - ENCODER_FIRST) as usize;
                let held = self.held_stages();
                if self.mode == Mode::Steps && !held.is_empty() {
                    // Held stages: a lock on each, from where it was.
                    if i < PARAMS {
                        let (t, s) = c.steps_slot();
                        let now = c.param(t, i);
                        c.edit_stages(t, s, |st| {
                            for &h in &held {
                                let l = &mut st.stages[h].locks[i];
                                *l = Some((l.unwrap_or(now) + step).clamp(0.0, 1.0));
                            }
                        });
                    }
                    self.used |= self.down;
                    return;
                }
                match self.encoders {
                    Encoders::Device => {
                        if i < PARAMS {
                            let t = c.selected_track();
                            c.set_param(t, i, c.param(t, i) + step);
                        }
                    }
                    Encoders::Mix(m) => {
                        if i < c.tracks.len() {
                            c.set_mix(i, m, c.mix(i, m) + step);
                        }
                    }
                }
            }
        }
    }

    /// The messages that bring every light up to date; only changes are
    /// sent.
    pub fn lights(&mut self, c: &Controls) -> Vec<[u8; 3]> {
        let mut out = Vec::new();
        let stages = match self.mode {
            Mode::Steps => {
                let (t, s) = c.steps_slot();
                c.stages(t, s)
            }
            _ => None,
        };
        for index in 0..64 {
            let led = self.pad_led(index, c, stages.as_ref());
            if self.pads[index] != Some(led) {
                self.pads[index] = Some(led);
                send(&mut out, 0x90, PAD_FIRST + index as u8, led);
            }
        }
        for (cc, led) in self.button_leds(c) {
            let slot = &mut self.buttons[cc as usize];
            if *slot != Some(led) {
                *slot = Some(led);
                send(&mut out, 0xB0, cc, led);
            }
        }
        out
    }

    fn pad_led(&self, index: usize, c: &Controls, stages: Option<&steps::Stages>) -> Led {
        let (row, col) = (index / 8, index % 8);
        let m = &c.monitor;
        match self.mode {
            Mode::Session => {
                let (t, s) = (col, 7 - row);
                if t >= c.tracks.len() {
                    return Led::OFF;
                }
                match m.slot(t, s) {
                    // Before the audio thread's first report.
                    SlotState::Empty if c.has_clip(t, s) => Led::solid(track_dim(t)),
                    SlotState::Empty => Led::OFF,
                    SlotState::Stopped => Led::solid(track_dim(t)),
                    SlotState::Queued => Led::animate(track_dim(t), BLINK_EIGHTH, GREEN),
                    SlotState::Playing if m.stopping(t) => {
                        Led::animate(track_color(t), BLINK_EIGHTH, RED)
                    }
                    SlotState::Playing => Led::animate(track_color(t), PULSE_QUARTER, WHITE),
                }
            }
            Mode::Note => {
                let t = c.selected_track();
                if self.held[index].is_some() {
                    return Led::solid(GREEN);
                }
                let Some(pitch) = self.note_pitch(c, t, row, col) else {
                    return Led::OFF;
                };
                if c.tracks[t].drums() || (pitch + 12 - ROOT).is_multiple_of(12) {
                    Led::solid(track_color(t))
                } else {
                    Led::solid(SOFT)
                }
            }
            Mode::Steps => {
                if self.down & (1 << index) != 0 {
                    return Led::solid(GREEN);
                }
                let (t, s) = c.steps_slot();
                let Some(st) = stages else {
                    return Led::OFF;
                };
                let playing = m.slot(t, s) == SlotState::Playing && m.stage(t) == Some(col);
                step_led(&st.stages[col], self.layer, row as u8, t, playing)
            }
        }
    }

    fn button_leds(&self, c: &Controls) -> Vec<(u8, Led)> {
        let m = &c.monitor;
        let tracks = c.tracks.len();
        let white = |on: bool| Led::solid(if on { W_ON } else { W_DIM });
        let mut v = Vec::with_capacity(40);
        v.push((PLAY, Led::solid(if m.playing() { GREEN } else { GREY })));
        v.push((STOP, Led::solid(GREY)));
        for s in 0..8 {
            if self.mode == Mode::Steps {
                let led = match Layer::ALL.get(s) {
                    Some(&l) if l == self.layer => Led::solid(WHITE),
                    Some(_) => Led::solid(SOFT),
                    None => Led::OFF,
                };
                v.push((SCENE_FIRST + s as u8, led));
                continue;
            }
            let playing = (0..tracks).any(|t| m.slot(t, s) == SlotState::Playing);
            let queued = (0..tracks).any(|t| m.slot(t, s) == SlotState::Queued);
            let any = (0..tracks).any(|t| c.has_clip(t, s));
            let led = if queued {
                Led::animate(GREY, BLINK_EIGHTH, GREEN)
            } else if playing {
                Led::solid(GREEN)
            } else if any {
                Led::solid(GREY)
            } else {
                Led::OFF
            };
            v.push((SCENE_FIRST + s as u8, led));
        }
        let selected = c.selected_track();
        for t in 0..8 {
            let lower = if t >= tracks {
                Led::OFF
            } else if t == selected {
                Led::solid(track_color(t))
            } else {
                Led::solid(track_dim(t))
            };
            v.push((LOWER_FIRST + t as u8, lower));
            let playing = t < tracks && m.progress(t) >= 0.0;
            let upper = if !playing {
                Led::OFF
            } else if m.stopping(t) {
                Led::animate(RED_DIM, BLINK_EIGHTH, RED)
            } else {
                Led::solid(RED_DIM)
            };
            v.push((UPPER_FIRST + t as u8, upper));
        }
        v.push((SESSION, white(self.mode == Mode::Session)));
        v.push((NOTE, white(matches!(self.mode, Mode::Note | Mode::Steps))));
        let steps = self.mode == Mode::Steps;
        v.push((
            LAYOUT,
            Led::solid(if steps || self.mode == Mode::Note {
                W_DIM
            } else {
                W_OFF
            }),
        ));
        v.push((
            SELECT,
            Led::solid(if self.select {
                W_ON
            } else if steps {
                W_DIM
            } else {
                W_OFF
            }),
        ));
        v.push((
            DELETE,
            Led::solid(if steps && self.down != 0 { W_ON } else { W_OFF }),
        ));
        v.push((DEVICE, white(self.encoders == Encoders::Device)));
        v.push((MIX, white(matches!(self.encoders, Encoders::Mix(_)))));
        v.push((SHIFT, white(self.shift)));
        let mix = matches!(self.encoders, Encoders::Mix(_));
        for cc in [PAGE_LEFT, PAGE_RIGHT] {
            v.push((cc, Led::solid(if mix { W_DIM } else { W_OFF })));
        }
        for cc in [LEFT, RIGHT, UP, DOWN, QUANTIZE] {
            v.push((cc, Led::solid(W_DIM)));
        }
        let note = self.mode == Mode::Note;
        for cc in [OCTAVE_DOWN, OCTAVE_UP] {
            v.push((cc, Led::solid(if note { W_DIM } else { W_OFF })));
        }
        v
    }

    /// Messages turning every light off, for when the studio lets go.
    pub fn all_off() -> Vec<[u8; 3]> {
        let mut out = Vec::new();
        for n in PAD_FIRST..=PAD_LAST {
            out.push([0x90, n, 0]);
        }
        for cc in 0..128u8 {
            out.push([0xB0, cc, 0]);
        }
        out
    }
}

/// A Steps mode pad: `row` (from the bottom) of `stage` on `layer`.
fn step_led(stage: &Stage, layer: Layer, row: u8, t: usize, playing: bool) -> Led {
    let on = Led::solid(if playing { WHITE } else { track_color(t) });
    let dim = Led::solid(track_dim(t));
    // A bar up to `level` rows (1..=8).
    let bar = |level: u8| {
        if row + 1 == level {
            on
        } else if row + 1 < level {
            dim
        } else {
            Led::OFF
        }
    };
    if stage.skip && layer != Layer::Chance {
        return if row == 0 {
            Led::solid(RED_DIM)
        } else {
            Led::OFF
        };
    }
    match layer {
        Layer::Pitch => {
            if row == stage.degree {
                if stage.sharp {
                    Led::solid(AMBER)
                } else {
                    on
                }
            } else if stage.sharp && row == stage.degree + 1 {
                Led::solid(AMBER)
            } else {
                Led::OFF
            }
        }
        Layer::Octave => {
            let at = (stage.octave + 3).clamp(0, 7) as u8;
            if row == at {
                on
            } else if row == 3 {
                Led::solid(GREY)
            } else {
                Led::OFF
            }
        }
        Layer::Velocity => bar((stage.velocity * 8.0).round().clamp(1.0, 8.0) as u8),
        Layer::Gate => {
            if row >= stage.pulses {
                Led::OFF
            } else if stage.gate(row)
                || (stage.tie && stage.gates != 0 && row > stage.gates.trailing_zeros() as u8)
            {
                if stage.tie && !stage.gate(row) {
                    dim
                } else {
                    on
                }
            } else {
                Led::solid(GREY)
            }
        }
        Layer::Chance => {
            if row == 0 {
                Led::solid(if stage.skip { RED } else { GREY })
            } else if stage.skip {
                Led::OFF
            } else {
                let level = (stage.chance * 7.0).round() as u8 + 1;
                bar(level)
            }
        }
        Layer::Locks => {
            let level = (stage.lock_amount * 7.0).round() as u8 + 1;
            if !stage.has_locks() {
                if row < level {
                    Led::solid(GREY)
                } else {
                    Led::OFF
                }
            } else if row + 1 == level {
                Led::solid(if playing { track_color(t) } else { WHITE })
            } else if row + 1 < level {
                Led::solid(SOFT)
            } else {
                Led::OFF
            }
        }
    }
}

fn send(out: &mut Vec<[u8; 3]>, kind: u8, number: u8, led: Led) {
    // Channel 0 sets the colour and stops any animation; another channel
    // then starts one towards the second colour.
    out.push([kind, number, led.color]);
    if led.anim != 0 {
        out.push([kind | led.anim, number, led.to]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{Command, Studio};

    fn controls() -> (Controls, rtrb::Consumer<Command>) {
        let set = crate::clip::demo_set();
        let (tx, rx) = rtrb::RingBuffer::new(64);
        let monitor = Studio::new(&set, 48_000.0, 120.0).1.monitor;
        (Controls::new(&set, &[0xff0000; 4], tx, monitor), rx)
    }

    #[test]
    fn top_left_pad_launches_track_one_scene_one() {
        let (c, mut rx) = controls();
        let mut s = Surface::new();
        // Top-left pad: note 92.
        s.input(&[0x90, 92, 100], &c);
        assert!(matches!(
            rx.pop(),
            Ok(Command::Launch { track: 0, scene: 0 })
        ));
        // Bottom-right of the four tracks: note 39, scene 8.
        s.input(&[0x90, 39, 100], &c);
        assert!(matches!(
            rx.pop(),
            Ok(Command::Launch { track: 3, scene: 7 })
        ));
        // Columns past the last track do nothing.
        s.input(&[0x90, 40, 100], &c);
        assert!(rx.pop().is_err());
    }

    #[test]
    fn scene_buttons_and_stops() {
        let (c, mut rx) = controls();
        let mut s = Surface::new();
        s.input(&[0xB0, SCENE_FIRST + 2, 127], &c);
        assert!(matches!(rx.pop(), Ok(Command::Scene(2))));
        // Release does nothing.
        s.input(&[0xB0, SCENE_FIRST + 2, 0], &c);
        assert!(rx.pop().is_err());
        s.input(&[0xB0, UPPER_FIRST + 1, 127], &c);
        assert!(matches!(rx.pop(), Ok(Command::Stop(1))));
        s.input(&[0xB0, STOP, 127], &c);
        assert!(matches!(rx.pop(), Ok(Command::StopAll)));
    }

    #[test]
    fn encoders_turn_device_knobs_and_mixer() {
        let (c, mut rx) = controls();
        let mut s = Surface::new();
        s.input(&[0xB0, LOWER_FIRST + 1, 127], &c);
        assert_eq!(c.selected_track(), 1);
        let before = c.param(1, 0);
        s.input(&[0xB0, ENCODER_FIRST, 4], &c);
        assert!((c.param(1, 0) - before - 0.02).abs() < 1e-5);
        assert!(matches!(
            rx.pop(),
            Ok(Command::Param {
                track: 1,
                index: 0,
                ..
            })
        ));
        // Mix, then left turns on encoder 3: track 3's volume down.
        s.input(&[0xB0, MIX, 127], &c);
        let before = c.mix(2, Mix::Volume);
        s.input(&[0xB0, ENCODER_FIRST + 2, 126], &c);
        assert!((before - c.mix(2, Mix::Volume) - 0.01).abs() < 1e-5);
        assert!(matches!(rx.pop(), Ok(Command::Volume { track: 2, .. })));
        // Mix again: pan.
        s.input(&[0xB0, MIX, 127], &c);
        assert_eq!(s.encoders, Encoders::Mix(Mix::Pan));
    }

    #[test]
    fn note_mode_plays_and_releases() {
        let (c, mut rx) = controls();
        let mut s = Surface::new();
        // Drums are track 1: bottom-left pad is the kick.
        s.input(&[0xB0, NOTE, 127], &c);
        s.input(&[0x90, 36, 127], &c);
        assert!(matches!(
            rx.pop(),
            Ok(Command::Note {
                track: 0,
                pitch: 36,
                ..
            })
        ));
        s.input(&[0x80, 36, 0], &c);
        assert!(matches!(
            rx.pop(),
            Ok(Command::Note { track: 0, pitch: 36, velocity }) if velocity == 0.0
        ));
        // Bass: bottom-left is A1, the next pad up a fourth (D2).
        s.input(&[0xB0, LOWER_FIRST + 1, 127], &c);
        s.input(&[0x90, 44, 90], &c);
        assert!(matches!(
            rx.pop(),
            Ok(Command::Note {
                track: 1,
                pitch: 38,
                ..
            })
        ));
    }

    #[test]
    fn lights_only_send_changes() {
        let (c, _rx) = controls();
        let mut s = Surface::new();
        let first = s.lights(&c);
        // Track 1 scene 1 holds a clip: its pad (92) is lit dim in the
        // track's colour.
        assert!(first.contains(&[0x90, 92, track_dim(0)]));
        assert!(first.contains(&[0xB0, PLAY, GREY]));
        assert!(s.lights(&c).is_empty());
        s.input(&[0xB0, NOTE, 127], &c);
        assert!(!s.lights(&c).is_empty());
    }

    #[test]
    fn palette_messages_are_valid_sysex() {
        let msgs = palette(&[0xff8a65, 0x4fc3f7]);
        assert_eq!(msgs.len(), 16 + 7 + 1);
        for m in &msgs {
            assert_eq!(&m[..6], &SYSEX);
            assert_eq!(*m.last().unwrap(), 0xF7);
            assert!(m[1..m.len() - 1].iter().all(|b| *b < 0x80));
        }
        // Entry 64, track 1: 0xff8a65 = 255, 138, 101.
        assert_eq!(
            &msgs[0][6..16],
            &[0x03, 64, 0x7F, 1, 0x0A, 1, 101, 0, 0x24, 1]
        );
        assert_eq!(user_mode(), vec![0xF0, 0, 0x21, 0x1D, 1, 1, 0x0A, 1, 0xF7]);
    }

    #[test]
    fn steps_mode_edits_and_locks() {
        let (c, _rx) = controls();
        let mut s = Surface::new();
        let press = |s: &mut Surface, cc: u8| {
            s.input(&[0xB0, cc, 127], &c);
            s.input(&[0xB0, cc, 0], &c);
        };
        let pad = |s: &mut Surface, row: u8, col: u8, down: bool| {
            s.input(
                &[0x90, PAD_FIRST + row * 8 + col, if down { 100 } else { 0 }],
                &c,
            );
        };
        // Lead (track 3), scene 8 is empty: Steps makes a clip there.
        press(&mut s, LOWER_FIRST + 3);
        s.input(&[0xB0, SHIFT, 127], &c);
        press(&mut s, SCENE_FIRST + 7);
        s.input(&[0xB0, SHIFT, 0], &c);
        press(&mut s, NOTE);
        press(&mut s, NOTE);
        assert_eq!(s.mode, Mode::Steps);
        assert!(!c.has_clip(3, 7));
        press(&mut s, SCENE_FIRST + 3);
        assert_eq!(s.layer, Layer::Gate);
        pad(&mut s, 2, 0, true);
        pad(&mut s, 2, 0, false);
        let st = c.stages(3, 7).expect("stage clip");
        assert_eq!((st.stages[0].pulses, st.stages[0].gates), (3, 0b100));
        assert!(c.has_clip(3, 7));
        // Hold stage 2 and turn the first encoder: a lock, and the release
        // sets nothing.
        pad(&mut s, 0, 1, true);
        s.input(&[0xB0, ENCODER_FIRST, 10], &c);
        pad(&mut s, 0, 1, false);
        let st = c.stages(3, 7).unwrap();
        let base = c.param(3, 0);
        assert!((st.stages[1].locks[0].unwrap() - (base + 0.05)).abs() < 1e-4);
        assert_eq!(st.stages[1].gates, 0);
        // Hold and Delete clears it.
        pad(&mut s, 0, 1, true);
        press(&mut s, DELETE);
        pad(&mut s, 0, 1, false);
        assert!(!c.stages(3, 7).unwrap().stages[1].has_locks());
        // A note clip isn't touched.
        s.input(&[0xB0, SHIFT, 127], &c);
        press(&mut s, SCENE_FIRST + 3);
        s.input(&[0xB0, SHIFT, 0], &c);
        assert!(c.stages(3, 3).is_none());
        pad(&mut s, 0, 0, true);
        pad(&mut s, 0, 0, false);
        assert!(c.stages(3, 3).is_none());
        let lights = s.lights(&c);
        assert!(lights.contains(&[0x90, PAD_FIRST, 0]));
    }
}
