// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Compact native strip. Geometry is authored here; typed SDK widgets inherit CSS.
use super::*;

fn view(n: f32) -> UiValue {
    equals("panel_view", n)
}

fn visible(ui: &mut DeviceUi, condition: UiValue) {
    if let Some(c) = ui.controls.last_mut() {
        c.visible_when = Some(condition);
    }
}

fn dial(ui: &mut DeviceUi, id: &str, caption: &str, x: f32, condition: UiValue) {
    label(ui, caption, [x, 122., 54., 13.], None);
    visible(ui, condition.clone());
    let format = match id {
        "cutoff" => TextFormat::Frequency,
        "gain" => TextFormat::Decibels,
        "velocity" => TextFormat::Percent,
        "pan" => TextFormat::Balance,
        _ => TextFormat::Number,
    };
    control(
        ui,
        id,
        Widget::Dial { label: String::new(), decimals: usize::from(id == "resonance"), format },
        [x, 135., 54., 33.],
        UiGesture::Drag,
        None,
    );
    visible(ui, condition);
    if let Some(c) = ui.controls.last_mut() {
        c.y = c.x.take();
        if let Some(b) = &mut c.y {
            b.logarithmic = id == "cutoff";
            if matches!(id, "cutoff" | "resonance") {
                b.drag_scale = 2.;
            }
        }
    }
}

fn rates(ui: &mut DeviceUi, x: f32, y: f32, condition: UiValue) {
    control(ui, "lfo_shape", Widget::WaveformMenu, [x, y, 80., 18.], UiGesture::Menu, None);
    visible(ui, condition.clone());
    control(ui, "lfo_rate_mode", Widget::Menu, [x + 84., y, 62., 18.], UiGesture::Menu, None);
    visible(ui, condition.clone());
    for mode in 0..6 {
        let id = if mode == 0 {
            "lfo_rate"
        } else if mode == 1 {
            "lfo_time"
        } else {
            "lfo_division"
        };
        if mode < 2 {
            value(ui, id, "", [x, y + 22., 146., 20.], None);
            if let Some(c) = ui.controls.last_mut()
                && let Some(Widget::Number { bordered, suffix, .. }) = &mut c.widget
            {
                *bordered = true;
                if mode == 1 {
                    *suffix = " ms".into();
                }
            }
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
            control(ui, id, Widget::ValueMenu { choices }, [x, y + 22., 146., 20.], UiGesture::Menu, None);
        }
        visible(ui, condition.clone() * equals("lfo_rate_mode", mode as f32));
    }
}

pub(super) fn panel() -> DeviceUi {
    let mut ui = DeviceUi {
        size: [900., 168.],
        viewport_size: Some([900., 168.]),
        hidden_parameters: PARAMS.iter().map(|p| p.id.into()).collect(),
        ..DeviceUi::default()
    };
    // No literal palette: the active theme and user CSS own every widget's paint.
    for (i, (name, mode)) in [("Sample", 0), ("Slice", 2), ("Grain", 1)].iter().enumerate() {
        button(&mut ui, "mode", name, [0., 22. + i as f32 * 48., 60., 46.], UiGesture::Set(*mode), None);
    }
    control(&mut ui, "grid_division", Widget::Menu, [64., 0., 80., 18.], UiGesture::Menu, None);
    button(&mut ui, "grid_snap", "Snap", [148., 0., 46., 18.], UiGesture::Toggle, None);
    value(&mut ui, "first_beat", "First beat", [198., 0., 112., 18.], None);
    control(&mut ui, "recording", Widget::SampleRecording, [314., 0., 316., 18.], UiGesture::Drag, None);
    if let Some(c) = ui.controls.last_mut() {
        c.x = None;
    }
    for (caption, value, x, w) in
        [("Sample", 0, 638., 56.), ("LFO", 2, 698., 48.), ("Envelopes", 1, 750., 86.), ("More", 3, 840., 60.)]
    {
        let gesture = if value == 0 { UiGesture::Set(0) } else { UiGesture::Tab(value) };
        button(&mut ui, "panel_view", caption, [x, 0., w, 18.], gesture, None);
    }
    control(&mut ui, "sample", Widget::SampleWaveform, [64., 22., 836., 78.], UiGesture::Position, None);
    if let Some(c) = ui.controls.last_mut() {
        c.x = None;
    }
    let normal = view(0.);
    let source_row = view(0.) + view(2.);
    let slice = equals("mode", 2.) + equals("mode", 0.) * equals("playback", 3.);
    let sample = equals("mode", 0.) * (UiValue::from(1.) + equals("playback", 3.) * (-1f32).into());
    let grain = equals("mode", 1.);
    // The source row stays in place when the LFO editor replaces the lower controls.
    for (id, x, w, condition) in [
        ("playback", 64., 106., sample.clone()),
        ("slice_release", 64., 106., slice.clone()),
        ("slice_by", 176., 96., slice.clone()),
        ("sensitivity", 278., 100., slice.clone() * equals("slice_by", 0.)),
        ("slice_division", 278., 100., slice.clone() * equals("slice_by", 1.)),
        ("slice_regions", 278., 100., slice.clone() * equals("slice_by", 2.)),
        ("slice_overlap", 384., 78., slice.clone()),
        ("voice_mode", 384., 78., sample.clone() + grain.clone()),
    ] {
        if matches!(id, "sensitivity" | "slice_regions") {
            value(&mut ui, id, if id == "sensitivity" { "Sensitivity" } else { "Regions" }, [x, 102., w, 18.], None);
        } else {
            control(&mut ui, id, Widget::Menu, [x, 102., w, 18.], UiGesture::Menu, None);
        }
        visible(&mut ui, source_row.clone() * condition);
    }
    for (id, caption, x) in [("start", "Start", 176.), ("end", "End", 278.)] {
        value(&mut ui, id, caption, [x, 102., 96., 18.], None);
        visible(&mut ui, source_row.clone() * sample.clone());
    }
    for (id, caption, x) in [("position", "Position", 64.), ("scan", "Scan", 176.), ("size", "Size ms", 278.)] {
        value(&mut ui, id, caption, [x, 102., 96., 18.], None);
        visible(&mut ui, source_row.clone() * grain.clone());
    }
    value(&mut ui, "poly", "Voices", [468., 102., 76., 18.], None);
    visible(&mut ui, source_row.clone());
    button(&mut ui, "warp", "Warp", [550., 102., 52., 18.], UiGesture::Toggle, None);
    visible(&mut ui, source_row.clone());
    control(&mut ui, "warp_mode", Widget::Menu, [608., 102., 122., 18.], UiGesture::Menu, None);
    visible(&mut ui, source_row.clone());
    value(&mut ui, "source_bpm", "Source", [736., 102., 164., 18.], None);
    visible(&mut ui, source_row);

    let common = normal.clone() + view(2.);
    control(&mut ui, "filter", Widget::Menu, [64., 124., 100., 18.], UiGesture::Menu, None);
    visible(&mut ui, common.clone());
    control(&mut ui, "filter_slope", Widget::Menu, [64., 146., 100., 18.], UiGesture::Menu, None);
    visible(&mut ui, common.clone());
    dial(&mut ui, "cutoff", "Cutoff", 170., common.clone());
    dial(&mut ui, "resonance", "Res", 228., common);
    rates(&mut ui, 294., 124., normal.clone() + view(2.));
    for (id, caption, x) in [
        ("fade_in", "Fade in ms", 454.),
        ("fade_out", "Fade out ms", 516.),
        ("pitch", "Pitch", 578.),
        ("velocity", "Vel", 640.),
        ("gain", "Volume", 702.),
        ("pan", "Pan", 764.),
    ] {
        dial(&mut ui, id, caption, x, normal.clone());
    }
    button(&mut ui, "reverse", "Reverse", [826., 124., 74., 18.], UiGesture::Toggle, None);
    visible(&mut ui, normal.clone());
    button(&mut ui, "retrigger", "Retrigger", [826., 146., 74., 18.], UiGesture::Toggle, None);
    visible(&mut ui, normal);
    control(
        &mut ui,
        "lfo_rate",
        Widget::WaveformPreview { shape: "lfo_shape".into() },
        [454., 124., 220., 42.],
        UiGesture::Position,
        None,
    );
    visible(&mut ui, view(2.));
    if let Some(c) = ui.controls.last_mut() {
        c.id = "lfo_preview".into();
        if let Some(b) = &mut c.x {
            b.logarithmic = true;
        }
    }
    for (i, (id, caption)) in
        [("lfo_pitch", "Pitch st"), ("lfo_filter", "Filter oct"), ("lfo_volume", "Volume"), ("lfo_pan", "Pan")]
            .iter()
            .enumerate()
    {
        value(&mut ui, id, caption, [682. + (i % 2) as f32 * 110., 124. + (i / 2) as f32 * 22., 106., 20.], None);
        visible(&mut ui, view(2.));
    }
    // Extra controls stay accessible without making the host row taller.
    for (i, (id, caption)) in [
        ("cents", "Cents"),
        ("root", "Root"),
        ("key_low", "Key min"),
        ("key_high", "Key max"),
        ("velocity_low", "Vel min"),
        ("velocity_high", "Vel max"),
        ("glide", "Glide"),
        ("start", "Start"),
        ("end", "End"),
        ("loop_start", "Loop start"),
        ("loop_end", "Loop end"),
        ("density", "Grains/s"),
        ("jitter", "Jitter"),
        ("spread", "Spread"),
    ]
    .iter()
    .enumerate()
    {
        value(&mut ui, id, caption, [64. + (i % 5) as f32 * 168., 104. + (i / 5) as f32 * 22., 160., 20.], None);
        visible(&mut ui, view(3.));
    }
    button(&mut ui, "freeze", "Freeze", [736., 148., 160., 20.], UiGesture::Toggle, None);
    visible(&mut ui, view(3.));
    for (column, (prefix, title)) in [("AMP", "Amplitude"), ("FILTER", "Filter")].iter().enumerate() {
        let start = ui.controls.len();
        adsr(&mut ui, prefix, title, 0.);
        for c in &mut ui.controls[start..] {
            c.bounds[0] = 64. + column as f32 * 280. + c.bounds[0] * 0.92;
            c.bounds[2] *= 0.92;
            c.bounds[1] = match c.bounds[1] {
                86. => 102.,
                104. => 116.,
                _ => 134.,
            };
            if matches!(c.widget, Some(Widget::Envelope { .. })) {
                c.bounds[3] = 34.;
            } else if c.bounds[1] == 116. {
                c.bounds[3] = 16.;
            }
        }
    }
    label(&mut ui, "Pitch", [632., 102., 68., 14.], Some(1));
    for (id, caption, x) in [("pitch_attack", "A", 632.), ("pitch_decay", "D", 720.), ("pitch_env", "Amt", 808.)] {
        value(&mut ui, id, caption, [x, 116., 86., 16.], Some(1));
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
        [632., 134., 260., 34.],
        UiGesture::Position,
        Some(1),
    );
    if let Some(c) = ui.controls.last_mut() {
        c.id = "PITCH_envelope".into();
        c.x = None;
    }
    for (id, caption, x) in [
        ("velocity", "Vel", 190.),
        ("filter_env", "Amt", 404.),
        ("velocity_filter", "Vel", 476.),
        ("key_filter", "Key", 548.),
    ] {
        value(&mut ui, id, caption, [x, 102., 70., 14.], Some(1));
    }
    // Stable names let user CSS address controls without depending on insertion order.
    let mut counts = std::collections::BTreeMap::<String, usize>::new();
    for c in &mut ui.controls {
        if matches!(c.widget, Some(Widget::Envelope { .. })) || c.id == "lfo_preview" {
            continue;
        }
        let base = c.x.as_ref().or(c.y.as_ref()).map_or_else(|| c.name.replace(' ', "_"), |b| b.parameter.clone());
        let index = counts.entry(base.clone()).or_default();
        c.id = if *index == 0 { base } else { format!("{base}_{index}") };
        *index += 1;
    }
    ui
}
