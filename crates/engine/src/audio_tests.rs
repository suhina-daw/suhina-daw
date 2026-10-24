// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
use suhina_model::*;

const SR: u32 = 48_000;

/// One second of a ramp, so the sample value tells the file position: value = seconds.
fn ramp() -> Arc<AudioData> {
    Arc::new(AudioData { sample_rate: 8000, channels: 1, samples: (0..8000).map(|i| i as f32 / 8000.0).collect() })
}

fn region(warped: bool, markers: &[(f64, f64)], transpose: f64) -> AudioRegion {
    AudioRegion {
        sample_rate: Some(8000),
        channels: Some(1),
        duration_seconds: Some(1.0),
        warped,
        warp_markers: markers.iter().map(|&(time, audio_time)| WarpMarker { time, audio_time }).collect(),
        transpose,
        ..AudioRegion::new("ramp.wav")
    }
}

fn clip(length: f64, offset: f64, loop_range: Option<(f64, f64)>) -> Clip {
    Clip {
        offset,
        loop_range: loop_range.map(|(start, end)| LoopRange { start, end }),
        ..Clip::new(ContentId(1), 0.0, length)
    }
}

/// File position (seconds) played at `beat` in the clip, at 120 bpm (0.5 s per beat).
fn file_seconds_at(event: &AudioEvent, beat: f64) -> f32 {
    event.sample((beat * event.frames_per_beat) as u64).0
}

fn event(clip: &Clip, region: &AudioRegion) -> AudioEvent {
    AudioEvent::new(
        clip,
        region,
        ramp(),
        Timing { frames_per_beat: 60.0 / 120.0 * f64::from(SR), tempo_bpm: 120.0, sample_rate: SR, map: None },
        (1.0, 1.0),
    )
    .unwrap()
}

#[test]
fn fades_are_fitted_to_the_clip_and_meet_the_click_guard() {
    // 4 beats of fades on a 2-beat clip are fitted to 1.5 + 0.5 beats, at 100 frames a beat.
    let c = Clip { fade_in: 3.0, fade_out: 1.0, ..clip(2.0, 0.0, None) };
    let fades = Fades::new(&c, 100.0, 1000);
    assert_eq!((fades.fade_in, fades.fade_out), (150.0, 50.0));
    assert_eq!(fades.click, 2.0);
    // Half-way into the fade-in; the guard is over after two frames.
    assert!((fades.gain(75.0, 125.0) - 0.5).abs() < 1e-6);
    assert!((fades.gain(1.0, 199.0) - 0.5 / 150.0).abs() < 1e-6, "guard and fade multiply");
    assert!((fades.gain(160.0, 40.0) - 0.78).abs() < 1e-6, "39 frames after this one, of 50");
    assert_eq!(fades.gain(100.0, 1.0), 0.0, "the last frame");
    assert_eq!(fades.gain(0.0, 200.0), 0.0);
    let c = Clip { fade_in: f64::NAN, fade_out: -1.0, ..clip(2.0, 0.0, None) };
    assert_eq!(Fades::new(&c, 100.0, 1000).gain(100.0, 100.0), 1.0, "bad fades are no fades");
}

/// The direct reader refuses a tempo of 0 and a file sample rate outside 1 to 384 kHz, as the
/// stretcher refuses them (#164).
#[test]
fn a_zero_tempo_or_an_unplayable_sample_rate_is_no_event() {
    let timing = |tempo_bpm: f64| Timing { frames_per_beat: 24_000.0, tempo_bpm, sample_rate: SR, map: None };
    let c = clip(2.0, 0.0, None);
    assert!(AudioEvent::new(&c, &region(false, &[], 0.0), ramp(), timing(0.0), (1.0, 1.0)).is_none());
    assert!(AudioEvent::new(&c, &region(false, &[], 0.0), ramp(), timing(-120.0), (1.0, 1.0)).is_none());
    for rate in [0, 999, 384_001, 1_000_000] {
        let data = Arc::new(AudioData { sample_rate: rate, ..(*ramp()).clone() });
        assert!(AudioEvent::new(&c, &region(false, &[], 0.0), data, timing(120.0), (1.0, 1.0)).is_none(), "{rate}");
    }
    assert!(AudioEvent::new(&c, &region(false, &[], 0.0), ramp(), timing(120.0), (1.0, 1.0)).is_some());
}

/// Hostile warp markers and offsets put the read position far past the file (or at
/// infinity): silence, never a panic or a NaN (issue #152). Stereo, where the right
/// channel's index used to overflow.
#[test]
fn a_huge_position_in_a_stereo_file_is_silence() {
    let stereo = Arc::new(AudioData { sample_rate: 1000, channels: 2, samples: vec![0.5; 2000] });
    for position in [1e300, f64::INFINITY, usize::MAX as f64, 1000.0, f64::NAN, -1.0] {
        assert_eq!(stereo.frame_at(position), (0.0, 0.0), "{position}");
    }
    assert_eq!(stereo.frame_at(999.0), (0.5, 0.5));
    let warped = AudioRegion { channels: Some(2), ..region(true, &[(0.0, 0.0), (1.0, 1e300)], 0.0) };
    let e = AudioEvent::new(
        &clip(2.0, 0.0, None),
        &warped,
        stereo,
        Timing { frames_per_beat: 60.0 / 120.0 * f64::from(SR), tempo_bpm: 120.0, sample_rate: SR, map: None },
        (1.0, 1.0),
    )
    .unwrap();
    for frame in [0u64, 1, 24_000, 47_999] {
        let (l, r) = e.sample(frame);
        assert!(l.is_finite() && r.is_finite(), "frame {frame}: {l} {r}");
    }
}

#[test]
fn unwarped_plays_at_file_speed() {
    let e = event(&clip(2.0, 0.0, None), &region(false, &[], 0.0));
    assert!((file_seconds_at(&e, 1.0) - 0.5).abs() < 1e-3);
    assert!((file_seconds_at(&e, 1.5) - 0.75).abs() < 1e-3);
}

/// Without a stretcher a warped clip's pitch follows the tempo, so transpose changes nothing
/// (Repitch mode, and clips the stretcher refuses; #168).
#[test]
fn transpose_leaves_a_warped_clip_read_directly_unchanged() {
    let markers = [(0.0, 0.0), (2.0, 0.75)];
    let plain = event(&clip(2.0, 0.0, None), &region(true, &markers, 0.0));
    let transposed = event(&clip(2.0, 0.0, None), &region(true, &markers, 7.0));
    for beat in [0.0, 0.3, 1.0, 1.7] {
        assert_eq!(file_seconds_at(&plain, beat), file_seconds_at(&transposed, beat), "beat {beat}");
    }
}

/// The binary search finds the same segment as a scan from the start (#162), at, between,
/// before and after markers.
#[test]
fn warp_lookup_matches_a_linear_scan() {
    let linear = |markers: &[(f64, f64)], beat: f64| {
        let i = markers.iter().position(|(b, _)| *b > beat).unwrap_or(markers.len()).clamp(1, markers.len() - 1);
        let ((b0, s0), (b1, s1)) = (markers[i - 1], markers[i]);
        s0 + (beat - b0) * (s1 - s0) / (b1 - b0)
    };
    let markers: Vec<(f64, f64)> = (0..500).map(|i| (f64::from(i) * 0.37, f64::from(i * i) * 0.001)).collect();
    for k in -40..2000 {
        let beat = f64::from(k) * 0.0925;
        assert_eq!(seconds_at(&markers, beat), linear(&markers, beat), "beat {beat}");
    }
    for &(beat, _) in &markers {
        assert_eq!(seconds_at(&markers, beat), linear(&markers, beat), "on the marker at {beat}");
    }
}

/// Markers past the cap are ignored, so no clip placement copies an unbounded list.
#[test]
fn warp_markers_are_capped() {
    let many: Vec<(f64, f64)> = (0..MAX_WARP_MARKERS + 10).map(|i| (i as f64, i as f64 * 0.01)).collect();
    assert_eq!(warp_markers(&region(true, &many, 0.0)).len(), MAX_WARP_MARKERS);
}

#[test]
fn transpose_speeds_up_unwarped_clips() {
    let e = event(&clip(2.0, 0.0, None), &region(false, &[], 12.0));
    assert!((file_seconds_at(&e, 0.5) - 0.5).abs() < 1e-3, "an octave up plays twice as fast");
}

#[test]
fn warp_markers_map_beats_to_file_time() {
    // Beat 0 at 0 s, beat 2 at 0.5 s: the file plays at half speed against 120 bpm.
    let e = event(&clip(2.0, 0.0, None), &region(true, &[(0.0, 0.0), (2.0, 0.5)], 12.0));
    assert!((file_seconds_at(&e, 1.0) - 0.25).abs() < 1e-3);
    // Past the last marker the last segment continues.
    let e = event(&clip(4.0, 0.0, None), &region(true, &[(0.0, 0.0), (2.0, 0.5)], 0.0));
    assert!((file_seconds_at(&e, 3.0) - 0.75).abs() < 1e-3);
}

#[test]
fn offset_and_loop() {
    // Start at clip beat 1 (0.5 s), loop beats 0..1.5 (0..0.75 s).
    let e = event(&clip(4.0, 1.0, Some((0.0, 1.5))), &region(false, &[], 0.0));
    assert!((file_seconds_at(&e, 0.25) - 0.625).abs() < 1e-3);
    // After 0.5 beats the loop end is reached and playback wraps to its start.
    assert!((file_seconds_at(&e, 0.75) - 0.125).abs() < 1e-3);
}

/// An offset past the loop end starts at the loop start, as the notes do (#166).
#[test]
fn an_offset_past_the_loop_end_starts_at_the_loop_start() {
    let e = event(&clip(4.0, 5.0, Some((0.0, 1.5))), &region(false, &[], 0.0));
    assert!((file_seconds_at(&e, 0.25) - 0.125).abs() < 1e-3);
    assert!((file_seconds_at(&e, 1.75) - 0.125).abs() < 1e-3, "and loops from there");
    let at_end = event(&clip(4.0, 1.5, Some((0.0, 1.5))), &region(false, &[], 0.0));
    assert!((file_seconds_at(&at_end, 0.25) - 0.125).abs() < 1e-3);
}

#[test]
fn outside_the_file_is_silent_and_bad_clips_are_rejected() {
    let e = event(&clip(8.0, 0.0, None), &region(false, &[], 0.0));
    assert_eq!(e.sample(e.start + (3.0 * e.frames_per_beat) as u64), (0.0, 0.0));
    let fpb = 24_000.0;
    assert!(
        AudioEvent::new(
            &clip(0.0, 0.0, None),
            &region(false, &[], 0.0),
            ramp(),
            Timing { frames_per_beat: fpb, tempo_bpm: 120.0, sample_rate: SR, map: None },
            (1.0, 1.0)
        )
        .is_none()
    );
    assert!(
        AudioEvent::new(
            &clip(f64::NAN, 0.0, None),
            &region(false, &[], 0.0),
            ramp(),
            Timing { frames_per_beat: fpb, tempo_bpm: 120.0, sample_rate: SR, map: None },
            (1.0, 1.0)
        )
        .is_none()
    );
    let empty = Arc::new(AudioData { sample_rate: 1000, channels: 2, samples: Vec::new() });
    assert!(
        AudioEvent::new(
            &clip(1.0, 0.0, None),
            &region(false, &[], 0.0),
            empty,
            Timing { frames_per_beat: fpb, tempo_bpm: 120.0, sample_rate: SR, map: None },
            (1.0, 1.0)
        )
        .is_none()
    );
}
