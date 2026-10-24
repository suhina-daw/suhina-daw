// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

/// A pending read whose instance was detached while the host held its control.
fn detached_gather() -> Rack {
    Rack {
        state_gather: Some(StateGather {
            out: vec![1],
            deferred: Vec::new(),
            back: Default::default(),
            states: Default::default(),
            fallback: [(1, SavedState::from(vec![1]))].into(),
            names: [(1, "Test instrument".to_string())].into(),
        }),
        ..Rack::default()
    }
}

#[test]
fn export_gather_consumes_a_detached_reply_without_using_the_cached_state() {
    let mut rack = detached_gather();
    let fresh = SavedState::from(vec![2]);
    rack.returned_sender
        .send(Returned { key: 1, generation: 0, control: None, done: Done::Saved(Some(fresh.clone())) })
        .unwrap();
    assert!(rack.gather_states(Instant::now() + SAVE_WAIT));
    assert_eq!(rack.known_states().get(&1), Some(&fresh));
    assert!(rack.notices.is_empty());
}

#[test]
fn export_gather_names_a_failed_read_before_using_its_fallback() {
    let mut rack = detached_gather();
    rack.returned_sender.send(Returned { key: 1, generation: 0, control: None, done: Done::Saved(None) }).unwrap();
    // A normal frame may receive the reply before the export pumps its gather.
    rack.take_returned(&mut Vec::new());
    assert!(rack.gather_states(Instant::now() + SAVE_WAIT));
    assert_eq!(rack.known_states().get(&1), Some(&SavedState::from(vec![1])));
    assert!(rack.notices.iter().any(|n| n.contains("Test instrument") && n.contains("failed")), "{:?}", rack.notices);
}

#[test]
fn a_backup_keeps_the_last_completed_state_while_a_worker_has_the_control_half() {
    let mut rack = Rack::default();
    let mut instance = Instance::new(PluginStatus::Running, None, 0);
    let state = SavedState::from(vec![3, 2, 1]);
    instance.last_state = Some(state.clone());
    rack.instances.insert(1, instance);
    assert_eq!(rack.known_states().get(&1), Some(&state));
}

#[test]
fn polling_a_busy_host_returns_without_waiting_and_finishes_at_the_deadline() {
    let mut rack = Rack::default();
    rack.instances.insert(1, Instance::new(PluginStatus::Running, None, 0));
    let started = Instant::now();
    assert!(!rack.poll_edits());
    assert!(started.elapsed() < Duration::from_millis(100));
    rack.edit_check = Some((Instant::now(), 2));
    assert!(rack.poll_edits());
    assert!(rack.edit_check.is_none());
    assert!(!rack.poll_edits(), "the next quit starts a fresh check");
}

#[test]
fn checking_a_busy_host_has_one_shared_deadline() {
    let mut rack = Rack::default();
    // The control half is on a worker that never returns. This must not cost a wait
    // per read, per plug-in, or per host when the window is leaving the project.
    for key in 1..=8 {
        rack.instances.insert(key, Instance::new(PluginStatus::Running, None, 0));
    }
    let started = Instant::now();
    rack.check_edits();
    assert!(started.elapsed() < SAVE_WAIT + Duration::from_millis(800));
}

#[test]
fn progress_counts_each_state() {
    let mut rack = Rack::default();
    let instance =
        |status: PluginStatus, name: &str| Instance::new(status, Some((name.to_string(), "V".into(), "same")), 0);
    rack.instances.insert(1, instance(PluginStatus::Running, "A"));
    rack.instances.insert(2, instance(PluginStatus::Loading, "C"));
    rack.instances.insert(3, instance(PluginStatus::Loading, "B"));
    rack.instances.insert(4, instance(PluginStatus::Missing, "D"));
    rack.instances.insert(5, instance(PluginStatus::Failed("no".into()), "E"));
    assert_eq!(
        rack.progress(),
        PluginProgress {
            total: 4,
            ready: 1,
            loading: vec!["B".into(), "C".into()],
            missing: 1,
            failed: vec!["E".into()]
        }
    );
    // A new catalog looks for the missing and failed ones again.
    rack.set_env(std::sync::Arc::new(PluginEnv::new(Catalog::new(), PathBuf::from("host"))));
    rack.set_env(std::sync::Arc::new(PluginEnv::new(Catalog::new(), PathBuf::from("host"))));
    assert_eq!(rack.progress().missing, 0);
    assert!(rack.progress().failed.is_empty());
    assert_eq!(rack.progress().total, 3);
}

#[test]
fn a_failed_listing_is_tried_again_as_seldom_as_a_good_one() {
    let now = Instant::now();
    let mut instance = Instance::new(PluginStatus::Running, Some(("A".into(), "V".into(), "same")), 0);
    assert!(instance.list_due(now), "never listed");
    instance.listing = true;
    assert!(!instance.list_due(now + LIST_EVERY * 3), "one is under way");
    // The plug-in did not answer: nothing is listed, and it is not asked again at once.
    instance.list_done(None, now);
    assert!(!instance.listing && instance.listed.is_none());
    assert!(!instance.list_due(now), "not on the next frame");
    assert!(!instance.list_due(now + LIST_EVERY - Duration::from_millis(1)));
    assert!(instance.list_due(now + LIST_EVERY));
    // An answer is kept and waits as long.
    let param = suhina_plugins::ParamInfo {
        id: 7,
        name: "Gain".into(),
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
    let later = now + LIST_EVERY;
    instance.list_done(Some(vec![param]), later);
    assert_eq!(instance.ranges, Some(vec![(7, 0.0, 1.0)]));
    assert!(!instance.list_due(later) && instance.list_due(later + LIST_EVERY));
    // A failure afterwards keeps the list it had.
    instance.list_done(None, later + LIST_EVERY);
    assert!(instance.listed.is_some());
    // A plug-in that does not run is not asked.
    instance.status = PluginStatus::Loading;
    assert!(!instance.list_due(later + LIST_EVERY * 9));
}

#[test]
fn releases_of_a_block_not_sent_are_kept_for_the_next() {
    let events = [
        Event::note_on(1, 0, 60, 1.0),
        Event::note_off(2, 0, 61, 0.5),
        Event { frame: 3, note: -1, kind: EventKind::Controller { channel: 0, number: SUSTAIN, value: 0.0 } },
        Event { frame: 3, note: -1, kind: EventKind::Controller { channel: 0, number: 1, value: 0.3 } },
    ];
    let mut carried = Vec::with_capacity(2);
    carry_releases(&events, &mut carried);
    assert_eq!(carried, [events[1], events[2]]);
    // Never past the room made for them.
    carry_releases(&events, &mut carried);
    assert_eq!(carried.len(), 2);
}

#[test]
fn carried_parameter_values_leave_room_for_the_songs_place_and_wait_for_the_next_block() {
    // A full carry of parameter values: the block takes all but the song's places, and the
    // rest waits; the vectors never grow past what they were made with.
    let mut carried: Vec<Event> = (0..8).map(|id| Event::param(5, id, 0.5)).collect();
    carried.push(Event::note_off(3, 0, 60, 0.0));
    let (mut params, mut notes) = (Vec::with_capacity(8), Vec::with_capacity(8));
    take_carried(&mut carried, &mut params, &mut notes, SONG_EVENTS);
    assert_eq!(params.len(), 8 - SONG_EVENTS);
    assert!(params.iter().all(|e| e.frame == 0));
    assert_eq!(notes, [Event::note_off(0, 0, 60, 0.0)]);
    assert_eq!(carried.len(), SONG_EVENTS, "the rest is kept for the next block");
    for _ in 0..SONG_EVENTS {
        params.push(Event::tempo(120.0));
    }
    assert_eq!(params.capacity(), 8);
}

#[test]
fn parameter_values_of_a_block_not_sent_are_kept_the_latest_of_each() {
    let events =
        [Event::param(0, 3, 0.1), Event::note_on(1, 0, 60, 1.0), Event::param(5, 3, 0.2), Event::param(6, 4, 1.0)];
    let mut carried = Vec::with_capacity(4);
    carry_releases(&events, &mut carried);
    assert_eq!(carried, [Event::param(5, 3, 0.2), Event::param(6, 4, 1.0)]);
}

#[test]
fn a_bend_and_pressure_of_a_block_not_sent_are_kept_the_latest_of_each_channel() {
    let bend = |frame, channel, value| Event { frame, note: -1, kind: EventKind::PitchBend { channel, value } };
    let pressure = |frame, channel, value| Event { frame, note: -1, kind: EventKind::Pressure { channel, value } };
    // The wheel let go (back to 0) and the key's pressure released, in the block not sent.
    let events = [bend(0, 0, 0.8), pressure(1, 0, 0.6), bend(2, 1, -0.5), bend(3, 0, 0.0), pressure(4, 0, 0.0)];
    let mut carried = Vec::with_capacity(8);
    carry_releases(&events, &mut carried);
    assert_eq!(carried, [bend(3, 0, 0.0), pressure(4, 0, 0.0), bend(2, 1, -0.5)]);
}

#[test]
fn parameter_values_and_notes_merge_in_frame_order() {
    let params = [Event::param(0, 1, 0.5), Event::param(4, 1, 0.75)];
    let notes = [Event::note_on(0, 0, 60, 1.0), Event::note_off(2, 0, 60, 0.5), Event::note_on(4, 0, 62, 1.0)];
    let mut out = Vec::with_capacity(8);
    merge(&params, &notes, &mut out);
    assert_eq!(out, [params[0], notes[0], notes[1], params[1], notes[2]]);
    // Never past the room made for them.
    let mut short = Vec::with_capacity(2);
    merge(&params, &notes, &mut short);
    assert_eq!(short, [params[0], notes[0]]);
}

#[test]
fn notes_become_the_hosts_events() {
    let mut out = Vec::with_capacity(8);
    push_event(&mut out, 3, DeviceEvent::On { key: 60, velocity: 2.0 });
    push_event(&mut out, 4, DeviceEvent::Off { key: 200, velocity: 0.5 });
    push_event(&mut out, 5, DeviceEvent::PitchBend(-3.0));
    push_event(&mut out, 6, DeviceEvent::Pressure(0.5));
    push_event(&mut out, 7, DeviceEvent::AllNotesOff);
    assert_eq!(
        out,
        [
            Event { frame: 3, note: -1, kind: EventKind::NoteOn { channel: 0, key: 60, velocity: 1.0 } },
            Event { frame: 5, note: -1, kind: EventKind::PitchBend { channel: 0, value: -1.0 } },
            Event { frame: 6, note: -1, kind: EventKind::Pressure { channel: 0, value: 0.5 } },
            Event { frame: 7, note: -1, kind: EventKind::Controller { channel: 0, number: 64, value: 0.0 } },
            Event { frame: 7, note: -1, kind: EventKind::Controller { channel: 0, number: 123, value: 0.0 } },
        ]
    );
    // Never more than the room made for them.
    let mut full = Vec::with_capacity(1);
    push_event(&mut full, 0, DeviceEvent::AllNotesOff);
    assert_eq!(full.len(), 1);
}

#[test]
fn instruments_are_told_by_their_features() {
    let plugin = |features: &[&str]| PluginInfo {
        path: PathBuf::from("x.vst3"),
        id: "x".into(),
        name: "X".into(),
        vendor: "V".into(),
        version: "1".into(),
        features: features.iter().map(|f| f.to_string()).collect(),
        replaces: Vec::new(),
        ara: false,
    };
    assert!(is_instrument(&plugin(&["Instrument|Synth"])));
    assert!(is_instrument(&plugin(&["instrument", "stereo"])));
    assert!(!is_instrument(&plugin(&["Fx|Reverb"])));
    assert!(!is_instrument(&plugin(&[])));
    assert_eq!(device_kind(&plugin(&[])), DeviceKind::Vst3 { id: "x".into() });
}
