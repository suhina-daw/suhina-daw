// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Chains that wait for their own plug-ins only (#151): a chain goes on to its next plug-in as
//! soon as the one it handed its block to answers, while the other chains' plug-ins are still
//! out, and a slow or hung plug-in holds up only its own chain, and that only until the block's
//! deadline. Stand-in plug-ins answer after a set time, measured on the clock.

use crate::{RenderInputs, RenderOptions};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use suhina_device_api::NoteEvent;
use suhina_model::*;

use crate::chain_tests::{device, max_diff, note, peak, project};
use crate::{AudioPool, PluginProcessor, PluginSlotInfo, Plugins, Renderer, Sent, plugin_devices};

/// A low sample rate, so a block of the most frames a chain runs at once (1024) has a deadline
/// (60 % of 128 ms) long enough for plug-ins that take tens of milliseconds.
const SR: u32 = 8000;
const FRAMES: usize = 1024;
const EFFECT: PluginSlotInfo = PluginSlotInfo { instrument: false, latency: 0 };
/// How long a slow stand-in takes to answer, and a quick one.
const SLOW: Duration = Duration::from_millis(25);
const QUICK: Duration = Duration::from_millis(1);
/// Longer than any block's deadline: a plug-in that hangs.
const HUNG: Duration = Duration::from_secs(3600);

/// When a stand-in was handed a block and when its answer was due.
#[derive(Clone, Copy, Debug)]
struct Hand {
    name: &'static str,
    sent: Instant,
    due: Instant,
}

/// A plug-in that scales the sound by `gain` and answers each block `delay` after it is handed
/// over, logging each hand-over.
struct Timed {
    name: &'static str,
    gain: f32,
    delay: Duration,
    due: Option<Instant>,
    result: Vec<f32>,
    log: Arc<Mutex<Vec<Hand>>>,
}

impl Timed {
    fn new(name: &'static str, gain: f32, delay: Duration, log: &Arc<Mutex<Vec<Hand>>>) -> Self {
        Self { name, gain, delay, due: None, result: Vec::new(), log: Arc::clone(log) }
    }
}

/// Waits until `at`, spinning rather than sleeping or yielding: a short sleep ends on the
/// system timer's tick on some systems (up to 15.6 ms late on Windows), and on a busy shared
/// runner a yield can hand the core away for a whole scheduler slice (10 to 30 ms seen on the
/// macOS and Windows runners), both much later than the engine's 50 µs look at a plug-in.
fn sleep_until(at: Instant) {
    while Instant::now() < at {
        std::hint::spin_loop();
    }
}

impl PluginProcessor for Timed {
    fn process(&mut self, buffer: &mut [f32], _: &[(usize, NoteEvent)], timeout: Duration) -> bool {
        if self.delay > timeout {
            std::thread::sleep(timeout);
            return false;
        }
        std::thread::sleep(self.delay);
        buffer.iter_mut().for_each(|s| *s *= self.gain);
        true
    }

    fn send(&mut self, buffer: &[f32], _: &[(usize, NoteEvent)], _: Duration) -> Sent {
        let sent = Instant::now();
        let due = sent + self.delay;
        self.due = Some(due);
        self.result = buffer.iter().map(|s| s * self.gain).collect();
        self.log.lock().unwrap().push(Hand { name: self.name, sent, due });
        Sent::Out
    }

    fn poll(&mut self, until: Instant) -> bool {
        let Some(due) = self.due else { return true };
        sleep_until(until.min(due));
        Instant::now() >= due
    }

    fn wait(&mut self, buffer: &mut [f32], timeout: Duration) -> bool {
        let Some(due) = self.due.take() else { return false };
        let limit = Instant::now() + timeout;
        if due > limit {
            sleep_until(limit);
            return false;
        }
        sleep_until(due);
        buffer.copy_from_slice(&self.result[..buffer.len()]);
        true
    }
}

fn plug() -> Device {
    let mut d = device("plug-in", &[]);
    d.kind = DeviceKind::Vst3 { id: "565354416263646566".into() };
    d
}

/// A project of one track per entry of `chains`, each playing a long note through its devices.
fn tracks(chains: Vec<Vec<Device>>) -> Project {
    let mut p = project(vec![note(0.0, 64.0, 69)], 64.0, Vec::new());
    let (track, channel, lane, column) =
        (p.tracks[0].clone(), p.channels[0].clone(), p.arrangement.lanes[0].clone(), p.launcher.columns[0].clone());
    p.tracks.clear();
    p.channels.clear();
    p.arrangement.lanes.clear();
    p.launcher.columns.clear();
    for (n, devices) in chains.into_iter().enumerate() {
        let id = n as u32 + 1;
        p.tracks.push(Track { id: TrackId(id), channel: Some(ChannelId(id)), ..track.clone() });
        p.channels.push(Channel { id: ChannelId(id), devices, ..channel.clone() });
        p.arrangement.lanes.push(ArrangementLane { track: Some(TrackId(id)), ..lane.clone() });
        p.launcher.columns.push(LauncherColumn { track: TrackId(id), ..column.clone() });
    }
    p
}

/// The project's plug-ins, in order, running in `stands`.
fn plugins(p: &Project, stands: Vec<Timed>) -> Plugins {
    let mut plugins = Plugins::new();
    for (found, stand) in plugin_devices(p).into_iter().zip(stands) {
        plugins.insert(found.key, EFFECT, Box::new(stand));
    }
    plugins
}

/// Runs `count` blocks in real time; the longest a block took.
fn play(renderer: &mut Renderer, count: usize) -> (Vec<f32>, Duration) {
    let mut out = Vec::new();
    let mut block = vec![0.0f32; FRAMES * 2];
    let mut longest = Duration::ZERO;
    for _ in 0..count {
        let started = Instant::now();
        renderer.process(&mut block);
        longest = longest.max(started.elapsed());
        out.extend_from_slice(&block);
    }
    (out, longest)
}

fn find(log: &[Hand], name: &str) -> Hand {
    *log.iter().find(|hand| hand.name == name).unwrap()
}

/// Two chains of three plug-ins whose slow plug-ins are at different places: one chain's first,
/// the other's last. In stages the block would take both slow plug-ins one after another (the
/// second stage of every chain waiting for the first chain's slow one); a chain that goes on
/// as its own plug-in answers takes the block about one slow plug-in long.
#[test]
fn a_chain_goes_on_as_soon_as_its_own_plug_in_answers() {
    // A shared runner can take the core away from the test for longer than a slow plug-in, which
    // no engine can make up for: the best of three tries counts. In stages no try could pass.
    let mut tries = Vec::new();
    for _ in 0..3 {
        match one_block_with_slow_plug_ins_at_different_places() {
            Ok(()) => return,
            Err(why) => tries.push(why),
        }
    }
    panic!("no try went on as its own plug-in answered: {tries:#?}");
}

/// One block of [`a_chain_goes_on_as_soon_as_its_own_plug_in_answers`]; why it failed.
fn one_block_with_slow_plug_ins_at_different_places() -> Result<(), String> {
    let p = tracks(vec![vec![plug(), plug(), plug()], vec![plug(), plug(), plug()]]);
    let log = Arc::default();
    let stands = vec![
        Timed::new("a1", 0.5, SLOW, &log),
        Timed::new("a2", 0.5, QUICK, &log),
        Timed::new("a3", 0.5, QUICK, &log),
        Timed::new("b1", 0.5, QUICK, &log),
        Timed::new("b2", 0.5, QUICK, &log),
        Timed::new("b3", 0.5, SLOW, &log),
    ];
    let mut renderer = Renderer::build(
        &p,
        RenderInputs::from_audio(&AudioPool::new()).with_plugins(plugins(&p, stands)),
        SR,
        RenderOptions::PLAYING,
    )
    .unwrap();
    let (out, longest) = play(&mut renderer, 1);
    let log = log.lock().unwrap().clone();
    assert_eq!(log.len(), 6, "every plug-in had the block: {log:?}");
    assert_eq!(renderer.graph.chains.iter_mut().map(|c| c.missed()).sum::<u64>(), 0);
    assert!(peak(&out) > 0.0);
    // The second chain's slow plug-in had its block before the first chain's slow one answered.
    if find(&log, "b3").sent >= find(&log, "a1").due {
        return Err(format!("b3 went out after a1 answered: {log:?}"));
    }
    // And the first chain went on as soon as its slow plug-in answered, not after the other's.
    if find(&log, "a2").sent >= find(&log, "b3").due {
        return Err(format!("a2 went out after b3 answered: {log:?}"));
    }
    if longest >= SLOW * 2 - QUICK * 4 {
        return Err(format!("the block took {longest:?}, as long as two slow plug-ins"));
    }
    Ok(())
}

/// A chain whose plug-in hangs misses that block at the deadline; the other chains, whose
/// plug-ins answer in time, are not held up by it: they run all their plug-ins long before the
/// deadline, and the block returns at the deadline, not later.
#[test]
fn a_hung_plug_in_holds_up_only_its_own_chain_and_only_until_the_deadline() {
    let p = tracks(vec![vec![plug(), plug()], vec![plug(), plug(), plug()], vec![plug(), plug()]]);
    let log = Arc::default();
    let stands = vec![
        Timed::new("hung", 0.5, HUNG, &log),
        Timed::new("after-hung", 0.5, QUICK, &log),
        Timed::new("b1", 0.5, QUICK, &log),
        Timed::new("b2", 0.5, QUICK, &log),
        Timed::new("b3", 0.5, QUICK, &log),
        Timed::new("slow", 0.5, SLOW, &log),
        Timed::new("c2", 0.5, QUICK, &log),
    ];
    let mut renderer = Renderer::build(
        &p,
        RenderInputs::from_audio(&AudioPool::new()).with_plugins(plugins(&p, stands)),
        SR,
        RenderOptions::PLAYING,
    )
    .unwrap();
    let deadline = Duration::from_secs_f64(FRAMES as f64 / f64::from(SR) * crate::plugin::DEADLINE_SHARE);
    let started = Instant::now();
    let (_, longest) = play(&mut renderer, 1);
    let log = log.lock().unwrap().clone();
    // The quick chain finished in a few quick round trips, the slow one after its slow plug-in:
    // neither waited for the hung plug-in.
    let b3 = find(&log, "b3");
    assert!(b3.due.duration_since(started) < SLOW, "{log:?}");
    let c2 = find(&log, "c2");
    assert!(c2.sent < started + SLOW + QUICK * 8, "{log:?}");
    // The block took until the deadline, for the hung plug-in, and no longer.
    assert!(longest >= deadline.mul_f64(0.9), "{longest:?}");
    assert!(longest < deadline + Duration::from_millis(15), "{longest:?} past the {deadline:?} deadline");
    let missed: Vec<u64> = renderer.graph.chains.iter_mut().map(|c| c.missed()).collect();
    // The hung plug-in missed, and the one after it, handed the block past the deadline, too.
    assert!(missed[0] >= 1, "{missed:?}");
    assert_eq!(&missed[1..], [0, 0], "the other chains answered in time");
}

/// An offline render sounds the same whatever order the chains' plug-ins answer in, and the
/// same as plug-ins that each answer at once: one render path, bit for bit. A built-in device
/// and an `.sd` device between the plug-ins run in the same loop.
#[test]
fn offline_the_order_plug_ins_answer_in_changes_nothing() {
    let mut graph =
        suhina_device_sdk::DeviceGraph::new("test.gain", "Test Gain", suhina_device_sdk::DeviceRole::Effect);
    graph.parameters.push(suhina_device_sdk::Parameter::new("gain", "Gain", (0.0, 2.0), 0.5));
    graph.nodes = vec![
        suhina_device_sdk::Node::audio_input("in"),
        suhina_device_sdk::Node::gain("gain", "gain"),
        suhina_device_sdk::Node::audio_output("out"),
    ];
    graph.connections =
        vec![suhina_device_sdk::Connection::new("in", "gain"), suhina_device_sdk::Connection::new("gain", "out")];
    let mut sd = device("sd", &[]);
    sd.kind = DeviceKind::Sd;
    sd.state = graph.to_sd().ok().map(String::into_bytes);
    let mut p =
        tracks(vec![vec![plug(), device("utility", &[("gain", -6.0)]), plug()], vec![plug(), sd, plug(), plug()]]);
    for content in &mut p.contents {
        content.length = 2.0;
    }
    for lane in &mut p.arrangement.lanes {
        for clip in &mut lane.clips {
            clip.length = 2.0;
        }
    }
    let render = |delays: [Duration; 5]| {
        let log = Arc::default();
        let gains = [0.5, 0.8, 0.7, 0.9, 0.6];
        let names = ["a1", "a2", "b1", "b2", "b3"];
        let stands = (0..5).map(|i| Timed::new(names[i], gains[i], delays[i], &log)).collect();
        crate::render_offline_from(
            &p,
            RenderInputs::from_audio(&AudioPool::new()).with_plugins(plugins(&p, stands)),
            SR,
            512,
            None,
        )
        .unwrap()
    };
    let at_once = render([Duration::ZERO; 5]);
    let first_slow = render([QUICK * 3, Duration::ZERO, Duration::ZERO, QUICK, Duration::ZERO]);
    let last_slow = render([Duration::ZERO, QUICK, Duration::ZERO, Duration::ZERO, QUICK * 3]);
    assert!(peak(&at_once) > 0.01);
    assert_eq!(at_once, first_slow);
    assert_eq!(at_once, last_slow);
    // And it is the plug-ins' sound: not the dry track.
    let dry = crate::render_offline(&tracks(vec![vec![], vec![]]), SR, 512).unwrap();
    assert!(max_diff(&at_once[..dry.len().min(at_once.len())], &dry) > 1e-3);
}
