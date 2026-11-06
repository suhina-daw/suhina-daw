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

fn region(markers: &[(f64, f64)], mode: StretchMode) -> AudioRegion {
    AudioRegion {
        warped: true,
        stretch: mode,
        warp_markers: markers.iter().map(|&(time, audio_time)| WarpMarker { time, audio_time }).collect(),
        ..AudioRegion::new("sine.wav")
    }
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
    let (pieces, _, _) = playlist(&clip(8.0, 3.0, Some((2.0, 6.0))), &r, &data, 120.0).unwrap();
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
    let (pieces, _, _) = playlist(&c, &r, &data, 120.0).unwrap();
    assert_eq!(pieces.len(), 40_000);
    let mut voice = StretchVoice::new(&c, &r, data, 120.0, 48_000).unwrap().prepared();
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
    let mut voice = StretchVoice::new(&c, &r, data, 120.0, rate).unwrap().prepared();
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
    let (pieces, _, _) = playlist(&c, &r, &data, 120.0).unwrap();
    assert!(pieces.len() <= MAX_PIECES);
    // At 120 bpm a beat is 24 000 project frames. Beats 1 000 and 25 000 are both at the
    // loop's start, the second far past where the playlist was cut; and playing on across
    // the cut sounds like playing on across any other loop start.
    let last = pieces.last().unwrap();
    let cut = (last.out_start + (last.src_end - last.src_start) / 8000.0 / last.rate) * 2.0;
    assert!(cut > 1_000.0 && cut < 30_000.0 && (cut * 2.0 - (cut * 2.0).round()).abs() < 1e-6, "{cut}");
    let render_from = |beat: f64| {
        let mut voice = StretchVoice::new(&c, &r, data.clone(), 120.0, 48_000).unwrap().prepared();
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
    let (pieces, _, _) = playlist(&clip(4.0, 7.0, Some((2.0, 6.0))), &r, &data, 120.0).unwrap();
    let summary: Vec<(f64, f64, f64)> = pieces.iter().map(|p| (p.out_start, p.src_start, p.src_end)).collect();
    assert_eq!(summary, [(0.0, 1000.0, 2000.0), (1.0, 2000.0, 4000.0)]);
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
    let r = AudioRegion { transpose, ..region(markers, StretchMode::Smooth) };
    let c = clip(length_beats, 0.0, None);
    let mut voice = StretchVoice::new(&c, &r, data, 120.0, sr).unwrap().prepared();
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
    let r = AudioRegion { transpose, ..region(markers, StretchMode::Smooth) };
    let mut voice = StretchVoice::new(&clip(beats, 0.0, None), &r, data, 120.0, sr).unwrap().prepared();
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
            let mut voice =
                StretchVoice::new(&clip(60.0, 0.0, None), &r, data.clone(), 120.0, device_rate).unwrap().prepared();
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
    assert!(StretchVoice::new(&clip(1.0, 0.0, None), &one_marker, data.clone(), 120.0, 48_000).is_none());
    let unity = region(&[(0.0, 0.0), (2.0, 1.0)], StretchMode::Smooth);
    assert!(StretchVoice::new(&clip(1.0, 0.0, None), &unity, data.clone(), 120.0, 48_000).is_none());
    let repitch = region(&[(0.0, 0.0), (2.0, 0.9)], StretchMode::Repitch);
    assert!(StretchVoice::new(&clip(1.0, 0.0, None), &repitch, data.clone(), 120.0, 48_000).is_none());
    // 1 beat holding 0.02 s at 120 bpm: 0.04x speed, too slow to stretch.
    let extreme = region(&[(0.0, 0.0), (1.0, 0.02)], StretchMode::Smooth);
    assert!(StretchVoice::new(&clip(1.0, 0.0, None), &extreme, data, 120.0, 48_000).is_none());
}
