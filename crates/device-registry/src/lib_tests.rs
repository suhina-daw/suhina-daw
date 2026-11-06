// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

#[test]
fn the_fm_synths_panel_shows_every_algorithm_and_draws_its_partials() {
    use suhina_device_sdk::ui::UiGesture;
    let panel = panel("fm-synth").unwrap();
    let (compact, expanded) = (panel.ui.as_ref().unwrap(), panel.expanded.as_ref().unwrap());
    let count = |canvas: &suhina_device_sdk::ui::DeviceUi, prefix: &str| {
        canvas.controls.iter().filter(|c| c.id.starts_with(prefix)).count()
    };
    assert_eq!(count(compact, "algorithm_picture_"), 43);
    assert_eq!(count(expanded, "x_pick_"), 43);
    for canvas in [compact, expanded] {
        let bars = canvas.controls.iter().filter(|c| matches!(c.gesture, UiGesture::Draw(_))).count();
        assert_eq!(bars, 6 * (16 + 32 + 64));
    }
    assert!(compact.controls.iter().any(|c| matches!(c.gesture, UiGesture::Expand(8))), "Random's way to its settings");
    // The larger view is a strip above the row, which stays in view, and is no higher than it needs.
    assert!(expanded.strip);
    assert!(compact.size[0] < 1100.0 && expanded.size[1] < 240.0);
    // The LFO's and the filter's main controls stay in view: nothing makes them follow the page.
    for id in ["filterFreq_main", "filterRes_main", "filterType_main", "lfo2Wave_main", "lfo2Amount_main"] {
        let control = compact.controls.iter().find(|c| c.id == id).unwrap();
        assert!(control.visible_when.is_none(), "{id}");
    }
    // A control that changes a value says what it is for, not how a mouse moves it.
    for canvas in [compact, expanded] {
        for control in canvas.controls.iter().filter(|c| !matches!(c.gesture, UiGesture::Draw(_))) {
            assert!(!control.tooltip.contains("drag"), "{}: {}", control.id, control.tooltip);
            if control.y.is_some() && matches!(control.gesture, UiGesture::Drag) {
                assert!(control.tooltip.len() > control.name.len(), "{} says nothing", control.id);
            }
        }
    }
    // An operator's square in the picture selects it, in every algorithm and on both canvases.
    for canvas in [compact, expanded] {
        let squares = canvas.controls.iter().filter(|c| c.id.starts_with("pick_op")).count();
        assert_eq!(squares, 43 * 6);
    }
    // A card on the Algorithm page takes the click anywhere on it, its picture included.
    let card = expanded.controls.iter().find(|c| c.id == "x_pick_5").unwrap();
    assert!(matches!(card.gesture, UiGesture::Set(5)));
    assert!(card.bounds[3] > 40.0, "the whole card, not its number");
    let drawn = suhina_device_fm_synth::params::WAVES.iter().position(|w| *w == "Drawn");
    let wanted = compact.controls.iter().find_map(|c| match c.gesture {
        UiGesture::Draw(wave) => Some(usize::from(wave)),
        _ => None,
    });
    assert_eq!(wanted, drawn, "drawing asks for the Drawn wave");
}

#[test]
fn a_dx7_voice_is_a_preset_for_the_fm_synth() {
    use suhina_device_fm_synth::patch::{Patch, write_bank, write_voice};
    let mut patch = Patch::default();
    patch.set_name("GLASS BELL");
    patch.set(134, 21); // algorithm 22
    let single = write_voice(&patch);
    let preset = preset(&single, 1).unwrap();
    assert_eq!((preset.device, preset.name.as_str()), ("fm-synth", "GLASS BELL"));
    assert_eq!(preset.params.len(), suhina_device_fm_synth::INFO.params.len());
    let mut synth = suhina_device_fm_synth::FmSynth::new();
    synth.load_patch(&patch);
    for (i, (id, value)) in preset.params.iter().enumerate() {
        assert_eq!(Some(*value), synth.param(i), "{id}");
    }
    assert_eq!(preset_voices(&single).unwrap(), vec![(1, "GLASS BELL".to_string())]);

    let bank = write_bank(&vec![patch; 32]);
    assert_eq!(preset_voices(&bank).unwrap().len(), 32);
    assert_eq!(super::preset(&bank, 32).unwrap().name, "GLASS BELL");
    assert!(super::preset(&bank, 33).is_err());
    assert!(super::preset(&bank, 0).is_err());
    assert!(super::preset(b"not a preset", 1).is_err());
    assert!(super::preset(&vec![0u8; MAX_PRESET_BYTES + 1], 1).is_err());
}

#[test]
fn every_stock_preset_reads_and_gives_every_parameter_of_its_device() {
    assert_eq!(stock_presets().len(), STOCK_FILES.len(), "a file does not read");
    for (key, text) in STOCK_FILES {
        let file: serde_json::Value = serde_json::from_str(text).unwrap();
        let stock = stock_preset(key).unwrap();
        let device = info(stock.preset.device).unwrap();
        assert_eq!(key.split('/').next(), Some(&*device.id), "{key} lies in its device's folder");
        assert!(text.len() <= MAX_PRESET_BYTES, "{key}");
        let values = file["params"].as_object().unwrap();
        assert_eq!(values.len(), device.params.len(), "{key}: every parameter, and no other");
        for (param, (id, value)) in device.params.iter().zip(&stock.preset.params) {
            assert_eq!(param.id, *id);
            assert_eq!(values[*id].as_f64(), Some(*value), "{key}: {id}");
        }
        assert!(!stock.tags.is_empty(), "{key}");
    }
}

#[test]
fn every_preset_file_in_the_folder_is_built_in() {
    let folder = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../presets");
    let mut files = 0;
    for device in std::fs::read_dir(folder).unwrap().flatten().filter(|e| e.path().is_dir()) {
        let device_id = device.file_name().to_string_lossy().into_owned();
        for file in std::fs::read_dir(device.path()).unwrap().flatten() {
            let name = file.file_name().to_string_lossy().into_owned();
            assert!(name.ends_with(&format!(".{device_id}.preset.suhina")), "{name}");
            let stem = name.split('.').next().unwrap().to_string();
            assert!(
                stock_presets().iter().any(|s| s.preset.name == stem),
                "{name} is not in STOCK_FILES, or has another name inside"
            );
            files += 1;
        }
    }
    assert_eq!(files, STOCK_FILES.len());
}

#[test]
fn a_device_preset_file_is_read_and_a_bad_one_refused() {
    let file = |rest: &str| {
        format!(
            r#"{{ "format": "suhina-preset", "version": 1, "device": "fm-synth", "role": "instrument", "name": "Mine", {rest} }}"#
        )
    };
    let good = file(r#""params": { "volume": -3.0, "gone": 1.0 }"#);
    let read = preset(good.as_bytes(), 1).unwrap();
    assert_eq!((read.device, read.name.as_str()), ("fm-synth", "Mine"));
    let value = |id: &str| read.params.iter().find(|(p, _)| *p == id).unwrap().1;
    assert_eq!(value("volume"), -3.0);
    assert_eq!(value("voices"), 16.0, "what it leaves out takes its default");
    assert!(read.params.iter().all(|(id, _)| *id != "gone"), "what the device lacks is left out");
    assert_eq!(preset_voices(good.as_bytes()).unwrap(), vec![(1, "Mine".to_string())]);
    assert!(preset(good.as_bytes(), 2).is_err());
    for bad in [
        file(r#""params": { "volume": 99.0 }"#),
        file(r#""params": { "volume": "loud" }"#),
        file(r#""nothing": 1"#),
        good.replace("suhina-preset", "other"),
        good.replace(r#""version": 1"#, r#""version": 2"#),
        good.replace(r#""fm-synth""#, r#""nothing""#),
        good.replace(r#""instrument""#, r#""effect""#),
        good.replace(r#""Mine""#, r#""  ""#),
        "{ not json".to_string(),
    ] {
        assert!(preset(bad.as_bytes(), 1).is_err(), "{bad}");
    }
    let mut large = good.into_bytes();
    large.resize(MAX_PRESET_BYTES + 1, b' ');
    assert!(preset(&large, 1).is_err());
}

#[test]
fn every_layout_loads_and_names_only_its_devices_parameters() {
    // A layout that names a parameter id its device lacks fails here, rather than falling
    // back to the generated list unnoticed.
    for info in DEVICES {
        if let Some(layout) = layout(&info.id) {
            let panel = layout.and_then(|l| l.panel(info)).unwrap_or_else(|e| panic!("{}: {e}", info.id));
            assert_eq!(panel_ids(&panel), info.params.iter().map(|p| p.id.clone()).collect::<Vec<_>>());
        }
    }
    for (id, _) in layout_files() {
        assert!(super::info(id).is_some(), "panel.json for unknown device {id}");
    }
    assert!(layout("sampler").is_some(), "the sampler has its own layout");
}

#[test]
fn the_compressors_knobs_fill_the_compact_panel_without_scrolling() {
    use suhina_device_sdk::ParameterControl::{Knob, LogKnob};
    let panel = panel("compressor").unwrap();
    for p in &panel.parameters {
        assert!(matches!(p.control, Knob | LogKnob), "{} is not a knob", p.id);
        let ([x, y], [w, h]) = (p.ui.position.unwrap(), p.ui.size.unwrap());
        assert!(x + w <= builtin::COMPACT_WIDTH && y + h <= builtin::COMPACT_HEIGHT, "{} is outside", p.id);
    }
}

#[test]
fn the_overdrives_canvas_holds_every_parameter_in_the_compact_panel() {
    let canvas = panel("overdrive").unwrap().ui.clone().unwrap();
    assert_eq!(canvas.size, [builtin::COMPACT_WIDTH, builtin::COMPACT_HEIGHT]);
    // Every parameter is on the canvas, so the generated list shows none of them twice.
    assert_eq!(canvas.hidden_parameters.len(), suhina_device_overdrive::INFO.params.len());
    for c in &canvas.controls {
        let [x, y, w, h] = c.bounds;
        assert!(x + w <= canvas.size[0] && y + h <= canvas.size[1], "{} is outside", c.id);
    }
    let band = canvas.controls.iter().find(|c| c.id == "band").unwrap();
    let axes = (band.x.as_ref().map(|b| b.parameter.as_str()), band.y.as_ref().map(|b| b.parameter.as_str()));
    assert_eq!(axes, (Some("bandFrequency"), Some("bandWidth")));
}

#[test]
fn the_reverbs_expanded_panel_has_every_parameter_and_the_compact_one_fits() {
    let panel = panel("reverb").unwrap();
    let compact = panel.ui.as_ref().unwrap();
    assert_eq!(compact.size, [builtin::COMPACT_WIDTH, builtin::COMPACT_HEIGHT]);
    for c in &compact.controls {
        let [x, y, w, h] = c.bounds;
        assert!(x + w <= compact.size[0] && y + h <= compact.size[1], "{} is outside", c.id);
    }
    // The compact panel shows the main ones; the rest are reachable only in the expanded one.
    let mut bound: Vec<&str> = (panel.expanded.as_ref().unwrap().controls.iter())
        .flat_map(|c| [c.x.as_ref(), c.y.as_ref()])
        .flatten()
        .map(|b| b.parameter.as_str())
        .collect();
    bound.sort_unstable();
    bound.dedup();
    assert_eq!(bound.len(), suhina_device_reverb::INFO.params.len());
}

#[test]
fn every_device_has_a_panel_with_its_parameters() {
    for info in DEVICES {
        let panel = panel(&info.id).unwrap_or_else(|| panic!("{} has no panel", info.id));
        assert_eq!(panel_ids(panel), info.params.iter().map(|p| &*p.id).collect::<Vec<_>>(), "{}", info.id);
        for (parameter, native) in panel.parameters.iter().zip(info.params.iter()) {
            assert_eq!((parameter.min, parameter.max, parameter.default), (native.min, native.max, native.default));
        }
    }
    // A device without a layout gets the generated list; MIDI effects get a panel too.
    assert!(panel("utility").is_some_and(|p| p.ui.is_none()));
    assert!(panel("pitch").is_some_and(|p| p.ui.is_some()));
    assert!(panel("sampler").is_some_and(|p| p.ui.is_some()));
    assert!(panel("no-such-device").is_none());
}

#[test]
fn every_json_layout_fits_its_canvases_and_reaches_every_parameter() {
    for (id, _) in layout_files() {
        let panel = panel(id).unwrap();
        if let Some(compact) = &panel.ui {
            assert!(compact.size[1] <= builtin::COMPACT_HEIGHT, "{id}: the compact canvas scrolls");
        }
        let canvases: Vec<_> = [&panel.ui, &panel.expanded].into_iter().flatten().collect();
        for canvas in &canvases {
            for c in &canvas.controls {
                let [x, y, w, h] = c.bounds;
                assert!(x + w <= canvas.size[0] && y + h <= canvas.size[1], "{id}: {} is outside", c.id);
            }
        }
        // A parameter is reachable on a canvas control or, when the compact canvas does not
        // hide it, in the generated list under it.
        let hidden = panel.ui.as_ref().map_or(&[][..], |c| &c.hidden_parameters[..]);
        for p in &panel.parameters {
            let bound = canvases
                .iter()
                .flat_map(|c| &c.controls)
                .any(|c| [&c.x, &c.y].into_iter().flatten().any(|b| b.parameter == p.id));
            assert!(bound || !hidden.contains(&p.id), "{id}: {} cannot be reached", p.id);
        }
    }
}

fn panel_ids(panel: &Panel) -> Vec<&str> {
    panel.parameters.iter().map(|p| p.id.as_str()).collect()
}

#[test]
fn every_listed_device_can_be_made_and_ids_are_unique() {
    for (i, info) in DEVICES.iter().enumerate() {
        let device = create(&info.id).unwrap_or_else(|| panic!("{} cannot be made", info.id));
        assert_eq!(device.info(), *info);
        assert_eq!(super::info(&info.id), Some(*info));
        assert!(DEVICES.iter().skip(i + 1).all(|other| other.id != info.id), "{} twice", info.id);
    }
    assert!(create("no-such-device").is_none());
    assert!(info("").is_none());
}

#[test]
fn only_the_nam_devices_run_a_file_and_every_row_is_found_by_its_id() {
    let files: Vec<&str> = ENTRIES.iter().filter(|e| e.file.is_some()).map(|e| &*e.info.id).collect();
    assert_eq!(files, ["nam-amp-modeler", "nam-cab-loader"]);
    for (entry, info) in ENTRIES.iter().zip(DEVICES) {
        assert_eq!(entry.info, info);
        assert_eq!(is_file_device(&info.id), entry.file.is_some());
    }
    assert!(!is_file_device("no-such-device"));
    assert_eq!(file_extension("nam-amp-modeler"), Some("nam"));
    assert_eq!(file_extension("nam-cab-loader"), Some("wav"));
    assert_eq!(file_extension("compressor"), None);
}

#[test]
fn a_file_is_refused_before_it_is_read_when_the_device_runs_no_file_or_another_kind() {
    let path = std::path::Path::new("/no/such/folder/model.wav");
    assert_eq!(load_file("compressor", path, 48_000).unwrap_err(), "\"compressor\" does not run a file");
    assert_eq!(load_file("nam-amp-modeler", path, 48_000).unwrap_err(), "a nam-amp-modeler runs a .nam file");
    let unc = std::path::Path::new(r"\\server\share\model.txt");
    assert_eq!(load_file("nam-cab-loader", unc, 48_000).unwrap_err(), "a nam-cab-loader runs a .wav file");
    // The right extension is read, and a missing file says so.
    assert!(load_file("nam-cab-loader", path, 48_000).is_err());
}

#[test]
fn a_device_ignores_an_asset_another_device_read() {
    let dir = std::env::temp_dir().join(format!("suhina-registry-asset-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let wav = dir.join("ir.wav");
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 48_000,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut writer = hound::WavWriter::create(&wav, spec).unwrap();
    for s in [1.0f32, 0.5, 0.25] {
        writer.write_sample(s).unwrap();
    }
    writer.finalize().unwrap();
    let asset = load_file("nam-cab-loader", &wav, 48_000).unwrap();
    assert_eq!(asset.device(), "nam-cab-loader");
    assert_eq!(asset_name(&asset, &wav), dir.file_name().map(|n| n.to_string_lossy().into_owned()));
    assert!(create_with("nam-cab-loader", Some(&asset)).is_some());
    // The Amp Modeler is given the IR: it starts at its defaults, as with no file.
    let amp = create_with("nam-amp-modeler", Some(&asset)).unwrap();
    assert_eq!(amp.info().id, "nam-amp-modeler");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn every_layout_loads_so_none_is_reported() {
    assert_eq!(layout_errors(), Vec::<(&str, String)>::new());
}

#[test]
fn the_multiband_panel_switches_bands_and_opens_an_expanded_spectrum() {
    let panel = panel("multiband").unwrap();
    let compact = panel.ui.as_ref().unwrap();
    // One tab per band sets the shown band; each band's knobs show only for it.
    let tabs = compact.controls.iter().filter(|c| c.x.as_ref().is_some_and(|b| b.parameter == "band")).count();
    assert_eq!(tabs, 3);
    for band in ["low", "mid", "high"] {
        let knob = compact.controls.iter().find(|c| c.id == format!("{band}Attack")).unwrap();
        assert!(knob.visible_when.is_some(), "{band}: its knobs show for its tab only");
    }
    assert!(panel.expanded.as_ref().is_some_and(|e| e.size[0] > compact.size[0]));
}
