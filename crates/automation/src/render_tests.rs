// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use suhina_engine::model::*;

use super::*;

const SR: u32 = 48_000;

/// One track whose only slot loops a short note on every beat, at 120 bpm.
fn project() -> Project {
    Project {
        tracks: vec![Track {
            content: ContentKinds { notes: true, audio: false },
            channel: Some(ChannelId(1)),
            ..Track::new(TrackId(1), "Drums")
        }],
        channels: vec![Channel::new(ChannelId(1), "Drums")],
        contents: vec![ClipContent {
            id: ContentId(1),
            name: "Beat".into(),
            length: 1.0,
            lanes: vec![ContentLane { target: None, data: LaneData::Notes(vec![Note::new(0.0, 0.25, 36, 1.0)]) }],
            extensions: Extensions::default(),
            scale: None,
        }],
        launcher: Launcher {
            mappings: Vec::new(),
            scenes: vec![Scene::default()],
            columns: vec![LauncherColumn {
                track: TrackId(1),
                slots: vec![Slot {
                    clip: Some(Clip {
                        loop_range: Some(LoopRange { start: 0.0, end: 1.0 }),
                        ..Clip::new(ContentId(1), 0.0, 1.0)
                    }),
                    has_stop: true,
                }],
            }],
        },
        ..Project::default()
    }
}

fn step(at: f64, id: &str, params: Value) -> LaunchStep {
    LaunchStep { at, id: id.into(), params }
}

fn temp_wav(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("suhina-render-{name}-{}.wav", std::process::id()))
}

/// Frames where sound starts after more than 100 silent frames, in the left channel.
fn onsets(path: &Path) -> Vec<usize> {
    let mut reader = hound::WavReader::open(path).unwrap();
    let left: Vec<f32> = reader.samples::<f32>().step_by(2).map(Result::unwrap).collect();
    let mut result = Vec::new();
    let mut silent_run = usize::MAX;
    for (i, s) in left.iter().enumerate() {
        let loud = s.abs() > 1e-6;
        if loud && silent_run > 100 {
            result.push(i);
        }
        silent_run = if loud { 0 } else { silent_run.saturating_add(1) };
    }
    result
}

fn samples(path: &Path) -> Vec<f32> {
    hound::WavReader::open(path).unwrap().samples::<f32>().map(Result::unwrap).collect()
}

#[test]
fn the_cue_renders_beside_the_main_mix() {
    let (main_path, cue_path) = (temp_wav("cue-main"), temp_wav("cue-bus"));
    let mut song = scene_as_arrangement(&project(), 0).unwrap();
    let alone = temp_wav("cue-alone");
    render_wav(&song, &AudioPool::new(), &suhina_engine::DeviceAssets::new(), &alone, SR, None).unwrap();
    // Nothing cued: the cue file is silent and the main mix the same.
    render_wavs(&song, &AudioPool::new(), &suhina_engine::DeviceAssets::new(), &main_path, Some(&cue_path), SR, None)
        .unwrap();
    assert_eq!(samples(&main_path), samples(&alone));
    assert!(samples(&cue_path).iter().all(|s| *s == 0.0));
    assert_eq!(samples(&cue_path).len(), samples(&main_path).len());
    // The track cued: on the cue as in the mix.
    song.channels[0].cue = true;
    render_wavs(&song, &AudioPool::new(), &suhina_engine::DeviceAssets::new(), &main_path, Some(&cue_path), SR, None)
        .unwrap();
    let same = samples(&main_path).iter().zip(samples(&alone)).all(|(a, b)| (a - b).abs() < 1e-6);
    assert!(same && samples(&main_path).len() == samples(&alone).len());
    let (main, cue) = (samples(&main_path), samples(&cue_path));
    assert!(cue.iter().any(|s| s.abs() > 0.1));
    assert!(main.iter().zip(&cue).all(|(m, c)| (m - c).abs() < 1e-5));
    for path in [main_path, cue_path, alone] {
        let _ = std::fs::remove_file(path);
    }
}

#[test]
fn a_launch_script_renders_launches_at_their_times() {
    let path = temp_wav("script");
    let steps = [
        step(0.1, "clip.launch", json!({ "track": "Drums", "scene": 1 })),
        step(0.3, "track.stop", json!({ "track": "Drums" })),
        step(0.31, "launcher.state", json!({})),
        step(9.0, "transport.stop", json!({})),
    ];
    let (summary, results) =
        render_launch_script(&project(), &AudioPool::new(), &path, SR, &steps, 3.0, None, None).unwrap();
    assert!((summary.seconds - 3.0).abs() < 1e-9);
    assert!(summary.peak > 0.1);
    // Launched at once (the transport was stopped), so the bar starts at 0.1 s; the stop waits
    // for the next bar, 2.1 s.
    let start = SR as usize / 10;
    let beat = SR as usize / 2;
    let expected = [start, start + beat, start + 2 * beat, start + 3 * beat];
    let actual = onsets(&path);
    assert_eq!(actual.len(), 4, "{actual:?}");
    assert!(actual.iter().zip(expected).all(|(a, e)| a.abs_diff(e) <= 2), "{actual:?}");
    assert_eq!(results.len(), 4);
    assert_eq!(results[0]["ok"], true);
    assert_eq!(results[2]["result"]["tracks"][0]["queued"], "stop");
    assert_eq!(results[3]["ok"], false);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn a_launch_script_can_move_the_play_start_marker() {
    let path = temp_wav("position");
    let steps = [
        step(0.0, "transport.setPosition", json!({ "beats": 8 })),
        step(0.0, "transport.play", json!({})),
        step(0.5, "launcher.state", json!({})),
        step(0.6, "transport.setPosition", json!({ "beats": -1 })),
    ];
    let (_, results) = render_launch_script(&project(), &AudioPool::new(), &path, SR, &steps, 1.0, None, None).unwrap();
    assert_eq!(results[2]["result"]["start_beat"], 8.0);
    // Half a second at 120 bpm: about a beat on from the marker (read as the last block began).
    let beat = results[2]["result"]["beat"].as_f64().unwrap_or_default();
    assert!((8.9..=9.0).contains(&beat), "{beat}");
    assert_eq!(results[3]["ok"], false);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn launch_scripts_take_only_transport_and_read_commands() {
    let path = temp_wav("bad");
    let run = |steps: &[LaunchStep], seconds: f64| {
        render_launch_script(&project(), &AudioPool::new(), &path, SR, steps, seconds, None, None).map(|_| ())
    };
    assert!(run(&[step(0.0, "transport.setTempo", json!({ "bpm": 90 }))], 1.0).is_err());
    assert!(run(&[step(0.0, "edit.undo", json!({}))], 1.0).is_err());
    assert!(run(&[step(0.0, "nope", json!({}))], 1.0).is_err());
    assert!(run(&[step(f64::NAN, "transport.play", json!({}))], 1.0).is_err());
    assert!(run(&[step(-1.0, "transport.play", json!({}))], 1.0).is_err());
    for seconds in [0.0, -1.0, f64::NAN, f64::INFINITY, MAX_SCRIPT_SECONDS + 1.0] {
        assert!(run(&[], seconds).is_err(), "{seconds}");
    }
    // A failing command is reported in its step, not as a failed render.
    let steps = [step(0.0, "clip.launch", json!({ "track": "Drums", "scene": 5 }))];
    let (_, results) = render_launch_script(&project(), &AudioPool::new(), &path, SR, &steps, 0.1, None, None).unwrap();
    assert_eq!(results[0]["ok"], false);
    let _ = std::fs::remove_file(&path);
}

/// A read and write root in a temp folder, for the confinement of a script's steps.
fn roots(name: &str) -> (std::path::PathBuf, Workspace) {
    let dir = std::env::temp_dir().join(format!("suhina-script-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("in")).unwrap();
    std::fs::create_dir_all(dir.join("elsewhere")).unwrap();
    std::fs::write(dir.join("in").join("kick.wav"), b"x").unwrap();
    let workspace = Workspace::new(Some(&dir.join("in")), Some(&dir.join("in"))).unwrap();
    (dir, workspace)
}

#[test]
fn a_launch_script_never_runs_a_file_writing_command() {
    let path = temp_wav("writers");
    let (dir, workspace) = roots("writers");
    // Even when the destination is inside the roots, the export and the save are not part
    // of a render: a script's render writes only its own output, at the path it was given.
    let inside = dir.join("in").join("mix.wav").display().to_string();
    for (id, params) in [
        ("project.export", json!({ "path": inside, "overwrite": true })),
        ("project.save", json!({ "path": dir.join("in").join("song.suhina"), "overwrite": true })),
    ] {
        let steps = [step(0.0, id, params)];
        assert!(
            render_launch_script(&project(), &AudioPool::new(), &path, SR, &steps, 0.1, None, Some(&workspace))
                .is_err(),
            "{id}"
        );
        assert!(
            render_launch_script(&project(), &AudioPool::new(), &path, SR, &steps, 0.1, None, None).is_err(),
            "{id}"
        );
    }
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_launch_scripts_steps_read_only_below_the_read_root() {
    let path = temp_wav("confined");
    let (dir, workspace) = roots("confined");
    let run = |steps: &[LaunchStep], seconds: f64| {
        render_launch_script(&project(), &AudioPool::new(), &path, SR, steps, seconds, None, Some(&workspace))
    };
    // A step's paths are confined the way a tool call's are.
    let outside = dir.join("elsewhere").join("mix.wav").display().to_string();
    assert!(run(&[step(0.0, "preview.play", json!({ "path": outside }))], 1.0).is_err());
    let steps =
        [step(0.0, "clip.launch", json!({ "track": "Drums", "scene": 1 })), step(0.5, "launcher.state", json!({}))];
    let (_, results) = run(&steps, 1.0).unwrap();
    assert!(results.iter().all(|r| r["ok"] == json!(true)), "{results:?}");
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_dir_all(dir);
}

/// Writes a two-channel 16-bit wav of a quiet tone at `seconds`, decodable by preview.play.
fn sample_at(path: &Path, seconds: f64) {
    let spec =
        hound::WavSpec { channels: 2, sample_rate: SR, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
    let mut writer = hound::WavWriter::create(path, spec).unwrap();
    let frames = (seconds * f64::from(SR)) as u32;
    for n in 0..frames {
        let value = ((std::f64::consts::TAU * 220.0 * f64::from(n) / f64::from(SR)).sin() * 8_000.0) as i16;
        writer.write_sample(value).unwrap();
        writer.write_sample(value).unwrap();
    }
    writer.finalize().unwrap();
}

#[test]
fn a_launch_scripts_step_runs_the_confined_parameters() {
    let path = temp_wav("confined-params");
    let (dir, workspace) = roots("confined-params");
    // Two files share the name a step gives relatively — one in the permitted root, one in
    // the process's working directory. Confinement resolves the name to the root's; the
    // step must read and preview that one, not the working directory's. The name is the
    // test's own, created only here and exclusively, so a file the working directory
    // already held of any name is untouched.
    let cwd = std::env::current_dir().unwrap();
    let mut name = format!(
        "kick-{}-{}.wav",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().subsec_nanos()
    );
    let cwd_file = loop {
        match std::fs::File::create_new(cwd.join(&name)) {
            Ok(_) => break cwd.join(&name),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                name = format!(
                    "kick-{}-{}-next.wav",
                    std::process::id(),
                    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().subsec_nanos()
                );
            }
            Err(e) => panic!("the test's own file in the working directory: {e}"),
        }
    };
    sample_at(&dir.join("in").join(&name), 0.25);
    sample_at(&cwd_file, 1.0);
    let in_root = dir.join("in").join(&name);
    let (_, results) = render_launch_script(
        &project(),
        &AudioPool::new(),
        &path,
        SR,
        &[step(0.0, "preview.play", json!({ "path": name }))],
        1.0,
        None,
        Some(&workspace),
    )
    .unwrap();
    assert_eq!(results[0]["ok"], json!(true), "the step runs the confined file: {results:?}");
    // Both sides name the root's file: the displayed path of what was read, against the
    // root file's own, without the verbatim prefix a canonicalised path carries on
    // Windows and without the two spellings one path has on macOS.
    let expected = crate::workspace::display_path(&in_root.canonicalize().unwrap());
    assert_eq!(results[0]["result"]["path"], json!(expected), "the preview reads the root's file: {results:?}");
    assert_eq!(results[0]["result"]["seconds"], json!(0.25), "{results:?}");
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_dir_all(dir);
    let _ = std::fs::remove_file(&cwd_file);
}
