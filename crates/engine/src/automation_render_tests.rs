// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Offline renders that prove mixer and built-in device parameters follow their lanes (#395).

use suhina_model::*;

use crate::chain_tests::{BEAT, SR, device, max_diff, note, project};
use crate::render_offline;

fn lane(param: &str, points: &[(f64, f64)]) -> AutomationLane {
    AutomationLane {
        param: param.into(),
        points: points.iter().map(|&(beat, value)| AutomationPoint::new(beat, value)).collect(),
        enabled: true,
    }
}

/// One held note over four beats, with `lanes` on the track's mixer and `devices`.
fn automated(mixer: Vec<AutomationLane>, devices: Vec<Device>) -> Project {
    let mut p = project(vec![note(0.0, 4.0, 69)], 4.0, devices);
    p.channels[0].automation = mixer;
    p
}

fn channels(out: &[f32]) -> (Vec<f32>, Vec<f32>) {
    out.as_chunks::<2>().0.iter().map(|[l, r]| (*l, *r)).unzip()
}

/// The largest difference between `out` and `plain` scaled frame by frame by `gain(beat)`.
fn off_by(out: &[f32], plain: &[f32], gain: impl Fn(f64) -> (f64, f64)) -> f32 {
    let mut worst = 0.0f32;
    for (frame, ([l, r], [pl, pr])) in out.as_chunks::<2>().0.iter().zip(plain.as_chunks::<2>().0).enumerate() {
        let (gl, gr) = gain(frame as f64 / BEAT as f64);
        worst = worst.max((l - pl * gl as f32).abs()).max((r - pr * gr as f32).abs());
    }
    worst
}

#[test]
fn the_volume_follows_its_lane() {
    let plain = render_offline(&automated(vec![], vec![]), SR, 512).unwrap();
    let p = automated(vec![lane("volume", &[(0.0, 0.0), (4.0, 1.0)])], vec![]);
    let out = render_offline(&p, SR, 512).unwrap();
    // Evenly along the fader, from its bottom to 0 dB at 0.85 of it. The sine peaks at a
    // quarter: within half a percent of it everywhere.
    use suhina_model::automation::{Taper, VolumeTaper, shape};
    let fader = |share: f64| VolumeTaper.value(0.85 * share.clamp(0.0, 1.0));
    let worst = off_by(&out[..plain.len()], &plain, |beat| (fader(beat / 4.0), fader(beat / 4.0)));
    assert!(worst < 0.25 * 0.005, "{worst}");
    // Not evenly in gain: a quarter of the way in, the gain is well under a quarter.
    assert!(fader(0.25) < 0.05);
    // A curve bends the rise, in the fader's position.
    let mut bent = p.clone();
    bent.channels[0].automation[0].points[0].curve = -1.0;
    let bent = render_offline(&bent, SR, 512).unwrap();
    // Leaving out the last tenth of a beat, where a full bend rises almost straight up and the
    // gains' steps cut across it.
    let worst = off_by(&bent[..plain.len()], &plain, |beat| {
        let v = fader(shape(beat / 4.0, -1.0));
        if (3.9..4.0).contains(&beat) { (f64::NAN, f64::NAN) } else { (v, v) }
    });
    assert!(worst < 0.25 * 0.01, "{worst}");
}

#[test]
fn the_pan_follows_its_lane() {
    let plain = render_offline(&automated(vec![], vec![]), SR, 512).unwrap();
    let out = render_offline(&automated(vec![lane("pan", &[(0.0, -1.0), (4.0, 1.0)])], vec![]), SR, 512).unwrap();
    let worst = off_by(&out[..plain.len()], &plain, |beat| {
        let pan = (beat / 2.0 - 1.0).min(1.0);
        let angle = (pan + 1.0) * std::f64::consts::FRAC_PI_4;
        (angle.cos() * std::f64::consts::SQRT_2, angle.sin() * std::f64::consts::SQRT_2)
    });
    assert!(worst < 0.25 * 0.005, "{worst}");
    // Hard left at the start, hard right at the end.
    let (l, r) = channels(&out);
    let peak = |s: &[f32]| s.iter().fold(0.0f32, |m, x| m.max(x.abs()));
    assert!(peak(&r[200..600]) < 0.01 && peak(&l[200..600]) > 0.1);
    assert!(peak(&l[BEAT * 4 - 600..BEAT * 4 - 200]) < 0.01);
}

#[test]
fn a_mute_lane_steps_and_overrides_the_channels_mute() {
    let plain = render_offline(&automated(vec![], vec![]), SR, 512).unwrap();
    // Muted on the channel, but the lane says when: sounding, silent from beat 1 to beat 2.
    let mut p = automated(vec![lane("mute", &[(0.0, 0.0), (1.0, 1.0), (2.0, 0.0)])], vec![]);
    p.channels[0].mute = true;
    let out = render_offline(&p, SR, 512).unwrap();
    let gain = |beat: f64| if (1.0..2.0).contains(&beat) { (0.0, 0.0) } else { (1.0, 1.0) };
    // Away from the two switches, which ramp over one step.
    let near = |frame: usize| [BEAT, BEAT * 2].iter().any(|&at| frame.abs_diff(at) <= crate::automation::STEP);
    let mut worst = 0.0f32;
    for (frame, ([l, _], [pl, _])) in out.as_chunks::<2>().0.iter().zip(plain.as_chunks::<2>().0).enumerate() {
        if !near(frame) {
            worst = worst.max((l - pl * gain(frame as f64 / BEAT as f64).0 as f32).abs());
        }
    }
    assert!(worst < 1e-5, "{worst}");
    // Turned off, the lane does not play: the channel's mute holds.
    p.channels[0].automation[0].enabled = false;
    let out = render_offline(&p, SR, 512).unwrap();
    assert!(out.iter().all(|s| s.abs() < 1e-9));
}

#[test]
fn a_device_parameter_follows_its_lane() {
    let plain = render_offline(&automated(vec![], vec![device("utility", &[])]), SR, 512).unwrap();
    // The utility's gain: unity, then a jump to half (-6 dB) at beat 2, then a ramp down to
    // silence (-60 dB is far below the sine) at beat 4.
    let mut utility = device("utility", &[("gain", -20.0)]);
    utility.automation = vec![lane("gain", &[(0.0, 0.0), (2.0, 0.0), (2.0, -6.0206), (3.0, -6.0206)])];
    let out = render_offline(&automated(vec![], vec![utility.clone()]), SR, 512).unwrap();
    // The lane, not the set value (-20 dB), from the first frame; the device smooths a jump
    // over 20 ms.
    let settle = SR as usize / 40;
    let half = |from: usize, to: usize| {
        max_diff(&out[from * 2..to * 2], &plain[from * 2..to * 2].iter().map(|s| s * 0.5).collect::<Vec<_>>())
    };
    assert!(max_diff(&out[..BEAT * 4], &plain[..BEAT * 4]) < 1e-4, "unity before the jump");
    assert!(half(BEAT * 2 + settle, BEAT * 3) < 1e-4, "half after it: {}", half(BEAT * 2 + settle, BEAT * 3));
    assert!(max_diff(&out[BEAT * 4 - 400..BEAT * 4], &plain[BEAT * 4 - 400..BEAT * 4]) < 1e-4);

    // Turned off, the device plays its set value.
    utility.automation[0].enabled = false;
    let set = render_offline(&automated(vec![], vec![utility]), SR, 512).unwrap();
    let tenth: Vec<f32> = plain.iter().map(|s| s * 0.1).collect();
    assert!(max_diff(&set[BEAT * 2..BEAT * 6], &tenth[BEAT * 2..BEAT * 6]) < 1e-4);
}

#[test]
fn automation_sounds_the_same_in_any_block_size() {
    let mut utility = device("utility", &[]);
    utility.automation = vec![lane("gain", &[(0.0, -12.0), (1.3, 3.0), (2.7, -30.0)])];
    let mut p = automated(
        vec![lane("volume", &[(0.0, 1.0), (3.1, 0.2)]), lane("pan", &[(0.5, 0.7), (2.0, -0.4)])],
        vec![utility],
    );
    p.channels[0].automation[0].points[0].curve = 0.6;
    let reference = render_offline(&p, SR, 512).unwrap();
    for size in [1, 64, 333, 4_096] {
        let other = render_offline(&p, SR, size).unwrap();
        assert!(max_diff(&reference, &other) < 1e-6, "block {size}: {}", max_diff(&reference, &other));
    }
}

#[test]
fn a_groups_volume_lane_scales_the_tracks_routed_into_it() {
    let plain = render_offline(&automated(vec![], vec![]), SR, 512).unwrap();
    let mut p = automated(vec![], vec![]);
    p.channels.push(Channel {
        role: ChannelRole::Submix,
        volume: 0.1,
        automation: vec![lane("volume", &[(0.0, 1.0), (2.0, 1.0), (2.0, 0.5)])],
        ..Channel::new(ChannelId(2), "G")
    });
    p.tracks.push(Track { channel: Some(ChannelId(2)), ..Track::new(TrackId(2), "G") });
    p.channels[0].output = Some(ChannelId(2));
    p.tracks[0].parent = Some(TrackId(2));
    let out = render_offline(&p, SR, 512).unwrap();
    let mut worst = 0.0f32;
    for (frame, ([l, _], [pl, _])) in out.as_chunks::<2>().0.iter().zip(plain.as_chunks::<2>().0).enumerate() {
        // The jump is smoothed over a few milliseconds around it.
        let ramp = (crate::automation::VOLUME_SMOOTHING * f64::from(SR)) as usize / 2 + crate::automation::MIX_STEP;
        if frame.abs_diff(BEAT * 2) > ramp {
            let gain = if frame < BEAT * 2 { 1.0 } else { 0.5 };
            worst = worst.max((l - pl * gain).abs());
        }
    }
    assert!(worst < 1e-5, "{worst}");
}
