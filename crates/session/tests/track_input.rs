// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The default track input (#1191, ADR 0017): `audio.setTrackInput`, kept with the input for each
//! audio device, new audio tracks that take the chosen device's, and armed tracks that hear
//! nothing getting it.
#![allow(clippy::unwrap_used)] // test helpers outside #[test] functions

use std::collections::BTreeMap;

use serde_json::{Value, json};
use suhina_model::InputChannels;
use suhina_session::{DeviceInputs, InputChoice, OutputChoice, Projects, Session};

use crate::support::run;

/// Projects with the blank project open and, when `input` is on, an input device chosen. No
/// audio system: nothing opens, the choice is only kept. The project has been changed (its
/// tempo), so the tracks that are there keep their inputs as an opened project's do.
fn projects(input: bool) -> Projects {
    let mut projects = Projects::new();
    if input {
        projects.set_output_choice(OutputChoice { input: InputChoice::Default, ..OutputChoice::default() });
    }
    projects.add(Session::new(suhina_session::blank_project()), None).unwrap();
    run(&mut projects, "transport.setTempo", json!({ "bpm": 121 })).unwrap();
    projects
}

/// The audio input each track hears, by track id, in track order.
fn inputs(projects: &mut Projects) -> Vec<(u64, String)> {
    let tracks = run(projects, "project.tracks", json!({})).unwrap();
    tracks
        .as_array()
        .unwrap()
        .iter()
        .map(|t| (t["id"].as_u64().unwrap(), t["audio_input"]["input"].as_str().unwrap().to_string()))
        .collect()
}

fn input_of(projects: &mut Projects, track: &Value) -> String {
    let track = track.as_u64().unwrap();
    inputs(projects).into_iter().find(|(id, _)| *id == track).unwrap().1
}

#[test]
fn new_audio_tracks_take_the_default_track_input() {
    let mut projects = projects(true);
    let before = inputs(&mut projects);
    let set = run(&mut projects, "audio.setTrackInput", json!({ "input": "1/2" })).unwrap();
    assert_eq!(set, json!({ "track_input": "1/2", "input_device": "default|default" }));
    assert_eq!(projects.track_input(), InputChannels::Stereo(1));
    assert_eq!(inputs(&mut projects), before, "the tracks that are there keep theirs");

    let audio = run(&mut projects, "track.add", json!({ "kind": "audio" })).unwrap();
    assert_eq!(input_of(&mut projects, &audio["track"]), "1/2");
    let midi = run(&mut projects, "track.add", json!({ "kind": "midi" })).unwrap();
    assert_eq!(input_of(&mut projects, &midi["track"]), "none", "a MIDI track hears no audio input");
    let many = run(&mut projects, "track.add", json!({ "kind": "audio", "count": 3 })).unwrap();
    for track in many["tracks"].as_array().unwrap() {
        assert_eq!(input_of(&mut projects, track), "1/2");
    }

    // A new default is for the tracks added from now on.
    run(&mut projects, "audio.setTrackInput", json!({ "input": "3" })).unwrap();
    let mono = run(&mut projects, "track.add", json!({ "kind": "audio" })).unwrap();
    assert_eq!(input_of(&mut projects, &mono["track"]), "3");
    assert_eq!(input_of(&mut projects, &audio["track"]), "1/2");

    // Adding the track is one undo step, its input with it.
    run(&mut projects, "edit.undo", json!({})).unwrap();
    assert!(inputs(&mut projects).iter().all(|(id, _)| Some(*id) != mono["track"].as_u64()));

    // A project opened later takes it too.
    projects.add(Session::new(suhina_session::blank_project()), None).unwrap();
    let later = run(&mut projects, "track.add", json!({ "kind": "audio" })).unwrap();
    assert_eq!(input_of(&mut projects, &later["track"]), "3");

    // None goes back to tracks that hear nothing until given an input.
    run(&mut projects, "audio.setTrackInput", json!({ "input": "none" })).unwrap();
    let plain = run(&mut projects, "track.add", json!({ "kind": "audio" })).unwrap();
    assert_eq!(input_of(&mut projects, &plain["track"]), "none");
}

/// The input device chosen on the audio device there is.
fn input_device(projects: &mut Projects, input: InputChoice) {
    let choice = OutputChoice { input, ..projects.output_choice().clone() };
    projects.set_output_choice(choice);
}

/// An ASIO audio device chosen by name, with `input`.
fn audio_device(projects: &mut Projects, device: &str, input: InputChoice) {
    projects.set_output_choice(OutputChoice {
        host: Some("ASIO".into()),
        device: Some(device.into()),
        input,
        ..OutputChoice::default()
    });
}

#[test]
fn each_audio_device_keeps_its_input_and_default_track_input() {
    let mut projects = projects(false);
    audio_device(&mut projects, "Universal Audio Thunderbolt", InputChoice::Default);
    let set = run(&mut projects, "audio.setTrackInput", json!({ "input": "1" })).unwrap();
    assert_eq!(set["input_device"], json!("asio|universal audio thunderbolt"));

    // Another sound card has none until one is chosen for it.
    audio_device(&mut projects, "SSL 2+", InputChoice::Named("SSL 2+".into()));
    assert_eq!(projects.track_input(), InputChannels::None);
    let plain = run(&mut projects, "track.add", json!({ "kind": "audio" })).unwrap();
    assert_eq!(input_of(&mut projects, &plain["track"]), "none");
    run(&mut projects, "audio.setTrackInput", json!({ "input": "1/2" })).unwrap();
    let pair = run(&mut projects, "track.add", json!({ "kind": "audio" })).unwrap();
    assert_eq!(input_of(&mut projects, &pair["track"]), "1/2");

    // Changing the input on one card does not lose the card's default track input.
    input_device(&mut projects, InputChoice::Default);
    assert_eq!(projects.track_input(), InputChannels::Stereo(1));

    // Back on the first card: its own comes back; the name is matched in any case.
    audio_device(&mut projects, "UNIVERSAL AUDIO THUNDERBOLT", InputChoice::Default);
    assert_eq!(projects.track_input(), InputChannels::Mono(1));
    let one = run(&mut projects, "track.add", json!({ "kind": "audio" })).unwrap();
    assert_eq!(input_of(&mut projects, &one["track"]), "1");
    assert_eq!(input_of(&mut projects, &pair["track"]), "1/2", "the tracks that are there keep theirs");
    let info = run(&mut projects, "audio.info", json!({})).unwrap();
    assert_eq!(info["chosen"]["track_input"], json!("1"));

    // Kept for the settings, and taken back from them at start.
    let kept = projects.device_inputs().clone();
    assert_eq!(
        kept,
        BTreeMap::from([
            (
                "asio|ssl 2+".to_string(),
                DeviceInputs { input: InputChoice::Default, track_input: InputChannels::Stereo(1) }
            ),
            (
                "asio|universal audio thunderbolt".to_string(),
                DeviceInputs { input: InputChoice::Default, track_input: InputChannels::Mono(1) }
            ),
        ])
    );
    let mut started = self::projects(false);
    started.set_device_inputs(kept);
    assert_eq!(started.track_input(), InputChannels::None, "no device with Input on yet");
    audio_device(&mut started, "SSL 2+", InputChoice::Default);
    assert_eq!(started.track_input(), InputChannels::Stereo(1));
    let restored = run(&mut started, "track.add", json!({ "kind": "audio" })).unwrap();
    assert_eq!(input_of(&mut started, &restored["track"]), "1/2");
}

#[test]
fn choosing_another_audio_device_brings_back_its_input() {
    let mut projects = projects(false);
    let set = |projects: &mut Projects, params: Value| run(projects, "audio.setOutput", params).unwrap();
    set(&mut projects, json!({ "host": "ASIO", "device": "Interface", "input": "Interface In" }));
    run(&mut projects, "audio.setTrackInput", json!({ "input": "6" })).unwrap();
    set(&mut projects, json!({ "device": "SSL 2+", "input": "SSL 2+ In" }));
    run(&mut projects, "audio.setTrackInput", json!({ "input": "1/2" })).unwrap();

    // Only the device given: its input and default track input come back.
    let back = set(&mut projects, json!({ "device": "Interface" }));
    assert_eq!(back["chosen"]["input"], json!("Interface In"));
    assert_eq!(back["chosen"]["track_input"], json!("6"));
    let six = run(&mut projects, "track.add", json!({ "kind": "audio" })).unwrap();
    assert_eq!(input_of(&mut projects, &six["track"]), "6");

    // An input given with the device wins, and is what the device keeps from then on.
    set(&mut projects, json!({ "device": "SSL 2+", "input": "off" }));
    assert_eq!(projects.output_choice().input, InputChoice::Off);
    assert_eq!(projects.track_input(), InputChannels::None);
    set(&mut projects, json!({ "device": "Interface" }));
    let again = set(&mut projects, json!({ "device": "SSL 2+" }));
    assert_eq!(again["chosen"]["input"], json!("off"));

    // A device never chosen keeps the input there is.
    let new = set(&mut projects, json!({ "device": "USB Mic" }));
    assert_eq!(new["chosen"]["input"], json!("off"));
    assert_eq!(projects.remembered_input(&OutputChoice::default()), None);
}

#[test]
fn the_default_track_input_does_nothing_while_the_input_device_is_off() {
    let mut projects = projects(true);
    run(&mut projects, "audio.setTrackInput", json!({ "input": "1/2" })).unwrap();
    input_device(&mut projects, InputChoice::Off);
    let audio = run(&mut projects, "track.add", json!({ "kind": "audio" })).unwrap();
    assert_eq!(input_of(&mut projects, &audio["track"]), "none", "no microphone is chosen unasked");
    assert_eq!(projects.track_input(), InputChannels::None);
    // With no input to hear it on, a choice is refused and changes nothing.
    assert!(run(&mut projects, "audio.setTrackInput", json!({ "input": "3" })).is_err());

    // Turning Input on again puts the device's own back in effect.
    input_device(&mut projects, InputChoice::Default);
    let next = run(&mut projects, "track.add", json!({ "kind": "audio" })).unwrap();
    assert_eq!(input_of(&mut projects, &next["track"]), "1/2");
    assert_eq!(input_of(&mut projects, &audio["track"]), "none");
}

#[test]
fn arming_a_track_that_hears_nothing_gives_it_the_default_track_input() {
    let mut projects = projects(true);
    run(&mut projects, "audio.setTrackInput", json!({ "input": "2" })).unwrap();
    let tracks = inputs(&mut projects);
    // The blank project's third and fourth tracks are audio tracks that hear nothing.
    let (audio, _) = tracks[2].clone();
    assert_eq!(tracks[2].1, "none");
    let armed = run(&mut projects, "track.setArm", json!({ "track": audio, "armed": true })).unwrap();
    assert_eq!(armed["input_given"], json!("2"));
    assert_eq!(armed["armed"], json!(true));
    assert_eq!(input_of(&mut projects, &json!(audio)), "2");

    // Armed again with an input of its own, it keeps it.
    run(&mut projects, "track.setArm", json!({ "track": audio, "armed": false })).unwrap();
    run(&mut projects, "track.setAudioInput", json!({ "track": audio, "input": "5/6" })).unwrap();
    let again = run(&mut projects, "track.setArm", json!({ "track": audio, "armed": true })).unwrap();
    assert!(again.get("input_given").is_none(), "{again}");
    assert_eq!(input_of(&mut projects, &json!(audio)), "5/6");

    // A MIDI track records notes: it is given no audio input.
    let (midi, _) = tracks[0].clone();
    let notes = run(&mut projects, "track.setArm", json!({ "track": midi, "armed": true })).unwrap();
    assert!(notes.get("input_given").is_none() && notes.get("no_input").is_none(), "{notes}");
    assert_eq!(input_of(&mut projects, &json!(midi)), "none");

    // A return track records nothing: it is given nothing either.
    let (ret, _) = tracks[4].clone();
    let returned = run(&mut projects, "track.setArm", json!({ "track": ret, "armed": true })).unwrap();
    assert!(returned.get("input_given").is_none(), "{returned}");
    assert_eq!(input_of(&mut projects, &json!(ret)), "none");

    // The input given is an edit of its own that undo takes back; the track stays armed.
    run(&mut projects, "track.setArm", json!({ "track": tracks[3].0, "armed": true })).unwrap();
    assert_eq!(input_of(&mut projects, &json!(tracks[3].0)), "2");
    run(&mut projects, "edit.undo", json!({})).unwrap();
    assert_eq!(input_of(&mut projects, &json!(tracks[3].0)), "none");
}

#[test]
fn arming_with_no_input_to_give_says_so() {
    // No input device is open and no default can be in effect: the track is armed, hearing
    // nothing, and the result says there was no input to give.
    let mut projects = projects(true);
    run(&mut projects, "audio.setTrackInput", json!({ "input": "1/2" })).unwrap();
    input_device(&mut projects, InputChoice::Off);
    let audio = inputs(&mut projects)[2].0;
    let armed = run(&mut projects, "track.setArm", json!({ "track": audio, "armed": true })).unwrap();
    assert_eq!((armed["armed"].clone(), armed["no_input"].clone()), (json!(true), json!(true)));
    assert_eq!(input_of(&mut projects, &json!(audio)), "none");
    // Disarming says nothing about inputs.
    let disarmed = run(&mut projects, "track.setArm", json!({ "track": audio, "armed": false })).unwrap();
    assert!(disarmed.get("no_input").is_none(), "{disarmed}");
}

#[test]
fn the_default_track_input_refuses_nonsense_and_shows_with_the_output() {
    let mut projects = projects(true);
    for bad in [json!({ "input": "0" }), json!({ "input": "1/3" }), json!({ "input": "left" }), json!({ "input": 2 })] {
        assert!(run(&mut projects, "audio.setTrackInput", bad.clone()).is_err(), "{bad}");
    }
    assert!(run(&mut projects, "audio.setTrackInput", json!({})).is_err(), "input is needed");
    assert_eq!(projects.track_input(), InputChannels::None, "a refused choice changes nothing");
    run(&mut projects, "audio.setTrackInput", json!({ "input": "7/8" })).unwrap();
    let info = run(&mut projects, "audio.info", json!({})).unwrap();
    assert_eq!(info["chosen"]["track_input"], json!("7/8"));
}

#[test]
fn an_arm_that_is_refused_gives_no_input() {
    let mut projects = projects(true);
    run(&mut projects, "audio.setTrackInput", json!({ "input": "1/2" })).unwrap();
    let audio = inputs(&mut projects)[2].0;
    let refused = run(&mut projects, "track.setArm", json!({ "track": audio, "armed": true, "exclusive": "yes" }));
    assert!(refused.is_err());
    assert_eq!(input_of(&mut projects, &json!(audio)), "none", "nothing changed");
    let tracks = run(&mut projects, "project.tracks", json!({})).unwrap();
    assert_eq!(tracks[2]["armed"], json!(false));
}

#[test]
fn channels_no_track_could_hear_are_not_a_default() {
    let mut projects = projects(true);
    for input in
        [InputChannels::Mono(0), InputChannels::Stereo(0), InputChannels::Stereo(suhina_model::MAX_INPUT_CHANNEL)]
    {
        assert!(projects.set_track_input(input).is_err(), "{input:?}");
    }
    assert_eq!(projects.track_input(), InputChannels::None);
    // Read back from settings, they are dropped and the card's input is kept.
    let key = projects.output_choice().device_key();
    projects.set_device_inputs(BTreeMap::from([(
        key,
        DeviceInputs { input: InputChoice::Default, track_input: InputChannels::Mono(0) },
    )]));
    assert_eq!(projects.track_input(), InputChannels::None);
    let audio = run(&mut projects, "track.add", json!({ "kind": "audio" })).unwrap();
    assert_eq!(input_of(&mut projects, &audio["track"]), "none");
}

#[test]
fn a_new_project_nothing_has_changed_follows_the_default_track_input() {
    let mut projects = Projects::new();
    projects.set_output_choice(OutputChoice { input: InputChoice::Default, ..OutputChoice::default() });
    run(&mut projects, "audio.setTrackInput", json!({ "input": "1/2" })).unwrap();
    projects.add(Session::new(suhina_session::blank_project()), None).unwrap();
    // The blank project's audio tracks hear the default from the start; MIDI tracks and returns
    // do not, and it is not an edit: nothing to undo, nothing to save.
    let tracks = inputs(&mut projects);
    let heard: Vec<&str> = tracks.iter().map(|(_, input)| input.as_str()).collect();
    assert_eq!(heard, ["none", "none", "1/2", "1/2", "none", "none"]);
    let session = projects.get(projects.active().unwrap()).unwrap();
    assert!(!session.can_undo() && !session.has_unsaved_changes());

    // While it stays unchanged, the tracks follow another sound card's default.
    projects.set_output_choice(OutputChoice {
        host: Some("ASIO".into()),
        device: Some("SSL 2+".into()),
        input: InputChoice::Default,
        ..OutputChoice::default()
    });
    assert_eq!(input_of(&mut projects, &json!(tracks[2].0)), "none", "nothing chosen for this card");
    run(&mut projects, "audio.setTrackInput", json!({ "input": "2" })).unwrap();
    assert_eq!(input_of(&mut projects, &json!(tracks[3].0)), "2");

    // A track given an input by hand is the project's first change: from then on nothing follows.
    run(&mut projects, "track.setAudioInput", json!({ "track": tracks[2].0, "input": "5" })).unwrap();
    run(&mut projects, "audio.setTrackInput", json!({ "input": "1" })).unwrap();
    assert_eq!(input_of(&mut projects, &json!(tracks[2].0)), "5");
    assert_eq!(input_of(&mut projects, &json!(tracks[3].0)), "2");
}
