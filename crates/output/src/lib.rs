// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The audio device: lists the system's hosts and output devices and plays an engine
//! [`Renderer`] on the one chosen ([`OutputChoice`]), or on the default output.
//!
//! The renderer is built on the audio owner's thread and then moved to a render thread of its
//! own, which runs it ahead of the device in blocks of the chosen buffer size (see
//! [`engine_block`]) into a lock-free ring ([`rtrb`]); the device's callback only copies from
//! the ring, converts samples and sets atomics: no allocation, locks or I/O on either thread
//! (AGENTS.md rule 3). The plug-ins of a block get a share of that block's playing time
//! (ADR 0006), so the deadline follows the buffer the user chose. The ring keeps two blocks
//! ahead, or a device buffer and a block when the device asks for more at once; while a track
//! is armed to play a MIDI controller, one block or a device buffer, so a key sounds sooner
//! (ADR 0017). Launcher
//! commands reach the renderer through its lock-free command queue, and stretched clips are
//! rendered ahead on a worker thread of their own (ADR 0003).
//!
//! The ring carries four samples a frame: the main output's left and right, then the cue's
//! (the engine's cue bus, [`Renderer::process_with_cue`]). The device's callback writes the
//! main pair to channels 1 and 2 and, when the chosen cue pair is on the device
//! ([`OutputChoice::cue_pair`]), the cue to that pair; otherwise the cue plays with the main
//! output (the engine adds the preview there) and is not written.
//!
//! Hosts are cpal's (WASAPI shared on Windows, ASIO with the `asio` feature, ALSA, JACK,
//! CoreAudio), and WASAPI exclusive mode through the `wasapi` crate. A stream is opened and
//! closed on the audio owner's thread, never on the render or device thread. The buffer size
//! chosen is fitted to what the device declares ([`devices`]) and a driver is never asked for a
//! size outside the range it reports; where the device's own buffer is not ours to set (WASAPI
//! shared mode, where Windows plays the period it gives the device) the number is the engine's
//! block alone, and `audio.info` reports the device's measured buffer next to it.

pub mod devices;
#[cfg(windows)]
mod exclusive;
mod input;

use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SizedSample};
use suhina_engine::model::Project;
use suhina_engine::{AudioPool, Controller, LaunchCommand, Plugins, Renderer};
pub use suhina_realtime::Priority;
use suhina_realtime::RealTime;
use suhina_session::Player;
use suhina_session::audio::{
    AudioSystem, HostInfo, LiveOutput, MAX_BUFFER_FRAMES, MIN_BUFFER_FRAMES, Opened, OutputChoice, OutputInfo,
};

/// Samples per frame in the ring: the main output's left and right, then the cue's.
pub(crate) const RING_CHANNELS: usize = 4;

/// Frames the engine renders per block when no buffer size is chosen.
pub const ENGINE_BLOCK: usize = 1024;
/// Blocks the ring keeps ahead of the device: the latency from the engine to the device is up
/// to this many blocks (more when the device takes more than a block at once).
const RING_BLOCKS: usize = 2;
/// Blocks the ring keeps ahead while a track is armed (ADR 0017): a key pressed waits for one
/// block less, and the render thread has one device buffer's time to fill the next.
const LIVE_RING_BLOCKS: usize = 1;
/// The largest device buffer the ring makes room for, in frames.
const MAX_DEVICE_FRAMES: usize = 8192;
/// How long the render thread waits when the ring is full before looking again.
const RENDER_WAIT: Duration = Duration::from_millis(1);
/// How long closing a stream may take before its plug-ins are given up.
const CLOSE_TIMEOUT: Duration = Duration::from_secs(5);
/// How long starting a stream waits for the render thread to say what priority it got: longer
/// than asking rtkit may take ([`suhina_realtime`]). The thread plays on whatever it got.
const PRIORITY_WAIT: Duration = Duration::from_secs(3);

/// Whether an ASIO stream is open: only one ASIO driver can be loaded, so listing ASIO devices
/// meanwhile shows the ones listed before.
static ASIO_OPEN: AtomicBool = AtomicBool::new(false);

/// Whether the user has been told that the render thread could not have real-time priority:
/// once per process, not at every stream.
static PRIORITY_TOLD: AtomicBool = AtomicBool::new(false);

/// The device's end of the ring.
pub(crate) type Ring = rtrb::Consumer<f32>;

/// State the audio callback shares with the main thread.
#[derive(Default)]
pub struct Progress {
    frames: AtomicU64,
    underruns: AtomicU64,
    stream_error: AtomicBool,
    /// Frames the device asked for in its last callback.
    callback_frames: AtomicUsize,
    /// The last measured latency, engine to speaker, in microseconds.
    latency_us: AtomicU64,
    /// The render thread keeps the shallower queue of live playing (a track is armed).
    live: AtomicBool,
    /// The input device's own latency, measured in its callback, in frames: from the last frame
    /// of a buffer being captured to the buffer reaching the ring.
    input_latency: AtomicUsize,
    /// Frames the engine renders per block, set when the render thread starts.
    block: AtomicUsize,
    /// The render thread, woken when the device has taken from the ring.
    render: std::sync::OnceLock<std::thread::Thread>,
}

impl Progress {
    /// Frames the device has been given so far.
    pub fn frames(&self) -> u64 {
        self.frames.load(Ordering::Relaxed)
    }

    /// Device buffers the render thread had not filled in time, so far: those played silence
    /// for what was missing.
    pub fn underruns(&self) -> u64 {
        self.underruns.load(Ordering::Relaxed)
    }

    /// Whether the device reported an error.
    pub fn failed(&self) -> bool {
        self.stream_error.load(Ordering::Relaxed)
    }

    /// Frames the device asked for in its last callback; 0 before the first.
    pub fn callback_frames(&self) -> usize {
        self.callback_frames.load(Ordering::Relaxed)
    }

    /// The last measured time from the engine rendering a frame to the device playing it.
    pub fn latency(&self) -> Duration {
        Duration::from_micros(self.latency_us.load(Ordering::Relaxed))
    }

    /// Whether the render thread keeps the shallower queue of live playing (ADR 0017).
    pub fn live(&self) -> bool {
        self.live.load(Ordering::Relaxed)
    }

    /// The time from a key pressed on a MIDI controller to the speaker: the measured latency,
    /// and the fixed delay a stamped note is played after (#269), the longest the render thread
    /// waits between blocks (a block, or a device buffer) and one block more.
    pub fn live_latency(&self, sample_rate: u32) -> Duration {
        let block = self.block.load(Ordering::Relaxed);
        let delay = block.max(self.callback_frames()).saturating_add(block);
        self.latency().saturating_add(Duration::from_secs_f64(delay as f64 / f64::from(sample_rate.max(1))))
    }

    /// The input device's own latency in frames, as last measured; 0 without one.
    pub fn input_latency(&self) -> usize {
        self.input_latency.load(Ordering::Relaxed)
    }

    pub(crate) fn set_input_latency(&self, frames: usize) {
        self.input_latency.store(frames, Ordering::Relaxed);
    }

    pub(crate) fn fail(&self) {
        self.stream_error.store(true, Ordering::Relaxed);
    }

    pub(crate) fn set_latency(&self, latency: Duration) {
        self.latency_us.store(u64::try_from(latency.as_micros()).unwrap_or(u64::MAX), Ordering::Relaxed);
    }

    pub(crate) fn set_callback_frames(&self, frames: usize) {
        self.callback_frames.store(frames, Ordering::Relaxed);
    }

    pub(crate) fn add_frames(&self, frames: usize) {
        self.frames.fetch_add(frames as u64, Ordering::Relaxed);
    }

    /// Wakes the render thread, if there is one, to fill what the device took: no waiting for
    /// its next look, which a coarse system timer can make late. Lock-free, never allocates.
    pub(crate) fn wake_render(&self) {
        if let Some(render) = self.render.get() {
            render.unpark();
        }
    }
}

/// Fills `out` (whole frames of [`RING_CHANNELS`] samples) from the ring, with silence, counted
/// as an underrun, for what is not there yet. Never allocates.
pub(crate) fn read_ring(ring: &mut Ring, out: &mut [f32], progress: &Progress) {
    let ready = ring.slots().min(out.len()) / RING_CHANNELS * RING_CHANNELS;
    match ring.read_chunk(ready) {
        Ok(chunk) => {
            for (sample, slot) in chunk.into_iter().zip(out.iter_mut()) {
                *slot = sample;
            }
        }
        Err(_) => out.fill(0.0),
    }
    if ready < out.len() {
        if let Some(missing) = out.get_mut(ready..) {
            missing.fill(0.0);
        }
        progress.underruns.fetch_add(1, Ordering::Relaxed);
    }
    progress.wake_render();
}

/// The time from the engine to the speaker: the frames queued in the ring, played at
/// `sample_rate`, and the device's own delay after the callback.
pub(crate) fn measure_latency(queued_frames: usize, device: Duration, sample_rate: u32) -> Duration {
    let ring = Duration::from_secs_f64(queued_frames as f64 / f64::from(sample_rate.max(1)));
    ring.saturating_add(device)
}

/// The frames the engine renders per block: `SUHINA_ENGINE_BLOCK` (64 to 4096) when set, to
/// try a size in tests; else the buffer size chosen, within 64 … 4096; else [`ENGINE_BLOCK`].
pub fn engine_block(chosen: Option<u32>) -> usize {
    let overridden = std::env::var("SUHINA_ENGINE_BLOCK").ok();
    block_for(chosen, overridden.as_deref())
}

fn block_for(chosen: Option<u32>, overridden: Option<&str>) -> usize {
    let range = MIN_BUFFER_FRAMES as usize..=MAX_BUFFER_FRAMES as usize;
    overridden
        .and_then(|v| v.trim().parse::<usize>().ok())
        .filter(|n| range.contains(n))
        .or_else(|| chosen.map(|n| (n as usize).clamp(*range.start(), *range.end())))
        .unwrap_or(ENGINE_BLOCK)
}

/// Frames the render thread keeps ahead in the ring: two blocks, or a device buffer and a
/// block when the device takes more at once. While someone may be playing live (`live`, a
/// track armed), one block or one device buffer: the device's next callback is then all the
/// render thread has to fill the ring again, so underruns tell when it is too little.
fn ring_target(block: usize, device_frames: usize, live: bool) -> usize {
    let device = device_frames.min(MAX_DEVICE_FRAMES);
    if live { (LIVE_RING_BLOCKS * block).max(device) } else { (RING_BLOCKS * block).max(device + block) }
}

/// The first channel (counted from 0) the cue is written to on a device with `channels`
/// output channels, for the chosen cue `pair` (1 is channels 3 and 4): `None` when no pair is
/// chosen or the device does not have it, so the cue plays with the main output.
pub(crate) fn cue_channel(pair: Option<u16>, channels: u16) -> Option<usize> {
    let first = usize::from(pair.filter(|p| *p > 0)?) * 2;
    (first + 2 <= usize::from(channels)).then_some(first)
}

/// The channel count to open a device with so that the cue `pair` fits: the default's when it
/// is enough, else the fewest of `offered` (the counts the device offers at the rate) that
/// hold the pair; `None` when none does.
pub(crate) fn channels_for_cue(pair: u16, default: u16, offered: &[u16]) -> Option<u16> {
    let needed = u32::from(pair) * 2 + 2;
    if u32::from(default) >= needed {
        return Some(default);
    }
    offered.iter().copied().filter(|c| u32::from(*c) >= needed).min()
}

/// The sample for `channel` of a device with `channels` channels, from a ring frame (main left
/// and right, cue left and right): mono gets the main output's average, channels 1 and 2 the
/// main output, the cue's pair from `cue` the cue, the rest silence.
pub(crate) fn channel_sample(frame: &[f32; RING_CHANNELS], channels: usize, channel: usize, cue: Option<usize>) -> f32 {
    let [left, right, cue_left, cue_right] = *frame;
    match channel {
        _ if channels == 1 => (left + right) * 0.5,
        0 => left,
        1 => right,
        c if Some(c) == cue => cue_left,
        c if cue.is_some_and(|first| c == first + 1) => cue_right,
        _ => 0.0,
    }
}

/// The device's side of a playing stream.
enum DeviceStream {
    Cpal(cpal::Stream),
    #[cfg(windows)]
    Exclusive(exclusive::ExclusiveStream),
}

impl DeviceStream {
    fn play(&self) -> Result<(), String> {
        match self {
            Self::Cpal(stream) => stream.play().map_err(|e| e.to_string()),
            #[cfg(windows)]
            Self::Exclusive(stream) => {
                stream.play();
                Ok(())
            }
        }
    }

    /// Frames per device buffer, when the device says before it plays.
    fn buffer_frames(&self) -> Option<u32> {
        match self {
            Self::Cpal(stream) => stream.buffer_size().ok().filter(|n| *n > 0),
            #[cfg(windows)]
            Self::Exclusive(stream) => Some(stream.buffer_frames),
        }
    }
}

/// A playing stream: the device's, and the render thread that feeds it. Dropping it stops
/// both.
pub struct Stream {
    device: Option<DeviceStream>,
    stop: Arc<AtomicBool>,
    render: Option<std::thread::JoinHandle<Renderer>>,
    asio: bool,
    /// The render thread's priority; `None` when it did not say in time.
    priority: Option<Priority>,
    /// Why the render thread could not have real-time priority, the first time in this process.
    priority_notice: Option<String>,
}

impl Stream {
    /// The priority the render thread got; `None` when it did not say in time.
    pub fn priority(&self) -> Option<Priority> {
        self.priority
    }

    /// Why the render thread could not have real-time priority, for the user: given by the
    /// first stream in a process only, so it is told once.
    pub fn priority_notice(&self) -> Option<&str> {
        self.priority_notice.as_deref()
    }

    /// Stops the device, then the render thread, and gives back the renderer, so its plug-ins
    /// can move to the next one ([`Renderer::into_plugins`]).
    pub fn close(mut self) -> Option<Renderer> {
        self.stop_all()
    }

    fn stop_all(&mut self) -> Option<Renderer> {
        drop(self.device.take());
        if self.asio {
            ASIO_OPEN.store(false, Ordering::Release);
            self.asio = false;
        }
        self.stop.store(true, Ordering::Release);
        self.render.take().and_then(|render| render.join().ok())
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        let _renderer = self.stop_all();
    }
}

enum Backend {
    Cpal {
        device: cpal::Device,
        config: cpal::StreamConfig,
        format: cpal::SampleFormat,
    },
    /// WASAPI exclusive mode, on the device with this name (the default when `None`).
    #[cfg(windows)]
    Exclusive {
        device: Option<String>,
        buffer: Option<u32>,
        /// The chosen cue pair; whether the device has it is known once it is opened.
        cue_pair: Option<u16>,
    },
}

/// An output device, the format it is opened with and the engine's block on it.
pub struct Output {
    backend: Backend,
    host: String,
    device_name: String,
    sample_rate: u32,
    channels: u16,
    block: usize,
    exclusive: bool,
    /// The first device channel (from 0) the cue is written to, or `None` when it plays with
    /// the main output.
    cue: Option<usize>,
    /// The cue volume, linear.
    cue_gain: f32,
    /// What could not be had as chosen.
    notices: Vec<String>,
}

impl Output {
    /// The default output device in the format it asks for.
    pub fn open() -> Result<Self, String> {
        Self::choose(&OutputChoice::default())
    }

    /// The output `choice` names, made to fit what the device offers: a host or device that is
    /// not there, or a sample rate it does not offer, falls back to the default with a notice
    /// ([`Output::notices`]); a buffer size goes to the nearest the driver allows, and the
    /// device is never asked for one outside the range its driver declared — a driver whose
    /// sizes the engine cannot render in is asked for none and keeps its own, with a notice.
    pub fn choose(choice: &OutputChoice) -> Result<Self, String> {
        let mut notices = Vec::new();
        let default_host = cpal::default_host().id();
        let host_id = match &choice.host {
            Some(name) => match cpal::available_hosts().into_iter().find(|h| devices::same_name(h.name(), name)) {
                Some(id) => id,
                None => {
                    notices.push(format!("the audio host {name} is not available; {} plays", default_host.name()));
                    default_host
                }
            },
            None => default_host,
        };
        let host = cpal::host_from_id(host_id).map_err(|e| format!("audio host {}: {e}", host_id.name()))?;
        let host_name = host_id.name().to_string();
        let chosen = choice.device.as_ref().and_then(|name| {
            let found =
                host.output_devices().ok().and_then(|mut list| list.find(|d| devices::same_name(&d.to_string(), name)));
            if found.is_none() {
                notices.push(format!("the audio device {name} is not there; the default output plays"));
            }
            found
        });
        let device = match chosen {
            Some(device) => device,
            None => host.default_output_device().ok_or("no audio output device found")?,
        };
        let device_name = device.to_string();
        let default = device.default_output_config().map_err(|e| format!("audio output {device_name}: {e}"))?;

        #[cfg(windows)]
        if choice.exclusive && host_name == "WASAPI" {
            let sample_rate = choice.sample_rate.unwrap_or(default.sample_rate());
            // The device is asked for this period itself, so it stays within what the engine
            // renders in; the device aligns it further, or refuses it and says why (`exclusive`).
            let buffer = choice.buffer_frames.map(|frames| frames.clamp(MIN_BUFFER_FRAMES, MAX_BUFFER_FRAMES));
            let channels = default.channels().max(1);
            let cue = cue_channel(choice.cue_pair, channels);
            if let Some(pair) = choice.cue_pair.filter(|_| cue.is_none()) {
                notices.push(cue_missing(&device_name, pair, channels));
            }
            return Ok(Self {
                backend: Backend::Exclusive {
                    device: Some(device_name.clone()),
                    buffer,
                    cue_pair: cue.and(choice.cue_pair),
                },
                host: host_name,
                device_name,
                sample_rate,
                channels,
                block: engine_block(choice.buffer_frames),
                exclusive: true,
                cue,
                cue_gain: choice.cue_gain,
                notices,
            });
        }

        let mut supported = default;
        if let Some(rate) = choice.sample_rate.filter(|r| *r != supported.sample_rate()) {
            let channels = supported.channels();
            let format = supported.sample_format();
            let found = device.supported_output_configs().ok().and_then(|configs| {
                let fitting: Vec<_> = configs
                    .filter(|c| c.channels() == channels && (c.min_sample_rate()..=c.max_sample_rate()).contains(&rate))
                    .collect();
                let same_format = fitting.iter().position(|c| c.sample_format() == format);
                let index = same_format.or(if fitting.is_empty() { None } else { Some(0) })?;
                fitting.into_iter().nth(index).and_then(|c| c.try_with_sample_rate(rate))
            });
            match found {
                Some(config) => supported = config,
                None => notices.push(format!(
                    "{device_name} does not play at {rate} Hz; it plays at {} Hz",
                    supported.sample_rate()
                )),
            }
        }
        // A cue pair past the format's channels: the same rate and format with more channels.
        if let Some(pair) = choice.cue_pair.filter(|p| cue_channel(Some(*p), supported.channels()).is_none()) {
            let rate = supported.sample_rate();
            let format = supported.sample_format();
            let fitting: Vec<cpal::SupportedStreamConfigRange> = device
                .supported_output_configs()
                .map(|configs| {
                    configs.filter(|c| (c.min_sample_rate()..=c.max_sample_rate()).contains(&rate)).collect()
                })
                .unwrap_or_default();
            let offered: Vec<u16> = fitting.iter().map(cpal::SupportedStreamConfigRange::channels).collect();
            let wider = channels_for_cue(pair, supported.channels(), &offered).and_then(|channels| {
                let same = |c: &&cpal::SupportedStreamConfigRange| c.channels() == channels;
                let found = fitting.iter().filter(same).find(|c| c.sample_format() == format);
                found.or_else(|| fitting.iter().find(same)).and_then(|c| c.try_with_sample_rate(rate))
            });
            match wider {
                Some(config) => supported = config,
                None => notices.push(cue_missing(
                    &device_name,
                    pair,
                    offered.iter().copied().max().unwrap_or(supported.channels()),
                )),
            }
        }
        let range = devices::buffer_range(supported.buffer_size());
        let fitted = choice.buffer_frames.map(|wanted| devices::fit_for_host(&host_name, wanted, range));
        if let (Some(wanted), Some((block, asked))) = (choice.buffer_frames, fitted)
            && let Some(notice) = devices::buffer_notice(&host_name, &device_name, wanted, block, asked, range)
        {
            notices.push(notice);
        }
        let buffer = fitted.map(|(block, _)| block);
        let format = supported.sample_format();
        let mut config: cpal::StreamConfig = supported.into();
        if config.channels == 0 {
            return Err(format!("audio output {device_name} reports no channels"));
        }
        if let Some((_, Some(frames))) = fitted {
            config.buffer_size = cpal::BufferSize::Fixed(frames);
        }
        let exclusive = host_name == "ASIO";
        Ok(Self {
            backend: Backend::Cpal { device, config, format },
            host: host_name,
            device_name,
            sample_rate: config.sample_rate,
            channels: config.channels,
            block: engine_block(buffer),
            exclusive,
            cue: cue_channel(choice.cue_pair, config.channels),
            cue_gain: choice.cue_gain,
            notices,
        })
    }

    /// The pair the cue plays on (1 is channels 3 and 4), or `None` with the main output.
    pub fn cue_pair(&self) -> Option<u16> {
        self.cue.and_then(|first| u16::try_from(first / 2).ok())
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Frames the engine renders per block on this output.
    pub fn block(&self) -> usize {
        self.block
    }

    pub fn host(&self) -> &str {
        &self.host
    }

    /// What could not be had as chosen.
    pub fn notices(&self) -> &[String] {
        &self.notices
    }

    /// The device's name, host, sample rate and channels, for messages.
    pub fn describe(&self) -> String {
        let mode = match &self.backend {
            Backend::Cpal { format, .. } => format!("{format}"),
            #[cfg(windows)]
            Backend::Exclusive { .. } => "exclusive".to_string(),
        };
        let cue = match self.cue {
            Some(first) => format!(", cue on {}/{}", first + 1, first + 2),
            None => String::new(),
        };
        format!(
            "{} ({}, {} Hz, {} channels, {mode}, {}-frame blocks{cue})",
            self.device_name, self.host, self.sample_rate, self.channels, self.block
        )
    }

    /// Opens the device's stream, reading from `ring`; it plays once [`DeviceStream::play`]
    /// is called.
    fn open_device(&self, ring: Ring, progress: &Arc<Progress>) -> Result<DeviceStream, String> {
        match &self.backend {
            Backend::Cpal { device, config, format } => {
                let (device, config, cue) = (device, *config, self.cue);
                let stream = match format {
                    cpal::SampleFormat::F32 => build::<f32>(device, config, cue, ring, progress),
                    cpal::SampleFormat::I16 => build::<i16>(device, config, cue, ring, progress),
                    cpal::SampleFormat::I32 => build::<i32>(device, config, cue, ring, progress),
                    cpal::SampleFormat::U16 => build::<u16>(device, config, cue, ring, progress),
                    other => {
                        return Err(format!(
                            "audio output {device} uses sample format {other}, which is not supported yet"
                        ));
                    }
                }?;
                Ok(DeviceStream::Cpal(stream))
            }
            #[cfg(windows)]
            Backend::Exclusive { device, buffer, cue_pair } => {
                exclusive::open(device.clone(), self.sample_rate, *buffer, *cue_pair, ring, progress)
                    .map(DeviceStream::Exclusive)
            }
        }
    }

    /// Starts playing `renderer` on the device, rendered ahead on a thread of its own, with
    /// its stretched clips rendered ahead on a worker thread that `controller` keeps (and
    /// stops when it is dropped). The stream plays until it is dropped.
    pub fn start(
        &self,
        mut renderer: Renderer,
        controller: &mut Controller,
        progress: &Arc<Progress>,
    ) -> Result<Stream, String> {
        let (producer, consumer) = ring(self.block);
        let device = self.open_device(consumer, progress)?;
        controller.run_stretch_worker(&mut renderer).map_err(|e| format!("could not start the stretch worker: {e}"))?;
        self.play(device, renderer, producer, progress)
    }

    fn play(
        &self,
        device: DeviceStream,
        mut renderer: Renderer,
        producer: rtrb::Producer<f32>,
        progress: &Arc<Progress>,
    ) -> Result<Stream, String> {
        // The cue's volume, and whether it has outputs of its own, apply from the first block.
        renderer.handle(LaunchCommand::SetCue { gain: Some(self.cue_gain), separate: Some(self.cue.is_some()) });
        if let Some(frames) = device.buffer_frames() {
            progress.set_callback_frames(frames as usize);
        }
        let stop = Arc::new(AtomicBool::new(false));
        let (stopping, watching, block) = (Arc::clone(&stop), Arc::clone(progress), self.block);
        let (joined, priority) = mpsc::sync_channel(1);
        let render = std::thread::Builder::new()
            .name("suhina-render".into())
            .spawn(move || render_ahead(renderer, producer, block, &stopping, &watching, &joined))
            .map_err(|e| format!("could not start the render thread: {e}"))?;
        let _set = progress.render.set(render.thread().clone());
        let asio = self.host == "ASIO";
        if asio {
            ASIO_OPEN.store(true, Ordering::Release);
        }
        // Before the device starts: the render thread has its priority, and the user hears of a
        // refusal, before anything plays.
        let (priority, notice) = priority.recv_timeout(PRIORITY_WAIT).map_or((None, None), |(p, n)| (Some(p), n));
        let priority_notice = tell_once(&PRIORITY_TOLD, notice);
        let stream = Stream { device: Some(device), stop, render: Some(render), asio, priority, priority_notice };
        stream
            .device
            .as_ref()
            .map_or(Ok(()), DeviceStream::play)
            .map_err(|e| format!("could not start audio output {}: {e}", self.device_name))?;
        Ok(stream)
    }
}

/// A ring for blocks of `block` frames: room for the target and a device buffer more.
fn ring(block: usize) -> (rtrb::Producer<f32>, Ring) {
    rtrb::RingBuffer::<f32>::new((ring_target(block, MAX_DEVICE_FRAMES, false) + block) * RING_CHANNELS)
}

/// The render thread: runs the engine a block at a time whenever the ring holds less than its
/// target, until told to stop; then gives the renderer back. Real-time: at the highest priority
/// the system allows ([`RealTime`]), which it says through `joined` before it renders, and never
/// allocating after its scratch block.
fn render_ahead(
    mut renderer: Renderer,
    mut ring: rtrb::Producer<f32>,
    frames: usize,
    stop: &AtomicBool,
    progress: &Progress,
    joined: &mpsc::SyncSender<(Priority, Option<String>)>,
) -> Renderer {
    let mut realtime = RealTime::join(frames, renderer.sample_rate());
    let _unheard = joined.try_send((realtime.priority(), realtime.notice()));
    let mut block = vec![0.0f32; frames * 2];
    let mut cue = vec![0.0f32; frames * 2];
    let capacity = ring.buffer().capacity();
    progress.block.store(frames, Ordering::Relaxed);
    while !stop.load(Ordering::Acquire) {
        let queued = (capacity - ring.slots()) / RING_CHANNELS;
        // Arming shrinks the queue as the device drains it, and disarming fills it again: no
        // jump either way.
        let live = renderer.live_armed();
        progress.live.store(live, Ordering::Relaxed);
        let target = ring_target(frames, progress.callback_frames(), live);
        if queued >= target || ring.slots() < frames * RING_CHANNELS {
            std::thread::park_timeout(RENDER_WAIT);
            realtime.waited();
            continue;
        }
        let started = Instant::now();
        // A stamped live note plays this long after it was played (#269): the longest the
        // thread waits between blocks (a block, or a device buffer that takes several at once)
        // and the block it lands in.
        let delay = frames.max(progress.callback_frames()).saturating_add(frames);
        renderer.set_live_clock(suhina_engine::live_now(), u32::try_from(delay).unwrap_or(u32::MAX));
        // A take is lined up with what was heard (docs/spec/recording.md): the output's
        // latency and the input device's, in frames.
        let output = progress.latency().as_secs_f64() * f64::from(renderer.sample_rate());
        let input = u32::try_from(progress.input_latency()).unwrap_or(u32::MAX);
        renderer.set_record_latency(output.round().clamp(0.0, f64::from(u32::MAX)) as u32, input);
        renderer.process_with_cue(&mut block, &mut cue);
        // A device that takes faster than it plays, or a project too heavy to render in time,
        // keeps the thread from waiting: it rests rather than outlive its real-time limit.
        realtime.worked(started.elapsed());
        if let Ok(chunk) = ring.write_chunk_uninit(frames * RING_CHANNELS) {
            let frames = block.as_chunks::<2>().0.iter().zip(cue.as_chunks::<2>().0);
            chunk.fill_from_iter(frames.flat_map(|([l, r], [cl, cr])| [*l, *r, *cl, *cr]));
        }
    }
    renderer
}

/// A stream playing a project with its transport stopped, and what it needs to be heard.
pub struct Stopped {
    pub controller: Controller,
    pub output: Output,
    pub progress: Arc<Progress>,
    pub stream: Stream,
}

/// `notice` the first time one is given with `told` not yet set, `None` after: what is told once
/// per process.
fn tell_once(told: &AtomicBool, notice: Option<String>) -> Option<String> {
    notice.filter(|_| !told.swap(true, Ordering::Relaxed))
}

/// Opens the default output and plays `project` on it with the transport stopped.
pub fn start_stopped(project: &Project, audio: &AudioPool) -> Result<Stopped, String> {
    let output = Output::open()?;
    let mut renderer = Renderer::stopped(project, audio, output.sample_rate).map_err(|e| e.to_string())?;
    let mut controller = renderer.controller();
    let progress = Arc::new(Progress::default());
    let stream = output.start(renderer, &mut controller, &progress)?;
    Ok(Stopped { controller, output, progress, stream })
}

/// Opens the device for `choice` and its stream: as chosen, then without a buffer size of its
/// own, then shared instead of exclusive, then the default output, saying why in the notices. A
/// driver can refuse a size it declared — an ASIO range with a step its sizes have to sit on,
/// which cpal's range cannot tell us about — and the device is not given up over it. The stream
/// does not play yet.
fn open_fitting(choice: &OutputChoice) -> Result<(Output, DeviceStream, rtrb::Producer<f32>, Arc<Progress>), String> {
    let mut tries = vec![choice.clone()];
    if choice.buffer_frames.is_some() {
        tries.push(OutputChoice { buffer_frames: None, ..choice.clone() });
    }
    if choice.exclusive {
        tries.push(OutputChoice { exclusive: false, ..choice.clone() });
    }
    // The default output keeps the cue's settings; it may have the cue's pair too.
    if choice.device_only() != OutputChoice::default() {
        tries.push(OutputChoice::default().with_cue_of(choice));
    }
    let mut notices = Vec::new();
    let mut last = String::from("no audio output");
    for (i, attempt) in tries.iter().enumerate() {
        let output = match Output::choose(attempt) {
            Ok(output) => output,
            Err(e) => {
                last = e;
                continue;
            }
        };
        let progress = Arc::new(Progress::default());
        let (producer, consumer) = ring(output.block);
        match output.open_device(consumer, &progress) {
            Ok(device) => {
                let mut output = output;
                notices.append(&mut output.notices);
                output.notices = notices;
                return Ok((output, device, producer, progress));
            }
            Err(e) => {
                let next = match tries.get(i + 1) {
                    Some(next) if next.buffer_frames != attempt.buffer_frames => "it keeps its own buffer size",
                    Some(next) if !next.exclusive && attempt.exclusive => "it plays shared",
                    Some(_) => "the default output plays",
                    None => "",
                };
                if !next.is_empty() {
                    notices.push(format!("{} could not be opened ({e}); {next}", output.device_name));
                }
                last = e;
            }
        }
    }
    Err(last)
}

/// What keeps a player's output open: closing it ends the stream on the owner's thread, which
/// hands back the renderer's plug-ins.
struct Owner {
    close: mpsc::Sender<()>,
    back: mpsc::Receiver<(u32, Option<Plugins>)>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Owner {
    /// Closes the stream and returns its sample rate and plug-ins, when they come back in
    /// time.
    fn close(mut self) -> Option<(u32, Plugins)> {
        let _closed = self.close.send(());
        let back = self.back.recv_timeout(CLOSE_TIMEOUT).ok();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        back.and_then(|(rate, plugins)| plugins.map(|p| (rate, p)))
    }
}

/// What a playing output runs with, for `audio.info`.
struct Live {
    progress: Arc<Progress>,
    host: String,
    device: String,
    sample_rate: u32,
    block: usize,
    exclusive: bool,
    fallback: bool,
    channels: u16,
    cue_pair: Option<u16>,
    /// The input device open beside it: its name, channels and the engine's count of blocks
    /// that found too little input.
    input: Option<(String, usize, Arc<AtomicU64>)>,
}

impl LiveOutput for Live {
    fn info(&self) -> OutputInfo {
        OutputInfo {
            host: self.host.clone(),
            device: self.device.clone(),
            sample_rate: self.sample_rate,
            buffer_frames: u32::try_from(self.progress.callback_frames()).unwrap_or(u32::MAX),
            engine_block: u32::try_from(self.block).unwrap_or(u32::MAX),
            exclusive: self.exclusive,
            latency_ms: self.progress.latency().as_secs_f64() * 1000.0,
            live: self.progress.live(),
            live_latency_ms: self.progress.live_latency(self.sample_rate).as_secs_f64() * 1000.0,
            underruns: self.progress.underruns(),
            failed: self.progress.failed(),
            fallback: self.fallback,
            channels: self.channels,
            cue_pair: self.cue_pair,
            input: self.input.as_ref().map(|(name, _, _)| name.clone()),
            input_channels: self
                .input
                .as_ref()
                .map_or(0, |(_, channels, _)| u16::try_from(*channels).unwrap_or(u16::MAX)),
            input_underruns: self.input.as_ref().map_or(0, |(_, _, underruns)| underruns.load(Ordering::Relaxed)),
        }
    }
}

/// Plays `project` on the default output with its transport stopped, from a thread that owns
/// the audio stream (a stream cannot move between threads on every platform). Returns a player
/// for the session and a description of the device; dropping the player closes the stream.
pub fn open_player(project: &Project, audio: &AudioPool) -> Result<(Player, String), String> {
    let opened = open_chosen(project, audio, &suhina_engine::DeviceAssets::new(), &OutputChoice::default(), None)?;
    let description = opened.player.output_info().map(|i| i.describe()).unwrap_or_default();
    Ok((opened.player, description))
}

/// Plays `project` with its transport stopped on the output `choice` names (see
/// [`AudioSystem::open`]), from a thread that owns the stream. `previous` is closed first; its
/// plug-ins carry over when the sample rate stays the same.
pub fn open_chosen(
    project: &Project,
    audio: &AudioPool,
    assets: &suhina_engine::DeviceAssets,
    choice: &OutputChoice,
    previous: Option<Player>,
) -> Result<Opened, String> {
    let carried = previous.and_then(|player| {
        let (controller, keep) = player.into_parts();
        let owner = keep.and_then(|k| k.downcast::<Owner>().ok());
        let back = owner.and_then(|o| o.close());
        drop(controller);
        back
    });
    let (project, audio, assets, choice) =
        (project.clone(), audio.clone(), std::sync::Arc::new(assets.clone()), choice.clone());
    let (ready, opened) = mpsc::channel();
    let (close, closed) = mpsc::channel::<()>();
    let (back_tx, back) = mpsc::channel();
    let thread = std::thread::Builder::new()
        .name("suhina-audio".into())
        .spawn(move || {
            let started = (|| {
                let (mut output, device, producer, progress) = open_fitting(&choice)?;
                let (plugins, kept) = match carried {
                    Some((rate, plugins)) if rate == output.sample_rate => (plugins, true),
                    _ => (Plugins::new(), false),
                };
                let mut renderer =
                    Renderer::stopped_with_assets(&project, &audio, &assets, output.sample_rate, plugins)
                        .map_err(|e| e.to_string())?;
                // The input device, when one is chosen: a failure leaves the output playing.
                let input = match input::open(&output.host, &choice.input, output.sample_rate, Arc::clone(&progress)) {
                    Ok(Some((stream, ring))) => {
                        renderer.set_audio_input(ring, stream.channels, Arc::clone(&stream.underruns));
                        Some(stream)
                    }
                    Ok(None) => None,
                    Err(e) => {
                        output.notices.push(format!("{e}; there is no audio input"));
                        None
                    }
                };
                let mut controller = renderer.controller();
                controller
                    .run_stretch_worker(&mut renderer)
                    .map_err(|e| format!("could not start the stretch worker: {e}"))?;
                let stream = output.play(device, renderer, producer, &progress)?;
                output.notices.extend(stream.priority_notice().map(str::to_owned));
                Ok::<_, String>((output, controller, progress, stream, kept, input))
            })();
            match started {
                Ok((output, controller, progress, stream, kept, input)) => {
                    let live = Live {
                        progress,
                        host: output.host.clone(),
                        device: output.device_name.clone(),
                        sample_rate: output.sample_rate,
                        block: output.block,
                        exclusive: output.exclusive,
                        fallback: in_place_of(&choice, &output.host, &output.device_name),
                        channels: output.channels,
                        cue_pair: output.cue_pair(),
                        input: input.as_ref().map(|i| (i.name.clone(), i.channels, Arc::clone(&i.underruns))),
                    };
                    let notice = (!output.notices.is_empty()).then(|| output.notices.join("; "));
                    if ready.send(Ok((controller, live, notice, kept))).is_ok() {
                        // Returns once the player, which holds `close`, closes or is dropped.
                        let _closed = closed.recv();
                    }
                    let plugins = stream.close().map(Renderer::into_plugins);
                    drop(input);
                    let _unheard = back_tx.send((output.sample_rate, plugins));
                }
                Err(e) => {
                    let _unheard = ready.send(Err(e));
                }
            }
        })
        .map_err(|e| format!("could not start the audio thread: {e}"))?;
    let (controller, live, notice, plugins_kept) =
        opened.recv().map_err(|_| "the audio thread stopped".to_string())??;
    let owner = Owner { close, back, thread: Some(thread) };
    let player = Player::with_output(controller, Some(Box::new(owner)), Arc::new(live));
    Ok(Opened { player, notice, plugins_kept })
}

/// Whether an output opened on `host`'s `device` plays in place of what `choice` names: the
/// chosen host or device was not there.
fn in_place_of(choice: &OutputChoice, host: &str, device: &str) -> bool {
    choice.host.as_deref().is_some_and(|h| !devices::same_name(h, host))
        || choice.device.as_deref().is_some_and(|d| !devices::same_name(d, device))
}

/// Whether the host and device `choice` names can be found now. Only names are read, no
/// formats, so it is quick; an ASIO host is not looked at while an ASIO stream is open (one
/// driver at a time). Runs on a thread of its own ([`DeviceOutput::chosen_present`]).
fn chosen_device_present(choice: &OutputChoice) -> bool {
    let id = match &choice.host {
        Some(name) => match cpal::available_hosts().into_iter().find(|h| devices::same_name(h.name(), name)) {
            Some(id) => id,
            None => return false,
        },
        None => cpal::default_host().id(),
    };
    let Some(device) = &choice.device else { return true };
    if id.name() == "ASIO" && ASIO_OPEN.load(Ordering::Acquire) {
        return false;
    }
    let Ok(host) = cpal::host_from_id(id) else { return false };
    host.output_devices().is_ok_and(|mut list| list.any(|d| devices::same_name(&d.to_string(), device)))
}

/// The background look for the chosen device: one at a time, its answer kept for the next ask.
#[derive(Default)]
struct Watch {
    looking: bool,
    answer: Option<(OutputChoice, bool)>,
}

/// The system's audio devices for the session ([`AudioSystem`]): the hosts cpal offers here
/// and opening a project on the device chosen.
#[derive(Default)]
pub struct DeviceOutput {
    /// The last listing, for the ASIO host while its driver is in use.
    listed: std::sync::Mutex<Vec<HostInfo>>,
    watch: Arc<std::sync::Mutex<Watch>>,
}

impl DeviceOutput {
    pub fn new() -> Self {
        Self::default()
    }
}

impl AudioSystem for DeviceOutput {
    fn hosts(&self) -> Vec<HostInfo> {
        let mut hosts = devices::list_hosts();
        let Ok(mut listed) = self.listed.lock() else { return hosts };
        if ASIO_OPEN.load(Ordering::Acquire)
            && let Some(before) = listed.iter().find(|h| h.name == "ASIO")
            && let Some(now) = hosts.iter_mut().find(|h| h.name == "ASIO")
            && now.devices.len() < before.devices.len()
        {
            *now = before.clone();
        }
        listed.clone_from(&hosts);
        hosts
    }

    fn open(
        &self,
        project: &Project,
        audio: &AudioPool,
        assets: &suhina_engine::DeviceAssets,
        choice: &OutputChoice,
        previous: Option<Player>,
    ) -> Result<Opened, String> {
        open_chosen(project, audio, assets, choice, previous)
    }

    /// Gives the answer of the last look for `choice`'s device, once, and starts the next look
    /// on a thread of its own, so the caller never waits for the system's device list.
    fn chosen_present(&self, choice: &OutputChoice) -> Option<bool> {
        let Ok(mut watch) = self.watch.lock() else { return None };
        let answer = watch.answer.take().filter(|(asked, _)| asked == choice).map(|(_, present)| present);
        if !watch.looking {
            let (shared, choice) = (Arc::clone(&self.watch), choice.clone());
            let look = move || {
                let present = chosen_device_present(&choice);
                if let Ok(mut watch) = shared.lock() {
                    watch.answer = Some((choice, present));
                    watch.looking = false;
                }
            };
            watch.looking = std::thread::Builder::new().name("suhina-device-watch".into()).spawn(look).is_ok();
        }
        answer
    }
}

/// The device's stream: its callback takes what the render thread put in the ring, and
/// silence (counted as an underrun) for what is not there yet, and measures the latency.
fn build<T>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    cue: Option<usize>,
    mut ring: Ring,
    progress: &Arc<Progress>,
) -> Result<cpal::Stream, String>
where
    T: SizedSample + FromSample<f32>,
{
    let channels = usize::from(config.channels);
    let sample_rate = config.sample_rate;
    let mut scratch = vec![0.0f32; MAX_DEVICE_FRAMES * RING_CHANNELS];
    let on_data = Arc::clone(progress);
    let on_error = Arc::clone(progress);

    device
        .build_output_stream(
            config,
            move |data: &mut [T], info: &cpal::OutputCallbackInfo| {
                let timestamp = info.timestamp();
                let device_delay = timestamp.playback.duration_since(timestamp.callback);
                let queued = ring.slots() / RING_CHANNELS;
                on_data.set_callback_frames(data.len() / channels.max(1));
                on_data.set_latency(measure_latency(queued, device_delay, sample_rate));
                for block in data.chunks_mut(MAX_DEVICE_FRAMES * channels) {
                    let frames = block.len() / channels;
                    let Some(ringed) = scratch.get_mut(..frames * RING_CHANNELS) else { return };
                    read_ring(&mut ring, ringed, &on_data);
                    write_frames(block, channels, ringed, cue);
                    on_data.add_frames(frames);
                }
            },
            move |err: cpal::Error| match err.kind() {
                // The driver was late (an ASIO overload): heard as a dropout, counted as one.
                cpal::ErrorKind::Xrun => {
                    on_error.underruns.fetch_add(1, Ordering::Relaxed);
                }
                // Rerouted to the new default device, or no real-time rights: it plays on.
                cpal::ErrorKind::DeviceChanged | cpal::ErrorKind::RealtimeDenied => {}
                // A reset the driver asked for, a device gone: the output is opened again.
                _ => on_error.fail(),
            },
            None,
        )
        .map_err(|e| format!("could not open audio output {device}: {e}"))
}

/// Writes ring frames (main left and right, cue left and right) into a device buffer with any
/// channel count, as [`channel_sample`] maps them: the main output on channels 1 and 2 (mono
/// gets the average), the cue on its pair from channel `cue`, silence on the rest.
fn write_frames<T: SizedSample + FromSample<f32>>(out: &mut [T], channels: usize, ringed: &[f32], cue: Option<usize>) {
    if channels == 0 {
        return;
    }
    for (frame, ring_frame) in out.chunks_mut(channels).zip(ringed.as_chunks::<RING_CHANNELS>().0) {
        for (channel, sample) in frame.iter_mut().enumerate() {
            *sample = T::from_sample(channel_sample(ring_frame, channels, channel, cue));
        }
    }
}

/// Says that `device`, with `channels` output channels, has no cue `pair`, so the cue plays
/// with the main output.
fn cue_missing(device: &str, pair: u16, channels: u16) -> String {
    let (first, second) = suhina_session::audio::cue_channels(pair);
    format!("{device} has {channels} output channels, not {first}/{second}; the cue plays with the main output")
}

#[cfg(test)]
mod tests;
