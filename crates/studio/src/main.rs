//! openDJ Studio: a clip-launching DAW on openDJ's engine. This is the
//! first test app (OpenSpec change add-opendj-studio, step 1): four tracks
//! by eight scenes of MIDI clips playing built-in sounds, launched on the
//! bar of the studio's own clock or of an Ableton Link session shared with
//! openDJ.
//!
//! `opendj-studio` opens the window. `opendj-studio --check` renders
//! offline and prints launch timing and CPU figures; `--render out.wav`
//! writes 32 bars of the demo set going through its scenes.
//!
//! openDJ never depends on this crate: it can move to its own repository.

mod audio;
mod check;
mod clip;
mod control;
mod engine;
mod link;
mod push;
mod sound;
mod steps;

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use cpal::traits::DeviceTrait;
use slint::{Color, ComponentHandle, Model, ModelRc, SharedString, VecModel};

use clip::{SCENES, TRACKS};
use control::{Controls, Mix};
use engine::{Quantize, Studio};

slint::include_modules!();

/// The demo set's tempo.
const TEMPO: f64 = 124.0;

const TRACK_COLORS: [u32; TRACKS] = [0xff8a65, 0x4fc3f7, 0xb69cff, 0xf48fb1];

fn color(rgb: u32) -> Color {
    Color::from_rgb_u8((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
}

/// What the device row calls a track's instrument.
fn device_name(sound: clip::Sound) -> String {
    match sound {
        clip::Sound::Drums(kit) => format!("Drum Kit · {kit}"),
        clip::Sound::Bass => "Bass Synth".into(),
        clip::Sound::Pad => "Pad Synth".into(),
        clip::Sound::Lead => "Lead Synth".into(),
    }
}

/// A meter's next height (0..1 over -60..+6 dB) from its last height and
/// the peak since: jumps up, falls back about 20 dB a second.
fn meter(last: f32, peak: f32) -> f32 {
    let now = if peak > 0.0 {
        ((20.0 * peak.log10() + 60.0) / 66.0).clamp(0.0, 1.0)
    } else {
        0.0
    };
    now.max(last - 0.01)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--check") => std::process::exit(check::run()),
        Some("--render") => {
            let path = args.get(1).map(String::as_str).unwrap_or("studio.wav");
            std::process::exit(check::render(path));
        }
        Some("--link-check") => std::process::exit(check::link()),
        Some("--push-preview") => {
            let path = args.get(1).map(String::as_str).unwrap_or("push.png");
            std::process::exit(check::push_preview(path));
        }
        Some(other) => {
            eprintln!(
                "unknown option {other}; use --check, --render <file.wav>, --link-check or --push-preview <file.png>"
            );
            std::process::exit(2);
        }
        None => {}
    }
    if let Err(e) = run_ui() {
        eprintln!("openDJ Studio: {e}");
        std::process::exit(1);
    }
}

fn run_ui() -> Result<(), slint::PlatformError> {
    let set = clip::demo_set();
    let device = if std::env::var_os("OPENDJ_NO_AUDIO").is_some() {
        None
    } else {
        audio::probe()
    };
    let rate = device
        .as_ref()
        .map_or(48_000.0, |(_, c)| c.sample_rate() as f64);
    let (mut studio, remote) = Studio::new(&set, rate, TEMPO);
    let mut link_clock = link::LinkClock::new(TEMPO, rate);
    link_clock.set_latency(audio::BUFFER_FRAMES as f64 / rate);
    let link = link_clock.handle();
    studio.link = Some(link_clock);

    let (output, status) = match &device {
        Some((dev, config)) => {
            let name = dev
                .description()
                .map(|d| d.name().to_string())
                .unwrap_or_else(|_| "audio device".into());
            match audio::start(dev, config, studio) {
                Ok(out) => (
                    out,
                    format!(
                        "{name} · {} Hz · {} frames",
                        config.sample_rate(),
                        audio::BUFFER_FRAMES
                    ),
                ),
                Err(e) => {
                    eprintln!("audio output: {e}");
                    // The studio moved into the failed stream; start again
                    // without sound.
                    let (studio, remote2) = Studio::new(&set, rate, TEMPO);
                    return ui(
                        set,
                        audio::start_silent(studio),
                        remote2,
                        link,
                        format!("No sound: {e}"),
                    );
                }
            }
        }
        None => (
            audio::start_silent(studio),
            "No audio output found: playing silently".to_string(),
        ),
    };
    ui(set, output, remote, link, status)
}

fn ui(
    set: Vec<clip::Track>,
    output: audio::Output,
    remote: engine::Remote,
    link: Arc<rusty_link::AblLink>,
    status: String,
) -> Result<(), slint::PlatformError> {
    let window = AppWindow::new()?;
    let monitor = remote.monitor.clone();
    let controls = Arc::new(Controls::new(
        &set,
        &TRACK_COLORS,
        remote.commands,
        monitor.clone(),
    ));
    let tracks = Rc::new(VecModel::from(
        (0..set.len())
            .map(|i| track_row(&controls, i))
            .collect::<Vec<_>>(),
    ));
    let slots = Rc::new(VecModel::from(
        (0..SCENES)
            .flat_map(|s| {
                set.iter().enumerate().map(move |(t, track)| {
                    let clip = track.slots[s].as_ref();
                    SlotData {
                        name: clip.map_or(SharedString::new(), |c| c.name.as_str().into()),
                        color: color(TRACK_COLORS[t]),
                        state: if clip.is_some() {
                            SlotState::Stopped
                        } else {
                            SlotState::Empty
                        },
                        beats: clip.map_or(0.0, |c| c.beats as f32),
                    }
                })
            })
            .collect::<Vec<_>>(),
    ));
    window.set_tracks(ModelRc::from(tracks.clone()));
    window.set_slots(ModelRc::from(slots.clone()));
    window.set_scenes(SCENES as i32);
    window.set_status(status.clone().into());
    window.set_quantize(Quantize::Bar.label().into());
    window.set_main_volume(controls.main_volume());

    let trash = RefCell::new(remote.trash);
    let push_status: push::Status = Arc::new(Mutex::new(String::new()));
    let push = push::start(controls.clone(), push_status.clone());

    {
        let c = controls.clone();
        window.on_launch_clip(move |t, s| c.launch(t as usize, s as usize));
    }
    {
        let c = controls.clone();
        window.on_select(move |t, s| c.select(t as usize, usize::try_from(s).ok()));
    }
    {
        let c = controls.clone();
        window.on_launch_scene(move |s| c.launch_scene(s as usize));
    }
    {
        let c = controls.clone();
        window.on_stop_track(move |t| c.stop(t as usize));
    }
    {
        let c = controls.clone();
        window.on_stop_all(move || c.stop_all());
    }
    {
        let c = controls.clone();
        window.on_toggle_play(move || c.toggle_play());
    }
    {
        let c = controls.clone();
        window.on_tempo_step(move |d| c.tempo_step(d as f64));
    }
    {
        let link = link.clone();
        window.on_toggle_link(move || {
            let link = link.clone();
            // Opening and closing sockets: off the UI thread.
            std::thread::spawn(move || link.enable(!link.is_enabled()));
        });
    }
    {
        let c = controls.clone();
        window.on_next_quantize(move || {
            c.next_quantize();
        });
    }
    {
        let c = controls.clone();
        window.on_mix(move |t, which, v| {
            if let Some(&m) = Mix::ALL.get(which as usize) {
                c.set_mix(t as usize, m, v);
            }
        });
    }
    {
        let c = controls.clone();
        window.on_main_volume_changed(move |v| c.set_main_volume(v));
    }
    {
        let c = controls.clone();
        window.on_param(move |t, i, v| c.set_param(t as usize, i as usize, v));
    }
    {
        let c = controls.clone();
        window.on_play_note(move |t, p, v| c.note(t as usize, p.clamp(0, 127) as u8, v));
    }

    let timer = slint::Timer::default();
    let w = window.as_weak();
    let mut seen = 0u64;
    timer.start(
        slint::TimerMode::Repeated,
        Duration::from_millis(33),
        move || {
            let Some(w) = w.upgrade() else { return };
            // Programs the audio thread replaced are dropped here.
            while trash.borrow_mut().pop().is_ok() {}
            for s in 0..SCENES {
                for t in 0..TRACKS {
                    let i = s * TRACKS + t;
                    let state = match monitor.slot(t, s) {
                        engine::SlotState::Empty => SlotState::Empty,
                        engine::SlotState::Stopped => SlotState::Stopped,
                        engine::SlotState::Queued => SlotState::Queued,
                        engine::SlotState::Playing => SlotState::Playing,
                    };
                    if let Some(mut row) = slots.row_data(i) {
                        if row.state != state {
                            row.state = state;
                            slots.set_row_data(i, row);
                        }
                    }
                }
            }
            // Mixer, knobs and selection: from either the window or Push.
            let version = controls.version();
            let changed = version != seen;
            seen = version;
            if changed {
                // Clips Push made or changed in Steps mode.
                for i in 0..slots.row_count() {
                    let (t, sc) = (i % TRACKS, i / TRACKS);
                    let info = controls.clip_info(t, sc);
                    if let Some(mut row) = slots.row_data(i) {
                        let name: SharedString = info
                            .as_ref()
                            .map_or(SharedString::new(), |(n, _)| n.as_str().into());
                        let beats = info.as_ref().map_or(0.0, |(_, b)| *b as f32);
                        if row.name != name || row.beats != beats {
                            row.name = name;
                            row.beats = beats;
                            slots.set_row_data(i, row);
                        }
                    }
                }
                w.set_selected_track(controls.selected_track() as i32);
                w.set_selected_scene(controls.selected_scene().map_or(-1, |s| s as i32));
                w.set_main_volume(controls.main_volume());
                w.set_quantize(controls.quantize().label().into());
            }
            for t in 0..tracks.row_count() {
                if let Some(mut row) = tracks.row_data(t) {
                    let progress = monitor.progress(t);
                    let stopping = monitor.stopping(t);
                    let level = meter(row.level, monitor.take_peak(t));
                    if changed {
                        let fresh = track_row(&controls, t);
                        row.volume = fresh.volume;
                        row.pan = fresh.pan;
                        row.pan_text = fresh.pan_text;
                        row.send_a = fresh.send_a;
                        row.send_b = fresh.send_b;
                        row.volume_text = fresh.volume_text;
                        row.params = fresh.params;
                    }
                    if changed
                        || (row.progress - progress).abs() > 0.002
                        || row.stopping != stopping
                        || (row.level - level).abs() > 0.005
                    {
                        row.progress = progress;
                        row.stopping = stopping;
                        row.level = level;
                        tracks.set_row_data(t, row);
                    }
                }
            }
            w.set_main_level(meter(w.get_main_level(), monitor.take_main_peak()));
            let beat = monitor.beat();
            let bar = (beat / engine::QUANTUM).floor();
            let in_bar = beat.rem_euclid(engine::QUANTUM).floor();
            w.set_position(format!("{}.{}", bar as i64 + 1, in_bar as i64 + 1).into());
            w.set_tempo(format!("{:.1}", monitor.tempo()).into());
            w.set_playing(monitor.playing());
            w.set_blink(beat.rem_euclid(1.0) < 0.5);
            w.set_link(link.is_enabled());
            w.set_peers(link.num_peers() as i32);
            w.set_load(format!("{:.0}%", monitor.load() * 100.0).into());
            let push = push_status.lock().map(|s| s.clone()).unwrap_or_default();
            let line = if push.is_empty() {
                status.clone()
            } else {
                format!("{status} · {push}")
            };
            if w.get_status().as_str() != line {
                w.set_status(line.into());
            }
        },
    );

    let result = window.run();
    // Push back to Live before the sound stops.
    drop(push);
    drop(output);
    result
}

/// A track's row for the window, from the shared controls.
fn track_row(c: &Controls, t: usize) -> TrackData {
    let info = &c.tracks[t];
    let params: Vec<ParamData> = (0..sound::PARAMS)
        .map(|i| ParamData {
            name: c.param_name(t, i).into(),
            value: c.param(t, i),
            text: c.param_text(t, i).into(),
        })
        .collect();
    let volume = c.mix(t, Mix::Volume);
    let pan = c.mix(t, Mix::Pan);
    TrackData {
        name: info.name.as_str().into(),
        color: color(info.color),
        progress: c.monitor.progress(t),
        stopping: c.monitor.stopping(t),
        device: device_name(info.sound).into(),
        drums: info.drums(),
        level: 0.0,
        volume,
        volume_text: control::fader_text(volume).into(),
        pan,
        pan_text: control::pan_text(pan).into(),
        send_a: c.mix(t, Mix::SendA),
        send_b: c.mix(t, Mix::SendB),
        params: ModelRc::new(VecModel::from(params)),
    }
}
