// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
use suhina_model::*;

fn sine(hz: f64, seconds: f64, sample_rate: u32) -> Arc<AudioData> {
    let frames = (seconds * f64::from(sample_rate)) as usize;
    let samples = (0..frames)
        .map(|i| (std::f64::consts::TAU * hz * i as f64 / f64::from(sample_rate)).sin() as f32 * 0.5)
        .collect();
    Arc::new(AudioData { sample_rate, channels: 1, samples })
}

fn region(markers: &[(f64, f64)], mode: StretchMode) -> ClipAudio {
    ClipAudio {
        warped: true,
        stretch: mode,
        warp_markers: markers.iter().map(|&(time, audio_time)| WarpMarker { time, audio_time }).collect(),
        ..ClipAudio::default()
    }
}

/// `clip` playing with `audio`.
fn with(clip: &Clip, audio: &ClipAudio) -> Clip {
    Clip { audio: audio.clone(), ..clip.clone() }
}

fn clip(length: f64, offset: f64, loop_range: Option<(f64, f64)>) -> Clip {
    Clip {
        offset,
        loop_range: loop_range.map(|(start, end)| LoopRange { start, end }),
        ..Clip::new(ContentId(1), 0.0, length)
    }
}

#[test]
fn playlist_follows_markers_and_loops() {
    let data = sine(440.0, 4.0, 1000);
    // Beats 0–4 = 0–2 s, beats 4–6 = 2–4 s; loop beats 2–6; play 8 beats from beat 3.
    let r = region(&[(0.0, 0.0), (4.0, 2.0), (6.0, 4.0)], StretchMode::Smooth);
    let (pieces, _, _) = playlist(&with(&clip(8.0, 3.0, Some((2.0, 6.0))), &r), &data, 120.0).unwrap();
    let summary: Vec<(f64, f64, f64, f64, f64)> =
        pieces.iter().map(|p| (p.out_start, p.src_start, p.src_end, p.rate, p.stream_start)).collect();
    assert_eq!(
        summary,
        [
            (0.0, 1500.0, 2000.0, 1.0, 0.0),    // beat 3–4
            (0.5, 2000.0, 4000.0, 2.0, 500.0),  // beat 4–6, twice as fast
            (1.5, 1000.0, 2000.0, 1.0, 2500.0), // loop back to beat 2–4
            (2.5, 2000.0, 4000.0, 2.0, 3500.0), // beat 4–6
            (3.5, 1000.0, 1500.0, 1.0, 5500.0), // beat 2–3, clip ends
        ]
    );
}

/// A clip with tens of thousands of warp markers builds its playlist and plays: lookups are
/// binary searches, so the playlist is no longer quadratic in the markers (#162).
#[test]
fn a_clip_with_many_warp_markers_builds_and_plays() {
    let data = sine(440.0, 30.0, 8000);
    // 40 000 segments over 60 beats (30 s at 120 bpm), alternating 1.1x and 0.9x.
    let markers: Vec<(f64, f64)> = (0..=40_000)
        .scan(0.0, |seconds, i| {
            let beat = f64::from(i) * 60.0 / 40_000.0;
            let here = (beat, *seconds);
            *seconds += 60.0 / 40_000.0 * 0.5 * if i % 2 == 0 { 1.1 } else { 0.9 };
            Some(here)
        })
        .collect();
    let r = region(&markers, StretchMode::Smooth);
    let c = clip(60.0, 0.0, None);
    let (pieces, _, _) = playlist(&with(&c, &r), &data, 120.0).unwrap();
    assert_eq!(pieces.len(), 40_000);
    let mut voice = StretchVoice::new(&with(&c, &r), data, 120.0, 48_000).unwrap().prepared();
    voice.seek(48_000 * 15);
    let mut out = vec![0.0f32; 2 * 4096];
    for _ in 0..4 {
        voice.render(&mut out);
    }
    assert!(out.iter().all(|s| s.is_finite()) && out.iter().any(|s| s.abs() > 0.1));
}

/// The level of a 440 Hz sine stretched at 2x for four beats and then at `slower`, in 10 ms
/// windows, with how far each window is from a clean sine (the residual of the best-fitting
/// 440 Hz sine, relative to the window's level).
fn seam(slower: f64) -> Vec<(f64, f32, f32)> {
    let rate = 48_000;
    let data = sine(440.0, 8.0, rate);
    let r = region(&[(0.0, 0.0), (4.0, 4.0), (8.0, 4.0 + 2.0 * slower)], StretchMode::Smooth);
    let c = clip(8.0, 0.0, None);
    let mut voice = StretchVoice::new(&with(&c, &r), data, 120.0, rate).unwrap().prepared();
    let mut out = vec![0.0f32; 2 * 4 * rate as usize];
    for block in out.chunks_mut(2 * 512) {
        voice.render(block);
    }
    let left: Vec<f64> = out.as_chunks::<2>().0.iter().map(|[l, _]| f64::from(*l)).collect();
    let window = rate as usize / 100;
    left.chunks_exact(window)
        .enumerate()
        .map(|(i, w)| {
            let t0 = (i * window) as f64;
            let w_hz = std::f64::consts::TAU * 440.0 / f64::from(rate);
            let (mut ss, mut sc, mut cc, mut ys, mut yc, mut yy) = (0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
            for (k, y) in w.iter().enumerate() {
                let (sn, cs) = ((t0 + k as f64) * w_hz).sin_cos();
                ss += sn * sn;
                sc += sn * cs;
                cc += cs * cs;
                ys += y * sn;
                yc += y * cs;
                yy += y * y;
            }
            let det = ss * cc - sc * sc;
            let (a, b) = ((ys * cc - yc * sc) / det, (yc * ss - ys * sc) / det);
            let fitted = a * ys + b * yc;
            let rms = (yy / w.len() as f64).sqrt();
            let residual = ((yy - fitted).max(0.0) / yy.max(1e-12)).sqrt();
            (i as f64 * 0.01, rms as f32, residual as f32)
        })
        .collect()
}

/// A speed drop at a warp marker keeps the sound: before, the stretcher got no input for a
/// few calls after the drop and froze its sound, a 60-200 ms dip (#165).
#[test]
fn a_speed_drop_keeps_the_level() {
    for slower in [1.0, 0.5] {
        let windows = seam(slower);
        let level = |from: f64, to: f64| {
            let w: Vec<f32> = windows.iter().filter(|(t, _, _)| (from..to).contains(t)).map(|w| w.1).collect();
            w.iter().sum::<f32>() / w.len() as f32
        };
        let (before, after) = (level(1.0, 1.9), level(2.5, 3.9));
        for &(t, rms, residual) in windows.iter().filter(|(t, _, _)| (1.0..3.9).contains(t)) {
            assert!(rms > 0.85 * before.min(after) && rms < 1.15 * before.max(after), "{slower}x at {t:.2} s: {rms}");
            assert!(residual < 0.05, "{slower}x at {t:.2} s: {residual} off a clean sine");
        }
    }
}

/// A looping clip that needs more than `MAX_PIECES` pieces keeps looping where its playlist
/// was cut, instead of running on through the file and going silent (#167).
#[test]
fn a_loop_past_the_piece_cap_keeps_looping() {
    let data = sine(330.0, 2.0, 8000);
    // A half-beat loop of five 0.1-beat segments at alternating speeds: 5 pieces a loop, so a
    // 30 000-beat clip (60 000 loops) needs 300 000 pieces, three times the cap.
    let markers: Vec<(f64, f64)> = (0..=10)
        .scan(0.0, |seconds, i| {
            let here = (f64::from(i) * 0.1, *seconds);
            *seconds += 0.05 * if i % 2 == 0 { 1.2 } else { 0.8 };
            Some(here)
        })
        .collect();
    let r = region(&markers, StretchMode::Smooth);
    let c = clip(30_000.0, 0.0, Some((0.0, 0.5)));
    let (pieces, _, _) = playlist(&with(&c, &r), &data, 120.0).unwrap();
    assert!(pieces.len() <= MAX_PIECES);
    // At 120 bpm a beat is 24 000 project frames. Beats 1 000 and 25 000 are both at the
    // loop's start, the second far past where the playlist was cut; and playing on across
    // the cut sounds like playing on across any other loop start.
    let last = pieces.last().unwrap();
    let cut = (last.out_start + (last.src_end - last.src_start) / 8000.0 / last.rate) * 2.0;
    assert!(cut > 1_000.0 && cut < 30_000.0 && (cut * 2.0 - (cut * 2.0).round()).abs() < 1e-6, "{cut}");
    let render_from = |beat: f64| {
        let mut voice = StretchVoice::new(&with(&c, &r), data.clone(), 120.0, 48_000).unwrap().prepared();
        voice.seek((beat * 24_000.0).round() as u64);
        let mut out = vec![0.0f32; 2 * 24_000];
        voice.render(&mut out);
        out
    };
    let level = |s: &[f32]| (s.iter().map(|x| x * x).sum::<f32>() / s.len() as f32).sqrt();
    let same = |a: &[f32], b: &[f32]| {
        let diff: Vec<f32> = a.iter().zip(b).map(|(x, y)| x - y).collect();
        assert!(level(a) > 0.1, "{}", level(a));
        assert!(level(&diff) < 0.01 * level(a), "{} vs {}, apart by {}", level(b), level(a), level(&diff));
    };
    same(&render_from(1_000.0), &render_from(25_000.0));
    same(&render_from(1_000.25), &render_from(cut - 0.25));
}

/// An offset past the loop end starts at the loop start, like the direct reader and the
/// notes, instead of playing what lies past the loop first (#166).
#[test]
fn an_offset_past_the_loop_end_starts_at_the_loop_start() {
    let data = sine(440.0, 4.0, 1000);
    let r = region(&[(0.0, 0.0), (4.0, 2.0), (6.0, 4.0)], StretchMode::Smooth);
    let (pieces, _, _) = playlist(&with(&clip(4.0, 7.0, Some((2.0, 6.0))), &r), &data, 120.0).unwrap();
    let summary: Vec<(f64, f64, f64)> = pieces.iter().map(|p| (p.out_start, p.src_start, p.src_end)).collect();
    assert_eq!(summary, [(0.0, 1000.0, 2000.0), (1.0, 2000.0, 4000.0)]);
}

/// A grain that runs on past a loop pass's end reads the next pass from the loop's start, as the
/// spectral stretcher's input does, never the file past the loop.
#[test]
fn grains_read_the_next_loop_pass_past_a_loop_end() {
    // A ramp at 8 kHz (the least rate a voice takes), each frame its own number, longer than the
    // loop's end in the file. Beats 0–4 are 0–2 s, beats 4–6 2–4 s; loop beats 2–6.
    const SR: u32 = 8_000;
    let samples = (0..5 * SR as usize).map(|i| i as f32 / 100_000.0).collect();
    let data = Arc::new(AudioData { sample_rate: SR, channels: 1, samples });
    let r = region(&[(0.0, 0.0), (4.0, 2.0), (6.0, 4.0)], StretchMode::Texture);
    let frame = |f: f64| f / 100_000.0;
    // Eight beats from beat 3: the loop is passed and comes round again.
    let Some(voice) = StretchVoice::new(&with(&clip(8.0, 3.0, Some((2.0, 6.0))), &r), Arc::clone(&data), 120.0, SR)
    else {
        panic!("no voice")
    };
    // The stream (as `playlist_follows_markers_and_loops` lays it out, at 8 kHz): its frame
    // 19 999 is the file's 31 999, the loop's last; frame 20 000 is the loop's start again, file
    // frame 8 000.
    assert_eq!(voice.stream_frame(19_999.0, 0), frame(31_999.0));
    assert_eq!(voice.stream_frame(20_000.0, 0), frame(8_000.0), "the next pass, not file frame 32 000");
    let between = voice.stream_sample(19_999.5, 0);
    assert!((between - (frame(31_999.0) + frame(8_000.0)) / 2.0).abs() < 1e-6, "{between}");
    // A clip that ends in its first loop pass (two beats from beat 3, in a loop of beats 2–6): past
    // the last piece the stream is silent, not the file past the loop.
    let Some(short) = StretchVoice::new(&with(&clip(2.0, 3.0, Some((2.0, 6.0))), &r), Arc::clone(&data), 120.0, SR)
    else {
        panic!("no voice")
    };
    let end = short.pieces.last().map_or(0.0, |p| p.stream_start + (p.src_end - p.src_start));
    assert!(short.stream_frame(end - 1.0, 0) > 0.0, "the clip's last frame sounds");
    assert_eq!(short.stream_frame(end + 10.0, 0), 0.0, "past the last pass, silence");
}

/// Where a Rhythmic slice must stop: the end of the stretch of file the clip's time plays in, past
/// the warp segments that follow on in the file, at the loop's end; unbounded for a clip that
/// neither loops nor repeats.
#[test]
fn slices_stop_at_the_end_of_the_stretch_the_clip_plays_in() {
    const SR: u32 = 8_000;
    let data = Arc::new(AudioData { sample_rate: SR, channels: 1, samples: vec![0.0; 5 * SR as usize] });
    let r = region(&[(0.0, 0.0), (4.0, 2.0), (6.0, 4.0)], StretchMode::Rhythmic);
    // Eight beats from beat 3, loop beats 2–6: beats 3–4 (file 12 000–16 000) and 4–6 (16 000–
    // 32 000) follow on in the file, and the loop jumps back after file frame 32 000.
    let Some(looped) = StretchVoice::new(&with(&clip(8.0, 3.0, Some((2.0, 6.0))), &r), Arc::clone(&data), 120.0, SR)
    else {
        panic!("no voice")
    };
    assert_eq!(looped.run_end(0), 32_000.0, "beat 3: on through beat 4 to the loop's end");
    assert_eq!(looped.run_end(u64::from(SR)), 32_000.0, "beat 5");
    // The second pass (from 1.5 s of output, the loop's start again) ends at the loop's end too.
    assert_eq!(looped.run_end(u64::from(SR) * 2), 32_000.0);
    let Some(plain) = StretchVoice::new(&with(&clip(4.0, 0.0, None), &r), data, 120.0, SR) else { panic!("no voice") };
    assert_eq!(plain.run_end(0), f64::INFINITY);
}

/// A position that is not finite (an extreme transpose can make one infinite) reads silence,
/// never NaN: `at - at.floor()` of infinity is NaN, which reached the stretcher's output.
#[test]
fn a_position_that_is_not_finite_reads_silence() {
    let samples = (0..5_000).map(|i| i as f32 / 10_000.0).collect();
    let data = Arc::new(AudioData { sample_rate: 1000, channels: 1, samples });
    let r = region(&[(0.0, 0.0), (4.0, 2.0), (6.0, 4.0)], StretchMode::Texture);
    let Some(voice) = StretchVoice::new(&with(&clip(4.0, 0.0, None), &r), data, 120.0, 1000) else {
        panic!("no voice")
    };
    for at in [f64::INFINITY, f64::NEG_INFINITY, f64::NAN] {
        assert_eq!(voice.sample_at(at, 0), 0.0, "{at}");
        assert_eq!(voice.stream_sample(at, 0), 0.0, "{at}");
        assert_eq!(voice.stream_frame(at, 0), 0.0, "{at}");
    }
}

/// Clicks every `every` seconds: a 2 ms burst at full scale, silence between.
fn clicks(every: f64, seconds: f64, sample_rate: u32) -> Arc<AudioData> {
    let frames = (seconds * f64::from(sample_rate)) as usize;
    let period = (every * f64::from(sample_rate)) as usize;
    let burst = (0.002 * f64::from(sample_rate)) as usize;
    let samples = (0..frames).map(|i| if i % period < burst { 0.9 } else { 0.0 }).collect();
    Arc::new(AudioData { sample_rate, channels: 1, samples })
}

/// Onset times (seconds) in interleaved stereo: where the level first rises above a threshold
/// after at least 50 ms below it.
fn onsets(stereo: &[f32], sample_rate: u32) -> Vec<f64> {
    let quiet_needed = (0.05 * f64::from(sample_rate)) as usize;
    let mut quiet = quiet_needed;
    let mut found = Vec::new();
    for (i, frame) in stereo.as_chunks::<2>().0.iter().enumerate() {
        if frame[0].abs() > 0.2 {
            if quiet >= quiet_needed {
                found.push(i as f64 / f64::from(sample_rate));
            }
            quiet = 0;
        } else {
            quiet += 1;
        }
    }
    found
}

/// Renders a warped click clip (transposed by `transpose`, from `seek` seconds into it) and
/// returns how far each click lands from where the warp markers put it, in milliseconds.
fn click_errors(markers: &[(f64, f64)], length_beats: f64, seek: f64, transpose: f64) -> Vec<f64> {
    let sr = 48_000;
    let data = clicks(0.5, 40.0, sr);
    let r = ClipAudio { transpose, ..region(markers, StretchMode::Smooth) };
    let c = clip(length_beats, 0.0, None);
    let mut voice = StretchVoice::new(&with(&c, &r), data, 120.0, sr).unwrap().prepared();
    let skipped = (seek * f64::from(sr)) as usize;
    if skipped > 0 {
        voice.seek(skipped as u64);
    }
    let frames = (length_beats * 0.5 * f64::from(sr)) as usize - skipped;
    let mut out = vec![0.0; frames * 2];
    for block in out.chunks_mut(512 * 2) {
        voice.render(block);
    }
    // Where each click should play: file time → beat through the markers → seconds at 120 bpm.
    let beat_of = |seconds: f64| {
        let i = markers.iter().position(|m| m.1 > seconds).unwrap_or(markers.len()).clamp(1, markers.len() - 1);
        let (a, b) = (markers[i - 1], markers[i]);
        a.0 + (seconds - a.1) * (b.0 - a.0) / (b.1 - a.1)
    };
    onsets(&out, sr)
        .iter()
        .map(|&t| t + seek)
        .map(|t| {
            // The nearest click in the file to this onset.
            let expected = (0..80)
                .map(|k| beat_of(k as f64 * 0.5) * 0.5)
                .fold(f64::INFINITY, |best, e| if (e - t).abs() < (best - t).abs() { e } else { best });
            (t - expected) * 1000.0
        })
        .collect()
}

fn worst(errors: &[f64]) -> f64 {
    errors.iter().fold(0.0f64, |w, x| w.max(x.abs()))
}

const SEGMENTS: [(f64, f64); 6] = [(0.0, 0.0), (8.0, 3.9), (16.0, 8.1), (24.0, 11.9), (32.0, 16.2), (48.0, 23.8)];

#[test]
fn transients_stay_on_the_grid() {
    for (markers, limit) in [
        (vec![(0.0, 0.0), (64.0, 30.4)], 3.0), // 0.95x
        (vec![(0.0, 0.0), (64.0, 33.6)], 3.0), // 1.05x
        (vec![(0.0, 0.0), (64.0, 22.4)], 4.0), // 0.7x
        (vec![(0.0, 0.0), (64.0, 48.0)], 4.0), // 1.5x
        (SEGMENTS.to_vec(), 3.0),              // changing speed
    ] {
        let errors = click_errors(&markers, 46.0, 0.0, 0.0);
        assert!(errors.len() >= 30, "{markers:?}: {} clicks found", errors.len());
        assert!(worst(&errors) < limit, "{markers:?}: {errors:?}");
    }
}

/// Clicks stay on the grid after a speed drop: feeding the stretcher at least some input
/// after the drop must not leave the input ahead for good (#165).
#[test]
fn transients_stay_on_the_grid_after_a_speed_drop() {
    // 16 beats at 2x (16 s of file in 8 s), then 24 beats at 1x or 0.5x.
    for markers in [vec![(0.0, 0.0), (16.0, 16.0), (40.0, 28.0)], vec![(0.0, 0.0), (16.0, 16.0), (40.0, 22.0)]] {
        let errors = click_errors(&markers, 40.0, 0.0, 0.0);
        // At 0.5x a stretched click can show a second onset 120 ms in (as before #165): only
        // the clicks themselves are timed here.
        let clicks: Vec<f64> = errors.iter().copied().filter(|e| e.abs() < 50.0).collect();
        assert!(clicks.len() >= 45, "{markers:?}: {} clicks found", clicks.len());
        assert!(worst(&clicks) < 4.0, "{markers:?}: {errors:?}");
    }
}

#[test]
fn a_voice_can_start_anywhere_in_its_clip() {
    // Seek into the middle of a beat, of a speed segment, and just past a segment change.
    for seek in [3.3, 9.87, 16.01] {
        let errors = click_errors(&SEGMENTS, 46.0, seek, 0.0);
        assert!(errors.len() >= 10, "seek {seek}: {} clicks found", errors.len());
        assert!(worst(&errors) < 4.0, "seek {seek}: {errors:?}");
    }
}

#[test]
fn transposed_clips_keep_transients_on_the_grid() {
    for transpose in [-7.0, -3.0, 2.0, 7.0] {
        let errors = click_errors(&[(0.0, 0.0), (64.0, 30.4)], 46.0, 0.0, transpose);
        assert!(errors.len() >= 40, "{transpose} st: {} clicks found", errors.len());
        assert!(worst(&errors) < 4.0, "{transpose} st: {errors:?}");
    }
}

/// Renders a warped sine clip and returns (frequency in Hz, frames with sound) at 48 kHz.
fn transposed(hz: f64, markers: &[(f64, f64)], beats: f64, transpose: f64) -> (f64, usize) {
    let sr = 48_000;
    let data = sine(hz, 20.0, 44_100);
    let r = ClipAudio { transpose, ..region(markers, StretchMode::Smooth) };
    let mut voice = StretchVoice::new(&with(&clip(beats, 0.0, None), &r), data, 120.0, sr).unwrap().prepared();
    let frames = (beats * 0.5 * f64::from(sr)) as usize;
    let mut out = vec![0.0; frames * 2];
    for block in out.chunks_mut(512 * 2) {
        voice.render(block);
    }
    let left: Vec<f32> = out.iter().step_by(2).copied().collect();
    let middle = &left[left.len() / 4..left.len() * 3 / 4];
    let ups = middle.windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count();
    let sounding = left.iter().rposition(|s| s.abs() > 0.05).map_or(0, |i| i + 1);
    (ups as f64 / (middle.len() as f64 / f64::from(sr)), sounding)
}

#[test]
fn transpose_shifts_pitch_but_not_time_on_warped_clips() {
    // 8 beats at 120 bpm = 4 s, the file playing at its own speed, then at 0.9x.
    for (markers, transpose, expected) in [
        (vec![(0.0, 0.0), (8.0, 4.0)], 12.0, 880.0),
        (vec![(0.0, 0.0), (8.0, 4.0)], -5.0, 440.0 * 2f64.powf(-5.0 / 12.0)),
        (vec![(0.0, 0.0), (8.0, 3.6)], 7.0, 440.0 * 2f64.powf(7.0 / 12.0)),
        (vec![(0.0, 0.0), (8.0, 3.6)], 0.0, 440.0),
    ] {
        let (hz, sounding) = transposed(440.0, &markers, 8.0, transpose);
        assert!((hz - expected).abs() < expected * 0.01, "{transpose} st: {hz:.1} Hz, expected {expected:.1}");
        // The clip still fills its 4 s.
        assert!(sounding > 4 * 48_000 - 2_000, "{transpose} st: sound ends at frame {sounding}");
    }
}

#[test]
fn plays_through_at_any_device_rate_and_block_size() {
    // A 48 kHz file on 44.1 kHz and 96 kHz devices, in odd block sizes.
    let data = clicks(0.25, 40.0, 48_000);
    for markers in [vec![(0.0, 0.0), (64.0, 30.4)], vec![(0.0, 0.0), (64.0, 45.0)]] {
        let r = region(&markers, StretchMode::Smooth);
        for (device_rate, block) in [(44_100, 441), (44_100, 448), (96_000, 37), (22_050, 1000)] {
            let mut voice = StretchVoice::new(&with(&clip(60.0, 0.0, None), &r), data.clone(), 120.0, device_rate)
                .unwrap()
                .prepared();
            let mut out = vec![0.0; block * 2];
            for _ in 0..(30 * device_rate as usize / block) {
                voice.render(&mut out);
            }
        }
    }
}

#[test]
fn unusable_clips_get_no_voice() {
    let data = sine(440.0, 1.0, 48_000);
    let one_marker = region(&[(0.0, 0.0)], StretchMode::Smooth);
    assert!(StretchVoice::new(&with(&clip(1.0, 0.0, None), &one_marker), data.clone(), 120.0, 48_000).is_none());
    let unity = region(&[(0.0, 0.0), (2.0, 1.0)], StretchMode::Smooth);
    assert!(StretchVoice::new(&with(&clip(1.0, 0.0, None), &unity), data.clone(), 120.0, 48_000).is_none());
    let repitch = region(&[(0.0, 0.0), (2.0, 0.9)], StretchMode::Repitch);
    assert!(StretchVoice::new(&with(&clip(1.0, 0.0, None), &repitch), data.clone(), 120.0, 48_000).is_none());
    // 1 beat holding 0.005 s at 120 bpm: 0.01x speed, slower than the stretcher is given.
    let extreme = region(&[(0.0, 0.0), (1.0, 0.005)], StretchMode::Smooth);
    assert!(StretchVoice::new(&with(&clip(1.0, 0.0, None), &extreme), data, 120.0, 48_000).is_none());
}

/// The frequency of `left` (a mono signal) between `from` and `to` samples, from its zero
/// crossings.
fn crossing_hz(left: &[f32], from: usize, to: usize, rate: u32) -> f64 {
    let part = left.get(from..to).unwrap_or_default();
    let crossings = part.windows(2).filter(|w| (w[0] < 0.0) != (w[1] < 0.0)).count();
    crossings as f64 / 2.0 / ((to - from) as f64 / f64::from(rate))
}

/// A segment squeezed or stretched far past what a narrower range allowed keeps its pitch: a
/// stretching mode never re-pitches a clip because one warp marker was dragged close to another.
#[test]
fn a_segment_far_faster_or_slower_than_its_audio_keeps_its_pitch() {
    let rate = 48_000;
    // 4 beats (2 s at 120 BPM) of 20 s of audio is 10x; of 0.1 s it is 1/20x.
    for (seconds, label) in [(20.0_f64, "10x"), (0.1, "1/20x")] {
        let data = sine(440.0, seconds.max(1.0), rate);
        let r = region(&[(0.0, 0.0), (4.0, seconds)], StretchMode::Smooth);
        let c = clip(4.0, 0.0, None);
        let voice = StretchVoice::new(&with(&c, &r), data, 120.0, rate);
        assert!(voice.is_some(), "{label}: stretched, not re-pitched");
        let Some(mut voice) = voice.map(StretchVoice::prepared) else { continue };
        let mut out = vec![0.0f32; 2 * 2 * rate as usize];
        for block in out.chunks_mut(2 * 512) {
            voice.render(block);
        }
        let left: Vec<f32> = out.as_chunks::<2>().0.iter().map(|[l, _]| *l).collect();
        let hz = crossing_hz(&left, rate as usize / 4, rate as usize * 3 / 2, rate);
        assert!((hz - 440.0).abs() < 440.0 * 0.06, "{label}: {hz} Hz, not 440");
    }
}

/// Rhythmic keeps a beat as a cut when no cut is within half a beat of it, half a beat measured
/// from that beat's own neighbours: uneven warp markers squeeze some beats close together in the
/// file, and a mean spacing hid the beat between two close ones (#1540).
#[test]
fn a_beat_between_close_markers_is_still_cut() {
    // Markers at (beat, frame) (0, 0), (2, 0.2) and (4, 2.2): the beats fall at these frames.
    let fill = [0.0, 0.1, 0.2, 1.2, 2.2];
    // The markers are cuts; no transients.
    let near = [0.0, 0.2, 2.2];
    assert_eq!(beats_without_a_cut(&fill, &near, f64::INFINITY), vec![0.1, 1.2], "the beat at 0.1 is not hidden by 0");
    // An even grid keeps its behavior: a cut within half a beat hides the beat, one further out
    // does not.
    let even = [0.0, 1.0, 2.0, 3.0];
    assert_eq!(beats_without_a_cut(&even, &[1.4], f64::INFINITY), vec![0.0, 2.0, 3.0]);
    assert_eq!(beats_without_a_cut(&even, &[1.6], f64::INFINITY), vec![0.0, 1.0, 3.0]);
    assert_eq!(beats_without_a_cut(&even, &[], f64::INFINITY), even.to_vec());
}

/// A lone beat (the file holds one) has no neighbours to measure half a beat from: it measures
/// from the warp map's beat length there, so a far cut no longer hides it.
#[test]
fn a_lone_beat_measures_half_a_beat_from_the_warp_map() {
    // One beat at frame 5, four frames long: a cut 5 frames off is past half a beat, 1 is not.
    assert_eq!(beats_without_a_cut(&[5.0], &[0.0], 4.0), vec![5.0]);
    assert_eq!(beats_without_a_cut(&[5.0], &[4.0], 4.0), Vec::<f64>::new());
    // Half a second of a file at two beats a second: one beat on its grid, its length from
    // the map.
    let markers = [(0.0, 0.25), (4.0, 2.25)];
    let fill = grid_frames(&markers, 1.0, (0.0, 0.5), 0.5, 1000.0);
    assert_eq!(fill, vec![250.0]);
    assert_eq!(beat_frames_at(&markers, 250.0, 1000.0), 500.0);
}

/// The grid of a short clip far into a long file covers where it plays: a grid from the file's
/// start stopped at the cap (25 000 beats at 1/32) long before it got there.
#[test]
fn a_short_clip_far_into_a_long_file_gets_its_note_cuts() {
    // 120 bpm: two beats a second, over a file of 20 000 s (40 000 beats).
    let markers = [(0.0, 0.0), (2.0, 1.0)];
    let (every, seconds, rate) = (0.125, 20_000.0, 1000.0);
    let whole = grid_frames(&markers, every, (f64::NAN, f64::NAN), seconds, rate);
    assert_eq!(whole.len(), MAX_CUTS, "the whole file is capped");
    assert!(whole.last().is_some_and(|f| *f < 19_000.0 * rate), "and never gets to 19 000 s");
    let reached = grid_frames(&markers, every, (19_000.0, 19_001.0), seconds, rate);
    // Every 1/32 note of the second played, and two on either side.
    assert_eq!(reached.len(), 16 + 1 + 4);
    assert_eq!(reached.first().copied(), Some(19_000.0 * rate - 2.0 * 62.5));
    assert_eq!(reached.last().copied(), Some(19_001.0 * rate + 2.0 * 62.5));
}

/// A Rhythmic clip whose loop is shorter than the slice it sits in plays the slice's start on
/// every pass, with each tail: the clip's time jumping back to the loop's start starts the slice
/// again, though it is the same slice (#1540).
#[test]
fn a_rhythmic_loop_inside_one_slice_plays_its_start_on_every_pass() {
    // At 8 kHz, a click on every beat of the file: beats 0–8 are 0–2 s (a beat is 2 000 file
    // frames), played at 120 bpm, so at half the file's speed. Slices on every quarter note (the
    // warp markers at frames 0 and 16 000, and every beat), with no transients: the slice at beat
    // 2 runs from file frame 4 000 to 6 000. The loop, beats 2–2.5, is file frames 4 000–5 000:
    // no slice starts inside it, and its first frame is a click.
    const SR: u32 = 8_000;
    let data = clicks(0.25, 3.0, SR);
    for tail in [SliceTail::Off, SliceTail::Loop, SliceTail::BackAndForth] {
        let r = ClipAudio {
            warp: WarpSettings { slice: SliceAt::Quarter, tail, ..WarpSettings::default() },
            ..region(&[(0.0, 0.0), (8.0, 2.0)], StretchMode::Rhythmic)
        };
        // Eight passes of half a beat (0.25 s each), from the loop's start.
        let c = clip(4.0, 2.0, Some((2.0, 2.5)));
        let mut voice = StretchVoice::new(&with(&c, &r), Arc::clone(&data), 120.0, SR).unwrap().prepared();
        let mut out = vec![0.0f32; 2 * 2 * SR as usize];
        for block in out.chunks_mut(2 * 500) {
            voice.render(block);
        }
        let found = onsets(&out, SR);
        assert_eq!(found.len(), 8, "{tail:?}: {found:?}");
        for (k, at) in found.iter().enumerate() {
            let pass = k as f64 * 0.25;
            assert!((at - pass).abs() < 0.003, "{tail:?}: pass {k} at {pass} s, a click at {at} s: {found:?}");
        }
    }
}

/// A looped Texture clip on its last pass reads nothing past the loop: a grain that runs on past
/// the clip's end reads silence, where reading the file played what follows the loop.
#[test]
fn a_looped_texture_clip_reads_nothing_past_the_loop_on_its_last_pass() {
    // At 8 kHz, a file silent up to the loop's end (frame 8 000, 1 s) and loud after it. Beats 0–8
    // are 0–2 s, played at 120 bpm: half the file's speed, so each 60 ms grain reads ahead of the
    // clip's time, past a pass's end in its last moments.
    const SR: u32 = 8_000;
    let samples = (0..3 * SR as usize).map(|i| if i < SR as usize { 0.0 } else { 0.5 }).collect();
    let data = Arc::new(AudioData { sample_rate: SR, channels: 1, samples });
    let r = region(&[(0.0, 0.0), (8.0, 2.0)], StretchMode::Texture);
    let render = |c: &Clip, frames: usize| {
        let mut voice = StretchVoice::new(&with(c, &r), Arc::clone(&data), 120.0, SR).unwrap().prepared();
        let mut out = vec![0.0f32; 2 * frames];
        for block in out.chunks_mut(2 * 500) {
            voice.render(block);
        }
        out
    };
    let peak = |s: &[f32]| s.iter().fold(0.0f32, |m, x| m.max(x.abs()));
    // Loop beats 2–4 (file frames 4 000–8 000), two passes of 1 s from the loop's start, rendered
    // to the clip's end: the first pass's grains go on into the second pass, the last pass's into
    // silence.
    let looped = render(&clip(4.0, 2.0, Some((2.0, 4.0))), 2 * SR as usize);
    assert!(peak(&looped) < 1e-6, "{}", peak(&looped));
    // The same beats without a loop read on into the file, so the file past the loop is heard at
    // the end: the render above would hear it too if its grains read the file.
    let plain = render(&clip(2.0, 2.0, None), SR as usize);
    let tail = plain.get(plain.len() - 2 * 480..).unwrap_or_default();
    assert!(peak(tail) > 0.05, "{}", peak(tail));
}
