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
