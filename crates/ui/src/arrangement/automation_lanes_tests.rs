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

/// A whole-number lane places a value between two numbers where it falls, unrounded, and shows
/// what it plays: the value with its fraction cut off toward zero.
#[test]
fn a_whole_number_lane_places_values_between_numbers_and_shows_what_plays() {
    let integer = Scale::Device(ParamInfo::integer("steps", "Steps", (0, 3), 0, Unit::None));
    assert_eq!(integer.position(1.5), 0.5);
    assert_eq!(integer.text(1.5), "1");
    assert_eq!(integer.text(2.999), "2");
    let semitones = Scale::Device(ParamInfo::integer("t", "T", (-48, 48), 0, Unit::Semitones));
    assert_eq!(semitones.text(-0.94), "+0 st");
    assert_eq!(semitones.text(-2.97), "-2 st");
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

/// Moving values together keeps them in range; a silent volume rises from the lane's bottom
/// (-70 dB), so points picked at the bottom move with the rest, and a level moved down to the
/// bottom is silent (#916).
#[test]
fn a_shift_keeps_values_in_range_and_moves_silence_from_the_bottom() {
    assert!((gain_to_db(Scale::Volume.shift(0.0, 6.0)) + 64.0).abs() < 1e-9);
    assert!((gain_to_db(Scale::Volume.shift(0.0, 64.0)) + 6.0).abs() < 1e-9);
    assert_eq!(Scale::Volume.shift(10f64.powf(-60.0 / 20.0), -20.0), 0.0);
    assert!((gain_to_db(Scale::Volume.shift(1.0, 10.0)) - 6.0).abs() < 1e-9);
    assert_eq!(Scale::Pan.shift(0.5, 1.0), 1.0);
    assert_eq!(Scale::Mute.shift(1.0, -0.4), 1.0);
}

/// A track's lanes, opened the first time, are the top lane (the volume) and those with points
/// mounted under it (#1322).
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
    // The top lane shows the volume, the pan with its points is mounted under it (#1322).
    assert_eq!(state.shown(track, &params), [LaneKey::mixer(VOLUME), LaneKey::mixer(PAN)]);
    assert_eq!(state.height(track, &params), 2.0 * (LANE + LINE));
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
        id: Default::default(),
        name: "Utility".into(),
        kind: DeviceKind::Builtin("utility".into()),
        enabled: true,
        state: None,
        params: Vec::new(),
        automation: vec![lane],
        clip_host: false,
        extensions: Default::default(),
    });
    let params = Params::build(&project);
    let track = project.tracks[0].id;
    let gain = params.param(track, &LaneKey { device: Some(0), param: "gain".into() });
    assert_eq!(gain.map(Param::label).as_deref(), Some("Utility: Gain"));
    assert_eq!(gain.map(|p| p.points.len()), Some(1));
    let mut state = LanesState::default();
    state.toggle(track, &params);
    // Under the top lane's volume, as a lane with points (#1322).
    assert_eq!(
        state.shown(track, &params),
        [LaneKey::mixer(VOLUME), LaneKey { device: Some(0), param: "gain".into() }]
    );
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
        id: Default::default(),
        name: "Test Filter".into(),
        kind: DeviceKind::Sd,
        enabled: true,
        state: graph.to_sd().ok().map(String::into_bytes).map(Into::into),
        params: Vec::new(),
        automation: vec![lane],
        clip_host: false,
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
    channel.devices.push(suhina_engine::model::Device {
        id: Default::default(),
        state: state.map(Into::into),
        ..suhina_engine::model::Device::new("Test Gain", DeviceKind::Sd)
    });
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

/// A lane's plot of the default height (a 40-point lane inside its 4-point inset) and geometry
/// with a beat every 100 points.
fn plot_and_geometry() -> (Rect, Geometry) {
    let plot = plot(Rect::from_min_size(Pos2::ZERO, Vec2::new(1000.0, LANE)));
    let geometry = Geometry {
        time: egui::Rangef::new(0.0, 1000.0),
        headers: egui::Rangef::new(0.0, 0.0),
        visible: Rect::from_min_size(Pos2::ZERO, Vec2::splat(1000.0)),
        scroll_x: 0.0,
        beat_width: 100.0,
        grid: TimelineGrid::new(1.0, 100.0, 4.0, 8.0),
    };
    (plot, geometry)
}

/// A pan lane with one point at `value`, beat 4.
fn pan_lane(value: f64) -> Param {
    Param { points: vec![AutomationPoint::new(4.0, value)], ..param(None, None, "Pan") }
}

/// A drag of the lane's only point, taken hold of at `grab` (beat and place on the lane).
fn point_drag(grab: (f64, f64)) -> PointDrag {
    PointDrag {
        track: TrackId(1),
        key: LaneKey::mixer(PAN),
        point: 0,
        together: vec![0],
        grab,
        now: grab,
        segment: false,
        body: false,
    }
}

/// Within two points of a lane's top or bottom a place on it is the end itself, so a point a
/// pixel under the top is the maximum (#811); further in it stays where it is.
#[test]
fn a_place_next_to_a_lanes_top_or_bottom_is_the_end() {
    let (plot, _) = plot_and_geometry();
    assert_eq!(plot.height(), 32.0);
    let one_point = 1.0 / 32.0;
    assert_eq!(at_ends(plot, 1.0 - one_point), 1.0);
    assert_eq!(at_ends(plot, 1.0 - 2.0 * one_point), 1.0);
    assert_eq!(at_ends(plot, one_point), 0.0);
    assert_eq!(at_ends(plot, 0.0), 0.0);
    let inside = 1.0 - 3.0 * one_point;
    assert_eq!(at_ends(plot, inside), inside);
    assert_eq!(at_ends(plot, 0.5), 0.5);
    // A click a point under the top puts in full right, not 47R.
    assert_eq!(Scale::Pan.text(Scale::Pan.value_at(position_of(plot, plot.top() + 1.0))), "47R");
    assert_eq!(Scale::Pan.text(Scale::Pan.value_at(at_ends(plot, position_of(plot, plot.top() + 1.0)))), "50R");
    // The volume's top is its maximum, +6 dB.
    let top = Scale::Volume.value_at(at_ends(plot, position_of(plot, plot.top() + 1.0)));
    assert!((gain_to_db(top) - 6.0).abs() < 1e-9);
    // A plot of no height has no ends to snap to.
    assert_eq!(at_ends(Rect::from_min_size(Pos2::ZERO, Vec2::new(10.0, 0.0)), 0.99), 0.99);
}

/// A point dragged to a pixel under the top of a pan lane is let go of at 50R exactly, even when
/// it was taken hold of off its centre; the value moved there is the one the command sets, the
/// one the engine plays. A Shift drag, for fine values, does not jump to the end.
#[test]
fn a_point_dragged_next_to_the_top_or_bottom_gets_the_end_value() {
    let (plot, geometry) = plot_and_geometry();
    let param = pan_lane(0.0);
    // Taken hold of a little above the point's centre (C, the lane's middle).
    let grab = (4.0, 0.55);
    let start = grabbed_at(&param, &point_drag(grab));
    assert_eq!(start, 0.5);
    let near_top = position_of(plot, plot.top() + 1.0) + (grab.1 - start);
    let mut drag = point_drag(grab);
    drag.now = dragged_to(grab, 4.0, near_top, start, plot, &geometry, false);
    assert_eq!(dragged_text(&param, &drag, &geometry, false).as_deref(), Some("50R at beat 4"));
    let action = drop_points(&drag, &param, &geometry, false);
    assert_eq!(action.as_ref().map(|a| a.id), Some("automation.movePoint"));
    assert_eq!(action.map(|a| a.params["value"].clone()), Some(json!(1.0)));

    // To the bottom: 50L.
    let near_bottom = position_of(plot, plot.bottom() - 1.0) + (grab.1 - start);
    drag.now = dragged_to(grab, 4.0, near_bottom, start, plot, &geometry, false);
    assert_eq!(dragged_text(&param, &drag, &geometry, false).as_deref(), Some("50L at beat 4"));
    assert_eq!(drop_points(&drag, &param, &geometry, false).map(|a| a.params["value"].clone()), Some(json!(-1.0)));

    // Further in, the point follows the pointer.
    let inside = 0.8 + (grab.1 - start);
    drag.now = dragged_to(grab, 4.0, inside, start, plot, &geometry, false);
    assert_eq!(dragged_text(&param, &drag, &geometry, false).as_deref(), Some("30R at beat 4"));

    // With Shift the point moves a quarter of the pointer's way and stops short of the end.
    let fine = point_drag((4.0, 0.5));
    let mut fine_drag = fine.clone();
    fine_drag.now = dragged_to(fine.grab, 4.0, 0.5 + 4.0 * 0.47, 0.5, plot, &geometry, true);
    assert_eq!(dragged_text(&param, &fine_drag, &geometry, false).as_deref(), Some("47R at beat 4"));
}

/// The text shown while a point is dragged says where it will land, its beat on the grid.
#[test]
fn the_text_of_a_dragged_point_has_its_value_and_beat() {
    let (plot, geometry) = plot_and_geometry();
    let param = pan_lane(-0.5);
    let grab = (4.0, 0.25);
    let mut drag = point_drag(grab);
    drag.now = dragged_to(grab, 6.2, 0.75, grabbed_at(&param, &drag), plot, &geometry, false);
    assert_eq!(dragged_text(&param, &drag, &geometry, false).as_deref(), Some("25R at beat 6"));
    assert_eq!(dragged_text(&param, &drag, &geometry, true).as_deref(), Some("25R at beat 6.2"));
    let gone = PointDrag { point: 3, ..drag };
    assert_eq!(dragged_text(&param, &gone, &geometry, false), None);
}

/// A point taken hold of well off its centre still reaches the lane's ends: the pointer is
/// measured past the plot's top and bottom, not held in it (#811).
#[test]
fn a_point_taken_off_its_centre_reaches_the_lanes_ends() {
    let (plot, geometry) = plot_and_geometry();
    let param = pan_lane(0.0);
    // 4.8 points above the point's centre, near the edge of the square that takes it.
    let grab = (4.0, place_of(plot, plot.center().y - 4.8));
    let start = grabbed_at(&param, &point_drag(grab));
    let mut drag = point_drag(grab);
    // The pointer at the plot's top would leave the point 4.8 points short of it; above the top
    // it reaches the end.
    assert!((place_of(plot, plot.top() - 10.0) - (1.0 + 10.0 / 32.0)).abs() < 1e-9);
    drag.now = dragged_to(grab, 4.0, place_of(plot, plot.top() - 10.0), start, plot, &geometry, false);
    assert_eq!(dragged_text(&param, &drag, &geometry, false).as_deref(), Some("50R at beat 4"));
    assert_eq!(drop_points(&drag, &param, &geometry, false).map(|a| a.params["value"].clone()), Some(json!(1.0)));
    drag.now = dragged_to(grab, 4.0, place_of(plot, plot.bottom() + 10.0), start, plot, &geometry, false);
    assert_eq!(dragged_text(&param, &drag, &geometry, false).as_deref(), Some("50L at beat 4"));
    assert_eq!(drop_points(&drag, &param, &geometry, false).map(|a| a.params["value"].clone()), Some(json!(-1.0)));
}

/// The text shown while dragging is that of the value the command sets: a pan rounded to a
/// thousandth (0.2499 sets 0.25, 13R, not the 12R its unrounded value reads), a volume to a
/// hundredth of a decibel, silent at its floor.
#[test]
fn the_dragged_text_is_that_of_the_value_the_command_sets() {
    assert_eq!(Scale::Pan.text(0.2499), "12R");
    assert_eq!(Scale::Pan.committed(0.2499), 0.25);
    assert_eq!(Scale::Pan.text(Scale::Pan.committed(0.2499)), "13R");
    assert_eq!(Scale::Pan.command(0.2499), json!(0.25));
    let gain = 10f64.powf(-6.004 / 20.0);
    assert!((gain_to_db(Scale::Volume.committed(gain)) + 6.0).abs() < 1e-9);
    assert_eq!(Scale::Volume.committed(0.0), 0.0);
    assert_eq!(Scale::Volume.committed(10f64.powf(-80.0 / 20.0)), 0.0);
    assert_eq!(Scale::Mute.committed(0.7), 1.0);
}

/// The parameter list a hosted plug-in gives: a CLAP gain in 0 … 1 with its text for the value
/// it listed, a 4-step choice in a group, a hidden and a read-only parameter, and one it does not
/// let be automated.
fn plugin_list() -> std::sync::Arc<[suhina_plugins::ParamInfo]> {
    let param = |id: u32, name: &str| suhina_plugins::ParamInfo {
        id,
        name: name.into(),
        group: String::new(),
        min: 0.0,
        max: 1.0,
        default: 0.5,
        value: 0.5,
        text: String::new(),
        steps: 0,
        automatable: true,
        read_only: false,
        hidden: false,
    };
    vec![
        suhina_plugins::ParamInfo { text: "50 %".into(), ..param(0, "Gain") },
        suhina_plugins::ParamInfo {
            group: "Osc".into(),
            min: 0.0,
            max: 3.0,
            steps: 3,
            value: 1.0,
            ..param(4, "Osc Shape")
        },
        suhina_plugins::ParamInfo { hidden: true, ..param(5, "Hidden") },
        suhina_plugins::ParamInfo { read_only: true, ..param(6, "Meter") },
        suhina_plugins::ParamInfo { automatable: false, ..param(7, "Bypass") },
    ]
    .into()
}

/// A project whose first track's channel runs a CLAP plug-in, with a lane on parameter 9, which
/// the plug-in does not list.
fn plugin_project() -> (Project, TrackId) {
    let mut project = crate::demo::demo_project();
    let track = project.tracks[0].id;
    let channel = project.tracks[0].channel.unwrap();
    let mut device = suhina_engine::model::Device::new(
        "Synth",
        suhina_engine::model::DeviceKind::Clap { id: "com.example.synth".into() },
    );
    let mut lane = AutomationLane::new("9");
    lane.points = vec![AutomationPoint::new(0.0, 0.0), AutomationPoint::new(4.0, 2.0)].into();
    device.automation.push(lane);
    project.channel_mut(channel).unwrap().devices.insert(0, device);
    (project, track)
}

/// A hosted plug-in's parameters are rows of the same list as a built-in device's: under the
/// device's name, in the plug-in's own groups, only those it lets be automated, seen and changed;
/// a lane on a parameter it does not list stays with its points and says so (#1205).
#[test]
fn a_plug_in_s_parameters_are_listed_like_a_device_s() {
    let (project, track) = plugin_project();
    // Without the plug-in's list (it has not answered yet) only its lane is there.
    let alone = Params::build(&project);
    let names: Vec<String> = alone.of(track).iter().filter(|p| p.key.device == Some(0)).map(Param::label).collect();
    assert_eq!(names, ["Synth: Parameter 9 (not available)"]);
    let missing = alone.param(track, &LaneKey { device: Some(0), param: "9".into() }).unwrap();
    assert_eq!(missing.points.len(), 2, "the lane keeps its points");
    assert_eq!(missing.scale.position(2.0), 1.0, "its range reaches its points");

    let mut lists = PluginLists::default();
    lists.lists.insert((track, 0), plugin_list());
    let params = Params::build_with(&project, lists.clone());
    let rows: Vec<(String, Option<&str>)> =
        params.of(track).iter().filter(|p| p.key.device == Some(0)).map(|p| (p.label(), p.group.as_deref())).collect();
    assert_eq!(
        rows,
        [
            ("Synth: Gain".to_string(), None),
            ("Synth: Osc Shape".to_string(), Some("Osc")),
            ("Synth: Parameter 9 (not available)".to_string(), None),
        ]
    );
    // The search finds it by the device and the group, as a device's.
    let shape = params.param(track, &LaneKey { device: Some(0), param: "4".into() }).unwrap();
    assert!(shape.search_text().contains("synth osc"));
    assert_eq!(shape.short_name(), "Shape");
    assert_eq!(shape.current, 1.0, "the value it listed");
    // Built again with the very same lists, nothing changes; a new list builds them again.
    let mut again = params.clone();
    again.take_plugins(&project, lists);
    assert_eq!(again, params);
}

/// A plug-in lane's scale: evenly over the plug-in's range, on its steps for a stepped one, and
/// the plug-in's own text for a value it named; a value it did not name shows the number and is
/// asked about (#1205).
#[test]
fn a_plug_in_lane_takes_the_plug_in_s_range_steps_and_text() {
    let (project, track) = plugin_project();
    let mut lists = PluginLists::default();
    lists.lists.insert((track, 0), plugin_list());
    let mut texts = std::collections::HashMap::new();
    texts.insert((0, 0.25f64.to_bits()), "25 %".to_string());
    lists.texts.insert((track, 0), std::sync::Arc::new(std::sync::Mutex::new(texts)));
    let params = Params::build_with(&project, lists);
    let gain = params.param(track, &LaneKey { device: Some(0), param: "0".into() }).unwrap();
    assert!(!gain.scale.stepped());
    assert_eq!(gain.scale.position(0.25), 0.25);
    assert_eq!(gain.scale.text(0.5), "50 %", "the text it listed");
    assert_eq!(gain.scale.text(0.25), "25 %", "a text it gave since");
    assert_eq!(gain.scale.text(0.75), "0.750", "not named yet: the number");
    assert_eq!(gain.scale.command(1.5), json!(1.0), "kept in range");
    let shape = params.param(track, &LaneKey { device: Some(0), param: "4".into() }).unwrap();
    assert!(shape.scale.stepped());
    assert_eq!(shape.scale.value_at(0.4), 1.0, "on its nearest step");
    assert_eq!(shape.scale.text(2.0), "2");
    assert_eq!(shape.scale.nudge_step(), 1.0, "Up and Down move a step");
    // The values it was asked about and did not name go to the app to ask, once.
    let asked = params.asked_texts();
    let mut asked: Vec<(u32, Vec<f64>)> = asked.into_iter().map(|(_, _, id, values)| (id, values)).collect();
    asked.sort_by_key(|(id, _)| *id);
    assert_eq!(asked, [(0, vec![0.75]), (4, vec![2.0])]);
    assert!(params.asked_texts().is_empty());
}

/// A plug-in's range is not trusted: an empty or upside-down one and numbers that are not finite
/// stay on the lane (#1205).
#[test]
fn a_plug_in_scale_survives_hostile_ranges() {
    let upside_down = PluginScale { min: 1.0, max: -1.0, ..PluginScale::default() };
    assert_eq!(upside_down.snap(5.0), 1.0);
    assert_eq!(Scale::Plugin(Box::new(upside_down)).position(0.0), 0.5);
    let empty = Scale::Plugin(Box::new(PluginScale { min: 2.0, max: 2.0, steps: 4, ..PluginScale::default() }));
    assert_eq!((empty.position(2.0), empty.value_at(0.7)), (0.0, 2.0));
    let wild = Scale::Plugin(Box::new(PluginScale { min: f64::NAN, max: f64::INFINITY, ..PluginScale::default() }));
    assert_eq!(wild.value_at(0.5), 0.5, "0 … 1 instead");
    assert_eq!(wild.position(f64::NAN), 0.0);
}

/// The demo project's first track with `layout` as its lanes' layout, its lanes open, and the
/// parameters built from it.
fn laid_out(layout: Option<Layout>) -> (LanesState, TrackId, Params) {
    let mut project = crate::demo::demo_project();
    let track = project.tracks[0].id;
    let channel = project.tracks[0].channel.unwrap();
    if let Some(layout) = layout {
        let lanes = layout.lanes.iter().map(|k| suhina_engine::model::LaneRef::mixer(k.param.clone())).collect();
        let groups = layout
            .groups
            .iter()
            .map(|g| suhina_engine::model::LaneGroup {
                name: g.name.clone(),
                lanes: g.keys.iter().map(|k| suhina_engine::model::LaneRef::mixer(k.param.clone())).collect(),
                folded: g.folded,
                device: None,
            })
            .collect();
        project.lane_layouts.push(suhina_engine::model::LaneLayout { channel, lanes, groups });
    }
    let params = Params::build(&project);
    let mut lanes = LanesState::default();
    lanes.open.insert(track);
    (lanes, track, params)
}

fn mixer(keys: &[&str]) -> Vec<LaneKey> {
    keys.iter().map(|k| LaneKey::mixer(k)).collect()
}

/// The top lane shows the parameter worked on: before anything is chosen, the one the setting
/// names (the volume by default) unless it is mounted, never the next free one; a control moved
/// by hand shows there unless its lane is mounted; the top lane's x leaves it empty, never gone
/// (#1322).
#[test]
fn the_top_lane_shows_the_parameter_worked_on() {
    let (mut lanes, track, params) = laid_out(Some(Layout::default()));
    assert_eq!(lanes.active(track, &params), Some(LaneKey::mixer(VOLUME)), "the setting's default");
    lanes.top_lane = TopLane::Pan;
    assert_eq!(lanes.active(track, &params), Some(LaneKey::mixer(PAN)));
    lanes.top_lane = TopLane::None;
    assert_eq!(lanes.rows(track, &params), [LaneRow::Empty]);
    let (mut lanes, track, params) = laid_out(Some(Layout { lanes: mixer(&[VOLUME]), groups: Vec::new() }));
    assert_eq!(lanes.active(track, &params), None, "volume is mounted, and the pan is not shown instead");
    assert_eq!(lanes.rows(track, &params), [LaneRow::Empty, LaneRow::Lane(LaneKey::mixer(VOLUME))]);
    // Moved by hand: the mute shows at the top; the mounted volume stays in its own lane.
    lanes.focus(track, LaneKey::mixer(MUTE), &params);
    assert_eq!(lanes.active(track, &params), Some(LaneKey::mixer(MUTE)));
    lanes.focus(track, LaneKey::mixer(VOLUME), &params);
    assert_eq!(lanes.active(track, &params), Some(LaneKey::mixer(MUTE)), "a mounted lane is edited in place");
    assert_eq!(lanes.last_lane, Some((track, LaneKey::mixer(VOLUME))));
    // Cleared, the top lane stays as an empty row.
    lanes.clear_active(track);
    assert_eq!(lanes.rows(track, &params)[0], LaneRow::Empty);
    // Closed lanes stay closed.
    lanes.open.clear();
    lanes.focus(track, LaneKey::mixer(PAN), &params);
    assert!(lanes.rows(track, &params).is_empty());
}

/// A track never laid out shows its automated lanes mounted, and all three of volume, pan and
/// mute mounted leave the top lane empty (#1322).
#[test]
fn a_track_never_laid_out_mounts_its_automated_lanes() {
    let (lanes, track, params) = laid_out(None);
    let automated: Vec<LaneKey> =
        params.of(track).iter().filter(|p| !p.points.is_empty()).map(|p| p.key.clone()).collect();
    assert_eq!(lanes.mounted(track, &params), automated);
    let (lanes, track, params) = laid_out(Some(Layout { lanes: mixer(&[VOLUME, PAN, MUTE]), groups: Vec::new() }));
    assert_eq!(lanes.active(track, &params), None);
    assert_eq!(lanes.rows(track, &params)[0], LaneRow::Empty);
}

/// The top lane's plus mounts its parameter right under it, as one command; the top lane is then
/// empty, not the next of volume, pan and mute; the top lane's height goes with a parameter
/// chosen in it (#1322).
#[test]
fn the_top_lanes_plus_mounts_it() {
    let (mut lanes, track, params) = laid_out(Some(Layout::default()));
    let action = lanes.mount(track, &LaneKey::mixer(VOLUME), &params);
    assert_eq!(action.id, "automation.setLayout");
    assert_eq!(action.params["lanes"], json!([{ "param": "volume" }]));
    assert_eq!(lanes.active(track, &params), None, "empty after the plus");
    lanes.focus(track, LaneKey::mixer(MUTE), &params);
    assert_eq!(lanes.active(track, &params), Some(LaneKey::mixer(MUTE)), "a control moved shows again");
    lanes.set_active(track, LaneKey::mixer(PAN), &params);
    lanes.set_lane_height(track, &LaneKey::mixer(PAN), 90.0);
    lanes.set_active(track, LaneKey::mixer(MUTE), &params);
    assert_eq!(lanes.lane_height(track, &LaneKey::mixer(MUTE)), 90.0);
}

/// A layout's lanes move, group, ungroup and go; groups keep their lanes together; a lane
/// dropped among a group's joins it, one dragged out leaves it, one dropped on a folded group goes
/// in at its end; a plug-in's mounted lane goes into its device's group (#1322).
#[test]
fn a_layout_moves_groups_and_mounts_lanes() {
    let (v, p, m) = (LaneKey::mixer(VOLUME), LaneKey::mixer(PAN), LaneKey::mixer(MUTE));
    let mut layout = Layout { lanes: vec![v.clone(), p.clone(), m.clone()], groups: Vec::new() };
    layout.move_lanes(std::slice::from_ref(&m), &Place::Before { key: v.clone(), join: false }, false);
    assert_eq!(layout.lanes, [m.clone(), v.clone(), p.clone()]);
    let index = layout.group(&[m.clone(), p.clone()]).unwrap();
    assert_eq!(layout.groups[index].name, "Group 1");
    assert_eq!(layout.lanes, [m.clone(), p.clone(), v.clone()], "kept together at the first");
    layout.move_lanes(std::slice::from_ref(&v), &Place::Before { key: p.clone(), join: true }, false);
    assert!(layout.groups[index].keys.contains(&v));
    layout.move_lanes(std::slice::from_ref(&v), &Place::End, false);
    assert!(!layout.groups[index].keys.contains(&v));
    layout.groups[index].folded = true;
    layout.move_lanes(std::slice::from_ref(&v), &Place::Into(index), false);
    assert_eq!(layout.groups[index].keys.last(), Some(&v), "dropped into the folded group");
    assert_eq!(layout.group(std::slice::from_ref(&v)).map(|i| layout.groups[i].name.clone()), Some("Group 2".into()));
    layout.ungroup(&[0]);
    layout.remove(std::slice::from_ref(&m), &[]);
    assert!(!layout.lanes.contains(&m));
    // A device's lanes mount into its group, a second device of the same name into its own.
    let (a, b) = (LaneKey { device: Some(1), param: "7".into() }, LaneKey { device: Some(2), param: "7".into() });
    let mut layout = Layout::default();
    layout.mount(&a, Some("Serum 2"));
    layout.mount(&LaneKey { device: Some(1), param: "8".into() }, Some("Serum 2"));
    layout.mount(&b, Some("Serum 2"));
    let names: Vec<&str> = layout.groups.iter().map(|g| g.name.as_str()).collect();
    assert!(names.contains(&"Serum 2") && names.contains(&"Serum 2 2"), "{names:?}");
    assert_eq!(layout.groups.iter().find(|g| g.device == Some(1)).map(|g| g.keys.len()), Some(2));
    // Without the setting, a mounted lane stands on its own.
    let mut plain = Layout::default();
    plain.mount(&a, None);
    assert!(plain.groups.is_empty());
}

/// A lane dropped on a group's heading lands before the group, outside it, even the group's own
/// first lane; dropped before one of the group's lanes it joins the group (#1322).
#[test]
fn a_lane_dropped_on_a_groups_heading_lands_outside_the_group() {
    let (v, p, m) = (LaneKey::mixer(VOLUME), LaneKey::mixer(PAN), LaneKey::mixer(MUTE));
    let gain = LaneKey { device: Some(0), param: "7".into() };
    let group = |keys: Vec<LaneKey>| LaneGroup { name: "Synth".into(), keys, folded: false, device: Some(0) };
    // Mute, then the group: pan (its first lane) and the plug-in's lane, then volume.
    let start = Layout {
        lanes: vec![m.clone(), p.clone(), gain.clone(), v.clone()],
        groups: vec![group(vec![p.clone(), gain.clone()])],
    };
    // The group's own first lane dropped on its heading leaves the group and stays above it.
    let mut layout = start.clone();
    layout.move_lanes(std::slice::from_ref(&p), &Place::BeforeGroup(0), false);
    assert_eq!(layout.lanes, [m.clone(), p.clone(), gain.clone(), v.clone()]);
    assert_eq!(layout.groups[0].keys, std::slice::from_ref(&gain));
    // Another lane dropped on the heading lands above it, outside the group.
    let mut layout = start.clone();
    layout.move_lanes(std::slice::from_ref(&v), &Place::BeforeGroup(0), false);
    assert_eq!(layout.lanes, [m.clone(), v.clone(), p.clone(), gain.clone()]);
    assert_eq!(layout.groups[0].keys, [p.clone(), gain.clone()]);
    // Dropped before the group's first lane, under the heading, it joins the group.
    let mut layout = start.clone();
    layout.move_lanes(std::slice::from_ref(&m), &Place::Before { key: p.clone(), join: true }, false);
    assert_eq!(layout.lanes, [m.clone(), p.clone(), gain.clone(), v.clone()]);
    assert!(layout.groups[0].keys.contains(&m));
    // A lane dropped where it already is changes nothing.
    let mut layout = start.clone();
    layout.move_lanes(std::slice::from_ref(&p), &Place::Before { key: p.clone(), join: true }, false);
    assert_eq!(layout, start);
    // The whole group dropped on its own heading stays as it is.
    let mut layout = start.clone();
    layout.move_lanes(&[p.clone(), gain.clone()], &Place::BeforeGroup(0), true);
    assert_eq!(layout, start);
}

/// Lanes are picked by their headers, groups by their headings; Delete takes the picked out of
/// the layout as one command (#1322).
#[test]
fn picked_lanes_and_groups_are_deleted_in_one_command() {
    let (v, p, m) = (LaneKey::mixer(VOLUME), LaneKey::mixer(PAN), LaneKey::mixer(MUTE));
    let group = LaneGroup { name: "G".into(), keys: vec![p.clone(), m.clone()], folded: false, device: None };
    let (mut lanes, track, params) =
        laid_out(Some(Layout { lanes: vec![v.clone(), p.clone(), m.clone()], groups: vec![group] }));
    lanes.pick(track, &v, egui::Modifiers::NONE, &params);
    lanes.pick(track, &m, egui::Modifiers::SHIFT, &params);
    assert_eq!(lanes.picked.len(), 3);
    lanes.pick(track, &p, egui::Modifiers::COMMAND, &params);
    assert!(!lanes.is_picked(track, &p));
    assert_eq!(lanes.dragged_with(track, &m, &params), [v.clone(), m.clone()]);
    let layout = lanes.layout(track, &params);
    lanes.pick_group(track, 0, egui::Modifiers::NONE, &layout);
    assert!(lanes.picked.is_empty() && lanes.has_picks());
    let action = lanes.delete_picked(&params).unwrap();
    assert_eq!(action.params["lanes"], json!([{ "param": "volume" }]));
    assert_eq!(action.params["groups"], json!([]));
    assert!(!lanes.has_picks());
}

/// A parameter moved in a plug-in's own editor moves its lane's faint line at once, not only when
/// the plug-in lists its parameters again (#1205).
#[test]
fn a_plug_in_parameter_moved_in_its_editor_moves_its_line_at_once() {
    let (project, track) = plugin_project();
    let mut lists = PluginLists::default();
    lists.lists.insert((track, 0), plugin_list());
    let mut params = Params::build_with(&project, lists.clone());
    let gain = LaneKey { device: Some(0), param: "0".into() };
    assert_eq!(params.param(track, &gain).map(|p| p.current), Some(0.5));
    let mut moved = lists.clone();
    moved.moved.push((track, 0, 0, 0.8));
    params.take_plugins(&project, moved);
    assert_eq!(params.param(track, &gain).map(|p| p.current), Some(0.8));
    // The same lists: nothing else was built again.
    assert!(params.param(track, &LaneKey { device: Some(0), param: "4".into() }).is_some());
}

/// The range a duplicate moved goes back with that duplicate's undo and on again with its redo,
/// also once the session's history is full and its oldest step goes as each new one comes; an
/// undo of another step leaves it (#1322).
#[test]
fn a_duplicates_undo_brings_the_range_back_with_a_full_history() {
    let track = TrackId(1);
    let painted = LaneRange { track, key: LaneKey::mixer(VOLUME), start: 0.0, end: 4.0, more: Vec::new() };
    let mut lanes = LanesState { range: Some(painted.clone()), ..LanesState::default() };
    let full = 256;
    // The duplicate: a new step, the depth staying at the limit.
    assert_eq!(lanes.range_actions("duplicate").len(), 1);
    lanes.follow_history("automation.duplicate", (full, 10), (full, 11));
    let moved = lanes.range.clone();
    assert_eq!(moved.as_ref().map(|r| (r.start, r.end)), Some((4.0, 8.0)));
    // Another edit after it, at the limit too.
    lanes.follow_history("automation.addPoint", (full, 11), (full, 12));
    // Its undo leaves the range; the next undo, the duplicate's, brings it back.
    lanes.follow_history("edit.undo", (full, 12), (full - 1, 12));
    assert_eq!(lanes.range, moved, "the point's undo");
    lanes.follow_history("edit.undo", (full - 1, 12), (full - 2, 12));
    assert_eq!(lanes.range, Some(painted.clone()), "the duplicate's undo");
    lanes.follow_history("edit.redo", (full - 2, 12), (full - 1, 12));
    assert_eq!(lanes.range, moved, "its redo");
    // A step another project made (a depth the lanes did not follow) moves nothing.
    lanes.follow_history("edit.undo", (3, 40), (2, 40));
    assert_eq!(lanes.range, moved);
}

/// A group heading picked in one layout is let go of once the track's layout changes (an undo
/// adds or takes away a group before it), so Delete never acts on another group (#1322).
#[test]
fn picked_groups_are_let_go_of_when_the_layout_changes() {
    let track = TrackId(1);
    let (v, p) = (LaneKey::mixer(VOLUME), LaneKey::mixer(PAN));
    let group = |name: &str, key: &LaneKey| LaneGroup {
        name: name.into(),
        keys: vec![key.clone()],
        folded: false,
        device: None,
    };
    let two = Layout { lanes: vec![v.clone(), p.clone()], groups: vec![group("A", &v), group("B", &p)] };
    let mut lanes = LanesState::default();
    lanes.pick_group(track, 1, egui::Modifiers::NONE, &two);
    lanes.keep_picks(track, &two);
    assert_eq!(lanes.picked_groups, [(track, 1)], "the same layout keeps the pick");
    // An undo took group A away: index 1 is no group now, and B is index 0.
    let one = Layout { lanes: vec![v.clone(), p.clone()], groups: vec![group("B", &p)] };
    lanes.keep_picks(track, &one);
    assert!(lanes.picked_groups.is_empty());
}
