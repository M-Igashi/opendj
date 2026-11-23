//! The studio's audio engine: the Session grid as the audio thread plays it.
//!
//! Layers, after the architecture review in the add-opendj-studio change:
//!
//! - The UI holds the model ([`crate::clip::Track`]) and compiles it into a
//!   [`Program`]: plain clips, no sample rate, immutable once sent.
//! - The audio thread owns the [`Engine`]: the program, each track's
//!   instrument, and each track's playing state (playing clip, pending
//!   launch, sounding notes). Programs arrive over a wait-free queue and the
//!   old one goes back over a second queue to be dropped off the audio
//!   thread; playing state carries over.
//! - Launches, scenes and stops are small `Copy` commands on the same queue.
//!   They never touch undo.
//! - The engine reports to the UI through [`Monitor`]'s atomics only.
//!
//! Time is in beats (`f64`) on the shared clock: Ableton Link's session
//! beat when Link is on, else the studio's own clock, which computes each
//! block's beat from a frame count since its last tempo change rather than
//! adding block lengths up, so it never drifts. Launches wait for the next
//! launch point (bar, beat or none) and happen on the exact frame: a block
//! is split there.
//!
//! The callback never allocates, locks or does I/O.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicU8, Ordering};
use std::sync::Arc;

use crate::clip::{Clip, Sound, Track, SCENES, TRACKS};
use crate::sound::{self, Drums, Instrument, Synth};

/// Beats in a bar; also the Link quantum, so bars line up with openDJ's.
pub const QUANTUM: f64 = 4.0;

/// Where launches wait for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Quantize {
    Bar,
    Beat,
    Off,
}

impl Quantize {
    pub fn label(self) -> &'static str {
        match self {
            Quantize::Bar => "1 Bar",
            Quantize::Beat => "1/4",
            Quantize::Off => "None",
        }
    }

    pub fn next(self) -> Quantize {
        match self {
            Quantize::Bar => Quantize::Beat,
            Quantize::Beat => Quantize::Off,
            Quantize::Off => Quantize::Bar,
        }
    }

    fn grid(self) -> f64 {
        match self {
            Quantize::Bar => QUANTUM,
            Quantize::Beat => 1.0,
            Quantize::Off => 0.0,
        }
    }
}

/// The first launch point at or after `beat`.
pub fn next_launch(beat: f64, q: Quantize) -> f64 {
    let grid = q.grid();
    if grid <= 0.0 {
        return beat;
    }
    // A beat a hair past a grid line (from float rounding) counts as on it.
    let n = ((beat - 1e-9) / grid).ceil();
    n * grid
}

/// The clips the audio thread plays, compiled from the model.
pub struct Program {
    pub tracks: Vec<Vec<Option<Clip>>>,
}

impl Program {
    pub fn compile(tracks: &[Track]) -> Box<Program> {
        Box::new(Program {
            tracks: tracks.iter().map(|t| t.slots.clone()).collect(),
        })
    }

    fn clip(&self, track: usize, scene: usize) -> Option<&Clip> {
        self.tracks.get(track)?.get(scene)?.as_ref()
    }
}

/// From the UI to the audio thread.
pub enum Command {
    Launch {
        track: usize,
        scene: usize,
    },
    Scene(usize),
    Stop(usize),
    StopAll,
    Play(bool),
    Tempo(f64),
    Quantize(Quantize),
    /// A recompiled set, after an edit (clip editing comes in step 3).
    #[allow(dead_code)]
    Program(Box<Program>),
}

/// A slot as the UI shows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum SlotState {
    Empty = 0,
    Stopped = 1,
    Queued = 2,
    Playing = 3,
}

/// What the audio thread tells the UI.
pub struct Monitor {
    beat: AtomicU64,
    tempo: AtomicU64,
    playing: AtomicBool,
    slots: [AtomicU8; TRACKS * SCENES],
    progress: [AtomicU32; TRACKS],
    stopping: [AtomicBool; TRACKS],
    /// Time spent rendering over the block's length, smoothed.
    load: AtomicU32,
}

impl Monitor {
    fn new() -> Monitor {
        Monitor {
            beat: AtomicU64::new(0),
            tempo: AtomicU64::new(0),
            playing: AtomicBool::new(false),
            slots: std::array::from_fn(|_| AtomicU8::new(0)),
            progress: std::array::from_fn(|_| AtomicU32::new((-1f32).to_bits())),
            stopping: std::array::from_fn(|_| AtomicBool::new(false)),
            load: AtomicU32::new(0),
        }
    }

    pub fn beat(&self) -> f64 {
        f64::from_bits(self.beat.load(Ordering::Relaxed))
    }

    pub fn tempo(&self) -> f64 {
        f64::from_bits(self.tempo.load(Ordering::Relaxed))
    }

    pub fn playing(&self) -> bool {
        self.playing.load(Ordering::Relaxed)
    }

    pub fn slot(&self, track: usize, scene: usize) -> SlotState {
        match self.slots[scene * TRACKS + track].load(Ordering::Relaxed) {
            1 => SlotState::Stopped,
            2 => SlotState::Queued,
            3 => SlotState::Playing,
            _ => SlotState::Empty,
        }
    }

    /// How far through its clip `track` is (0..1), or -1 when stopped.
    pub fn progress(&self, track: usize) -> f32 {
        f32::from_bits(self.progress[track].load(Ordering::Relaxed))
    }

    pub fn stopping(&self, track: usize) -> bool {
        self.stopping[track].load(Ordering::Relaxed)
    }

    pub fn load(&self) -> f32 {
        f32::from_bits(self.load.load(Ordering::Relaxed))
    }

    pub fn set_load(&self, load: f32) {
        self.load.store(load.to_bits(), Ordering::Relaxed);
    }
}

/// One block of the shared clock: its beat range and tempo.
#[derive(Clone, Copy, Debug)]
pub struct Span {
    pub start: f64,
    pub end: f64,
    pub tempo: f64,
}

/// The studio's own clock, for when Link is off. A block's beat comes from
/// the frames since the last tempo change, so it never drifts.
pub struct Clock {
    rate: f64,
    tempo: f64,
    anchor_beat: f64,
    anchor_frame: u64,
    frame: u64,
}

impl Clock {
    pub fn new(rate: f64, tempo: f64) -> Clock {
        Clock {
            rate,
            tempo,
            anchor_beat: 0.0,
            anchor_frame: 0,
            frame: 0,
        }
    }

    fn beat_at(&self, frame: u64) -> f64 {
        self.anchor_beat + (frame - self.anchor_frame) as f64 * self.tempo / 60.0 / self.rate
    }

    /// Starts the clock at `beat` from now.
    pub fn locate(&mut self, beat: f64) {
        self.anchor_beat = beat;
        self.anchor_frame = self.frame;
    }

    pub fn set_tempo(&mut self, tempo: f64) {
        self.anchor_beat = self.beat_at(self.frame);
        self.anchor_frame = self.frame;
        self.tempo = tempo;
    }

    pub fn tempo(&self) -> f64 {
        self.tempo
    }

    /// The next `frames` frames.
    pub fn advance(&mut self, frames: usize) -> Span {
        let start = self.beat_at(self.frame);
        self.frame += frames as u64;
        Span {
            start,
            end: self.beat_at(self.frame),
            tempo: self.tempo,
        }
    }
}

/// A launch or stop waiting for its launch point.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Pending {
    /// The scene to play, or None to stop.
    scene: Option<usize>,
    at: f64,
}

const MAX_HELD: usize = 64;

#[derive(Clone, Copy)]
struct Held {
    pitch: u8,
    off: f64,
}

/// A track's playing state, which outlives programs.
struct Column {
    /// Scene playing and the beat it launched on.
    playing: Option<(usize, f64)>,
    pending: Option<Pending>,
    /// Notes sounding and the beat each ends on: the note tracker.
    held: [Held; MAX_HELD],
    held_len: usize,
    instrument: Instrument,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Kind {
    Off(u8),
    On(u8, f32),
    AllOff,
}

#[derive(Clone, Copy, Debug)]
struct Event {
    beat: f64,
    kind: Kind,
    /// The scene whose clip played the note, for the log.
    scene: u8,
}

/// A note event as played, for tests and `--check`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Logged {
    /// Frames since the engine started.
    pub frame: u64,
    pub track: usize,
    pub pitch: u8,
    pub on: bool,
    /// The scene of the clip that played it (255 for a note-off at a switch).
    pub scene: u8,
}

const MAX_EVENTS: usize = 1024;

/// The master level before the soft clipper: headroom for four tracks.
const MASTER: f32 = 0.7;

pub struct Engine {
    program: Box<Program>,
    columns: Vec<Column>,
    quantize: Quantize,
    playing: bool,
    /// Where the previous block ended, so blocks join up when Link nudges
    /// the beat.
    last_end: Option<f64>,
    events: Vec<Event>,
    scratch: Vec<[f32; 2]>,
    frames_done: u64,
    monitor: Arc<Monitor>,
    /// Programs replaced, on their way back to the UI thread to be dropped.
    trash: rtrb::Producer<Box<Program>>,
    log: Option<Vec<Logged>>,
}

/// The largest block rendered in one go; longer callbacks are cut up.
pub const MAX_BLOCK: usize = 4096;

impl Engine {
    pub fn new(
        tracks: &[Track],
        rate: f64,
        monitor: Arc<Monitor>,
        trash: rtrb::Producer<Box<Program>>,
    ) -> Engine {
        let columns = tracks
            .iter()
            .map(|t| Column {
                playing: None,
                pending: None,
                held: [Held { pitch: 0, off: 0.0 }; MAX_HELD],
                held_len: 0,
                instrument: match t.sound {
                    Sound::Drums(kit) => Instrument::Drums(Drums::new(kit, rate, 0.45)),
                    Sound::Bass => Instrument::Synth(Box::new(Synth::new(sound::BASS, rate))),
                    Sound::Pad => Instrument::Synth(Box::new(Synth::new(sound::PAD, rate))),
                    Sound::Lead => Instrument::Synth(Box::new(Synth::new(sound::LEAD, rate))),
                },
            })
            .collect();
        Engine {
            program: Program::compile(tracks),
            columns,
            quantize: Quantize::Bar,
            playing: false,
            last_end: None,
            events: Vec::with_capacity(MAX_EVENTS),
            scratch: vec![[0.0; 2]; MAX_BLOCK],
            frames_done: 0,
            monitor,
            trash,
            log: None,
        }
    }

    /// Keeps every note event, up to `capacity`, for [`Engine::log`].
    pub fn keep_log(&mut self, capacity: usize) {
        self.log = Some(Vec::with_capacity(capacity));
    }

    pub fn log(&self) -> &[Logged] {
        self.log.as_deref().unwrap_or(&[])
    }

    pub fn playing(&self) -> bool {
        self.playing
    }

    /// Applies a command. `now` is the beat the next block starts on.
    /// Returns a tempo or play change the clock must make.
    pub fn command(&mut self, cmd: Command, now: f64) -> Option<ClockChange> {
        match cmd {
            Command::Launch { track, scene } => {
                // An empty slot doesn't start the transport.
                if !self.playing && self.program.clip(track, scene).is_none() {
                    return None;
                }
                let start = self.ensure_playing(now);
                self.launch(track, Some(scene), start.unwrap_or(now));
                start.map(ClockChange::Start)
            }
            Command::Scene(scene) => {
                let start = self.ensure_playing(now);
                for t in 0..self.columns.len() {
                    self.launch(t, Some(scene), start.unwrap_or(now));
                }
                start.map(ClockChange::Start)
            }
            Command::Stop(track) => {
                if self.playing {
                    self.launch(track, None, now);
                }
                None
            }
            Command::StopAll => {
                if self.playing {
                    for t in 0..self.columns.len() {
                        self.launch(t, None, now);
                    }
                }
                None
            }
            Command::Play(on) => {
                if on == self.playing {
                    return None;
                }
                if on {
                    self.ensure_playing(now).map(ClockChange::Start)
                } else {
                    self.playing = false;
                    self.last_end = None;
                    for c in self.columns.iter_mut() {
                        c.playing = None;
                        c.pending = None;
                        c.held_len = 0;
                        c.instrument.all_off();
                    }
                    Some(ClockChange::Stop)
                }
            }
            Command::Tempo(bpm) => Some(ClockChange::Tempo(bpm)),
            Command::Quantize(q) => {
                self.quantize = q;
                None
            }
            Command::Program(p) => {
                let old = std::mem::replace(&mut self.program, p);
                // A clip whose slot emptied stops at the next launch point.
                for t in 0..self.columns.len() {
                    if let Some((scene, _)) = self.columns[t].playing {
                        if self.program.clip(t, scene).is_none() {
                            self.launch(t, None, now);
                        }
                    }
                }
                // Dropped on the UI thread; if its queue is full, here.
                let _ = self.trash.push(old);
                None
            }
        }
    }

    /// Starts the transport if it's stopped; returns the beat it starts on.
    fn ensure_playing(&mut self, now: f64) -> Option<f64> {
        if self.playing {
            return None;
        }
        self.playing = true;
        self.last_end = None;
        // The studio's own clock starts on bar 1 at once; Link keeps its
        // own beat (the caller passes Link's beat as `now`).
        Some(now)
    }

    fn launch(&mut self, track: usize, scene: Option<usize>, now: f64) {
        let Some(c) = self.columns.get_mut(track) else {
            return;
        };
        if let Some(s) = scene {
            if self.program.clip(track, s).is_none() {
                // An empty slot stops its track, as a scene row does.
                if c.playing.is_none() && c.pending.is_none() {
                    return;
                }
                c.pending = Some(Pending {
                    scene: None,
                    at: next_launch(now, self.quantize),
                });
                return;
            }
        } else if c.playing.is_none() {
            c.pending = None;
            return;
        }
        c.pending = Some(Pending {
            scene,
            at: next_launch(now, self.quantize),
        });
    }

    /// Renders one block of `out.len()` frames over `span` of the clock,
    /// adding the tracks into `out`.
    pub fn render(&mut self, span: Span, out: &mut [[f32; 2]]) {
        let frames = out.len();
        // Join onto the previous block unless the clock jumped.
        let start = match self.last_end {
            Some(end) if (span.start - end).abs() < 0.25 => end,
            _ => span.start,
        };
        let end = span.end.max(start);
        self.last_end = Some(end);
        let beats_per_frame = if end > start {
            (end - start) / frames as f64
        } else {
            span.tempo / 60.0 / 48_000.0
        };
        for t in 0..self.columns.len() {
            if self.playing {
                self.events.clear();
                self.track_events(t, start, end);
                self.events.sort_unstable_by(|a, b| {
                    a.beat
                        .total_cmp(&b.beat)
                        .then_with(|| order(a.kind).cmp(&order(b.kind)))
                });
            } else {
                self.events.clear();
            }
            let scratch = &mut self.scratch[..frames];
            scratch.fill([0.0; 2]);
            let mut done = 0;
            for i in 0..self.events.len() {
                let e = self.events[i];
                let at = (((e.beat - start) / beats_per_frame).round() as usize).min(frames);
                if at > done {
                    self.columns[t].instrument.render(&mut scratch[done..at]);
                    done = at;
                }
                let inst = &mut self.columns[t].instrument;
                match e.kind {
                    Kind::On(p, v) => inst.note_on(p, v),
                    Kind::Off(p) => inst.note_off(p),
                    Kind::AllOff => inst.all_off(),
                }
                if let (Some(log), Kind::On(p, _) | Kind::Off(p)) = (&mut self.log, e.kind) {
                    if log.len() < log.capacity() {
                        log.push(Logged {
                            frame: self.frames_done + at as u64,
                            track: t,
                            pitch: p,
                            on: matches!(e.kind, Kind::On(..)),
                            scene: e.scene,
                        });
                    }
                }
            }
            if done < frames {
                self.columns[t].instrument.render(&mut scratch[done..]);
            }
            for (o, s) in out.iter_mut().zip(scratch.iter()) {
                o[0] += s[0];
                o[1] += s[1];
            }
        }
        for o in out.iter_mut() {
            o[0] = soft_clip(o[0] * MASTER);
            o[1] = soft_clip(o[1] * MASTER);
        }
        self.frames_done += frames as u64;
        self.publish(end, span.tempo);
    }

    /// Collects `track`'s note events in [`lo`, `hi`) into `self.events`,
    /// switching clips at a pending launch point.
    fn track_events(&mut self, t: usize, lo: f64, hi: f64) {
        let mut from = lo;
        if let Some(p) = self.columns[t].pending {
            if p.at < hi {
                let at = p.at.max(lo);
                self.clip_events(t, from, at);
                // Every sounding note ends on the switch.
                let c = &mut self.columns[t];
                for h in &c.held[..c.held_len] {
                    push(&mut self.events, at, Kind::Off(h.pitch), 255);
                }
                c.held_len = 0;
                push(&mut self.events, at, Kind::AllOff, 255);
                c.playing = p.scene.map(|s| (s, at));
                c.pending = None;
                from = at;
            }
        }
        self.clip_events(t, from, hi);
    }

    fn clip_events(&mut self, t: usize, lo: f64, hi: f64) {
        if hi <= lo {
            return;
        }
        let c = &mut self.columns[t];
        // Note-offs due in this stretch.
        let mut i = 0;
        while i < c.held_len {
            if c.held[i].off < hi {
                push(
                    &mut self.events,
                    c.held[i].off.max(lo),
                    Kind::Off(c.held[i].pitch),
                    255,
                );
                c.held_len -= 1;
                c.held[i] = c.held[c.held_len];
            } else {
                i += 1;
            }
        }
        let Some((scene, launched)) = c.playing else {
            return;
        };
        let Some(clip) = self.program.clip(t, scene) else {
            return;
        };
        if clip.beats <= 0.0 {
            return;
        }
        let first = ((lo - launched) / clip.beats).floor().max(0.0) as i64;
        let last = ((hi - launched) / clip.beats).floor() as i64;
        for k in first..=last {
            let base = launched + k as f64 * clip.beats;
            for n in &clip.notes {
                let on = base + n.start;
                if on < lo {
                    continue;
                }
                if on >= hi {
                    break;
                }
                // Notes are cut at the loop's end.
                let off = on + n.length.min(clip.beats - n.start);
                push(
                    &mut self.events,
                    on,
                    Kind::On(n.pitch, n.velocity),
                    scene as u8,
                );
                if off < hi {
                    push(&mut self.events, off, Kind::Off(n.pitch), scene as u8);
                } else if c.held_len < MAX_HELD {
                    c.held[c.held_len] = Held {
                        pitch: n.pitch,
                        off,
                    };
                    c.held_len += 1;
                }
            }
        }
    }

    fn publish(&self, beat: f64, tempo: f64) {
        let m = &self.monitor;
        m.beat.store(beat.to_bits(), Ordering::Relaxed);
        m.tempo.store(tempo.to_bits(), Ordering::Relaxed);
        m.playing.store(self.playing, Ordering::Relaxed);
        for (t, c) in self.columns.iter().enumerate() {
            for s in 0..SCENES {
                let state = if c.playing.map(|(p, _)| p) == Some(s) {
                    SlotState::Playing
                } else if c.pending.and_then(|p| p.scene) == Some(s) {
                    SlotState::Queued
                } else if self.program.clip(t, s).is_some() {
                    SlotState::Stopped
                } else {
                    SlotState::Empty
                };
                m.slots[s * TRACKS + t].store(state as u8, Ordering::Relaxed);
            }
            let progress = match c.playing {
                Some((s, at)) => match self.program.clip(t, s) {
                    Some(clip) if clip.beats > 0.0 && beat >= at => {
                        (((beat - at) / clip.beats).fract()) as f32
                    }
                    _ => 0.0,
                },
                None => -1.0,
            };
            m.progress[t].store(progress.to_bits(), Ordering::Relaxed);
            m.stopping[t].store(
                c.pending.is_some_and(|p| p.scene.is_none()),
                Ordering::Relaxed,
            );
        }
    }
}

/// What the clock should do after a command.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ClockChange {
    /// The transport started on this beat.
    Start(f64),
    Stop,
    Tempo(f64),
}

fn push(events: &mut Vec<Event>, beat: f64, kind: Kind, scene: u8) {
    // Never grow on the audio thread: a block with more events than this
    // loses the rest.
    if events.len() < events.capacity() {
        events.push(Event { beat, kind, scene });
    }
}

/// Note-offs before all-offs before note-ons at the same beat, so a note
/// that ends where it starts again plays.
fn order(kind: Kind) -> u8 {
    match kind {
        Kind::Off(_) => 0,
        Kind::AllOff => 1,
        Kind::On(..) => 2,
    }
}

#[inline]
fn soft_clip(x: f32) -> f32 {
    if x.abs() < 0.8 {
        x
    } else {
        x.signum() * (0.8 + 0.2 * ((x.abs() - 0.8) / 0.2).tanh())
    }
}

/// The engine with its clock and command queue: what the audio callback
/// (or the offline check) runs.
pub struct Studio {
    pub engine: Engine,
    pub clock: Clock,
    pub link: Option<crate::link::LinkClock>,
    commands: rtrb::Consumer<Command>,
    pub monitor: Arc<Monitor>,
    rate: f64,
}

/// The UI's ends of the queues.
pub struct Remote {
    pub commands: rtrb::Producer<Command>,
    pub trash: rtrb::Consumer<Box<Program>>,
    pub monitor: Arc<Monitor>,
}

impl Studio {
    pub fn new(tracks: &[Track], rate: f64, tempo: f64) -> (Studio, Remote) {
        let monitor = Arc::new(Monitor::new());
        let (cmd_tx, cmd_rx) = rtrb::RingBuffer::new(256);
        let (trash_tx, trash_rx) = rtrb::RingBuffer::new(8);
        let engine = Engine::new(tracks, rate, monitor.clone(), trash_tx);
        let mut clock = Clock::new(rate, tempo);
        clock.set_tempo(tempo);
        monitor.tempo.store(tempo.to_bits(), Ordering::Relaxed);
        (
            Studio {
                engine,
                clock,
                link: None,
                commands: cmd_rx,
                monitor: monitor.clone(),
                rate,
            },
            Remote {
                commands: cmd_tx,
                trash: trash_rx,
                monitor,
            },
        )
    }

    pub fn rate(&self) -> f64 {
        self.rate
    }

    /// Renders `out` (stereo frames), cutting it into blocks of at most
    /// [`MAX_BLOCK`].
    pub fn process(&mut self, out: &mut [[f32; 2]]) {
        out.fill([0.0; 2]);
        for chunk in out.chunks_mut(MAX_BLOCK) {
            self.block(chunk);
        }
    }

    fn block(&mut self, out: &mut [[f32; 2]]) {
        let frames = out.len();
        let linked = self.link.as_mut().and_then(|l| l.begin(frames));
        // Commands land at the start of the block.
        let now = match &linked {
            Some(s) => s.start,
            // Stopped, the own clock waits on bar 1.
            None if !self.engine.playing() => 0.0,
            None => self.clock.beat_now(),
        };
        while let Ok(cmd) = self.commands.pop() {
            match self.engine.command(cmd, now) {
                Some(ClockChange::Start(_)) => {
                    if linked.is_none() {
                        // Bar 1 starts now.
                        self.clock.locate(0.0);
                    }
                }
                Some(ClockChange::Stop) => {}
                Some(ClockChange::Tempo(bpm)) => {
                    let bpm = bpm.clamp(40.0, 250.0);
                    self.clock.set_tempo(bpm);
                    if let Some(l) = self.link.as_mut() {
                        l.set_tempo(bpm);
                    }
                }
                None => {}
            }
        }
        let span = match linked {
            Some(s) => {
                // Keep the own clock where Link is, for when Link goes off.
                self.clock.advance(frames);
                self.clock.sync(s.end, s.tempo);
                s
            }
            None => {
                if self.engine.playing() {
                    self.clock.advance(frames)
                } else {
                    let b = self.clock.beat_now();
                    Span {
                        start: b,
                        end: b,
                        tempo: self.clock.tempo(),
                    }
                }
            }
        };
        self.engine.render(span, out);
    }
}

impl Clock {
    /// The beat the next block starts on.
    pub fn beat_now(&self) -> f64 {
        self.beat_at(self.frame)
    }

    /// Puts the clock on `beat` at `tempo` from now: after a block Link
    /// timed, so the clock carries on from Link's beat if Link goes off.
    fn sync(&mut self, beat: f64, tempo: f64) {
        self.tempo = tempo;
        self.anchor_frame = self.frame;
        self.anchor_beat = beat;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clip::{Clip, Note};

    fn one_note_set() -> Vec<Track> {
        let clip = |name: &str, pitch: u8| {
            Some(Clip::new(
                name,
                4.0,
                vec![Note {
                    start: 0.0,
                    length: 0.5,
                    pitch,
                    velocity: 1.0,
                }],
            ))
        };
        let mut slots = vec![None; SCENES];
        slots[0] = clip("a", 60);
        slots[1] = clip("b", 62);
        let mut tracks = vec![Track {
            name: "Lead".into(),
            sound: Sound::Lead,
            slots,
        }];
        for _ in 1..TRACKS {
            tracks.push(Track {
                name: "Empty".into(),
                sound: Sound::Lead,
                slots: vec![None; SCENES],
            });
        }
        tracks
    }

    const RATE: f64 = 48_000.0;
    const BLOCK: usize = 256;

    /// Runs `studio` for `blocks` blocks.
    fn run(studio: &mut Studio, blocks: usize) {
        let mut out = vec![[0.0; 2]; BLOCK];
        for _ in 0..blocks {
            studio.process(&mut out);
        }
    }

    fn frames_per_beat(tempo: f64) -> f64 {
        RATE * 60.0 / tempo
    }

    #[test]
    fn launch_points() {
        assert_eq!(next_launch(5.5, Quantize::Bar), 8.0);
        assert_eq!(next_launch(8.0, Quantize::Bar), 8.0);
        assert_eq!(next_launch(8.0 + 1e-12, Quantize::Bar), 8.0);
        assert_eq!(next_launch(5.5, Quantize::Beat), 6.0);
        assert_eq!(next_launch(5.5, Quantize::Off), 5.5);
    }

    #[test]
    fn first_launch_starts_at_once_and_loops() {
        let (mut s, mut r) = Studio::new(&one_note_set(), RATE, 120.0);
        s.engine.keep_log(64);
        r.commands.push(Command::Launch { track: 0, scene: 0 }).ok();
        // Three bars at 120 BPM = 6 seconds.
        run(&mut s, (6.0 * RATE) as usize / BLOCK + 1);
        let ons: Vec<u64> = s
            .engine
            .log()
            .iter()
            .filter(|e| e.on)
            .map(|e| e.frame)
            .collect();
        let bar = (4.0 * frames_per_beat(120.0)) as u64;
        assert_eq!(&ons[..3], &[0, bar, 2 * bar]);
    }

    #[test]
    fn launch_waits_for_the_bar_to_the_sample() {
        let (mut s, mut r) = Studio::new(&one_note_set(), RATE, 124.0);
        s.engine.keep_log(64);
        r.commands.push(Command::Launch { track: 0, scene: 0 }).ok();
        let fpb = frames_per_beat(124.0);
        // Run to beat 5.5, then launch scene b.
        run(&mut s, (5.5 * fpb) as usize / BLOCK);
        r.commands.push(Command::Launch { track: 0, scene: 1 }).ok();
        run(&mut s, (4.0 * fpb) as usize / BLOCK);
        let b_on = s
            .engine
            .log()
            .iter()
            .find(|e| e.on && e.pitch == 62)
            .expect("b played")
            .frame;
        let want = (8.0 * fpb).round() as u64;
        assert!(b_on.abs_diff(want) <= 1, "{b_on} vs {want}");
        // a's note was not left hanging, and a stopped playing on the switch.
        assert_eq!(r.monitor.slot(0, 1), SlotState::Playing);
        assert_eq!(r.monitor.slot(0, 0), SlotState::Stopped);
    }

    #[test]
    fn stop_waits_for_the_bar_and_releases_notes() {
        let mut set = one_note_set();
        // A note held across the whole bar.
        set[0].slots[0].as_mut().unwrap().notes[0].length = 4.0;
        let (mut s, mut r) = Studio::new(&set, RATE, 120.0);
        s.engine.keep_log(64);
        r.commands.push(Command::Launch { track: 0, scene: 0 }).ok();
        run(&mut s, 10);
        r.commands.push(Command::Stop(0)).ok();
        run(&mut s, 2);
        assert!(r.monitor.stopping(0));
        run(&mut s, (4.0 * frames_per_beat(120.0)) as usize / BLOCK);
        assert!(!r.monitor.stopping(0));
        assert_eq!(r.monitor.progress(0), -1.0);
        let log = s.engine.log();
        let on = log.iter().filter(|e| e.on).count();
        let off = log.iter().filter(|e| !e.on).count();
        assert_eq!(on, 1);
        assert_eq!(off, 1, "the held note got its note-off");
    }

    #[test]
    fn scene_stops_tracks_with_an_empty_slot() {
        let mut set = one_note_set();
        set[1].slots[0] = set[0].slots[0].clone();
        let (mut s, mut r) = Studio::new(&set, RATE, 120.0);
        r.commands.push(Command::Scene(0)).ok();
        run(&mut s, 4);
        assert_eq!(r.monitor.slot(1, 0), SlotState::Playing);
        // Scene 2 has a clip on track 0 only.
        r.commands.push(Command::Scene(1)).ok();
        run(&mut s, (4.0 * frames_per_beat(120.0)) as usize / BLOCK + 1);
        assert_eq!(r.monitor.slot(0, 1), SlotState::Playing);
        assert_eq!(r.monitor.progress(1), -1.0, "track 2 stopped");
    }

    #[test]
    fn tempo_change_keeps_the_beat_continuous() {
        let mut c = Clock::new(RATE, 120.0);
        let a = c.advance(1000);
        c.set_tempo(90.0);
        let b = c.advance(1000);
        assert!((a.end - b.start).abs() < 1e-12);
        assert!((b.end - b.start - 1000.0 * 90.0 / 60.0 / RATE).abs() < 1e-12);
    }

    #[test]
    fn programs_swap_and_go_back_to_be_dropped() {
        let set = one_note_set();
        let (mut s, mut r) = Studio::new(&set, RATE, 120.0);
        r.commands.push(Command::Launch { track: 0, scene: 0 }).ok();
        run(&mut s, 4);
        // A new program without clip a: the track stops on the next bar.
        let mut edited = set.clone();
        edited[0].slots[0] = None;
        r.commands
            .push(Command::Program(Program::compile(&edited)))
            .ok();
        run(&mut s, 2);
        assert!(r.trash.pop().is_ok(), "old program came back");
        assert!(r.monitor.stopping(0));
    }
}
