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

/// The host's devices that play. The host says which those are: asking each for its formats here
/// would drop one that cannot answer yet, as a Bluetooth endpoint often cannot, and the list would
/// not say why. Such a device is listed, and opening it says what is wrong.
pub(crate) fn output_devices(host: &cpal::Host) -> Result<Vec<cpal::Device>, String> {
    Ok(host_devices(host)?.into_iter().filter(DeviceTrait::supports_output).collect())
}

/// The host's devices that record; the host says which, as for [`output_devices`].
pub(crate) fn input_devices(host: &cpal::Host) -> Result<Vec<cpal::Device>, String> {
    Ok(host_devices(host)?.into_iter().filter(DeviceTrait::supports_input).collect())
}

/// The host that plays when none is chosen. On Linux and the BSDs that stays ALSA even with a sound
/// server running, whose own host is one to choose: ALSA's `default` already plays through the
/// server, so nobody's playback moves. Elsewhere it is cpal's default.
pub(crate) fn default_host_id() -> cpal::HostId {
    #[cfg(any(target_os = "linux", target_os = "dragonfly", target_os = "freebsd", target_os = "netbsd"))]
    {
        cpal::HostId::Alsa
    }
    #[cfg(not(any(target_os = "linux", target_os = "dragonfly", target_os = "freebsd", target_os = "netbsd")))]
    {
        cpal::default_host().id()
    }
}

/// A sound server's host: it converts whatever it is given, so it offers every format and channel
/// count. What its device really is shows only in its default config.
pub(crate) fn server_converts(host: &str) -> bool {
    host == "PulseAudio"
}

/// The buffer to fit on `host`: the one chosen, and on a sound server the engine's own block when
/// none is. A server left to its default buffers about two seconds ahead.
pub(crate) fn wanted_buffer(host: &str, chosen: Option<u32>, engine_block: u32) -> Option<u32> {
    chosen.or(server_converts(host).then_some(engine_block))
}

/// The name a device goes by, or `None` when it cannot say, as when it was just disconnected. cpal's
/// `Display` fails then, and `to_string` would panic.
pub(crate) fn name_of(device: &cpal::Device) -> Option<String> {
    device.description().ok().map(|description| description.name().to_string())
}

/// Whether `device` goes by `name`.
pub(crate) fn named(device: &cpal::Device, name: &str) -> bool {
    name_of(device).is_some_and(|own| same_name(&own, name))
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

/// Which sizes between its smallest and largest an ASIO driver takes: the granularity
/// `ASIOGetBufferSize` answers with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Granularity {
    /// -1: the powers of two from the smallest to the largest.
    PowersOfTwo,
    /// 0: with the smallest equal to the largest that is its only size; otherwise only the one it
    /// prefers is sure to be taken, or with a preferred size outside its range, its two ends.
    Unstated,
    /// The smallest size and every `step` frames after it.
    Step(u32),
}

impl Granularity {
    /// The granularity a driver answered with. One that is neither -1, 0 nor positive is taken as
    /// a step too large to reach any size but the smallest, as the stream's own check takes it.
    pub fn from_driver(granularity: i32) -> Self {
        match granularity {
            -1 => Self::PowersOfTwo,
            0 => Self::Unstated,
            step => Self::Step(u32::try_from(step).unwrap_or(u32::MAX)),
        }
    }
}

/// What an ASIO driver answers about its buffer sizes (`ASIOGetBufferSize`): the smallest, the
/// largest, the one it prefers and the steps between. A driver whose size is set in its own
/// control panel commonly answers with that one size as smallest, largest and preferred.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DriverBuffers {
    pub min: u32,
    pub max: u32,
    pub preferred: u32,
    pub granularity: Granularity,
}

impl DriverBuffers {
    /// The answer as `ASIOGetBufferSize` gives it, the granularity as the driver's number.
    pub fn new(min: u32, max: u32, preferred: u32, granularity: i32) -> Self {
        Self { min, max, preferred, granularity: Granularity::from_driver(granularity) }
    }

    /// The range declared, when it makes sense.
    pub fn range(&self) -> Option<(u32, u32)> {
        (self.min > 0 && self.min <= self.max).then_some((self.min, self.max))
    }

    /// Whether the driver takes `frames`: inside its range and on its granularity. A driver
    /// answering one size (the smallest is the largest) takes that size whatever granularity it
    /// states: drivers fixed to their control panel's size answer -1 with it too. With nothing
    /// stated (0) it takes the size it prefers, or, when that lies outside its range, only the
    /// two ends it declared.
    fn takes(&self, frames: u32) -> bool {
        within(self.range(), frames)
            && match self.granularity {
                _ if self.min == self.max => frames == self.min,
                Granularity::PowersOfTwo => frames.is_power_of_two(),
                Granularity::Unstated if within(self.range(), self.preferred) => frames == self.preferred,
                Granularity::Unstated => frames == self.min || frames == self.max,
                Granularity::Step(step) => step > 0 && (frames - self.min).is_multiple_of(step),
            }
    }

    /// The sizes it can be asked for, smallest first, within [`MIN_BUFFER_FRAMES`] …
    /// [`MAX_BUFFER_FRAMES`]: with powers of two every power in its range; with a step every size
    /// on it within the bounded engine range; with nothing stated its preferred size alone (its one size when the smallest
    /// is the largest). Its smallest, largest and preferred are in when it takes them. A range
    /// that makes no sense holds nothing: such a driver is asked for no size.
    pub fn sizes(&self) -> Vec<u32> {
        let Some((min, max)) = self.range() else {
            return Vec::new();
        };
        let engine = |n: &u32| (MIN_BUFFER_FRAMES..=MAX_BUFFER_FRAMES).contains(n);
        let mut sizes: Vec<u32> = match self.granularity {
            Granularity::Step(step) => {
                let top = max.min(MAX_BUFFER_FRAMES);
                if min <= top { (min..=top).step_by(step.max(1) as usize).filter(engine).collect() } else { Vec::new() }
            }
            // Powers of two; `takes` keeps those it takes, and the ends and preferred size added
            // below are the rest of what a driver with nothing stated takes.
            Granularity::PowersOfTwo | Granularity::Unstated => {
                (0..u32::BITS).filter_map(|shift| 1u32.checked_shl(shift)).collect()
            }
        };
        sizes.extend([min, max, self.preferred]);
        sizes.retain(|n| engine(n) && self.takes(*n));
        sizes.sort_unstable();
        sizes.dedup();
        sizes
    }

    /// The size that plays when none is chosen: the one the driver prefers (a driver whose size
    /// is set in its control panel prefers that size), or when it cannot be asked for that one,
    /// the size it takes nearest the engine's own block, so the engine renders in the size the
    /// driver plays. None when it takes no size the engine renders in.
    pub fn default_size(&self) -> Option<u32> {
        let sizes = self.sizes();
        if sizes.contains(&self.preferred) {
            Some(self.preferred)
        } else if sizes.is_empty() {
            None
        } else {
            Some(fit_buffer(u32::try_from(crate::ENGINE_BLOCK).unwrap_or(MAX_BUFFER_FRAMES), &sizes))
        }
    }

    /// The answer as the trace writes it, with what Suhina makes of it: `min=128 max=128
    /// preferred=128 granularity=Unstated; sizes [128], default Some(128)`.
    pub fn trace_text(&self) -> String {
        format!(
            "min={} max={} preferred={} granularity={:?}; sizes {:?}, default {:?}",
            self.min,
            self.max,
            self.preferred,
            self.granularity,
            self.sizes(),
            self.default_size()
        )
    }

    /// What it takes, in words for a notice: "only 128 frames", "64 to 2048 frames in powers of
    /// two", "48 to 480 frames in steps of 48".
    pub fn describe(&self) -> String {
        match self.granularity {
            _ if self.min == self.max => format!("only {} frames", self.min),
            Granularity::Unstated if within(self.range(), self.preferred) => format!("only {} frames", self.preferred),
            Granularity::Unstated => format!("{} or {} frames", self.min, self.max),
            Granularity::PowersOfTwo => format!("{} to {} frames in powers of two", self.min, self.max),
            Granularity::Step(step) => format!("{} to {} frames in steps of {step}", self.min, self.max),
        }
    }
}

/// Fits a wanted buffer size to an ASIO driver's answer: the frames the engine renders per block,
/// and the size to ask the driver for (none when it takes no size the engine renders in, and
/// the driver then keeps its own).
pub fn fit_to_driver(wanted: u32, driver: &DriverBuffers) -> (u32, Option<u32>) {
    let sizes = driver.sizes();
    let fitted = fit_buffer(wanted, &sizes);
    (fitted, sizes.contains(&fitted).then_some(fitted))
}

/// The engine block and the size to ask an ASIO driver for, from the size `chosen` (none: the
/// driver's default size) and the driver's answer; `None` when neither gives a size, and the
/// driver keeps its own while the engine renders in its own block.
pub fn plan_for_driver(chosen: Option<u32>, driver: &DriverBuffers) -> Option<(u32, Option<u32>)> {
    chosen.or_else(|| driver.default_size()).map(|wanted| fit_to_driver(wanted, driver))
}

/// What to tell the user when the size they chose is not the one that plays on an ASIO driver:
/// what the driver takes and what plays, or that it takes no size the engine renders in. `None`
/// when they got what they chose.
pub fn driver_notice(
    device: &str,
    wanted: u32,
    block: u32,
    asked: Option<u32>,
    driver: &DriverBuffers,
) -> Option<String> {
    match (asked, driver.sizes().as_slice()) {
        (Some(_), _) if block == wanted => None,
        (None, _) if driver.range().is_some() => Some(format!(
            "{device} takes {}, which Suhina cannot render in; it keeps its own buffer size",
            driver.describe()
        )),
        (None, _) if block == wanted => None,
        (None, _) => Some(format!("{device} sets its own buffer size; the engine renders in {block}-frame blocks")),
        (Some(_), [_]) => Some(format!("{device} plays only {block} frames, the size set in its control panel")),
        (Some(_), _) => Some(format!("{device} takes {}; {block} frames play", driver.describe())),
    }
}

/// Keeps an ASIO driver loaded while it lives (`cpal`'s `LoadedDriver`): the stream built next
/// uses the driver that answered instead of loading it again.
pub(crate) type DriverHold = Box<dyn std::any::Any + Send>;

/// What the ASIO `device`'s driver answered about its buffer sizes when last asked: at listing, or
/// when an output last opened on it. No driver is called. A device of another host has none.
#[allow(unused_variables)]
pub(crate) fn driver_buffers(device: &cpal::Device) -> Option<DriverBuffers> {
    #[cfg(all(windows, feature = "asio"))]
    if let cpal::platform::DeviceInner::Asio(device) = device.as_inner() {
        let sizes = device.buffer_sizes();
        return Some(DriverBuffers::new(sizes.min, sizes.max, sizes.preferred, sizes.granularity));
    }
    None
}

/// Asks the ASIO `device`'s driver for its buffer sizes again, so a size set in its control panel
/// since shows, and keeps the answer for the listing. The driver already loaded is asked when it is
/// this one; otherwise it is loaded, and the returned hold keeps it loaded for the stream built
/// next. When it cannot be asked (another driver is loaded), the last answer stands. A device of
/// another host has none.
#[allow(unused_variables)]
pub(crate) fn ask_driver(device: &cpal::Device) -> Option<(DriverBuffers, Option<DriverHold>)> {
    #[cfg(all(windows, feature = "asio"))]
    if let cpal::platform::DeviceInner::Asio(device) = device.as_inner() {
        let (sizes, hold) = match device.refresh_buffer_sizes() {
            Ok((sizes, loaded)) => (sizes, Some(Box::new(loaded) as DriverHold)),
            Err(_) => (device.buffer_sizes(), None),
        };
        let driver = DriverBuffers::new(sizes.min, sizes.max, sizes.preferred, sizes.granularity);
        return Some((driver, hold));
    }
    None
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

/// The names of the devices there now, per host and sorted, for noticing one that comes or goes.
/// Only names are read, no formats, so it is quick. ASIO is not looked at: its list stays until a
/// restart (see [`host_devices`]) and listing it loads drivers.
pub(crate) fn device_names() -> Vec<String> {
    let mut names: Vec<String> = cpal::available_hosts()
        .into_iter()
        .filter(|id| id.name() != "ASIO")
        .filter_map(|id| Some((id.name(), cpal::host_from_id(id).ok()?)))
        .filter_map(|(host_name, host)| Some((host_name, host.devices().ok()?)))
        .flat_map(|(host_name, devices)| {
            devices.filter_map(move |device| Some(format!("{host_name}: {}", name_of(&device)?)))
        })
        .collect();
    names.sort_unstable();
    names
}

/// Lists every host cpal offers here and each one's output devices.
pub fn list_hosts() -> Vec<HostInfo> {
    let default_host = default_host_id();
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
    let host_name = host.id().name();
    let default = default_input(host).as_ref().and_then(name_of);
    let Ok(devices) = input_devices(host) else { return Vec::new() };
    devices
        .into_iter()
        .filter_map(|device| {
            let name = name_of(&device)?;
            let mut ranges = Vec::new();
            let mut most = 0u16;
            if let Ok(configs) = device.supported_input_configs() {
                for config in configs {
                    ranges.push((config.min_sample_rate(), config.max_sample_rate()));
                    most = most.max(config.channels());
                }
            }
            let own = server_converts(host_name).then(|| device.default_input_config().ok()).flatten();
            Some(InputInfo {
                default: default.as_deref() == Some(name.as_str()),
                channels: listed_channels(host_name, own.map(|config| config.channels()), most),
                sample_rates: sample_rates(&ranges),
                name,
            })
        })
        .filter(|input| input.channels > 0)
        .collect()
}

/// One format a device offers, as the list reads it.
#[derive(Clone, Copy)]
struct Offer {
    rates: (u32, u32),
    channels: u16,
    buffer: Option<(u32, u32)>,
}

/// What the list asks of a device; cpal's devices answer it, and the tests' own.
trait Listed {
    fn name(&self) -> Option<String>;
    fn plays(&self) -> bool;
    fn offers(&self) -> Vec<Offer>;
    /// The format it asks for itself.
    fn own(&self) -> Option<Offer>;
    /// What its ASIO driver answered about buffer sizes; none on other hosts.
    fn driver(&self) -> Option<DriverBuffers> {
        None
    }
}

impl Listed for cpal::Device {
    fn name(&self) -> Option<String> {
        name_of(self)
    }

    fn plays(&self) -> bool {
        self.supports_output()
    }

    fn offers(&self) -> Vec<Offer> {
        self.supported_output_configs().map_or_else(
            |_| Vec::new(),
            |configs| {
                configs
                    .map(|config| Offer {
                        rates: (config.min_sample_rate(), config.max_sample_rate()),
                        channels: config.channels(),
                        buffer: buffer_range(config.buffer_size()),
                    })
                    .collect()
            },
        )
    }

    fn own(&self) -> Option<Offer> {
        self.default_output_config().ok().map(|config| Offer {
            rates: (config.sample_rate(), config.sample_rate()),
            channels: config.channels(),
            buffer: buffer_range(config.buffer_size()),
        })
    }

    fn driver(&self) -> Option<DriverBuffers> {
        // The answer kept from the last time the driver was asked: listing calls no driver.
        driver_buffers(self)
    }
}

/// The channels to list for a device: the most it offers, and on a sound server, which offers every
/// count, the ones it asks for itself. A stereo pair of headphones has no outputs 3/4 to offer.
fn listed_channels(host_name: &str, own: Option<u16>, most: u16) -> u16 {
    match own {
        Some(channels) if server_converts(host_name) => channels,
        _ => most.max(own.unwrap_or(0)),
    }
}

/// The devices that play, with what each offers. A device is left out only when the host says it
/// does not play or it has no name to choose it by: one that offers one rate, or cannot say what it
/// offers yet, is listed all the same.
fn listed_outputs<D: Listed>(
    devices: impl IntoIterator<Item = D>,
    host_name: &str,
    default: Option<&str>,
) -> Vec<DeviceInfo> {
    devices
        .into_iter()
        .filter(Listed::plays)
        .filter_map(|device| {
            let name = device.name()?;
            let offers = device.offers();
            let own = device.own();
            let ranges: Vec<(u32, u32)> = offers.iter().map(|offer| offer.rates).collect();
            let buffer_range = declared_range(
                own.as_ref().and_then(|offer| offer.buffer),
                offers.iter().filter_map(|offer| offer.buffer),
            );
            let most = offers.iter().map(|offer| offer.channels).max().unwrap_or(0);
            // An ASIO driver's answer, when it makes sense; a broken one lists what the engine
            // renders in, and the driver is asked for no size.
            let driver = device.driver().filter(|driver| driver.range().is_some());
            let buffer_range = driver.map_or(buffer_range, |driver| driver.range());
            let buffer_sizes = driver.map_or_else(|| offered_buffers(host_name, buffer_range), |driver| driver.sizes());
            Some(DeviceInfo {
                default: default == Some(name.as_str()),
                channels: listed_channels(host_name, own.as_ref().map(|offer| offer.channels), most),
                sample_rates: sample_rates(&ranges),
                default_sample_rate: own.map(|offer| offer.rates.0).filter(|r| *r > 0 && *r <= MAX_SAMPLE_RATE),
                buffer_sizes,
                buffer_range,
                default_buffer: driver.and_then(|driver| driver.default_size()),
                name,
                ..DeviceInfo::default()
            })
        })
        .collect()
}

fn list_devices(host: &cpal::Host, host_name: &str) -> Result<Vec<DeviceInfo>, String> {
    let default = default_output(host).as_ref().and_then(name_of);
    Ok(listed_outputs(host_devices(host)?, host_name, default.as_deref()))
}

#[cfg(test)]
#[path = "devices_tests.rs"]
mod tests;
