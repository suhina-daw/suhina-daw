// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The sampler's panel: mode buttons beside the waveform, a row of the mode's source controls
//! under it, and a row of sound controls the LFO, Envelopes and More pages replace. Geometry is
//! authored here; the typed SDK widgets take their paint from the theme and user CSS.
use super::*;

/// The waveform and the mode buttons beside it start under the top row.
const TOP: f32 = 22.;
/// The source row, under the waveform.
const SOURCE_ROW: f32 = 108.;
/// The sound row: its captions, then its controls.
const SOUND_ROW: f32 = 128.;
/// Where everything right of the mode buttons starts.
const LEFT: f32 = 62.;

fn view(n: f32) -> UiValue {
    equals("panel_view", n)
}

fn visible(ui: &mut DeviceUi, condition: UiValue) {
    if let Some(c) = ui.controls.last_mut() {
        c.visible_when = Some(condition);
    }
}

/// A caption, and the menu it names right of it, on the source row.
fn named_menu(ui: &mut DeviceUi, id: &str, caption: &str, x: f32, caption_width: f32, width: f32, when: UiValue) {
    label(ui, caption, [x, SOURCE_ROW + 3., caption_width, 13.], None);
    visible(ui, when.clone());
    control(ui, id, Widget::Menu, [x + caption_width, SOURCE_ROW, width, 18.], UiGesture::Menu, None);
    visible(ui, when);
}

fn dial(ui: &mut DeviceUi, id: &str, caption: &str, x: f32, width: f32, when: UiValue) {
    label(ui, caption, [x, SOUND_ROW, width, 12.], None);
    visible(ui, when.clone());
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
        [x, SOUND_ROW + 12., width, 28.],
        UiGesture::Drag,
        None,
    );
    visible(ui, when);
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

/// The LFO's shape and rate: the shape and the rate's unit on a line, the rate under them.
fn rates(ui: &mut DeviceUi, x: f32, when: UiValue) {
    let y = SOUND_ROW;
    control(ui, "lfo_shape", Widget::WaveformMenu, [x, y, 80., 18.], UiGesture::Menu, None);
    visible(ui, when.clone());
    control(ui, "lfo_rate_mode", Widget::Menu, [x + 84., y, 58., 18.], UiGesture::Menu, None);
    visible(ui, when.clone());
    for mode in 0..6 {
        let id = match mode {
            0 => "lfo_rate",
            1 => "lfo_time",
            _ => "lfo_division",
        };
        let bounds = [x, y + 22., 142., 18.];
        if mode < 2 {
            value(ui, id, "", bounds, None);
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
            control(ui, id, Widget::ValueMenu { choices }, bounds, UiGesture::Menu, None);
        }
        visible(ui, when.clone() * equals("lfo_rate_mode", mode as f32));
    }
}

/// An envelope's caption, its values on a line beside it and its graph under them.
fn envelope(ui: &mut DeviceUi, title: &str, x: f32, ids: &[(&str, &str)], graph: Widget, name: &str) {
    let when = view(1.);
    label(ui, title, [x, SOUND_ROW, 62., 13.], None);
    visible(ui, when.clone());
    for (i, (id, caption)) in ids.iter().enumerate() {
        value(ui, id, caption, [x + 64. + i as f32 * 52., SOUND_ROW - 2., 50., 16.], None);
        visible(ui, when.clone());
    }
    control(ui, name, graph, [x, SOUND_ROW + 16., 272., 24.], UiGesture::Position, None);
    visible(ui, when);
    if let Some(c) = ui.controls.last_mut() {
        c.id = name.into();
        c.x = None;
    }
}

pub(super) fn panel() -> DeviceUi {
    let mut ui = DeviceUi {
        size: [900., 168.],
        viewport_size: Some([900., 168.]),
        hidden_parameters: PARAMS.iter().map(|p| p.id.into()).collect(),
        ..DeviceUi::default()
    };
    // Sample, Slice and Grain beside the waveform; Slice keeps its value 2 after Grain's 1.
    for (i, (name, mode)) in [("Sample", 0), ("Slice", 2), ("Grain", 1)].iter().enumerate() {
        button(&mut ui, "mode", name, [0., TOP + i as f32 * 48., 56., 46.], UiGesture::Set(*mode), None);
    }
    control(&mut ui, "recording", Widget::SampleRecording, [LEFT, 0., 316., 18.], UiGesture::Drag, None);
    if let Some(c) = ui.controls.last_mut() {
        c.x = None;
    }
    // The pages: Sample is the controls; clicking an open page again goes back to them.
    for (caption, value, x, w) in
        [("Sample", 0, 628., 62.), ("LFO", 2, 694., 48.), ("Envelopes", 1, 746., 82.), ("More", 3, 832., 68.)]
    {
        let gesture = if value == 0 { UiGesture::Set(0) } else { UiGesture::Tab(value) };
        button(&mut ui, "panel_view", caption, [x, 0., w, 18.], gesture, None);
    }
    control(&mut ui, "sample", Widget::SampleWaveform, [LEFT, TOP, 900. - LEFT, 82.], UiGesture::Position, None);
    if let Some(c) = ui.controls.last_mut() {
        c.x = None;
    }

    // The source row stays on every page but More.
    let source_row = view(0.) + view(1.) + view(2.);
    let slice = equals("mode", 2.) + equals("mode", 0.) * equals("playback", 3.);
    let sample = equals("mode", 0.) * (UiValue::from(1.) + equals("playback", 3.) * (-1f32).into());
    let grain = equals("mode", 1.);
    let row = |when: UiValue| source_row.clone() * when;

    named_menu(&mut ui, "slice_release", "Note end", LEFT, 50., 104., row(slice.clone()));
    named_menu(&mut ui, "slice_by", "Slice by", 224., 46., 92., row(slice.clone()));
    value(&mut ui, "sensitivity", "Sensitivity", [370., SOURCE_ROW, 120., 18.], None);
    visible(&mut ui, row(slice.clone() * equals("slice_by", 0.)));
    named_menu(&mut ui, "slice_division", "Division", 370., 50., 70., row(slice.clone() * equals("slice_by", 1.)));
    value(&mut ui, "slice_regions", "Regions", [370., SOURCE_ROW, 120., 18.], None);
    visible(&mut ui, row(slice.clone() * equals("slice_by", 2.)));

    named_menu(&mut ui, "playback", "Playback", LEFT, 50., 104., row(sample.clone()));
    for (id, caption, x) in [("start", "Start", 224.), ("end", "End", 316.)] {
        value(&mut ui, id, caption, [x, SOURCE_ROW, 88., 18.], None);
        visible(&mut ui, row(sample.clone()));
    }

    for (id, caption, x, w) in
        [("position", "Position", LEFT, 100.), ("scan", "Scan", 166., 80.), ("size", "Size ms", 250., 110.)]
    {
        value(&mut ui, id, caption, [x, SOURCE_ROW, w, 18.], None);
        visible(&mut ui, row(grain.clone()));
    }

    // Voices: one, several or (slices) through to End, and how many.
    label(&mut ui, "Voices", [498., SOURCE_ROW + 3., 40., 13.], None);
    visible(&mut ui, source_row.clone());
    control(&mut ui, "slice_overlap", Widget::Menu, [538., SOURCE_ROW, 76., 18.], UiGesture::Menu, None);
    visible(&mut ui, row(slice.clone()));
    control(&mut ui, "voice_mode", Widget::Menu, [538., SOURCE_ROW, 76., 18.], UiGesture::Menu, None);
    visible(&mut ui, row(sample.clone() + grain.clone()));
    value(&mut ui, "poly", "", [618., SOURCE_ROW, 36., 18.], None);
    visible(&mut ui, source_row.clone());
    button(&mut ui, "warp", "Warp", [662., SOURCE_ROW, 46., 18.], UiGesture::Toggle, None);
    visible(&mut ui, source_row.clone());
    control(&mut ui, "warp_mode", Widget::Menu, [712., SOURCE_ROW, 92., 18.], UiGesture::Menu, None);
    visible(&mut ui, source_row.clone());
    value(&mut ui, "source_bpm", "Source", [808., SOURCE_ROW, 92., 18.], None);
    visible(&mut ui, source_row);

    // The sound row: the filter and the LFO's rate stay on the LFO page.
    let sound = view(0.) + view(2.);
    label(&mut ui, "Filter", [LEFT, SOUND_ROW, 60., 13.], None);
    visible(&mut ui, sound.clone());
    control(&mut ui, "filter", Widget::Menu, [LEFT, SOUND_ROW + 18., 72., 18.], UiGesture::Menu, None);
    visible(&mut ui, sound.clone());
    control(&mut ui, "filter_slope", Widget::Menu, [LEFT + 74., SOUND_ROW + 18., 70., 18.], UiGesture::Menu, None);
    visible(&mut ui, sound.clone());
    dial(&mut ui, "cutoff", "Cutoff", 212., 50., sound.clone());
    dial(&mut ui, "resonance", "Res", 264., 44., sound.clone());
    rates(&mut ui, 318., sound);

    let controls = view(0.);
    for (i, (id, caption)) in [
        ("fade_in", "Fade in"),
        ("fade_out", "Fade out"),
        ("pitch", "Pitch"),
        ("velocity", "Vel"),
        ("gain", "Volume"),
        ("pan", "Pan"),
    ]
    .iter()
    .enumerate()
    {
        dial(&mut ui, id, caption, 470. + i as f32 * 52., 50., controls.clone());
    }
    button(&mut ui, "reverse", "Reverse", [790., SOUND_ROW, 110., 18.], UiGesture::Toggle, None);
    visible(&mut ui, controls.clone());
    button(&mut ui, "retrigger", "Retrigger", [790., SOUND_ROW + 22., 110., 18.], UiGesture::Toggle, None);
    visible(&mut ui, controls);

    // LFO: its shape drawn, and how deep it moves pitch, filter, volume and pan.
    control(
        &mut ui,
        "lfo_rate",
        Widget::WaveformPreview { shape: "lfo_shape".into() },
        [470., SOUND_ROW, 200., 40.],
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
        let x = 678. + (i % 2) as f32 * 112.;
        value(&mut ui, id, caption, [x, SOUND_ROW + (i / 2) as f32 * 22., 108., 18.], None);
        visible(&mut ui, view(2.));
    }

    // Envelopes: amplitude and filter (attack, decay, sustain, release) and pitch (attack,
    // decay, amount), side by side in the sound row's place.
    let adsr = |prefix: &str| -> [String; 4] {
        ["attack", "decay", "sustain", "release"].map(|stage| format!("{prefix}{stage}"))
    };
    for (title, x, [a, d, s, r]) in [("Amplitude", LEFT, adsr("")), ("Filter", 342., adsr("filter_"))] {
        let graph =
            Widget::Envelope { attack: a.clone(), decay: d.clone(), level: s.clone(), release: Some(r.clone()) };
        let ids = [(a.as_str(), "A"), (d.as_str(), "D"), (s.as_str(), "S"), (r.as_str(), "R")];
        envelope(&mut ui, title, x, &ids, graph, &format!("{}_envelope", if x == LEFT { "AMP" } else { "FILTER" }));
    }
    envelope(
        &mut ui,
        "Pitch",
        622.,
        &[("pitch_attack", "A"), ("pitch_decay", "D"), ("pitch_env", "Amt")],
        Widget::Envelope {
            attack: "pitch_attack".into(),
            decay: "pitch_decay".into(),
            level: "pitch_env".into(),
            release: None,
        },
        "PITCH_envelope",
    );

    // More: the rest, in the source and sound rows' place.
    let more = view(3.);
    let cell = |i: usize| [LEFT + (i % 7) as f32 * 120., SOURCE_ROW + (i / 7) as f32 * 20., 116., 18.];
    let fields = [
        ("cents", "Cents"),
        ("root", "Root"),
        ("key_low", "Key min"),
        ("key_high", "Key max"),
        ("velocity_low", "Vel min"),
        ("velocity_high", "Vel max"),
        ("glide", "Glide"),
        ("loop_start", "Loop start"),
        ("loop_end", "Loop end"),
        ("crossfade", "Loop fade"),
        ("density", "Grains/s"),
        ("jitter", "Jitter"),
        ("spread", "Spread"),
        ("filter_env", "Filter env"),
        ("velocity_filter", "Vel > filter"),
        ("key_filter", "Key > filter"),
        ("first_beat", "First beat"),
    ];
    for (i, (id, caption)) in fields.iter().enumerate() {
        value(&mut ui, id, caption, cell(i), None);
        visible(&mut ui, more.clone());
    }
    control(&mut ui, "grid_division", Widget::Menu, cell(fields.len()), UiGesture::Menu, None);
    visible(&mut ui, more.clone());
    button(&mut ui, "grid_snap", "Snap", cell(fields.len() + 1), UiGesture::Toggle, None);
    visible(&mut ui, more.clone());
    button(&mut ui, "freeze", "Freeze", cell(fields.len() + 2), UiGesture::Toggle, None);
    visible(&mut ui, more);

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
