// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Sample instrument: shared SDK runtime and theme-aware SDK panel.
pub use suhina_device_sdk::sampler::{INFO, PARAMS, Sampler};
use suhina_device_sdk::ui::*;

/// The sampler's layout: its compact panel, every parameter drawn by the canvas (ADR 0021 ›
/// Amendment: device panels for built-in devices).
pub fn layout() -> suhina_device_sdk::builtin::Layout {
    suhina_device_sdk::builtin::Layout { ui: Some(panel()), ..Default::default() }
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
        2 => selected(0.) * UiValue::normalized("mode", false),
        _ => selected(0.) * (UiValue::from(1.) + UiValue::normalized("mode", false) * (-1f32).into()),
    }
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
fn section(ui: &mut DeviceUi, caption: &str, bounds: [f32; 4], view: Option<u16>) {
    control(
        ui,
        if caption.is_empty() { "section" } else { caption },
        Widget::Panel { label: caption.into() },
        bounds,
        UiGesture::Drag,
        view,
    );
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
            decimals: if matches!(id, "resonance" | "scan" | "lfo_rate" | "lfo_filter" | "lfo_pitch" | "warp_beats") {
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
}
fn adsr(ui: &mut DeviceUi, prefix: &str, title: &str, x: f32) {
    let ids = if prefix == "AMP" {
        ["attack", "decay", "sustain", "release"]
    } else {
        ["filter_attack", "filter_decay", "filter_sustain", "filter_release"]
    };
    label(ui, title, [x + 8., 86., 280., 14.], Some(1));
    for (i, caption) in ["A", "D", "S", "R"].iter().enumerate() {
        value(ui, ids[i], caption, [x + i as f32 * 72., 104., 72., 18.], Some(1));
    }
    control(
        ui,
        &format!("{prefix}_envelope"),
        Widget::Envelope {
            attack: ids[0].into(),
            decay: ids[1].into(),
            level: ids[2].into(),
            release: Some(ids[3].into()),
        },
        [x + 8., 126., 284., 40.],
        UiGesture::Position,
        Some(1),
    );
    if let Some(c) = ui.controls.last_mut() {
        c.id = format!("{prefix}_envelope");
        c.x = None;
    }
}
pub fn panel() -> DeviceUi {
    let mut ui = DeviceUi {
        size: [900., 168.],
        viewport_size: Some([900., 168.]),
        hidden_parameters: PARAMS.iter().map(|p| p.id.into()).collect(),
        ..DeviceUi::default()
    };
    for (i, caption) in ["Sample", "Grain"].iter().enumerate() {
        button(&mut ui, "mode", caption, [i as f32 * 82., 0., 78., 20.], UiGesture::Set(i as u16), None);
    }
    button(&mut ui, "panel_view", "LFO", [722., 0., 78., 20.], UiGesture::Tab(2), None);
    button(&mut ui, "panel_view", "Envelopes", [804., 0., 96., 20.], UiGesture::Tab(1), None);
    control(&mut ui, "sample", Widget::SampleWaveform, [0., 24., 900., 58.], UiGesture::Position, None);
    if let Some(c) = ui.controls.last_mut() {
        c.x = None;
    }
    control(&mut ui, "recording", Widget::SampleRecording, [390., 0., 316., 20.], UiGesture::Drag, None);
    if let Some(c) = ui.controls.last_mut() {
        c.x = None;
    }
    adsr(&mut ui, "AMP", "AMPLITUDE", 0.);
    adsr(&mut ui, "FILTER", "FILTER", 300.);
    value(&mut ui, "velocity", "Vel", [120., 86., 168., 14.], Some(1));
    for (i, (id, caption)) in
        [("filter_env", "Amt"), ("velocity_filter", "Vel"), ("key_filter", "Key")].iter().enumerate()
    {
        value(&mut ui, id, caption, [376. + i as f32 * 72., 86., 72., 14.], Some(1));
    }
    label(&mut ui, "PITCH ENVELOPE", [608., 86., 120., 14.], Some(1));
    value(&mut ui, "pitch_env", "Amount", [734., 86., 158., 14.], Some(1));
    for (i, (id, caption)) in [("pitch_attack", "A"), ("pitch_decay", "D")].iter().enumerate() {
        value(&mut ui, id, caption, [600. + i as f32 * 132., 104., 132., 18.], Some(1));
    }
    control(
        &mut ui,
        "PITCH_envelope",
        Widget::Envelope {
            attack: "pitch_attack".into(),
            decay: "pitch_decay".into(),
            level: "pitch_env".into(),
            release: None,
        },
        [608., 126., 284., 40.],
        UiGesture::Position,
        Some(1),
    );
    if let Some(c) = ui.controls.last_mut() {
        c.id = "PITCH_envelope".into();
        c.x = None;
    }
    let looping =
        UiValue::Expression(Box::new(UiExpression::Equal { a: UiValue::parameter("playback", false), b: 2f32.into() }));
    let slicing =
        UiValue::Expression(Box::new(UiExpression::Equal { a: UiValue::parameter("playback", false), b: 3f32.into() }));
    for is_loop in [false, true] {
        let fields = if is_loop {
            vec![
                ("start", "Start"),
                ("end", "End"),
                ("loop_start", "Loop start"),
                ("loop_end", "Loop end"),
                ("fade_in", "Fade in ms"),
                ("fade_out", "Fade out ms"),
            ]
        } else {
            vec![("start", "Start"), ("end", "End"), ("fade_in", "Fade in ms"), ("fade_out", "Fade out ms")]
        };
        let width = 604. / fields.len() as f32;
        for (i, (id, caption)) in fields.iter().enumerate() {
            value(&mut ui, id, caption, [8. + i as f32 * width, 86., width - 16., 18.], Some(0));
            if let Some(c) = ui.controls.last_mut() {
                c.visible_when = Some(
                    page(0)
                        * (UiValue::from(1.) + slicing.clone() * (-1f32).into())
                        * if is_loop { looping.clone() } else { UiValue::from(1.) + looping.clone() * (-1f32).into() },
                );
            }
        }
    }
    for (id, caption, x, width) in [
        ("start", "Start", 8., 76.),
        ("end", "End", 92., 76.),
        ("sensitivity", "Sensitivity", 176., 92.),
        ("slice_by", "Slice by", 276., 124.),
        ("fade_in", "Fade in ms", 408., 92.),
        ("fade_out", "Fade out ms", 508., 92.),
    ]
    .iter()
    {
        let bounds = [*x, 86., *width, 18.];
        if *id == "slice_by" {
            control(
                &mut ui,
                id,
                Widget::ValueMenu {
                    choices: ["Transient", "Beat", "Region", "Manual"]
                        .iter()
                        .enumerate()
                        .map(|(i, s)| (i as u16, format!("Slice by: {s}")))
                        .collect(),
                },
                bounds,
                UiGesture::Menu,
                Some(0),
            );
        } else {
            value(&mut ui, id, caption, bounds, Some(0));
        }
        if let Some(c) = ui.controls.last_mut() {
            c.visible_when = Some(page(0) * slicing.clone());
        }
    }
    button(&mut ui, "warp", "Warp", [612., 86., 54., 18.], UiGesture::Toggle, Some(0));
    if let Some(c) = ui.controls.last_mut() {
        c.widget =
            Some(Widget::Button { label: "Warp on".into(), off_label: Some("Warp off".into()), indicator: false });
        c.tooltip =
            "Warp on: fit the selected Start–End region to the project tempo. Source BPM is the original tempo of the loop."
                .into();
    }
    control(
        &mut ui,
        "warp_mode",
        Widget::ValueMenu {
            choices: ["Repitch", "Rhythmic", "Tonal", "Texture", "Smooth", "HighQuality"]
                .iter()
                .enumerate()
                .map(|(i, label)| (i as u16, (*label).into()))
                .collect(),
        },
        [672., 86., 106., 18.],
        UiGesture::Menu,
        Some(0),
    );
    value(&mut ui, "source_bpm", "Source", [784., 86., 108., 18.], Some(0));
    if let Some(c) = ui.controls.last_mut() {
        if let Some(binding) = &mut c.y {
            binding.drag_scale = 0.1;
        }
        c.tooltip = "Original tempo of this sample. Loading uses metadata, filename or detection; unknown tempo assumes the project BPM. With Warp on it follows the project BPM. Correct this value to the loop's original BPM if detection was wrong. Double-click re-detects the source tempo.".into();
    }
    for (i, (id, caption)) in [
        ("position", "Position"),
        ("scan", "Scan"),
        ("size", "Size ms"),
        ("density", "Grains/s"),
        ("jitter", "Jitter"),
        ("spread", "Spread"),
    ]
    .iter()
    .enumerate()
    {
        value(&mut ui, id, caption, [8. + i as f32 * 128., 86., 112., 18.], Some(2));
    }
    button(&mut ui, "freeze", "Freeze", [780., 86., 112., 18.], UiGesture::Toggle, Some(2));
    let normal_start = ui.controls.len();
    section(&mut ui, "", [0., 108., 900., 60.], None);
    for (caption, x, width) in [
        ("TUNING", 0., 214.),
        ("FILTER", 218., 206.),
        ("VOICES", 428., 188.),
        ("OUTPUT", 620., 128.),
        ("PLAYBACK", 752., 148.),
    ] {
        section(&mut ui, caption, [x, 108., width, 60.], None);
    }
    for (id, caption, bounds) in [
        ("pitch", "Tune", [8., 128., 60., 18.]),
        ("cents", "Cents", [78., 128., 60., 18.]),
        ("root", "Root", [148., 128., 60., 18.]),
        ("key_low", "Key min", [8., 148., 100., 18.]),
        ("key_high", "Key max", [114., 148., 92., 18.]),
        ("velocity_low", "Vel min", [436., 148., 88., 18.]),
        ("velocity_high", "Max", [530., 148., 78., 18.]),
        ("gain", "Gain", [628., 128., 112., 18.]),
        ("pan", "Pan", [628., 148., 50., 18.]),
        ("velocity", "Vel", [688., 148., 52., 18.]),
    ] {
        value(&mut ui, id, caption, bounds, None);
    }
    control(
        &mut ui,
        "filter",
        Widget::ValueMenu {
            choices: vec![
                (0, "Off".into()),
                (1, "Low pass".into()),
                (2, "High pass".into()),
                (3, "Band pass".into()),
                (4, "Notch".into()),
            ],
        },
        [226., 128., 78., 18.],
        UiGesture::Menu,
        None,
    );
    for (value, caption) in [(0, "12"), (1, "24")] {
        button(
            &mut ui,
            "filter_slope",
            caption,
            [226. + f32::from(value) * 44., 148., 38., 18.],
            UiGesture::Set(value),
            None,
        );
    }
    for (id, caption, x, format, decimals) in
        [("cutoff", "Cutoff", 312., TextFormat::Frequency, 0), ("resonance", "Res", 370., TextFormat::Number, 2)]
    {
        label(&mut ui, caption, [x, 110., 50., 14.], None);
        control(
            &mut ui,
            id,
            Widget::Dial { label: String::new(), decimals, format },
            [x, 126., 50., 40.],
            UiGesture::Drag,
            None,
        );
        if let Some(c) = ui.controls.last_mut() {
            c.y = c.x.take();
            if let Some(y) = &mut c.y {
                y.logarithmic = id == "cutoff";
                y.drag_scale = 2.;
            }
        }
    }
    control(
        &mut ui,
        "voice_mode",
        Widget::ValueMenu { choices: vec![(0, "Poly".into()), (1, "Mono".into())] },
        [436., 128., 68., 18.],
        UiGesture::Menu,
        None,
    );
    value(&mut ui, "poly", "Voices", [508., 128., 104., 18.], None);
    if let Some(c) = ui.controls.last_mut() {
        c.visible_when = Some(UiValue::from(1.) + UiValue::normalized("voice_mode", false) * (-1f32).into());
    }
    value(&mut ui, "glide", "Glide", [508., 128., 104., 18.], None);
    if let Some(c) = ui.controls.last_mut() {
        c.visible_when = Some(UiValue::normalized("voice_mode", false));
    }
    control(
        &mut ui,
        "playback",
        Widget::ValueMenu {
            choices: vec![(0, "Gate".into()), (1, "One shot".into()), (2, "Loop".into()), (3, "Slices".into())],
        },
        [760., 128., 132., 18.],
        UiGesture::Menu,
        None,
    );
    button(&mut ui, "reverse", "Reverse", [760., 148., 60., 18.], UiGesture::Toggle, None);
    button(&mut ui, "retrigger", "Retrigger", [824., 148., 68., 18.], UiGesture::Toggle, None);
    for c in &mut ui.controls[normal_start..] {
        let closed = page(0) + page(2);
        c.visible_when = Some(c.visible_when.take().map_or(closed.clone(), |v| v * closed));
    }
    section(&mut ui, "LFO", [0., 86., 900., 82.], Some(3));
    control(
        &mut ui,
        "lfo_rate",
        Widget::WaveformPreview { shape: "lfo_shape".into() },
        [12., 114., 464., 52.],
        UiGesture::Position,
        Some(3),
    );
    if let Some(c) = ui.controls.last_mut() {
        c.id = "lfo_preview".into();
        c.tooltip = "Drag horizontally to set LFO rate".into();
        if let Some(x) = &mut c.x {
            x.logarithmic = true;
        }
    }
    control(&mut ui, "lfo_shape", Widget::WaveformMenu, [56., 90., 112., 18.], UiGesture::Menu, Some(3));
    control(&mut ui, "lfo_rate_mode", Widget::Menu, [174., 90., 96., 18.], UiGesture::Menu, Some(3));
    for mode in 0..=5 {
        let id = if mode == 0 {
            "lfo_rate"
        } else if mode == 1 {
            "lfo_time"
        } else {
            "lfo_division"
        };
        if mode < 2 {
            value(&mut ui, id, if mode == 0 { "Rate" } else { "ms" }, [274., 90., 202., 18.], Some(3));
        } else {
            let choices = (1..=64)
                .map(|n| {
                    (
                        n,
                        match mode {
                            3 => format!("1/{n} T"),
                            4 => format!("1/{n} D"),
                            5 => format!("{n} x 1/16"),
                            _ => format!("1/{n}"),
                        },
                    )
                })
                .collect();
            control(&mut ui, id, Widget::ValueMenu { choices }, [274., 90., 202., 18.], UiGesture::Menu, Some(3));
        }
        if let Some(c) = ui.controls.last_mut() {
            c.visible_when = Some(
                page(3)
                    * UiValue::Expression(Box::new(UiExpression::Equal {
                        a: UiValue::parameter("lfo_rate_mode", false),
                        b: (mode as f32).into(),
                    })),
            );
        }
    }
    section(&mut ui, "MODULATION", [490., 86., 410., 82.], Some(3));
    for (id, caption, bounds) in [
        ("lfo_pitch", "Pitch st", [502., 112., 182., 20.]),
        ("lfo_filter", "Filter", [702., 112., 186., 20.]),
        ("lfo_volume", "Volume", [502., 142., 182., 20.]),
        ("lfo_pan", "Pan", [702., 142., 186., 20.]),
    ] {
        value(&mut ui, id, caption, bounds, Some(3));
    }

    ui
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
        let source = [vec![0.; 50], vec![1.; 50], vec![0.; 50], vec![0.5; 50]].concat();
        let analysis = SliceAnalysis::new(&source, 1, 1000.);
        let mut p: Vec<f64> = PARAMS.iter().map(|p| p.default).collect();
        p[59] = 1.;
        let points = analysis.points(&p, 200, 1000., 120.);
        assert!(points.contains(&0.25));
        assert!(points.contains(&0.75));
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
