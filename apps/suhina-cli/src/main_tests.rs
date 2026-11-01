// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

/// The device parameters a `.suhina` file stores reach the CLI's own render path: the same
/// `Session::open` and `render_wavs` the `render` command runs. A band the file boosts makes
/// the render louder, and Scale at 0 % puts it back. (The device's and the file format's own
/// tests cover each half; this checks the way between them.)
#[test]
fn a_project_files_device_params_reach_the_render() {
    let sine_project = |devices: Vec<Device>| {
        let notes = vec![Note::new(0.0, 2.0, 83, 1.0)];
        Project {
            tracks: vec![Track {
                content: ContentKinds { notes: true, audio: false },
                channel: Some(ChannelId(1)),
                ..Track::new(TrackId(1), "T")
            }],
            channels: vec![Channel { devices, ..Channel::new(ChannelId(1), "C") }],
            contents: vec![ClipContent {
                id: ContentId(1),
                name: "c".into(),
                length: 2.0,
                lanes: vec![ContentLane { target: None, data: LaneData::Notes(notes) }],
                extensions: Extensions::default(),
                scale: None,
            }],
            arrangement: Arrangement {
                lanes: vec![ArrangementLane {
                    id: Default::default(),
                    name: "l".into(),
                    track: Some(TrackId(1)),
                    clips: vec![Clip::new(ContentId(1), 0.0, 2.0)],
                }],
                markers: Vec::new(),
                takes: Vec::new(),
            },
            ..Project::default()
        }
    };
    let dir = std::env::temp_dir().join(format!("suhina-cli-params-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let write = |name: &str, devices: Vec<Device>| {
        let path = dir.join(name);
        suhina_project_file::write_file(&path, &sine_project(devices)).unwrap();
        path
    };
    let eq = |params: &[(&str, f64)]| Device {
        params: params.iter().map(|(p, v)| DeviceParam::new(*p, *v)).collect(),
        ..Device::new("EQ", DeviceKind::Builtin("eq".into()))
    };
    let flat = write("flat.suhina", vec![eq(&[])]);
    let boosted = write("boosted.suhina", vec![eq(&[("a3Gain", 12.0)])]);
    let scaled = write("scaled.suhina", vec![eq(&[("a3Gain", 12.0), ("scale", 0.0)])]);

    let peak_of = |path: &std::path::Path| {
        let (session, warnings) = suhina_session::Session::open(path).unwrap();
        assert!(warnings.is_empty(), "{warnings:?}");
        let mut library = suhina_plugins::library::Library::default();
        let env = suhina_session::PluginEnv::from_library(&mut library);
        let out = dir.join("out.wav");
        render_wavs(session.project(), &AudioPool::new(), session.device_assets(), &out, None, 48_000, Some(&env))
            .unwrap()
            .peak
    };
    let flat_peak = peak_of(&flat);
    let boosted_peak = peak_of(&boosted);
    let scaled_peak = peak_of(&scaled);
    let _ = std::fs::remove_dir_all(&dir);
    assert!(flat_peak > 0.05, "the note plays: {flat_peak}");
    let lift_db = 20.0 * f64::from(boosted_peak / flat_peak).log10();
    assert!((lift_db - 12.0).abs() < 0.5, "the stored band lifts its note: {lift_db} dB");
    assert!(
        20.0 * f64::from(scaled_peak / flat_peak).log10().abs() < 0.1,
        "Scale at 0 % puts it back: {}",
        20.0 * f64::from(scaled_peak / flat_peak).log10()
    );
}

#[test]
fn demo_project_is_valid_and_renders_two_bars() {
    let project = demo_project();
    assert_eq!(project.validate(), Ok(()));
    let samples = suhina_engine::render_offline(&project, 48_000, 512).unwrap();
    let seconds = samples.len() as f64 / 2.0 / 48_000.0;
    assert!((4.0..4.2).contains(&seconds), "{seconds} s");
    let peak = samples.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    assert!(peak > 0.1 && peak <= 1.0, "peak {peak}");
}

#[test]
fn render_args_need_one_project_and_an_output() {
    let args = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    let parse_render = |a: &[String]| parse_args(a, Command::Render);
    assert!(parse_render(&args(&["-o", "x.wav"])).is_err());
    assert!(parse_render(&args(&["--demo"])).is_err());
    assert!(parse_render(&args(&["--demo", "-o"])).is_err());
    assert!(parse_render(&args(&["--demo", "a.als", "-o", "x.wav"])).is_err());
    assert!(parse_render(&args(&["--demo", "-o", "x.wav", "--sample-rate", "abc"])).is_err());
    assert!(parse_render(&args(&["--demo", "-o", "x.wav", "--scene", "0"])).is_err());
    assert!(parse_render(&args(&["--demo", "-o", "x.wav", "--bogus"])).is_err());
    let ok = parse_render(&args(&["a.als", "-o", "x.wav", "--sample-rate", "44100", "--scene", "2"])).unwrap();
    assert_eq!(ok.sample_rate, 44_100);
    assert_eq!(ok.scene, Some(1));
    assert!(matches!(ok.source, Source::Als(_)));
    assert_eq!(ok.cue, None);
    let cued = parse_render(&args(&["--demo", "-o", "x.wav", "--cue", "cue.wav"])).unwrap();
    assert_eq!(cued.cue, Some(PathBuf::from("cue.wav")));
    assert!(parse_render(&args(&["--demo", "-o", "x.wav", "--cue"])).is_err());
    assert!(parse_args(&args(&["--demo", "--cue", "cue.wav"]), Command::Play).is_err());
    // --source: a name, render only, and no cue beside it.
    assert!(parse_render(&args(&["--demo", "-o", "x.wav", "--source"])).is_err());
    assert!(parse_args(&args(&["--demo", "--source", "Bass"]), Command::Play).is_err());
    assert!(parse_render(&args(&["--demo", "-o", "x.wav", "--cue", "cue.wav", "--source", "Bass"])).is_err());
    let ok = parse_render(&args(&["--demo", "-o", "x.wav", "--source", "Chords"])).unwrap();
    assert_eq!(ok.node.as_deref(), Some("Chords"));
    assert!(parse_render(&args(&["--demo", "-o", "x.wav"])).unwrap().node.is_none());
}

/// `render --source <track>` renders the sum at that node alone: the same export the
/// command runs, and not the whole mix.
#[test]
fn a_source_render_writes_the_nodes_sum_alone() {
    let peak_of = |path: &std::path::Path| {
        hound::WavReader::open(path).unwrap().samples::<f32>().map(Result::unwrap).fold(0.0f32, |m, s| m.max(s.abs()))
    };
    let dir = std::env::temp_dir().join(format!("suhina-cli-source-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let whole = dir.join("whole.wav");
    let chords = dir.join("chords.wav");
    let args = |output: &std::path::Path, node: Option<&str>| Args {
        source: Source::Demo,
        output: Some(output.to_path_buf()),
        cue: None,
        node: node.map(str::to_string),
        scene: None,
        launcher: false,
        sample_rate: 48_000,
        fm: None,
        audio_output: suhina_session::OutputChoice::default(),
    };
    render(&args(&whole, None)).unwrap();
    render(&args(&chords, Some("Chords"))).unwrap();
    let chords_peak = peak_of(&chords);
    assert!(chords_peak > 0.1, "the node sounds alone: {chords_peak}");
    assert!(peak_of(&whole) > chords_peak, "the whole mix sums its tracks over one of them");
    assert!(
        !hound::WavReader::open(&chords).unwrap().samples::<f32>().map(Result::unwrap).eq(hound::WavReader::open(
            &whole
        )
        .unwrap()
        .samples::<f32>()
        .map(Result::unwrap)),
        "the node render is not the whole mix"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn play_args_take_a_project_but_no_output() {
    let args = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    assert!(parse_args(&args(&["--demo"]), Command::Play).is_ok());
    assert!(parse_args(&args(&["a.als", "--scene", "3"]), Command::Play).is_ok());
    assert!(parse_args(&args(&["--demo", "-o", "x.wav"]), Command::Play).is_err());
    assert!(parse_args(&args(&[]), Command::Play).is_err());
    assert!(parse_args(&args(&["a.als", "--launcher"]), Command::Play).unwrap().launcher);
    assert!(parse_args(&args(&["a.als", "--launcher", "--scene", "1"]), Command::Play).is_err());
    assert!(parse_args(&args(&["a.als", "--launcher", "-o", "x.wav"]), Command::Render).is_err());
}

#[test]
fn scene_replaces_the_arrangement_with_its_clips() {
    let mut project = demo_project();
    let clip = project.arrangement.lanes[0].clips[0].clone();
    project.launcher = Launcher {
        mappings: Vec::new(),
        scenes: vec![Scene::default(), Scene::default()],
        columns: vec![LauncherColumn {
            track: TrackId(1),
            slots: vec![Slot::default(), Slot { clip: Some(clip), has_stop: true }],
        }],
    };
    let scene_2 = scene_as_arrangement(&project, 1).unwrap();
    assert_eq!(scene_2.arrangement.lanes.len(), 1);
    assert_eq!(scene_2.arrangement.lanes[0].clips.len(), 1);
    assert!(scene_as_arrangement(&project, 0).unwrap().arrangement.lanes[0].clips.is_empty());
    assert!(scene_as_arrangement(&project, 2).is_err());
}

#[test]
fn fm_plays_every_note_track_with_a_voice_from_a_bank() {
    use suhina_device_fm_synth::{FmSynth, Patch, patch};
    // A bank whose second voice is a two-operator sound, written to a file.
    let mut synth = FmSynth::new();
    let index = |id: &str| suhina_device_fm_synth::INFO.param_index(id).unwrap();
    use suhina_device_api::Device as _;
    synth.set_param(index("op2.level"), 80.0);
    synth.set_param(index("op2.coarse"), 3.0);
    let mut voice = synth.patch();
    voice.set_name("TWO OPS");
    let path = std::env::temp_dir().join(format!("suhina-cli-bank-{}.syx", std::process::id()));
    std::fs::write(&path, patch::write_bank(&[Patch::default(), voice])).unwrap();

    let choice = FmChoice { voices: Some(path.clone()), voice: 1 };
    let loaded = load_voice(&choice).unwrap();
    assert_eq!(loaded.name(), "TWO OPS");
    assert!(load_voice(&FmChoice { voices: Some(path.clone()), voice: 40 }).is_err());
    assert!(fm_synth(&["voices".into(), path.display().to_string()]).is_ok());

    let mut project = demo_project();
    put_fm_synth_on_note_tracks(&mut project, &loaded);
    // Twice: the synth replaces the instrument it put there before.
    put_fm_synth_on_note_tracks(&mut project, &loaded);
    assert!(project.channels.iter().all(|c| c.devices.len() == 1));
    let stored = &project.channels[0].devices[0];
    assert_eq!(stored.kind, DeviceKind::Builtin("fm-synth".into()));
    assert!(stored.params.iter().any(|p| p.id == "op2.coarse" && p.value == 3.0));
    let samples = suhina_engine::render_offline(&project, 48_000, 512).unwrap();
    let peak = samples.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    assert!(peak > 0.05 && peak <= 1.0, "peak {peak}");
    let _ = std::fs::remove_file(path);

    // A MIDI effect leading a chain stays before the synth.
    let pitch = Device::new("Pitch", DeviceKind::Builtin("pitch".into()));
    project.channels[0].devices.insert(0, pitch);
    put_fm_synth_on_note_tracks(&mut project, &loaded);
    let ids: Vec<&DeviceKind> = project.channels[0].devices.iter().map(|d| &d.kind).collect();
    assert_eq!(ids, [&DeviceKind::Builtin("pitch".into()), &DeviceKind::Builtin("fm-synth".into())]);
}

#[test]
fn theme_pack_and_validate_work_on_a_folder_and_its_package() {
    let dir = std::env::temp_dir().join(format!("suhina-cli-theme-{}", std::process::id()));
    let _gone = std::fs::remove_dir_all(&dir);
    let folder = dir.join("night");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("theme.css"), "theme { name: \"Night\"; } :root { --accent: #ff8800; }").unwrap();
    let args = |a: &[&str]| a.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
    theme(&args(&["validate", folder.to_str().unwrap()])).unwrap();
    theme(&args(&["pack", folder.to_str().unwrap()])).unwrap();
    let package = dir.join("night.theme.suhina");
    theme(&args(&["validate", package.to_str().unwrap()])).unwrap();
    let read = suhina_theme_package::read(&std::fs::read(&package).unwrap()).unwrap();
    assert_eq!((read.id.as_str(), read.name.as_str()), ("night", "Night"));
    std::fs::write(&package, b"damaged").unwrap();
    assert!(theme(&args(&["validate", package.to_str().unwrap()])).is_err());
    assert!(theme(&args(&["pack"])).is_err());
    std::fs::remove_dir_all(&dir).unwrap();
}
