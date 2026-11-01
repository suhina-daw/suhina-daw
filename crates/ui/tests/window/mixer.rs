// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Mixer controls in both views: track on, solo, cue, arming, faders, pan, sends, returns and inputs.

use super::*;

#[test]
fn return_tracks_have_a_mixer_but_no_slots() {
    let harness = view();
    assert!(harness.query_by_label("Turn A Reverb off").is_some());
    assert!(harness.query_by_label_contains("on A Reverb").is_none());
}

#[test]
fn turning_a_track_off_is_an_edit_that_undoes() {
    let mut harness = window(demo_app());
    let muted = |h: &Harness<'static, App>| h.state().session().map(|s| s.project().channels[1].mute);
    assert_eq!(muted(&harness), Some(false));

    harness.get_by_label("Turn Bass off").click();
    harness.run();
    assert_eq!(muted(&harness), Some(true));
    assert!(harness.query_by_label("Turn Bass on").is_some());

    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run();
    assert_eq!(muted(&harness), Some(false));
}

/// An audio track's strip, grown past the input block: it carries "Audio From" with what it is,
/// and the three monitor picks with the on one lit; the strip's picks run the same
/// `track.setAudioInput` the track menu's do. A notes track's strip has nothing of it.
#[test]
fn an_audio_tracks_strip_shows_audio_from_and_the_monitor_picks() {
    use suhina_engine::model::{ContentKinds, Monitor};
    let mut project = demo_project();
    project.tracks[0].content = ContentKinds { notes: false, audio: true };
    let name = project.tracks[0].name.clone();
    let mut app = App::new();
    app.set_browser_open(false);
    app.set_session(Session::new(project), &[]);
    let mut settings = suhina_ui::settings::Settings::default();
    settings.view.mixer_height = 320;
    app.set_settings(&egui::Context::default(), settings);
    let mut harness = window(app);

    // The rows are there: the source (the test's device has no channels, so eight are offered)
    // and the three monitor buttons.
    assert!(harness.query_by_label_contains(&format!("{name} audio from: No input")).is_some());
    assert!(harness.query_by_label(&format!("{name} monitor In")).is_some());
    assert!(harness.query_by_label(&format!("{name} monitor Auto")).is_some());
    assert!(harness.query_by_label(&format!("{name} monitor Off")).is_some());
    assert!(
        harness.query_by_label_contains("Keys audio from:").is_none(),
        "a track that plays notes has no audio block"
    );

    // The dropdown offers the same list the track menu's does (`audio_input_picks`, and that
    // menu is driven by the track-menu test and the hand check: under kittest's window the
    // strip's popup itself does not open — its picks are the menu's). The monitor's buttons
    // are the strip's own: driven here, with the project as the outcome.
    harness.get_by_label(&format!("{name} monitor In")).click();
    harness.run();
    let heard = |h: &Harness<'static, App>| h.state().session().map(|s| s.project().tracks[0].audio_input);
    assert_eq!(heard(&harness).map(|i| i.monitor), Some(Monitor::In));
    harness.get_by_label(&format!("{name} monitor In")).click();
    harness.run();
    assert_eq!(heard(&harness).map(|i| i.monitor), Some(Monitor::In), "the re-click changes nothing");
    // One undo takes the monitor back; clicking Auto sets it again.
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run();
    assert_eq!(heard(&harness).map(|i| i.monitor), Some(Monitor::Auto));
    harness.get_by_label(&format!("{name} monitor Auto")).click();
    harness.run();
    assert_eq!(heard(&harness).map(|i| i.monitor), Some(Monitor::Auto));
}

#[test]
fn the_plugin_key_says_when_a_track_has_no_running_plug_in() {
    let mut harness = window(demo_app());
    // No device panel selection and no plug-in on the demo's tracks: the key says so.
    harness.get_by_label("Track Bass").click();
    harness.run();
    harness.key_press(Key::P);
    harness.run();
    assert!(
        matches!(harness.state().status(), Status::Hint(text) if text.contains("no running plug-in")),
        "{:?}",
        harness.state().status()
    );
    // The action is on the Shortcuts page under its default key, mappable like any other
    // (the keymap tests cover the default and the conflicts).
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Comma);
    harness.run();
    harness.get_by_label("Settings page: Keys").click();
    harness.run();
    assert!(harness.query_by_label_contains("Open or close the selected track's plug-in editor").is_some());
    harness.key_press(Key::Escape);
    harness.run();
}

/// The demo project with only Drums heard, held notes and no sends, so what is heard is one
/// track's pan and volume.
fn drums_alone() -> suhina_engine::model::Project {
    let mut project = demo_project();
    for channel in &mut project.channels {
        channel.sends.clear();
        channel.mute = channel.id.0 != 1 && channel.id.0 <= 4;
    }
    for content in &mut project.contents {
        for lane in &mut content.lanes {
            if let suhina_engine::model::LaneData::Notes(notes) = &mut lane.data {
                notes.iter_mut().for_each(|n| n.duration = 1.0);
            }
        }
    }
    project
}

/// The level of the left and right side over the next quarter of a second of `renderer`.
fn sides(renderer: &mut suhina_engine::Renderer) -> (f64, f64) {
    let mut out = vec![0.0f32; 2 * 12_000];
    for block in out.chunks_mut(2 * 64) {
        renderer.process(block);
    }
    let rms = |side: usize| {
        let sum: f64 = out.iter().skip(side).step_by(2).map(|s| f64::from(*s).powi(2)).sum();
        (sum / 12_000.0).sqrt()
    };
    (rms(0), rms(1))
}

/// Starts playing `renderer` from the window and moves the pointer onto `label`, pressed.
fn press_while_playing(harness: &mut Harness<'static, App>, renderer: &mut suhina_engine::Renderer, label: &str) {
    harness.get_by_label("Play").click();
    // While playing the window repaints on its own: step it instead of running it to rest.
    harness.run_steps(2);
    let at = harness.get_by_label(label).rect().center();
    harness.hover_at(at);
    harness.run_steps(1);
    harness.drag_at(at);
    harness.run_steps(1);
    sides(renderer);
}

#[test]
fn a_pan_drag_is_heard_while_dragging_and_is_one_undo_step() {
    let project = drums_alone();
    let mut renderer = suhina_engine::Renderer::stopped(&project, &suhina_engine::AudioPool::new(), 48_000).unwrap();
    let mut session = Session::new(project);
    session.attach_player(suhina_session::Player::new(renderer.controller(), None));
    let mut app = App::new();
    app.set_browser_open(false);
    app.set_session(session, &[]);
    // Frames close enough together for a double-click.
    let mut harness = window_with_step(app, 1.0 / 60.0);
    let pan = |h: &Harness<'static, App>| h.state().session().map_or(f64::NAN, |s| s.project().channels[0].pan);

    press_while_playing(&mut harness, &mut renderer, "Drums pan");
    let (left, right) = sides(&mut renderer);
    assert!(left > 0.01 && (left / right - 1.0).abs() < 0.01, "{left} {right}");
    let knob = harness.get_by_label("Drums pan").rect().center();
    // Up raises the pan, 0.01 a point; the button is still held after each step.
    let mut heard = Vec::new();
    for up in [20.0, 40.0, 60.0] {
        harness.hover_at(knob - egui::vec2(0.0, up));
        harness.run_steps(1);
        let (left, right) = sides(&mut renderer);
        heard.push((pan(&harness), right / left));
    }
    for pair in heard.windows(2) {
        assert!(pair[1].0 > pair[0].0 && pair[1].1 > pair[0].1, "louder on the right as it pans: {heard:?}");
    }
    // At a pan of p the sides are cos and sin of (p + 1) * 45 degrees.
    for &(p, ratio) in &heard {
        let want = ((p + 1.0) * std::f64::consts::FRAC_PI_4).tan();
        assert!(p > 0.0 && (ratio - want).abs() < 0.05 * want, "{p}: heard {ratio}, want {want}");
    }
    harness.drop_at(knob - egui::vec2(0.0, 60.0));
    harness.run_steps(2);
    let dropped = pan(&harness);
    assert!((dropped - heard[2].0).abs() < 1e-9, "{dropped} {heard:?}");

    // The whole drag is one step: one undo goes back to the centre, and there is nothing older.
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run_steps(2);
    assert_eq!(pan(&harness), 0.0);
    assert!(harness.state().session().is_some_and(|s| !s.can_undo()));
    let (left, right) = sides(&mut renderer);
    assert!((left / right - 1.0).abs() < 0.01, "{left} {right}");

    // Redo brings the drag's end back; a double-click then centres the knob as a step of its own.
    harness.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z);
    harness.run_steps(2);
    assert_eq!(pan(&harness), dropped);
    // Half a second later, as a person would, so the clicks do not count with the drag.
    harness.run_steps(30);
    for _ in 0..2 {
        harness.hover_at(knob);
        harness.drag_at(knob);
        harness.drop_at(knob);
        harness.step();
    }
    harness.run_steps(2);
    assert_eq!(pan(&harness), 0.0);
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run_steps(2);
    assert_eq!(pan(&harness), dropped);
}

#[test]
fn a_volume_drag_in_the_arrangement_is_heard_while_dragging_and_is_one_undo_step() {
    let project = drums_alone();
    let mut renderer = suhina_engine::Renderer::stopped(&project, &suhina_engine::AudioPool::new(), 48_000).unwrap();
    let mut session = Session::new(project);
    session.attach_player(suhina_session::Player::new(renderer.controller(), None));
    let mut app = App::new();
    app.set_browser_open(false);
    app.set_session(session, &[]);
    let mut harness = window(app);
    harness.get_by_label("Arrangement view").click();
    harness.run();
    let volume = |h: &Harness<'static, App>| h.state().session().map_or(f64::NAN, |s| s.project().channels[0].volume);

    press_while_playing(&mut harness, &mut renderer, "Drums volume");
    let (before, _) = sides(&mut renderer);
    let field = harness.get_by_label("Drums volume").rect().center();
    // Down lowers the volume, 0.25 dB a point: 24 points is about -6 dB, 48 about -12 dB.
    let mut heard = Vec::new();
    for down in [24.0, 48.0] {
        harness.hover_at(field + egui::vec2(0.0, down));
        harness.run_steps(1);
        let (left, _) = sides(&mut renderer);
        heard.push((volume(&harness), left / before));
    }
    for &(gain, ratio) in &heard {
        assert!(gain < 0.6 && (ratio - gain).abs() < 0.02, "heard {ratio} at a gain of {gain}: {heard:?}");
    }
    assert!(heard[1].0 < heard[0].0, "{heard:?}");
    harness.drop_at(field + egui::vec2(0.0, 48.0));
    harness.run_steps(2);

    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run_steps(2);
    assert_eq!(volume(&harness), 1.0);
    assert!(harness.state().session().is_some_and(|s| !s.can_undo()));
}

#[test]
fn solo_is_exclusive_unless_ctrl_is_held() {
    let mut harness = window(demo_app());
    let solos = |h: &Harness<'static, App>| {
        h.state().session().map(|s| s.project().channels.iter().take(4).map(|c| c.solo).collect::<Vec<_>>())
    };
    harness.get_by_label("Solo Drums").click();
    harness.run();
    harness.get_by_label("Solo Keys").click();
    harness.run();
    assert_eq!(solos(&harness), Some(vec![false, false, true, false]));
    harness.get_by_label("Solo Vox").click_modifiers(Modifiers::COMMAND);
    harness.run();
    assert_eq!(solos(&harness), Some(vec![false, false, true, true]));

    harness.get_by_label("Arrangement view").click();
    harness.run();
    harness.get_by_label("End solo on Keys").click();
    harness.run();
    assert_eq!(solos(&harness), Some(vec![false, false, false, true]));
}

#[test]
fn the_cue_button_cues_a_track_in_both_views_and_undoes() {
    let mut harness = window(demo_app());
    let cues = |h: &Harness<'static, App>| {
        h.state().session().map(|s| s.project().channels.iter().take(4).map(|c| c.cue).collect::<Vec<_>>())
    };
    harness.get_by_label("Cue Keys on the headphones").click();
    harness.run();
    harness.get_by_label("Cue Drums on the headphones").click();
    harness.run();
    // Not exclusive: several tracks can be cued at once.
    assert_eq!(cues(&harness), Some(vec![true, false, true, false]));

    harness.get_by_label("Arrangement view").click();
    harness.run();
    harness.get_by_label("Stop cueing Keys").click();
    harness.run();
    assert_eq!(cues(&harness), Some(vec![true, false, false, false]));
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run();
    assert_eq!(cues(&harness), Some(vec![true, false, true, false]));
}

#[test]
fn arming_is_exclusive_unless_ctrl_is_held_and_changes_no_project() {
    let mut harness = window(demo_app());
    let armed = |h: &Harness<'static, App>| {
        h.state().session().map(|s| s.project().tracks.iter().take(4).map(|t| s.is_armed(t.id)).collect::<Vec<_>>())
    };
    harness.get_by_label("Arm Drums for recording").click();
    harness.run();
    harness.get_by_label("Arm Keys for recording").click();
    harness.run();
    assert_eq!(armed(&harness), Some(vec![false, false, true, false]));
    harness.get_by_label("Arm Vox for recording").click_modifiers(Modifiers::COMMAND);
    harness.run();
    assert_eq!(armed(&harness), Some(vec![false, false, true, true]));
    assert!(!harness.state().session().is_some_and(Session::can_undo), "arming is not an edit");

    // The Arrangement headers show it and disarm too.
    harness.get_by_label("Arrangement view").click();
    harness.run();
    harness.get_by_label("Disarm Keys").click();
    harness.run();
    assert_eq!(armed(&harness), Some(vec![false, false, false, true]));
    assert!(harness.query_by_label("Disarm Vox").is_some());
    // Return tracks play no notes: nothing to arm.
    assert!(harness.query_by_label("Arm A Reverb for recording").is_none());
}

#[test]
fn a_tracks_menu_chooses_the_midi_port_and_channel_it_hears() {
    use suhina_engine::model::{MidiInput, MidiPort};
    let mut app = demo_app();
    app.projects_mut().set_midi_system(std::sync::Arc::new(StandMidi(vec!["Keys In".into(), "Pads In".into()])));
    let mut harness = window(app);
    let input = |h: &Harness<'static, App>| {
        h.state()
            .session()
            .and_then(|s| s.project().tracks.iter().find(|t| t.name == "Keys").map(|t| t.midi_input.clone()))
    };
    harness.get_by_label("Track Keys").click_secondary();
    harness.run();
    harness.get_by_label_contains("MIDI input: All ports").click();
    harness.run();
    harness.get_by_label("Pads In").click();
    harness.run();
    assert_eq!(
        input(&harness),
        Some(MidiInput { port: MidiPort::Named("Pads In".into()), channel: None, ..Default::default() })
    );

    harness.get_by_label("Track Keys").click_secondary();
    harness.run();
    harness.get_by_label_contains("MIDI input: Pads In").click();
    harness.run();
    harness.get_by_label("Ch 10").click();
    harness.run();
    assert_eq!(
        input(&harness),
        Some(MidiInput { port: MidiPort::Named("Pads In".into()), channel: Some(10), ..Default::default() })
    );
    assert!(harness.state().session().is_some_and(Session::can_undo), "an edit, saved with the project");
}

#[test]
fn an_audio_track_is_armed_to_hear_its_input_and_its_menu_chooses_the_channels() {
    use suhina_engine::model::{AudioInput, ContentKinds, InputChannels, Monitor};
    let mut project = demo_project();
    project.tracks[0].content = ContentKinds { notes: false, audio: true };
    let name = project.tracks[0].name.clone();
    let mut app = App::new();
    app.set_session(Session::new(project), &[]);
    let mut harness = window(app);
    harness.get_by_label(&format!("Arm {name} for recording")).click();
    harness.run();
    assert!(harness.state().session().is_some_and(|s| s.is_armed(suhina_engine::model::TrackId(1))));

    let input = |h: &Harness<'static, App>| h.state().session().map(|s| s.project().tracks[0].audio_input);
    harness.get_by_label(&format!("Track {name}")).click_secondary();
    harness.run();
    harness.get_by_label_contains("Audio input: No input").click();
    harness.run();
    assert!(harness.query_by_label_contains("No input device").is_some(), "says why only eight are offered");
    harness.get_by_label("In 3/4").click();
    harness.run();
    assert_eq!(input(&harness), Some(AudioInput { channels: InputChannels::Stereo(3), monitor: Monitor::Auto }));
    harness.get_by_label(&format!("Track {name}")).click_secondary();
    harness.run();
    harness.get_by_label_contains("Audio input: In 3/4").click();
    harness.run();
    harness.get_by_label("Monitor in").click();
    harness.run();
    assert_eq!(input(&harness), Some(AudioInput { channels: InputChannels::Stereo(3), monitor: Monitor::In }));
    // A track that plays notes has a MIDI input instead.
    harness.get_by_label("Track Keys").click_secondary();
    harness.run();
    assert!(harness.query_by_label_contains("MIDI input:").is_some());
    assert!(harness.query_by_label_contains("Audio input:").is_none());
}

/// A linear gain in dB, as the engine keeps it.
pub(super) fn gain_of(db: f64) -> f64 {
    10f64.powf(db / 20.0)
}

#[test]
fn dragging_one_fader_of_a_selection_moves_every_track_in_proportion() {
    let mut harness = arrangement_app();
    harness.state_mut().run(&Action::new("track.setVolume", json!({ "tracks": [1], "db": -6.0 })));
    harness.run();
    harness.get_by_label("Track Drums").click();
    harness.run();
    harness.get_by_label("Track Bass").click_modifiers(Modifiers::COMMAND);
    harness.run();
    let volumes = |h: &Harness<'static, App>| -> Vec<f64> {
        h.state().session().map(|s| s.project().channels.iter().map(|c| c.volume).collect()).unwrap_or_default()
    };
    let before = volumes(&harness);

    // 20 points down the field is 5 dB quieter: for the track whose field was dragged, and for the
    // other track of the selection, so what was between them stays.
    let at = harness.get_by_label("Drums volume").rect().center();
    harness.hover_at(at);
    harness.run();
    harness.drag_at(at);
    harness.step();
    harness.hover_at(at + egui::vec2(0.0, 20.0));
    harness.step();
    harness.drop_at(at + egui::vec2(0.0, 20.0));
    harness.run();
    let after = volumes(&harness);
    let quieter = gain_of(-5.0);
    assert!(before.iter().all(|v| *v > 0.0), "{before:?}");
    for (track, (was, now)) in ["Drums", "Bass"].iter().zip(before.iter().zip(&after)) {
        assert!((now / was - quieter).abs() < 1e-3, "{track}: {was} -> {now}");
    }
}

#[test]
fn a_selections_track_on_and_solo_buttons_take_the_whole_selection() {
    let mut harness = arrangement_app();
    harness.get_by_label("Track Drums").click();
    harness.run();
    harness.get_by_label("Track Bass").click_modifiers(Modifiers::COMMAND);
    harness.run();
    harness.get_by_label("Turn Drums off").click();
    harness.run();
    let muted: Vec<bool> =
        harness.state().session().map(|s| s.project().channels.iter().map(|c| c.mute).collect()).unwrap_or_default();
    assert!(muted[0] && muted[1], "both selected tracks went quiet: {muted:?}");

    harness.get_by_label("Solo Drums").click();
    harness.run();
    let soloed: Vec<bool> =
        harness.state().session().map(|s| s.project().channels.iter().map(|c| c.solo).collect()).unwrap_or_default();
    assert!(soloed[0] && soloed[1], "and both soloed together: {soloed:?}");
}

#[test]
fn a_return_in_the_arrangement_has_a_tracks_controls_and_main_a_volume_bar_and_they_grow_upward() {
    let mut harness = arrangement_app();
    harness.get_by_label("Expand A Reverb").click();
    harness.run();
    // A return's header is a track's: volume and pan, its on switch, its automation lanes.
    for label in ["A Reverb volume", "A Reverb pan", "Turn A Reverb off"] {
        assert!(harness.query_by_label(label).is_some(), "missing {label}");
    }
    assert!(harness.query_by_label("Show automation of A Reverb").is_some(), "its automation lanes button");
    // A track's header has a send knob to the return, as its strip has.
    assert!(harness.query_by_label("Drums send to A Reverb").is_some(), "a track's send knob");
    assert!(harness.query_by_label("A Reverb send to A Reverb").is_none(), "no send on a return");
    // Main: a bar a drag sets, one undo step.
    let main = |h: &Harness<'static, App>| {
        let p = h.state().session().unwrap().project();
        p.channels.iter().find(|c| c.role == suhina_engine::model::ChannelRole::Master).map(|c| c.volume).unwrap()
    };
    let bar = harness.get_by_label("Main volume").rect().center();
    harness.hover_at(bar);
    harness.step();
    harness.drag_at(bar);
    harness.step();
    for y in [10.0, 20.0, 40.0] {
        harness.hover_at(bar + egui::vec2(0.0, y));
        harness.step();
    }
    harness.drop_at(bar + egui::vec2(0.0, 40.0));
    harness.run_steps(3);
    assert!(main(&harness) < 1.0, "dragged down, quieter: {}", main(&harness));
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run_steps(3);
    assert!((main(&harness) - 1.0).abs() < 1e-9, "one undo step");

    // The fixed rows grow upward: Main's top edge dragged up 30 points takes its bar and the
    // return above it up as far.
    let (bar_top, reverb_top) =
        (harness.get_by_label("Main volume").rect().top(), harness.get_by_label("A Reverb volume").rect().top());
    let edge = egui::pos2(harness.get_by_label("Main volume").rect().center().x, bar_top - 1.0);
    harness.hover_at(edge);
    harness.step();
    harness.drag_at(edge);
    harness.step();
    for y in [-10.0, -20.0, -30.0] {
        harness.hover_at(edge + egui::vec2(0.0, y));
        harness.step();
    }
    harness.drop_at(edge + egui::vec2(0.0, -30.0));
    harness.run_steps(3);
    assert_eq!(harness.get_by_label("Main volume").rect().top(), bar_top - 30.0);
    assert_eq!(harness.get_by_label("A Reverb volume").rect().top(), reverb_top - 30.0);
}

#[test]
fn hide_returns_takes_the_return_rows_out_of_the_arrangement_and_show_brings_them_back() {
    let mut harness = arrangement_app();
    assert!(harness.query_by_label("A Reverb volume").is_some());
    harness.get_by_label("Hide returns").click();
    harness.run_steps(3);
    assert!(harness.query_by_label("A Reverb volume").is_none(), "the return's row is gone");
    assert!(harness.query_by_label("Main volume").is_some(), "Main stays");
    assert!(harness.state().settings().view.returns_hidden, "kept as a setting");
    harness.get_by_label("Show returns").click();
    harness.run_steps(3);
    assert!(harness.query_by_label("A Reverb volume").is_some(), "back again");
}

/// Bass's send to the A Reverb return in the project: level and before the fader.
fn bass_send(harness: &Harness<'static, App>) -> Option<(f64, bool)> {
    let project = harness.state().session()?.project();
    let bass = project.tracks.iter().find(|t| t.name == "Bass")?.channel?;
    let reverb = project.tracks.iter().find(|t| t.name == "A Reverb")?.channel?;
    let send = project.channel(bass)?.sends.iter().find(|s| s.target == reverb)?;
    Some((send.level, send.pre_fader))
}

#[test]
fn a_send_knob_sets_the_send_level_and_its_menu_the_mode() {
    let mut harness = window(demo_app());
    let before = bass_send(&harness).map(|(level, _)| 20.0 * level.log10()).unwrap_or_default();
    let knob = harness.get_by_label("Bass send to A Reverb").rect().center();
    harness.hover_at(knob);
    harness.run();
    // Stepped: the send is heard while it is dragged, an edit every frame (#705).
    harness.drag_at(knob);
    harness.step();
    harness.hover_at(knob - egui::vec2(0.0, 10.0));
    harness.step();
    let during = bass_send(&harness).map(|(level, _)| 20.0 * level.log10()).unwrap_or_default();
    assert!(during > before, "louder while dragged: {before} to {during}");
    harness.hover_at(knob - egui::vec2(0.0, 20.0));
    harness.step();
    harness.drop_at(knob - egui::vec2(0.0, 20.0));
    harness.step();
    harness.step();
    // 20 points up at 0.25 dB a point: 5 dB louder, rounded to 0.1 dB.
    let after = bass_send(&harness).map(|(level, _)| 20.0 * level.log10()).unwrap_or_default();
    assert!((after - before - 5.0).abs() < 0.11, "{before} to {after}");

    harness.get_by_label("Bass send to A Reverb").click_secondary();
    harness.run();
    harness.get_by_label("Before the fader (pre)").click();
    harness.run();
    assert_eq!(bass_send(&harness).map(|(_, pre)| pre), Some(true));

    // Two undo steps: the mode, then the level.
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run();
    let undone = bass_send(&harness).map(|(level, pre)| (20.0 * level.log10(), pre));
    assert!(undone.is_some_and(|(db, pre)| (db - before).abs() < 1e-9 && !pre), "{undone:?}");
}
