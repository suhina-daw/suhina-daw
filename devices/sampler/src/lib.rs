// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Sample instrument: shared SDK runtime and theme-aware SDK panel.
pub use suhina_device_sdk::sampler::{INFO, PARAMS, Sampler};
use suhina_device_sdk::{Parameter, ParameterControl, ui::*};

pub fn parameters() -> Vec<Parameter> {
    PARAMS
        .iter()
        .map(|p| {
            let mut param = Parameter::new(p.id, p.name, (p.min, p.max), p.default);
            param.control = match p.kind {
                suhina_device_api::ParamKind::Choice(_) => ParameterControl::Choice,
                suhina_device_api::ParamKind::Integer => ParameterControl::Integer,
                suhina_device_api::ParamKind::Toggle => ParameterControl::Toggle,
                _ => ParameterControl::Knob,
            };
            param
        })
        .collect()
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
                ("crossfade", "Loop fade ms"),
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
    for (i, (id, caption)) in [
        ("start", "Start"),
        ("end", "End"),
        ("sensitivity", "Sensitivity"),
        ("slice_by", "Slice by"),
        ("fade_in", "Fade in ms"),
        ("fade_out", "Fade out ms"),
    ]
    .iter()
    .enumerate()
    {
        let bounds = [8. + i as f32 * 100., 86., 92., 18.];
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
mod tests {
    use super::*;
    use suhina_device_api::{Device, NoteEvent, ProcessContext};

    fn loaded() -> Sampler {
        let mut s = Sampler::new();
        s.load_sample(
            48000,
            1,
            (0..48000)
                .map(|i| (i as f32 * std::f32::consts::TAU * 220. / 48000.).sin() * 0.2)
                .collect::<Vec<_>>()
                .into(),
        );
        s
    }
    fn set(s: &mut Sampler, id: &str, value: f64) {
        s.set_param(INFO.param_index(id).unwrap(), value);
    }
    fn note(s: &mut Sampler, key: u8) {
        s.event(NoteEvent::On { key, velocity: 1. });
    }
    fn render(s: &mut Sampler, frames: usize, block: usize) -> Vec<f32> {
        let mut out = vec![0.; frames * 2];
        for chunk in out.chunks_mut(block * 2) {
            s.process(&ProcessContext::default(), chunk);
        }
        out
    }
    #[test]
    fn warp_preserves_pitch_and_note_length_across_modes_and_blocks() {
        let make = |mode, block: usize, tempo| {
            let mut s = loaded();
            s.prepare(48000.).unwrap();
            for (id, value) in
                [("warp", 1.), ("warp_mode", mode), ("warp_beats", 4.), ("attack", 0.), ("sustain", 1.), ("pitch", 12.)]
            {
                set(&mut s, id, value);
            }
            let context = ProcessContext { tempo, ..ProcessContext::default() };
            s.set_context(&context);
            note(&mut s, 60);
            let mut out = vec![0.; 144000 * 2];
            for chunk in out.chunks_mut(block * 2) {
                s.process(&context, chunk);
            }
            out
        };
        for mode in 0..6 {
            let a = make(f64::from(mode), 64, 120.);
            assert_eq!(a, make(f64::from(mode), 127, 120.));
            let crossings = a[24000 * 2..48000 * 2]
                .as_chunks::<2>()
                .0
                .iter()
                .map(|f| f[0])
                .collect::<Vec<_>>()
                .windows(2)
                .filter(|w| w[0] <= 0. && w[1] > 0.)
                .count();
            let expected = if mode == 0 { 55 } else { 220 };
            assert!(crossings.abs_diff(expected) <= 3, "mode {mode}: {crossings}");
            assert!(a[90000 * 2..94000 * 2].iter().any(|v| v.abs() > 0.01));
            assert!(a[97000 * 2..].iter().all(|v| *v == 0.));
        }
        let fast = make(1., 64, 240.);
        assert!(fast[49000 * 2..].iter().all(|v| *v == 0.));
    }

    #[test]
    fn source_bpm_sets_speed_independently_of_loop_length_and_trim() {
        for seconds in [2, 4, 16] {
            let mut s = Sampler::new();
            s.load_sample(
                48000,
                1,
                (0..seconds * 48000)
                    .map(|i| (i as f32 * std::f32::consts::TAU * 220. / 48000.).sin() * 0.2)
                    .collect::<Vec<_>>()
                    .into(),
            );
            for (id, value) in [("source_bpm", 120.), ("warp", 1.), ("attack", 0.), ("sustain", 1.)] {
                set(&mut s, id, value);
            }
            note(&mut s, 60);
            let out = render(&mut s, 4800, 64);
            let crossings = out.as_chunks::<2>().0.windows(2).filter(|w| w[0][0] <= 0. && w[1][0] > 0.).count();
            assert!(
                crossings.abs_diff(22) <= 2,
                "a {seconds}s loop must retain its 220 Hz pitch: {crossings} crossings"
            );
            assert!((s.sample_position().unwrap() - 0.1 / seconds as f32).abs() < 1e-6);
            let before = s.sample_position().unwrap();
            let context = ProcessContext { tempo: 60., ..ProcessContext::default() };
            s.process(&context, &mut vec![0.; 9600]);
            assert!((s.sample_position().unwrap() - before - 0.05 / seconds as f32).abs() < 1e-6);
            set(&mut s, "start", 0.25);
            set(&mut s, "end", 0.75);
            note(&mut s, 60);
            let before = s.sample_positions().into_iter().flatten().last().unwrap();
            s.process(&context, &mut vec![0.; 9600]);
            let after = s.sample_positions().into_iter().flatten().last().unwrap();
            assert!((after - before - 0.05 / seconds as f32).abs() < 1e-6, "trimming must not alter the source tempo");
        }
    }

    #[test]
    fn changing_warp_during_a_held_note_keeps_its_head_and_audio() {
        let mut s = loaded();
        for (id, value) in [("playback", 2.), ("warp", 1.), ("attack", 0.), ("sustain", 1.)] {
            set(&mut s, id, value);
        }
        note(&mut s, 60);
        render(&mut s, 12000, 64);
        for (id, value) in [
            ("warp_mode", 2.),
            ("warp_mode", 3.),
            ("warp_mode", 4.),
            ("warp_mode", 5.),
            ("warp_mode", 0.),
            ("warp_mode", 1.),
            ("warp", 0.),
            ("warp", 1.),
        ] {
            let position = s.sample_position().unwrap();
            set(&mut s, id, value);
            assert_eq!(s.sample_position(), Some(position), "{id}={value} must keep the held note");
            let out = render(&mut s, 4800, 127);
            assert!(s.sample_position().is_some());
            for window in out.chunks(960) {
                assert!(window.iter().any(|s| s.abs() > 0.005), "{id}={value}: silent 10 ms window");
            }
        }
    }

    #[test]
    fn equivalent_pitch_preserving_modes_do_not_restart_the_stretcher() {
        let make = |change| {
            let mut s = loaded();
            set(&mut s, "warp", 1.);
            note(&mut s, 60);
            render(&mut s, 12000, 64);
            if change {
                set(&mut s, "warp_mode", 5.);
            }
            render(&mut s, 4800, 127)
        };
        assert_eq!(make(false), make(true));
    }

    #[test]
    fn warp_controls_share_the_fade_row_without_overlaps() {
        let ui = panel();
        let controls: Vec<_> = ui
            .controls
            .iter()
            .filter(|c| {
                c.x.iter()
                    .chain(c.y.iter())
                    .any(|b| matches!(b.parameter.as_str(), "warp" | "warp_mode" | "source_bpm"))
            })
            .collect();
        assert_eq!(controls.len(), 3);
        for c in &controls {
            assert_eq!(c.bounds[1], 86.);
        }
        assert!(controls.windows(2).all(|w| w[0].bounds[0] + w[0].bounds[2] <= w[1].bounds[0]));
    }
    #[test]
    fn stolen_voices_are_continuous_and_heads_are_independent() {
        for mono in [0., 1.] {
            let mut s = loaded();
            set(&mut s, "poly", 1.);
            set(&mut s, "voice_mode", mono);
            set(&mut s, "attack", 0.);
            note(&mut s, 60);
            let before = render(&mut s, 480, 64);
            note(&mut s, 72);
            let after = render(&mut s, 384, 127);
            assert!((after[0] - before[before.len() - 2]).abs() < 1e-6);
            set(&mut s, "retrigger", 0.);
            let position = s.sample_position();
            note(&mut s, 67);
            assert_eq!(s.sample_position(), position);
            assert_eq!(s.sample_positions().into_iter().flatten().count(), 1);
        }
        let mut s = loaded();
        note(&mut s, 60);
        render(&mut s, 480, 64);
        note(&mut s, 72);
        render(&mut s, 120, 64);
        let positions = s.sample_positions();
        assert!((positions[0].unwrap() - 0.0125).abs() < 1e-6);
        assert!((positions[1].unwrap() - 0.005).abs() < 1e-6);
        set(&mut s, "release", 0.);
        s.event(NoteEvent::Off { key: 72, velocity: 0. });
        render(&mut s, 1, 1);
        assert!(s.sample_positions()[0].is_some());
        assert!(s.sample_positions()[1].is_none());
    }

    #[test]
    fn filter_types_slopes_and_modulators_are_audible_and_block_independent() {
        let make = |kind, slope| {
            let mut s = loaded();
            for (id, value) in [
                ("filter", kind),
                ("filter_slope", slope),
                ("cutoff", 400.),
                ("filter_env", 2.),
                ("velocity_filter", 0.5),
                ("key_filter", 1.),
                ("filter_attack", 0.01),
                ("filter_decay", 0.02),
                ("filter_sustain", 0.3),
            ] {
                set(&mut s, id, value);
            }
            note(&mut s, 72);
            s
        };
        let mut outputs = Vec::new();
        for kind in [1., 2., 3., 4.] {
            for slope in [0., 1.] {
                let output = render(&mut make(kind, slope), 4096, 64);
                assert!(output.iter().all(|v| v.is_finite()));
                assert_eq!(output, render(&mut make(kind, slope), 4096, 127));
                assert!(outputs.iter().all(|other| other != &output));
                outputs.push(output);
            }
        }
        let baseline = render(&mut make(1., 0.), 4096, 64);
        for (id, value) in [("filter_env", 0.), ("velocity_filter", 0.), ("key_filter", 0.), ("filter_attack", 1.)] {
            let mut s = make(1., 0.);
            set(&mut s, id, value);
            assert_ne!(baseline, render(&mut s, 4096, 64), "{id}");
        }
    }

    #[test]
    fn playback_changes_stop_voices_but_reselecting_the_mode_does_not() {
        for mode in [0., 1., 2.] {
            let mut s = loaded();
            set(&mut s, "playback", mode);
            note(&mut s, 60);
            render(&mut s, 64, 16);
            set(&mut s, "playback", mode);
            assert!(s.sample_position().is_some());
            set(&mut s, "playback", (mode + 1.) % 3.);
            assert_eq!(s.sample_position(), None);
            assert!(render(&mut s, 128, 32).iter().all(|v| *v == 0.));
            note(&mut s, 60);
            assert!(render(&mut s, 128, 32).iter().any(|v| *v != 0.));
        }
    }

    #[test]
    fn compact_panel_keeps_controls_reachable_on_its_pages() {
        let ui = panel();
        assert_eq!(ui.size, [900., 168.]);
        assert!(ui.size[1] + 32. <= 200.);
        assert_eq!(ui.viewport_size, Some(ui.size));
        for c in &ui.controls {
            assert!(c.widget.is_some() && c.drawing.is_empty(), "{} must use an SDK widget", c.id);
            let [x, y, w, h] = c.bounds;
            assert!(x >= 0. && y >= 0. && x + w <= ui.size[0] && y + h <= ui.size[1], "{}", c.id);
        }
        for p in PARAMS {
            if p.id.starts_with("slice_")
                || matches!(
                    p.id,
                    "start"
                        | "end"
                        | "loop_start"
                        | "loop_end"
                        | "ping_pong"
                        | "grain_env"
                        | "envelope_tab"
                        | "warp_beats"
                )
            {
                continue;
            }
            assert!(ui.controls.iter().any(|c| c.x.iter().chain(c.y.iter()).any(|b| b.parameter == p.id)
                || matches!(&c.widget, Some(Widget::Envelope { attack, decay, level, release }) if [Some(attack.as_str()),Some(decay.as_str()),Some(level.as_str()),release.as_deref()].contains(&Some(p.id)))), "{}", p.id);
        }
        assert!(!ui.controls.iter().any(|c| c.id == "grain_cloud"));
        assert_eq!(
            ui.controls
                .iter()
                .filter(|c| matches!(&c.widget, Some(Widget::Button { label, .. }) if label == "Grain"))
                .count(),
            1
        );
        for view in [0., 1., 2.] {
            for mode in [0., 1.] {
                let controls: Vec<_> = ui
                    .controls
                    .iter()
                    .filter(|c| {
                        c.visible_when.as_ref().is_none_or(|v| {
                            v.evaluate(&|id, _, _| match id {
                                "panel_view" => view,
                                "mode" => mode,
                                _ => 0.,
                            }) > 0.
                        })
                    })
                    .collect();
                let waveforms: Vec<_> =
                    controls.iter().filter(|c| matches!(c.widget, Some(Widget::SampleWaveform))).collect();
                assert_eq!(waveforms.len(), 1);
                assert_eq!(waveforms[0].bounds, [0., 24., 900., 58.]);
                assert_eq!(controls.iter().any(|c| c.id == "lfo_preview"), view == 2.);
                if view == 0. {
                    for id in ["gain", "pan", "filter", "voice_mode"] {
                        assert!(controls.iter().any(|c| c.x.iter().chain(c.y.iter()).any(|b| b.parameter == id)));
                    }
                    assert_eq!(
                        controls.iter().any(|c| c.y.as_ref().is_some_and(|b| b.parameter == "jitter")),
                        mode == 1.
                    );
                }
            }
        }
    }
    #[test]
    fn envelopes_have_independent_stages_and_responsive_release() {
        let ui = panel();
        for (prefix, ids) in [
            ("AMP", ["attack", "decay", "sustain", "release"]),
            ("FILTER", ["filter_attack", "filter_decay", "filter_sustain", "filter_release"]),
        ] {
            let envelope = ui.controls.iter().find(|c| c.id == format!("{prefix}_envelope")).unwrap();
            assert!(
                matches!(&envelope.widget, Some(Widget::Envelope { attack, decay, level, release }) if attack == ids[0] && decay == ids[1] && level == ids[2] && release.as_deref() == Some(ids[3]))
            );
            assert!(envelope.x.is_none() && envelope.y.is_none());
        }
    }
    #[test]
    fn lfo_destinations_are_audible_bounded_and_block_independent_in_both_modes() {
        let make = |mode, destination: &str, depth| {
            let mut s = loaded();
            for (id, value) in [
                ("mode", mode),
                ("playback", 2.),
                ("attack", 0.),
                ("decay", 0.),
                ("sustain", 1.),
                ("filter", 1.),
                ("cutoff", 400.),
                ("lfo_rate", 5.),
            ] {
                set(&mut s, id, value);
            }
            if !destination.is_empty() {
                set(&mut s, destination, depth);
            }
            note(&mut s, 60);
            s
        };
        for mode in [0., 1.] {
            let baseline = render(&mut make(mode, "", 0.), 24000, 64);
            for (id, depth) in [("lfo_pitch", 4.), ("lfo_filter", 3.), ("lfo_volume", 1.), ("lfo_pan", 1.)] {
                let output = render(&mut make(mode, id, depth), 24000, 64);
                assert!(output.iter().all(|v| v.is_finite()));
                assert_ne!(output, baseline, "{mode}: {id}");
                assert_eq!(output, render(&mut make(mode, id, depth), 24000, 127), "{mode}: {id}");
                if matches!(id, "lfo_volume" | "lfo_pan") {
                    assert!(
                        output.iter().zip(&baseline).all(|(a, b)| a.abs() <= b.abs() + 1e-6),
                        "{id} must never boost"
                    );
                }
                assert_eq!(baseline, render(&mut make(mode, id, 0.), 24000, 64), "zero {id} must preserve sound");
            }
        }
    }

    #[test]
    fn volume_and_pan_lfo_match_the_sine_golden() {
        for destination in ["lfo_volume", "lfo_pan"] {
            let mut s = Sampler::new();
            s.prepare(48000.).unwrap();
            s.load_sample(48000, 1, vec![1.; 4800].into());
            for (id, value) in [("attack", 0.), ("decay", 0.), ("sustain", 1.), ("lfo_rate", 20.), (destination, 1.)] {
                set(&mut s, id, value);
            }
            note(&mut s, 60);
            let output = render(&mut s, 2400, 7);
            for (i, frame) in output.as_chunks::<2>().0.iter().enumerate() {
                let wave = ((i + 1) as f64 * std::f64::consts::TAU * 20. / 48000.).sin();
                let expected = if destination == "lfo_volume" {
                    [0.5 - wave * 0.5; 2]
                } else {
                    [1. - wave.max(0.), 1. + wave.min(0.)]
                };
                for channel in 0..2 {
                    assert!((f64::from(frame[channel]) - expected[channel]).abs() < 1e-6, "{destination} frame {i}");
                }
            }
        }
    }

    #[test]
    fn lfo_shapes_match_the_shared_oscillator_and_random_is_repeatable() {
        for shape_index in 0..6 {
            let make = || {
                let mut s = Sampler::new();
                s.load_sample(48000, 1, vec![1.; 48000].into());
                set(&mut s, "attack", 0.);
                set(&mut s, "sustain", 1.);
                set(&mut s, "lfo_rate", 20.);
                set(&mut s, "lfo_shape", shape_index as f64);
                set(&mut s, "lfo_volume", 1.);
                s.prepare(48000.).unwrap();
                note(&mut s, 60);
                s
            };
            let output = render(&mut make(), 10000, 64);
            assert_eq!(output, render(&mut make(), 10000, 127));
            let mut phase: f64 = 0.;
            let mut cycle = 0;
            for frame in output.as_chunks::<2>().0 {
                let next = phase + 20. / 48000.;
                cycle += usize::from(next >= 1.);
                phase = next.fract();
                let cycles = if shape_index == 5 { cycle as f64 } else { phase };
                let expected = (1. - waveform(shape_index, cycles)) * 0.5;
                assert!((f64::from(frame[0]) - expected).abs() < 1e-6, "shape {shape_index}");
            }
            let mut reset = make();
            render(&mut reset, 10000, 64);
            reset.reset();
            note(&mut reset, 60);
            assert_eq!(output, render(&mut reset, 10000, 64));
        }
    }

    #[test]
    fn source_fades_are_independent_and_match_linear_edges_in_both_directions() {
        for reverse in [0., 1.] {
            for (fade_in, fade_out) in [(0., 0.), (10., 0.), (0., 20.), (10., 20.)] {
                let make = || {
                    let mut s = Sampler::new();
                    s.load_sample(48000, 1, vec![1.; 4800].into());
                    set(&mut s, "attack", 0.);
                    set(&mut s, "sustain", 1.);
                    set(&mut s, "reverse", reverse);
                    set(&mut s, "fade_in", fade_in);
                    set(&mut s, "fade_out", fade_out);
                    note(&mut s, 60);
                    s
                };
                let output = render(&mut make(), 4800, 64);
                assert_eq!(output, render(&mut make(), 4800, 127));
                for (i, frame) in output.as_chunks::<2>().0.iter().enumerate() {
                    let pos = if reverse > 0. { 4799 - i } else { i } as f64;
                    let fade = |distance: f64, ms: f64| if ms == 0. { 1. } else { (distance / (ms * 48.)).min(1.) };
                    let expected = fade(pos, fade_in) * fade(4799. - pos, fade_out);
                    assert!((f64::from(frame[0]) - expected).abs() < 1e-6, "{reverse} {fade_in} {fade_out} {i}");
                }
            }
        }
        for mode in [0., 1.] {
            let make = |fade| {
                let mut s = loaded();
                set(&mut s, "mode", mode);
                set(&mut s, "position", 0.);
                set(&mut s, "jitter", 0.);
                set(&mut s, "fade_in", fade);
                note(&mut s, 60);
                render(&mut s, 4096, 64)
            };
            assert_ne!(make(0.), make(500.));
        }
    }

    #[test]
    fn changing_editor_tabs_does_not_change_audio() {
        let make = |view| {
            let mut s = loaded();
            set(&mut s, "panel_view", view);
            note(&mut s, 60);
            render(&mut s, 2048, 64)
        };
        assert_eq!(make(0.), make(1.));
        assert_eq!(make(1.), make(2.));
    }

    #[test]
    fn mono_replaces_the_voice_and_reports_the_read_head() {
        let mut s = loaded();
        set(&mut s, "voice_mode", 1.);
        set(&mut s, "attack", 0.);
        note(&mut s, 60);
        render(&mut s, 480, 64);
        assert!((s.sample_position().unwrap() - 0.01).abs() < 0.0001);
        note(&mut s, 72);
        let actual = render(&mut s, 384, 64);
        let mut reference = loaded();
        set(&mut reference, "attack", 0.);
        note(&mut reference, 72);
        assert_eq!(&actual[512..], &render(&mut reference, 384, 64)[512..]);
        s.reset();
        assert_eq!(s.sample_position(), None);
    }

    #[test]
    fn pitch_envelope_changes_audio_and_is_block_independent() {
        let mut plain = loaded();
        note(&mut plain, 60);
        let plain = render(&mut plain, 4096, 64);
        let mut s = loaded();
        set(&mut s, "pitch_env", 12.);
        note(&mut s, 60);
        let modulated = render(&mut s, 4096, 64);
        assert_ne!(plain, modulated);
        s.reset();
        note(&mut s, 60);
        assert_eq!(modulated, render(&mut s, 4096, 127));
    }

    #[test]
    fn device_contract() {
        let failures = suhina_device_api::conformance::check(&|| {
            let mut sampler = Sampler::new();
            sampler.load_sample(
                48000,
                1,
                (0..48000)
                    .map(|i| (i as f32 * std::f32::consts::TAU * 220. / 48000.).sin() * 0.2)
                    .collect::<Vec<_>>()
                    .into(),
            );
            Box::new(sampler)
        });
        assert!(failures.is_empty(), "{failures:?}");
    }

    #[test]
    fn pitch_reverse_and_region_golden() {
        let mut s = Sampler::new();
        s.load_sample(48000, 1, (0..100).map(|i| i as f32 / 100.).collect::<Vec<_>>().into());
        for (id, v) in [("attack", 0.), ("decay", 0.), ("sustain", 1.)] {
            set(&mut s, id, v);
        }
        note(&mut s, 72);
        assert_eq!(render(&mut s, 4, 3), vec![0., 0., 0.02, 0.02, 0.04, 0.04, 0.06, 0.06]);
        s.reset();
        set(&mut s, "reverse", 1.);
        note(&mut s, 60);
        assert_eq!(render(&mut s, 3, 1), vec![0.99, 0.99, 0.98, 0.98, 0.97, 0.97]);
        s.reset();
        set(&mut s, "start", 0.9);
        set(&mut s, "end", 0.1);
        note(&mut s, 60);
        assert!(render(&mut s, 32, 7).iter().all(|v| *v == 0.));
    }

    #[test]
    fn polyphonic_sum_keeps_headroom_and_hostile_audio_stays_finite() {
        let mut s = Sampler::new();
        s.load_sample(48000, 1, vec![1.; 16].into());
        for (id, v) in [("attack", 0.), ("decay", 0.), ("sustain", 1.), ("poly", 32.)] {
            set(&mut s, id, v);
        }
        for _ in 0..32 {
            note(&mut s, 60);
        }
        assert_eq!(render(&mut s, 1, 1), [32., 32.]);
        s.load_sample(48000, 1, vec![f32::MAX; 16].into());
        for _ in 0..32 {
            note(&mut s, 60);
        }
        assert!(render(&mut s, 1, 1).iter().all(|s| s.is_finite()));
    }

    #[test]
    fn granular_is_deterministic_across_blocks_and_reset() {
        let make = || {
            let mut s = loaded();
            set(&mut s, "mode", 1.);
            set(&mut s, "scan", 1.);
            set(&mut s, "spread", 0.8);
            note(&mut s, 60);
            s
        };
        let expected = render(&mut make(), 16000, 1024);
        let mut s = make();
        assert_eq!(render(&mut s, 16000, 7), expected);
        assert!(expected.iter().any(|v| v.abs() > 0.01));
        s.reset();
        note(&mut s, 60);
        assert_eq!(render(&mut s, 16000, 127), expected);
        s.event(NoteEvent::Off { key: 60, velocity: 0. });
        set(&mut s, "release", 0.);
        assert!(render(&mut s, 16, 3).iter().all(|v| *v == 0.));
    }

    #[test]
    fn reverse_changes_held_notes_and_glide_starts_from_the_previous_pitch() {
        let mut s = Sampler::new();
        s.load_sample(48000, 1, (0..100).map(|i| i as f32 / 100.).collect::<Vec<_>>().into());
        for (id, v) in [("attack", 0.), ("decay", 0.), ("sustain", 1.), ("poly", 1.)] {
            set(&mut s, id, v);
        }
        note(&mut s, 60);
        render(&mut s, 4, 1);
        set(&mut s, "reverse", 1.);
        assert_eq!(render(&mut s, 3, 1), [0.04, 0.04, 0.03, 0.03, 0.02, 0.02]);
        s.reset();
        set(&mut s, "reverse", 0.);
        set(&mut s, "glide", 0.1);
        note(&mut s, 60);
        render(&mut s, 4, 1);
        note(&mut s, 72);
        let output = render(&mut s, 3, 1);
        assert!((output[0] - 0.03).abs() < 1e-6, "{output:?}");
        assert!(s.sample_position().is_some_and(|p| p > 0.03 && p < 0.04));
    }

    #[test]
    fn loop_modes_keep_playing_and_empty_source_is_silent() {
        for reverse in [0., 1.] {
            for ping in [0., 1.] {
                let mut s = loaded();
                for (id, value) in [
                    ("reverse", reverse),
                    ("ping_pong", ping),
                    ("playback", 2.),
                    ("loop_start", 0.2),
                    ("loop_end", 0.3),
                ] {
                    set(&mut s, id, value);
                }
                note(&mut s, 60);
                let out = render(&mut s, 96000, 128);
                assert!(out.iter().all(|v| v.is_finite()));
                assert!(out[180000..].iter().any(|v| v.abs() > 0.01));
            }
        }
        let mut s = Sampler::new();
        note(&mut s, 60);
        assert!(render(&mut s, 64, 8).iter().all(|v| *v == 0.));
    }

    #[test]
    fn fractional_ping_pong_never_reads_past_the_region_boundary() {
        let mut s = Sampler::new();
        s.load_sample(48000, 1, vec![1.; 100].into());
        for (id, v) in
            [("attack", 0.), ("decay", 0.), ("sustain", 1.), ("playback", 2.), ("ping_pong", 1.), ("pitch", 7.)]
        {
            set(&mut s, id, v);
        }
        note(&mut s, 60);
        assert!(render(&mut s, 1000, 17).iter().all(|v| *v == 1.));
    }
}

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
