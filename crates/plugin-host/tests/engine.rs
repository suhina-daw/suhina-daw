// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Plug-ins in the engine's track chains (ADR 0006, issue #100), with the plug-ins built into
//! the host: a project's plug-in effect and instrument in an offline render, a plug-in that
//! crashes or hangs while the engine plays on, and a session that finds, starts, watches,
//! restarts and opens the editor of its project's plug-ins through commands.
#![allow(clippy::panic, clippy::unwrap_used)] // test helpers outside #[test] functions

mod common;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use suhina_engine::{RenderInputs, RenderOptions, render_offline_from};

use serde_json::json;
use suhina_engine::{AudioPool, Renderer, plugin_devices, render_offline};
use suhina_model::*;
use suhina_plugins::scan::{Catalog, PluginInfo};
use suhina_plugins::{Health, Launch, Plugin};
use suhina_session::{Hosted, Player, PluginEnv, Session};

const SR: u32 = 48_000;
const BEAT: usize = SR as usize / 2;
const PATIENT: Duration = Duration::from_secs(20);

fn host() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_suhina-plugin-host"))
}

/// A catalog that holds the host's test plug-ins under their own names, each with the id
/// `<name>` (`test:gain` has the id `gain`), instruments marked as such.
fn catalog() -> Catalog {
    let mut catalog = Catalog::new();
    for (name, features) in [
        ("gain", vec!["audio-effect"]),
        ("vst3", vec!["Instrument|Synth"]),
        ("clap", vec!["audio-effect"]),
        ("crash-after:40", vec!["audio-effect"]),
        ("hang-after:40", vec!["audio-effect"]),
        ("delay-after:60:480", vec!["audio-effect"]),
    ] {
        let path = PathBuf::from(format!("test:{name}"));
        let info = PluginInfo {
            path: path.clone(),
            id: name.into(),
            name: format!("Test {name}"),
            vendor: "Suhina".into(),
            version: "1".into(),
            features: features.into_iter().map(String::from).collect(),
            replaces: Vec::new(),
            ara: false,
        };
        catalog.insert(path, vec![info]);
    }
    catalog
}

fn env() -> Arc<PluginEnv> {
    Arc::new(PluginEnv::new(catalog(), host()))
}

fn plugin_device(id: &str) -> Device {
    Device {
        name: format!("Test {id}"),
        kind: DeviceKind::Clap { id: id.into() },
        enabled: true,
        state: None,
        params: Vec::new(),
        automation: Vec::new(),
        extensions: Extensions::default(),
    }
}

/// One track playing a note from beat 0 to `length` beats (and one at beat 1 when `two`),
/// with `devices`.
fn project(length: f64, two: bool, devices: Vec<Device>) -> Project {
    let note = |time: f64, duration: f64| Note {
        time,
        duration,
        key: 69,
        velocity: 1.0,
        release_velocity: 0.5,
        channel: 0,
        muted: false,
    };
    let notes = if two { vec![note(0.0, 0.5), note(1.0, 0.5)] } else { vec![note(0.0, length)] };
    Project {
        tracks: vec![Track {
            id: TrackId(1),
            name: "T".into(),
            color: None,
            content: ContentKinds { notes: true, audio: false },
            channel: Some(ChannelId(1)),
            parent: None,
            midi_input: Default::default(),
            audio_input: Default::default(),
            extensions: Extensions::default(),
        }],
        channels: vec![Channel {
            id: ChannelId(1),
            name: "C".into(),
            role: ChannelRole::Regular,
            volume: 1.0,
            pan: 0.0,
            mute: false,
            solo: false,
            cue: false,
            output: None,
            sends: Vec::new(),
            devices,
            extensions: Extensions::default(),
            automation: Vec::new(),
        }],
        contents: vec![ClipContent {
            id: ContentId(1),
            name: "c".into(),
            length,
            lanes: vec![ContentLane { target: None, data: LaneData::Notes(notes) }],
            extensions: Extensions::default(),
            scale: None,
        }],
        arrangement: Arrangement {
            lanes: vec![ArrangementLane {
                id: Default::default(),
                name: "l".into(),
                track: Some(TrackId(1)),
                clips: vec![Clip {
                    id: Default::default(),
                    content: ContentId(1),
                    start: 0.0,
                    length,
                    offset: 0.0,
                    loop_range: None,
                    enabled: true,
                    fade_in: 0.0,
                    fade_out: 0.0,
                    fade_in_curve: Default::default(),
                    fade_out_curve: Default::default(),
                    follow_action: Default::default(),
                    launch: Default::default(),
                    color: None,
                    extensions: Extensions::default(),
                }],
            }],
            markers: Vec::new(),
        },
        ..Project::default()
    }
}

fn max_diff(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).fold(0.0f32, |m, (x, y)| m.max((x - y).abs()))
}

#[test]
fn an_offline_render_runs_a_projects_plug_in_effect() {
    let p = project(2.0, false, vec![plugin_device("gain")]);
    let started = suhina_session::start_offline(&p, &AudioPool::new(), &env(), SR);
    assert_eq!(started.running(), 1, "{:?}", started.notices);
    let wet = render_offline_from(
        &p,
        RenderInputs::from_audio(&AudioPool::new()).with_plugins(started.plugins),
        SR,
        512,
        None,
    )
    .unwrap();
    // Without the plug-in (not installed) the sound passes; the test gain halves it. The chain
    // adds a tail for effects to ring out.
    let dry = render_offline(&p, SR, 512).unwrap();
    assert!(wet.len() > dry.len());
    let half: Vec<f32> = dry.iter().map(|s| s * 0.5).collect();
    assert!(max_diff(&wet[..dry.len()], &half) < 1e-6, "{}", max_diff(&wet[..dry.len()], &half));
    assert!(wet[dry.len()..].iter().all(|s| s.abs() < 1e-6));
    assert!(dry.iter().any(|s| s.abs() > 0.1));
}

#[test]
fn an_offline_render_plays_notes_on_a_plug_in_instrument() {
    // The VST3 test plug-in answers each note with its velocity on the note's frame and
    // reports 32 frames of latency; it is the track's instrument, so no sine voices play.
    let mut device = plugin_device("vst3");
    device.state = Some(1.0f32.to_le_bytes().to_vec());
    let p = project(2.0, true, vec![device]);
    let started = suhina_session::start_offline(&p, &AudioPool::new(), &env(), SR);
    assert!(started.notices.is_empty(), "{:?}", started.notices);
    let out = render_offline_from(
        &p,
        RenderInputs::from_audio(&AudioPool::new()).with_plugins(started.plugins),
        SR,
        256,
        None,
    )
    .unwrap();
    let left: Vec<f32> = out.chunks(2).map(|f| f[0]).collect();
    let loud: Vec<usize> = left.iter().enumerate().filter(|(_, s)| s.abs() > 1e-6).map(|(i, _)| i).collect();
    assert_eq!(loud, [0, BEAT], "notes land on their frames");
    assert!((left[0] - 1.0).abs() < 1e-6, "{}", left[0]);
}

/// Runs `blocks` blocks of 480 frames as playback does (with a real-time deadline) and returns
/// the output and the longest a block took.
fn play(renderer: &mut Renderer, blocks: usize) -> (Vec<f32>, Duration) {
    let mut out = Vec::new();
    let mut block = vec![0.0f32; 480 * 2];
    let mut longest = Duration::ZERO;
    for _ in 0..blocks {
        let started = Instant::now();
        renderer.process(&mut block);
        longest = longest.max(started.elapsed());
        out.extend_from_slice(&block);
    }
    (out, longest)
}

fn start(spec: &str) -> (Plugin, Hosted) {
    let launch = Launch {
        host: host(),
        plugin: spec.into(),
        channels: 2,
        max_frames: suhina_engine::MAX_BLOCK_FRAMES as u32,
        sample_rate: f64::from(SR),
        timeout: PATIENT,
        group: None,
        ara: None,
    };
    let (plugin, audio) = Plugin::start(launch).unwrap();
    (plugin, Hosted::new(audio))
}

/// Each round trip the engine makes counts once against a plug-in that does not answer, however
/// the block reaches it (#188): a host still busy with an earlier block refuses the next, and
/// that refusal is the round trip's one miss, not one for handing it over and another for
/// trying again. These blocks (480 frames, a few notes) are one round trip each (a block is
/// two only when its events fill the chain's queue), so a plug-in that hangs in its first block
/// is hung after exactly `HUNG_AFTER` blocks, on a quiet machine or a loaded one.
#[test]
fn a_block_a_plug_in_does_not_answer_counts_once() {
    let p = project(8.0, false, vec![plugin_device("x")]);
    let key = plugin_devices(&p)[0].key;
    let (mut control, hosted) = start("test:hang-after:0");
    let mut plugins = suhina_engine::Plugins::new();
    plugins.insert(key, suhina_engine::PluginSlotInfo::default(), Box::new(hosted));
    let mut renderer = Renderer::build(
        &p,
        RenderInputs::from_audio(&AudioPool::new()).with_plugins(plugins),
        SR,
        RenderOptions::PLAYING,
    )
    .unwrap();
    let below = suhina_plugins::HUNG_AFTER as usize - 1;
    play(&mut renderer, below);
    assert_eq!(control.health(), Health::Running, "after {below} unanswered blocks");
    play(&mut renderer, 1);
    assert_eq!(control.health(), Health::Hung, "after {} unanswered blocks", below + 1);
}

/// A plug-in still busy with its first blocks (just started or restarted): an offline render
/// waits for it, so every block goes through it and none is lost (ADR 0006, decision 4); in
/// real time a block it cannot take in time is lost to it, and the sound passes through.
#[test]
fn a_busy_plug_in_loses_no_block_offline_and_passes_the_sound_in_real_time() {
    let p = project(8.0, false, vec![plugin_device("x")]);
    let key = plugin_devices(&p)[0].key;
    let mut dry = Renderer::build(&p, RenderInputs::from_audio(&AudioPool::new()), SR, RenderOptions::PLAYING).unwrap();
    dry.set_offline(true);
    let (reference, _) = play(&mut dry, 20);

    let (_control, hosted) = start("test:slow-start:5");
    let counts = hosted.counts();
    let mut plugins = suhina_engine::Plugins::new();
    plugins.insert(key, suhina_engine::PluginSlotInfo::default(), Box::new(hosted));
    let mut renderer = Renderer::build(
        &p,
        RenderInputs::from_audio(&AudioPool::new()).with_plugins(plugins),
        SR,
        RenderOptions::PLAYING,
    )
    .unwrap();
    renderer.set_offline(true);
    let (out, _) = play(&mut renderer, 20);
    assert_eq!(counts.read(), (20, 0), "every block given, none late");
    let half: Vec<f32> = reference.iter().map(|s| s * 0.5).collect();
    assert!(max_diff(&out, &half) < 1e-6, "every block went through the plug-in: {}", max_diff(&out, &half));

    // The same in real time, a block every 10 ms as a device asks for them: its slow blocks
    // are late or refused, and those pass the sound; once settled it answers again.
    let (_control, hosted) = start("test:slow-start:5");
    let counts = hosted.counts();
    let mut plugins = suhina_engine::Plugins::new();
    plugins.insert(key, suhina_engine::PluginSlotInfo::default(), Box::new(hosted));
    let mut renderer = Renderer::build(
        &p,
        RenderInputs::from_audio(&AudioPool::new()).with_plugins(plugins),
        SR,
        RenderOptions::PLAYING,
    )
    .unwrap();
    let period = Duration::from_millis(10);
    for _ in 0..40 {
        let started = Instant::now();
        play(&mut renderer, 1);
        std::thread::sleep(period.saturating_sub(started.elapsed()));
    }
    let (given, late) = counts.read();
    assert_eq!(given, 40);
    assert!(late >= 1, "a 30 ms block misses a 10 ms block's deadline");
    assert!(late < 40, "it answers again once settled ({late} late)");
}

#[test]
fn a_plug_in_that_crashes_or_hangs_leaves_the_engine_playing() {
    for (spec, health) in [("test:crash-after:40", Health::Crashed), ("test:hang-after:40", Health::Hung)] {
        let p = project(8.0, false, vec![plugin_device("x")]);
        let key = plugin_devices(&p)[0].key;
        let (mut control, hosted) = start(spec);
        let mut plugins = suhina_engine::Plugins::new();
        plugins.insert(key, suhina_engine::PluginSlotInfo::default(), Box::new(hosted));
        let mut renderer = Renderer::build(
            &p,
            RenderInputs::from_audio(&AudioPool::new()).with_plugins(plugins),
            SR,
            RenderOptions::PLAYING,
        )
        .unwrap();
        let mut controller = renderer.controller();
        // The same project without the plug-in, block for block: the effect passes the sound
        // through before it fails, and the engine passes it on without it after.
        let mut dry =
            Renderer::build(&p, RenderInputs::from_audio(&AudioPool::new()), SR, RenderOptions::PLAYING).unwrap();
        let deadline = Instant::now() + PATIENT;
        let mut longest = Duration::ZERO;
        let mut blocks = 0;
        // Until well after it failed (block 40 for the plug-in; more for the engine when a
        // busy test machine makes it miss some).
        while control.health() != health || blocks < 100 {
            assert!(Instant::now() < deadline, "{spec}: still {:?}", control.health());
            let (out, took) = play(&mut renderer, 1);
            let (reference, _) = play(&mut dry, 1);
            assert!(max_diff(&out, &reference) < 1e-6, "{spec}: {}", max_diff(&out, &reference));
            longest = longest.max(took);
            blocks += 1;
        }
        // No block waited much past its deadline (10 ms of audio, 60 % of it for plug-ins),
        // however the plug-in failed; the slack is for a busy test machine.
        assert!(longest < Duration::from_millis(100), "{spec}: a block took {longest:?}");
        // Restarted, it answers again: blocks come back done, so it is not counted as hung,
        // and its health says so at once, not the old instance's.
        let audio = control.restart().unwrap();
        assert_eq!(control.health(), Health::Running, "{spec}: right after the restart");
        let hosted = Hosted::new(audio);
        let counts = hosted.counts();
        controller.send_plugin(key, Box::new(hosted)).unwrap();
        // A block every 10 ms, as a device asks for them, until the new process answers one;
        // the blocks before that are misses, never enough to read as hung.
        let deadline = Instant::now() + PATIENT;
        loop {
            let started = Instant::now();
            play(&mut renderer, 1);
            let (given, late) = counts.read();
            if given > late {
                break;
            }
            assert!(Instant::now() < deadline, "{spec}: the restarted plug-in never answered ({late} late)");
            std::thread::sleep(Duration::from_millis(10).saturating_sub(started.elapsed()));
        }
        play(&mut renderer, 20);
        assert_eq!(control.health(), Health::Running, "{spec}");
    }
}

/// A session playing `project` through a renderer driven by the test, with the test catalog.
/// `offline` waits for each block as an offline render does, so what is heard does not depend
/// on how busy the test machine is (the deadline is tested above).
fn session(project: Project, offline: bool) -> (Session, Renderer) {
    let mut renderer = Renderer::stopped(&project, &AudioPool::new(), SR).unwrap();
    renderer.set_offline(offline);
    let controller = renderer.controller();
    let mut session = Session::new(project);
    session.attach_player(Player::new(controller, None));
    session.set_plugin_env(env());
    (session, renderer)
}

/// Plays blocks while the session looks after its plug-ins, until `done` says so.
fn until(session: &mut Session, renderer: &mut Renderer, what: &str, done: impl Fn(&serde_json::Value) -> bool) {
    let deadline = Instant::now() + PATIENT;
    loop {
        session.sync_plugins();
        play(renderer, 2);
        let status = session.execute("device.plugin", &json!({ "track": 1, "device": 0 })).unwrap();
        if done(&status) {
            return;
        }
        assert!(Instant::now() < deadline, "{what}: {status}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn a_session_starts_its_projects_plug_ins_and_reports_the_missing_ones() {
    let mut missing = plugin_device("not-installed");
    missing.name = "Gone".into();
    missing.state = Some(vec![1, 2, 3]);
    // A note long enough to still sound however long the plug-in takes to start.
    let p = project(256.0, false, vec![plugin_device("gain"), missing]);
    let (mut session, mut renderer) = session(p.clone(), true);
    session.execute("transport.play", &json!({})).unwrap();
    let notices = session.sync_plugins();
    assert!(notices.iter().any(|n| n.contains("Gone")), "{notices:?}");
    until(&mut session, &mut renderer, "starting", |s| s["status"] == "running");
    let status = session.execute("device.plugin", &json!({ "track": 1, "device": 1 })).unwrap();
    assert_eq!(status["status"], "missing");
    let listed = session.execute("track.devices", &json!({ "track": 1 })).unwrap();
    assert_eq!(listed[0]["plugin"]["status"], "running");
    assert_eq!(listed[0]["plugin"]["plugin"]["name"], "Test gain");
    // The missing plug-in keeps its state untouched, to be saved back as it was.
    assert_eq!(session.project().channels[0].devices[1].state, Some(vec![1, 2, 3]));
    assert_eq!(session.project().channels[0].devices, p.channels[0].devices);

    // The plug-in is heard: the track at half its level, once the graph with it plays.
    let (out, _) = play(&mut renderer, 10);
    let mut dry = Renderer::build(&p, RenderInputs::from_audio(&AudioPool::new()), SR, RenderOptions::PLAYING).unwrap();
    let (reference, _) = play(&mut dry, 10);
    let peak = |s: &[f32]| s.iter().fold(0.0f32, |m, x| m.max(x.abs()));
    assert!((peak(&out) - peak(&reference) * 0.5).abs() < 0.02, "{} vs {}", peak(&out), peak(&reference));

    // Removing the device stops its plug-in; undo starts it again.
    session.execute("device.remove", &json!({ "track": 1, "device": 0 })).unwrap();
    session.sync_plugins();
    let status = session.execute("device.plugin", &json!({ "track": 1, "device": 0 })).unwrap();
    assert_eq!(status["status"], "missing", "the other device is at 0 now");
    session.execute("edit.undo", &json!({})).unwrap();
    until(&mut session, &mut renderer, "after undo", |s| s["status"] == "running");
}

#[test]
fn a_crashed_plug_in_is_reported_and_restarts_by_command() {
    let p = project(16.0, false, vec![plugin_device("crash-after:40")]);
    let (mut session, mut renderer) = session(p, false);
    session.execute("transport.play", &json!({})).unwrap();
    until(&mut session, &mut renderer, "starting", |s| s["status"] == "running");
    until(&mut session, &mut renderer, "crashing", |s| s["status"] == "crashed");
    let error = session.execute("device.openEditor", &json!({ "track": 1, "device": 0 })).unwrap_err();
    assert!(error.message().to_owned().contains("crashed"), "{error}");
    session.execute("device.restart", &json!({ "track": 1, "device": 0 })).unwrap();
    until(&mut session, &mut renderer, "restarting", |s| s["status"] == "running");
}

#[test]
fn a_plug_in_devices_editor_opens_and_closes_by_command() {
    let p = project(8.0, false, vec![plugin_device("vst3")]);
    let (mut session, mut renderer) = session(p, false);
    until(&mut session, &mut renderer, "starting", |s| s["status"] == "running");
    let device = json!({ "track": 1, "device": 0 });
    let opened = session.execute("device.openEditor", &device);
    if cfg!(windows)
        || cfg!(target_os = "macos")
        || (cfg!(target_os = "linux") && std::env::var_os("DISPLAY").is_some_and(|d| !d.is_empty()))
    {
        let opened = opened.unwrap();
        assert_eq!((opened["width"].clone(), opened["height"].clone()), (json!(300), json!(200)));
        assert_eq!(session.execute("device.plugin", &device).unwrap()["editor_open"], true);
        session.execute("device.closeEditor", &device).unwrap();
        assert_eq!(session.execute("device.plugin", &device).unwrap()["editor_open"], false);
    } else {
        assert!(opened.is_err());
    }
    // A device that is not a plug-in has no editor.
    let mut other = session.project().clone();
    other.channels[0].devices.push(Device { kind: DeviceKind::Builtin("utility".into()), ..plugin_device("u") });
    let (mut session, _renderer) = self::session(other, false);
    let error = session.execute("device.openEditor", &json!({ "track": 1, "device": 1 })).unwrap_err();
    assert!(error.message().to_owned().contains("not a plug-in"), "{error}");
}

/// On Windows an editor opened by command floats above the window the session was given: that
/// window owns it. The owner lives on a thread that serves its messages, as the app's window
/// does (an owner that does not would hold up the host making the editor's window).
#[cfg(windows)]
#[test]
fn an_editor_is_owned_by_the_engines_window() {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DestroyWindow, DispatchMessageW, FindWindowExW, GW_OWNER, GetMessageW, GetWindow, MSG,
        PostThreadMessageW, WM_QUIT, WS_OVERLAPPEDWINDOW,
    };
    let (sender, receiver) = std::sync::mpsc::channel();
    let owner_thread = std::thread::spawn(move || {
        let class: Vec<u16> = "STATIC".encode_utf16().chain([0]).collect();
        // SAFETY: a system window class and no title, parent, menu or creation data.
        let owner = unsafe {
            CreateWindowExW(
                0,
                class.as_ptr(),
                std::ptr::null(),
                WS_OVERLAPPEDWINDOW,
                0,
                0,
                100,
                100,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null(),
            )
        };
        // SAFETY: no arguments.
        let thread = unsafe { windows_sys::Win32::System::Threading::GetCurrentThreadId() };
        sender.send((owner as usize, thread)).unwrap();
        // SAFETY: a zeroed message is a valid value to be filled.
        let mut message: MSG = unsafe { std::mem::zeroed() };
        // SAFETY: any message of this thread, until WM_QUIT.
        while unsafe { GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) } > 0 {
            // SAFETY: a message just taken from the queue.
            unsafe { DispatchMessageW(&message) };
        }
        // SAFETY: the window made above, on this thread.
        unsafe { DestroyWindow(owner as _) };
    });
    let (owner, thread) = receiver.recv().unwrap();
    assert_ne!(owner, 0);
    let p = project(8.0, false, vec![plugin_device("vst3")]);
    let (mut session, mut renderer) = session(p, false);
    session.set_editor_owner(Some(owner as u64));
    until(&mut session, &mut renderer, "starting", |s| s["status"] == "running");
    // The editor opens in the background: the session's thread (the app's window thread)
    // must not wait for the host, which waits for the owner window while it makes the editor's.
    let asked = std::time::Instant::now();
    let opening = session.execute("device.openEditor", &json!({ "track": 1, "device": 0 })).unwrap();
    assert_eq!(opening["editor_open"], "opening");
    assert!(asked.elapsed() < Duration::from_millis(500), "{:?}", asked.elapsed());
    until(&mut session, &mut renderer, "opening the editor", |s| s["editor_open"] == true);
    let editor_class: Vec<u16> = "SuhinaPluginEditor".encode_utf16().chain([0]).collect();
    let mut found = false;
    let mut after = std::ptr::null_mut();
    loop {
        // SAFETY: a class name that outlives the call; the previous window found, or none.
        let window = unsafe { FindWindowExW(std::ptr::null_mut(), after, editor_class.as_ptr(), std::ptr::null()) };
        if window.is_null() {
            break;
        }
        // SAFETY: a window just found.
        found |= unsafe { GetWindow(window, GW_OWNER) } as usize == owner;
        after = window;
    }
    assert!(found, "no editor window owned by the engine's window");
    session.execute("device.closeEditor", &json!({ "track": 1, "device": 0 })).unwrap();
    until(&mut session, &mut renderer, "closing the editor", |s| s["editor_open"] == false);
    // SAFETY: a message without pointers to the owner's thread.
    unsafe { PostThreadMessageW(thread, WM_QUIT, 0, 0) };
    owner_thread.join().unwrap();
}

/// With embedding on (the window's setting), an editor opened by command is a child inside
/// the window the session was given, the report carries its size for the panel to lay out
/// around, the window says where it goes, and turning embedding off moves it to a window of
/// its own.
#[test]
fn an_editor_embeds_in_the_engines_window_when_the_setting_says_so() {
    use common::{ParentWindow, soon};
    use suhina_session::Placement;
    if !common::editors_open_here() {
        eprintln!("no display for editors; skipped");
        return;
    }
    let parent = ParentWindow::open();
    let p = project(8.0, false, vec![plugin_device("vst3")]);
    let (mut session, mut renderer) = session(p, false);
    session.set_editor_owner(Some(parent.id()));
    session.set_editor_embedding(true);
    until(&mut session, &mut renderer, "starting", |s| s["status"] == "running");
    let device = json!({ "track": 1, "device": 0 });
    assert_eq!(session.execute("device.openEditor", &device).unwrap()["editor_open"], "opening");
    until(&mut session, &mut renderer, "opening the editor", |s| s["editor_open"] == true);
    if common::embeds_editors_here() {
        let report = session.plugin_report(TrackId(1), 0).unwrap();
        assert_eq!(report.editor_embedded, Some((300, 200)));
        assert!(soon(PATIENT, || parent.editor_children() == 1), "no editor child in the engine's window");
        // The window says where the panel shows it, every frame; a placement is sent once.
        let shown = Placement { x: 8, y: 40, clip_x: 0, clip_y: 40, clip_width: 400, clip_height: 260 };
        session.place_editors(&[(TrackId(1), 0, shown)]);
        session.sync_plugins();
    } else {
        // macOS: the host's process cannot attach its view to this window's, so the rack fell
        // back to a window of the editor's own: open, but never embedded.
        let report = session.plugin_report(TrackId(1), 0).unwrap();
        assert!(
            report.editor_open && report.editor_embedded.is_none(),
            "the editor did not fall back to a window of its own"
        );
    }
    // Embedding off: the editor closes and opens again in a window of its own.
    session.set_editor_embedding(false);
    assert!(
        soon(PATIENT, || {
            session.sync_plugins();
            session.plugin_report(TrackId(1), 0).is_some_and(|r| r.editor_open && r.editor_embedded.is_none())
                && parent.editor_children() == 0
        }),
        "the editor did not move to a window of its own"
    );
    // The reopened editor's control half comes back a moment after it reports open; the close
    // command says "busy" until then, so it is retried while the returns are pumped.
    assert!(
        soon(PATIENT, || {
            session.sync_plugins();
            session.execute("device.closeEditor", &device).is_ok()
        }),
        "the editor did not close"
    );
    until(&mut session, &mut renderer, "closing the editor", |s| s["editor_open"] == false);
}

#[test]
fn a_plug_in_that_could_not_start_starts_again_on_restart() {
    // In the catalog, but the host cannot load it.
    let mut catalog = catalog();
    let path = PathBuf::from("test:no-such-plugin");
    let info = PluginInfo {
        path: path.clone(),
        id: "broken".into(),
        name: "Broken".into(),
        vendor: "Suhina".into(),
        version: "1".into(),
        features: vec!["audio-effect".into()],
        replaces: Vec::new(),
        ara: false,
    };
    catalog.insert(path, vec![info]);
    let p = project(8.0, false, vec![plugin_device("broken")]);
    let (mut session, mut renderer) = session(p, false);
    session.set_plugin_env(Arc::new(PluginEnv::new(catalog, host())));
    until(&mut session, &mut renderer, "failing", |s| s["status"] == "failed");
    session.execute("device.restart", &json!({ "track": 1, "device": 0 })).unwrap();
    let status = session.execute("device.plugin", &json!({ "track": 1, "device": 0 })).unwrap();
    assert_eq!(status["status"], "stopped", "removed, to be started again");
    until(&mut session, &mut renderer, "failing again", |s| s["status"] == "failed");
}

/// `project` with a second track playing the same, panned apart: the first track hard left with
/// no devices, the second hard right through `devices`.
fn left_and_right(length: f64, devices: Vec<Device>) -> Project {
    let mut p = project(length, false, Vec::new());
    p.channels[0].pan = -1.0;
    let mut track = p.tracks[0].clone();
    track.id = TrackId(2);
    track.channel = Some(ChannelId(2));
    p.tracks.push(track);
    let mut channel = p.channels[0].clone();
    channel.id = ChannelId(2);
    channel.pan = 1.0;
    channel.devices = devices;
    p.channels.push(channel);
    let mut lane = p.arrangement.lanes[0].clone();
    lane.track = Some(TrackId(2));
    p.arrangement.lanes.push(lane);
    p
}

/// The left and right channels of `out`, apart.
fn sides(out: &[f32]) -> (Vec<f32>, Vec<f32>) {
    (out.chunks(2).map(|f| f[0]).collect(), out.chunks(2).map(|f| f[1]).collect())
}

/// A plug-in that changes its latency while the project plays (#110): the session hears of it
/// from the host, builds a new graph, and the tracks are lined up again. Two tracks play the
/// same note, one hard left without devices, one hard right through a plug-in that starts with
/// no latency and, from its 60th block, delays by 480 frames and says so. Rendered offline
/// through the session, left and right are the same sound before the change and again after it.
#[test]
fn tracks_stay_lined_up_when_a_plug_in_changes_its_latency() {
    let p = left_and_right(256.0, vec![plugin_device("delay-after:60:480")]);
    let (mut session, mut renderer) = session(p, true);
    session.execute("transport.play", &json!({})).unwrap();
    let device = json!({ "track": 2, "device": 0 });
    let status = |session: &mut Session| session.execute("device.plugin", &device).unwrap();
    let deadline = Instant::now() + PATIENT;
    while status(&mut session)["status"] != "running" {
        assert!(Instant::now() < deadline, "starting: {}", status(&mut session));
        session.sync_plugins();
        play(&mut renderer, 1);
        // Offline blocks render as fast as they can: unpaced, a slow start (tests running side
        // by side) plays past the note's end before the plug-in is up.
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(status(&mut session)["latency"], 0);
    // The graph with the plug-in in it, then a few blocks through it: lined up, no delay.
    session.sync_plugins();
    play(&mut renderer, 1);
    let (out, _) = play(&mut renderer, 10);
    let (left, right) = sides(&out);
    assert!(left.iter().any(|s| s.abs() > 0.1), "the note sounds");
    assert!(max_diff(&left, &right) < 1e-6, "before: {}", max_diff(&left, &right));
    assert_eq!(renderer.latency(), 0);

    // Played on until the plug-in has changed its latency and the session has taken it in.
    let deadline = Instant::now() + PATIENT;
    while status(&mut session)["latency"] != 480 {
        assert!(Instant::now() < deadline, "changing: {}", status(&mut session));
        session.sync_plugins();
        play(&mut renderer, 1);
        std::thread::sleep(Duration::from_millis(2));
    }
    // The new graph takes over with the next block; its delay lines fill in one more.
    play(&mut renderer, 2);
    assert_eq!(renderer.latency(), 480, "the other track waits for the plug-in's");
    let (out, _) = play(&mut renderer, 10);
    let (left, right) = sides(&out);
    assert!(right.iter().any(|s| s.abs() > 0.1), "the note still sounds");
    assert!(max_diff(&left, &right) < 1e-6, "after: {}", max_diff(&left, &right));
    // Out of line, the same note 480 frames apart is not the same sound: the check above
    // would see it.
    assert!(max_diff(&left[480..], &right[..right.len() - 480]) > 0.1);
}

#[test]
fn plug_ins_sharing_a_process_render_the_same_as_each_in_its_own() {
    // Four plug-ins in a row (the test processor and the CLAP test plug-in, twice each).
    let devices = vec![plugin_device("gain"), plugin_device("clap"), plugin_device("gain"), plugin_device("clap")];
    let p = project(2.0, true, devices);
    let render = |group: suhina_session::ProcessGroup| {
        let env = PluginEnv { catalog: catalog(), host: host(), group };
        let started = suhina_session::start_offline(&p, &AudioPool::new(), &env, SR);
        assert_eq!(started.running(), 4, "{:?}", started.notices);
        let processes = started.process_ids();
        let out = render_offline_from(
            &p,
            RenderInputs::from_audio(&AudioPool::new()).with_plugins(started.plugins),
            SR,
            512,
            None,
        )
        .unwrap();
        (out, processes)
    };
    // Each in its own process: four round trips a block.
    let (alone, processes) = render(Box::new(|_| None));
    assert!(processes.windows(2).all(|w| w[0] != w[1]), "{processes:?}");
    // All in one process: one round trip a block runs the four, one after another.
    let (together, processes) = render(Box::new(|_| Some("one".into())));
    assert!(processes.windows(2).all(|w| w[0] == w[1]), "{processes:?}");
    assert_eq!(alone.len(), together.len());
    assert!(alone.iter().zip(&together).all(|(a, b)| a.to_bits() == b.to_bits()), "{}", max_diff(&alone, &together));
    assert!(together.iter().any(|s| *s != 0.0));
}
