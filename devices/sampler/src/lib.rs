// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Sample instrument: shared SDK runtime and theme-aware SDK panel.
pub use suhina_device_sdk::sampler::{INFO, PARAMS, Sampler};
use suhina_device_sdk::ui::*;

/// The sampler's layout: its compact panel, every parameter drawn by the canvas (ADR 0021 ›
/// Amendment: device panels for built-in devices).
pub fn layout() -> suhina_device_sdk::builtin::Layout {
    suhina_device_sdk::builtin::Layout { ui: Some(panel()), header: Some(panel::header()), ..Default::default() }
}

const ACCENT: [u8; 4] = [166, 163, 255, 255];
fn page(value: u16) -> UiValue {
    let selected = |value: f32| {
        UiValue::Expression(Box::new(UiExpression::Equal {
            a: UiValue::parameter("panel_view", false),
            b: value.into(),
        }))
    };
    match value {
        1 => selected(1.),
        3 => selected(2.),
        2 => selected(0.) * equals("mode", 1.),
        _ => selected(0.) * (UiValue::from(1.) + equals("mode", 1.) * (-1f32).into()),
    }
}
fn equals(id: &str, value: f32) -> UiValue {
    UiValue::Expression(Box::new(UiExpression::Equal { a: UiValue::parameter(id, false), b: value.into() }))
}
fn control(ui: &mut DeviceUi, id: &str, widget: Widget, bounds: [f32; 4], gesture: UiGesture, view: Option<u16>) {
    ui.controls.push(CustomControl {
        id: format!("{id}_{}", ui.controls.len()),
        name: id.replace('_', " "),
        tooltip: id.replace('_', " "),
        bounds,
        widget: Some(widget),
        gesture,
        x: Some(UiBinding::new(id)),
        y: None,
        drawing: vec![],
        visible_when: view.map(page),
    });
}
fn label(ui: &mut DeviceUi, text: &str, bounds: [f32; 4], view: Option<u16>) {
    control(ui, text, Widget::Label { label: text.into() }, bounds, UiGesture::Drag, view);
    if let Some(c) = ui.controls.last_mut() {
        c.x = None;
    }
}
fn value(ui: &mut DeviceUi, id: &str, caption: &str, bounds: [f32; 4], view: Option<u16>) {
    let format = match id {
        "attack" | "decay" | "release" | "glide" | "pitch_attack" | "pitch_decay" | "filter_attack"
        | "filter_decay" | "filter_release" => TextFormat::Seconds,
        "start" | "end" | "loop_start" | "loop_end" | "sustain" | "filter_sustain" | "velocity" | "position"
        | "jitter" | "spread" | "velocity_low" | "velocity_high" | "lfo_volume" | "lfo_pan" | "sensitivity" => {
            TextFormat::Percent
        }
        "cutoff" | "lfo_rate" => TextFormat::Frequency,
        "source_bpm" => TextFormat::Tempo,
        "gain" => TextFormat::Decibels,
        "pan" => TextFormat::Balance,
        _ => TextFormat::Number,
    };
    control(
        ui,
        id,
        Widget::Number {
            decimals: if matches!(
                id,
                "resonance" | "scan" | "lfo_rate" | "lfo_filter" | "lfo_pitch" | "warp_beats" | "first_beat"
            ) {
                2
            } else {
                0
            },
            format,
            color: ACCENT,
            center: false,
            bordered: false,
            prefix: if caption.is_empty() { String::new() } else { format!("{caption} ") },
            suffix: String::new(),
        },
        bounds,
        UiGesture::Drag,
        view,
    );
    if let Some(c) = ui.controls.last_mut() {
        c.y = c.x.take();
        if let Some(binding) = &mut c.y {
            binding.logarithmic = matches!(id, "cutoff" | "lfo_rate" | "warp_beats");
        }
        c.tooltip = format!("{}: drag vertically; Shift for precision; double-click resets", id.replace('_', " "));
    }
}
fn button(ui: &mut DeviceUi, id: &str, caption: &str, bounds: [f32; 4], gesture: UiGesture, view: Option<u16>) {
    control(ui, id, Widget::Button { label: caption.into(), off_label: None, indicator: false }, bounds, gesture, view);
    if let Some(c) = ui.controls.last_mut() {
        c.name = caption.into();
        c.tooltip = caption.into();
    }
}
mod panel;
pub fn panel() -> DeviceUi {
    panel::panel()
}

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;

#[cfg(test)]
mod slice_tests {
    use super::*;
    use std::sync::Arc;
    use suhina_device_api::{Device, NoteEvent, ProcessContext};
    fn set(s: &mut Sampler, id: &str, value: f64) {
        s.set_param(PARAMS.iter().position(|p| p.id == id).unwrap(), value);
    }
    #[test]
    fn slices_play_separate_regions_at_original_pitch_and_stop_at_boundary() {
        let mut s = Sampler::new();
        s.prepare(48000.).unwrap();
        s.load_sample(48000, 1, Arc::from([vec![0.25; 100], vec![0.75; 100]].concat()));
        for (id, v) in [("playback", 3.), ("slice_by", 2.), ("sensitivity", 1. / 31.), ("attack", 0.), ("sustain", 1.)]
        {
            set(&mut s, id, v);
        }
        for (key, expected) in [(60, 0.25), (61, 0.75)] {
            s.reset();
            s.event(NoteEvent::On { key, velocity: 1. });
            s.event(NoteEvent::Off { key, velocity: 0. });
            let mut buffer = vec![0.; 240];
            s.process(&ProcessContext::default(), &mut buffer);
            assert!((buffer[100] - expected).abs() < 0.001);
            assert!(buffer[202..].iter().all(|v| *v == 0.));
        }
        s.event(NoteEvent::On { key: 62, velocity: 1. });
        let mut buffer = [0.; 16];
        s.process(&ProcessContext::default(), &mut buffer);
        assert_eq!(buffer, [0.; 16]);
    }
    #[test]
    fn transient_manual_and_beat_maps_are_bounded() {
        use suhina_device_sdk::sampler::SliceAnalysis;
        // A hit, then a quieter one after a short silence: both found, a reading before each.
        let source = [vec![0.; 500], vec![1.; 500], vec![0.; 500], vec![0.5; 500]].concat();
        let analysis = SliceAnalysis::new(&source, 1, 1000.);
        let mut p: Vec<f64> = PARAMS.iter().map(|p| p.default).collect();
        p[59] = 1.;
        let points = analysis.points(&p, 2000, 1000., 120.);
        for hit in [0.25, 0.75] {
            assert!(points.iter().any(|p| (p - hit).abs() < 0.005), "{hit} in {:?}", &points[..4]);
        }
        p[60] = 3.;
        p[61] = 0.6;
        p[62] = 0.2;
        p[63] = 0.6;
        let points = analysis.points(&p, 200, 1000., 120.);
        assert_eq!(&points[..4], &[0., 0.2, 0.6, 1.]);
        p[60] = 1.;
        p[59] = 0.4;
        let points = analysis.points(&p, 2000, 1000., 120.);
        assert!(points.contains(&0.25));
    }
}

#[cfg(test)]
mod dynamic_slice_tests {
    use super::*;
    use std::sync::Arc;
    use suhina_device_api::{Device, NoteEvent, ProcessContext};
    #[test]
    fn manual_maps_beyond_legacy_and_midi_limits_crop_and_swap_without_stopping_voices() {
        let mut s = Sampler::new();
        s.prepare(48000.).unwrap();
        s.load_sample(48000, 1, Arc::from(vec![1.; 48000]));
        for (id, value) in [("playback", 3.), ("slice_by", 3.), ("root", 0.)] {
            s.set_param(PARAMS.iter().position(|p| p.id == id).unwrap(), value);
        }
        s.load_sample_slices(Some(Arc::from((1..=300).map(|i| i as f64 / 301.).collect::<Vec<_>>())));
        s.event(NoteEvent::On { key: 127, velocity: 1. });
        assert!((f64::from(s.sample_position().unwrap()) - 127. / 301.).abs() < 1e-6);
        let mut next = Some(Arc::from([0.2, 0.8]));
        s.swap_sample_slices(&mut next);
        assert_eq!(next.unwrap().len(), 300);
        assert!(s.sample_position().is_some());
        s.reset();
        s.event(NoteEvent::On { key: 1, velocity: 1. });
        assert!((s.sample_position().unwrap() - 0.2).abs() < 1e-6);
        s.reset();
        s.set_param(PARAMS.iter().position(|p| p.id == "start").unwrap(), 0.3);
        s.event(NoteEvent::On { key: 0, velocity: 1. });
        assert!((s.sample_position().unwrap() - 0.3).abs() < 1e-6);
        s.load_sample_slices(Some(Arc::from([])));
        s.reset();
        s.event(NoteEvent::On { key: 1, velocity: 1. });
        assert!(s.sample_position().is_none());
        s.event(NoteEvent::On { key: 0, velocity: 1. });
        let mut a = [0.; 128];
        s.process(&ProcessContext::default(), &mut a);
        assert!(a.iter().any(|v| *v > 0.));
    }
}
