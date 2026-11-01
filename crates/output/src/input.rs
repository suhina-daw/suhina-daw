// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The audio input device ([ADR 0017](../../../docs/decisions/0017-inputs-and-latency.md)):
//! opened beside the output on the same host, at the output's sample rate, when one is chosen
//! (off by default, so nothing opens a microphone unasked). Its callback converts every channel
//! (up to [`MAX_INPUT_CHANNELS`]) to `f32` and writes the frames into a lock-free ring the
//! engine's render thread reads; frames that find the ring full are dropped and counted. No
//! allocation, locks or I/O in the callback.

use std::sync::Arc;
use std::sync::atomic::AtomicU64;

use cpal::traits::{DeviceTrait, StreamTrait};
use cpal::{FromSample, Sample, SizedSample};
use suhina_engine::MAX_INPUT_CHANNELS;
use suhina_session::InputChoice;

use crate::{MAX_DEVICE_FRAMES, Progress, devices};

/// Device buffers' worth of input the ring holds.
const RING_BUFFERS: usize = 8;

/// An open input device: dropping it closes the stream.
pub(crate) struct InputStream {
    _stream: cpal::Stream,
    pub(crate) name: String,
    /// Channels written per frame (the device's, up to [`MAX_INPUT_CHANNELS`]).
    pub(crate) channels: usize,
    /// Blocks the engine found too little input for (the engine counts them).
    pub(crate) underruns: Arc<AtomicU64>,
}

/// Opens the input `choice` names on the host `host_name` at `sample_rate`, playing into a ring
/// whose reading end is returned for the engine. `Ok(None)` when the choice is off.
/// Its callback measures the device's own latency into `progress`.
pub(crate) fn open(
    host_name: &str,
    choice: &InputChoice,
    sample_rate: u32,
    progress: Arc<Progress>,
) -> Result<Option<(InputStream, rtrb::Consumer<f32>)>, String> {
    if *choice == InputChoice::Off {
        return Ok(None);
    }
    let host_id = cpal::available_hosts()
        .into_iter()
        .find(|h| devices::same_name(h.name(), host_name))
        .ok_or_else(|| format!("the audio host {host_name} is gone"))?;
    let host = cpal::host_from_id(host_id).map_err(|e| e.to_string())?;
    let device = match choice {
        InputChoice::Named(name) => devices::input_devices(&host)?
            .into_iter()
            .find(|d| devices::same_name(&d.to_string(), name))
            .ok_or_else(|| format!("the audio input {name} is not there"))?,
        _ => devices::default_input(&host).ok_or("there is no audio input device")?,
    };
    let name = device.to_string();
    // The device's own format at the output's rate: the engine takes no resampled input.
    let supported = device
        .supported_input_configs()
        .map_err(|e| format!("audio input {name}: {e}"))?
        .filter(|c| (c.min_sample_rate()..=c.max_sample_rate()).contains(&sample_rate))
        .max_by_key(|c| (c.channels(), c.sample_format() == cpal::SampleFormat::F32))
        .and_then(|c| c.try_with_sample_rate(sample_rate))
        .ok_or_else(|| format!("audio input {name} does not record at {sample_rate} Hz, the output's rate"))?;
    let format = supported.sample_format();
    let config: cpal::StreamConfig = supported.into();
    let device_channels = usize::from(config.channels);
    if device_channels == 0 {
        return Err(format!("audio input {name} reports no channels"));
    }
    let channels = device_channels.min(MAX_INPUT_CHANNELS);
    let (producer, consumer) = rtrb::RingBuffer::new(MAX_DEVICE_FRAMES * RING_BUFFERS * channels);
    let underruns = Arc::new(AtomicU64::new(0));
    let stream = match format {
        cpal::SampleFormat::F32 => build::<f32>(&device, &config, producer, progress),
        cpal::SampleFormat::I16 => build::<i16>(&device, &config, producer, progress),
        cpal::SampleFormat::I32 => build::<i32>(&device, &config, producer, progress),
        cpal::SampleFormat::U16 => build::<u16>(&device, &config, producer, progress),
        other => return Err(format!("audio input {name} uses sample format {other}, which is not supported yet")),
    }?;
    stream.play().map_err(|e| format!("could not start audio input {name}: {e}"))?;
    Ok(Some((InputStream { _stream: stream, name, channels, underruns }, consumer)))
}

fn build<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    mut ring: rtrb::Producer<f32>,
    progress: Arc<Progress>,
) -> Result<cpal::Stream, String>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let device_channels = usize::from(config.channels).max(1);
    let sample_rate = config.sample_rate;
    device
        .build_input_stream(
            *config,
            move |data: &[T], info: &cpal::InputCallbackInfo| {
                let stamp = info.timestamp();
                let since_capture = stamp.callback.duration_since(stamp.capture);
                progress.set_input_latency(input_latency(since_capture, data.len() / device_channels, sample_rate));
                write_frames(data, device_channels, &mut ring);
            },
            // A device unplugged ends its input; the output plays on, and the next reopen finds it
            // gone.
            |_: cpal::Error| {},
            None,
        )
        .map_err(|e| format!("could not open audio input {device}: {e}"))
}

/// The input device's own latency in frames: `since_capture` is the time from its buffer's
/// first frame being captured to the callback, and the buffer's `frames` came in over that time,
/// so the last frame waited for what is left of it.
pub(crate) fn input_latency(since_capture: std::time::Duration, frames: usize, sample_rate: u32) -> usize {
    let waited = since_capture.as_secs_f64() * f64::from(sample_rate);
    (waited - frames as f64).max(0.0).round() as usize
}

/// Writes whole frames of `data` (`device_channels` interleaved) into `ring` as `f32`, the first
/// [`MAX_INPUT_CHANNELS`] channels of each, dropping the frames that do not fit. Never allocates.
pub(crate) fn write_frames<T>(data: &[T], device_channels: usize, ring: &mut rtrb::Producer<f32>)
where
    T: Sample,
    f32: FromSample<T>,
{
    let device_channels = device_channels.max(1);
    let kept = device_channels.min(MAX_INPUT_CHANNELS);
    let frames = (data.len() / device_channels).min(ring.slots() / kept);
    let Ok(chunk) = ring.write_chunk_uninit(frames * kept) else { return };
    let samples = data
        .chunks_exact(device_channels)
        .take(frames)
        .flat_map(|frame| frame.iter().take(kept).map(|s| f32::from_sample(*s)));
    chunk.fill_from_iter(samples);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_are_written_whole_with_at_most_the_channels_the_engine_reads() {
        let (mut producer, mut consumer) = rtrb::RingBuffer::<f32>::new(64);
        write_frames(&[0i16, i16::MAX, i16::MIN, 0, 7], 2, &mut producer);
        // Two whole frames; the odd sample is not a frame.
        assert_eq!(consumer.slots(), 4);
        let read: Vec<f32> = (0..4).filter_map(|_| consumer.pop().ok()).collect();
        assert_eq!(read[0], 0.0);
        assert!((read[1] - 1.0).abs() < 1e-3 && (read[2] + 1.0).abs() < 1e-3);

        // A device with more channels than the engine reads gives the first ones only.
        let (mut producer, consumer) = rtrb::RingBuffer::<f32>::new(1024);
        let wide: Vec<f32> = (0..MAX_INPUT_CHANNELS + 6).map(|c| c as f32).collect();
        write_frames(&wide, wide.len(), &mut producer);
        assert_eq!(consumer.slots(), MAX_INPUT_CHANNELS);
    }

    #[test]
    fn a_full_ring_drops_what_does_not_fit_in_whole_frames() {
        let (mut producer, consumer) = rtrb::RingBuffer::<f32>::new(5);
        write_frames(&[0.1f32, 0.2, 0.3, 0.4, 0.5, 0.6], 2, &mut producer);
        assert_eq!(consumer.slots(), 4, "two frames of two fit in five slots");
        write_frames(&[0.1f32; 4], 0, &mut producer);
    }

    /// Opening works on any machine: off opens nothing, and a device that is not there (or no
    /// audio at all, in CI) is an error, not a crash.
    #[test]
    fn an_input_off_or_missing_opens_nothing() {
        let progress = || Arc::new(Progress::default());
        assert!(open("anything", &InputChoice::Off, 48_000, progress()).is_ok_and(|o| o.is_none()));
        let missing =
            open(cpal::default_host().id().name(), &InputChoice::Named("No Such Input".into()), 48_000, progress());
        assert!(missing.is_err());
        assert!(open("No Such Host", &InputChoice::Default, 48_000, progress()).is_err());
    }

    #[test]
    fn the_input_latency_is_how_long_the_last_frame_waited() {
        let ms = std::time::Duration::from_millis;
        // A 480-frame buffer called back 15 ms after its first frame: its last frame waited 5 ms.
        assert_eq!(input_latency(ms(15), 480, 48_000), 240);
        assert_eq!(input_latency(ms(5), 480, 48_000), 0, "a stamp that is too early says nothing");
        assert_eq!(input_latency(ms(0), 0, 0), 0);
    }
}
