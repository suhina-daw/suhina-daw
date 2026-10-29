// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Several plug-in instances in one `suhina-plugin-host` process (ADR 0006, decision 2):
//! instances in one group share a process, each with its own audio and state; a crash or hang
//! takes the group's process, and restarts bring every instance back into one new process.
#![allow(clippy::panic, clippy::unwrap_used)] // test helpers outside #[test] functions

use std::path::PathBuf;
use std::time::{Duration, Instant};

use suhina_ipc::Outcome;
use suhina_plugins::{HUNG_AFTER, Health, Launch, Plugin, PluginAudio};

const FRAMES: usize = 64;
const DEADLINE: Duration = Duration::from_millis(2);
const PATIENT: Duration = Duration::from_secs(5);

fn launch(plugin: &str, group: Option<&str>) -> Launch {
    Launch {
        host: PathBuf::from(env!("CARGO_BIN_EXE_suhina-plugin-host")),
        plugin: plugin.into(),
        channels: 2,
        max_frames: FRAMES as u32,
        sample_rate: 48_000.0,
        timeout: PATIENT,
        group: group.map(str::to_string),
        ara: None,
    }
}

fn start(plugin: &str, group: Option<&str>) -> (Plugin, PluginAudio) {
    Plugin::start(launch(plugin, group)).unwrap_or_else(|e| panic!("{plugin}: {e}"))
}

/// Runs one block of ones and returns the outcome and the first output sample.
fn block(audio: &mut PluginAudio, timeout: Duration) -> (Outcome, f32) {
    let mut output = [9.0f32; FRAMES * 2];
    let outcome = audio.process(&[1.0; FRAMES * 2], &mut output, timeout);
    (outcome, output[0])
}

/// Missing the audio deadline is not a hang when every request is eventually answered.
#[test]
fn repeatedly_late_answers_do_not_mark_a_responsive_plugin_hung() {
    let (mut control, mut audio) = start("test:busy:10000", None);
    let mut output = [0.0; FRAMES * 2];
    for _ in 0..HUNG_AFTER + 2 {
        let until = Instant::now() + PATIENT;
        while !audio.ready(Instant::now()) {
            assert!(Instant::now() < until, "previous request never answered");
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(audio.send(&[1.0; FRAMES * 2], &[], output.len(), Duration::ZERO));
        let _ = audio.finish(&mut output, Duration::ZERO);
        assert_eq!(control.health(), Health::Running);
    }
}

fn wait_for(plugin: &mut Plugin, health: Health) {
    let deadline = Instant::now() + PATIENT;
    while plugin.health() != health {
        assert!(Instant::now() < deadline, "never became {health:?}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Whether a process with id `pid` runs.
fn process_runs(pid: u32) -> bool {
    if cfg!(windows) {
        let filter = format!("PID eq {pid}");
        let output = std::process::Command::new("tasklist").args(["/FI", &filter, "/NH", "/FO", "CSV"]).output();
        output.is_ok_and(|o| String::from_utf8_lossy(&o.stdout).contains(&format!("\"{pid}\"")))
    } else if cfg!(target_os = "macos") {
        // No `/proc` here; `ps` says whether a process with this id exists at all.
        std::process::Command::new("ps").args(["-p", &pid.to_string()]).output().is_ok_and(|o| o.status.success())
    } else {
        PathBuf::from(format!("/proc/{pid}")).exists()
    }
}

#[test]
fn instances_in_a_group_share_a_process_and_keep_apart() {
    let (mut first, mut first_audio) = start("test:gain", Some("share"));
    let (mut second, mut second_audio) = start("test:gain", Some("share"));
    assert_ne!(first.process_id(), 0);
    assert_eq!(first.process_id(), second.process_id());
    first.load_state(0.25f32.to_le_bytes().to_vec()).unwrap();
    for _ in 0..10 {
        assert_eq!(block(&mut first_audio, PATIENT), (Outcome::Done, 0.25));
        assert_eq!(block(&mut second_audio, PATIENT), (Outcome::Done, 0.5));
    }
    assert_eq!(first.save_state().unwrap().data, 0.25f32.to_le_bytes());
    assert_eq!(second.save_state().unwrap().data, 0.5f32.to_le_bytes());
    assert!(first.ping() && second.ping());
    assert_eq!((first.health(), second.health()), (Health::Running, Health::Running));
}

#[test]
fn instances_without_a_group_have_processes_of_their_own() {
    let (first, _a) = start("test:gain", None);
    let (second, _b) = start("test:gain", None);
    assert_ne!(first.process_id(), second.process_id());
    // Another group is another process too.
    let (third, _c) = start("test:gain", Some("own-one"));
    let (fourth, _d) = start("test:gain", Some("own-two"));
    assert_ne!(third.process_id(), fourth.process_id());
}

#[test]
fn a_crash_takes_the_group_and_restarts_bring_both_back_into_one_process() {
    let (mut gain, mut gain_audio) = start("test:gain", Some("crash"));
    gain.load_state(0.25f32.to_le_bytes().to_vec()).unwrap();
    assert!(gain.params().unwrap().is_empty());
    let (mut crashing, mut crashing_audio) = start("test:crash-after:2", Some("crash"));
    let before = gain.process_id();
    assert_eq!(crashing.process_id(), before);
    assert_eq!(block(&mut gain_audio, PATIENT), (Outcome::Done, 0.25));
    for _ in 0..2 {
        assert_eq!(block(&mut crashing_audio, PATIENT), (Outcome::Done, 1.0));
    }
    assert_ne!(block(&mut crashing_audio, DEADLINE).0, Outcome::Done);
    wait_for(&mut crashing, Health::Crashed);
    assert_eq!(gain.health(), Health::Crashed);
    assert_ne!(block(&mut gain_audio, DEADLINE).0, Outcome::Done);

    // The first restart starts a new process for the group; the sibling still has lost its
    // process (it is the old one), and its restart joins the new one.
    let mut crashing_audio = crashing.restart().unwrap();
    let after = crashing.process_id();
    assert_ne!(after, before);
    assert_eq!(crashing.health(), Health::Running);
    assert_eq!(gain.health(), Health::Crashed);
    assert!(!gain.ping());
    let mut gain_audio = gain.restart().unwrap();
    assert_eq!(gain.process_id(), after);
    assert_eq!((gain.health(), crashing.health()), (Health::Running, Health::Running));
    assert_eq!(block(&mut gain_audio, PATIENT), (Outcome::Done, 0.25));
    assert_eq!(block(&mut crashing_audio, PATIENT), (Outcome::Done, 1.0));
}

#[test]
fn a_hung_instance_restarts_its_group_and_the_others_follow() {
    let (mut gain, mut gain_audio) = start("test:gain", Some("hang"));
    let (mut hanging, mut hanging_audio) = start("test:hang-after:1", Some("hang"));
    let before = gain.process_id();
    assert_eq!(block(&mut hanging_audio, PATIENT), (Outcome::Done, 1.0));
    assert_ne!(block(&mut hanging_audio, DEADLINE).0, Outcome::Done);
    for _ in 1..HUNG_AFTER {
        assert_ne!(block(&mut hanging_audio, DEADLINE).0, Outcome::Done);
    }
    // Only the hung instance counts as hung; the other's audio still flows.
    assert_eq!(hanging.health(), Health::Hung);
    assert_eq!(gain.health(), Health::Running);
    assert_eq!(block(&mut gain_audio, PATIENT), (Outcome::Done, 0.5));

    // Its audio thread is stuck in the process, so its restart replaces the process.
    let mut hanging_audio = hanging.restart().unwrap();
    assert_ne!(hanging.process_id(), before);
    assert_eq!(gain.health(), Health::Crashed);
    let mut gain_audio = gain.restart().unwrap();
    assert_eq!(gain.process_id(), hanging.process_id());
    assert_eq!(block(&mut gain_audio, PATIENT), (Outcome::Done, 0.5));
    assert_eq!(block(&mut hanging_audio, PATIENT), (Outcome::Done, 1.0));
}

#[test]
fn a_healthy_restart_in_a_group_keeps_the_process() {
    let (mut first, mut first_audio) = start("test:gain", Some("healthy"));
    let (mut second, mut second_audio) = start("test:gain", Some("healthy"));
    let pid = first.process_id();
    first.load_state(0.25f32.to_le_bytes().to_vec()).unwrap();
    let mut first_audio_after = first.restart().unwrap();
    assert_eq!(first.process_id(), pid);
    assert_eq!(block(&mut first_audio_after, PATIENT), (Outcome::Done, 0.25));
    // The old half was let go of; the sibling never noticed.
    assert_ne!(block(&mut first_audio, DEADLINE).0, Outcome::Done);
    assert_eq!(second.health(), Health::Running);
    assert_eq!(block(&mut second_audio, PATIENT), (Outcome::Done, 0.5));
    assert!(second.ping());
}

#[test]
fn dropping_one_instance_leaves_the_others_and_the_last_ends_the_process() {
    let (first, mut first_audio) = start("test:gain", Some("drop"));
    let (mut second, mut second_audio) = start("test:gain", Some("drop"));
    let pid = second.process_id();
    assert_eq!(block(&mut first_audio, PATIENT), (Outcome::Done, 0.5));
    drop(first);
    assert_ne!(block(&mut first_audio, DEADLINE).0, Outcome::Done);
    assert_eq!(second.health(), Health::Running);
    assert!(second.ping());
    second.load_state(0.75f32.to_le_bytes().to_vec()).unwrap();
    assert_eq!(block(&mut second_audio, PATIENT), (Outcome::Done, 0.75));
    assert!(process_runs(pid));
    drop(second);
    let deadline = Instant::now() + PATIENT;
    while process_runs(pid) {
        assert!(Instant::now() < deadline, "the process outlived its last instance");
        std::thread::sleep(Duration::from_millis(20));
    }
    // The group starts a new process for the next instance.
    let (third, mut third_audio) = start("test:gain", Some("drop"));
    assert_ne!(third.process_id(), pid);
    assert_eq!(block(&mut third_audio, PATIENT), (Outcome::Done, 0.5));
}

#[test]
fn dropping_a_hung_instance_leaves_the_others_running() {
    let (mut gain, mut gain_audio) = start("test:gain", Some("drop-hung"));
    let (hanging, mut hanging_audio) = start("test:hang-after:0", Some("drop-hung"));
    assert_ne!(block(&mut hanging_audio, DEADLINE).0, Outcome::Done);
    drop(hanging);
    assert_eq!(gain.health(), Health::Running);
    assert!(gain.ping());
    assert_eq!(block(&mut gain_audio, PATIENT), (Outcome::Done, 0.5));
}

#[test]
fn clap_and_vst3_plugins_load_twice_in_one_process_with_their_own_state() {
    for spec in ["test:clap", "test:vst3"] {
        let group = format!("twice-{spec}");
        let (mut first, mut first_audio) = start(spec, Some(&group));
        let (mut second, mut second_audio) = start(spec, Some(&group));
        assert_eq!(first.process_id(), second.process_id(), "{spec}");
        first.load_state(0.25f32.to_le_bytes().to_vec()).unwrap();
        second.set_param(0, 0.75).unwrap();
        assert_eq!(block(&mut first_audio, PATIENT), (Outcome::Done, 0.25), "{spec}");
        assert_eq!(block(&mut second_audio, PATIENT), (Outcome::Done, 0.75), "{spec}");
        assert_eq!(first.save_state().unwrap().data, 0.25f32.to_le_bytes(), "{spec}");
        assert_eq!(second.save_state().unwrap().data, 0.75f32.to_le_bytes(), "{spec}");
        assert_eq!(first.params().unwrap()[0].value, 0.25, "{spec}");

        // Letting go of one leaves the module loaded for the other, and for a third.
        drop(first);
        assert_eq!(block(&mut second_audio, PATIENT), (Outcome::Done, 0.75), "{spec}");
        let (third, mut third_audio) = start(spec, Some(&group));
        assert_eq!(third.process_id(), second.process_id(), "{spec}");
        assert_eq!(block(&mut third_audio, PATIENT), (Outcome::Done, 0.5), "{spec}");
        assert_eq!(block(&mut second_audio, PATIENT), (Outcome::Done, 0.75), "{spec}");
    }
}

#[test]
fn several_editors_open_at_once_in_one_process() {
    let editors_open_here =
        cfg!(windows) || (cfg!(target_os = "linux") && std::env::var_os("DISPLAY").is_some_and(|d| !d.is_empty()));
    if !editors_open_here {
        eprintln!("no display for editors; skipped");
        return;
    }
    let (mut first, mut first_audio) = start("test:vst3", Some("editors"));
    let (mut second, mut second_audio) = start("test:vst3", Some("editors"));
    assert_eq!(first.open_editor().unwrap(), (300, 200));
    assert_eq!(second.open_editor().unwrap(), (300, 200));
    assert!(first.editor_open() && second.editor_open());
    // The test plug-in's editor turns its own instance's gain to 0.125 when it opens.
    assert_eq!(block(&mut first_audio, PATIENT), (Outcome::Done, 0.125));
    assert_eq!(block(&mut second_audio, PATIENT), (Outcome::Done, 0.125));
    first.close_editor().unwrap();
    assert!(!first.editor_open());
    assert!(second.editor_open());
    second.close_editor().unwrap();
    assert!(!second.editor_open());
}

#[test]
fn asking_how_an_instance_is_never_waits_for_a_sibling_loading() {
    let (mut plugin, _audio) = start("test:gain", Some("slow-sibling"));
    assert_eq!(plugin.health(), Health::Running);
    // A sibling that takes two seconds to load holds the process meanwhile.
    let sibling = std::thread::spawn(|| start("test:slow-load:2000", Some("slow-sibling")));
    std::thread::sleep(Duration::from_millis(300));
    let asked = Instant::now();
    assert_eq!(plugin.health(), Health::Running);
    assert!(!plugin.editor_open());
    assert!(asked.elapsed() < Duration::from_millis(100), "{:?}", asked.elapsed());
    let (mut sibling, _sibling_audio) = sibling.join().unwrap();
    assert_eq!(sibling.process_id(), plugin.process_id());
    assert_eq!(sibling.health(), Health::Running);
}

/// One block of `input` sent jointly: `first`'s host runs it, then each of `followers`.
fn joint_block(
    first: &mut PluginAudio,
    followers: &[&PluginAudio],
    input: &[f32],
    timeout: Duration,
) -> (Outcome, Vec<f32>) {
    let members: Vec<u32> = followers.iter().map(|f| f.seat().unwrap().instance).collect();
    let mut output = vec![9.0f32; input.len()];
    let outcome = if first.send_joint(input, &[], &members, output.len(), Duration::ZERO) {
        first.finish(&mut output, timeout)
    } else {
        Outcome::Busy
    };
    for follower in followers {
        follower.followed(outcome);
    }
    (outcome, output)
}

#[test]
fn instances_of_one_process_run_a_block_in_one_round_trip() {
    // Three formats in one process: the test processor, the CLAP and the VST3 test plug-ins.
    let (_a, mut a) = start("test:gain", Some("joint"));
    let (_b, mut b) = start("test:clap", Some("joint"));
    let (_c, mut c) = start("test:vst3", Some("joint"));
    let (_d, d) = start("test:gain", None);
    let seats = [a.seat().unwrap(), b.seat().unwrap(), c.seat().unwrap()];
    assert!(seats.iter().all(|s| s.process == seats[0].process));
    assert_ne!(seats[0].instance, seats[1].instance);
    assert_ne!(d.seat().unwrap().process, seats[0].process, "a process of its own");

    let input: Vec<f32> = (0..FRAMES * 2).map(|n| (n as f32 * 0.37).sin()).collect();
    for _ in 0..20 {
        // Each on its own: three round trips, one after another.
        let mut alone = input.clone();
        for audio in [&mut a, &mut b, &mut c] {
            let mut output = vec![0.0f32; alone.len()];
            assert_eq!(audio.process(&alone, &mut output, PATIENT), Outcome::Done);
            alone = output;
        }
        // Together: one round trip, sample for sample the same.
        let (outcome, together) = joint_block(&mut a, &[&b, &c], &input, PATIENT);
        assert_eq!(outcome, Outcome::Done);
        assert!(together.iter().zip(&alone).all(|(x, y)| x.to_bits() == y.to_bits()));
        assert!(together.iter().any(|s| *s != 0.0));
    }
    // The followers' own links were left alone and still work.
    assert_eq!(block(&mut b, PATIENT).0, Outcome::Done);
}

#[test]
fn a_follower_that_crashes_takes_the_joint_block_and_the_group() {
    let (mut first, mut audio) = start("test:gain", Some("joint-crash"));
    let (mut crashing, crashing_audio) = start("test:crash-after:3", Some("joint-crash"));
    let input = [1.0f32; FRAMES * 2];
    for _ in 0..3 {
        assert_eq!(joint_block(&mut audio, &[&crashing_audio], &input, PATIENT).0, Outcome::Done);
    }
    // In the fourth block the follower takes the process with it: the block is a miss.
    let (outcome, output) = joint_block(&mut audio, &[&crashing_audio], &input, Duration::from_millis(500));
    assert_eq!(outcome, Outcome::Missed);
    assert!(output.iter().all(|s| *s == 9.0), "a missed block leaves the output alone");
    wait_for(&mut first, Health::Crashed);
    wait_for(&mut crashing, Health::Crashed);
}

#[test]
fn a_follower_that_hangs_counts_against_every_plug_in_of_the_block() {
    let (mut first, mut audio) = start("test:gain", Some("joint-hang"));
    let (mut hanging, hanging_audio) = start("test:hang-after:0", Some("joint-hang"));
    let input = [1.0f32; FRAMES * 2];
    let (outcome, _) = joint_block(&mut audio, &[&hanging_audio], &input, DEADLINE);
    assert_eq!(outcome, Outcome::Missed);
    // The host never answers that block: every later one is refused, for both.
    for _ in 0..HUNG_AFTER {
        assert_eq!(joint_block(&mut audio, &[&hanging_audio], &input, DEADLINE).0, Outcome::Busy);
    }
    wait_for(&mut first, Health::Hung);
    wait_for(&mut hanging, Health::Hung);
}

/// Measures what a joint round trip saves (#108): four instances of one process at 64 and 128
/// frames, as four round trips a block and as one, blocks paced as in playback. Prints the
/// average and the 99th percentile per block; run with
/// `cargo test -p suhina-plugin-host --release --test groups measure -- --ignored --nocapture`.
#[test]
#[ignore = "a measurement, not a check"]
fn measure_joint_round_trips() {
    const PLUGINS: usize = 4;
    const BLOCKS: usize = 2_000;
    for frames in [64usize, 128] {
        let launch = |group: &str| Launch { max_frames: frames as u32, ..launch("test:gain", Some(group)) };
        let started: Vec<(Plugin, PluginAudio)> =
            (0..PLUGINS).map(|_| Plugin::start(launch("measure")).unwrap()).collect();
        let (_controls, mut audios): (Vec<Plugin>, Vec<PluginAudio>) = started.into_iter().unzip();
        let period = Duration::from_secs_f64(frames as f64 / 48_000.0);
        let input = vec![0.25f32; frames * 2];
        let mut output = vec![0.0f32; frames * 2];
        let mut run = |joint: bool| {
            let mut times = Vec::with_capacity(BLOCKS);
            for _ in 0..BLOCKS {
                let started = Instant::now();
                if joint {
                    let (first, rest) = audios.split_first_mut().unwrap();
                    let members: Vec<u32> = rest.iter().map(|a| a.seat().unwrap().instance).collect();
                    assert!(first.send_joint(&input, &[], &members, output.len(), PATIENT));
                    assert_eq!(first.finish(&mut output, PATIENT), Outcome::Done);
                } else {
                    let mut block = input.clone();
                    for audio in audios.iter_mut() {
                        assert_eq!(audio.process(&block, &mut output, PATIENT), Outcome::Done);
                        block.copy_from_slice(&output);
                    }
                }
                times.push(started.elapsed());
                std::thread::sleep(period.saturating_sub(started.elapsed()));
            }
            times.sort();
            let mean = times.iter().sum::<Duration>() / BLOCKS as u32;
            (mean, times[BLOCKS * 99 / 100])
        };
        let (each_mean, each_p99) = run(false);
        let (joint_mean, joint_p99) = run(true);
        println!(
            "{frames} frames, {PLUGINS} plug-ins: one round trip each {each_mean:?} (99 %: {each_p99:?}); \
             one joint round trip {joint_mean:?} (99 %: {joint_p99:?})"
        );
    }
}
