// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Every built-in effect or instrument by its stable id (ADR 0004): what the model stores as
//! `DeviceKind::Builtin(id)` turns into a running device here. The engine builds device chains
//! through this crate; front ends list what can be added.

use std::any::Any;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use suhina_device_api::{Device, DeviceInfo};
use suhina_device_sdk::DeviceSdkError;
use suhina_device_sdk::builtin::{self, Layout, Panel};

/// One built-in device: everything the rest of Suhina asks the registry about it, in one row
/// (#764 › 9). [`DEVICES`], [`create`], [`layout`], [`panel`] and [`is_file_device`] read this
/// table, so a new device is one row here.
pub struct Entry {
    pub info: &'static DeviceInfo,
    /// A new device at its defaults.
    make: fn() -> Box<dyn Device>,
    /// Where its panel's layout comes from.
    layout: LayoutSource,
    /// The file it runs, whose path its `Device::state` holds (docs/spec/nam-amp-and-cab.md), and
    /// how it is read: the rest of Suhina loads a device's file through this, never by the
    /// device's id (#763 › 5).
    pub file: Option<FileKind>,
}

/// A file as its device read it: only that device knows what it holds.
type AssetData = dyn Any + Send + Sync;

/// What a device that runs a file reads, and how. Every function runs off the audio thread.
pub struct FileKind {
    /// The file's extension, lower case and without the dot. A path with another extension is
    /// refused before it is read, so a project cannot point the device at some other file.
    pub extension: &'static str,
    /// Reads and checks the file at `path` for an engine running at `rate` Hz.
    read: fn(&Path, u32) -> Result<Arc<AssetData>, String>,
    /// Reads and checks the pack's bytes ([ADR 0036] › the starter pack), the same parse the
    /// path form gave them.
    read_bytes: fn(&[u8], u32) -> Result<Arc<AssetData>, String>,
    /// The library folder the pack unpacks beside ([ADR 0036] › the library): "Amp models" for
    /// the Amp Modeler, "Cabinets" for the Cab Loader.
    pub library_folder: &'static str,
    /// The old defaults' folder the device's files may still be named by, before the library's
    /// move emptied it into `library_folder` ([ADR 0036] › 4): "NAM models" for the Amp
    /// Modeler, "IRs" for the Cab Loader, none for a device that ran no files then.
    pub legacy_folder: Option<&'static str>,
    /// What a panel calls the loaded file at `path`.
    name: fn(&Path, &AssetData) -> Option<String>,
    /// A new device at its defaults, running the file.
    make: fn(&AssetData) -> Option<Box<dyn Device>>,
}

/// Where a device's panel layout comes from (ADR 0021 › Amendment: device panels).
enum LayoutSource {
    /// None: the generated parameter list.
    Generated,
    /// `devices/<name>/panel.json`.
    Json(&'static str),
    /// Built in Rust by the device crate.
    Rust(fn() -> Layout),
}

/// The built-in effects, instruments and MIDI effects, in the order a device browser lists them.
pub const ENTRIES: [Entry; 17] = [
    Entry {
        info: &suhina_device_fm_synth::INFO,
        make: || Box::new(suhina_device_fm_synth::FmSynth::new()),
        layout: LayoutSource::Json(include_str!("../../../devices/fm-synth/panel.json")),
        file: None,
    },
    Entry {
        info: &suhina_device_sampler::INFO,
        make: || Box::new(suhina_device_sampler::Sampler::new()),
        layout: LayoutSource::Rust(suhina_device_sampler::layout),
        file: None,
    },
    Entry {
        info: &suhina_device_utility::INFO,
        make: || Box::new(suhina_device_utility::Utility::new()),
        layout: LayoutSource::Generated,
        file: None,
    },
    Entry {
        info: &suhina_device_auto_pan::INFO,
        make: || Box::new(suhina_device_auto_pan::AutoPan::new()),
        layout: LayoutSource::Json(include_str!("../../../devices/auto-pan/panel.json")),
        file: None,
    },
    Entry {
        info: &suhina_device_dj_eq::INFO,
        make: || Box::new(suhina_device_dj_eq::DjEq::new()),
        layout: LayoutSource::Json(include_str!("../../../devices/dj-eq/panel.json")),
        file: None,
    },
    Entry {
        info: &suhina_device_eq::INFO,
        make: || Box::new(suhina_device_eq::Eq::new()),
        layout: LayoutSource::Rust(suhina_device_eq::panel::layout),
        file: None,
    },
    Entry {
        info: &suhina_device_vinyl_distortion::INFO,
        make: || Box::new(suhina_device_vinyl_distortion::VinylDistortion::new()),
        layout: LayoutSource::Json(include_str!("../../../devices/vinyl-distortion/panel.json")),
        file: None,
    },
    Entry {
        info: &suhina_device_overdrive::INFO,
        make: || Box::new(suhina_device_overdrive::Overdrive::new()),
        layout: LayoutSource::Json(include_str!("../../../devices/overdrive/panel.json")),
        file: None,
    },
    Entry {
        info: &suhina_device_saturator::INFO,
        make: || Box::new(suhina_device_saturator::Saturator::new()),
        layout: LayoutSource::Generated,
        file: None,
    },
    Entry {
        info: &suhina_device_compressor::INFO,
        make: || Box::new(suhina_device_compressor::Compressor::new()),
        layout: LayoutSource::Json(include_str!("../../../devices/compressor/panel.json")),
        file: None,
    },
    Entry {
        info: &suhina_device_multiband::INFO,
        make: || Box::new(suhina_device_multiband::Multiband::new()),
        layout: LayoutSource::Json(include_str!("../../../devices/multiband/panel.json")),
        file: None,
    },
    Entry {
        info: &suhina_device_limiter::INFO,
        make: || Box::new(suhina_device_limiter::Limiter::new()),
        layout: LayoutSource::Json(include_str!("../../../devices/limiter/panel.json")),
        file: None,
    },
    Entry {
        info: &suhina_device_nam_amp_modeler::INFO,
        make: || Box::new(suhina_device_nam_amp_modeler::NamAmp::new()),
        layout: LayoutSource::Json(include_str!("../../../devices/nam-amp-modeler/panel.json")),
        file: Some(FileKind {
            extension: "nam",
            read: read_nam,
            read_bytes: read_nam_bytes,
            library_folder: "Amp models",
            legacy_folder: Some("NAM models"),
            name: nam_name,
            make: make_nam,
        }),
    },
    Entry {
        info: &suhina_device_nam_cab_loader::INFO,
        make: || Box::new(suhina_device_nam_cab_loader::CabLoader::new()),
        layout: LayoutSource::Json(include_str!("../../../devices/nam-cab-loader/panel.json")),
        file: Some(FileKind {
            extension: "wav",
            read: read_ir,
            read_bytes: read_ir_bytes,
            library_folder: "Cabinets",
            legacy_folder: Some("IRs"),
            name: ir_name,
            make: make_cab,
        }),
    },
    Entry {
        info: &suhina_device_delay::INFO,
        make: || Box::new(suhina_device_delay::Delay::new()),
        layout: LayoutSource::Json(include_str!("../../../devices/delay/panel.json")),
        file: None,
    },
    Entry {
        info: &suhina_device_reverb::INFO,
        make: || Box::new(suhina_device_reverb::Reverb::new()),
        layout: LayoutSource::Json(include_str!("../../../devices/reverb/panel.json")),
        file: None,
    },
    Entry {
        info: &suhina_device_pitch::INFO,
        make: || Box::new(suhina_device_pitch::Pitch::new()),
        layout: LayoutSource::Json(include_str!("../../../devices/pitch/panel.json")),
        file: None,
    },
];

/// What each entry is, in the order of [`ENTRIES`].
pub const DEVICES: [&DeviceInfo; ENTRIES.len()] = {
    let mut out = [ENTRIES[0].info; ENTRIES.len()];
    let mut i = 0;
    while i < ENTRIES.len() {
        out[i] = ENTRIES[i].info;
        i += 1;
    }
    out
};

fn entry(id: &str) -> Option<&'static Entry> {
    ENTRIES.iter().find(|entry| entry.info.id == id)
}

/// What the device with this id is, or `None` for an unknown id.
pub fn info(id: &str) -> Option<&'static DeviceInfo> {
    entry(id).map(|entry| entry.info)
}

/// The layout of the device with this id, when it has one (ADR 0021 › Amendment: device panels
/// for built-in devices): the `.sd` UI part only, its parameters named by id. A device crate
/// gives it in Rust, or as `devices/<name>/panel.json`.
pub fn layout(id: &str) -> Option<Result<Layout, DeviceSdkError>> {
    match entry(id)?.layout {
        LayoutSource::Generated => None,
        LayoutSource::Json(text) => Some(Layout::from_json(text)),
        LayoutSource::Rust(make) => Some(Ok(make())),
    }
}

/// The ids of the devices whose layout is a `panel.json`, with its text.
pub fn layout_files() -> impl Iterator<Item = (&'static str, &'static str)> {
    ENTRIES.iter().filter_map(|entry| match entry.layout {
        LayoutSource::Json(text) => Some((&*entry.info.id, text)),
        LayoutSource::Generated | LayoutSource::Rust(_) => None,
    })
}

/// What a device just added to a project starts with beside its parameters' defaults.
pub fn new_device_params(id: &str) -> &'static [(&'static str, f64)] {
    if id == suhina_device_fm_synth::INFO.id { &suhina_device_fm_synth::params::NEW_DEVICE } else { &[] }
}

/// New values for a device's randomizer from its current `values`, in parameter order: which
/// parameters change and to what. `None` for a device without a randomizer.
pub fn randomize(id: &str, values: &[f64], seed: Option<u32>, amount: Option<f64>) -> Option<Vec<(usize, f64)>> {
    (id == suhina_device_fm_synth::INFO.id).then(|| suhina_device_fm_synth::random::randomize(values, seed, amount))
}

/// The layouts that do not load, by device id with the reason: such a device shows the generated
/// parameter list instead, so the app writes these to its log at start.
pub fn layout_errors() -> Vec<(&'static str, String)> {
    DEVICES
        .iter()
        .filter_map(|info| {
            let id: &'static str = &info.id;
            match layout(id)? {
                Err(e) => Some((id, e.to_string())),
                Ok(layout) => layout.panel(info).err().map(|e| (id, e.to_string())),
            }
        })
        .collect()
}

/// The panel of the device with this id: its parameters from its `DeviceInfo`, laid out by its
/// layout, or the generated parameter list when it has none or its layout does not load.
/// Made once per id while Suhina runs, off the audio thread. `None` for an unknown id.
pub fn panel(id: &str) -> Option<&'static Panel> {
    /// Every device's panel, in the order of [`DEVICES`].
    static PANELS: OnceLock<Vec<Panel>> = OnceLock::new();
    let index = DEVICES.iter().position(|info| info.id == id)?;
    PANELS
        .get_or_init(|| {
            DEVICES
                .iter()
                .map(|info| {
                    layout(&info.id)
                        .and_then(Result::ok)
                        .and_then(|layout| layout.panel(info).ok())
                        .unwrap_or_else(|| builtin::generated(info))
                })
                .collect()
        })
        .get(index)
}

/// A new device with this id at its defaults, prepared for 48 kHz, or `None` for an unknown
/// id. Allocates: call it off the audio thread.
pub fn create(id: &str) -> Option<Box<dyn Device>> {
    entry(id).map(|entry| (entry.make)())
}

pub use suhina_device_sdk::{DeviceAsset, DeviceFactory, Devices};

/// What a panel calls the file `asset` holds, read from `path`: a NAM model's own name (its
/// metadata), an IR's folder.
pub fn asset_name(asset: &DeviceAsset, path: &Path) -> Option<String> {
    file_kind(asset.device()).and_then(|file| (file.name)(path, asset.data()))
}

/// A shipped file's key as a library-shaped path, for the name fns that read a folder from a
/// path: the Cab Loader's shown name is its pack's name ([docs/spec/nam-amp-and-cab.md]) and
/// the shipped pack's name is "Suhina" ([ADR 0036]). The Amp Modeler's name fn reads the
/// model's metadata and leaves the path alone, so the shape is only for the folder.
pub fn builtin_file_path(key: &str) -> Option<PathBuf> {
    let rest = key.strip_prefix("builtin:")?;
    let (_, name) = rest.split_once('/')?;
    Some(PathBuf::from(format!("Suhina/{name}")))
}

/// Every built-in device, as the factory the engine builds with (#1147).
pub struct Registry;

impl DeviceFactory for Registry {
    fn create(&self, id: &str, asset: Option<&DeviceAsset>) -> Option<Box<dyn Device>> {
        create_with(id, asset)
    }

    fn graph_device(
        &self,
        graph: &suhina_device_sdk::DeviceGraph,
        rate: f64,
        samples: suhina_device_sdk::sample_resource::PreparedSamples,
    ) -> Result<suhina_device_sdk::GraphDevice, suhina_device_sdk::DeviceSdkError> {
        graph_device(graph, rate, samples)
    }

    fn runs_a_file(&self, id: &str) -> bool {
        is_file_device(id)
    }

    /// The FM synth, which plays a previewed note on a track with no instrument.
    fn preview_instrument(&self) -> Option<Box<dyn Device>> {
        create("fm-synth")
    }
}

/// The registry as the engine's device factory: one value, shared.
pub fn factory() -> Devices {
    static FACTORY: OnceLock<Devices> = OnceLock::new();
    Arc::clone(FACTORY.get_or_init(|| Arc::new(Registry)))
}

/// The ids whose devices run a file stored in `Device::state` (docs/spec/nam-amp-and-cab.md).
pub fn is_file_device(id: &str) -> bool {
    file_kind(id).is_some()
}

fn file_kind(id: &str) -> Option<&'static FileKind> {
    entry(id).and_then(|entry| entry.file.as_ref())
}

/// The extension of the file the device `id` runs, or `None` for a device that runs no file.
/// The old defaults' folder a file device's files may still be named by, before the library's
/// move emptied it into the device's library folder ([ADR 0036] › 4): `None` for a device
/// that ran no files then.
pub fn legacy_folder(id: &str) -> Option<&'static str> {
    file_kind(id).and_then(|file| file.legacy_folder)
}

/// The library folder a file device's pack unpacks beside ([ADR 0036] › the library):
/// "Amp models" for the Amp Modeler, "Cabinets" for the Cab Loader.
pub fn library_folder(id: &str) -> Option<&'static str> {
    file_kind(id).map(|file| file.library_folder)
}

pub fn file_extension(id: &str) -> Option<&'static str> {
    file_kind(id).map(|file| file.extension)
}

/// Reads the file at `path` for the device `id`, for an engine running at `rate` Hz. Fails for a
/// device that runs no file, for a path without the device's extension, and for a file the
/// device cannot run, saying why. Reads and allocates: call it off the audio thread.
pub fn load_file(id: &str, path: &Path, rate: u32) -> Result<DeviceAsset, String> {
    let Some((entry, file)) = entry(id).and_then(|entry| Some((entry, entry.file.as_ref()?))) else {
        return Err(format!("{id:?} does not run a file"));
    };
    let extension = path.extension().map(|e| e.to_string_lossy().to_lowercase());
    if extension.as_deref() != Some(file.extension) {
        return Err(format!("a {id} runs a .{} file", file.extension));
    }
    Ok(DeviceAsset::new(&entry.info.id, (file.read)(path, rate)?))
}

/// Every shipped file ([ADR 0036] › the starter pack): the device's id, the file's library
/// name, and its bytes — `include_bytes!` puts the raw pack inside the executable, so the
/// devices work at once and offline, with nothing on disk. Keep the pack small: the ADR caps
/// the whole thing, and a test here keeps the rows honest.
const PACK_FILES: &[(&str, &str, &[u8])] = &[
    (
        "nam-amp-modeler",
        "Helga B 5150 BlockLetter - NoBoost.nam",
        include_bytes!("../../../packs/nam-amp-modeler/Helga B 5150 BlockLetter - NoBoost.nam"),
    ),
    (
        "nam-amp-modeler",
        "Jason Z KSR CERES - light blue channel - all noon EQ.nam",
        include_bytes!("../../../packs/nam-amp-modeler/Jason Z KSR CERES - light blue channel - all noon EQ.nam"),
    ),
    (
        "nam-amp-modeler",
        "Mikhail K Sovtek MIG50.nam",
        include_bytes!("../../../packs/nam-amp-modeler/Mikhail K Sovtek MIG50.nam"),
    ),
    (
        "nam-amp-modeler",
        "Peter N HM-2_V5_L10_H10_D0_Std_ESR-0.0004.nam",
        include_bytes!("../../../packs/nam-amp-modeler/Peter N HM-2_V5_L10_H10_D0_Std_ESR-0.0004.nam"),
    ),
    (
        "nam-amp-modeler",
        "Phillipe P Bug333-Clean-ESR0,002.nam",
        include_bytes!("../../../packs/nam-amp-modeler/Phillipe P Bug333-Clean-ESR0,002.nam"),
    ),
    (
        "nam-amp-modeler",
        "Phillipe P Bug333-Crunch-NoDrive-ESR0,002.nam",
        include_bytes!("../../../packs/nam-amp-modeler/Phillipe P Bug333-Crunch-NoDrive-ESR0,002.nam"),
    ),
    (
        "nam-amp-modeler",
        "Phillipe P LaneyGH100S-Lo-CrunchGain-ESR0,024.nam",
        include_bytes!("../../../packs/nam-amp-modeler/Phillipe P LaneyGH100S-Lo-CrunchGain-ESR0,024.nam"),
    ),
    (
        "nam-amp-modeler",
        "Roman A LT_MESA_MARKIV_1.nam",
        include_bytes!("../../../packs/nam-amp-modeler/Roman A LT_MESA_MARKIV_1.nam"),
    ),
    (
        "nam-amp-modeler",
        "Roman A YERASOV_MESHUGGAH.nam",
        include_bytes!("../../../packs/nam-amp-modeler/Roman A YERASOV_MESHUGGAH.nam"),
    ),
    (
        "nam-amp-modeler",
        "Sascha S DirtyShirleyMini_Clean_B1_M6_T7_MV10_G4.nam",
        include_bytes!("../../../packs/nam-amp-modeler/Sascha S DirtyShirleyMini_Clean_B1_M6_T7_MV10_G4.nam"),
    ),
    (
        "nam-amp-modeler",
        "Sascha S DirtyShirleyMini_crunch_G6.nam",
        include_bytes!("../../../packs/nam-amp-modeler/Sascha S DirtyShirleyMini_crunch_G6.nam"),
    ),
    ("nam-amp-modeler", "Suhina Overdrive.nam", include_bytes!("../../../packs/nam-amp-modeler/Suhina Overdrive.nam")),
    ("nam-amp-modeler", "Suhina Clean.nam", include_bytes!("../../../packs/nam-amp-modeler/Suhina Clean.nam")),
    (
        "nam-amp-modeler",
        "Tim R Fender TwinVerb Vibrato Bright.nam",
        include_bytes!("../../../packs/nam-amp-modeler/Tim R Fender TwinVerb Vibrato Bright.nam"),
    ),
    (
        "nam-amp-modeler",
        "Tim R JCM2000 Crunch.nam",
        include_bytes!("../../../packs/nam-amp-modeler/Tim R JCM2000 Crunch.nam"),
    ),
    (
        "nam-amp-modeler",
        "Tim R Splawn Pro Mod 2nd Gear - G12.nam",
        include_bytes!("../../../packs/nam-amp-modeler/Tim R Splawn Pro Mod 2nd Gear - G12.nam"),
    ),
    ("nam-amp-modeler", "Tim R TS9.nam", include_bytes!("../../../packs/nam-amp-modeler/Tim R TS9.nam")),
    (
        "nam-amp-modeler",
        "Tudor N Driftwood Low Gain.nam",
        include_bytes!("../../../packs/nam-amp-modeler/Tudor N Driftwood Low Gain.nam"),
    ),
    ("nam-cab-loader", "Blackat Leon S7.wav", include_bytes!("../../../packs/nam-cab-loader/Blackat Leon S7.wav")),
    (
        "nam-cab-loader",
        "Cicognani Imperivm Luxury.wav",
        include_bytes!("../../../packs/nam-cab-loader/Cicognani Imperivm Luxury.wav"),
    ),
    ("nam-cab-loader", "DV Mark Triple 6.wav", include_bytes!("../../../packs/nam-cab-loader/DV Mark Triple 6.wav")),
    ("nam-cab-loader", "EVH 5150 III.wav", include_bytes!("../../../packs/nam-cab-loader/EVH 5150 III.wav")),
    ("nam-cab-loader", "Engl Retro Tube.wav", include_bytes!("../../../packs/nam-cab-loader/Engl Retro Tube.wav")),
    (
        "nam-cab-loader",
        "Engl Special Edition.wav",
        include_bytes!("../../../packs/nam-cab-loader/Engl Special Edition.wav"),
    ),
    ("nam-cab-loader", "Fortin Natas.wav", include_bytes!("../../../packs/nam-cab-loader/Fortin Natas.wav")),
    ("nam-cab-loader", "Kaos Sludge 15.wav", include_bytes!("../../../packs/nam-cab-loader/Kaos Sludge 15.wav")),
    (
        "nam-cab-loader",
        "Krank Krankenstein.wav",
        include_bytes!("../../../packs/nam-cab-loader/Krank Krankenstein.wav"),
    ),
    ("nam-cab-loader", "Laney ironheart.wav", include_bytes!("../../../packs/nam-cab-loader/Laney ironheart.wav")),
    (
        "nam-cab-loader",
        "MakosampCustomHatred.wav",
        include_bytes!("../../../packs/nam-cab-loader/MakosampCustomHatred.wav"),
    ),
    (
        "nam-cab-loader",
        "Marshall JMP 2203  Jose Arredondo mod.wav",
        include_bytes!("../../../packs/nam-cab-loader/Marshall JMP 2203  Jose Arredondo mod.wav"),
    ),
    ("nam-cab-loader", "Marshall MG 15.wav", include_bytes!("../../../packs/nam-cab-loader/Marshall MG 15.wav")),
    (
        "nam-cab-loader",
        "Mesa Boogie Mark V.wav",
        include_bytes!("../../../packs/nam-cab-loader/Mesa Boogie Mark V.wav"),
    ),
    ("nam-cab-loader", "Peavey Vypyr 15.wav", include_bytes!("../../../packs/nam-cab-loader/Peavey Vypyr 15.wav")),
    ("nam-cab-loader", "Randall Satan.wav", include_bytes!("../../../packs/nam-cab-loader/Randall Satan.wav")),
    ("nam-cab-loader", "Randall thrasher.wav", include_bytes!("../../../packs/nam-cab-loader/Randall thrasher.wav")),
    ("nam-cab-loader", "Splawn Nitro.wav", include_bytes!("../../../packs/nam-cab-loader/Splawn Nitro.wav")),
    ("nam-cab-loader", "Splawn Quick Rod.wav", include_bytes!("../../../packs/nam-cab-loader/Splawn Quick Rod.wav")),
    (
        "nam-cab-loader",
        "Synthetic Cabinet Bright.wav",
        include_bytes!("../../../packs/nam-cab-loader/Synthetic Cabinet Bright.wav"),
    ),
    (
        "nam-cab-loader",
        "Synthetic Cabinet Dark.wav",
        include_bytes!("../../../packs/nam-cab-loader/Synthetic Cabinet Dark.wav"),
    ),
    (
        "nam-cab-loader",
        "Synthetic Cabinet Off Axis.wav",
        include_bytes!("../../../packs/nam-cab-loader/Synthetic Cabinet Off Axis.wav"),
    ),
    (
        "nam-cab-loader",
        "Synthetic Cabinet Plain.wav",
        include_bytes!("../../../packs/nam-cab-loader/Synthetic Cabinet Plain.wav"),
    ),
    (
        "nam-cab-loader",
        "Synthetic Cabinet Room.wav",
        include_bytes!("../../../packs/nam-cab-loader/Synthetic Cabinet Room.wav"),
    ),
    (
        "nam-cab-loader",
        "Synthetic Cabinet Worse.wav",
        include_bytes!("../../../packs/nam-cab-loader/Synthetic Cabinet Worse.wav"),
    ),
    ("nam-cab-loader", "Taurus Stomphead.wav", include_bytes!("../../../packs/nam-cab-loader/Taurus Stomphead.wav")),
];

/// The shipped file in the pack's state key ([ADR 0036] · `builtin:<device id>/<file name>`).
pub fn builtin_file_key(id: &str, name: &str) -> String {
    format!("builtin:{id}/{name}")
}

/// The shipped pack's folder inside each device's library folder ([ADR 0036] › the library).
pub const SHIPPED_PACK: &str = "Suhina";

/// Every shipped file, flat: `(device id, library folder, pack folder, file name, bytes)`.
/// The device's own library folder and the pack's own folder inside it compose the unpack
/// path ([ADR 0036] › the library), so an installer or the app unpacks the pack without
/// naming device ids itself.
pub fn pack_files_flat() -> Vec<(&'static str, &'static str, &'static str, &'static str, &'static [u8])> {
    PACK_FILES
        .iter()
        .filter_map(|(device, name, bytes)| {
            file_kind(device).map(|file| (*device, file.library_folder, SHIPPED_PACK, *name, *bytes))
        })
        .collect()
}

/// A device's shipped files, `(name, bytes)`, the pack the panels list first ([ADR 0036]).
/// An empty list for a device that runs no file or carries none.
pub fn pack_files(id: &str) -> Vec<(&'static str, &'static [u8])> {
    PACK_FILES.iter().filter_map(|(device, name, bytes)| (*device == id).then_some((*name, *bytes))).collect()
}

/// Sizes every shipped file, all of them together (the ADR's ceiling, a test's bound).
pub fn pack_bytes() -> usize {
    PACK_FILES.iter().map(|(_, _, bytes)| bytes.len()).sum()
}

/// The pack's provenance and licence records, `(library folder, pack folder, file name,
/// bytes)`, written beside the installed files at unpack: the README that says where every
/// file came from and under what licence, and the licence texts the collected IRs were
/// published under — a redistribution keeps them beside the files ([ADR 0036] › 6). A record
/// the user already carries is never overwritten, like a pack file's.
pub fn pack_records() -> Vec<(&'static str, &'static str, &'static str, &'static [u8])> {
    vec![
        ("Amp models", SHIPPED_PACK, "README.md", include_bytes!("../../../packs/README.md")),
        ("Cabinets", SHIPPED_PACK, "README.md", include_bytes!("../../../packs/README.md")),
        (
            "Cabinets",
            SHIPPED_PACK,
            "LICENSE.BestPlugins-Amps.txt",
            include_bytes!("../../../packs/nam-cab-loader/LICENSE.BestPlugins-Amps.txt"),
        ),
        (
            "Cabinets",
            SHIPPED_PACK,
            "LICENSE.BestPlugins-Bands.txt",
            include_bytes!("../../../packs/nam-cab-loader/LICENSE.BestPlugins-Bands.txt"),
        ),
    ]
}

/// Runs the device `id` over the shipped file named `name`, for an engine at `rate` Hz — the
/// same readers, gates and failure messages as [`load_file`] gave a path, at the pack's bytes
/// ([ADR 0036]). Reads and allocates: call it off the audio thread.
pub fn load_builtin_file(id: &str, name: &str, rate: u32) -> Result<DeviceAsset, String> {
    let Some((entry, file)) = entry(id).and_then(|entry| Some((entry, entry.file.as_ref()?))) else {
        return Err(format!("{id:?} does not run a file"));
    };
    if !name.ends_with(&format!(".{}", file.extension)) {
        return Err(format!("a {id} runs a .{} file", file.extension));
    }
    let Some((_, bytes)) = pack_files(id).into_iter().find(|(file_name, _)| *file_name == name) else {
        return Err(format!("no shipped file {name:?} for {id:?}; pack_files lists them"));
    };
    Ok(DeviceAsset::new(&entry.info.id, (file.read_bytes)(bytes, rate)?))
}

/// A new device with this id, its defaults, and `asset` already loaded — `None` for an unknown
/// id. An asset another device read is ignored. Allocation: off the audio thread.
pub fn create_with(id: &str, asset: Option<&DeviceAsset>) -> Option<Box<dyn Device>> {
    let loaded = asset
        .filter(|asset| asset.device() == id)
        .and_then(|asset| file_kind(id).and_then(|file| (file.make)(asset.data())));
    loaded.or_else(|| create(id))
}

fn read_nam(path: &Path, rate: u32) -> Result<Arc<AssetData>, String> {
    let model = suhina_device_nam_amp_modeler::model::read(path)?;
    // The verdicts the panel can say why a file is silent: what the model's runtime will
    // not run, and a rate too far from the project's to resample. The device checks the
    // rate again at every prepare, so a project's rate changing later is as safe as this.
    if let Err(e) = nam_rs::Model::from_nam(&model) {
        return Err(format!("the model does not run: {e}"));
    }
    suhina_device_nam_amp_modeler::model::runnable_rate(f64::from(rate), model.expected_sample_rate())?;
    Ok(Arc::new(Arc::new(model)))
}

fn nam_name(_path: &Path, data: &AssetData) -> Option<String> {
    data.downcast_ref::<Arc<suhina_device_nam_amp_modeler::NamModel>>()
        .and_then(|model| suhina_device_nam_amp_modeler::model::display_name(model))
}

fn make_nam(data: &AssetData) -> Option<Box<dyn Device>> {
    let model = data.downcast_ref::<Arc<suhina_device_nam_amp_modeler::NamModel>>()?;
    Some(Box::new(suhina_device_nam_amp_modeler::NamAmp::with_model(Arc::clone(model))))
}

fn read_ir(path: &Path, rate: u32) -> Result<Arc<AssetData>, String> {
    Ok(Arc::new(suhina_device_nam_cab_loader::ir::read(path, rate)?))
}

fn read_nam_bytes(bytes: &[u8], rate: u32) -> Result<Arc<AssetData>, String> {
    let model = suhina_device_nam_amp_modeler::model::read_bytes(bytes)?;
    // The same gates as the path form: what the runtime will not run, and a rate the device
    // would refuse again at prepare. Read at command or project-load time, off the audio
    // thread.
    if let Err(e) = nam_rs::Model::from_nam(&model) {
        return Err(format!("the model does not run: {e}"));
    }
    suhina_device_nam_amp_modeler::model::runnable_rate(f64::from(rate), model.expected_sample_rate())?;
    Ok(Arc::new(Arc::new(model)))
}

fn read_ir_bytes(bytes: &[u8], rate: u32) -> Result<Arc<AssetData>, String> {
    Ok(Arc::new(suhina_device_nam_cab_loader::ir::read_bytes(bytes, rate)?))
}

fn ir_name(path: &Path, _data: &AssetData) -> Option<String> {
    // The file's own name without its extension, never the folder's: the pack row already
    // reads the folder, and a file whose name shares its folder's made the two pickers look
    // alike (~issue 1289).
    path.file_stem().map(|stem| stem.to_string_lossy().into_owned())
}

fn make_cab(data: &AssetData) -> Option<Box<dyn Device>> {
    let ir = data.downcast_ref::<suhina_dsp::Ir>()?;
    Some(Box::new(suhina_device_nam_cab_loader::CabLoader::with_ir(ir.clone())))
}

/// Largest preset file read.
pub const MAX_PRESET_BYTES: usize = 64 * 1024;

/// A device's values from a preset file (`docs/spec/browser.md` › Built-in effects and instruments, presets
/// and plug-ins).
#[derive(Debug, Clone, PartialEq)]
pub struct Preset {
    /// The device it is for.
    pub device: &'static str,
    /// The preset's own name (a DX7 voice's name).
    pub name: String,
    /// Every parameter by its id, with the preset's value.
    pub params: Vec<(&'static str, f64)>,
    /// The file the preset's device selects, for a device that runs one: a shipped file's
    /// `builtin:<device id>/<file name>` key, or the file's path as the project carries it
    /// (`device.setNamFile`; the library-relative one is portable, an absolute one opens where
    /// the file sits). Left out for a device that runs none.
    pub file: Option<String>,
}

/// A preset that ships with Suhina (ADR 0014): a file of `presets/` built into the executable.
#[derive(Debug, Clone, PartialEq)]
pub struct StockPreset {
    /// What names it in a browser item, `stock:<key>`, and in `device.add`: `<device id>/<name>`. It
    /// stays when the shown name changes, as pins and recent items keep it.
    pub key: &'static str,
    pub tags: Vec<String>,
    pub preset: Preset,
}

/// Every stock preset's key and file.
const STOCK_FILES: &[(&str, &str)] = &[
    ("fm-synth/warm-fm-bass", include_str!("../../../presets/fm-synth/Warm FM Bass.fm-synth.preset.suhina")),
    ("fm-synth/drawbar-organ", include_str!("../../../presets/fm-synth/Drawbar Organ.fm-synth.preset.suhina")),
    ("fm-synth/struck-fm-keys", include_str!("../../../presets/fm-synth/Struck FM Keys.fm-synth.preset.suhina")),
    ("fm-synth/wide-trance-pad", include_str!("../../../presets/fm-synth/Wide Trance Pad.fm-synth.preset.suhina")),
    ("fm-synth/slow-warm-pad", include_str!("../../../presets/fm-synth/Slow Warm Pad.fm-synth.preset.suhina")),
    (
        "nam-amp-modeler/bright-clean",
        include_str!("../../../presets/nam-amp-modeler/Bright Clean.nam-amp-modeler.preset.suhina"),
    ),
    (
        "nam-amp-modeler/tight-high-gain",
        include_str!("../../../presets/nam-amp-modeler/Tight High Gain.nam-amp-modeler.preset.suhina"),
    ),
    (
        "nam-amp-modeler/warm-crunch",
        include_str!("../../../presets/nam-amp-modeler/Warm Crunch.nam-amp-modeler.preset.suhina"),
    ),
    (
        "nam-amp-modeler/dark-lead",
        include_str!("../../../presets/nam-amp-modeler/Dark Lead.nam-amp-modeler.preset.suhina"),
    ),
    (
        "nam-amp-modeler/clean-boost",
        include_str!("../../../presets/nam-amp-modeler/Clean Boost.nam-amp-modeler.preset.suhina"),
    ),
    (
        "nam-cab-loader/rumble-cut",
        include_str!("../../../presets/nam-cab-loader/Rumble Cut.nam-cab-loader.preset.suhina"),
    ),
    (
        "nam-cab-loader/dark-cabinet",
        include_str!("../../../presets/nam-cab-loader/Dark Cabinet.nam-cab-loader.preset.suhina"),
    ),
    (
        "nam-cab-loader/bright-cabinet",
        include_str!("../../../presets/nam-cab-loader/Bright Cabinet.nam-cab-loader.preset.suhina"),
    ),
    ("nam-cab-loader/mid-dip", include_str!("../../../presets/nam-cab-loader/Mid Dip.nam-cab-loader.preset.suhina")),
    (
        "nam-cab-loader/second-cab",
        include_str!("../../../presets/nam-cab-loader/Second Cab.nam-cab-loader.preset.suhina"),
    ),
];

/// The stock presets, read once. A file that does not read is left out; a test keeps there being none.
pub fn stock_presets() -> &'static [StockPreset] {
    static STOCK: OnceLock<Vec<StockPreset>> = OnceLock::new();
    STOCK.get_or_init(|| {
        STOCK_FILES
            .iter()
            .filter_map(|&(key, text)| {
                let (preset, tags) = read_preset_file(text.as_bytes()).ok()?;
                Some(StockPreset { key, tags, preset })
            })
            .collect()
    })
}

/// The stock preset with this key.
pub fn stock_preset(key: &str) -> Option<&'static StockPreset> {
    stock_presets().iter().find(|stock| stock.key == key)
}

/// A device preset's file (ADR 0014) as a preset and its tags. A parameter it does not name takes
/// its default, and one the device no longer has is left out.
fn read_preset_file(data: &[u8]) -> Result<(Preset, Vec<String>), String> {
    let file: serde_json::Value = serde_json::from_slice(data).map_err(|e| format!("not a preset file: {e}"))?;
    if file["format"] != "suhina-preset" {
        return Err("not a preset file".into());
    }
    if file["version"] != 1 {
        return Err("a preset file of a version this Suhina does not know".into());
    }
    let id = file["device"].as_str().ok_or("a preset file names its device")?;
    let info = info(id).ok_or_else(|| format!("a preset for {id:?}, which is not a built-in device"))?;
    let role = match info.role {
        suhina_device_api::Role::Effect => "effect",
        suhina_device_api::Role::MidiEffect => "midi_effect",
        suhina_device_api::Role::Instrument => "instrument",
    };
    if file["role"] != role {
        return Err(format!("{id:?} is an {role}, and the preset says otherwise"));
    }
    let name = file["name"].as_str().map(str::trim).filter(|n| !n.is_empty()).ok_or("a preset file has a name")?;
    let values = file["params"].as_object().ok_or("a preset file has its parameters")?;
    let mut params = Vec::with_capacity(info.params.len());
    for param in info.params.iter() {
        let value = match values.get(&*param.id) {
            None => param.default,
            Some(value) => value.as_f64().ok_or_else(|| format!("{} is not a number", param.id))?,
        };
        if !(param.min..=param.max).contains(&value) {
            return Err(format!("{} is {value}, outside {} to {}", param.id, param.min, param.max));
        }
        params.push((&*param.id, value));
    }
    let tags = file["tags"].as_array().into_iter().flatten().filter_map(|t| t.as_str().map(String::from)).collect();
    let file = match &file["file"] {
        serde_json::Value::Null => None,
        serde_json::Value::String(text) => Some(text.trim()).filter(|f| !f.is_empty()).map(str::to_owned),
        _ => return Err("a preset file's file is text (the file its device runs)".into()),
    };
    if file.is_some() && !is_file_device(&info.id) {
        return Err(format!("the preset names a file, which {} does not run", info.name));
    }
    // A shipped file's key is the preset's own device's, refused at the read as at the write
    // ([ADR 0014] › Amendment: a preset may name a file); the loader would refuse it later,
    // but then the device is added without its file instead of the preset being refused here.
    if let Some((packed_id, _)) =
        file.as_deref().and_then(|f| f.strip_prefix("builtin:")).and_then(|rest| rest.split_once('/'))
        && packed_id != info.id.as_ref()
    {
        return Err(format!("{} runs a shipped file of its own kind, not {packed_id:?}", info.name));
    }
    Ok((Preset { device: &info.id, name: name.to_string(), params, file }, tags))
}

/// Writes `device`'s values as a preset file's JSON: one name, the file its device selects (for a
/// device that runs one, [`ADR 0036`] › **A preset may name a file**) and every declared
/// parameter with the given value. The tags come from the device's presets alone; a saved preset
/// carries none. [`ADR 0014`] names the shape; `device.savePreset` calls this.
pub fn write_preset_file(
    device: &str,
    name: &str,
    file: Option<&str>,
    params: &[(&str, f64)],
) -> Result<Vec<u8>, String> {
    let info =
        info(device).ok_or_else(|| format!("no built-in effect or instrument {device:?}; device.list lists them"))?;
    let name = name.trim();
    if name.is_empty() {
        return Err("a preset has a name".into());
    }
    if name.contains(['/', '\\', ':']) || name.trim_matches('.') != name || name.chars().any(char::is_control) {
        return Err(format!("{name:?} is not a preset file's name"));
    }
    // A shipped file's key is the running device's own (load_builtin_file refuses another), and
    // a device that runs no file names none at all. Where the file is the user's own path, the
    // writer leaves it as it is: the state already carries it.
    if let Some(file) = file.map(str::trim).filter(|file| !file.is_empty()) {
        if !is_file_device(info.id.as_ref()) {
            return Err(format!("{} runs no file, so the preset names none", info.name));
        }
        if let Some((packed_id, _)) = file.strip_prefix("builtin:").and_then(|rest| rest.split_once('/'))
            && packed_id != info.id.as_ref()
        {
            return Err(format!("{} runs a shipped file of its own kind, not {packed_id:?}", info.name));
        }
    }
    let mut written = serde_json::Map::new();
    written.insert("format".into(), serde_json::Value::from("suhina-preset"));
    written.insert("version".into(), serde_json::Value::from(1));
    written.insert("device".into(), serde_json::Value::from(info.id.clone()));
    written.insert(
        "role".into(),
        serde_json::Value::from(match info.role {
            suhina_device_api::Role::Effect => "effect",
            suhina_device_api::Role::MidiEffect => "midi_effect",
            suhina_device_api::Role::Instrument => "instrument",
        }),
    );
    written.insert("name".into(), serde_json::Value::from(name));
    if let Some(file) = file.map(str::trim).filter(|file| !file.is_empty()) {
        written.insert("file".into(), serde_json::Value::from(file));
    }
    let mut values = serde_json::Map::new();
    for param in info.params.iter() {
        let value = params.iter().find(|(id, _)| *id == param.id).map_or(param.default, |(_, value)| *value);
        if !(param.min..=param.max).contains(&value) {
            return Err(format!("{} is {value}, outside {} to {}", param.id, param.min, param.max));
        }
        values.insert((*param.id).into(), serde_json::Value::from(value));
    }
    written.insert("params".into(), serde_json::Value::Object(values));
    let mut bytes =
        serde_json::to_vec_pretty(&serde_json::Value::Object(written)).map_err(|e| format!("not written: {e}"))?;
    bytes.push(b'\n');
    if bytes.len() > MAX_PRESET_BYTES {
        return Err(format!("a preset file is at most {} KiB", MAX_PRESET_BYTES / 1024));
    }
    Ok(bytes)
}

/// Whether a preset file's data is a device preset's JSON rather than DX7 voice data.
fn is_preset_json(data: &[u8]) -> bool {
    data.trim_ascii_start().first() == Some(&b'{')
}

/// The voices a preset file holds: one for a device preset or a DX7 single voice, 32 for a DX7 bank.
/// Each is `(number from 1, name)`.
pub fn preset_voices(data: &[u8]) -> Result<Vec<(usize, String)>, String> {
    if is_preset_json(data) {
        check_preset_size(data)?;
        return Ok(vec![(1, read_preset_file(data)?.0.name)]);
    }
    let voices = read_voices(data)?;
    Ok(voices.patches.iter().enumerate().map(|(i, p)| (i + 1, p.name().trim().to_string())).collect())
}

/// The preset `voice` (from 1) of a preset file: a device preset (its only voice), or DX7 voice data
/// for the FM synth.
pub fn preset(data: &[u8], voice: usize) -> Result<Preset, String> {
    if is_preset_json(data) {
        check_preset_size(data)?;
        if voice != 1 {
            return Err(format!("voice {voice} is not in the file; it holds 1"));
        }
        return Ok(read_preset_file(data)?.0);
    }
    let voices = read_voices(data)?;
    let count = voices.patches.len();
    let patch = voice
        .checked_sub(1)
        .and_then(|i| voices.patches.get(i))
        .ok_or_else(|| format!("voice {voice} is not in the file; it holds {count}"))?;
    let mut synth = suhina_device_fm_synth::FmSynth::new();
    synth.load_patch(patch);
    let info = &suhina_device_fm_synth::INFO;
    let params =
        info.params.iter().enumerate().filter_map(|(i, p)| synth.param(i).map(|value| (&*p.id, value))).collect();
    Ok(Preset { device: &info.id, name: patch.name().trim().to_string(), params, file: None })
}

fn check_preset_size(data: &[u8]) -> Result<(), String> {
    if data.len() > MAX_PRESET_BYTES {
        return Err(format!("a preset file is at most {} KiB", MAX_PRESET_BYTES / 1024));
    }
    Ok(())
}

fn read_voices(data: &[u8]) -> Result<suhina_device_fm_synth::patch::Voices, String> {
    check_preset_size(data)?;
    suhina_device_fm_synth::patch::read(data).map_err(|e| e.to_string())
}

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;

/// Prepare a data-only graph using only registered, zero-latency built-in effects.
/// Unknown ids fail before the graph reaches the audio thread.
pub fn compile_graph(
    graph: &suhina_device_sdk::DeviceGraph,
    rate: f64,
) -> Result<suhina_device_sdk::PreparedEffect, suhina_device_sdk::DeviceSdkError> {
    compile_graph_with_samples(graph, rate, &Default::default())
}

/// A `.sd` graph as a device like any built-in (#763 › 6), its native nodes made from this
/// registry and its sample nodes reading `samples`. Off the audio thread.
pub fn graph_device(
    graph: &suhina_device_sdk::DeviceGraph,
    rate: f64,
    samples: suhina_device_sdk::sample_resource::PreparedSamples,
) -> Result<suhina_device_sdk::GraphDevice, suhina_device_sdk::DeviceSdkError> {
    if graph.role == suhina_device_sdk::DeviceRole::MidiEffect {
        return suhina_device_sdk::GraphDevice::midi(graph);
    }
    let compiled = graph.clone();
    let compile = move |rate| compile_graph_with_samples(&compiled, rate, &samples);
    suhina_device_sdk::GraphDevice::audio(graph, rate, Box::new(compile))
}

/// Prepare graph resources supplied by the host; decoding and retirement stay off-thread.
pub fn compile_graph_with_samples(
    graph: &suhina_device_sdk::DeviceGraph,
    rate: f64,
    samples: &suhina_device_sdk::sample_resource::PreparedSamples,
) -> Result<suhina_device_sdk::PreparedEffect, suhina_device_sdk::DeviceSdkError> {
    graph.compile_with_samples_and_native_at(rate, samples, |id, rate| {
        let mut device =
            create(id).ok_or_else(|| suhina_device_sdk::DeviceSdkError(format!("unknown graph effect {id:?}")))?;
        if device.info().role != suhina_device_api::Role::Effect {
            return Err(suhina_device_sdk::DeviceSdkError(format!("graph kernel {id:?} is not an audio effect")));
        }
        device.prepare(rate).map_err(|e| suhina_device_sdk::DeviceSdkError(e.to_string()))?;
        if device.latency() != 0 {
            return Err(suhina_device_sdk::DeviceSdkError(
                "graph kernels with latency require path compensation".into(),
            ));
        }
        Ok(Box::new(GraphEffect { device, context: Default::default(), rate }))
    })
}

struct GraphEffect {
    device: Box<dyn Device>,
    context: suhina_device_api::ProcessContext,
    rate: f64,
}
impl suhina_device_sdk::native_node::NativeNode for GraphEffect {
    fn prepare_block(&mut self, context: &suhina_device_api::ProcessContext) {
        self.context = *context;
        self.device.set_context(context);
    }
    fn process(&mut self, inputs: [&[f32]; 5], output: &mut [f32]) {
        output.copy_from_slice(inputs[0]);
        self.device.process(&self.context, output);
        if self.context.playing {
            self.context.beat += (output.len() / 2) as f64 * self.context.tempo / (60.0 * self.rate);
        }
    }
    fn connected_inputs(&mut self, mask: [bool; 5]) -> Result<(), suhina_device_sdk::DeviceSdkError> {
        if mask[1..].iter().any(|&v| v) {
            return Err(suhina_device_sdk::DeviceSdkError(
                "registered effect control bindings are not implemented".into(),
            ));
        }
        Ok(())
    }
    fn parameter_range(&self, id: &str) -> Option<(f64, f64)> {
        self.device.info().params.iter().find(|p| p.id == id).map(|p| (p.min, p.max))
    }
    fn set_parameter(&mut self, id: &str, value: f64) {
        if let Some(index) = self.device.info().param_index(id) {
            self.device.set_param(index, value);
        }
    }
    fn event(&mut self, event: suhina_device_api::NoteEvent) {
        self.device.event(event);
    }
    fn reset(&mut self) {
        self.device.reset();
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod graph_kernel_tests {
    use super::*;
    use suhina_device_sdk::{Connection, DeviceGraph, DeviceRole, Node, NodeKind};
    #[test]
    fn stored_graph_resolves_registered_effect_and_rejects_unknown() {
        let mut graph = DeviceGraph::new("org.test.registry", "Registry", DeviceRole::Effect);
        graph.nodes = vec![
            Node::audio_input("in"),
            Node::new("fx", NodeKind::NativeEffect { effect: "utility".into() }),
            Node::audio_output("out"),
        ];
        graph.connections = vec![Connection::new("in", "fx"), Connection::new("fx", "out")];
        let graph = DeviceGraph::from_sd(&graph.to_sd().unwrap()).unwrap();
        let mut prepared = compile_graph(&graph, 48000.).unwrap();
        let mut audio = [0.25, -0.5, 0.125, 0.75];
        prepared.process(&mut audio);
        assert!(audio.iter().all(|v| v.is_finite()));
        assert!(audio.iter().any(|v| v.abs() > 0.01));
        let mut controlled = graph.clone();
        controlled.connections.push(Connection::ports("in", "out", "fx", "control1"));
        assert!(compile_graph(&controlled, 48000.).is_err());
        let mut nested = DeviceGraph::new("org.test.nested", "Nested", DeviceRole::Effect);
        nested.nodes = vec![
            Node::audio_input("in"),
            Node::new(
                "child",
                NodeKind::Component { name: "Hidden effect".into(), version: 1, graph: controlled.to_sd().unwrap() },
            ),
            Node::audio_output("out"),
        ];
        nested.connections = vec![Connection::new("in", "child"), Connection::new("child", "out")];
        assert!(compile_graph(&nested, 48000.).is_err());

        for id in ["missing", "sampler", "limiter"] {
            let mut invalid = graph.clone();
            invalid.nodes[1].kind = NodeKind::NativeEffect { effect: id.into() };
            assert!(compile_graph(&invalid, 48000.).is_err(), "{id}");
        }
    }
}
