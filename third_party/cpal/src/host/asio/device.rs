use std::{
    fmt,
    hash::{Hash, Hasher},
    sync::{atomic::AtomicU32, Arc, Mutex},
};

use super::sys;
pub use crate::iter::{SupportedInputConfigs, SupportedOutputConfigs};
use crate::{
    host::com, ChannelCount, DeviceDescription, DeviceDescriptionBuilder, DeviceId, Error,
    ErrorKind, FrameCount, SampleFormat, SampleRate, SupportedBufferSize, SupportedStreamConfig,
    SupportedStreamConfigRange,
};

/// A ASIO Device
#[derive(Clone)]
pub struct Device {
    name: String,

    // Metadata cached during enumeration
    channels_in: ChannelCount,
    channels_out: ChannelCount,
    sample_rate: SampleRate,
    /// What `ASIOGetBufferSize` answered when last asked, shared by every clone of this
    /// descriptor so a fresh answer reaches the listing too (Suhina patch).
    buffer_sizes: Arc<SharedBufferSizes>,
    input_sample_format: Option<SampleFormat>,
    output_sample_format: Option<SampleFormat>,
    supported_sample_rates: Box<[SampleRate]>,

    pub(super) current_callback_flag: Arc<AtomicU32>,
}

/// What `ASIOGetBufferSize` answered: the smallest, largest and preferred buffer size in frames,
/// and the granularity as the driver gave it (-1: powers of two between the smallest and the
/// largest; 0 with the smallest equal to the largest: that one size; a positive step otherwise).
/// Suhina patch: CPAL keeps only the smallest and largest.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BufferSizes {
    pub min: FrameCount,
    pub max: FrameCount,
    pub preferred: FrameCount,
    pub granularity: i32,
}

impl BufferSizes {
    fn from_range(range: &sys::BufferSizeRange) -> Self {
        let (preferred, granularity) = match range.preferred {
            sys::BufferPreference::Only(preferred) => (preferred, -1),
            sys::BufferPreference::Preferred(preferred) => (preferred, 0),
            sys::BufferPreference::Stepped { preferred, step } => (preferred, step as i32),
        };
        Self {
            min: range.min.max(0) as FrameCount,
            max: range.max.max(0) as FrameCount,
            preferred,
            granularity,
        }
    }
}

/// The last answer, behind one lock so a reader never sees half of a new answer. It is never
/// touched from an audio callback.
#[derive(Debug)]
struct SharedBufferSizes(Mutex<BufferSizes>);

impl SharedBufferSizes {
    fn new(sizes: BufferSizes) -> Self {
        Self(Mutex::new(sizes))
    }

    fn store(&self, sizes: BufferSizes) {
        *self.0.lock().unwrap_or_else(|e| e.into_inner()) = sizes;
    }

    fn load(&self) -> BufferSizes {
        *self.0.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// Keeps an ASIO driver loaded while it lives (Suhina patch). [`Device::refresh_buffer_sizes`]
/// hands one back so the stream built next uses the driver that answered, instead of the driver
/// being exited and loaded again in between. Dropping the last handle exits the driver.
#[must_use]
pub struct LoadedDriver {
    _driver: sys::Driver,
}

/// All available devices.
pub struct Devices {
    asio: Arc<sys::Asio>,
    drivers: std::vec::IntoIter<String>,
    current_driver: Option<sys::Driver>,
}

impl Device {
    pub fn description(&self) -> Result<DeviceDescription, Error> {
        let direction = crate::device_description::direction_from_counts(
            Some(self.channels_in),
            Some(self.channels_out),
        );

        Ok(DeviceDescriptionBuilder::new(&self.name)
            .driver(&self.name)
            .direction(direction)
            .build())
    }

    pub fn id(&self) -> Result<DeviceId, Error> {
        Ok(DeviceId::new(crate::platform::HostId::Asio, &self.name))
    }

    /// What the driver answered about its buffer sizes when last asked: at enumeration, or at the
    /// last [`Device::refresh_buffer_sizes`] on this descriptor or a clone of it. No driver is
    /// called.
    pub fn buffer_sizes(&self) -> BufferSizes {
        self.buffer_sizes.load()
    }

    /// Asks the driver for its buffer sizes again and keeps the answer: a size set in the
    /// driver's control panel since enumeration shows only this way. Uses the driver already
    /// loaded when it is this one, and otherwise loads it; fails while another driver is
    /// loaded. The driver stays loaded while the returned [`LoadedDriver`] lives, so a stream
    /// built meanwhile uses it rather than loading it again.
    pub fn refresh_buffer_sizes(&self) -> Result<(BufferSizes, LoadedDriver), Error> {
        com::com_initialized();
        let asio = super::GLOBAL_ASIO.get().ok_or_else(|| {
            Error::with_message(ErrorKind::DeviceNotAvailable, "ASIO is not initialized")
        })?;
        let driver = asio
            .load_driver(&self.name)
            .map_err(|e| Error::with_message(ErrorKind::DeviceNotAvailable, e.to_string()))?;
        let range = driver
            .buffersize_range()
            .map_err(|e| Error::with_message(ErrorKind::BackendError, e.to_string()))?;
        let sizes = BufferSizes::from_range(&range);
        self.buffer_sizes.store(sizes);
        Ok((sizes, LoadedDriver { _driver: driver }))
    }

    /// Gets the supported input configs.
    /// TODO currently only supports the default.
    /// Need to find all possible configs.
    pub fn supported_input_configs(&self) -> Result<SupportedInputConfigs, Error> {
        let default = self.default_input_config()?;
        Ok(self.configs_for(default).into_iter())
    }

    /// Gets the supported output configs.
    /// TODO currently only supports the default.
    /// Need to find all possible configs.
    pub fn supported_output_configs(&self) -> Result<SupportedOutputConfigs, Error> {
        let default = self.default_output_config()?;
        Ok(self.configs_for(default).into_iter())
    }

    /// Returns the default input config
    pub fn default_input_config(&self) -> Result<SupportedStreamConfig, Error> {
        self.default_config(self.channels_in, self.input_sample_format)
    }

    /// Returns the default output config
    pub fn default_output_config(&self) -> Result<SupportedStreamConfig, Error> {
        self.default_config(self.channels_out, self.output_sample_format)
    }

    fn default_config(
        &self,
        channels: ChannelCount,
        sample_format: Option<SampleFormat>,
    ) -> Result<SupportedStreamConfig, Error> {
        if channels == 0 {
            return Err(Error::with_message(
                ErrorKind::UnsupportedOperation,
                "Device reports no channels for this direction",
            ));
        }
        let sample_format = sample_format.ok_or_else(|| {
            Error::with_message(
                ErrorKind::UnsupportedOperation,
                "No supported sample format",
            )
        })?;
        Ok(SupportedStreamConfig {
            channels,
            sample_rate: self.sample_rate,
            buffer_size: {
                let sizes = self.buffer_sizes.load();
                SupportedBufferSize::Range {
                    min: sizes.min,
                    max: sizes.max,
                }
            },
            sample_format,
        })
    }

    fn configs_for(&self, default: SupportedStreamConfig) -> Vec<SupportedStreamConfigRange> {
        let mut configs = Vec::with_capacity(default.channels as usize);
        for &rate in &self.supported_sample_rates {
            for channels in 1..=default.channels {
                configs.push(SupportedStreamConfigRange {
                    channels,
                    min_sample_rate: rate,
                    max_sample_rate: rate,
                    buffer_size: default.buffer_size,
                    sample_format: default.sample_format,
                });
            }
        }
        configs
    }
}

impl PartialEq for Device {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}

impl Eq for Device {}

impl Hash for Device {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.name.hash(state);
    }
}

impl fmt::Display for Device {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let desc = self.description().map_err(|_| fmt::Error)?;
        f.write_str(desc.name())
    }
}

impl fmt::Debug for Device {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Device")
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

impl Devices {
    pub fn new(asio: Arc<sys::Asio>) -> Result<Self, Error> {
        // Make sure that COM is initialized.
        com::com_initialized();
        let drivers = asio.driver_names().into_iter();
        Ok(Self {
            asio,
            drivers,
            current_driver: None,
        })
    }
}

impl Iterator for Devices {
    type Item = Device;

    /// Enumerate devices by briefly loading each driver to capture its metadata.
    fn next(&mut self) -> Option<Self::Item> {
        // Drop the previously loaded driver before attempting to load the next one.
        self.current_driver = None;

        loop {
            let name = self.drivers.next()?;
            match self.asio.load_driver(&name) {
                Ok(driver) => {
                    let Ok(channels) = driver.channels() else {
                        continue;
                    };
                    if channels.ins == 0 && channels.outs == 0 {
                        continue;
                    }

                    // Some drivers (e.g. Realtek ASIO) return 0 for sample_rate() until a
                    // stream is active. Treat 0 as "not yet known" rather than skipping.
                    let sample_rate = driver.sample_rate().unwrap_or(0.0);

                    let Ok(buffer_size_range) = driver.buffersize_range() else {
                        continue;
                    };

                    let input_sample_format = driver
                        .input_data_type()
                        .ok()
                        .and_then(|t| convert_data_type(&t));
                    let output_sample_format = driver
                        .output_data_type()
                        .ok()
                        .and_then(|t| convert_data_type(&t));

                    let supported_sample_rates: Box<[SampleRate]> = crate::COMMON_SAMPLE_RATES
                        .iter()
                        .copied()
                        .filter(|&r| driver.can_sample_rate(r.into()).unwrap_or(false))
                        .collect();

                    self.current_driver = Some(driver);

                    return Some(Device {
                        name,
                        channels_in: channels.ins as ChannelCount,
                        channels_out: channels.outs as ChannelCount,
                        sample_rate: sample_rate as SampleRate,
                        buffer_sizes: Arc::new(SharedBufferSizes::new(BufferSizes::from_range(
                            &buffer_size_range,
                        ))),
                        input_sample_format,
                        output_sample_format,
                        supported_sample_rates,
                        // Initialize with sentinel value so it never matches global flag state (0 or 1).
                        current_callback_flag: Arc::new(AtomicU32::new(u32::MAX)),
                    });
                }
                // A different driver is already loaded (e.g. an active Stream holds it). Stop
                // cleanly rather than spinning through the rest of the list.
                Err(sys::LoadDriverError::DriverAlreadyExists) => return None,
                // Driver failed to load for its own reasons; skip and try the next.
                Err(_) => continue,
            }
        }
    }
}

pub(crate) fn convert_data_type(ty: &sys::AsioSampleType) -> Option<SampleFormat> {
    let fmt = match *ty {
        sys::AsioSampleType::ASIOSTInt16MSB => SampleFormat::I16,
        sys::AsioSampleType::ASIOSTInt16LSB => SampleFormat::I16,
        sys::AsioSampleType::ASIOSTInt24MSB => SampleFormat::I24,
        sys::AsioSampleType::ASIOSTInt24LSB => SampleFormat::I24,
        sys::AsioSampleType::ASIOSTInt32MSB => SampleFormat::I32,
        sys::AsioSampleType::ASIOSTInt32LSB => SampleFormat::I32,
        sys::AsioSampleType::ASIOSTFloat32MSB => SampleFormat::F32,
        sys::AsioSampleType::ASIOSTFloat32LSB => SampleFormat::F32,
        sys::AsioSampleType::ASIOSTFloat64MSB => SampleFormat::F64,
        sys::AsioSampleType::ASIOSTFloat64LSB => SampleFormat::F64,
        _ => return None,
    };
    Some(fmt)
}
