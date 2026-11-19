// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

#[test]
fn the_variable_asks_for_one_thread_unless_it_names_more() {
    assert_eq!(render_threads_from(None), 1);
    assert_eq!(render_threads_from(Some("0")), 1);
    assert_eq!(render_threads_from(Some("1")), 1);
    assert_eq!(render_threads_from(Some(" 4 ")), 4);
    assert_eq!(render_threads_from(Some("many")), 1);
    assert_eq!(render_threads_from(Some("1000")), MAX_RENDER_THREADS);
}

#[test]
fn a_pool_of_none_is_no_pool() {
    assert!(RenderPool::new(0, PoolKind::RealTime).is_none());
    let pool = RenderPool::new(3, PoolKind::RealTime);
    assert_eq!(pool.as_ref().map(|p| p.threads()), Some(4));
    let lane = pool.as_ref().and_then(|p| p.lane(8_000));
    assert_eq!(lane.as_ref().map(Lane::threads), Some(4));
    assert!(!lane.as_ref().is_some_and(Lane::any_away));
    assert_eq!(RenderPool::new(100, PoolKind::Offline).map(|p| p.threads()), Some(MAX_RENDER_THREADS));
}

#[test]
fn a_pool_lends_each_lane_once_and_takes_it_back() {
    let Some(pool) = RenderPool::new(1, PoolKind::RealTime) else { panic!("a worker starts") };
    let lanes: Vec<Lane> = (0..LANES).filter_map(|_| pool.lane(8_000)).collect();
    assert_eq!(lanes.len(), LANES);
    assert!(pool.lane(8_000).is_none(), "every lane is taken");
    drop(lanes);
    assert!(pool.lane(8_000).is_some(), "a dropped renderer's lane is free again");
}

#[test]
fn a_worker_takes_the_lanes_that_play_to_a_device_first() {
    let shared = Shared {
        stop: AtomicBool::new(false),
        real_time_pool: true,
        real_time: AtomicUsize::new(0),
        real_time_rate: AtomicUsize::new(0),
        lane_real_time: std::array::from_fn(|_| AtomicBool::new(false)),
    };
    let mut producers = Vec::new();
    let mut ends = Vec::new();
    for _ in 0..3 {
        let (jobs_tx, jobs) = rtrb::RingBuffer::<Job>::new(DEPTH);
        let (replies, _rx) = rtrb::RingBuffer::<Reply>::new(DEPTH);
        producers.push(jobs_tx);
        ends.push(WorkerEnd { jobs, replies });
    }
    let job = |index| Job {
        index,
        chain: Chain::passthrough(TrackId(1), crate::NO_COLUMN, 8_000),
        frames: 64,
        context: ProcessContext::default(),
        reply_to: std::thread::current(),
        clock: crate::clock::handle(),
    };
    // Lane 0 renders offline, lane 2 plays: lane 2's chain goes first though lane 0 queued first.
    for (lane, index) in [(0usize, 10usize), (2, 20)] {
        assert!(producers.get_mut(lane).is_some_and(|p| p.push(job(index)).is_ok()));
    }
    shared.lane_real_time[2].store(true, Ordering::Release);
    let order: Vec<(usize, usize)> =
        std::iter::from_fn(|| next_job(&shared, &mut ends)).map(|(lane, job)| (lane, job.index)).collect();
    assert_eq!(order, [(2, 20), (0, 10)]);
}

use suhina_model::{ChannelId, ChannelRole, Clip, ContentId, Device, DeviceKind, DeviceParam, Note, Send};
use suhina_test_support::ProjectBuilder;

use crate::Renderer;

const SR: u32 = 48_000;
const BLOCK: usize = 512;

fn device(id: &str, params: &[(&str, f64)]) -> Device {
    Device {
        params: params.iter().map(|(p, v)| DeviceParam::new(*p, *v)).collect(),
        ..Device::new(id, DeviceKind::Builtin(id.into()))
    }
}

/// Four instrument tracks with effects: tracks 1 and 2 play into the main channel and send to
/// return 6 (a reverb), tracks 3 and 4 into group 5 (an EQ and a compressor), which plays into
/// the main channel too. Every chain may run on a worker.
fn project() -> suhina_model::Project {
    let notes = |key: u8| (0..8).map(|n| Note::new(f64::from(n) * 0.5, 0.4, key + (n % 3) as u8, 0.8)).collect();
    let mut builder =
        ProjectBuilder::new().bus_track(5, "Group", ChannelRole::Submix).bus_track(6, "Return", ChannelRole::Effect);
    let chains: [&[Device]; 4] = [
        &[device("fm-synth", &[]), device("overdrive", &[]), device("delay", &[])],
        &[device("fm-synth", &[]), device("saturator", &[]), device("auto-pan", &[])],
        &[device("fm-synth", &[]), device("compressor", &[]), device("reverb", &[])],
        &[device("fm-synth", &[]), device("eq", &[]), device("delay", &[])],
    ];
    for (n, devices) in (1u32..).zip(chains) {
        builder = builder
            .midi_track(n, &format!("T{n}"))
            .notes(n, &format!("c{n}"), 4.0, notes(48 + 7 * n as u8))
            .clip(n, Clip::new(ContentId(n), 0.0, 4.0));
        let devices = devices.to_vec();
        builder = builder.with(move |p| {
            if let Some(channel) = p.channel_mut(ChannelId(n)) {
                channel.devices = devices;
            }
        });
    }
    builder
        .output(3, 5)
        .parent(3, 5)
        .output(4, 5)
        .parent(4, 5)
        .with(|p| {
            for (source, level) in [(1, 0.5), (2, 0.7)] {
                if let Some(channel) = p.channel_mut(ChannelId(source)) {
                    channel.sends.push(Send { target: ChannelId(6), level, pre_fader: false });
                }
            }
            if let Some(group) = p.channel_mut(ChannelId(5)) {
                group.devices = vec![device("eq", &[]), device("compressor", &[])];
            }
            if let Some(ret) = p.channel_mut(ChannelId(6)) {
                ret.devices = vec![device("reverb", &[])];
            }
        })
        .build()
}

/// The project's first `blocks` blocks rendered on `threads` threads, and the chains the render
/// sent to workers.
fn render(threads: usize, blocks: usize) -> (Vec<f32>, u64) {
    let mut renderer = Renderer::new(&project(), SR, suhina_device_registry::factory()).unwrap();
    renderer.set_offline(true);
    renderer.set_render_threads(threads);
    assert_eq!(renderer.render_threads(), threads);
    let mut out = Vec::with_capacity(blocks * BLOCK * 2);
    let mut block = vec![0.0f32; BLOCK * 2];
    for _ in 0..blocks {
        renderer.process(&mut block);
        out.extend_from_slice(&block);
    }
    let sent = renderer.pool.as_ref().map_or(0, |lane| lane.sent);
    (out, sent)
}

fn bits(samples: &[f32]) -> Vec<u32> {
    samples.iter().map(|s| s.to_bits()).collect()
}

#[test]
fn a_render_on_several_threads_is_the_render_on_one_bit_for_bit() {
    // Two seconds: the notes, the delays' repeats and the reverbs' tails.
    let blocks = 2 * SR as usize / BLOCK;
    let (alone, sent) = render(1, blocks);
    assert_eq!(sent, 0);
    assert!(suhina_test_support::peak(&alone) > 0.01, "the project plays");
    for threads in [2, 3, 4, 8] {
        let (parallel, sent) = render(threads, blocks);
        // Every block's first chains go to idle workers before the render thread runs one.
        assert!(sent >= blocks as u64, "{threads} threads sent {sent} chains to workers");
        assert!(bits(&parallel) == bits(&alone), "{threads} threads render other samples than one");
    }
}

#[test]
fn render_threads_can_be_changed_and_turned_off() {
    let mut renderer = Renderer::new(&project(), SR, suhina_device_registry::factory()).unwrap();
    renderer.set_render_threads(3);
    assert_eq!(renderer.render_threads(), 3);
    renderer.set_render_threads(0);
    assert_eq!(renderer.render_threads(), 1);
    renderer.set_render_threads(usize::MAX);
    assert_eq!(renderer.render_threads(), MAX_RENDER_THREADS);
}

#[test]
fn a_few_chains_stay_on_the_render_thread() {
    // One instrument track and Main: fewer chains than PARALLEL_MIN, so no worker is woken.
    let project = ProjectBuilder::new()
        .midi_track(1, "T1")
        .notes(1, "c1", 4.0, vec![Note::new(0.0, 1.0, 60, 0.8)])
        .clip(1, Clip::new(ContentId(1), 0.0, 4.0))
        .with(|p| {
            if let Some(channel) = p.channel_mut(ChannelId(1)) {
                channel.devices = vec![device("fm-synth", &[])];
            }
        })
        .build();
    let mut renderer = Renderer::new(&project, SR, suhina_device_registry::factory()).unwrap();
    renderer.set_offline(true);
    renderer.set_render_threads(4);
    let mut block = vec![0.0f32; BLOCK * 2];
    for _ in 0..8 {
        renderer.process(&mut block);
    }
    assert_eq!(renderer.pool.as_ref().map_or(0, |lane| lane.sent), 0);
}

#[test]
fn a_tracks_late_blocks_carry_over_to_the_next_graph() {
    // The count is the track's: an edit while playing builds a new graph, whose chain adopts the
    // old one's, and the Status row and the trace keep counting from where they were.
    let mut old = Chain::passthrough(TrackId(3), crate::NO_COLUMN, 8_000);
    old.missed_blocks = 5;
    let mut new = Chain::passthrough(TrackId(3), crate::NO_COLUMN, 8_000);
    new.adopt(&mut old);
    assert_eq!(new.missed_blocks, 5);
    let mut other = Chain::passthrough(TrackId(4), crate::NO_COLUMN, 8_000);
    other.adopt(&mut new);
    assert_eq!(other.missed_blocks, 0, "another track's count stays its own");
}

#[test]
fn an_offline_pools_workers_never_join_real_time_priority() {
    // An export's workers stay at normal priority, so the system preempts them for playback.
    let Some(pool) = RenderPool::new(2, PoolKind::Offline) else { panic!("workers start") };
    let Some(lane) = pool.lane(8_000) else { panic!("a lane is free") };
    assert_eq!(lane.kind(), PoolKind::Offline);
    lane.join_real_time(256, 48_000);
    assert_eq!(pool.shared.real_time.load(Ordering::Acquire), 0);
    assert!(!pool.shared.lane_real_time.iter().any(|f| f.load(Ordering::Acquire)));
    let Some(playing) = RenderPool::new(2, PoolKind::RealTime) else { panic!("workers start") };
    let Some(lane) = playing.lane(8_000) else { panic!("a lane is free") };
    lane.join_real_time(256, 48_000);
    assert_eq!(playing.shared.real_time.load(Ordering::Acquire), 256);
}

/// A worker whose wake-up was spent while it was away (joining real time waits on a channel, which
/// parks the thread too) does not park once the pool is stopping: nothing would wake it again, and
/// the pool's drop would wait on it for good. `an_offline_pools_workers_never_join_real_time_priority`
/// hung so on Linux, now and then.
#[test]
fn a_worker_does_not_park_once_its_pool_is_stopping() {
    let shared = Shared {
        stop: AtomicBool::new(true),
        real_time_pool: true,
        real_time: AtomicUsize::new(0),
        real_time_rate: AtomicUsize::new(0),
        lane_real_time: std::array::from_fn(|_| AtomicBool::new(false)),
    };
    // No wake-up waits for this thread: a park here would never return.
    assert!(!park_unless_stopping(&shared));
}

/// The next job observes the reply ring synchronously: no thread, sleep or deadline is
/// needed to prove that an unrelated job cannot hold a completed feeder's reply back.
#[test]
fn a_completed_chain_is_returned_before_the_next_job_runs() {
    struct CheckReply {
        replies: Option<rtrb::Consumer<Reply>>,
        shared: Arc<Shared>,
    }
    impl crate::PluginProcessor for CheckReply {
        fn process(&mut self, _: &mut [f32], _: &[(usize, suhina_device_api::NoteEvent)], _: Duration) -> bool {
            let mut replies = self.replies.take().unwrap();
            assert_eq!(replies.pop().map(|reply| reply.index), Ok(0), "the feeder is already back");
            self.shared.stop.store(true, Ordering::Release);
            true
        }
    }
    let shared = Arc::new(Shared {
        stop: AtomicBool::new(false),
        real_time_pool: false,
        real_time: AtomicUsize::new(0),
        real_time_rate: AtomicUsize::new(0),
        lane_real_time: std::array::from_fn(|_| AtomicBool::new(false)),
    });
    let (mut jobs, jobs_rx) = rtrb::RingBuffer::new(DEPTH);
    let (replies, replies_rx) = rtrb::RingBuffer::new(DEPTH);
    let first = Chain::passthrough(TrackId(2), crate::NO_COLUMN, 8_000);
    let stored = Device::new("Observer", DeviceKind::Vst3 { id: "1".into() });
    let project = crate::chain_tests::project(Vec::new(), 1.0, vec![stored]);
    let mut plugins = crate::Plugins::new();
    plugins.insert(
        crate::plugin_devices(&project)[0].key,
        crate::PluginSlotInfo { instrument: false, latency: 0 },
        Box::new(CheckReply { replies: Some(replies_rx), shared: Arc::clone(&shared) }),
    );
    let next = Chain::build(
        &project,
        &crate::AudioPool::new(),
        TrackId(1),
        crate::NO_COLUMN,
        (1.0, 1.0),
        8_000,
        &mut plugins,
        (&crate::DeviceAssets::new(), suhina_device_registry::factory().as_ref()),
    )
    .unwrap();
    for (index, chain) in [first, next].into_iter().enumerate() {
        let job = Job {
            index,
            chain,
            frames: 1,
            context: ProcessContext::default(),
            reply_to: std::thread::current(),
            clock: crate::clock::handle(),
        };
        assert!(jobs.push(job).is_ok());
    }
    work(&shared, vec![WorkerEnd { jobs: jobs_rx, replies }]);
    assert!(shared.stop.load(Ordering::Acquire), "the next job ran");
}

#[test]
fn an_offline_renderer_renders_on_an_offline_pool() {
    let mut renderer = Renderer::new(&project(), SR, suhina_device_registry::factory()).unwrap();
    renderer.set_render_threads(3);
    assert_eq!(renderer.pool.as_ref().map(Lane::kind), Some(PoolKind::RealTime));
    renderer.set_offline(true);
    assert_eq!(renderer.pool.as_ref().map(Lane::kind), Some(PoolKind::Offline));
    assert_eq!(renderer.render_threads(), 3, "turning it offline keeps its threads");
    renderer.set_offline(false);
    assert_eq!(renderer.pool.as_ref().map(Lane::kind), Some(PoolKind::RealTime));
}

/// A stand-in for a hosted plug-in: hands its block out ([`PluginProcessor::send`]) and gives back
/// the sound bent through a curve of its own, as a host process would after a round trip.
struct Bend {
    drive: f32,
    result: Vec<f32>,
    key: [f32; BLOCK * 2],
    heard_key: Arc<AtomicBool>,
}

impl crate::PluginProcessor for Bend {
    fn process(&mut self, buffer: &mut [f32], _: &[(usize, suhina_device_api::NoteEvent)], _: Duration) -> bool {
        for (sample, key) in buffer.iter_mut().zip(&self.key) {
            *sample = (*sample * self.drive / (1.0 + key.abs())).tanh();
        }
        true
    }

    fn send(&mut self, buffer: &[f32], _: &[(usize, suhina_device_api::NoteEvent)], _: Duration) -> crate::Sent {
        self.result.clear();
        self.result.extend(buffer.iter().zip(&self.key).map(|(s, key)| (s * self.drive / (1.0 + key.abs())).tanh()));
        crate::Sent::Out
    }

    fn set_key(&mut self, key: &[f32]) {
        self.key.fill(0.0);
        for (to, from) in self.key.iter_mut().zip(key) {
            *to = *from;
        }
        if key.iter().any(|sample| *sample != 0.0) {
            self.heard_key.store(true, Ordering::Relaxed);
        }
    }

    fn wait(&mut self, buffer: &mut [f32], _: Duration) -> bool {
        let len = buffer.len().min(self.result.len());
        buffer[..len].copy_from_slice(&self.result[..len]);
        true
    }
}

/// [`project`] with a plug-in after every track's instrument, and a device group with a plug-in
/// in each of its two branches on the group track, including a nested group followed by an
/// effect. Track 2 keys track 1's plug-in after its devices, and track 4 keys track 3's
/// compressor after its fader. This covers the workers' key taps and reader dependencies as
/// well as suspension and resumption, without adding render blocks to the test.
fn plug_in_project() -> suhina_model::Project {
    let plug =
        |n: u32| Device { name: format!("P{n}"), ..Device::new("plug", DeviceKind::Vst3 { id: format!("{n:032}") }) };
    let mut p = project();
    for (n, channel) in (1u32..).zip(p.channels.iter_mut()) {
        let at = channel.devices.len().min(1);
        channel.devices.insert(at, plug(n));
        if channel.id == ChannelId(5) {
            let nested = Device::group(
                "Nested",
                suhina_model::DeviceGroup::with_branches(vec![
                    suhina_model::Branch { devices: vec![plug(102)], ..suhina_model::Branch::new("C") },
                    suhina_model::Branch { devices: vec![plug(103)], ..suhina_model::Branch::new("D") },
                ]),
            );
            let branches = vec![
                suhina_model::Branch { devices: vec![nested, plug(100)], ..suhina_model::Branch::new("A") },
                suhina_model::Branch { devices: vec![plug(101), device("eq", &[])], ..suhina_model::Branch::new("B") },
            ];
            channel.devices.push(Device::group("G", suhina_model::DeviceGroup::with_branches(branches)));
        }
    }
    p.channel_mut(ChannelId(1)).unwrap().devices[1].inputs =
        vec![suhina_model::DeviceInput::key(ChannelId(2), suhina_model::TapPoint::PostFx)];
    p.channel_mut(ChannelId(3)).unwrap().devices[2].inputs =
        vec![suhina_model::DeviceInput::key(ChannelId(4), suhina_model::TapPoint::PostFader)];
    p.assign_ids().unwrap();
    p.validate().unwrap();
    p
}

/// [`plug_in_project`]'s first `blocks` blocks rendered offline on `threads` threads, and the
/// chains sent to workers.
fn render_plug_ins(threads: usize, blocks: usize) -> (Vec<f32>, u64) {
    let p = plug_in_project();
    let mut plugins = crate::Plugins::new();
    let heard_key = Arc::new(AtomicBool::new(false));
    for (n, found) in crate::plugin_devices(&p).into_iter().enumerate() {
        let processor = Bend {
            drive: 1.0 + n as f32 * 0.25,
            result: Vec::with_capacity(BLOCK * 2),
            key: [0.0; BLOCK * 2],
            heard_key: Arc::clone(&heard_key),
        };
        plugins.insert(found.key, crate::PluginSlotInfo { instrument: false, latency: 0 }, Box::new(processor));
    }
    let mut renderer = Renderer::build(
        &p,
        crate::RenderInputs::from_audio(&crate::AudioPool::new(), suhina_device_registry::factory())
            .with_plugins(plugins),
        SR,
        crate::RenderOptions::PLAYING,
    )
    .unwrap();
    renderer.set_offline(true);
    renderer.set_render_threads(threads);
    let mut out = Vec::with_capacity(blocks * BLOCK * 2);
    let mut block = vec![0.0f32; BLOCK * 2];
    for _ in 0..blocks {
        renderer.process(&mut block);
        out.extend_from_slice(&block);
    }
    assert!(heard_key.load(Ordering::Relaxed), "the hosted effect received its source's key");
    (out, renderer.chains_sent_to_workers())
}

/// ADR 0038 › 4: chains with plug-ins go to the workers too, and a render on any number of threads
/// is still the render on one, bit for bit, plug-ins in groups included.
#[test]
fn plug_in_chains_render_on_several_threads_as_on_one_bit_for_bit() {
    let blocks = SR as usize / BLOCK;
    let (alone, sent) = render_plug_ins(1, blocks);
    assert_eq!(sent, 0);
    assert!(suhina_test_support::peak(&alone) > 0.01, "the project plays");
    for threads in [2, 4, 8] {
        let (parallel, sent) = render_plug_ins(threads, blocks);
        assert!(sent >= blocks as u64, "{threads} threads sent {sent} chains to workers");
        assert!(bits(&parallel) == bits(&alone), "{threads} threads render other samples than one");
    }
}
