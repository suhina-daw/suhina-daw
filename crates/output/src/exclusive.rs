// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! WASAPI in exclusive mode (Windows): the device is Suhina's alone, with the buffer size
//! asked for and no system mixer in between. cpal opens WASAPI only shared, so this goes
//! through the `wasapi` crate. Everything COM happens on the stream's own thread, which sets
//! the device up, says whether that worked, then runs the event loop: wait for the device,
//! take a buffer from the ring, convert it, hand it over. No allocation, locks or I/O after
//! setup (AGENTS.md rule 3); a setup that fails or panics inside the driver's wrapper is an
//! error for the caller, who falls back to shared mode.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::time::Duration;

use wasapi::{Direction, SampleType, StreamMode, WaveFormat};

use crate::{Progress, RING_CHANNELS, Ring, channel_sample, cue_channel, measure_latency, read_device_ring};

/// How long the loop waits for the device before checking whether it should stop, in ms.
const WAIT_MS: u32 = 100;
/// How long opening may take before it counts as failed.
const OPEN_TIMEOUT: Duration = Duration::from_secs(10);

/// A sample layout the device may take, tried in this order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Encoding {
    F32,
    /// 24 bits of signal in a 32-bit container, or a full 32 bits: both are written as i32.
    I32 {
        valid: u16,
    },
    /// Packed 24-bit.
    I24,
    I16,
}

impl Encoding {
    const ALL: [Self; 5] = [Self::F32, Self::I32 { valid: 24 }, Self::I32 { valid: 32 }, Self::I24, Self::I16];

    fn bytes(self) -> usize {
        match self {
            Self::F32 | Self::I32 { .. } => 4,
            Self::I24 => 3,
            Self::I16 => 2,
        }
    }

    fn format(self, sample_rate: u32, channels: u16) -> WaveFormat {
        let (store, valid, kind) = match self {
            Self::F32 => (32, 32, SampleType::Float),
            Self::I32 { valid } => (32, usize::from(valid), SampleType::Int),
            Self::I24 => (24, 24, SampleType::Int),
            Self::I16 => (16, 16, SampleType::Int),
        };
        WaveFormat::new(store, valid, &kind, sample_rate as usize, usize::from(channels), None)
    }
}

/// Writes ring frames (main left and right, cue left and right) into `out` as the device's
/// frames: `channels` samples each in `encoding`, little-endian, mapped as
/// [`channel_sample`] maps them (the cue on its pair from channel `cue`). `out` holds whole
/// frames; frames past the end of `ringed` are left alone.
pub(crate) fn encode(encoding: Encoding, ringed: &[f32], channels: usize, cue: Option<usize>, out: &mut [u8]) {
    let size = encoding.bytes();
    let frame_bytes = size * channels;
    if frame_bytes == 0 {
        return;
    }
    for (frame, ring_frame) in out.chunks_exact_mut(frame_bytes).zip(ringed.as_chunks::<RING_CHANNELS>().0) {
        for (channel, sample) in frame.chunks_exact_mut(size).enumerate() {
            let value = channel_sample(ring_frame, channels, channel, cue);
            let value = if value.is_finite() { value.clamp(-1.0, 1.0) } else { 0.0 };
            match encoding {
                Encoding::F32 => sample.copy_from_slice(&value.to_le_bytes()),
                Encoding::I32 { .. } => {
                    let int = (f64::from(value) * f64::from(i32::MAX)) as i32;
                    sample.copy_from_slice(&int.to_le_bytes());
                }
                Encoding::I24 => {
                    let int = (f64::from(value) * 8_388_607.0) as i32;
                    if let Some(bytes) = int.to_le_bytes().get(..3) {
                        sample.copy_from_slice(bytes);
                    }
                }
                Encoding::I16 => {
                    let int = (value * f32::from(i16::MAX)) as i16;
                    sample.copy_from_slice(&int.to_le_bytes());
                }
            }
        }
    }
}

/// An exclusive-mode stream: its thread runs until the stream is dropped.
pub(crate) struct ExclusiveStream {
    stop: Arc<AtomicBool>,
    go: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
    /// Frames per device buffer.
    pub(crate) buffer_frames: u32,
    pub(crate) channels: u16,
}

impl ExclusiveStream {
    /// Starts taking sound from the ring; until then the device gets silence.
    pub(crate) fn play(&self) {
        self.go.store(true, Ordering::Release);
    }
}

impl Drop for ExclusiveStream {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// What the stream's thread found when it set the device up.
struct Ready {
    buffer_frames: u32,
    channels: u16,
}

/// Opens `device` (by its name; the default output when `None`) for Suhina alone at
/// `sample_rate`, with a buffer near `buffer_frames` (the device's default period when `None`),
/// reading from `ring`.
///
/// WASAPI tells us the device's minimum and default period, never the largest one its driver
/// takes, so a period past what the device does cannot be fitted here: the device refuses it in
/// `IAudioClient::Initialize`, [`reason`] says why in words, and the output plays shared instead
/// (`crate::open_fitting`). A buffer the device cannot align is the documented exception: it
/// says the size it wants and the client is set up again with that.
pub(crate) fn open(
    device: Option<String>,
    sample_rate: u32,
    buffer_frames: Option<u32>,
    cue_pair: Option<u16>,
    ring: Ring,
    progress: &Arc<Progress>,
) -> Result<ExclusiveStream, String> {
    let stop = Arc::new(AtomicBool::new(false));
    let go = Arc::new(AtomicBool::new(false));
    let (ready_tx, ready_rx) = mpsc::channel();
    let (stopping, going, progress) = (Arc::clone(&stop), Arc::clone(&go), Arc::clone(progress));
    let thread = std::thread::Builder::new()
        .name("suhina-wasapi".into())
        .spawn(move || {
            let format = Format { sample_rate, buffer_frames, cue_pair };
            run(device, format, ring, &progress, &stopping, &going, &ready_tx);
        })
        .map_err(|e| format!("could not start the exclusive-mode thread: {e}"))?;
    let mut stream = ExclusiveStream { stop, go, thread: Some(thread), buffer_frames: 0, channels: 0 };
    // A panic inside the driver's wrapper ends the thread: the sender goes, and so does this
    // wait.
    match ready_rx.recv_timeout(OPEN_TIMEOUT) {
        Ok(Ok(ready)) => {
            stream.buffer_frames = ready.buffer_frames;
            stream.channels = ready.channels;
            Ok(stream)
        }
        Ok(Err(e)) => Err(e),
        Err(_) => Err("the device did not open in exclusive mode".into()),
    }
}

/// The device names that can be opened in exclusive mode at their own sample rate.
pub(crate) fn capable_devices() -> Vec<String> {
    let probe = std::thread::Builder::new().name("suhina-wasapi-probe".into()).spawn(|| {
        if wasapi::initialize_mta().is_err() {
            return Vec::new();
        }
        let Ok(collection) =
            wasapi::DeviceEnumerator::new().and_then(|devices| devices.get_device_collection(&Direction::Render))
        else {
            return Vec::new();
        };
        let count = collection.get_nbr_devices().unwrap_or(0);
        (0..count)
            .filter_map(|i| {
                let device = collection.get_device_at_index(i).ok()?;
                let name = device.get_friendlyname().ok()?;
                let client = device.get_iaudioclient().ok()?;
                let mix = client.get_mixformat().ok()?;
                let (rate, channels) = (mix.get_samplespersec(), mix.get_nchannels());
                Encoding::ALL
                    .iter()
                    .any(|e| client.is_supported_exclusive_with_quirks(&e.format(rate, channels)).is_ok())
                    .then_some(name)
            })
            .collect()
    });
    probe.ok().and_then(|t| t.join().ok()).unwrap_or_default()
}

/// What an exclusive stream is opened with.
struct Format {
    sample_rate: u32,
    buffer_frames: Option<u32>,
    /// The chosen cue pair, written when the device has it.
    cue_pair: Option<u16>,
}

fn run(
    device: Option<String>,
    format: Format,
    mut ring: Ring,
    progress: &Progress,
    stop: &AtomicBool,
    go: &AtomicBool,
    ready: &mpsc::Sender<Result<Ready, String>>,
) {
    let Format { sample_rate, buffer_frames, cue_pair } = format;
    let set_up = match set_up(device.as_deref(), sample_rate, buffer_frames) {
        Ok(set_up) => set_up,
        Err(e) => {
            let _unheard = ready.send(Err(e));
            return;
        }
    };
    let Device { client, render, event, encoding, channels, frames } = set_up;
    let channels_n = usize::from(channels);
    let frame_bytes = encoding.bytes() * channels_n;
    let mut bytes = vec![0u8; frames * frame_bytes];
    let cue = cue_channel(cue_pair, channels);
    let mut ringed = vec![0.0f32; frames * RING_CHANNELS];
    // A buffer of silence before starting, so the first period has something to play.
    if render.write_to_device(frames, &bytes, None).is_err() || client.start_stream().is_err() {
        let _unheard = ready.send(Err("the device would not start in exclusive mode".into()));
        return;
    }
    let buffer = u32::try_from(frames).unwrap_or(u32::MAX);
    if ready.send(Ok(Ready { buffer_frames: buffer, channels })).is_err() {
        let _ = client.stop_stream();
        return;
    }
    let _realtime = suhina_realtime::RealTime::join(frames, sample_rate);
    let period = Duration::from_secs_f64(frames as f64 / f64::from(sample_rate.max(1)));
    let mut envelope = crate::transition::Envelope::new(sample_rate);
    while !stop.load(Ordering::Acquire) {
        if event.wait_for_event(WAIT_MS).is_err() {
            continue;
        }
        let Ok(space) = client.get_available_space_in_frames() else {
            progress.fail();
            break;
        };
        let space = (space as usize).min(frames);
        if space == 0 {
            continue;
        }
        let Some(out) = ringed.get_mut(..space * RING_CHANNELS) else { continue };
        if go.load(Ordering::Acquire) {
            let queued = ring.slots() / RING_CHANNELS;
            read_device_ring(&mut ring, out, progress, &mut envelope, period);
            progress.set_latency(measure_latency(queued, period, sample_rate));
        } else {
            out.fill(0.0);
        }
        progress.set_callback_frames(space);
        let Some(data) = bytes.get_mut(..space * frame_bytes) else { continue };
        encode(encoding, out, channels_n, cue, data);
        if render.write_to_device(space, data, None).is_err() {
            progress.fail();
            break;
        }
        progress.add_frames(space);
    }
    let _ = client.stop_stream();
}

struct Device {
    client: wasapi::AudioClient,
    render: wasapi::AudioRenderClient,
    event: wasapi::Handle,
    encoding: Encoding,
    channels: u16,
    frames: usize,
}

// IAudioClient::Initialize results that say why exclusive mode could not be had.
const AUDCLNT_E_BUFFER_SIZE_NOT_ALIGNED: u32 = 0x8889_0019;
const AUDCLNT_E_DEVICE_IN_USE: u32 = 0x8889_000A;
const AUDCLNT_E_EXCLUSIVE_MODE_NOT_ALLOWED: u32 = 0x8889_000E;
const AUDCLNT_E_UNSUPPORTED_FORMAT: u32 = 0x8889_0008;
const AUDCLNT_E_ENDPOINT_CREATE_FAILED: u32 = 0x8889_000F;

/// The Windows error code behind a `wasapi` error, if it is one.
fn hresult(e: &wasapi::WasapiError) -> Option<u32> {
    match e {
        wasapi::WasapiError::Windows(windows) => Some(windows.code().0 as u32),
        _ => None,
    }
}

/// Why opening the device exclusively failed, in words.
fn init_error(e: &wasapi::WasapiError) -> String {
    reason(hresult(e)).map_or_else(|| format!("could not open the device exclusively: {e}"), String::from)
}

fn reason(code: Option<u32>) -> Option<&'static str> {
    match code? {
        AUDCLNT_E_DEVICE_IN_USE => Some("another program has the device in exclusive mode"),
        AUDCLNT_E_EXCLUSIVE_MODE_NOT_ALLOWED => {
            Some("the device does not allow exclusive mode (Sound settings › device › Advanced)")
        }
        AUDCLNT_E_UNSUPPORTED_FORMAT => Some("the device does not take this format in exclusive mode"),
        AUDCLNT_E_ENDPOINT_CREATE_FAILED => Some("the device is not there or not ready"),
        AUDCLNT_E_BUFFER_SIZE_NOT_ALIGNED => Some("the device cannot use this buffer size"),
        _ => None,
    }
}

fn set_up(name: Option<&str>, sample_rate: u32, buffer_frames: Option<u32>) -> Result<Device, String> {
    let error = |what: &str, e: wasapi::WasapiError| format!("{what}: {e}");
    wasapi::initialize_mta().ok().map_err(|e| format!("could not start COM: {e}"))?;
    let devices = wasapi::DeviceEnumerator::new().map_err(|e| error("no output devices", e))?;
    let device = match name {
        Some(name) => {
            let collection =
                devices.get_device_collection(&Direction::Render).map_err(|e| error("no output devices", e))?;
            let count = collection.get_nbr_devices().map_err(|e| error("no output devices", e))?;
            (0..count)
                .filter_map(|i| collection.get_device_at_index(i).ok())
                .find(|d| d.get_friendlyname().is_ok_and(|n: String| crate::devices::same_name(&n, name)))
                .ok_or_else(|| format!("no output device {name:?}"))?
        }
        None => devices.get_default_device(&Direction::Render).map_err(|e| error("no default output", e))?,
    };
    let mut client = device.get_iaudioclient().map_err(|e| error("could not open the device", e))?;
    let channels = client.get_mixformat().map(|m| m.get_nchannels()).unwrap_or(2).clamp(1, 64);
    let (encoding, format) = Encoding::ALL
        .iter()
        .find_map(|e| client.is_supported_exclusive_with_quirks(&e.format(sample_rate, channels)).ok().map(|f| (*e, f)))
        .ok_or_else(|| format!("the device takes no format Suhina writes at {sample_rate} Hz in exclusive mode"))?;
    let (default_period, _) = client.get_device_period().map_err(|e| error("no device period", e))?;
    let wanted = match buffer_frames {
        Some(frames) => wasapi::calculate_period_100ns(i64::from(frames), i64::from(sample_rate)),
        None => default_period,
    };
    // Intel HDA devices need buffers in multiples of 128 bytes.
    let period = client.calculate_aligned_period_near(wanted, Some(128), &format).unwrap_or(wanted);
    let mode = StreamMode::EventsExclusive { period_hns: period };
    if let Err(e) = client.initialize_client(&format, &Direction::Render, &mode) {
        if hresult(&e) != Some(AUDCLNT_E_BUFFER_SIZE_NOT_ALIGNED) {
            return Err(init_error(&e));
        }
        // A buffer the device cannot align to: it says the size it can, and a new client is
        // set up with that (the documented way back).
        let frames = client.get_buffer_size().map_err(|e| error("could not open the device exclusively", e))?;
        let period = wasapi::calculate_period_100ns(i64::from(frames), i64::from(sample_rate));
        client = device.get_iaudioclient().map_err(|e| error("could not open the device", e))?;
        client
            .initialize_client(&format, &Direction::Render, &StreamMode::EventsExclusive { period_hns: period })
            .map_err(|e| init_error(&e))?;
    }
    let event = client.set_get_eventhandle().map_err(|e| error("no device event", e))?;
    let render = client.get_audiorenderclient().map_err(|e| error("no render client", e))?;
    let frames = client.get_buffer_size().map_err(|e| error("no buffer size", e))? as usize;
    if frames == 0 || frames > 64 * 1024 {
        return Err(format!("the device reports a buffer of {frames} frames"));
    }
    Ok(Device { client, render, event, encoding, channels, frames })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Stereo frames as ring frames with a silent cue.
    fn quad(stereo: &[f32]) -> Vec<f32> {
        stereo.as_chunks::<2>().0.iter().flat_map(|[l, r]| [*l, *r, 0.0, 0.0]).collect()
    }

    #[test]
    fn the_cue_encodes_onto_its_pair() {
        // Six channels, the cue on 5/6: main on 1/2, silence on 3/4.
        let ringed = [0.5f32, -0.5, 0.25, -0.25];
        let mut six = [9u8; 24];
        encode(Encoding::F32, &ringed, 6, Some(4), &mut six);
        let at = |i: usize| f32::from_le_bytes([six[i * 4], six[i * 4 + 1], six[i * 4 + 2], six[i * 4 + 3]]);
        assert_eq!([at(0), at(1), at(2), at(3), at(4), at(5)], [0.5, -0.5, 0.0, 0.0, 0.25, -0.25]);
    }

    #[test]
    fn stereo_encodes_into_each_layout() {
        let stereo = [1.0f32, -1.0, 0.5, 0.0];

        let mut f32s = [0u8; 16];
        encode(Encoding::F32, &quad(&stereo), 2, None, &mut f32s);
        assert_eq!(f32::from_le_bytes([f32s[0], f32s[1], f32s[2], f32s[3]]), 1.0);
        assert_eq!(f32::from_le_bytes([f32s[8], f32s[9], f32s[10], f32s[11]]), 0.5);

        let mut i16s = [0u8; 8];
        encode(Encoding::I16, &quad(&stereo), 2, None, &mut i16s);
        assert_eq!(i16::from_le_bytes([i16s[0], i16s[1]]), i16::MAX);
        assert_eq!(i16::from_le_bytes([i16s[2], i16s[3]]), -i16::MAX);

        let mut i24s = [0u8; 12];
        encode(Encoding::I24, &quad(&stereo), 2, None, &mut i24s);
        assert_eq!(&i24s[..3], &[0xff, 0xff, 0x7f]);
        assert_eq!(&i24s[3..6], &[0x01, 0x00, 0x80]);

        let mut i32s = [0u8; 16];
        encode(Encoding::I32 { valid: 24 }, &quad(&stereo), 2, None, &mut i32s);
        assert_eq!(i32::from_le_bytes([i32s[0], i32s[1], i32s[2], i32s[3]]), i32::MAX);
    }

    #[test]
    fn encoding_maps_channels_and_refuses_nonsense() {
        // Mono averages; four channels get silence past the first two.
        let mut mono = [0u8; 8];
        encode(Encoding::F32, &quad(&[1.0, 0.0, 0.5, 0.5]), 1, None, &mut mono);
        assert_eq!(f32::from_le_bytes([mono[0], mono[1], mono[2], mono[3]]), 0.5);
        let mut four = [9u8; 16];
        encode(Encoding::I16, &quad(&[0.5, 0.5, 0.5, 0.5]), 4, None, &mut four);
        assert_eq!(&four[4..8], &[0, 0, 0, 0]);
        assert_eq!(&four[12..16], &[0, 0, 0, 0]);
        // Out of range and not a number: clamped and silenced, never wrapped.
        let mut loud = [0u8; 8];
        encode(Encoding::I16, &quad(&[4.0, f32::NAN, -4.0, f32::INFINITY]), 2, None, &mut loud);
        assert_eq!(i16::from_le_bytes([loud[0], loud[1]]), i16::MAX);
        assert_eq!(i16::from_le_bytes([loud[2], loud[3]]), 0);
        assert_eq!(i16::from_le_bytes([loud[4], loud[5]]), -i16::MAX);
        // No channels: nothing written.
        let mut none = [7u8; 4];
        encode(Encoding::F32, &quad(&[1.0, 1.0]), 0, None, &mut none);
        assert_eq!(none, [7; 4]);
    }

    #[test]
    fn driver_errors_are_said_in_words() {
        assert!(reason(Some(AUDCLNT_E_DEVICE_IN_USE)).unwrap().contains("another program"));
        assert!(reason(Some(AUDCLNT_E_EXCLUSIVE_MODE_NOT_ALLOWED)).unwrap().contains("does not allow"));
        assert_eq!(reason(Some(0x8000_4005)), None);
        assert_eq!(reason(None), None);
        let other = wasapi::WasapiError::ClientNotInit;
        assert_eq!(hresult(&other), None);
        assert!(init_error(&other).starts_with("could not open the device exclusively"));
    }

    /// Probing works on any Windows machine, with or without exclusive-capable devices.
    #[test]
    fn probing_for_exclusive_devices_never_fails() {
        let names = capable_devices();
        assert!(names.iter().all(|n| !n.is_empty()));
    }
}
