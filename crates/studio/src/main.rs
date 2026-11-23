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
mod engine;
mod link;
mod sound;

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use cpal::traits::DeviceTrait;
use slint::{Color, ComponentHandle, Model, ModelRc, SharedString, VecModel};

use clip::{SCENES, TRACKS};
use engine::{Command, Quantize, Studio};

slint::include_modules!();

/// The demo set's tempo.
const TEMPO: f64 = 124.0;

const TRACK_COLORS: [u32; TRACKS] = [0xff8a65, 0x4fc3f7, 0xb69cff, 0xf48fb1];

fn color(rgb: u32) -> Color {
    Color::from_rgb_u8((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
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
        Some(other) => {
            eprintln!("unknown option {other}; use --check, --render <file.wav> or --link-check");
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
    let tracks = Rc::new(VecModel::from(
        set.iter()
            .enumerate()
            .map(|(i, t)| TrackData {
                name: t.name.as_str().into(),
                color: color(TRACK_COLORS[i]),
                progress: -1.0,
                stopping: false,
            })
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
                    }
                })
            })
            .collect::<Vec<_>>(),
    ));
    window.set_tracks(ModelRc::from(tracks.clone()));
    window.set_slots(ModelRc::from(slots.clone()));
    window.set_scenes(SCENES as i32);
    window.set_status(status.into());
    window.set_quantize(Quantize::Bar.label().into());

    let monitor = remote.monitor.clone();
    let commands = Rc::new(RefCell::new(remote.commands));
    let trash = RefCell::new(remote.trash);
    let send = {
        let commands = commands.clone();
        move |cmd: Command| {
            if commands.borrow_mut().push(cmd).is_err() {
                eprintln!("command queue full");
            }
        }
    };
    let send = Rc::new(send);

    {
        let send = send.clone();
        window.on_launch_clip(move |t, s| {
            send(Command::Launch {
                track: t as usize,
                scene: s as usize,
            })
        });
    }
    {
        let send = send.clone();
        window.on_launch_scene(move |s| send(Command::Scene(s as usize)));
    }
    {
        let send = send.clone();
        window.on_stop_track(move |t| send(Command::Stop(t as usize)));
    }
    {
        let send = send.clone();
        window.on_stop_all(move || send(Command::StopAll));
    }
    {
        let send = send.clone();
        let monitor = monitor.clone();
        window.on_toggle_play(move || send(Command::Play(!monitor.playing())));
    }
    {
        let send = send.clone();
        let monitor = monitor.clone();
        window.on_tempo_step(move |d| {
            let bpm = (monitor.tempo().round() + d as f64).clamp(40.0, 250.0);
            send(Command::Tempo(bpm));
        });
    }
    {
        let link = link.clone();
        window.on_toggle_link(move || {
            let link = link.clone();
            // Opening and closing sockets: off the UI thread.
            std::thread::spawn(move || link.enable(!link.is_enabled()));
        });
    }
    let quantize = Rc::new(Cell::new(Quantize::Bar));
    {
        let send = send.clone();
        let quantize = quantize.clone();
        let w = window.as_weak();
        window.on_next_quantize(move || {
            let q = quantize.get().next();
            quantize.set(q);
            send(Command::Quantize(q));
            if let Some(w) = w.upgrade() {
                w.set_quantize(q.label().into());
            }
        });
    }

    let timer = slint::Timer::default();
    let w = window.as_weak();
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
            for t in 0..TRACKS {
                if let Some(mut row) = tracks.row_data(t) {
                    let progress = monitor.progress(t);
                    let stopping = monitor.stopping(t);
                    if (row.progress - progress).abs() > 0.002 || row.stopping != stopping {
                        row.progress = progress;
                        row.stopping = stopping;
                        tracks.set_row_data(t, row);
                    }
                }
            }
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
        },
    );

    let result = window.run();
    drop(output);
    result
}
