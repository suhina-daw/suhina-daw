// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

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
fn every_layout_loads_and_names_only_its_devices_parameters() {
    // A layout that names a parameter id its device lacks fails here, rather than falling
    // back to the generated list unnoticed.
    for info in DEVICES {
        if let Some(layout) = layout(info.id) {
            let panel = layout.and_then(|l| l.panel(info)).unwrap_or_else(|e| panic!("{}: {e}", info.id));
            assert_eq!(panel_ids(&panel), info.params.iter().map(|p| p.id).collect::<Vec<_>>());
        }
    }
    for (id, _) in LAYOUT_FILES {
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
        let panel = panel(info.id).unwrap_or_else(|| panic!("{} has no panel", info.id));
        assert_eq!(panel_ids(panel), info.params.iter().map(|p| p.id).collect::<Vec<_>>(), "{}", info.id);
        for (parameter, native) in panel.parameters.iter().zip(info.params) {
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
    for (id, _) in LAYOUT_FILES {
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
        let device = create(info.id).unwrap_or_else(|| panic!("{} cannot be made", info.id));
        assert_eq!(device.info(), *info);
        assert_eq!(super::info(info.id), Some(*info));
        assert!(DEVICES.iter().skip(i + 1).all(|other| other.id != info.id), "{} twice", info.id);
    }
    assert!(create("no-such-device").is_none());
    assert!(info("").is_none());
}
