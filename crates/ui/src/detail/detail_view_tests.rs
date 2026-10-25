// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

#[test]
fn a_loop_shorter_than_a_quarter_beat_gives_bounds_that_do_not_cross() {
    use super::{ClipEdit, loop_bounds};
    // A loop ending at beat 0.1 (from `clip.setLoop` or an import): the start's bounds were
    // 0 to -0.15, and the first drag panicked in `f64::clamp`.
    let (min, max) = loop_bounds(ClipEdit::LoopStart, 0.0, 0.1);
    assert!(min <= max, "{min} > {max}");
    assert_eq!(0.3f64.clamp(min, max), 0.0);
    let (min, max) = loop_bounds(ClipEdit::LoopEnd, 119_999.9, 120_000.0);
    assert!(min <= max, "{min} > {max}");
    assert!(max <= 120_000.0 && min <= 120_000.0, "never past the longest loop: {min} to {max}");
    // A loop ending past the limit (an import) gives a start no further than it either.
    assert_eq!(loop_bounds(ClipEdit::LoopStart, 0.0, 130_000.0), (0.0, 120_000.0));
    assert_eq!(loop_bounds(ClipEdit::LoopStart, 0.0, 4.0), (0.0, 3.75));
    assert_eq!(loop_bounds(ClipEdit::LoopEnd, 1.0, 4.0), (1.25, 120_000.0));
}

#[test]
fn sample_name_is_in_header_once() {
    let mut device =
        suhina_engine::model::Device::new("Sampler", suhina_engine::model::DeviceKind::Builtin("sampler".into()));
    assert_eq!(super::device_display_name(&device), "Sampler");
    device.set_sample_file("samples/Piano.wav");
    assert_eq!(super::device_display_name(&device), "Sampler \u{b7} Piano.wav");
    device.name = "Piano".into();
    assert_eq!(super::device_display_name(&device), "Piano");
}

use super::*;

#[test]
fn top_label_places_value_below_control_and_reserves_height() {
    let mut parameter = GraphParameter {
        text: String::new(),
        default: 1.0,
        control: suhina_device_sdk::ParameterControl::Knob,
        unit: String::new(),
        id: "gain".into(),
        name: "Gain".into(),
        min: 0.0,
        max: 2.0,
        value: 1.0,
        ui: suhina_device_sdk::ParameterUi::default(),
    };
    parameter.ui.label_position = suhina_device_sdk::LabelPosition::Top;
    let with_value = parameter_height(&parameter);
    parameter.ui.show_value = false;
    assert!(with_value > parameter_height(&parameter));
}
use crate::demo::demo_project;
use suhina_engine::model::{Device, DeviceParam};

#[test]
fn a_selected_clip_shows_its_properties() {
    let project = demo_project();
    let beat_b = project.arrangement.lanes[0].clips[1].id;
    assert_eq!(project.arrangement.lanes[0].track, Some(TrackId(1)));
    let detail = build(&project, Some(TrackId(1)), Some(ClipRef::Arrangement(TrackId(1), beat_b)));
    let Detail::Clip(info, chain) = detail else { panic!("expected the clip") };
    assert_eq!(chain.track, TrackId(1));
    assert_eq!((info.name.as_str(), info.start.as_str(), info.length.as_str()), ("Beat B", "17.1", "12.0"));
    assert!(info.looping && !info.audio && !info.scene);
    assert_eq!(info.notes, 4);
}

/// The clip column's loop, warp and mode rows ask for the clip's commands (#699).
#[test]
fn the_clip_column_edits_the_clip() {
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;
    let project = demo_project();
    let beat_b = project.arrangement.lanes[0].clips[1].id;
    let Detail::Clip(mut info, chain) =
        build(&project, Some(TrackId(1)), Some(ClipRef::Arrangement(TrackId(1), beat_b)))
    else {
        panic!("expected the clip")
    };
    assert_eq!(info.at, Some(ClipRef::Arrangement(TrackId(1), beat_b)));
    // Shown as an audio clip, to reach the warp rows.
    info.audio = true;
    info.gain_db = Some(0.0);
    let detail = Detail::Clip(info, chain);
    for (label, command, key, value) in [
        ("Loop: On", "clip.setLoop", "loop", json!(false)),
        ("Warp: Off", "clip.setAudio", "warp", json!(true)),
        ("Mode: Smooth", "clip.setAudio", "mode", json!("high-quality")),
    ] {
        let mut harness = Harness::builder().with_size(egui::vec2(700.0, 400.0)).build_ui_state(
            |ui, actions: &mut Vec<Action>| {
                show_row(ui, &detail, &[], actions, &mut Vec::new());
            },
            Vec::new(),
        );
        harness.get_by_label(label).click();
        harness.run();
        let actions = harness.state();
        assert_eq!(actions.len(), 1, "{label}");
        assert_eq!(actions[0].id, command);
        assert_eq!(actions[0].params["clip"], json!(beat_b.0));
        assert_eq!(actions[0].params[key], value, "{label}");
    }
    // A slot's clip is named by its track and its scene, from 1.
    assert_eq!(
        on_clip(ClipRef::Slot(TrackId(2), 1), json!({ "loop": true })),
        json!({ "track": 2, "scene": 2, "loop": true })
    );
}

/// While the transport runs, a panel shows what its automation plays: a toggle holds each
/// point's value, a continuous parameter moves between them (#706).
#[test]
fn panels_follow_automation_as_the_engine_plays_it() {
    use suhina_engine::model::{AutomationLane, AutomationPoint};
    let mut project = demo_project();
    let mut utility = Device::new("Utility", DeviceKind::Builtin("utility".into()));
    for param in ["mono", "balance"] {
        let mut lane = AutomationLane::new(param);
        lane.points = vec![AutomationPoint::new(0.0, 1.0), AutomationPoint::new(4.0, -1.0)];
        utility.automation.push(lane);
    }
    project.channels[0].devices.push(utility);
    let mut detail = build(&project, Some(TrackId(1)), None);
    detail.follow_automation(&project, 1.0);
    let Detail::Devices(chain) = &detail else { panic!("expected the chain") };
    let value = |id: &str| chain.devices.last().and_then(|d| d.values.iter().find(|(p, _)| p == id)).map(|v| v.1);
    assert_eq!(value("mono"), Some(1.0), "a toggle holds its point's value");
    assert_eq!(value("balance"), Some(0.5), "a continuous one is a quarter of the way down");
    detail.follow_automation(&project, 5.0);
    let Detail::Devices(chain) = &detail else { panic!("expected the chain") };
    let value = |id: &str| chain.devices.last().and_then(|d| d.values.iter().find(|(p, _)| p == id)).map(|v| v.1);
    assert_eq!((value("mono"), value("balance")), (Some(0.0), Some(-1.0)), "the last point, kept in range");
}

#[test]
fn a_selected_slot_clip_shows_its_scene_loop_and_fades() {
    let mut project = demo_project();
    let column = project.launcher.columns.iter_mut().find(|c| c.track == TrackId(2)).expect("Bass has slots");
    let clip = column.slots[1].clip.as_mut().expect("Bass has a clip in scene 2");
    clip.fade_in = 1.5;
    clip.fade_out = 0.25;
    let content = clip.content;
    let name = project.content(content).map(|c| c.name.clone()).unwrap_or_default();
    let Detail::Clip(info, chain) = build(&project, None, Some(ClipRef::Slot(TrackId(2), 1))) else {
        panic!("expected the slot's clip")
    };
    assert_eq!(chain.track, TrackId(2));
    assert_eq!((info.name, info.start.as_str(), info.scene), (name, "2", true));
    assert_eq!(info.fades, (1.5, 0.25));
    // An empty slot, a scene past the last, or a track without slots shows the track, or nothing.
    let Detail::Devices(chain) = build(&project, Some(TrackId(3)), Some(ClipRef::Slot(TrackId(3), 0))) else {
        panic!("an empty slot shows the track")
    };
    assert_eq!(chain.track, TrackId(3));
    assert_eq!(build(&project, None, Some(ClipRef::Slot(TrackId(2), 99))), Detail::Nothing);
    assert_eq!(build(&project, None, Some(ClipRef::Slot(TrackId(10), 0))), Detail::Nothing);
}

#[test]
fn a_selected_track_shows_its_devices() {
    let mut project = demo_project();
    project.channels[0].devices.push(Device {
        enabled: false,
        params: vec![DeviceParam::new("gain", -3.5)],
        ..Device::new("Utility", DeviceKind::Builtin("utility".into()))
    });
    let Detail::Devices(chain) = build(&project, Some(TrackId(1)), None) else { panic!() };
    assert_eq!(chain.devices.len(), 1);
    assert_eq!(chain.devices[0].params, [("gain".to_string(), "-3.5".to_string())]);
    assert!(!chain.devices[0].enabled);
    // A clip on the track shows the same chain beside its properties.
    let first = project.arrangement.lanes.iter().find(|l| l.track == Some(TrackId(1))).unwrap().clips[0].id;
    let Detail::Clip(_, with_clip) = build(&project, None, Some(ClipRef::Arrangement(TrackId(1), first))) else {
        panic!()
    };
    assert_eq!(with_clip, chain);
    assert_eq!(build(&project, None, None), Detail::Nothing);
}

#[test]
fn a_built_ins_generated_list_fits_the_compact_panel() {
    let mut project = demo_project();
    project.channels[0].devices = vec![
        Device::new("Utility", DeviceKind::Builtin("utility".into())),
        Device::new("Pitch", DeviceKind::Builtin("pitch".into())),
    ];
    let Detail::Devices(chain) = build(&project, Some(TrackId(1)), None) else { panic!() };
    for device in &chain.devices {
        let graph = device.graph.as_ref().unwrap_or_else(|| panic!("{} has a panel", device.name));
        assert!(graph.canvas.is_none() && graph.nodes.is_empty(), "the generated list, no graph");
        let height = panel_height(device, None);
        assert!(height <= ROW + PAD * 2.0 + COMPACT_HEIGHT, "{}: {height}", device.name);
    }
    // Utility's thirteen parameters do not fit one column: the rest continue in a second one to
    // its right, every cell inside the standard height, so the panel widens and never scrolls.
    let utility = &chain.devices[0];
    let graph = utility.graph.as_ref().unwrap();
    let cells = parameter_layout(&graph.parameters, COMPACT_WIDTH, list_height(utility, graph), &[]);
    assert!(cells.iter().all(|c| c.bottom() <= COMPACT_HEIGHT + 0.5), "{cells:?}");
    assert_eq!(cells.first().unwrap().left(), 0.0);
    assert_eq!(cells.last().unwrap().left(), COMPACT_WIDTH);
    assert_eq!(STACK_WIDTH, 272.0);
}

#[test]
fn a_stacked_generated_list_keeps_its_width_in_any_column() {
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;
    let mut project = demo_project();
    project.channels[0].devices = vec![Device::new("Utility", DeviceKind::Builtin("utility".into()))];
    let Detail::Devices(chain) = build(&project, Some(TrackId(1)), None) else { panic!() };
    let detail = Detail::Devices(chain);
    // A narrow column, the default one and a wide one: Gain's slider is in the same place
    // each time, its row 260 px wide, scrolled past the narrow column's edge, not squeezed.
    let gain = |column: f32| {
        let mut harness = Harness::builder().with_size(egui::vec2(column, 220.0)).build_ui(|ui| {
            show_stack(ui, &detail, &[], &mut Vec::new(), &mut Vec::new());
        });
        harness.run();
        harness.get_by_label("Gain").rect()
    };
    let default = gain(STACK_WIDTH);
    assert!(default.right() <= PAD + COMPACT_WIDTH && default.right() > STACK_WIDTH / 2.0, "{default:?}");
    assert_eq!(gain(200.0), default, "a narrow column scrolls the list");
    assert_eq!(gain(600.0), default, "a wide column leaves it 260 px wide");
}

#[test]
fn an_audio_effect_shows_its_label_and_only_visible_nodes() {
    let mut graph =
        suhina_device_sdk::DeviceGraph::new("org.example.gain", "Example Gain", suhina_device_sdk::DeviceRole::Effect);
    graph.parameters.push(suhina_device_sdk::Parameter::new("gain", "Gain", (0.0, 2.0), 1.0));
    graph.nodes = vec![
        suhina_device_sdk::Node::audio_input("in"),
        suhina_device_sdk::Node::gain("gain", "gain"),
        suhina_device_sdk::Node::audio_output("out"),
    ];
    graph.nodes[0].visible = false;
    graph.nodes[2].visible = false;
    graph.connections =
        vec![suhina_device_sdk::Connection::new("in", "gain"), suhina_device_sdk::Connection::new("gain", "out")];
    let mut project = demo_project();
    project.channels[0].devices.push(Device {
        state: graph.to_sd().ok().map(String::into_bytes),
        ..Device::new("Example Gain", DeviceKind::Sd)
    });
    let Detail::Devices(chain) = build(&project, Some(TrackId(1)), None) else { panic!() };
    assert_eq!(chain.devices[0].kind, "AudioEffect");
    assert_eq!(chain.devices[0].graph.as_ref().map(|graph| graph.nodes.as_slice()), Some(["Gain".into()].as_slice()));
    assert_eq!(chain.devices[0].graph.as_ref().map(|graph| graph.parameters.len()), Some(1));
}

#[test]
fn every_sdk_control_renders_in_a_scrollable_device_panel() {
    use suhina_device_sdk::{Connection, DeviceGraph, DeviceRole, Node, Parameter, ParameterControl::*};
    for control in [Slider, LogSlider, Integer, Toggle, Knob, LogKnob, Number, VerticalSlider, Button, Choice, Radio] {
        let mut graph = DeviceGraph::new("org.test.ui", "UI", DeviceRole::Effect);
        let mut p = Parameter::new("gain", "Gain", (0.0, 2.0), 1.0);
        p.control = control;
        if matches!(control, LogSlider | LogKnob) {
            p.min = 0.01;
        }
        if matches!(control, Toggle | Button) {
            p.max = 1.0;
        }
        if matches!(control, Choice | Radio) {
            p.ui.options = vec!["Off".into(), "Unity".into(), "Boost".into()];
        }
        p.ui.size = Some([240.0, 140.0]);
        p.ui.position = Some([180.0, 40.0]);
        p.ui.font_size = 16.0;
        p.ui.monospace = true;
        p.ui.shape = suhina_device_sdk::ControlShape::Square;
        p.ui.label_position = suhina_device_sdk::LabelPosition::Top;
        if matches!(control, Knob | LogKnob) {
            p.ui.size = Some([40.0, 40.0]);
            p.ui.font_size = 12.0;
            p.ui.label_position = suhina_device_sdk::LabelPosition::Hidden;
            p.ui.show_value = false;
        }
        graph.parameters.push(p);
        graph.nodes = vec![Node::audio_input("in"), Node::gain("gain", "gain"), Node::audio_output("out")];
        graph.connections = vec![Connection::new("in", "gain"), Connection::new("gain", "out")];
        let mut project = demo_project();
        project.channels[0]
            .devices
            .push(Device { state: Some(graph.to_sd().unwrap().into_bytes()), ..Device::new("UI", DeviceKind::Sd) });
        let detail = build(&project, Some(TrackId(1)), None);
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            show_stack(ui, &detail, &[], &mut Vec::new(), &mut Vec::new());
        });
        assert!(!output.shapes.is_empty(), "{control:?}");
        output.textures_delta.clear();
    }
}

#[test]
fn authored_control_positions_and_auto_flow_have_the_expected_extent() {
    use suhina_device_sdk::{ParameterControl, ParameterUi};
    let mut p = GraphParameter {
        text: String::new(),
        default: 1.0,
        control: ParameterControl::Knob,
        unit: String::new(),
        id: "gain".into(),
        name: "Gain".into(),
        min: 0.0,
        max: 2.0,
        value: 1.0,
        ui: ParameterUi::default(),
    };
    p.ui.size = Some([240.0, 120.0]);
    p.ui.position = Some([300.0, 10.0]);
    let mut next = p.clone();
    next.ui.position = None;
    next.ui.size = Some([200.0, 60.0]);
    let cells = parameter_layout(&[p, next.clone()], 320.0, f32::INFINITY, &[]);
    assert_eq!(cells[0].min, Pos2::new(300.0, 10.0));
    assert_eq!(cells[0].max, Pos2::new(540.0, 130.0));
    assert_eq!(cells[1].min, Pos2::new(0.0, 130.0));
    assert_eq!(cells[1].max, Pos2::new(200.0, 190.0));
    // A cell placed in the second column, listed first: the list continues under it there.
    let mut placed = next.clone();
    placed.ui.position = Some([320.0, 0.0]);
    let auto = |id: &str| GraphParameter {
        id: id.into(),
        ui: ParameterUi { size: Some([200.0, 60.0]), ..ParameterUi::default() },
        ..next.clone()
    };
    let cells = parameter_layout(&[placed, auto("a"), auto("b"), auto("c")], 320.0, 130.0, &[]);
    assert_eq!(cells[1].min, Pos2::new(0.0, 0.0));
    assert_eq!(cells[2].min, Pos2::new(0.0, 60.0));
    assert_eq!(cells[3].min, Pos2::new(320.0, 60.0), "{cells:?}");
}

#[test]
#[ignore = "needs a GPU adapter; run with --ignored"]
fn render_the_authored_control_gallery() {
    let graph = styled_gallery::build().unwrap();
    for parameter in &graph.parameters {
        assert_eq!(parameter.ui.position.map(|position| position[1]), Some(0.0));
        assert_eq!(parameter.ui.label_position, suhina_device_sdk::LabelPosition::Top);
    }
    let mut project = demo_project();
    project.channels[0].devices.push(Device {
        state: Some(graph.to_sd().unwrap().into_bytes()),
        ..Device::new(graph.name.clone(), DeviceKind::Sd)
    });
    let detail = build(&project, Some(TrackId(1)), None);
    let mut harness =
        egui_kittest::Harness::builder().with_size((1180.0, 180.0)).with_pixels_per_point(1.0).wgpu().build_ui(|ui| {
            show_row(ui, &detail, &[], &mut Vec::new(), &mut Vec::new());
        });
    theme::install(&harness.ctx);
    harness.run();
    let image = harness.render().unwrap();
    let output = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/ui-snapshots");
    std::fs::create_dir_all(&output).unwrap();
    image.save(output.join("sdk-controls.png")).unwrap();
}

#[test]
#[ignore = "needs a GPU adapter; run with --ignored"]
fn render_sdk_example_with_stock_controls() {
    let graph = custom_panel::build().unwrap();
    let mut project = demo_project();
    project.channels[0].devices.push(Device {
        state: Some(graph.to_sd().unwrap().into_bytes()),
        ..Device::new(graph.name.clone(), DeviceKind::Sd)
    });
    let detail = build(&project, Some(TrackId(1)), None);
    let mut harness =
        egui_kittest::Harness::builder().with_size((660.0, 340.0)).with_pixels_per_point(1.0).wgpu().build_ui(|ui| {
            show_stack(ui, &detail, &[], &mut Vec::new(), &mut Vec::new());
        });
    theme::install(&harness.ctx);
    harness.run();
    let image = harness.render().unwrap();
    let output = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/ui-snapshots");
    std::fs::create_dir_all(&output).unwrap();
    image.save(output.join("sdk-example-controls.png")).unwrap();
}

#[test]
#[ignore = "needs a GPU adapter; run with --ignored"]
fn render_sdk_example_in_a_narrow_panel() {
    let graph = custom_panel::build().unwrap();
    let mut project = demo_project();
    project.channels[0].devices.push(Device {
        state: Some(graph.to_sd().unwrap().into_bytes()),
        ..Device::new(graph.name.clone(), DeviceKind::Sd)
    });
    let detail = build(&project, Some(TrackId(1)), None);
    if let Detail::Devices(chain) = &detail {
        assert_eq!(sdk_viewport(&chain.devices[0]), None);
    } else {
        panic!("expected device chain");
    }
    let mut harness =
        egui_kittest::Harness::builder().with_size((660.0, 340.0)).with_pixels_per_point(1.0).wgpu().build_ui(|ui| {
            show_stack(ui, &detail, &[], &mut Vec::new(), &mut Vec::new());
        });
    theme::install(&harness.ctx);
    harness.run();
    let image = harness.render().unwrap();
    let output = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/ui-snapshots");
    std::fs::create_dir_all(&output).unwrap();
    image.save(output.join("sdk-example-narrow.png")).unwrap();
}

fn report(status: PluginStatus, editor_open: bool) -> PluginReport {
    PluginReport {
        status,
        plugin: Some(("Verb".into(), "Vendor".into(), "same")),
        instrument: false,
        latency: 0,
        editor_open,
        editor_embedded: None,
        blocks: 0,
        late_blocks: 0,
        longest_us: 0,
        no_time: 0,
        state_refused: None,
        ara: false,
        params: None,
    }
}

fn param(id: u32, name: &str, steps: u32, value: f64) -> suhina_plugins::ParamInfo {
    suhina_plugins::ParamInfo {
        id,
        name: name.into(),
        group: String::new(),
        min: 0.0,
        max: 1.0,
        default: 0.0,
        value,
        text: format!("{name} at {value}"),
        steps,
        automatable: true,
        read_only: false,
        hidden: false,
    }
}

/// A running plug-in that listed a mix, a bypass switch, a hidden and a read-only one.
fn listing(status: PluginStatus) -> PluginReport {
    let hidden = suhina_plugins::ParamInfo { hidden: true, ..param(3, "Secret", 0, 0.0) };
    let meter = suhina_plugins::ParamInfo { read_only: true, ..param(4, "Meter", 0, 0.0) };
    let list = vec![param(1, "Mix", 0, 0.5), param(2, "Bypass", 1, 0.0), hidden, meter];
    PluginReport { params: Some(list.into()), ..report(status, false) }
}

#[test]
fn a_running_plug_in_shows_the_parameters_it_lists_as_a_generated_list() {
    let mut project = with_plugin();
    // A value the project keeps wins over the one the plug-in listed, without its text.
    project.channels[0].devices[0].params.push(suhina_engine::model::DeviceParam::new("1", 0.25));
    let graph = |status: PluginStatus| {
        let Detail::Devices(chain) = build_with(
            &project,
            Some(TrackId(1)),
            None,
            &mut |_, _| Some(listing(status.clone())),
            crate::detail::nam_panel::Lists::default(),
        ) else {
            panic!()
        };
        chain.devices[0].graph.clone()
    };
    let running = graph(PluginStatus::Running).unwrap();
    let shown: Vec<_> = running.parameters.iter().map(|p| (p.id.as_str(), p.name.as_str(), p.control)).collect();
    assert_eq!(
        shown,
        [
            ("1", "Mix", suhina_device_sdk::ParameterControl::Slider),
            ("2", "Bypass", suhina_device_sdk::ParameterControl::Toggle)
        ],
        "hidden and read-only ones are left out"
    );
    assert_eq!((running.parameters[0].value, running.parameters[0].text.as_str()), (0.25, ""));
    assert_eq!(running.parameters[1].text, "Bypass at 0");
    assert!(running.canvas.is_none() && running.expanded.is_none());
    // A plug-in that does not run has no controls to send to.
    assert_eq!(graph(PluginStatus::Crashed("gone".into())), None);
}

/// While automation plays, a plug-in's list shows the automated value, kept in range, and
/// drops the text the plug-in gave for the value it listed.
#[test]
fn automation_replaces_a_plug_ins_listed_text_with_the_value_it_plays() {
    use suhina_engine::model::{AutomationLane, AutomationPoint};
    let mut project = with_plugin();
    let point = |beat, value| AutomationPoint { beat, value, curve: 0.0 };
    project.channels[0].devices[0].automation = vec![
        AutomationLane { param: "1".into(), points: vec![point(0.0, 0.75), point(8.0, 0.75)], enabled: true },
        AutomationLane { param: "2".into(), points: vec![point(0.0, 3.0), point(8.0, 3.0)], enabled: true },
    ];
    let mut detail = build_with(
        &project,
        Some(TrackId(1)),
        None,
        &mut |_, _| Some(listing(PluginStatus::Running)),
        crate::detail::nam_panel::Lists::default(),
    );
    detail.follow_automation(&project, 4.0);
    let Detail::Devices(chain) = detail else { panic!() };
    let graph = chain.devices[0].graph.as_ref().unwrap();
    let shown: Vec<_> = graph.parameters.iter().map(|p| (p.value, p.text.as_str())).collect();
    assert_eq!(shown, [(0.75, ""), (1.0, "")]);
    assert_eq!(value_text(&graph.parameters[0], 0.75), "0.75");
}

/// A panel showing what automation plays sends nothing by itself, also where the value
/// falls between the control's steps or decimals: egui's slider rounds the value it draws,
/// and that rounding was sent as a change, which turned the lane off as soon as playback
/// reached it (reported on Discord).
#[test]
fn a_followed_value_between_steps_is_not_sent_as_an_edit() {
    use egui_kittest::Harness;
    use suhina_engine::model::{AutomationLane, AutomationPoint};
    let mut project = demo_project();
    let mut utility = Device::new("Utility", DeviceKind::Builtin("utility".into()));
    for param in ["mono", "balance", "gain", "width"] {
        let mut lane = AutomationLane::new(param);
        lane.points = vec![AutomationPoint::new(0.0, 0.0), AutomationPoint::new(4.0, 1.0)];
        utility.automation.push(lane);
    }
    project.channels[0].devices.push(utility);
    let mut detail = build(&project, Some(TrackId(1)), None);
    detail.follow_automation(&project, 1.234_567);
    for stacked in [false, true] {
        let mut harness = Harness::builder().with_size(egui::vec2(900.0, 400.0)).build_ui_state(
            |ui, actions: &mut Vec<Action>| {
                let mut places = Vec::new();
                if stacked {
                    show_stack(ui, &detail, &[], actions, &mut places);
                } else {
                    show_row(ui, &detail, &[], actions, &mut places);
                }
            },
            Vec::new(),
        );
        harness.run_steps(3);
        let set: Vec<_> = harness.state().iter().filter(|a| a.id == "device.setParam").collect();
        assert!(set.is_empty(), "stacked {stacked}: {set:?}");
    }
}

#[test]
fn a_plug_ins_generated_list_sets_its_parameters_by_id() {
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;
    let project = with_plugin();
    let Detail::Devices(chain) = build_with(
        &project,
        Some(TrackId(1)),
        None,
        &mut |_, _| Some(listing(PluginStatus::Running)),
        crate::detail::nam_panel::Lists::default(),
    ) else {
        panic!()
    };
    let detail = Detail::Devices(chain);
    for stacked in [false, true] {
        let mut harness = Harness::builder().with_size(egui::vec2(600.0, 220.0)).build_ui_state(
            |ui, actions: &mut Vec<Action>| {
                let mut places = Vec::new();
                if stacked {
                    show_stack(ui, &detail, &[], actions, &mut places);
                } else {
                    show_row(ui, &detail, &[], actions, &mut places);
                }
            },
            Vec::new(),
        );
        harness.get_by_label("Bypass").click();
        harness.run();
        let actions = harness.state();
        let set: Vec<_> = actions.iter().filter(|a| a.id == "device.setParam").collect();
        assert_eq!(set.len(), 1, "stacked {stacked}: {actions:?}");
        assert_eq!((&set[0].params["param"], &set[0].params["value"]), (&json!("2"), &json!(1.0)));
        assert!(harness.query_by_label("Open editor Verb").is_some(), "the editor button stays");
    }
}

fn with_plugin() -> Project {
    let mut project = demo_project();
    project.channels[0]
        .devices
        .push(Device { state: Some(vec![1, 2]), ..Device::new("Verb", DeviceKind::Vst3 { id: "ABC".into() }) });
    project
}

#[test]
fn a_plug_in_shows_where_it_stands_and_what_its_button_does() {
    let project = with_plugin();
    let shown = |status: Option<PluginStatus>, open: bool| {
        let Detail::Devices(chain) = build_with(
            &project,
            Some(TrackId(1)),
            None,
            &mut |_, _| status.clone().map(|s| report(s, open)),
            crate::detail::nam_panel::Lists::default(),
        ) else {
            panic!()
        };
        chain.devices[0].plugin.clone().unwrap()
    };
    let running = shown(Some(PluginStatus::Running), false);
    assert_eq!((running.state, running.line.as_str()), (PluginState::Running, "Verb by Vendor"));
    assert_eq!(running.button(), Some(("Open editor", "device.openEditor")));
    assert_eq!(shown(Some(PluginStatus::Running), true).button(), Some(("Close editor", "device.closeEditor")));
    let crashed = shown(Some(PluginStatus::Crashed("its host process exited".into())), false);
    assert_eq!(crashed.button(), Some(("Restart", "device.restart")));
    assert!(crashed.line.contains("exited"));
    let missing = shown(Some(PluginStatus::Missing), false);
    assert_eq!((missing.state, missing.button()), (PluginState::Missing, None));
    assert_eq!(shown(Some(PluginStatus::Loading), false).button(), None);
    // Not playing: nothing runs, nothing to press.
    let stopped = shown(None, false);
    assert_eq!((stopped.state, stopped.button()), (PluginState::Stopped, None));
    // A built-in effect or instrument has no plug-in part.
    let Detail::Devices(chain) = build(&demo_project(), Some(TrackId(1)), None) else { panic!() };
    assert!(chain.devices.iter().all(|d| d.plugin.is_none()));
}

#[test]
fn a_clips_ara_editor_lands_in_the_clip_view_or_offers_to_open() {
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;
    let project = with_plugin();
    let chain_with = |report: PluginReport| {
        let Detail::Devices(chain) = build_with(
            &project,
            Some(TrackId(1)),
            None,
            &mut |_, _| Some(report.clone()),
            crate::detail::nam_panel::Lists::default(),
        ) else {
            panic!()
        };
        chain
    };
    let bound = PluginReport { ara: true, ..report(PluginStatus::Running, false) };
    assert!(chain_with(bound.clone()).devices[0].plugin.as_ref().unwrap().ara);
    // Embedded: it lands at the area's corner, the whole of it showing.
    let embedded =
        chain_with(PluginReport { editor_embedded: Some((300, 200)), ..report(PluginStatus::Running, true) });
    let mut harness = Harness::builder().with_size(egui::vec2(600.0, 300.0)).build_ui_state(
        |ui, (places, actions): &mut (Vec<EditorPlace>, Vec<Action>)| {
            places.clear();
            let area = ui.available_rect_before_wrap();
            show_ara_editor(ui, area, &embedded, 0, actions, places);
        },
        (Vec::new(), Vec::new()),
    );
    harness.run();
    let place = harness.state().0[0];
    assert_eq!((place.track, place.device), (TrackId(1), 0));
    assert_eq!((place.placement.clip_width, place.placement.clip_height), (300, 200));
    // Float above it takes it out to a window of its own.
    harness.get_by_label("Float Verb").click();
    harness.run();
    let float = &harness.state().1[0];
    assert_eq!((float.id, float.params.clone()), ("view.araFloat", json!({ "track": 1, "device": 0, "float": true })));
    // Closed: the area offers to open it, and the button opens that plug-in's editor.
    let closed = chain_with(bound);
    let mut harness = Harness::builder().with_size(egui::vec2(600.0, 300.0)).build_ui_state(
        |ui, actions: &mut Vec<Action>| {
            let area = ui.available_rect_before_wrap();
            show_ara_editor(ui, area, &closed, 0, actions, &mut Vec::new());
        },
        Vec::new(),
    );
    harness.run();
    harness.get_by_label("Open editor Verb").click();
    harness.run();
    assert_eq!(harness.state()[0].id, "device.openEditor");
    assert_eq!(harness.state()[0].params, json!({ "track": 1, "device": 0 }));
}

#[test]
fn an_embedded_editor_takes_its_room_and_says_where_it_lands() {
    use egui_kittest::Harness;
    let project = with_plugin();
    let embedded = PluginReport { editor_embedded: Some((300, 200)), ..report(PluginStatus::Running, true) };
    let Detail::Devices(chain) = build_with(
        &project,
        Some(TrackId(1)),
        None,
        &mut |_, _| Some(embedded.clone()),
        crate::detail::nam_panel::Lists::default(),
    ) else {
        panic!()
    };
    assert_eq!(chain.devices[0].plugin.as_ref().unwrap().embedded, Some((300, 200)));
    let detail = Detail::Devices(chain);
    for stacked in [false, true] {
        let mut harness = Harness::builder().with_size(egui::vec2(600.0, 300.0)).build_ui_state(
            |ui, places: &mut Vec<EditorPlace>| {
                places.clear();
                let mut actions = Vec::new();
                if stacked {
                    show_stack(ui, &detail, &[], &mut actions, places);
                } else {
                    show_row(ui, &detail, &[], &mut actions, places);
                }
            },
            Vec::new(),
        );
        harness.run();
        let places = harness.state();
        assert_eq!(places.len(), 1, "stacked {stacked}");
        let place = places[0];
        assert_eq!((place.track, place.device), (TrackId(1), 0));
        // Under the plug-in's line and button (past the harness's own margin), the whole
        // editor showing (at 1 pixel a point).
        let p = place.placement;
        let margin = p.y - EDITOR_TOP as i32;
        assert!(margin >= 0, "stacked {stacked}: {p:?}");
        assert_eq!((p.x, p.clip_x, p.clip_y), (PAD as i32 + margin, p.x, p.y), "stacked {stacked}");
        assert_eq!((p.clip_width, p.clip_height), (300, 200), "stacked {stacked}");
    }
    // A panel too short for it shows what fits; one with no room hides it.
    let margin = {
        let mut probe = Harness::builder().with_size(egui::vec2(600.0, 300.0)).build_ui_state(
            |ui, places: &mut Vec<EditorPlace>| {
                places.clear();
                show_row(ui, &detail, &[], &mut Vec::new(), places);
            },
            Vec::new(),
        );
        probe.run();
        probe.state()[0].placement.y as f32 - EDITOR_TOP
    };
    let mut short = Harness::builder().with_size(egui::vec2(600.0, EDITOR_TOP + 50.0 + margin * 2.0)).build_ui_state(
        |ui, places: &mut Vec<EditorPlace>| {
            places.clear();
            show_row(ui, &detail, &[], &mut Vec::new(), places);
        },
        Vec::new(),
    );
    short.run();
    assert_eq!((short.state()[0].placement.clip_width, short.state()[0].placement.clip_height), (300, 50));
    let mut none = Harness::builder().with_size(egui::vec2(600.0, EDITOR_TOP - 1.0 + margin * 2.0)).build_ui_state(
        |ui, places: &mut Vec<EditorPlace>| {
            places.clear();
            show_row(ui, &detail, &[], &mut Vec::new(), places);
        },
        Vec::new(),
    );
    none.run();
    assert_eq!(none.state()[0].placement, Placement::HIDDEN);
}

#[test]
fn the_editor_button_asks_for_the_device_command() {
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;
    let project = with_plugin();
    let Detail::Devices(chain) = build_with(
        &project,
        Some(TrackId(1)),
        None,
        &mut |_, _| Some(report(PluginStatus::Running, false)),
        crate::detail::nam_panel::Lists::default(),
    ) else {
        panic!()
    };
    let detail = Detail::Devices(chain);
    for stacked in [false, true] {
        let mut harness = Harness::builder().with_size(egui::vec2(600.0, 200.0)).build_ui_state(
            |ui, actions: &mut Vec<Action>| {
                let mut places = Vec::new();
                if stacked {
                    show_stack(ui, &detail, &[], actions, &mut places);
                } else {
                    show_row(ui, &detail, &[], actions, &mut places);
                }
                assert!(places.is_empty());
            },
            Vec::new(),
        );
        harness.get_by_label("Open editor Verb").click();
        harness.run();
        let actions = harness.state();
        assert_eq!(actions.len(), 1, "stacked {stacked}");
        assert_eq!(actions[0].id, "device.openEditor");
        assert_eq!(actions[0].params, json!({ "track": 1, "device": 0 }));
    }
}
