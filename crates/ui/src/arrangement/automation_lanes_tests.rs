// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

#[test]
fn points_snap_to_the_lines_the_arrangement_draws() {
    // 3/4 with a half-note grid: lines at 0, 2, 3, 5, 6, ... (they start again at each bar).
    let grid = TimelineGrid::new(2.0, 400.0, 3.0, 8.0);
    let geometry = Geometry {
        time: egui::Rangef::new(0.0, 1000.0),
        headers: egui::Rangef::new(0.0, 0.0),
        visible: Rect::from_min_size(Pos2::ZERO, Vec2::splat(1000.0)),
        scroll_x: 0.0,
        beat_width: 400.0,
        grid,
    };
    let drawn: Vec<f64> = grid.lines(0.0, 9.0, 100).into_iter().map(|(beat, _)| beat).collect();
    assert_eq!(drawn, vec![0.0, 2.0, 3.0, 5.0, 6.0, 8.0, 9.0]);
    assert_eq!(geometry.snap(3.9, false), 3.0, "not beat 4, which has no line");
    assert_eq!(geometry.snap(4.2, false), 5.0);
    assert_eq!(geometry.snap(2.6, false), 3.0);
    for n in 0..900 {
        let snapped = geometry.snap(f64::from(n) / 100.0, false);
        assert!(drawn.contains(&snapped), "{n}: {snapped}");
    }
    assert_eq!(geometry.snap(3.9123, true), 3.912);
    let none = Geometry { grid: TimelineGrid { snaps: false, ..grid }, ..geometry };
    assert_eq!(none.snap(3.9123, false), 3.912);
    assert_eq!(geometry.snap(f64::NAN, false), 0.0);
}

/// A volume placed on the lane and read back is the same, and sits where the mixer's field puts it.
#[test]
fn volume_positions_and_values_are_inverses() {
    let scale = Scale::Volume;
    for db in [-60.0, -24.0, -6.0, 0.0, 3.0, 6.0] {
        let gain = 10f64.powf(db / 20.0);
        let back = scale.value_at(scale.position(gain));
        assert!((gain_to_db(back) - db).abs() < 1e-6, "{db}");
    }
    assert_eq!(scale.value_at(0.0), 0.0);
    assert_eq!(scale.command(0.0), json!("-inf"));
    assert_eq!(scale.command(1.0), json!(0.0));
    assert!((scale.position(1.0) - 0.85).abs() < 1e-6);
    // The lane places a volume where the mixer's field does.
    for db in [-70.0, -30.0, -6.0, 0.0, 4.0] {
        let field = f64::from(crate::kit::widgets::volume_position(db));
        assert!((scale.position(10f64.powf(db / 20.0)) - field).abs() < 1e-6, "{db}");
    }
}

/// A whole-number lane cannot keep a value between two numbers; a whole number is kept.
#[test]
fn a_whole_number_lane_keeps_only_whole_numbers() {
    let integer = Scale::Device(ParamInfo::integer("steps", "Steps", (0, 3), 0, Unit::None));
    assert!(!integer.keeps(1.5));
    assert!(integer.keeps(1.0));
    assert!(Scale::Pan.keeps(0.123_456));
}

/// The distance to a line is to its nearest segment, end or corner.
#[test]
fn the_distance_to_a_line_is_to_its_nearest_part() {
    let line = [Pos2::new(0.0, 0.0), Pos2::new(10.0, 0.0), Pos2::new(10.0, 10.0)];
    assert_eq!(distance_to(&line, Pos2::new(5.0, 3.0)), 3.0);
    assert_eq!(distance_to(&line, Pos2::new(12.0, 5.0)), 2.0);
    assert_eq!(distance_to(&line, Pos2::new(-4.0, 3.0)), 5.0);
    assert_eq!(distance_to(&[], Pos2::ZERO), f32::INFINITY);
}

/// The pan spreads from left at the bottom to right at the top; the mute is a step.
#[test]
fn pan_and_mute_map_across_the_lane() {
    assert_eq!(Scale::Pan.position(-1.0), 0.0);
    assert_eq!(Scale::Pan.value_at(0.75), 0.5);
    assert_eq!(Scale::Mute.value_at(0.7), 1.0);
    assert!(Scale::Mute.stepped());
    assert!(!Scale::Pan.stepped());
}

/// Moving values together keeps them in range, and a silent volume stays silent.
#[test]
fn a_shift_keeps_values_in_range_and_silence_silent() {
    assert_eq!(Scale::Volume.shift(0.0, 6.0), 0.0);
    assert!((gain_to_db(Scale::Volume.shift(1.0, 10.0)) - 6.0).abs() < 1e-9);
    assert_eq!(Scale::Pan.shift(0.5, 1.0), 1.0);
    assert_eq!(Scale::Mute.shift(1.0, -0.4), 1.0);
}

/// A track's lanes, opened the first time, are those with points, or the volume's when none has.
#[test]
fn the_first_open_shows_the_lanes_with_points_or_the_volume() {
    let mut project = crate::demo::demo_project();
    let track = project.tracks[0].id;
    let mut state = LanesState::default();
    let params = Params::build(&project);
    state.toggle(track, &params);
    assert_eq!(state.shown(track, &params), [LaneKey::mixer(VOLUME)]);
    state.toggle(track, &params);
    assert!(state.shown(track, &params).is_empty());

    let channel = project.tracks[1].channel;
    if let Some(channel) = project.channels.iter_mut().find(|c| Some(c.id) == channel) {
        let mut lane = AutomationLane::new(PAN);
        lane.insert(AutomationPoint::new(0.0, 0.5));
        channel.automation.push(lane);
    }
    let params = Params::build(&project);
    let track = project.tracks[1].id;
    state.toggle(track, &params);
    assert_eq!(state.shown(track, &params), [LaneKey::mixer(PAN)]);
    assert_eq!(state.height(track, &params), LANE + LINE);
}

/// A built-in device's parameters are listed under its name, with their lanes' points.
#[test]
fn a_built_in_devices_parameters_can_be_shown_with_its_lanes() {
    let mut project = crate::demo::demo_project();
    let channel = project.tracks[0].channel;
    let Some(channel) = project.channels.iter_mut().find(|c| Some(c.id) == channel) else { return };
    let mut lane = AutomationLane::new("gain");
    lane.insert(AutomationPoint::new(4.0, -6.0));
    channel.devices.push(suhina_engine::model::Device {
        name: "Utility".into(),
        kind: DeviceKind::Builtin("utility".into()),
        enabled: true,
        state: None,
        params: Vec::new(),
        automation: vec![lane],
        extensions: Default::default(),
    });
    let params = Params::build(&project);
    let track = project.tracks[0].id;
    let gain = params.param(track, &LaneKey { device: Some(0), param: "gain".into() });
    assert_eq!(gain.map(Param::label).as_deref(), Some("Utility: Gain"));
    assert_eq!(gain.map(|p| p.points.len()), Some(1));
    let mut state = LanesState::default();
    state.toggle(track, &params);
    assert_eq!(state.shown(track, &params), [LaneKey { device: Some(0), param: "gain".into() }]);
}

/// A data-only device's parameters are listed and placed as their controls place them.
#[test]
fn sd_parameters_are_listed_and_use_their_control_scale() {
    let mut project = crate::demo::demo_project();
    let track = project.tracks[0].id;
    let Some(channel) = project.tracks[0].channel.and_then(|id| project.channel_mut(id)) else {
        return;
    };
    let mut graph =
        suhina_device_sdk::DeviceGraph::new("test.filter", "Test Filter", suhina_device_sdk::DeviceRole::Effect);
    let mut parameter = suhina_device_sdk::Parameter::new("frequency", "Frequency", (20.0, 20_000.0), 1_000.0);
    parameter.control = suhina_device_sdk::ParameterControl::LogKnob;
    parameter.unit = "Hz".into();
    graph.parameters.push(parameter);
    graph.nodes = vec![
        suhina_device_sdk::Node::audio_input("in"),
        suhina_device_sdk::Node::gain("gain", "frequency"),
        suhina_device_sdk::Node::audio_output("out"),
    ];
    graph.connections =
        vec![suhina_device_sdk::Connection::new("in", "gain"), suhina_device_sdk::Connection::new("gain", "out")];
    let mut lane = AutomationLane::new("frequency");
    lane.insert(AutomationPoint::new(4.0, 1_000.0));
    channel.devices.push(suhina_engine::model::Device {
        name: "Test Filter".into(),
        kind: DeviceKind::Sd,
        enabled: true,
        state: graph.to_sd().ok().map(String::into_bytes),
        params: Vec::new(),
        automation: vec![lane],
        extensions: Default::default(),
    });
    let params = Params::build(&project);
    let frequency = params.param(track, &LaneKey { device: Some(0), param: "frequency".into() });
    assert_eq!(frequency.map(Param::label).as_deref(), Some("Test Filter: Frequency"));
    let Some(Scale::Sd(_)) = frequency.map(|p| &p.scale) else { panic!("expected .sd scale") };
    assert_eq!(frequency.map(|p| p.scale.text(1_000.0)).as_deref(), Some("1000.00 Hz"));
    let position = (1_000.0_f64.ln() - 20.0_f64.ln()) / (20_000.0_f64.ln() - 20.0_f64.ln());
    assert!((frequency.map_or(0.0, |p| p.scale.position(1_000.0)) - position).abs() < 1e-9);
}

/// A parameter of device `device` (`None`: the mixer) in `group`.
fn param(device: Option<usize>, group: Option<&str>, name: &str) -> Param {
    Param {
        key: LaneKey { device, param: name.to_lowercase() },
        name: name.into(),
        device_name: device.map(|d| format!("Device {d}")),
        group: group.map(str::to_string),
        scale: Scale::Pan,
        points: Vec::new(),
        enabled: true,
        current: 0.0,
    }
}

/// The names of `params`, in order.
fn names<'a>(params: &[&'a Param]) -> Vec<&'a str> {
    params.iter().map(|p| p.name.as_str()).collect()
}

/// A device's parameters without a group come first, and a group is listed once, under its
/// device, even when its parameters are not next to each other.
#[test]
fn the_menu_lists_each_device_with_loose_parameters_first_and_a_group_once() {
    let choices = [
        param(None, None, "Volume"),
        param(Some(0), Some("Op 1"), "Op 1 Rate"),
        param(Some(0), None, "Algorithm"),
        param(Some(0), Some("LFO"), "LFO Speed"),
        // Not next to the rest of its group: still listed under it.
        param(Some(0), Some("Op 1"), "Op 1 Level"),
        param(Some(1), Some("Op 1"), "Op 1 Rate"),
    ];
    let sections = sections(&choices, &[]);
    assert_eq!(sections.len(), 3);
    assert_eq!((sections[0].device, sections[0].name), (None, None));
    assert_eq!(names(&sections[0].loose), ["Volume"]);
    assert_eq!(names(&sections[1].loose), ["Algorithm"]);
    let groups: Vec<_> = sections[1].groups.iter().map(|(g, params)| (*g, names(params))).collect();
    assert_eq!(groups, [("Op 1", vec!["Op 1 Rate", "Op 1 Level"]), ("LFO", vec!["LFO Speed"])]);
    let order: Vec<_> = sections[1].params().map(|p| p.name.as_str()).collect();
    assert_eq!(order, ["Algorithm", "Op 1 Rate", "Op 1 Level", "LFO Speed"]);
    // The same group in another device is another group.
    assert_eq!(sections[2].device, Some(1));
    assert_eq!(sections[2].groups.len(), 1);
}

/// A search matches its words against the device, group and name together.
#[test]
fn a_search_matches_a_group_name_and_words_across_device_group_and_name() {
    let choices = [
        param(Some(0), Some("Op 1"), "Rate 1"),
        param(Some(0), Some("Op 3"), "Rate 1"),
        param(Some(0), Some("LFO"), "Speed"),
    ];
    let found = |query: &str| -> Vec<String> {
        sections(&choices, &query_words(query))
            .iter()
            .flat_map(Section::params)
            .map(|p| format!("{} {}", p.group.as_deref().unwrap_or(""), p.name))
            .collect()
    };
    assert_eq!(found("OP 3 rate"), ["Op 3 Rate 1"]);
    assert_eq!(found("lfo"), ["LFO Speed"]);
    assert_eq!(found("device 0 rate"), ["Op 1 Rate 1", "Op 3 Rate 1"]);
    assert!(found("op 2").is_empty());
}

/// A row under a group's heading drops the group's name in front, unless nothing would be left.
#[test]
fn a_name_under_its_group_leaves_out_the_group() {
    assert_eq!(param(Some(0), Some("Op 1"), "Op 1 Rate 1").short_name(), "Rate 1");
    assert_eq!(param(Some(0), Some("LFO"), "Speed").short_name(), "Speed");
    // Nothing would be left, or the group is only the start of a word.
    assert_eq!(param(Some(0), Some("Decay"), "Decay").short_name(), "Decay");
    assert_eq!(param(Some(0), Some("Op"), "Opacity").short_name(), "Opacity");
    assert_eq!(param(Some(0), None, "Op 1 Rate").short_name(), "Op 1 Rate");
    // The lane's own name keeps the whole name.
    assert_eq!(param(Some(0), Some("Op 1"), "Op 1 Rate 1").label(), "Device 0: Op 1 Rate 1");
}

/// Built-in and `.sd` parameters take their group from the device; the mixer's have none.
#[test]
fn built_in_and_sd_parameters_carry_their_group() {
    let mut project = crate::demo::demo_project();
    let track = project.tracks[0].id;
    let Some(channel) = project.tracks[0].channel.and_then(|id| project.channel_mut(id)) else {
        return;
    };
    channel.devices.push(suhina_engine::model::Device::new("FM Synth", DeviceKind::Builtin("fm-synth".into())));
    let mut graph =
        suhina_device_sdk::DeviceGraph::new("test.gain", "Test Gain", suhina_device_sdk::DeviceRole::Effect);
    let mut parameter = suhina_device_sdk::Parameter::new("level", "Level", (0.0, 1.0), 1.0);
    parameter.ui.group = "Output".into();
    graph.parameters.push(parameter);
    graph.nodes = vec![
        suhina_device_sdk::Node::audio_input("in"),
        suhina_device_sdk::Node::gain("gain", "level"),
        suhina_device_sdk::Node::audio_output("out"),
    ];
    graph.connections =
        vec![suhina_device_sdk::Connection::new("in", "gain"), suhina_device_sdk::Connection::new("gain", "out")];
    let state = graph.to_sd().ok().map(String::into_bytes);
    channel
        .devices
        .push(suhina_engine::model::Device { state, ..suhina_engine::model::Device::new("Test Gain", DeviceKind::Sd) });
    let params = Params::build(&project);
    let group = |device, id: &str| {
        params.param(track, &LaneKey { device: Some(device), param: id.into() }).and_then(|p| p.group.clone())
    };
    assert_eq!(group(0, "op3.rate1").as_deref(), Some("Op 3"));
    assert_eq!(group(0, "lfoSpeed").as_deref(), Some("LFO"));
    assert_eq!(group(0, "volume"), None);
    assert_eq!(group(1, "level").as_deref(), Some("Output"));
    assert!(params.of(track).iter().filter(|p| p.key.device.is_none()).all(|p| p.group.is_none()));
}

/// A choice point loaded outside a data-only device's range shows its nearest option.
#[test]
fn an_sd_choice_point_outside_its_range_names_the_nearest_option() {
    let mut shape = suhina_device_sdk::Parameter::new("shape", "Shape", (1.0, 3.0), 1.0);
    shape.control = suhina_device_sdk::ParameterControl::Choice;
    shape.ui.options = vec!["Sine".into(), "Square".into(), "Saw".into()];
    let scale = Scale::Sd(Box::new(shape));
    assert_eq!(scale.text(2.0), "Square");
    assert_eq!(scale.text(9.0), "Saw");
    assert_eq!(scale.text(-4.0), "Sine");
}
