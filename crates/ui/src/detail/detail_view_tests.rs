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
    // The mode is a drop-down: its menu's choice sets it.
    for (label, choice, command, key, value) in [
        ("Loop: On", None, "clip.setLoop", "loop", json!(false)),
        ("Warp: Off", None, "clip.setAudio", "warp", json!(true)),
        ("Mode: Smooth", Some("High quality"), "clip.setAudio", "mode", json!("high-quality")),
    ] {
        let mut harness = Harness::builder().allow_missing_glyphs().with_size(egui::vec2(900.0, 400.0)).build_ui_state(
            |ui, actions: &mut Vec<Action>| {
                show_row(ui, &detail, &[], actions, &mut Vec::new());
            },
            Vec::new(),
        );
        harness.get_by_label(label).click();
        harness.run();
        if let Some(choice) = choice {
            harness.get_by_label(choice).click();
            harness.run();
        }
        let actions = harness.state();
        assert_eq!(actions.len(), 1, "{label}");
        assert_eq!(actions[0].id, command);
        assert_eq!(actions[0].params["clip"], json!(beat_b.0));
        assert_eq!(actions[0].params[key], value, "{label}");
    }
    // A slot's clip is named by its track and its scene, from 1.
    assert_eq!(
        on_clip(ClipRef::Slot(TrackId(2), 1), json!({ "loop": true })),
        json!({ "track": 2, "scene": 1, "loop": true })
    );
}

/// An audio clip's column shows its start marker, and its end marker when it does not loop; a
/// slot's clip edits its gain too (#699).
#[test]
fn an_audio_clips_column_shows_its_markers_and_gain() {
    use egui_kittest::Harness;
    use egui_kittest::kittest::{NodeT, Queryable};
    let project = demo_project();
    let beat_b = project.arrangement.lanes[0].clips[1].id;
    let Detail::Clip(mut info, chain) =
        build(&project, Some(TrackId(1)), Some(ClipRef::Arrangement(TrackId(1), beat_b)))
    else {
        panic!("expected the clip")
    };
    info.audio = true;
    info.gain_db = Some(0.0);
    info.markers = (1.0, 5.0);
    info.looping = false;
    info.loop_range = None;
    info.at = Some(ClipRef::Slot(TrackId(1), 0));
    let detail = Detail::Clip(info, chain);
    let harness = Harness::builder().allow_missing_glyphs().with_size(egui::vec2(900.0, 500.0)).build_ui_state(
        |ui, actions: &mut Vec<Action>| {
            show_row(ui, &detail, &[], actions, &mut Vec::new());
        },
        Vec::new(),
    );
    // Bars, beats and sixteenths, counted from 1.
    harness.get_by_label("Start marker: 1.2.1");
    harness.get_by_label("End marker: 2.2.1");
    // The slot's gain is a control to drag, not only a value shown.
    let gain = harness.get_by_label("Gain: 0.0 dB");
    assert_eq!(gain.accesskit_node().role(), egui::accesskit::Role::Slider);
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
        lane.points = vec![AutomationPoint::new(0.0, 1.0), AutomationPoint::new(4.0, -1.0)].into();
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

/// While a device parameter's lane is dragged, its knob shows what the line as it is dragged
/// plays at the playhead; a drag on another track or device changes nothing (#811).
#[test]
fn panels_follow_a_dragged_line_at_the_playhead() {
    use crate::arrangement::automation_lanes::{Dragged, LaneKey};
    use suhina_engine::model::{AutomationLane, AutomationPoint};
    let mut project = demo_project();
    let mut utility = Device::new("Utility", DeviceKind::Builtin("utility".into()));
    let mut lane = AutomationLane::new("balance");
    lane.points = vec![AutomationPoint::new(0.0, 1.0), AutomationPoint::new(4.0, -1.0)].into();
    utility.automation.push(lane);
    project.channels[0].devices.push(utility);
    let index = project.channels[0].devices.len() - 1;
    let balance = |detail: &Detail| {
        let Detail::Devices(chain) = detail else { return None };
        chain.devices.last().and_then(|d| d.values.iter().find(|(p, _)| p == "balance")).map(|v| v.1)
    };
    // The first point dragged from full right down to the centre: at beat 2 the line is half way
    // from it to the second point, -0.5, where it played 0 before.
    let points = vec![AutomationPoint::new(0.0, 0.0), AutomationPoint::new(4.0, -1.0)];
    let key = LaneKey { device: Some(index), param: "balance".into() };
    let mut detail = build(&project, Some(TrackId(1)), None);
    detail.follow_automation_dragged(&project, 2.0, None);
    assert_eq!(balance(&detail), Some(0.0), "the project's line");
    let other_track = Dragged { track: TrackId(2), key: key.clone(), points: points.clone(), at: None };
    let mut detail = build(&project, Some(TrackId(1)), None);
    detail.follow_automation_dragged(&project, 2.0, Some(&other_track));
    assert_eq!(balance(&detail), Some(0.0), "another track's drag");
    let elsewhere = LaneKey { device: Some(index + 1), param: "balance".into() };
    let other_device = Dragged { track: TrackId(1), key: elsewhere, points: points.clone(), at: None };
    let mut detail = build(&project, Some(TrackId(1)), None);
    detail.follow_automation_dragged(&project, 2.0, Some(&other_device));
    assert_eq!(balance(&detail), Some(0.0), "another device's drag");
    let held = Dragged { track: TrackId(1), key: key.clone(), points: points.clone(), at: None };
    let mut detail = build(&project, Some(TrackId(1)), None);
    detail.follow_automation_dragged(&project, 2.0, Some(&held));
    assert_eq!(balance(&detail), Some(-0.5));
    // A range being painted: the knob shows the line at its moving edge, not at the playhead.
    let painting = Dragged { track: TrackId(1), key, points, at: Some(4.0) };
    let mut detail = build(&project, Some(TrackId(1)), None);
    detail.follow_automation_dragged(&project, 2.0, Some(&painting));
    assert_eq!(balance(&detail), Some(-1.0));
}

#[test]
fn a_clips_start_counts_bars_through_the_time_signature_changes() {
    // Eight bars of 4/4 (32 beats), then 3/4: beat 64 is beat 3 of bar 19, not bar 17.
    let mut project = demo_project();
    project.transport.time_signature_changes = vec![suhina_engine::model::TimeSignatureChange {
        bar: 8,
        time_signature: suhina_engine::model::TimeSignature { numerator: 3, denominator: 4 },
    }];
    let beat_b = project.arrangement.lanes[0].clips[1].id;
    let detail = build(&project, Some(TrackId(1)), Some(ClipRef::Arrangement(TrackId(1), beat_b)));
    let Detail::Clip(info, _) = detail else { panic!("expected the clip") };
    assert_eq!(info.start, "19.3");
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

/// The clip host (the plug-in that edits a track's clips) is not in the chain's devices but is
/// its host, so the clip editor finds it (#990).
#[test]
fn a_clip_host_is_the_chains_host_not_one_of_its_devices() {
    let mut project = demo_project();
    project.channels[0]
        .devices
        .insert(0, Device { clip_host: true, ..Device::new("Melodyne", DeviceKind::Builtin("utility".into())) });
    project.channels[0].devices.push(Device::new("Utility", DeviceKind::Builtin("utility".into())));
    let Detail::Devices(chain) = build(&project, Some(TrackId(1)), None) else { panic!() };
    assert_eq!(chain.devices.iter().map(|d| d.name.as_str()).collect::<Vec<_>>(), ["Utility"]);
    let host = chain.host.as_ref().expect("the clip host");
    assert_eq!((host.name.as_str(), host.index), ("Melodyne", 0));
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
    project.channels[0].devices = vec![Device::new("Utility", DeviceKind::Builtin("utility".into()))];
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
        let mut harness =
            Harness::builder().allow_missing_glyphs().with_size(egui::vec2(column, 220.0)).build_ui(|ui| {
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
        state: graph.to_sd().ok().map(String::into_bytes).map(Into::into),
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
        project.channels[0].devices.push(Device {
            state: Some(graph.to_sd().unwrap().into_bytes().into()),
            ..Device::new("UI", DeviceKind::Sd)
        });
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
    // A second column as tall as the limit too: the list goes on to the first with room.
    let mut tall = next.clone();
    tall.ui.position = Some([320.0, 0.0]);
    tall.ui.size = Some([200.0, 130.0]);
    let cells = parameter_layout(&[tall, auto("a"), auto("b"), auto("c")], 320.0, 130.0, &[]);
    assert_eq!(cells[3].min, Pos2::new(640.0, 0.0), "{cells:?}");
    // A cell wider than its column keeps the next column clear of it.
    let mut wide = auto("w");
    wide.ui.size = Some([400.0, 100.0]);
    let cells = parameter_layout(&[wide, auto("a")], 320.0, 130.0, &[]);
    assert_eq!(cells[1].min, Pos2::new(400.0, 0.0), "{cells:?}");
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
        state: Some(graph.to_sd().unwrap().into_bytes().into()),
        ..Device::new(graph.name.clone(), DeviceKind::Sd)
    });
    let detail = build(&project, Some(TrackId(1)), None);
    let mut harness = egui_kittest::Harness::builder()
        .allow_missing_glyphs()
        .allow_missing_glyphs()
        .with_size((1180.0, 180.0))
        .with_pixels_per_point(1.0)
        .wgpu()
        .build_ui(|ui| {
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
        state: Some(graph.to_sd().unwrap().into_bytes().into()),
        ..Device::new(graph.name.clone(), DeviceKind::Sd)
    });
    let detail = build(&project, Some(TrackId(1)), None);
    let mut harness = egui_kittest::Harness::builder()
        .allow_missing_glyphs()
        .allow_missing_glyphs()
        .with_size((660.0, 340.0))
        .with_pixels_per_point(1.0)
        .wgpu()
        .build_ui(|ui| {
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
        state: Some(graph.to_sd().unwrap().into_bytes().into()),
        ..Device::new(graph.name.clone(), DeviceKind::Sd)
    });
    let detail = build(&project, Some(TrackId(1)), None);
    if let Detail::Devices(chain) = &detail {
        assert_eq!(sdk_viewport(&chain.devices[0]), None);
    } else {
        panic!("expected device chain");
    }
    let mut harness = egui_kittest::Harness::builder()
        .allow_missing_glyphs()
        .allow_missing_glyphs()
        .with_size((660.0, 340.0))
        .with_pixels_per_point(1.0)
        .wgpu()
        .build_ui(|ui| {
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
            crate::detail::device_widgets::Lists::default(),
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
    let point = |beat, value| AutomationPoint { beat, value, curve: Default::default() };
    project.channels[0].devices[0].automation = vec![
        AutomationLane { param: "1".into(), points: vec![point(0.0, 0.75), point(8.0, 0.75)].into(), enabled: true },
        AutomationLane { param: "2".into(), points: vec![point(0.0, 3.0), point(8.0, 3.0)].into(), enabled: true },
    ];
    let mut detail = build_with(
        &project,
        Some(TrackId(1)),
        None,
        &mut |_, _| Some(listing(PluginStatus::Running)),
        crate::detail::device_widgets::Lists::default(),
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
        lane.points = vec![AutomationPoint::new(0.0, 0.0), AutomationPoint::new(4.0, 1.0)].into();
        utility.automation.push(lane);
    }
    project.channels[0].devices.push(utility);
    let mut detail = build(&project, Some(TrackId(1)), None);
    detail.follow_automation(&project, 1.234_567);
    for stacked in [false, true] {
        let mut harness = Harness::builder().allow_missing_glyphs().with_size(egui::vec2(900.0, 400.0)).build_ui_state(
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
        crate::detail::device_widgets::Lists::default(),
    ) else {
        panic!()
    };
    let detail = Detail::Devices(chain);
    for stacked in [false, true] {
        let mut harness = Harness::builder().allow_missing_glyphs().with_size(egui::vec2(600.0, 220.0)).build_ui_state(
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
        .push(Device { state: Some(vec![1, 2].into()), ..Device::new("Verb", DeviceKind::Vst3 { id: "ABC".into() }) });
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
            crate::detail::device_widgets::Lists::default(),
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
            crate::detail::device_widgets::Lists::default(),
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
    let mut harness = Harness::builder().allow_missing_glyphs().with_size(egui::vec2(600.0, 300.0)).build_ui_state(
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
    let mut harness = Harness::builder().allow_missing_glyphs().with_size(egui::vec2(600.0, 300.0)).build_ui_state(
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
        crate::detail::device_widgets::Lists::default(),
    ) else {
        panic!()
    };
    assert_eq!(chain.devices[0].plugin.as_ref().unwrap().embedded, Some((300, 200)));
    let detail = Detail::Devices(chain);
    for stacked in [false, true] {
        let mut harness = Harness::builder().allow_missing_glyphs().with_size(egui::vec2(600.0, 300.0)).build_ui_state(
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
        let mut probe = Harness::builder().allow_missing_glyphs().with_size(egui::vec2(600.0, 300.0)).build_ui_state(
            |ui, places: &mut Vec<EditorPlace>| {
                places.clear();
                show_row(ui, &detail, &[], &mut Vec::new(), places);
            },
            Vec::new(),
        );
        probe.run();
        probe.state()[0].placement.y as f32 - EDITOR_TOP
    };
    let mut short = Harness::builder()
        .allow_missing_glyphs()
        .allow_missing_glyphs()
        .with_size(egui::vec2(600.0, EDITOR_TOP + 50.0 + margin * 2.0))
        .build_ui_state(
            |ui, places: &mut Vec<EditorPlace>| {
                places.clear();
                show_row(ui, &detail, &[], &mut Vec::new(), places);
            },
            Vec::new(),
        );
    short.run();
    assert_eq!((short.state()[0].placement.clip_width, short.state()[0].placement.clip_height), (300, 50));
    let mut none = Harness::builder()
        .allow_missing_glyphs()
        .allow_missing_glyphs()
        .with_size(egui::vec2(600.0, EDITOR_TOP - 1.0 + margin * 2.0))
        .build_ui_state(
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
        crate::detail::device_widgets::Lists::default(),
    ) else {
        panic!()
    };
    let detail = Detail::Devices(chain);
    for stacked in [false, true] {
        let mut harness = Harness::builder().allow_missing_glyphs().with_size(egui::vec2(600.0, 200.0)).build_ui_state(
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

#[test]
fn a_theme_lays_a_generated_list_side_by_side() {
    let mut project = demo_project();
    project.channels[0].devices = vec![Device::new("Utility", DeviceKind::Builtin("utility".into()))];
    let Detail::Devices(chain) = build(&project, Some(TrackId(1)), None) else { panic!() };
    let graph = chain.devices[0].graph.as_ref().unwrap();
    let (theme, _) = crate::kit::theme::from_css(".device { --suhina-param-columns: 3; --suhina-param-row: 48px; }");
    crate::kit::skin::configure(&theme.skin);
    let cells = parameter_layout(&graph.parameters, 300.0, f32::INFINITY, &[]);
    crate::kit::skin::configure(&crate::kit::skin::Skin::default());
    let lefts: Vec<f32> = cells.iter().take(4).map(|c| c.left()).collect();
    assert_eq!(lefts, [0.0, 100.0, 200.0, 0.0], "three to a line, then the next line");
    assert!(cells[0].height() >= 48.0 && cells[3].top() >= 48.0, "{cells:?}");
}

#[test]
fn stock_value_text_drops_the_minus_of_a_negative_that_rounds_to_zero() {
    let mut parameter = GraphParameter {
        text: String::new(),
        default: 0.0,
        control: suhina_device_sdk::ParameterControl::Knob,
        unit: "dB".into(),
        id: "gain".into(),
        name: "Gain".into(),
        min: -1.0,
        max: 1.0,
        value: 0.0,
        ui: suhina_device_sdk::ParameterUi::default(),
    };
    parameter.ui.decimals = 2;
    assert_eq!(super::parameters::value_text(&parameter, -0.004), "0.00 dB");
    assert_eq!(super::parameters::value_text(&parameter, -0.014), "-0.01 dB");
}

#[test]
fn an_embedded_editor_hides_the_list_so_the_list_takes_no_room() {
    let mut project = demo_project();
    project.channels[0].devices = vec![Device::new("Utility", DeviceKind::Builtin("utility".into()))];
    let Detail::Devices(mut chain) = build(&project, Some(TrackId(1)), None) else { panic!() };
    let Detail::Devices(plugin) = build_with(
        &with_plugin(),
        Some(TrackId(1)),
        None,
        &mut |_, _| Some(PluginReport { editor_embedded: Some((300, 200)), ..report(PluginStatus::Running, true) }),
        crate::detail::device_widgets::Lists::default(),
    ) else {
        panic!()
    };
    let device = &mut chain.devices[0];
    let graph = device.graph.clone().unwrap();
    assert!(list_right(device, &graph) > COMPACT_WIDTH, "two columns");
    device.plugin = plugin.devices[0].plugin.clone();
    assert_eq!(list_right(device, &graph), 0.0);
}

#[test]
fn a_long_list_shows_its_first_two_columns() {
    let mut project = demo_project();
    project.channels[0].devices = vec![Device::new("Utility", DeviceKind::Builtin("utility".into()))];
    let Detail::Devices(chain) = build(&project, Some(TrackId(1)), None) else { panic!() };
    let graph = chain.devices[0].graph.as_ref().unwrap();
    // Utility's list fills two columns of this height, so nothing is left out.
    assert_eq!(list_fit(&graph.parameters, COMPACT_HEIGHT), None);
    // Three rows a column: the rest wait behind the chevron.
    let row = parameter_height(&graph.parameters[0]);
    let fit = list_fit(&graph.parameters, row * 3.0).unwrap();
    assert!((1..graph.parameters.len()).contains(&fit), "{fit}");
    let cells = parameter_layout(&graph.parameters, COMPACT_WIDTH, row * 3.0, &[]);
    assert!(cells[..fit].iter().all(|c| c.left() < COMPACT_WIDTH * 2.0), "{cells:?}");
    assert!(cells[fit].left() >= COMPACT_WIDTH * 2.0, "{cells:?}");
}

#[test]
fn a_tall_list_stays_in_two_columns() {
    let mut project = demo_project();
    project.channels[0].devices = vec![Device::new("Utility", DeviceKind::Builtin("utility".into()))];
    let Detail::Devices(chain) = build(&project, Some(TrackId(1)), None) else { panic!() };
    let graph = chain.devices[0].graph.as_ref().unwrap();
    let mut parameters = graph.parameters.clone();
    for _ in 0..5 {
        parameters.extend_from_slice(&graph.parameters);
    }
    let cells = parameter_layout(&parameters, COMPACT_WIDTH, tall_list_height(&parameters), &[]);
    assert!(cells.iter().all(|c| c.left() < COMPACT_WIDTH * 2.0), "{cells:?}");
    assert!(cells.iter().any(|c| c.left() >= COMPACT_WIDTH), "{cells:?}");
}

#[test]
fn columns_get_room_between_them_as_many_as_there_are() {
    let cell = |left: f32| Rect::from_min_size(Pos2::new(left, 0.0), Vec2::new(100.0, 20.0));
    let mut cells = [cell(0.0), cell(100.0), cell(200.0), cell(200.0)];
    let lines = spread_columns(&mut cells, 100.0);
    let lefts: Vec<f32> = cells.iter().map(|c| c.left()).collect();
    assert_eq!(lefts, [0.0, 100.0 + LIST_GAP, 200.0 + LIST_GAP * 2.0, 200.0 + LIST_GAP * 2.0]);
    assert_eq!(lines, [100.0 + LIST_GAP / 2.0, 200.0 + LIST_GAP * 1.5]);
    // Each line stands between the end of a column and the start of the next.
    assert!(cells[0].right() < lines[0] && lines[0] < cells[1].left());
    assert!(cells[1].right() < lines[1] && lines[1] < cells[2].left());
}

#[test]
fn a_theme_s_cells_in_one_column_stay_together() {
    // Three cells to a 300 point column: no room and no line between them.
    let cell = |left: f32| Rect::from_min_size(Pos2::new(left, 0.0), Vec2::new(100.0, 48.0));
    let mut cells = [cell(0.0), cell(100.0), cell(200.0), cell(0.0)];
    let before = cells;
    assert!(spread_columns(&mut cells, 300.0).is_empty());
    assert_eq!(cells, before);
}

#[test]
fn a_list_with_a_canvas_or_placed_cells_keeps_its_positions() {
    let mut project = demo_project();
    project.channels[0].devices = vec![Device::new("Utility", DeviceKind::Builtin("utility".into()))];
    let Detail::Devices(chain) = build(&project, Some(TrackId(1)), None) else { panic!() };
    let mut graph = chain.devices[0].graph.clone().unwrap();
    assert!(list_spreads(&graph));
    // Two columns: the panel is as wide as them and the room between them.
    assert_eq!(list_right(&chain.devices[0], &graph), COMPACT_WIDTH * 2.0 + LIST_GAP);
    graph.parameters[0].ui.position = Some([0.0, 0.0]);
    assert!(!list_spreads(&graph));
    assert!(list_spreads(&chain.devices[0].graph.clone().unwrap()));
}

#[test]
fn host_values_reach_devices_inside_groups_with_or_without_live_values() {
    use super::{Detail, DeviceAt, DeviceBox, build, give_host_values};
    use suhina_engine::model::{Branch, Channel, ChannelId, Device, DeviceGroup, DeviceKind, Project, Track, TrackId};
    let multiband = || Device::new("Multiband", DeviceKind::Builtin("multiband".into()));
    let mut project = Project::default();
    project.tracks.push(Track { channel: Some(ChannelId(1)), ..Track::new(TrackId(1), "T") });
    let mut channel = Channel::new(ChannelId(1), "T");
    let branch = Branch { devices: vec![multiband()], ..Branch::new("B") };
    channel.devices = vec![multiband(), Device::group("G", DeviceGroup::with_branches(vec![branch]))];
    project.channels.push(channel);
    let Detail::Devices(mut chain) = build(&project, Some(TrackId(1)), None) else {
        panic!("the track's devices");
    };
    let top = DeviceAt::at(0);
    let inner = DeviceAt(vec![1, 0, 0]);
    let value = |devices: &[DeviceBox], at: &DeviceAt, id: &str| {
        let device = if *at == top {
            &devices[0]
        } else {
            &devices[1].group.as_ref().expect("the group").branches[0].devices[0]
        };
        let graph = device.graph.as_ref().expect("its panel");
        graph.parameters.iter().rfind(|p| p.id == id).map(|p| p.value)
    };

    // No player: the panel still knows it is open, and nothing is live.
    assert!(!give_host_values(&mut chain.devices, Some(&inner), &mut |_| None));
    assert_eq!(value(&chain.devices, &inner, suhina_device_sdk::ui::EXPANDED_ID), Some(1.0));
    assert_eq!(value(&chain.devices, &top, suhina_device_sdk::ui::EXPANDED_ID), Some(0.0));
    assert_eq!(value(&chain.devices, &inner, "#r0"), None);

    // A player whose monitor holds the device inside the group's values.
    let live = give_host_values(&mut chain.devices, None, &mut |at| {
        (*at == inner).then_some([-3.0; suhina_device_api::MAX_READOUTS])
    });
    assert!(live);
    assert_eq!(value(&chain.devices, &inner, "#r0"), Some(-3.0));
    assert_eq!(value(&chain.devices, &top, "#r0"), None);
}

/// The demo's Beat B shown as an audio clip with `panel`, gain `db`, transpose `transpose`, and
/// its start marker at 1 beat.
fn audio_detail(panel: AudioPanel, db: f64, transpose: f64) -> (Detail, ClipId) {
    let project = demo_project();
    let beat_b = project.arrangement.lanes[0].clips[1].id;
    let Detail::Clip(mut info, chain) =
        build(&project, Some(TrackId(1)), Some(ClipRef::Arrangement(TrackId(1), beat_b)))
    else {
        panic!("expected the clip")
    };
    info.audio = true;
    info.gain_db = Some(db);
    info.transpose = transpose;
    info.warped = true;
    info.markers = (1.0, 5.0);
    info.looping = false;
    info.loop_range = None;
    info.panel = Some(AudioPanel { clip: beat_b, ..panel });
    (Detail::Clip(info, chain), beat_b)
}

fn panel_harness(detail: Detail) -> egui_kittest::Harness<'static, Vec<Action>> {
    panel_harness_tall(detail, 260.0)
}

/// [`panel_harness`] in a window `height` points tall.
fn panel_harness_tall(detail: Detail, height: f32) -> egui_kittest::Harness<'static, Vec<Action>> {
    egui_kittest::Harness::builder()
        .allow_missing_glyphs()
        .with_size(egui::vec2(1000.0, height))
        .with_step_dt(1.0 / 60.0)
        .with_max_steps(60)
        .build_ui_state(
            move |ui, actions: &mut Vec<Action>| {
                show_row(ui, &detail, &[], actions, &mut Vec::new());
            },
            Vec::new(),
        )
}

/// Two presses at `at` close together: a double-click. It waits first, as a person would, so a
/// click just before does not make it a triple click.
/// One click at `at`, well after any earlier one (so it is no double-click).
fn click(harness: &mut egui_kittest::Harness<'static, Vec<Action>>, at: egui::Pos2) {
    harness.hover_at(at);
    harness.run();
    for _ in 0..40 {
        harness.step();
    }
    harness.drag_at(at);
    harness.drop_at(at);
    harness.run();
}

fn double_click(harness: &mut egui_kittest::Harness<'static, Vec<Action>>, at: egui::Pos2) {
    harness.hover_at(at);
    harness.run();
    for _ in 0..40 {
        harness.step();
    }
    for _ in 0..2 {
        harness.hover_at(at);
        harness.drag_at(at);
        harness.drop_at(at);
        harness.step();
    }
    harness.run();
}

/// The Gain group's fader: dragged up it raises the gain in one command when let go; a
/// double-click puts it at 0 dB (`docs/spec/clip-panel.md` › Groups).
#[test]
fn the_gain_fader_drags_and_resets() {
    use egui_kittest::kittest::{NodeT, Queryable};
    let (detail, beat_b) = audio_detail(AudioPanel::default(), -6.0, 0.0);
    let mut harness = panel_harness(detail);
    let fader = harness.get_by_label("Gain fader: -6.0 dB");
    assert_eq!(fader.accesskit_node().role(), egui::accesskit::Role::Slider);
    let at = fader.rect().center();
    harness.hover_at(at);
    harness.run();
    harness.drag_at(at);
    harness.run();
    for dy in [5.0, 10.0, 20.0] {
        harness.hover_at(at - egui::vec2(0.0, dy));
        harness.run();
    }
    assert!(!harness.state().is_empty(), "the gain follows while it is dragged");
    harness.drop_at(at - egui::vec2(0.0, 20.0));
    harness.run();
    let actions = std::mem::take(harness.state_mut());
    assert!(actions.len() >= 2, "{actions:?}");
    assert!(actions.iter().all(|a| a.id == "clip.setGain" && a.params["clip"] == json!(beat_b.0)));
    // One undo step: every call of the drag carries its gesture.
    assert!(actions.iter().all(|a| a.params["gesture"] == json!("clip-panel")), "{actions:?}");
    let last = actions.last().and_then(|a| a.params["db"].as_f64());
    assert!(last.is_some_and(|db| db > -6.0), "{last:?}");
    double_click(&mut harness, at);
    let reset = harness.state().last().map(|a| a.params["db"].clone());
    assert_eq!(reset, Some(json!(0.0)));
}

/// The Pitch group's knob moves whole semitones and keeps the cents; the Cents field typed after
/// a double-click keeps the semitones.
#[test]
fn the_pitch_knob_and_the_cents_field_set_the_transpose() {
    use egui_kittest::kittest::Queryable;
    let (detail, _) = audio_detail(AudioPanel::default(), 0.0, 2.25);
    let mut harness = panel_harness(detail);
    let at = harness.get_by_label("Pitch: +2 st").rect().center();
    harness.hover_at(at);
    harness.run();
    harness.drag_at(at);
    harness.run();
    harness.hover_at(at - egui::vec2(0.0, 6.0));
    harness.run();
    harness.hover_at(at - egui::vec2(0.0, 12.0));
    harness.run();
    harness.drop_at(at - egui::vec2(0.0, 12.0));
    harness.run();
    let actions = std::mem::take(harness.state_mut());
    assert!(actions.iter().all(|a| a.id == "clip.setAudio"));
    // Up by whole semitones (a quarter semitone a point, counted once the drag has started past
    // egui's threshold), the 25 cents kept.
    let last = actions.last().and_then(|a| a.params["transpose"].as_f64()).unwrap_or(0.0);
    assert!(last > 2.25 && ((last - 0.25) - (last - 0.25).round()).abs() < 1e-9, "{last}");
    let cents = harness.get_by_label("Cents: +25").rect().center();
    double_click(&mut harness, cents);
    harness.get_by_label("Cents value").click();
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    harness.get_by_label("Cents value").type_text("-10");
    harness.key_press(egui::Key::Enter);
    harness.run();
    let typed = harness.state().last().map(|a| a.params["transpose"].clone());
    assert_eq!(typed, Some(json!(1.9)), "2 semitones less 10 cents");
}

/// The tempo is typed into a text field opened by a double-click, halved and doubled, each a
/// `clip.setTempo` set by hand; Escape leaves a typed tempo unset.
#[test]
fn the_tempo_is_typed_halved_and_doubled() {
    use egui_kittest::kittest::Queryable;
    let panel = AudioPanel {
        tempo: Some(96.0),
        tempo_source: Some(suhina_engine::model::TempoSource::Name),
        ..AudioPanel::default()
    };
    let (detail, beat_b) = audio_detail(panel, 0.0, 0.0);
    let mut harness = panel_harness(detail);
    assert!(harness.query_by_label("Tempo: 96.000").is_some());
    harness.get_by_label("Halve the tempo").click();
    harness.run();
    harness.get_by_label("Double the tempo").click();
    harness.run();
    // One click opens the text field.
    let field = harness.get_by_label("Tempo: 96.000").rect().center();
    click(&mut harness, field);
    harness.get_by_label("Tempo value").click();
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    harness.get_by_label("Tempo value").type_text("124.5");
    harness.key_press(egui::Key::Enter);
    harness.run();
    let sets: Vec<(f64, String)> = harness
        .state()
        .iter()
        .filter(|a| a.id == "clip.setTempo")
        .map(|a| (a.params["bpm"].as_f64().unwrap_or(0.0), a.params["source"].as_str().unwrap_or("").into()))
        .collect();
    assert_eq!(sets, [(48.0, "manual".into()), (192.0, "manual".into()), (124.5, "manual".into())]);
    assert!(harness.state().iter().all(|a| a.params["clip"] == json!(beat_b.0)));
    let before = harness.state().len();
    click(&mut harness, field);
    harness.get_by_label("Tempo value").click();
    harness.get_by_label("Tempo value").type_text("80");
    harness.key_press(egui::Key::Escape);
    harness.run();
    assert_eq!(harness.state().len(), before);
}

/// The tempo field opened with one click still steps with the arrows, a whole BPM each.
#[test]
fn the_tempo_field_opened_with_a_click_steps_with_the_arrows() {
    use egui_kittest::kittest::Queryable;
    let panel = AudioPanel {
        tempo: Some(96.0),
        tempo_source: Some(suhina_engine::model::TempoSource::Name),
        ..AudioPanel::default()
    };
    let (detail, _) = audio_detail(panel, 0.0, 0.0);
    let mut harness = panel_harness(detail);
    let field = harness.get_by_label("Tempo: 96.000").rect().center();
    click(&mut harness, field);
    assert!(harness.query_by_label("Tempo value").is_some(), "one click opens the text field");
    harness.key_press(egui::Key::ArrowUp);
    harness.run();
    let sets: Vec<f64> =
        harness.state().iter().filter(|a| a.id == "clip.setTempo").filter_map(|a| a.params["bpm"].as_f64()).collect();
    assert_eq!(sets, [97.0]);
}

/// Set beside a marker puts it at the playhead, as a beat of the clip's content, on a quarter beat.
#[test]
fn set_puts_a_marker_at_the_playhead() {
    use egui_kittest::kittest::Queryable;
    let panel = AudioPanel { playhead: Some(3.3), set_buttons: true, ..AudioPanel::default() };
    let (detail, _) = audio_detail(panel, 0.0, 0.0);
    let mut harness = panel_harness(detail);
    harness.get_by_label("Set Start marker to the playhead").click();
    harness.run();
    harness.get_by_label("Set End marker to the playhead").click();
    harness.run();
    let actions = harness.state();
    assert_eq!(actions[0].id, "clip.setStart");
    assert_eq!(actions[0].params["offset"], json!(3.25));
    assert_eq!(actions[1].id, "clip.setLength");
    assert_eq!(actions[1].params["length"], json!(2.25), "the end at 3.25, from the start marker at 1");
}

/// A slot's clip has no place on the arrangement's timeline: no Set buttons.
#[test]
fn a_slots_clip_has_no_set_buttons() {
    use egui_kittest::kittest::Queryable;
    let (Detail::Clip(mut info, chain), _) = audio_detail(AudioPanel::default(), 0.0, 0.0) else {
        panic!("expected the clip")
    };
    info.at = Some(ClipRef::Slot(TrackId(1), 0));
    let harness = panel_harness(Detail::Clip(info, chain));
    assert!(harness.query_by_label("Set Start marker to the playhead").is_none());
    assert!(harness.query_by_label("Start marker: 1.2.1").is_some());
}

/// The fields keep a gap between them: the column is no longer one grey box over its rows
/// (#1190). Start and End sit side by side, Warp over Mode.
#[test]
fn the_panels_fields_do_not_touch() {
    use egui_kittest::kittest::Queryable;
    let (detail, _) = audio_detail(AudioPanel { tempo: Some(120.0), ..AudioPanel::default() }, 0.0, 0.0);
    let harness = panel_harness(detail);
    let start = harness.get_by_label("Start marker: 1.2.1").rect();
    let end = harness.get_by_label("End marker: 2.2.1").rect();
    assert!(end.left() - start.right() >= 4.0, "{start:?} {end:?}");
    let warp = harness.get_by_label("Warp: On").rect();
    let mode = harness.get_by_label("Mode: Smooth").rect();
    assert!(mode.top() - warp.bottom() >= 4.0, "{warp:?} {mode:?}");
}

/// The scale's root and mode are lists under their fields; a pick sets the clip's scale.
#[test]
fn the_scale_is_picked_from_lists_under_its_fields() {
    use egui_kittest::kittest::Queryable;
    let (detail, beat_b) = audio_detail(AudioPanel::default(), 0.0, 0.0);
    // Tall enough for the list to open under its field (with no room it opens over it).
    let mut harness = panel_harness_tall(detail, 600.0);
    let field = harness.get_by_label("Scale: None").rect();
    harness.get_by_label("Scale: None").click();
    harness.run();
    let minor = harness.get_by_label("Minor").rect();
    assert!(
        minor.top() >= field.bottom() && (minor.left() - field.left()).abs() < 80.0,
        "under its field: {minor:?} {field:?}"
    );
    harness.get_by_label("Minor").click();
    harness.run();
    harness.get_by_label("Scale root: —").click();
    harness.run();
    harness.get_by_label("F#/Gb").click();
    harness.run();
    let actions = harness.state();
    assert_eq!(actions.len(), 2, "{actions:?}");
    assert_eq!((actions[0].id, &actions[0].params["mode"]), ("clip.setScale", &json!("minor")));
    assert_eq!((actions[1].id, &actions[1].params["root"]), ("clip.setScale", &json!("F#")));
    assert!(actions.iter().all(|a| a.params["clip"] == json!(beat_b.0)));
}

#[test]
fn a_transpose_splits_into_semitones_and_cents_and_the_fader_maps_both_ways() {
    assert_eq!(super::clip_panel::split_transpose(2.25), (2.0, 25.0));
    assert_eq!(super::clip_panel::split_transpose(-0.4), (-0.0, -40.0));
    for db in [-70.0, -40.0, -24.0, -6.0, 0.0, 12.0, 24.0] {
        let back = super::clip_panel::fader_db(super::clip_panel::fader_position(db));
        assert!((back - db).abs() < 1e-9, "{db} {back}");
    }
    assert_eq!(super::clip_panel::fader_position(0.0), 0.5, "0 dB half-way");
    assert_eq!(super::clip_panel::fader_position(24.0), 1.0);
    assert_eq!(super::clip_panel::fader_position(-70.0), 0.0);
}

/// Markers read and type as bars, beats and sixteenths (4/4 here; 6/8 has three beats a bar).
#[test]
fn markers_are_bars_beats_and_sixteenths() {
    use super::clip_panel::{bars_parse, bars_text};
    assert_eq!(bars_text(0.0, 4.0, true), "1.1.1");
    assert_eq!(bars_text(9.25, 4.0, true), "3.2.2");
    assert_eq!(bars_text(196.65, 4.0, true), "50.1.3");
    assert_eq!(bars_text(4.5, 4.0, false), "1.0.2");
    assert_eq!(bars_text(3.0, 3.0, true), "2.1.1");
    assert_eq!(bars_parse("3.2.2", 4.0, true), Some(9.25));
    assert_eq!(bars_parse("3", 4.0, true), Some(8.0), "a bar alone is its start");
    assert_eq!(bars_parse("1.0.2", 4.0, false), Some(4.5));
    assert_eq!(bars_parse("x", 4.0, true), None);
}

/// A marker dragged by the beat number under the pointer moves by whole beats, following as it
/// goes, in one undo step, as the project's tempo field steps the place it was grabbed by.
#[test]
fn a_marker_is_dragged_by_the_place_under_the_pointer() {
    use egui_kittest::kittest::Queryable;
    let (detail, _) = audio_detail(AudioPanel::default(), 0.0, 0.0);
    let mut harness = panel_harness(detail);
    let field = harness.get_by_label("Start marker: 1.2.1").rect();
    // The text is right-aligned: "1.2.1", its beat the third character from the right.
    let width = 7.0;
    let at = egui::pos2(field.right() - 3.0 - width * 2.5, field.center().y);
    harness.hover_at(at);
    harness.run();
    harness.drag_at(at);
    harness.run();
    for dy in [4.0, 8.0] {
        harness.hover_at(at - egui::vec2(0.0, dy));
        harness.run();
    }
    harness.drop_at(at - egui::vec2(0.0, 8.0));
    harness.run();
    let offsets: Vec<f64> = harness
        .state()
        .iter()
        .filter(|a| a.id == "clip.setStart")
        .filter_map(|a| a.params["offset"].as_f64())
        .collect();
    assert!(offsets.last().is_some_and(|o| (o - o.round()).abs() < 1e-9 && *o > 1.0), "whole beats: {offsets:?}");
}

/// A slot's clip shows its colour, but no palette: `clip.setColor` names arrangement clips.
#[test]
fn a_slots_clip_has_no_colour_palette() {
    use egui_kittest::kittest::Queryable;
    let (Detail::Clip(mut info, chain), _) = audio_detail(AudioPanel::default(), 0.0, 0.0) else {
        panic!("expected the clip")
    };
    info.at = Some(ClipRef::Slot(TrackId(1), 0));
    let harness = panel_harness(Detail::Clip(info, chain));
    assert!(harness.query_by_label("Clip colour").is_none());
}

/// Semitones at the top and cents added stay in the transpose's range.
#[test]
fn the_transpose_stays_within_48_semitones() {
    use egui_kittest::kittest::Queryable;
    let (detail, _) = audio_detail(AudioPanel::default(), 0.0, 48.0);
    let mut harness = panel_harness(detail);
    let cents = harness.get_by_label("Cents: 0").rect().center();
    double_click(&mut harness, cents);
    harness.get_by_label("Cents value").click();
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    harness.get_by_label("Cents value").type_text("25");
    harness.key_press(egui::Key::Enter);
    harness.run();
    let sent = harness.state().last().map(|a| a.params["transpose"].clone());
    assert!(sent.is_none() || sent == Some(json!(48.0)), "{sent:?}");
}

/// With the playhead outside the clip the Set buttons stay, greyed: a click sets nothing.
#[test]
fn set_buttons_stay_greyed_while_the_playhead_is_outside_the_clip() {
    use egui_kittest::kittest::Queryable;
    let (detail, _) = audio_detail(AudioPanel { set_buttons: true, ..AudioPanel::default() }, 0.0, 0.0);
    let mut harness = panel_harness(detail);
    harness.get_by_label("Set Start marker to the playhead").click();
    harness.run();
    assert!(harness.state().is_empty());
}

/// A plug-in's panel, a generated list and a canvas panel in one row are as tall as each other,
/// the tallest's height, never under a compact panel's (#1238).
#[test]
fn the_panels_in_a_row_are_one_height() {
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;
    let mut project = with_plugin();
    project.channels[0].devices.push(Device::new("Utility", DeviceKind::Builtin("utility".into())));
    project.channels[0].devices.push(Device::new("EQ", DeviceKind::Builtin("eq".into())));
    let detail = build(&project, Some(TrackId(1)), None);
    let mut harness = Harness::builder().allow_missing_glyphs().with_size(egui::vec2(2400.0, 500.0)).build_ui(|ui| {
        show_row(ui, &detail, &[], &mut Vec::new(), &mut Vec::new());
    });
    harness.run_steps(3);
    let heights: Vec<f32> = ["Verb", "Utility", "EQ"]
        .iter()
        .map(|name| harness.get_by_label(&format!("Device {name}")).rect().height())
        .collect();
    assert!(heights.iter().all(|h| (h - heights[0]).abs() < 0.5), "one height: {heights:?}");
    assert!(heights[0] >= device_row_height() - 0.5, "at least a compact panel's: {heights:?}");
}

/// Stacked lists each keep their own scroll, also when a device before them goes: the scroll is
/// the device's, not its place's (#826).
#[test]
fn a_stacked_list_keeps_its_scroll_when_a_device_before_it_goes() {
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;
    let mut project = demo_project();
    project.channels[0].devices = vec![Device::new("Pitch", DeviceKind::Builtin("pitch".into()))];
    for _ in 0..2 {
        project.channels[0].devices.push(Device::new("Utility", DeviceKind::Builtin("utility".into())));
    }
    let _ = project.assign_ids();
    let mut harness =
        Harness::builder().allow_missing_glyphs().with_size(egui::vec2(STACK_WIDTH, 700.0)).build_ui_state(
            |ui, detail: &mut Detail| {
                show_stack(ui, detail, &[], &mut Vec::new(), &mut Vec::new());
            },
            build(&project, Some(TrackId(1)), None),
        );
    harness.run_steps(10);
    let gains =
        |harness: &Harness<'_, Detail>| harness.query_all_by_label("Gain").map(|n| n.rect().left()).collect::<Vec<_>>();
    let start = gains(&harness);
    // The second Utility's list, scrolled sideways.
    let second = harness.query_all_by_label("Device Utility").nth(1).unwrap().rect().center();
    harness.hover_at(second);
    harness.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(-150.0, 0.0),
        modifiers: egui::Modifiers::NONE,
        phase: egui::TouchPhase::Move,
        source: egui::MouseWheelSource::Unknown,
    });
    harness.run_steps(10);
    let scrolled = gains(&harness);
    assert!(scrolled[1] < start[1] - 50.0 && scrolled[0] == start[0], "only the second list scrolls: {scrolled:?}");
    // Pitch goes: the Utilities move up a place each and keep their own scroll.
    project.channels[0].devices.remove(0);
    *harness.state_mut() = build(&project, Some(TrackId(1)), None);
    harness.hover_at(egui::pos2(1.0, 699.0));
    harness.run_steps(10);
    assert_eq!(gains(&harness), scrolled, "each list keeps its own scroll");
}
