// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
use suhina_model::{AudioRegion, Clip, ClipAudio, ContentId, StretchMode, WarpMarker};

const SR: u32 = 48_000;
const LENGTH: u64 = 2 * SR as u64;

/// A reader of one stretched voice: two seconds of a sine, warped to four beats at 100 bpm.
fn reader() -> AheadReader {
    let samples = (0..2 * SR as usize).map(|i| (i as f32 * 0.05).sin() * 0.5).collect();
    let data = Arc::new(crate::AudioData { sample_rate: SR, channels: 1, samples });
    let audio = ClipAudio {
        warped: true,
        stretch: StretchMode::Smooth,
        warp_markers: vec![WarpMarker { time: 0.0, audio_time: 0.0 }, WarpMarker { time: 4.0, audio_time: 2.0 }],
        ..ClipAudio::default()
    };
    let clip = Clip { audio, ..Clip::new(ContentId(1), 0.0, 4.0) };
    let Some(voice) = StretchVoice::new(&clip, data, 100.0, SR) else { panic!("no voice") };
    AheadReader::new(vec![(voice, 0, LENGTH, 7, 1)], Vec::new(), SR)
}

/// A reader of one stretched voice for each of `keys`, as [`reader`]'s, each a clip of its own.
fn reader_of(keys: &[u64]) -> AheadReader {
    let voices = keys
        .iter()
        .enumerate()
        .map(|(n, key)| {
            let samples = (0..2 * SR as usize).map(|i| (i as f32 * (0.03 + 0.01 * n as f32)).sin() * 0.5).collect();
            let data = Arc::new(crate::AudioData { sample_rate: SR, channels: 1, samples });
            let audio = ClipAudio {
                warped: true,
                stretch: StretchMode::Smooth,
                warp_markers: vec![
                    WarpMarker { time: 0.0, audio_time: 0.0 },
                    WarpMarker { time: 4.0, audio_time: 2.0 },
                ],
                ..ClipAudio::default()
            };
            let clip = Clip { audio, ..Clip::new(ContentId(1), 0.0, 4.0) };
            let Some(voice) = StretchVoice::new(&clip, data, 100.0, SR) else { panic!("no voice") };
            (voice, 0, LENGTH, *key, n as u32 + 1)
        })
        .collect();
    AheadReader::new(voices, Vec::new(), SR)
}

#[test]
fn voices_are_shared_out_by_clip_and_the_next_graph_hands_them_to_the_same_thread() {
    let keys = [3, 4, 5, 6, 7];
    let (mut old, mut new) = (reader_of(&keys), reader_of(&keys));
    let (Some(old_worker), Some(new_worker)) = (old.take_worker_unfilled(), new.take_worker_unfilled()) else {
        panic!("no workers")
    };
    let (mut old_shards, mut new_shards) = (old_worker.split(3), new_worker.split(3));
    assert_eq!(old_shards.len(), 3);
    assert_eq!(old.shared.shard_count(), 3);
    for (shard, worker) in old_shards.iter().enumerate() {
        assert_eq!(worker.shard, shard);
        assert!(worker.writers.iter().all(|w| w.clip as usize % 3 == shard), "shard {shard}");
        assert!(!worker.writers.is_empty(), "clips 1 to 5 fill every shard");
    }
    assert_eq!(old_shards.iter().map(|w| w.writers.len()).sum::<usize>(), keys.len(), "every voice once");
    // Each thread's shard of the next graph takes its voices over from the one before.
    for (new_worker, old_worker) in new_shards.iter_mut().zip(old_shards.iter_mut()) {
        new_worker.adopt_from(old_worker);
    }
    assert!(new.shared.adopted.iter().all(|slot| slot.load(Ordering::Acquire) != 0), "every voice handed over");
    new.adopt_from(&mut old, 0);
}

#[test]
fn a_rewind_waits_until_every_thread_has_stopped_pushing() {
    let mut reader = reader_of(&[2, 3]);
    let Some(worker) = reader.take_worker_unfilled() else { panic!("no worker") };
    let mut shards = worker.split(2);
    reader.move_voices(1_000);
    let requested = reader.shared.requested.load(Ordering::Acquire);
    assert_ne!(reader.shared.drained.load(Ordering::Acquire), requested, "nobody paused yet");
    shards[0].work();
    reader.service();
    assert_ne!(reader.shared.drained.load(Ordering::Acquire), requested, "one thread still pushes");
    shards[1].work();
    reader.service();
    assert_eq!(reader.shared.drained.load(Ordering::Acquire), requested, "both paused: the rings are emptied");
    // And each moves its voices there.
    assert!(shards.iter_mut().all(|w| w.work()));
    assert_eq!(shards.iter().map(|w| w.generation).collect::<Vec<_>>(), [requested, requested]);
}

#[test]
fn voices_rendered_on_several_threads_give_the_same_samples_as_on_one() {
    let keys = [10, 11, 12];
    let (mut one, mut many) = (reader_of(&keys), reader_of(&keys));
    let (Some(mut whole), Some(split)) = (one.take_worker_unfilled(), many.take_worker_unfilled()) else {
        panic!("no workers")
    };
    whole.fill();
    let mut shards = split.split(2);
    for shard in &mut shards {
        shard.fill();
    }
    let frames = 4 * BLOCK_FRAMES;
    for voice in 0..keys.len() {
        let (mut a, mut b) = (vec![0.0f32; frames * 2], vec![0.0f32; frames * 2]);
        one.read(voice, 0, &mut a);
        many.read(voice, 0, &mut b);
        assert!(a.iter().any(|v| *v != 0.0), "voice {voice} sounds");
        assert_eq!(a, b, "voice {voice}");
    }
}

/// The new graph's reader after its worker took the voice over and the swap, with a jump
/// asked for on the old graph in between when `jump_between`.
fn swapped(jump_between: bool) -> AheadReader {
    let (mut old, mut new) = (reader(), reader());
    let (Some(mut old_worker), Some(mut new_worker)) = (old.take_worker_unfilled(), new.take_worker_unfilled()) else {
        panic!("no workers")
    };
    new_worker.adopt_from(&mut old_worker);
    assert_ne!(new.shared.adopted[0].load(Ordering::Acquire), 0, "handed over");
    if jump_between {
        // As `read` asks for one when the voice fell too far behind.
        old.shared.jumps[0].to.store(30_000, Ordering::Release);
        old.shared.jumps[0].requested.store(1, Ordering::Release);
        old.readers[0].jumping = Some(1);
    }
    new.adopt_from(&mut old, 0);
    new
}

#[test]
fn a_handed_over_voice_plays_on_from_where_it_was() {
    let new = swapped(false);
    assert_eq!(new.readers[0].jumping, None);
}

#[test]
fn a_handed_over_voice_keeps_the_lead_its_jumps_go_by() {
    // A voice still catching up after jumps that left it behind: a swap that hands it over
    // must not reset its lead to the shortest, or it chases the playhead again from the start.
    let (mut old, mut new) = (reader(), reader());
    let (Some(mut old_worker), Some(mut new_worker)) = (old.take_worker_unfilled(), new.take_worker_unfilled()) else {
        panic!("no workers")
    };
    new_worker.adopt_from(&mut old_worker);
    old.readers[0].jump_blocks = JUMP_BLOCKS * 4;
    new.adopt_from(&mut old, 0);
    assert_eq!(new.readers[0].jump_blocks, JUMP_BLOCKS * 4);
}

#[test]
fn stopping_the_lanes_ends_every_thread_started() {
    // What `spawn_with` does with the threads already started when a later one fails to start.
    let mut thread = StretchThread::spawn_with(2).expect("threads start");
    let mut lanes = std::mem::take(&mut thread.lanes);
    stop_lanes(&thread.stop, &mut lanes);
    assert!(thread.stop.load(Ordering::Relaxed));
    assert!(lanes.iter().all(|lane| lane.handle.is_none()), "every thread joined");
}

#[test]
fn a_jump_the_old_graph_asked_for_after_the_handover_is_asked_for_again() {
    // The new writer renders the ring now and never saw the old graph's jump: the new
    // graph jumps the voice to the same frame, so the ring and the reader line up again.
    let new = swapped(true);
    assert_eq!(new.readers[0].jumping, Some(1));
    assert_eq!(new.shared.jumps[0].to.load(Ordering::Acquire), 30_000);
    assert_eq!(new.shared.jumps[0].requested.load(Ordering::Acquire), 1);
}

#[test]
fn a_voice_with_a_jump_under_way_is_handed_over_and_jumps_again() {
    // A jump asked for before the worker hands the voice over: the voice still goes to the
    // new graph, which asks for the same jump, instead of starting afresh.
    let (mut old, mut new) = (reader(), reader());
    let (Some(mut old_worker), Some(mut new_worker)) = (old.take_worker_unfilled(), new.take_worker_unfilled()) else {
        panic!("no workers")
    };
    old.shared.jumps[0].to.store(30_000, Ordering::Release);
    old.shared.jumps[0].requested.store(1, Ordering::Release);
    old.readers[0].jumping = Some(1);
    new_worker.adopt_from(&mut old_worker);
    assert_ne!(new.shared.adopted[0].load(Ordering::Acquire), 0, "handed over");
    new.adopt_from(&mut old, 0);
    assert_eq!(new.readers[0].jumping, Some(1), "the jump is asked for again");
    assert_eq!(new.shared.jumps[0].to.load(Ordering::Acquire), 30_000);
}

#[test]
fn a_voice_ahead_of_the_playhead_jumps_back_to_it() {
    // Left at a loop's end while the playhead went round to its start (a swap after a wrap
    // could read a clip from that voice again): silent until the playhead came round to it,
    // a whole pass, unless it moves.
    let mut reader = reader();
    let _ = reader.take_worker_unfilled();
    reader.readers[0].next = 60_000;
    let mut out = vec![0.0; 512];
    reader.read(0, 1_000, &mut out);
    assert!(reader.readers[0].jumping.is_some(), "it jumps");
    let to = reader.shared.jumps[0].to.load(Ordering::Acquire);
    assert!((1_000..10_000).contains(&to), "to just past the playhead: {to}");
}

#[test]
fn a_voice_still_behind_after_a_jump_jumps_further_and_a_whole_read_brings_it_back() {
    // A voice whose start-up takes longer than a jump's lead (its stretcher built and seeked
    // where it jumped to) lands behind the playhead again: a fixed lead chased it for ever and
    // the clip stayed silent. Each jump that still leaves it behind goes twice as far, within
    // the ring.
    let mut reader = reader();
    let _ = reader.take_worker_unfilled();
    let mut out = vec![0.0; 512];
    let wanted = 256u64;
    let mut leads = Vec::new();
    let mut from = 10_000u64;
    for _ in 0..6 {
        reader.read(0, from, &mut out[..wanted as usize * 2]);
        let to = reader.shared.jumps[0].to.load(Ordering::Acquire);
        leads.push((to - from - wanted) / BLOCK_FRAMES as u64);
        // The worker stops pushing for the jump; the reader drains the ring and goes on from
        // `to`, and the voice has rendered nothing by the time the playhead is past it.
        let requested = reader.shared.jumps[0].requested.load(Ordering::Acquire);
        reader.shared.jumps[0].paused.store(requested, Ordering::Release);
        reader.read(0, from, &mut out[..wanted as usize * 2]);
        from = to + 2 * BLOCK_FRAMES as u64;
    }
    let most = (reader.ring_frames / BLOCK_FRAMES as u64).saturating_sub(2).max(JUMP_BLOCKS);
    assert_eq!(leads[0], JUMP_BLOCKS, "the first jump goes the usual way");
    assert!(leads.windows(2).all(|w| w[1] >= w[0]), "each goes as far or further: {leads:?}");
    assert_eq!(leads.last().copied(), Some(most), "up to what the ring holds: {leads:?}");
}

#[test]
fn new_samples_under_the_same_pool_key_are_another_voice() {
    let region = AudioRegion::new("sine.wav");
    let clip = Clip::new(ContentId(1), 0.0, 4.0);
    let audio = || Arc::new(crate::AudioData { sample_rate: SR, channels: 1, samples: vec![0.0; 16] });
    let (a, b) = (audio(), audio());
    let key = |data: &Arc<crate::AudioData>| voice_key(&clip, &region, data, (0, LENGTH), 1);
    assert_eq!(key(&a), key(&a));
    assert_ne!(key(&a), key(&b), "the voice of the old samples is not taken over");
}

#[test]
fn a_changed_warp_mode_setting_is_another_voice() {
    let region = AudioRegion::new("sine.wav");
    let data = Arc::new(crate::AudioData { sample_rate: SR, channels: 1, samples: vec![0.0; 16] });
    let clip = Clip::new(ContentId(1), 0.0, 4.0);
    let key = |clip: &Clip| voice_key(clip, &region, &data, (0, LENGTH), 1);
    let mut changed = Vec::new();
    for edit in [
        |w: &mut suhina_model::WarpSettings| w.grain += 10.0,
        |w: &mut suhina_model::WarpSettings| w.scatter += 10.0,
        |w: &mut suhina_model::WarpSettings| w.envelope -= 10.0,
        |w: &mut suhina_model::WarpSettings| w.slice = suhina_model::SliceAt::Eighth,
        |w: &mut suhina_model::WarpSettings| w.tail = suhina_model::SliceTail::Off,
    ] {
        let mut other = clip.clone();
        edit(&mut other.audio.warp);
        changed.push(key(&other) != key(&clip));
    }
    assert_eq!(changed, [true; 5], "the old settings' voice is not taken over");
}
