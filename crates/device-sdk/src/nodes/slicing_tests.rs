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
