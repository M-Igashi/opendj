//! Running the studio on the sound card, or on a timer when there is none.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample};

use crate::engine::{Studio, MAX_BLOCK};

/// Frames per callback we ask for: 5.3 ms at 48 kHz.
pub const BUFFER_FRAMES: u32 = 256;

/// Keeps the audio running; stops it when dropped.
pub enum Output {
    /// Held only to keep the stream playing.
    Device(#[allow(dead_code)] cpal::Stream),
    /// No sound card: the engine runs on a timer so the UI still moves.
    Silent(Arc<AtomicBool>),
}

impl Drop for Output {
    fn drop(&mut self) {
        if let Output::Silent(stop) = self {
            stop.store(true, Ordering::Relaxed);
        }
    }
}

/// The default output's sample rate and a name for the status line, or
/// None when there is no output.
pub fn probe() -> Option<(cpal::Device, cpal::SupportedStreamConfig)> {
    let device = cpal::default_host().default_output_device()?;
    let config = device.default_output_config().ok()?;
    Some((device, config))
}

/// Starts `studio` on `device`.
pub fn start(
    device: &cpal::Device,
    config: &cpal::SupportedStreamConfig,
    studio: Studio,
) -> Result<Output, String> {
    let open = |studio: Studio, size| match config.sample_format() {
        SampleFormat::F32 => build::<f32>(device, config, size, studio),
        SampleFormat::I16 => build::<i16>(device, config, size, studio),
        SampleFormat::I32 => build::<i32>(device, config, size, studio),
        other => Err(format!("unsupported sample format {other:?}")),
    };
    let size = match config.buffer_size() {
        cpal::SupportedBufferSize::Range { min, max } => {
            cpal::BufferSize::Fixed(BUFFER_FRAMES.clamp(*min, *max))
        }
        cpal::SupportedBufferSize::Unknown => cpal::BufferSize::Default,
    };
    let stream = open(studio, size)?;
    stream.play().map_err(|e| e.to_string())?;
    Ok(Output::Device(stream))
}

fn build<T>(
    device: &cpal::Device,
    config: &cpal::SupportedStreamConfig,
    buffer_size: cpal::BufferSize,
    mut studio: Studio,
) -> Result<cpal::Stream, String>
where
    T: SizedSample + FromSample<f32>,
{
    let channels = config.channels() as usize;
    let rate = config.sample_rate() as f64;
    // Allocated here, never in the callback.
    let mut stereo = vec![[0.0f32; 2]; MAX_BLOCK];
    let monitor = studio.monitor.clone();
    let mut load = 0.0f32;
    device
        .build_output_stream::<T, _, _>(
            cpal::StreamConfig {
                buffer_size,
                ..config.config()
            },
            move |data: &mut [T], _| {
                let began = Instant::now();
                for chunk in data.chunks_mut(MAX_BLOCK * channels) {
                    let frames = chunk.len() / channels;
                    let buf = &mut stereo[..frames];
                    studio.process(buf);
                    for (frame, s) in chunk.chunks_mut(channels).zip(buf.iter()) {
                        for (c, d) in frame.iter_mut().enumerate() {
                            *d = T::from_sample(if c < 2 { s[c] } else { 0.0 });
                        }
                    }
                }
                let frames = data.len() / channels.max(1);
                let budget = frames as f32 / rate as f32;
                let used = began.elapsed().as_secs_f32() / budget.max(1e-6);
                load += (used - load) * 0.05;
                monitor.set_load(load);
            },
            |e| eprintln!("audio output: {e}"),
            None,
        )
        .map_err(|e| e.to_string())
}

/// Runs `studio` in real time on a thread, sending the sound nowhere.
pub fn start_silent(mut studio: Studio) -> Output {
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    let rate = studio.rate();
    std::thread::Builder::new()
        .name("studio-silent-audio".into())
        .spawn(move || {
            let mut buf = vec![[0.0f32; 2]; BUFFER_FRAMES as usize];
            let block = Duration::from_secs_f64(BUFFER_FRAMES as f64 / rate);
            let mut next = Instant::now();
            while !flag.load(Ordering::Relaxed) {
                let began = Instant::now();
                studio.process(&mut buf);
                let used = began.elapsed().as_secs_f32() / block.as_secs_f32();
                studio.monitor.set_load(used);
                next += block;
                if let Some(wait) = next.checked_duration_since(Instant::now()) {
                    std::thread::sleep(wait);
                }
            }
        })
        .expect("audio thread starts");
    Output::Silent(stop)
}
