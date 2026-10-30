// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The system's audio hosts and output devices, and fitting a choice to what a device offers.
//! Drivers report what they like: every number from them is checked, and a host or device that
//! fails to answer is listed with the error or left out, never a panic (AGENTS.md rule 2).

use cpal::traits::{DeviceTrait, HostTrait};
use suhina_session::audio::{DeviceInfo, HostInfo, InputInfo, MAX_BUFFER_FRAMES, MAX_SAMPLE_RATE, MIN_BUFFER_FRAMES};

/// The buffer sizes offered when the driver gives no range: powers of two the engine renders
/// well in.
const BUFFER_SIZES: [u32; 7] = [64, 128, 256, 512, 1024, 2048, 4096];

/// Sample rates looked for within a device's supported ranges.
const SAMPLE_RATES: [u32; 13] =
    [8_000, 11_025, 16_000, 22_050, 32_000, 44_100, 48_000, 88_200, 96_000, 176_400, 192_000, 352_800, 384_000];

/// CPAL 0.18's ASIO descriptors own metadata, not loaded drivers. Keep them before
/// opening a stream: enumerating another driver while one plays ends the iterator.
#[cfg(all(windows, feature = "asio"))]
static ASIO_DEVICES: std::sync::Mutex<Option<Vec<cpal::Device>>> = std::sync::Mutex::new(None);

pub(crate) fn host_devices(host: &cpal::Host) -> Result<Vec<cpal::Device>, String> {
    #[cfg(all(windows, feature = "asio"))]
    if host.id().name() == "ASIO" {
        let mut cached = ASIO_DEVICES.lock().map_err(|e| e.to_string())?;
        return cached_listing(&mut cached, || host.devices().map(|d| d.collect()).map_err(|e| e.to_string()));
    }
    host.devices().map(|devices| devices.collect()).map_err(|e| e.to_string())
}

#[cfg(any(test, all(windows, feature = "asio")))]
fn cached_listing<T: Clone>(
    cached: &mut Option<Vec<T>>,
    load: impl FnOnce() -> Result<Vec<T>, String>,
) -> Result<Vec<T>, String> {
    if let Some(devices) = cached.as_ref() {
        return Ok(devices.clone());
    }
    let devices = load()?;
    *cached = Some(devices.clone());
    Ok(devices)
}

pub(crate) fn output_devices(host: &cpal::Host) -> Result<Vec<cpal::Device>, String> {
    Ok(host_devices(host)?
        .into_iter()
        .filter(|d| d.supported_output_configs().is_ok_and(|mut c| c.next().is_some()))
        .collect())
}

pub(crate) fn input_devices(host: &cpal::Host) -> Result<Vec<cpal::Device>, String> {
    Ok(host_devices(host)?
        .into_iter()
        .filter(|d| d.supported_input_configs().is_ok_and(|mut c| c.next().is_some()))
        .collect())
}

pub(crate) fn default_output(host: &cpal::Host) -> Option<cpal::Device> {
    if host.id().name() == "ASIO" {
        output_devices(host).ok()?.into_iter().next()
    } else {
        host.default_output_device()
    }
}

pub(crate) fn default_input(host: &cpal::Host) -> Option<cpal::Device> {
    if host.id().name() == "ASIO" { input_devices(host).ok()?.into_iter().next() } else { host.default_input_device() }
}

/// The buffer sizes a device with the driver range `range` can be asked for: the powers of two
/// inside it, and its ends, within [`MIN_BUFFER_FRAMES`] … [`MAX_BUFFER_FRAMES`]. A range that
/// makes no sense (empty, reversed) counts as none, and the device is then asked for no size at
/// all. So does a range the engine has no size in — a driver whose smallest buffer is past what
/// the engine renders in — and it holds nothing on purpose: a driver is never asked for a size
/// outside the range it declared ([`fit_for_host`] then leaves the size to the driver).
pub fn allowed_buffers(range: Option<(u32, u32)>) -> Vec<u32> {
    let Some((min, max)) = range.filter(|(min, max)| *min > 0 && min <= max) else {
        return BUFFER_SIZES.to_vec();
    };
    let inside = |n: u32| (min..=max).contains(&n) && (MIN_BUFFER_FRAMES..=MAX_BUFFER_FRAMES).contains(&n);
    let mut sizes: Vec<u32> = BUFFER_SIZES.iter().copied().chain([min, max]).filter(|n| inside(*n)).collect();
    sizes.sort_unstable();
    sizes.dedup();
    sizes
}

/// Whether the driver's declared `range` holds `frames`: only such a size may be asked for.
pub fn within(range: Option<(u32, u32)>, frames: u32) -> bool {
    range.is_some_and(|(min, max)| min > 0 && min <= max && (min..=max).contains(&frames))
}

/// The allowed size nearest to `wanted` (the larger of two as near).
pub fn fit_buffer(wanted: u32, allowed: &[u32]) -> u32 {
    allowed
        .iter()
        .copied()
        .min_by_key(|n| (n.abs_diff(wanted), std::cmp::Reverse(*n)))
        .unwrap_or_else(|| wanted.clamp(MIN_BUFFER_FRAMES, MAX_BUFFER_FRAMES))
}

/// The sample rates inside the supported `ranges` (min, max): the common ones, and a range's
/// only rate when it has one. Rates of 0 or past [`MAX_SAMPLE_RATE`] are left out.
pub fn sample_rates(ranges: &[(u32, u32)]) -> Vec<u32> {
    let usable = |r: u32| r > 0 && r <= MAX_SAMPLE_RATE;
    let mut rates: Vec<u32> = SAMPLE_RATES
        .iter()
        .copied()
        .filter(|rate| ranges.iter().any(|(min, max)| (*min..=*max).contains(rate)))
        .chain(ranges.iter().filter(|(min, max)| min == max && usable(*min)).map(|(min, _)| *min))
        .collect();
    rates.sort_unstable();
    rates.dedup();
    rates
}

/// The largest buffer offered on WASAPI (N48): Windows plays a shared stream in its own period
/// (about 10 ms), so a larger engine block only adds latency, and exclusive mode asks the device
/// for the period itself, which interfaces such as Apollo refuse past 2048 frames.
pub const WASAPI_MOST_FRAMES: u32 = 2048;

/// The period exclusive WASAPI asks the device for, from the buffer size chosen: within what the
/// engine renders in and [`WASAPI_MOST_FRAMES`], so a size saved before the cap comes down to it.
/// The engine renders in the same number of frames, so the block and the period stay equal.
pub fn exclusive_buffer(chosen: Option<u32>) -> Option<u32> {
    chosen.map(|frames| frames.clamp(MIN_BUFFER_FRAMES, MAX_BUFFER_FRAMES.min(WASAPI_MOST_FRAMES)))
}

/// The buffer sizes offered on `host`: on WASAPI the engine's block, up to
/// [`WASAPI_MOST_FRAMES`] (Windows decides the device's own buffer); elsewhere the sizes the
/// driver allows.
pub fn offered_buffers(host: &str, range: Option<(u32, u32)>) -> Vec<u32> {
    if host == "WASAPI" {
        allowed_buffers(None).into_iter().filter(|n| *n <= WASAPI_MOST_FRAMES).collect()
    } else {
        allowed_buffers(range)
    }
}

/// Fits a wanted buffer size to `host` and the driver's `range`: the frames the engine renders
/// per block, and the size to ask the device for (none where the driver decides, or where the
/// range it declared holds no size the engine can render in).
pub fn fit_for_host(host: &str, wanted: u32, range: Option<(u32, u32)>) -> (u32, Option<u32>) {
    if host == "WASAPI" {
        // Windows plays a shared-mode stream in its own period and takes any positive duration
        // asked for, so the number is the engine's block alone and the device is asked for none
        // of it (cpal's WASAPI stream; `exclusive` mode, which does ask the device, is fitted
        // separately). See `docs/decisions/0006-plug-in-hosting.md` › the block. A size saved
        // before the cap (N48) comes down to it.
        return (wanted.clamp(MIN_BUFFER_FRAMES, MAX_BUFFER_FRAMES.min(WASAPI_MOST_FRAMES)), None);
    }
    let fitted = fit_buffer(wanted, &allowed_buffers(range));
    // Only a size the driver declared is ever asked for: a fit outside its range (a range with
    // no size the engine renders in) leaves the size with the driver, which keeps its own —
    // `cpal::BufferSize::Default`.
    let device = within(range, fitted).then_some(fitted);
    (fitted, device)
}

/// What to tell the user when the size they asked for is not the one that plays: the range the
/// driver declared and the size it fitted to, or that its sizes are outside what the engine
/// renders in and it keeps its own. `None` when they got what they asked for, when the driver
/// sets no size of its own (`host` WASAPI, where Windows owns the device's buffer), or when it
/// declared no range and the block became what was asked for anyway.
pub fn buffer_notice(
    host: &str,
    device: &str,
    wanted: u32,
    block: u32,
    asked: Option<u32>,
    range: Option<(u32, u32)>,
) -> Option<String> {
    if host == "WASAPI" {
        return None;
    }
    // A range that makes no sense counts as none, as it does everywhere else.
    let declared = range.filter(|(min, max)| *min > 0 && min <= max);
    match (declared, asked) {
        // Nothing of the driver's fits what the engine renders in: it keeps its own size.
        (Some((min, max)), None) => Some(format!(
            "{device} takes {min} to {max} frames, which Suhina cannot render in; it keeps its own buffer size"
        )),
        (Some((min, max)), Some(_)) if block != wanted => {
            Some(format!("{device} takes {min} to {max} frames; {block} frames play"))
        }
        // A driver that declared no range keeps its own size too, so the block is the engine's.
        (None, _) if block != wanted => {
            Some(format!("{device} sets its own buffer size; the engine renders in {block}-frame blocks"))
        }
        _ => None,
    }
}

/// Whether two names are the same, ignoring case and the space around them.
pub fn same_name(a: &str, b: &str) -> bool {
    a.trim().eq_ignore_ascii_case(b.trim())
}

/// The driver's buffer range, when it reports a usable one.
pub fn buffer_range(size: &cpal::SupportedBufferSize) -> Option<(u32, u32)> {
    match *size {
        cpal::SupportedBufferSize::Range { min, max } if min > 0 && min <= max => Some((min, max)),
        _ => None,
    }
}

/// The buffer range to report for a device: its default config's own first, then a supported
/// config's. WASAPI reports the device period in frames *at each config's sample rate*, lowest
/// rate first, so the default config is the one whose numbers belong to the device's own rate —
/// the same period is 80 frames at 8 kHz and 480 at 48 kHz. A device whose default config says
/// nothing keeps a supported config's range.
fn declared_range(default: Option<(u32, u32)>, supported: impl IntoIterator<Item = (u32, u32)>) -> Option<(u32, u32)> {
    default.or_else(|| supported.into_iter().next())
}

/// Lists every host cpal offers here and each one's output devices.
pub fn list_hosts() -> Vec<HostInfo> {
    let default_host = cpal::default_host().id();
    #[cfg(windows)]
    let exclusive = crate::exclusive::capable_devices();
    cpal::available_hosts()
        .into_iter()
        .map(|id| {
            let mut info = HostInfo { name: id.name().to_string(), default: id == default_host, ..HostInfo::default() };
            match cpal::host_from_id(id) {
                Ok(host) => {
                    match list_devices(&host, id.name()) {
                        Ok(devices) => info.devices = devices,
                        Err(e) => info.error = Some(e),
                    }
                    info.inputs = list_inputs(&host);
                }
                Err(e) => info.error = Some(e.to_string()),
            }
            #[cfg(windows)]
            if info.name == "WASAPI" {
                for device in &mut info.devices {
                    device.exclusive = exclusive.iter().any(|name| same_name(name, &device.name));
                }
            }
            info
        })
        .collect()
}

/// The host's input devices with their channels and sample rates; none when they cannot be
/// listed.
fn list_inputs(host: &cpal::Host) -> Vec<InputInfo> {
    let default = default_input(host).map(|d| d.to_string());
    let Ok(devices) = input_devices(host) else { return Vec::new() };
    devices
        .into_iter()
        .map(|device| {
            let name = device.to_string();
            let mut ranges = Vec::new();
            let mut channels = 0;
            if let Ok(configs) = device.supported_input_configs() {
                for config in configs {
                    ranges.push((config.min_sample_rate(), config.max_sample_rate()));
                    channels = channels.max(config.channels());
                }
            }
            InputInfo {
                default: default.as_deref() == Some(name.as_str()),
                channels,
                sample_rates: sample_rates(&ranges),
                name,
            }
        })
        .filter(|input| input.channels > 0)
        .collect()
}

fn list_devices(host: &cpal::Host, host_name: &str) -> Result<Vec<DeviceInfo>, String> {
    let default = default_output(host).map(|d| d.to_string());
    let devices = output_devices(host)?;
    Ok(devices
        .into_iter()
        .map(|device| {
            let name = device.to_string();
            let mut info = DeviceInfo { default: default.as_deref() == Some(name.as_str()), ..DeviceInfo::default() };
            let mut ranges = Vec::new();
            let mut buffer_ranges = Vec::new();
            if let Ok(configs) = device.supported_output_configs() {
                for config in configs {
                    ranges.push((config.min_sample_rate(), config.max_sample_rate()));
                    info.channels = info.channels.max(config.channels());
                    if let Some(range) = buffer_range(config.buffer_size()) {
                        buffer_ranges.push(range);
                    }
                }
            }
            let default_config = device.default_output_config().ok();
            if let Some(config) = &default_config {
                info.default_sample_rate = Some(config.sample_rate()).filter(|r| *r > 0 && *r <= MAX_SAMPLE_RATE);
                info.channels = info.channels.max(config.channels());
            }
            info.buffer_range = declared_range(
                default_config.as_ref().and_then(|config| buffer_range(config.buffer_size())),
                buffer_ranges,
            );
            info.sample_rates = sample_rates(&ranges);
            info.buffer_sizes = offered_buffers(host_name, info.buffer_range);
            info.name = name;
            info
        })
        .collect())
}

#[cfg(test)]
#[path = "devices_tests.rs"]
mod tests;
