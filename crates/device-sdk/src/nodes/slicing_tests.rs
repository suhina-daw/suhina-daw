// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
use suhina_device_api::sample_edits::SampleEdits;

fn parameters(method: f64) -> Vec<f64> {
    let mut params: Vec<_> = crate::sampler::PARAMS.iter().map(|p| p.default).collect();
    params[0] = 2.;
    params[60] = method;
    params[83] = 120.;
    params[88] = 2.;
    params[89] = 4.;
    params
}

#[test]
fn beat_slices_use_source_tempo_and_first_beat() {
    let analysis = SliceAnalysis::default();
    let mut params = parameters(1.);
    params[85] = 0.125;
    let expected = analysis.points(&params, 96000, 48000., 120.);
    assert_eq!(&expected[..5], &[0., 0.125, 0.375, 0.625, 0.875]);
    assert_eq!(analysis.points(&params, 96000, 48000., 73.), expected);
    params[83] = 0.;
    assert_eq!(&analysis.points(&params, 96000, 48000., 120.)[..2], &[0., 1.]);
}

#[test]
fn moved_candidates_stay_suppressed_and_pins_survive_method_and_trim_changes() {
    let analysis = SliceAnalysis::default();
    let mut params = parameters(2.);
    let edits = SampleEdits::new(vec![0.3], vec![(2, 1)]).unwrap();
    let points = |p: &[f64]| analysis.edited_points(p, 96000, 48000., 120., &edits).collect::<Vec<_>>();
    assert_eq!(points(&params), [0.3, 0.5, 0.75]);
    params[1] = 0.4;
    assert_eq!(points(&params), [0.5, 0.75]);
    params[1] = 0.;
    assert_eq!(points(&params), [0.3, 0.5, 0.75]);
    params[60] = 3.;
    assert_eq!(points(&params), [0.3]);
    params[60] = 1.;
    assert_eq!(points(&params), [0.25, 0.3, 0.5, 0.75]);
}

#[test]
fn duplicate_automatic_and_pinned_boundaries_emit_once() {
    let analysis = SliceAnalysis::default();
    let edits = SampleEdits::new(vec![0.25, 0.5, 0.75], vec![]).unwrap();
    assert_eq!(
        analysis.edited_points(&parameters(2.), 96000, 48000., 120., &edits).collect::<Vec<_>>(),
        [0.25, 0.5, 0.75]
    );
}

const RATE: f64 = 48000.;

/// A decaying tone at `hz`, starting at `at` seconds, into a mono buffer.
fn tone(buffer: &mut [f32], at: f64, hz: f64, decay: f64) {
    let first = (at * RATE) as usize;
    for (i, sample) in buffer.iter_mut().enumerate().skip(first) {
        let t = (i - first) as f64 / RATE;
        *sample += ((t * hz * std::f64::consts::TAU).sin() * (-t / decay).exp() * 0.8) as f32;
    }
}

fn transients(buffer: &[f32], sensitivity: f64) -> Vec<f64> {
    let mut params = parameters(0.);
    params[59] = sensitivity;
    let points = SliceAnalysis::new(buffer, 1, RATE).points(&params, buffer.len(), RATE, 120.);
    points.iter().copied().filter(|p| *p > 0. && *p < 1.).collect()
}

#[test]
fn a_one_shot_bass_note_is_one_slice() {
    // A low note's cycles are longer than a reading of the level: they must not read as hits.
    for hz in [31., 41., 55., 82.] {
        let mut buffer = vec![0f32; (RATE * 1.2) as usize];
        tone(&mut buffer, 0.002, hz, 0.4);
        for sensitivity in [0.5, 1.] {
            assert_eq!(transients(&buffer, sensitivity), [] as [f64; 0], "{hz} Hz at {sensitivity}");
        }
    }
}

#[test]
fn hits_in_a_loop_are_found_where_they_start() {
    let mut buffer = vec![0f32; (RATE * 2.) as usize];
    for (i, at) in [0., 0.5, 1., 1.5].into_iter().enumerate() {
        tone(&mut buffer, at, if i % 2 == 0 { 60. } else { 400. }, 0.08);
    }
    let found = transients(&buffer, 0.5);
    assert_eq!(found.len(), 3, "{found:?}");
    for (point, expected) in found.iter().zip([0.25, 0.5, 0.75]) {
        assert!((point - expected).abs() < 0.006, "{point} for {expected}");
    }
}

#[test]
fn a_quiet_hit_needs_more_sensitivity() {
    let mut buffer = vec![0f32; (RATE * 1.) as usize];
    tone(&mut buffer, 0., 200., 0.3);
    let first = (0.5 * RATE) as usize;
    for (i, sample) in buffer.iter_mut().enumerate().skip(first) {
        let t = (i - first) as f64 / RATE;
        *sample += ((t * 3000. * std::f64::consts::TAU).sin() * (-t / 0.03).exp() * 0.1) as f32;
    }
    assert_eq!(transients(&buffer, 0.), [] as [f64; 0]);
    assert_eq!(transients(&buffer, 1.).len(), 1);
}

#[test]
fn silence_and_odd_input_find_nothing() {
    assert_eq!(transients(&[0.; 4800], 1.), [] as [f64; 0]);
    assert_eq!(transients(&[], 1.), [] as [f64; 0]);
    assert_eq!(transients(&[f32::NAN; 4800], 1.), [] as [f64; 0]);
}
