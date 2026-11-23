//! The studio's built-in sounds: a drum kit from openDJ's Groove Box and a
//! small polyphonic synth.
//!
//! Both are made ready before the audio thread starts and never allocate
//! while playing: voices live in fixed arrays and are stolen when all are
//! busy.

use std::f32::consts::PI;
use std::sync::Arc;

use opendj_engine::kits;
use opendj_engine::sampler::Sample;

/// A track's instrument.
pub enum Instrument {
    Drums(Box<Drums>),
    Synth(Box<Synth>),
}

impl Instrument {
    pub fn note_on(&mut self, pitch: u8, velocity: f32) {
        match self {
            Instrument::Drums(d) => d.note_on(pitch, velocity),
            Instrument::Synth(s) => s.note_on(pitch, velocity),
        }
    }

    pub fn note_off(&mut self, pitch: u8) {
        if let Instrument::Synth(s) = self {
            s.note_off(pitch);
        }
    }

    /// Releases every sounding note (a clip stopped).
    pub fn all_off(&mut self) {
        match self {
            Instrument::Drums(_) => {}
            Instrument::Synth(s) => s.all_off(),
        }
    }

    /// Adds `out.len()` frames of sound to `out`.
    pub fn render(&mut self, out: &mut [[f32; 2]]) {
        match self {
            Instrument::Drums(d) => d.render(out),
            Instrument::Synth(s) => s.render(out),
        }
    }
}

// ---------------------------------------------------------------- drums

/// General MIDI drum notes for the kit's nine roles (see [`kits::ROLES`]):
/// kick, snare, closed hat, open hat, clap, rim, cowbell, crash, ride.
pub const DRUM_NOTES: [u8; 9] = [36, 38, 42, 46, 39, 37, 56, 49, 51];

pub const KICK: u8 = 36;
pub const SNARE: u8 = 38;
pub const CLOSED_HAT: u8 = 42;
pub const OPEN_HAT: u8 = 46;
pub const CLAP: u8 = 39;
pub const RIM: u8 = 37;

const DRUM_VOICES: usize = 24;

#[derive(Clone, Copy, Default)]
struct DrumVoice {
    sound: usize,
    pos: usize,
    gain: f32,
    /// Fading out after a choke (open hat cut by a closed hat).
    fade: f32,
    active: bool,
}

/// One Groove Box kit, played by General MIDI drum notes.
pub struct Drums {
    sounds: Vec<Arc<Sample>>,
    voices: [DrumVoice; DRUM_VOICES],
    gain: f32,
}

impl Drums {
    /// Kit `name` (one of [`kits::KITS`]) at the engine's `rate`.
    pub fn new(name: &str, rate: f64, gain: f32) -> Drums {
        let all = kits::build(rate);
        let kit = all
            .into_iter()
            .find(|k| k.name == name)
            .expect("kit exists");
        Drums {
            sounds: kit.sounds.iter().map(|s| s.sample.clone()).collect(),
            voices: [DrumVoice::default(); DRUM_VOICES],
            gain,
        }
    }

    fn note_on(&mut self, pitch: u8, velocity: f32) {
        let Some(sound) = DRUM_NOTES.iter().position(|&n| n == pitch) else {
            return;
        };
        if pitch == CLOSED_HAT {
            let open = DRUM_NOTES.iter().position(|&n| n == OPEN_HAT);
            for v in self.voices.iter_mut().filter(|v| v.active) {
                if Some(v.sound) == open {
                    v.fade = v.fade.clamp(0.999, 1.0);
                }
            }
        }
        let slot = self
            .voices
            .iter()
            .position(|v| !v.active)
            .unwrap_or_else(|| {
                // Steal the voice that has played longest.
                (0..DRUM_VOICES)
                    .max_by_key(|&i| self.voices[i].pos)
                    .unwrap_or(0)
            });
        self.voices[slot] = DrumVoice {
            sound,
            pos: 0,
            gain: velocity * self.gain,
            fade: 2.0,
            active: true,
        };
    }

    fn render(&mut self, out: &mut [[f32; 2]]) {
        for v in self.voices.iter_mut().filter(|v| v.active) {
            let sample = &self.sounds[v.sound];
            let len = sample.len();
            for frame in out.iter_mut() {
                if v.pos >= len || v.fade < 1e-4 {
                    v.active = false;
                    break;
                }
                let [l, r] = sample.frame(v.pos);
                let g = v.gain * v.fade.min(1.0);
                frame[0] += l * g;
                frame[1] += r * g;
                v.pos += 1;
                if v.fade < 1.5 {
                    v.fade *= 0.999;
                }
            }
        }
    }
}

// ---------------------------------------------------------------- synth

/// How a synth track sounds.
#[derive(Clone, Copy, Debug)]
pub struct Preset {
    pub attack: f32,
    pub decay: f32,
    pub sustain: f32,
    pub release: f32,
    /// Filter cutoff with the filter envelope closed, in Hz.
    pub cutoff: f32,
    /// How far the filter envelope opens it, in Hz.
    pub env_amount: f32,
    pub filter_decay: f32,
    /// 0 (none) to 1 (self-oscillating).
    pub resonance: f32,
    /// Second saw's detune, in cents.
    pub detune: f32,
    /// Square-wave sub oscillator an octave down.
    pub sub: f32,
    pub gain: f32,
    /// One note at a time, the newest wins.
    pub mono: bool,
}

pub const BASS: Preset = Preset {
    attack: 0.002,
    decay: 0.25,
    sustain: 0.6,
    release: 0.06,
    cutoff: 180.0,
    env_amount: 1400.0,
    filter_decay: 0.18,
    resonance: 0.45,
    detune: 6.0,
    sub: 0.6,
    gain: 0.5,
    mono: true,
};

pub const PAD: Preset = Preset {
    attack: 0.03,
    decay: 0.4,
    sustain: 0.7,
    release: 0.35,
    cutoff: 900.0,
    env_amount: 1800.0,
    filter_decay: 0.5,
    resonance: 0.2,
    detune: 14.0,
    sub: 0.0,
    gain: 0.18,
    mono: false,
};

pub const LEAD: Preset = Preset {
    attack: 0.003,
    decay: 0.18,
    sustain: 0.35,
    release: 0.12,
    cutoff: 1200.0,
    env_amount: 4200.0,
    filter_decay: 0.12,
    resonance: 0.35,
    detune: 9.0,
    sub: 0.0,
    gain: 0.22,
    mono: false,
};

const SYNTH_VOICES: usize = 12;
/// Samples between filter coefficient updates.
const CONTROL_RATE: usize = 16;

#[derive(Clone, Copy, PartialEq, Debug)]
enum Stage {
    Off,
    Attack,
    Decay,
    Sustain,
    Release,
}

#[derive(Clone, Copy)]
struct Voice {
    note: u8,
    velocity: f32,
    stage: Stage,
    level: f32,
    filter_env: f32,
    phase: [f32; 2],
    sub_phase: f32,
    inc: [f32; 2],
    /// State-variable filter memory, per channel.
    ic1: [f32; 2],
    ic2: [f32; 2],
    /// Order the voice started in, to steal the oldest.
    born: u64,
}

impl Voice {
    const OFF: Voice = Voice {
        note: 0,
        velocity: 0.0,
        stage: Stage::Off,
        level: 0.0,
        filter_env: 0.0,
        phase: [0.0; 2],
        sub_phase: 0.0,
        inc: [0.0; 2],
        ic1: [0.0; 2],
        ic2: [0.0; 2],
        born: 0,
    };
}

pub struct Synth {
    preset: Preset,
    rate: f32,
    voices: [Voice; SYNTH_VOICES],
    births: u64,
}

impl Synth {
    pub fn new(preset: Preset, rate: f64) -> Synth {
        Synth {
            preset,
            rate: rate as f32,
            voices: [Voice::OFF; SYNTH_VOICES],
            births: 0,
        }
    }

    fn note_on(&mut self, pitch: u8, velocity: f32) {
        let slot = if self.preset.mono {
            0
        } else if let Some(i) = self.voices.iter().position(|v| v.stage == Stage::Off) {
            i
        } else {
            (0..SYNTH_VOICES)
                .min_by_key(|&i| self.voices[i].born)
                .unwrap_or(0)
        };
        let freq = 440.0 * 2f32.powf((pitch as f32 - 69.0) / 12.0);
        let spread = 2f32.powf(self.preset.detune / 1200.0);
        let v = &mut self.voices[slot];
        let legato = self.preset.mono && v.stage != Stage::Off;
        v.note = pitch;
        v.velocity = velocity;
        v.inc = [freq / self.rate, freq * spread / self.rate];
        v.stage = Stage::Attack;
        v.filter_env = 1.0;
        if !legato {
            v.level = 0.0;
            v.phase = [0.0, 0.37];
            v.sub_phase = 0.0;
            v.ic1 = [0.0; 2];
            v.ic2 = [0.0; 2];
        }
        self.births += 1;
        v.born = self.births;
    }

    fn note_off(&mut self, pitch: u8) {
        for v in self.voices.iter_mut() {
            if v.note == pitch && v.stage != Stage::Off && v.stage != Stage::Release {
                v.stage = Stage::Release;
            }
        }
    }

    fn all_off(&mut self) {
        for v in self.voices.iter_mut() {
            if v.stage != Stage::Off {
                v.stage = Stage::Release;
            }
        }
    }

    fn render(&mut self, out: &mut [[f32; 2]]) {
        let p = self.preset;
        let sr = self.rate;
        let step = |secs: f32| 1.0 / (secs.max(0.0005) * sr);
        let (a, d, r) = (step(p.attack), step(p.decay), step(p.release));
        let fdecay = (-1.0 / (p.filter_decay.max(0.001) * sr)).exp();
        let k = 2.0 - 2.0 * p.resonance.clamp(0.0, 0.98);
        for v in self.voices.iter_mut().filter(|v| v.stage != Stage::Off) {
            let (mut a1, mut a2, mut a3) = (0.0, 0.0, 0.0);
            for (i, frame) in out.iter_mut().enumerate() {
                if i % CONTROL_RATE == 0 {
                    let cutoff = (p.cutoff + p.env_amount * v.filter_env * v.velocity)
                        .clamp(20.0, sr * 0.45);
                    let g = (PI * cutoff / sr).tan();
                    a1 = 1.0 / (1.0 + g * (g + k));
                    a2 = g * a1;
                    a3 = g * a2;
                }
                // Amplitude envelope.
                match v.stage {
                    Stage::Attack => {
                        v.level += a;
                        if v.level >= 1.0 {
                            v.level = 1.0;
                            v.stage = Stage::Decay;
                        }
                    }
                    Stage::Decay => {
                        v.level -= d * (1.0 - p.sustain);
                        if v.level <= p.sustain {
                            v.level = p.sustain;
                            v.stage = Stage::Sustain;
                        }
                    }
                    Stage::Sustain => {}
                    Stage::Release => {
                        v.level -= r;
                        if v.level <= 0.0 {
                            v.level = 0.0;
                            v.stage = Stage::Off;
                            break;
                        }
                    }
                    Stage::Off => break,
                }
                v.filter_env *= fdecay;
                let mut osc = 0.0;
                for o in 0..2 {
                    osc += saw(v.phase[o], v.inc[o]);
                    v.phase[o] += v.inc[o];
                    if v.phase[o] >= 1.0 {
                        v.phase[o] -= 1.0;
                    }
                }
                osc *= 0.5;
                if p.sub > 0.0 {
                    osc += p.sub * if v.sub_phase < 0.5 { 0.5 } else { -0.5 };
                    v.sub_phase += v.inc[0] * 0.5;
                    if v.sub_phase >= 1.0 {
                        v.sub_phase -= 1.0;
                    }
                }
                let amp = v.level * v.velocity * p.gain;
                for (c, out) in frame.iter_mut().enumerate() {
                    // Two slightly different inputs give the voice width.
                    let x = if c == 0 { osc } else { osc * 0.98 };
                    let v3 = x - v.ic2[c];
                    let v1 = a1 * v.ic1[c] + a2 * v3;
                    let v2 = v.ic2[c] + a2 * v.ic1[c] + a3 * v3;
                    v.ic1[c] = 2.0 * v1 - v.ic1[c];
                    v.ic2[c] = 2.0 * v2 - v.ic2[c];
                    *out += v2 * amp;
                }
            }
        }
    }
}

/// A band-limited sawtooth (polyBLEP) at `phase` (0..1) stepping by `inc`.
#[inline]
fn saw(phase: f32, inc: f32) -> f32 {
    let mut y = 2.0 * phase - 1.0;
    if phase < inc {
        let t = phase / inc;
        y -= t + t - t * t - 1.0;
    } else if phase > 1.0 - inc {
        let t = (phase - 1.0) / inc;
        y -= t * t + t + t + 1.0;
    }
    y
}

#[cfg(test)]
mod tests {
    use super::*;

    fn energy(out: &[[f32; 2]]) -> f32 {
        out.iter().map(|f| f[0] * f[0] + f[1] * f[1]).sum()
    }

    #[test]
    fn synth_plays_and_releases() {
        let mut s = Synth::new(LEAD, 48_000.0);
        let mut out = vec![[0.0; 2]; 4800];
        s.note_on(60, 1.0);
        s.render(&mut out);
        assert!(energy(&out) > 1.0);
        s.note_off(60);
        let mut tail = vec![[0.0; 2]; 48_000];
        s.render(&mut tail);
        assert!(s.voices.iter().all(|v| v.stage == Stage::Off));
        assert!(tail.iter().all(|f| f[0].is_finite() && f[1].is_finite()));
    }

    #[test]
    fn drums_play_a_kick() {
        let mut d = Drums::new("Punch", 48_000.0, 1.0);
        let mut out = vec![[0.0; 2]; 4800];
        d.note_on(KICK, 1.0);
        d.render(&mut out);
        assert!(energy(&out) > 1.0);
        // Notes outside the kit are ignored.
        d.note_on(100, 1.0);
        assert_eq!(d.voices.iter().filter(|v| v.active).count(), 1);
    }
}
