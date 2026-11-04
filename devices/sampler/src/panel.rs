// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The sampler's panel: mode buttons beside the waveform, a row of the mode's source controls
//! under it and a row of sound controls under that. The Controls page puts the LFO and the
//! envelopes in the waveform's place, More the rest in the rows' place; the pages, Input and
//! Record are in the device's header. Geometry is authored here; the typed SDK widgets take
//! their paint from the theme and user CSS.
use super::*;

/// The source row, under the waveform.
const SOURCE_ROW: f32 = 100.;
/// The sound row: its captions, then its controls.
const SOUND_ROW: f32 = 120.;
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
    label(ui, caption, [x, SOURCE_ROW + 2., caption_width, 16.], None);
    visible(ui, when.clone());
    control(ui, id, Widget::Menu, [x + caption_width, SOURCE_ROW, width, 18.], UiGesture::Menu, None);
    visible(ui, when);
}

fn dial(ui: &mut DeviceUi, id: &str, caption: &str, x: f32, width: f32, when: UiValue) {
    // 14 px holds an 11 px caption's descenders.
    label(ui, caption, [x, SOUND_ROW, width, 14.], None);
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
        // Tall enough for a ring over its readout, clear of the caption above it.
        [x, SOUND_ROW + 14., width, 34.],
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
    let y = SOUND_ROW + 2.;
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

/// A section's background, with its title when it has one, under the controls drawn after it.
fn section(ui: &mut DeviceUi, title: &str, bounds: [f32; 4], when: UiValue) {
    control(ui, "section", Widget::Panel { label: title.into() }, bounds, UiGesture::Drag, None);
    visible(ui, when);
    if let Some(c) = ui.controls.last_mut() {
        c.x = None;
        c.name = format!("{title} section");
    }
}

/// A line between two groups of a row.
fn divider(ui: &mut DeviceUi, x: f32, y: f32, when: UiValue) {
    // Eight pixels wide, the least a control is; the line is drawn at its middle.
    control(ui, "divider", Widget::Separator, [x - 4., y, 8., 18.], UiGesture::Drag, None);
    visible(ui, when);
    if let Some(c) = ui.controls.last_mut() {
        c.x = None;
    }
}

/// The Controls page: both its pages' values in place of the waveform.
fn controls_page() -> UiValue {
    view(1.) + view(2.)
}

/// An envelope on the Controls page: its section, its values two by two and its graph under them.
fn envelope(ui: &mut DeviceUi, title: &str, x: f32, ids: &[(&str, &str)], graph: Widget, name: &str) {
    let when = controls_page();
    section(ui, title, [x, 0., 186., 96.], when.clone());
    for (i, (id, caption)) in ids.iter().enumerate() {
        value(ui, id, caption, [x + 4. + (i % 2) as f32 * 90., 14. + (i / 2) as f32 * 20., 88., 18.], None);
        visible(ui, when.clone());
    }
    control(ui, name, graph, [x + 4., 56., 178., 36.], UiGesture::Position, None);
    visible(ui, when);
    if let Some(c) = ui.controls.last_mut() {
        c.id = name.into();
        c.x = None;
    }
}

/// The device header's controls: Input and Record, and the pages.
pub(super) fn header() -> DeviceUi {
    let mut ui = DeviceUi {
        // The header row's whole height, so the Input menu and the buttons are not cut.
        size: [HEADER_WIDTH, 20.],
        // Clear, so the header row stays one colour behind its controls.
        background: [0, 0, 0, 0],
        hidden_parameters: PARAMS.iter().map(|p| p.id.into()).collect(),
        ..DeviceUi::default()
    };
    control(&mut ui, "recording", Widget::SampleRecording, [0., 0., 200., 20.], UiGesture::Drag, None);
    if let Some(c) = ui.controls.last_mut() {
        c.x = None;
    }
    // Sample shows the waveform; Controls and More are pages a second click closes again.
    for (caption, value, x, w) in [("Sample", 0, 208., 62.), ("Controls", 1, 274., 70.), ("More", 3, 348., 52.)] {
        let gesture = if value == 0 { UiGesture::Set(0) } else { UiGesture::Tab(value) };
        button(&mut ui, "panel_view", caption, [x, 1., w, 18.], gesture, None);
    }
    ui
}

/// How wide the header's controls are.
const HEADER_WIDTH: f32 = 400.;

pub(super) fn panel() -> DeviceUi {
    let mut ui = DeviceUi {
        size: [900., 168.],
        viewport_size: Some([900., 168.]),
        hidden_parameters: PARAMS.iter().map(|p| p.id.into()).collect(),
        ..DeviceUi::default()
    };
    // Section backgrounds first, under the controls: the sound row's filter, LFO and voice.
    let rows = UiValue::from(1.) + view(3.) * (-1f32).into();
    for bounds in
        [[LEFT - 2., SOUND_ROW - 2., 250., 50.], [314., SOUND_ROW - 2., 150., 50.], [466., SOUND_ROW - 2., 320., 50.]]
    {
        section(&mut ui, "", bounds, rows.clone());
    }
    // Sample, Slice and Grain beside the waveform; Slice keeps its value 2 after Grain's 1.
    for (i, (name, mode)) in [("Sample", 0), ("Slice", 2), ("Grain", 1)].iter().enumerate() {
        button(&mut ui, "mode", name, [0., i as f32 * 57., 56., 54.], UiGesture::Set(*mode), None);
    }
    // Input and Record are in the header; this hidden one tells the waveform not to draw its own.
    control(&mut ui, "recording", Widget::SampleRecording, [LEFT, 0., 200., 18.], UiGesture::Drag, None);
    visible(&mut ui, UiValue::from(0.));
    if let Some(c) = ui.controls.last_mut() {
        c.x = None;
    }
    control(&mut ui, "sample", Widget::SampleWaveform, [LEFT, 0., 900. - LEFT, 96.], UiGesture::Position, None);
    visible(&mut ui, UiValue::from(1.) + controls_page() * (-1f32).into());
    if let Some(c) = ui.controls.last_mut() {
        c.x = None;
    }

    // The source row and the sound row stay on every page but More.
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
    divider(&mut ui, 494., SOURCE_ROW, source_row.clone());
    divider(&mut ui, 672., SOURCE_ROW, source_row.clone());

    // Voices: one, several or (slices) through to End, and how many.
    label(&mut ui, "Voices", [498., SOURCE_ROW + 2., 40., 16.], None);
    visible(&mut ui, source_row.clone());
    control(&mut ui, "slice_overlap", Widget::Menu, [538., SOURCE_ROW, 76., 18.], UiGesture::Menu, None);
    visible(&mut ui, row(slice.clone()));
    control(&mut ui, "voice_mode", Widget::Menu, [538., SOURCE_ROW, 76., 18.], UiGesture::Menu, None);
    visible(&mut ui, row(sample.clone() + grain.clone()));
    value(&mut ui, "poly", "Max", [618., SOURCE_ROW, 52., 18.], None);
    visible(&mut ui, source_row.clone());
    button(&mut ui, "warp", "Warp", [674., SOURCE_ROW, 44., 18.], UiGesture::Toggle, None);
    visible(&mut ui, source_row.clone());
    control(&mut ui, "warp_mode", Widget::Menu, [722., SOURCE_ROW, 86., 18.], UiGesture::Menu, None);
    visible(&mut ui, source_row.clone());
    value(&mut ui, "source_bpm", "Source", [812., SOURCE_ROW, 88., 18.], None);
    visible(&mut ui, source_row.clone());

    // The sound row: the filter, the LFO's shape and rate, the voice's knobs and its buttons.
    let sound = source_row.clone();
    label(&mut ui, "Filter", [LEFT, SOUND_ROW, 60., 14.], None);
    visible(&mut ui, sound.clone());
    control(&mut ui, "filter", Widget::Menu, [LEFT, SOUND_ROW + 22., 72., 18.], UiGesture::Menu, None);
    visible(&mut ui, sound.clone());
    control(&mut ui, "filter_slope", Widget::Menu, [LEFT + 74., SOUND_ROW + 22., 70., 18.], UiGesture::Menu, None);
    visible(&mut ui, sound.clone());
    dial(&mut ui, "cutoff", "Cutoff", 212., 50., sound.clone());
    dial(&mut ui, "resonance", "Res", 264., 44., sound.clone());
    rates(&mut ui, 318., sound.clone());

    let controls = sound.clone();
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
    button(&mut ui, "reverse", "Reverse", [790., SOUND_ROW + 2., 110., 18.], UiGesture::Toggle, None);
    visible(&mut ui, controls.clone());
    button(&mut ui, "retrigger", "Retrigger", [790., SOUND_ROW + 24., 110., 18.], UiGesture::Toggle, None);
    visible(&mut ui, controls);

    // Controls: in the waveform's place, the LFO's shape drawn and how deep it moves pitch,
    // filter, volume and pan, then the amplitude and filter envelopes (attack, decay, sustain,
    // release) and the pitch envelope (attack, decay, amount).
    section(&mut ui, "LFO", [LEFT, 0., 268., 96.], controls_page());
    control(
        &mut ui,
        "lfo_rate",
        Widget::WaveformPreview { shape: "lfo_shape".into() },
        [LEFT + 4., 14., 150., 78.],
        UiGesture::Position,
        None,
    );
    visible(&mut ui, controls_page());
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
        value(&mut ui, id, caption, [LEFT + 160., 14. + i as f32 * 20., 104., 18.], None);
        visible(&mut ui, controls_page());
    }
    let adsr = |prefix: &str| -> [String; 4] {
        ["attack", "decay", "sustain", "release"].map(|stage| format!("{prefix}{stage}"))
    };
    for (title, x, [a, d, s, r]) in [("Amplitude", 334., adsr("")), ("Filter", 524., adsr("filter_"))] {
        let graph =
            Widget::Envelope { attack: a.clone(), decay: d.clone(), level: s.clone(), release: Some(r.clone()) };
        let ids = [(a.as_str(), "A"), (d.as_str(), "D"), (s.as_str(), "S"), (r.as_str(), "R")];
        envelope(&mut ui, title, x, &ids, graph, &format!("{}_envelope", if x < 500. { "AMP" } else { "FILTER" }));
    }
    envelope(
        &mut ui,
        "Pitch",
        714.,
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
    let cell = |i: usize| [LEFT + (i % 7) as f32 * 120., SOURCE_ROW + (i / 7) as f32 * 22., 116., 18.];
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
