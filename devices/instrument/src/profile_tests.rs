// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
use suhina_dsp::modal::profile::ProfileMode;

fn profile() -> ModalProfile {
    ModalProfile::new(
        60,
        443.2,
        &[
            ProfileMode { ratio: 1.0, magnitude: 0.6, decay_seconds: 0.8 },
            ProfileMode { ratio: 2.756, magnitude: 0.25, decay_seconds: 0.31 },
            ProfileMode { ratio: 5.404, magnitude: 0.15, decay_seconds: 0.12 },
        ],
    )
    .unwrap()
}

fn render(profile: ModalProfile, change: Option<(usize, f64)>, block: usize) -> Vec<f32> {
    let mut core = Instrument::new();
    core.set_modal_profile(profile);
    if let Some((index, value)) = change {
        core.set_param(index, value);
    }
    core.reset();
    core.event(NoteEvent::On { key: 60, velocity: 0.8 });
    let mut output = vec![0.0; 8192];
    for buffer in output.chunks_mut(block * 2) {
        core.process(&ProcessContext::default(), buffer);
    }
    output
}

#[test]
fn accepted_profile_is_played_and_all_model_controls_transform_it() {
    let original = render(profile(), None, 128);
    assert!(original.iter().any(|sample| sample.abs() > 0.001));
    assert_eq!(original, render(profile(), None, 17));
    assert_eq!(original, render(profile(), None, 512));
    for change in [(param::DECAY, 0.5), (param::BRIGHTNESS, 0.9), (param::STRUCTURE, 0.04), (param::STRIKE, 0.7)] {
        let changed = render(profile(), Some(change), 128);
        let distance: f64 = original.iter().zip(&changed).map(|(a, b)| f64::from((a - b).abs())).sum();
        assert!(distance > 0.1, "control {} must shape the profile", change.0);
        assert!(changed.iter().all(|value| value.is_finite()));
    }
}

#[test]
fn measured_reference_pitch_and_transposition_match_analytical_modes() {
    let profile =
        ModalProfile::new(60, 443.2, &[ProfileMode { ratio: 1.0, magnitude: 0.8, decay_seconds: 0.8 }]).unwrap();
    for (key, bend) in [(60, 0.0), (72, 0.0), (60, 0.5)] {
        let mut core = Instrument::new();
        core.set_modal_profile(profile);
        core.set_param(param::GAIN, 0.0);
        core.reset();
        core.event(NoteEvent::PitchBend(bend));
        core.event(NoteEvent::On { key, velocity: 1.0 });
        let mut output = [0.0; 4096];
        core.process(&ProcessContext::default(), &mut output);
        let frequency = 443.2 * 2.0_f64.powf((f64::from(key) - 60.0 + f64::from(bend) * 2.0) / 12.0);
        for frame in 100..2048 {
            let expected = 0.2
                * (-1000.0_f64.ln() * frame as f64 / (48000.0 * 0.8)).exp()
                * (std::f64::consts::TAU * frequency * frame as f64 / 48000.0).sin();
            assert!((f64::from(output[frame * 2]) - expected).abs() < 2e-7, "{key} / {bend} / {frame}");
        }
        core.prepare(44100.0).unwrap();
        assert_eq!(core.modal_profile(), Some(&profile));
        assert!(core.model_supported());
    }
}

#[test]
fn rejected_profile_never_falls_back_and_sample_layer_is_independent() {
    let mut core = Instrument::new();
    core.set_modal_profile(profile());
    assert!(core.load_model_state(&[0; 408]).is_err());
    for revision in [1.0, 2.0, 65535.0] {
        core.set_param(param::REVISION, revision);
        core.event(NoteEvent::On { key: 60, velocity: 1.0 });
        let mut output = [1.0; 1024];
        core.process(&ProcessContext::default(), &mut output);
        assert!(output.iter().all(|value| *value == 0.0));
        assert!(!core.model_supported());
    }
    core.samples().unwrap().load_sample(48000, 1, std::sync::Arc::from(vec![0.5; 4096]));
    core.set_param(param::SOURCE, 2.0);
    core.reset();
    core.event(NoteEvent::On { key: 60, velocity: 1.0 });
    let mut output = [0.0; 1024];
    core.process(&ProcessContext::default(), &mut output);
    assert!(output.iter().any(|value| value.abs() > 0.001));
    core.load_model_state(&profile().encode()).unwrap();
    assert!(core.model_supported());
}

#[test]
fn profile_extremes_remain_finite_at_supported_rates() {
    let modes: Vec<_> = (0..16)
        .map(|i| ProfileMode { ratio: 2.0_f64.powf(i as f64 - 8.0), magnitude: 1.0, decay_seconds: 60.0 })
        .collect();
    let profile = ModalProfile::new(0, 20000.0, &modes).unwrap();
    for rate in [8000.0, 44100.0, 48000.0, 96000.0, 192000.0, 384000.0] {
        let mut core = Instrument::with_rate(rate).unwrap();
        core.set_modal_profile(profile);
        for (index, value) in
            [(param::DECAY, 20.0), (param::STRUCTURE, 0.05), (param::BRIGHTNESS, 1.0), (param::GAIN, 12.0)]
        {
            core.set_param(index, value);
        }
        for key in 0..128 {
            core.event(NoteEvent::On { key, velocity: 1.0 });
            let mut output = [0.0; 256];
            core.process(&ProcessContext::default(), &mut output);
            assert!(output.iter().all(|value| value.is_finite()));
        }
    }
}

#[test]
fn editor_labels_profile_revision_and_its_creative_strike_control_truthfully() {
    let layout = panel::layout();
    let ui = layout.expanded.as_ref().unwrap();
    for revision in [1.0, 2.0] {
        let visible = |name: &str| {
            ui.controls.iter().any(|control| {
                control.name == name
                    && control.visible_when.as_ref().is_none_or(|value| {
                        value.evaluate(&|id, _, _| match id {
                            "model_revision" => revision,
                            "#page" => 1.0,
                            _ => 0.0,
                        }) > 0.0
                    })
            })
        };
        assert_eq!(visible("Experimental struck modal · revision 1"), revision == 1.0);
        assert_eq!(visible("Experimental modal profile · revision 2"), revision == 2.0);
        assert_eq!(visible("Strike position"), revision == 1.0);
        assert_eq!(visible("Strike colour"), revision == 2.0);
    }
    assert!(layout.panel(&INFO).is_ok());
}
