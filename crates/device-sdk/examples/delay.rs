// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Stereo delay with an embedded control panel.
use suhina_device_sdk::{
    DeviceGraph, DeviceRole, FilterMode, NodeKind as K, ParameterControl as C, Waveform,
    author::{GraphBuilder, NodeRef},
};
pub fn build() -> Result<DeviceGraph, Box<dyn std::error::Error>> {
    let mut g = GraphBuilder::new("org.example.delay", "Delay", DeviceRole::Effect);
    let input = g.node("in", K::AudioInput)?;
    let output = g.node("out", K::AudioOutput)?;
    let feedback = g.parameter_signal(
        Parameter::new("feedback", "feedback", (0.0, 95.0), 50.0).unit("%").control(C::Knob).smoothing_ms(0.),
    )?;
    let wet = g.parameter_signal(
        Parameter::new("wet", "wet", (0.0, 100.0), 50.0).unit("%").control(C::Knob).smoothing_ms(0.),
    )?;
    let time_left = g.parameter_signal(
        Parameter::new("time_left", "time_left", (1.0, 4000.0), 250.0)
            .unit("ms")
            .control(C::LogKnob)
            .smoothing_ms(0.)
            .linked("time_right", "link"),
    )?;
    let time_right = g.parameter_signal(
        Parameter::new("time_right", "time_right", (1.0, 4000.0), 250.0)
            .unit("ms")
            .control(C::LogKnob)
            .smoothing_ms(0.)
            .linked("time_left", "link"),
    )?;
    let sync_left = g.parameter_signal(
        Parameter::new("sync_left", "sync_left", (0.0, 1.0), 1.0)
            .unit("")
            .control(C::Toggle)
            .smoothing_ms(0.)
            .linked("sync_right", "link"),
    )?;
    let sync_right = g.parameter_signal(
        Parameter::new("sync_right", "sync_right", (0.0, 1.0), 1.0)
            .unit("")
            .control(C::Toggle)
            .smoothing_ms(0.)
            .linked("sync_left", "link"),
    )?;
    let division_left = g.parameter_signal(
        Parameter::new("division_left", "division_left", (1.0, 16.0), 3.0)
            .unit("")
            .control(C::Integer)
            .smoothing_ms(0.)
            .linked("division_right", "link"),
    )?;
    let division_right = g.parameter_signal(
        Parameter::new("division_right", "division_right", (1.0, 16.0), 3.0)
            .unit("")
            .control(C::Integer)
            .smoothing_ms(0.)
            .linked("division_left", "link"),
    )?;
    let offset_left = g.parameter_signal(
        Parameter::new("offset_left", "offset_left", (-33.3, 33.3), 0.0)
            .unit("%")
            .control(C::Knob)
            .smoothing_ms(0.)
            .linked("offset_right", "link"),
    )?;
    let offset_right = g.parameter_signal(
        Parameter::new("offset_right", "offset_right", (-33.3, 33.3), 0.0)
            .unit("%")
            .control(C::Knob)
            .smoothing_ms(0.)
            .linked("offset_left", "link"),
    )?;
    let link = g.parameter_signal(
        Parameter::new("link", "link", (0.0, 1.0), 1.0).unit("").control(C::Toggle).smoothing_ms(0.),
    )?;
    let filter_on = g.parameter_signal(
        Parameter::new("filter_on", "filter_on", (0.0, 1.0), 1.0).unit("").control(C::Toggle).smoothing_ms(0.),
    )?;
    let cutoff = g.parameter_signal(
        Parameter::new("cutoff", "cutoff", (50.0, 18000.0), 1000.0).unit("Hz").control(C::LogKnob).smoothing_ms(0.),
    )?;
    let width = g.parameter_signal(
        Parameter::new("width", "width", (0.5, 9.0), 8.0).unit("").control(C::Knob).smoothing_ms(0.),
    )?;
    let ping_pong = g.parameter_signal(
        Parameter::new("ping_pong", "ping_pong", (0.0, 1.0), 0.0).unit("").control(C::Toggle).smoothing_ms(0.),
    )?;
    let freeze = g.parameter_signal(
        Parameter::new("freeze", "freeze", (0.0, 1.0), 0.0).unit("").control(C::Toggle).smoothing_ms(0.),
    )?;
    let mode = g.parameter_signal(
        Parameter::new("mode", "mode", (0.0, 2.0), 0.0).unit("").control(C::Integer).smoothing_ms(0.),
    )?;
    let mod_delay = g.parameter_signal(
        Parameter::new("mod_delay", "mod_delay", (0.0, 100.0), 0.0).unit("%").control(C::Knob).smoothing_ms(0.),
    )?;
    let mod_filter = g.parameter_signal(
        Parameter::new("mod_filter", "mod_filter", (0.0, 100.0), 0.0).unit("%").control(C::Knob).smoothing_ms(0.),
    )?;
    let mod_rate = g.parameter_signal(
        Parameter::new("mod_rate", "mod_rate", (0.01, 20.0), 0.5).unit("Hz").control(C::LogKnob).smoothing_ms(0.),
    )?;
    let mod_shape = g.parameter_signal(
        Parameter::new("mod_shape", "mod_shape", (0.0, 6.0), 0.0)
            .unit("")
            .control(C::Choice)
            .smoothing_ms(0.)
            .options(["Sine", "Triangle", "Ramp Up", "Ramp Down", "Square", "S&H", "Wander"]),
    )?;
    let mod_rate_mode = g.parameter_signal(
        Parameter::new("mod_rate_mode", "mod_rate_mode", (0.0, 5.0), 0.0)
            .unit("")
            .control(C::Choice)
            .smoothing_ms(0.)
            .options(["Hz", "Time", "Synced", "Triplet", "Dotted", "16th steps"]),
    )?;
    let mod_time = g.parameter_signal(
        Parameter::new("mod_time", "mod_time", (1.0, 20000.0), 2000.0).unit("ms").control(C::LogKnob).smoothing_ms(0.),
    )?;
    let mod_division = g.parameter_signal(
        Parameter::new("mod_division", "mod_division", (1.0, 64.0), 4.0).unit("").control(C::Integer).smoothing_ms(0.),
    )?;
    let mod_morph = g.parameter_signal(
        Parameter::new("mod_morph", "mod_morph", (0.0, 100.0), 0.0).unit("%").control(C::Knob).smoothing_ms(0.),
    )?;
    g.parameter(
        Parameter::new("lfo_expanded", "LFO details", (0.0, 1.0), 1.0).unit("").control(C::Toggle).smoothing_ms(0.),
    )?;
    let zero = g.constant("zero", 0.0)?;
    let one = g.constant("one", 1.0)?;
    let two = g.constant("two", 2.0)?;
    let four = g.constant("four", 4.0)?;
    let sixty = g.constant("sixty", 60.0)?;
    let percent = g.constant("percent", 0.01)?;
    let millisecond = g.constant("millisecond", 0.001)?;
    let tempo = g.op("tempo", K::Tempo, &[])?;
    let beat_time = g.binary("beat_time", K::Divide, sixty.out(), tempo.out())?;
    let sixteenth = g.binary("sixteenth", K::Divide, beat_time.out(), four.out())?;
    let timing_scales = [&millisecond, &sixteenth, &percent, &one];
    let base_left = timing(&mut g, "left", [&time_left, &division_left, &sync_left, &offset_left], timing_scales)?;
    let base_right =
        timing(&mut g, "right", [&time_right, &division_right, &sync_right, &offset_right], timing_scales)?;
    let right_time =
        g.op("right_time", K::Select, &[("a", base_right.out()), ("b", base_left.out()), ("condition", link.out())])?;
    let stereo_time = g.op("stereo_time", K::Stereo, &[("left", base_left.out()), ("right", right_time.out())])?;
    let mod_frequency = g.op(
        "mod_frequency",
        K::LfoRate,
        &[
            ("rate", mod_rate.out()),
            ("time", mod_time.out()),
            ("division", mod_division.out()),
            ("mode", mod_rate_mode.out()),
            ("tempo", tempo.out()),
        ],
    )?;
    let wave0 = g.op("wave0", K::Oscillator { shape: Waveform::Sine }, &[("frequency", mod_frequency.out())])?;
    let mut selected = wave0.clone();
    let mut random = wave0.clone();
    for (i, shape) in [Waveform::Triangle, Waveform::SawUp, Waveform::SawDown, Waveform::Square, Waveform::Random]
        .into_iter()
        .enumerate()
    {
        let index = i + 1;
        let wave = g.op(format!("wave{index}"), K::Oscillator { shape }, &[("frequency", mod_frequency.out())])?;
        if shape == Waveform::Random {
            random = wave.clone();
        }
        let threshold = g.constant(format!("shape_threshold{index}"), i as f64)?;
        let active = g.binary(format!("shape_active{index}"), K::Greater, mod_shape.out(), threshold.out())?;
        selected = g.op(
            format!("shape_mix{index}"),
            K::Select,
            &[("a", selected.out()), ("b", wave.out()), ("condition", active.out())],
        )?;
    }
    let wander_time = g.constant("wander_time", 0.2)?;
    let wander_threshold = g.constant("wander_threshold", 5.0)?;
    let wander = g.op("wander", K::Smooth, &[("in", random.out()), ("time", wander_time.out())])?;
    let wander_active = g.binary("wander_active", K::Greater, mod_shape.out(), wander_threshold.out())?;
    let selected_wave = g.op(
        "selected_wave",
        K::Select,
        &[("a", selected.out()), ("b", wander.out()), ("condition", wander_active.out())],
    )?;
    let morph_amount = g.binary("morph_amount", K::Multiply, mod_morph.out(), percent.out())?;
    let lfo = g.op("lfo", K::Mix, &[("a", selected_wave.out()), ("b", wave0.out()), ("amount", morph_amount.out())])?;
    let delay_amount = g.binary("delay_amount", K::Multiply, mod_delay.out(), percent.out())?;
    let delay_motion = g.binary("delay_motion", K::Multiply, lfo.out(), delay_amount.out())?;
    let delay_scale = g.binary("delay_scale", K::Add, one.out(), delay_motion.out())?;
    let delay_time = g.binary("delay_time", K::Multiply, stereo_time.out(), delay_scale.out())?;
    let filter_amount = g.binary("filter_amount", K::Multiply, mod_filter.out(), percent.out())?;
    let filter_motion = g.binary("filter_motion", K::Multiply, lfo.out(), filter_amount.out())?;
    let filter_octaves = g.binary("filter_octaves", K::Multiply, filter_motion.out(), four.out())?;
    let filter_scale = g.binary("filter_scale", K::Power, two.out(), filter_octaves.out())?;
    let filter_center = g.binary("filter_center", K::Multiply, cutoff.out(), filter_scale.out())?;
    let half_width = g.binary("half_width", K::Divide, width.out(), two.out())?;
    let band_ratio = g.binary("band_ratio", K::Power, two.out(), half_width.out())?;
    let low_edge = g.binary("low_edge", K::Divide, filter_center.out(), band_ratio.out())?;
    let high_edge = g.binary("high_edge", K::Multiply, filter_center.out(), band_ratio.out())?;
    let input_amount = g.binary("input_amount", K::Subtract, one.out(), freeze.out())?;
    let ping_input = g.op("ping_input", K::Stereo, &[("left", input.out()), ("right", zero.out())])?;
    let source =
        g.op("source", K::Select, &[("a", input.out()), ("b", ping_input.out()), ("condition", ping_pong.out())])?;
    let feed = g.binary("feed", K::Multiply, source.out(), input_amount.out())?;
    let sum = g.node("sum", K::Add)?;
    g.connect(feed.out(), sum.input("a"))?;
    let delay = g.op(
        "delay",
        K::Delay { max_seconds: 4.0 },
        &[("in", sum.out()), ("time", delay_time.out()), ("mode", mode.out())],
    )?;
    let highpass = g.op(
        "highpass",
        K::Filter { mode: FilterMode::Highpass },
        &[("in", delay.out()), ("frequency", low_edge.out())],
    )?;
    let lowpass = g.op(
        "lowpass",
        K::Filter { mode: FilterMode::Lowpass },
        &[("in", highpass.out()), ("frequency", high_edge.out())],
    )?;
    let filtered =
        g.op("filtered", K::Mix, &[("a", delay.out()), ("b", lowpass.out()), ("amount", filter_on.out())])?;
    let swapped = g.op("swapped", K::Stereo, &[("left", filtered.right()), ("right", filtered.left())])?;
    let return_audio = g.op(
        "return_audio",
        K::Select,
        &[("a", filtered.out()), ("b", swapped.out()), ("condition", ping_pong.out())],
    )?;
    let feedback_fraction = g.binary("feedback_fraction", K::Multiply, feedback.out(), percent.out())?;
    let wet_fraction = g.binary("wet_fraction", K::Multiply, wet.out(), percent.out())?;
    let feedback_amount = g.op(
        "feedback_amount",
        K::Select,
        &[("a", feedback_fraction.out()), ("b", one.out()), ("condition", freeze.out())],
    )?;
    let feedback_audio = g.binary("feedback_audio", K::Multiply, return_audio.out(), feedback_amount.out())?;
    let mix = g.op("mix", K::Mix, &[("a", input.out()), ("b", filtered.out()), ("amount", wet_fraction.out())])?;
    g.connect(feedback_audio.out(), sum.input("b"))?;
    g.connect(mix.out(), output.input("in"))?;
    let mut g = g.finish()?;
    g.ui = Some(build_panel(&g.parameters));
    finish(g)
}
fn timing(
    g: &mut GraphBuilder,
    side: &str,
    [time, division, sync, offset]: [&NodeRef; 4],
    [millisecond, sixteenth, percent, one]: [&NodeRef; 4],
) -> Result<NodeRef, suhina_device_sdk::DeviceSdkError> {
    let seconds = g.binary(format!("seconds_{side}"), K::Multiply, time.out(), millisecond.out())?;
    let sync_time = g.binary(format!("sync_time_{side}"), K::Multiply, sixteenth.out(), division.out())?;
    let chosen = g.op(
        format!("chosen_{side}"),
        K::Select,
        &[("a", seconds.out()), ("b", sync_time.out()), ("condition", sync.out())],
    )?;
    let fraction = g.binary(format!("offset_fraction_{side}"), K::Multiply, offset.out(), percent.out())?;
    let effective = g.binary(format!("effective_offset_{side}"), K::Multiply, fraction.out(), sync.out())?;
    let scale = g.binary(format!("offset_scale_{side}"), K::Add, one.out(), effective.out())?;
    g.binary(format!("base_{side}"), K::Multiply, chosen.out(), scale.out())
}

#[cfg(not(test))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    save(build()?, "delay")
}

use suhina_device_sdk::{Parameter, ui::*};
// Widget text is resolved against the host palette at render time.
const TEXT: [u8; 4] = [236, 232, 225, 255];
fn equal(id: &str, v: u16) -> UiValue {
    UiValue::parameter(id, false).equals(v as f32)
}
fn number(id: &str, bounds: [f32; 4], decimals: usize, log: bool, color: [u8; 4], center: bool) -> CustomControl {
    let mut control = CustomControl::number(id, bounds).logarithmic(log);
    control.widget = Some(Widget::Number {
        decimals,
        format: if id == "cutoff" { TextFormat::Frequency } else { TextFormat::Number },
        color,
        center,
        bordered: true,
        prefix: String::new(),
        suffix: String::new(),
    });
    control
}

fn build_panel(parameters: &[Parameter]) -> DeviceUi {
    let mut ui = DeviceUi {
        size: [944., 168.],
        viewport_size: Some([944., 168.]),
        hidden_parameters: parameters.iter().map(|p| p.id.clone()).collect(),
        ..DeviceUi::default()
    };
    for (label, x, width) in
        [("TIME", 14., 80.), ("FEEDBACK", 289., 74.), ("MIX", 872., 40.), ("TONE", 384., 80.), ("MOTION", 579., 90.)]
    {
        ui.controls.push(CustomControl::label(label, label, [x, 6., width, 16.]));
    }
    for x in [275., 367., 563., 851.] {
        ui.controls.push(CustomControl::separator(format!("divider_{x}"), [x - 4., 6., 8., 160.]));
    }
    for (side, x) in [("left", 14.), ("right", 164.)] {
        let title = if side == "left" { "Left" } else { "Right" };
        ui.controls.push(CustomControl::label(format!("{side}_label"), title, [x, 28., 100., 16.]));
        let sync = format!("sync_{side}");
        let mut timing_mode = CustomControl::toggle(&sync, "Beats", [x, 48., 42., 24.])
            .id(format!("{sync}_99"))
            .name(format!("{title} timing mode"))
            .tooltip("Choose beat-synchronized notes or milliseconds");
        timing_mode.widget =
            Some(Widget::Button { label: "Beats".into(), off_label: Some("ms".into()), indicator: false });
        ui.controls.push(timing_mode);
        let choices = (1..=16)
            .map(|steps| {
                let divisor = (1..=16).rev().find(|d| steps % d == 0 && 16 % d == 0).unwrap_or(1);
                (steps, format!("{}/{}", steps / divisor, 16 / divisor))
            })
            .collect();
        ui.controls.push(
            CustomControl::value_menu(format!("division_{side}"), choices, [x + 50., 48., 50., 24.])
                .name(format!("{title} delay"))
                .tooltip("Choose the delay length as a fraction of a whole note")
                .visible_when(equal(&sync, 1)),
        );
        ui.controls.push(
            number(&format!("time_{side}"), [x + 50., 48., 50., 24.], 0, true, TEXT, false)
                .visible_when(equal(&sync, 0)),
        );
        ui.controls.push(
            CustomControl::label(format!("{side}_offset_caption"), "Offset", [x, 80., 100., 16.])
                .visible_when(equal(&sync, 1)),
        );
        ui.controls.push(
            number(&format!("offset_{side}"), [x, 98., 100., 24.], 1, false, TEXT, false).visible_when(equal(&sync, 1)),
        );
    }
    ui.controls.push(
        CustomControl::toggle("link", "↔", [122., 48., 34., 24.])
            .id("link_99")
            .name("Link channels")
            .tooltip("Link both channels; enabling copies the left timing to the right"),
    );
    ui.controls.push(
        CustomControl::toggle("ping_pong", "Ping Pong", [864., 140., 72., 22.]).id(format!("{}_99", "ping_pong")),
    );
    ui.controls.push(CustomControl::toggle("freeze", "Freeze", [289., 140., 64., 22.]).id(format!("{}_99", "freeze")));
    for (i, label) in ["Repitch", "Fade", "Jump"].into_iter().enumerate() {
        ui.controls.push(CustomControl::set_button("mode", i as u16, label, [14. + i as f32 * 86., 140., 78., 22.]));
    }
    ui.controls.push(CustomControl::dial("feedback", "", [289., 48., 64., 86.]).name("Feedback"));
    ui.controls.push(CustomControl::dial("wet", "", [872., 48., 56., 86.]).name("Mix"));
    ui.controls
        .push(CustomControl::toggle("filter_on", "Filter", [384.0, 32., 51., 26.]).id(format!("{}_99", "filter_on")));
    ui.controls.push(number("cutoff", [444.0, 36., 56., 18.], 0, true, TEXT, false));
    let mut width = number("width", [504.0, 36., 46., 18.], 2, false, TEXT, false);
    if let Some(Widget::Number { prefix, .. }) = &mut width.widget {
        *prefix = "Oct: ".into();
    }
    ui.controls.push(width);
    ui.controls.push(CustomControl {
        widget: Some(Widget::FilterBand { min_hz: 50., max_hz: 18000. }),
        id: "filter_graph".into(),
        name: "Filter frequency and width".into(),
        tooltip: "Drag horizontally for frequency, vertically for width".into(),
        bounds: [384.0, 74., 166., 80.],
        visible_when: None,
        gesture: UiGesture::Position,
        x: Some(UiBinding { parameter: "cutoff".into(), logarithmic: true, drag_scale: 1. }),
        y: Some(UiBinding::new("width")),
        drawing: vec![],
    });
    ui.controls.push(CustomControl::label("motion_mode_caption", "Mode", [579., 28., 32., 16.]));
    ui.controls.push(
        CustomControl::menu("mod_rate_mode", [615., 28., 90., 20.])
            .id("mod_rate_mode_99")
            .name("Motion rate mode")
            .tooltip("mod_rate_mode: Rate"),
    );
    for (id, mode, decimals, log) in [("mod_rate", 0, 2, true), ("mod_time", 1, 0, true)] {
        ui.controls.push(
            number(id, [579., 50., 96., 20.], decimals, log, TEXT, false).visible_when(equal("mod_rate_mode", mode)),
        );
    }
    for mode in 2..=5 {
        let choices = (1..=64)
            .map(|value| {
                (
                    value,
                    match mode {
                        3 => format!("1/{value} T"),
                        4 => format!("1/{value} D"),
                        5 => format!("{value} x 1/16"),
                        _ => format!("1/{value}"),
                    },
                )
            })
            .collect();
        ui.controls.push(
            CustomControl::value_menu("mod_division", choices, [579., 50., 96., 20.])
                .id(format!("motion_division_{mode}"))
                .name("Motion rhythm")
                .tooltip("Choose the note length; T = triplet, D = dotted; 16th counts sixteenth-note steps")
                .visible_when(equal("mod_rate_mode", mode)),
        );
    }
    ui.controls.push(CustomControl::label("shape_caption", "Shape", [739., 28., 60., 16.]));
    ui.controls.push(CustomControl {
        widget: Some(Widget::WaveformMenu),
        id: "mod_shape".into(),
        name: "Motion waveform".into(),
        tooltip: "Choose the waveform".into(),
        bounds: [739., 46., 60., 22.],
        visible_when: None,
        gesture: UiGesture::Menu,
        x: Some(UiBinding::new("mod_shape")),
        y: None,
        drawing: vec![],
    });
    ui.controls.push(CustomControl::dial("mod_delay", "Delay\ndepth", [579.0, 78., 64., 86.]));
    ui.controls.push(CustomControl::dial("mod_filter", "Filter\ndepth", [659., 78., 64., 86.]));
    ui.controls.push(CustomControl::dial("mod_morph", "Morph", [739., 78., 64., 86.]));

    ui
}

fn style_parameter(parameter: &mut suhina_device_sdk::Parameter) {
    parameter.ui.font_size = 11.0;
    parameter.ui.label_position = suhina_device_sdk::LabelPosition::Top;
    let knob = matches!(
        parameter.control,
        suhina_device_sdk::ParameterControl::Knob | suhina_device_sdk::ParameterControl::LogKnob
    );
    parameter.ui.shape =
        if knob { suhina_device_sdk::ControlShape::Rounded } else { suhina_device_sdk::ControlShape::Square };
    parameter.ui.corner_radius = if knob { 5.0 } else { 0.0 };
}

fn finish(mut g: suhina_device_sdk::DeviceGraph) -> Result<suhina_device_sdk::DeviceGraph, Box<dyn std::error::Error>> {
    let mut visible_parameter = 0usize;
    // The examples teach their controls; their implementation graph does not need panel space.
    for node in &mut g.nodes {
        node.visible = false;
    }
    for p in &mut g.parameters {
        style_parameter(p);
        p.ui.tooltip = format!("{} ({})", p.name, p.unit);
        if !g.ui.as_ref().is_some_and(|ui| ui.hidden_parameters.contains(&p.id)) {
            p.ui.position = Some([(visible_parameter % 2) as f32 * 104.0, (visible_parameter / 2) as f32 * 84.0]);
            p.ui.size = Some([100.0, 80.0]);
            visible_parameter += 1;
        }
    }
    g.validate()?;
    Ok(g)
}

#[cfg(not(test))]
fn save(graph: suhina_device_sdk::DeviceGraph, name: &str) -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args_os()
        .nth(1)
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from(format!("target/sdk-examples/{name}.sd")));
    graph.write_file(&path)?;
    Ok(())
}
