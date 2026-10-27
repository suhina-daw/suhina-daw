// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
use suhina_device_api::{Device, NoteEvent, ProcessContext};

fn loaded() -> Sampler {
    let mut s = Sampler::new();
    s.load_sample(
        48000,
        1,
        (0..48000).map(|i| (i as f32 * std::f32::consts::TAU * 220. / 48000.).sin() * 0.2).collect::<Vec<_>>().into(),
    );
    s
}

#[test]
fn corrected_slice_render_is_block_independent_and_through_reaches_the_end() {
    use std::sync::Arc;
    use suhina_device_api::sample_edits::SampleEdits;
    let make = |overlap| {
        let mut sampler = loaded();
        sampler.prepare(48000.).unwrap();
        for (id, value) in [
            ("mode", 2.),
            ("slice_by", 2.),
            ("slice_regions", 4.),
            ("slice_overlap", overlap),
            ("attack", 0.),
            ("sustain", 1.),
        ] {
            set(&mut sampler, id, value);
        }
        sampler.load_sample_edits(Some(Arc::new(SampleEdits::new(vec![0.3], vec![(2, 1)]).unwrap())));
        note(&mut sampler, 60);
        sampler
    };
    let short = render(&mut make(1.), 48000, 64);
    assert_eq!(short, render(&mut make(1.), 48000, 127));
    assert!(short[1000..14000 * 2].iter().any(|sample| sample.abs() > 0.01));
    assert!(short[14402 * 2..].iter().all(|sample| *sample == 0.));
    let through = render(&mut make(2.), 48000, 64);
    assert!(through[24000 * 2..47000 * 2].iter().any(|sample| sample.abs() > 0.01));
}
fn set(s: &mut Sampler, id: &str, value: f64) {
    s.set_param(INFO.param_index(id).unwrap(), value);
}
fn note(s: &mut Sampler, key: u8) {
    s.event(NoteEvent::On { key, velocity: 1. });
}
fn render(s: &mut Sampler, frames: usize, block: usize) -> Vec<f32> {
    let mut out = vec![0.; frames * 2];
    for chunk in out.chunks_mut(block * 2) {
        s.process(&ProcessContext::default(), chunk);
    }
    out
}
#[test]
fn warp_preserves_pitch_and_note_length_across_modes_and_blocks() {
    let make = |mode, block: usize, tempo| {
        let mut s = loaded();
        s.prepare(48000.).unwrap();
        for (id, value) in
            [("warp", 1.), ("warp_mode", mode), ("warp_beats", 4.), ("attack", 0.), ("sustain", 1.), ("pitch", 12.)]
        {
            set(&mut s, id, value);
        }
        let context = ProcessContext { tempo, ..ProcessContext::default() };
        s.set_context(&context);
        note(&mut s, 60);
        let mut out = vec![0.; 144000 * 2];
        for chunk in out.chunks_mut(block * 2) {
            s.process(&context, chunk);
        }
        out
    };
    for mode in 0..6 {
        let a = make(f64::from(mode), 64, 120.);
        assert_eq!(a, make(f64::from(mode), 127, 120.));
        let crossings = a[24000 * 2..48000 * 2]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|f| f[0])
            .collect::<Vec<_>>()
            .windows(2)
            .filter(|w| w[0] <= 0. && w[1] > 0.)
            .count();
        let expected = if mode == 0 { 55 } else { 220 };
        assert!(crossings.abs_diff(expected) <= 3, "mode {mode}: {crossings}");
        assert!(a[90000 * 2..94000 * 2].iter().any(|v| v.abs() > 0.01));
        assert!(a[97000 * 2..].iter().all(|v| *v == 0.));
    }
    let fast = make(1., 64, 240.);
    assert!(fast[49000 * 2..].iter().all(|v| *v == 0.));
}

#[test]
fn source_bpm_sets_speed_independently_of_loop_length_and_trim() {
    for seconds in [2, 4, 16] {
        let mut s = Sampler::new();
        s.load_sample(
            48000,
            1,
            (0..seconds * 48000)
                .map(|i| (i as f32 * std::f32::consts::TAU * 220. / 48000.).sin() * 0.2)
                .collect::<Vec<_>>()
                .into(),
        );
        for (id, value) in [("source_bpm", 120.), ("warp", 1.), ("attack", 0.), ("sustain", 1.)] {
            set(&mut s, id, value);
        }
        note(&mut s, 60);
        let out = render(&mut s, 4800, 64);
        let crossings = out.as_chunks::<2>().0.windows(2).filter(|w| w[0][0] <= 0. && w[1][0] > 0.).count();
        assert!(crossings.abs_diff(22) <= 2, "a {seconds}s loop must retain its 220 Hz pitch: {crossings} crossings");
        assert!((s.sample_position().unwrap() - 0.1 / seconds as f32).abs() < 1e-6);
        let before = s.sample_position().unwrap();
        let context = ProcessContext { tempo: 60., ..ProcessContext::default() };
        s.process(&context, &mut vec![0.; 9600]);
        assert!((s.sample_position().unwrap() - before - 0.05 / seconds as f32).abs() < 1e-6);
        set(&mut s, "start", 0.25);
        set(&mut s, "end", 0.75);
        note(&mut s, 60);
        let before = s.sample_positions().into_iter().flatten().last().unwrap();
        s.process(&context, &mut vec![0.; 9600]);
        let after = s.sample_positions().into_iter().flatten().last().unwrap();
        assert!((after - before - 0.05 / seconds as f32).abs() < 1e-6, "trimming must not alter the source tempo");
    }
}

#[test]
fn changing_warp_during_a_held_note_keeps_its_head_and_audio() {
    let mut s = loaded();
    for (id, value) in [("playback", 2.), ("warp", 1.), ("attack", 0.), ("sustain", 1.)] {
        set(&mut s, id, value);
    }
    note(&mut s, 60);
    render(&mut s, 12000, 64);
    for (id, value) in [
        ("warp_mode", 2.),
        ("warp_mode", 3.),
        ("warp_mode", 4.),
        ("warp_mode", 5.),
        ("warp_mode", 0.),
        ("warp_mode", 1.),
        ("warp", 0.),
        ("warp", 1.),
    ] {
        let position = s.sample_position().unwrap();
        set(&mut s, id, value);
        assert_eq!(s.sample_position(), Some(position), "{id}={value} must keep the held note");
        let out = render(&mut s, 4800, 127);
        assert!(s.sample_position().is_some());
        for window in out.chunks(960) {
            assert!(window.iter().any(|s| s.abs() > 0.005), "{id}={value}: silent 10 ms window");
        }
    }
}

#[test]
fn equivalent_pitch_preserving_modes_do_not_restart_the_stretcher() {
    let make = |change| {
        let mut s = loaded();
        set(&mut s, "warp", 1.);
        note(&mut s, 60);
        render(&mut s, 12000, 64);
        if change {
            set(&mut s, "warp_mode", 5.);
        }
        render(&mut s, 4800, 127)
    };
    assert_eq!(make(false), make(true));
}

#[test]
fn warp_controls_share_the_source_row_without_overlaps() {
    let ui = panel();
    let controls: Vec<_> = ui
        .controls
        .iter()
        .filter(|c| {
            c.x.iter().chain(c.y.iter()).any(|b| matches!(b.parameter.as_str(), "warp" | "warp_mode" | "source_bpm"))
        })
        .collect();
    assert_eq!(controls.len(), 3);
    for c in &controls {
        assert_eq!(c.bounds[1], 102.);
    }
    assert!(controls.windows(2).all(|w| w[0].bounds[0] + w[0].bounds[2] <= w[1].bounds[0]));
}
#[test]
fn stolen_voices_are_continuous_and_heads_are_independent() {
    for mono in [0., 1.] {
        let mut s = loaded();
        set(&mut s, "poly", 1.);
        set(&mut s, "voice_mode", mono);
        set(&mut s, "attack", 0.);
        note(&mut s, 60);
        let before = render(&mut s, 480, 64);
        note(&mut s, 72);
        let after = render(&mut s, 384, 127);
        assert!((after[0] - before[before.len() - 2]).abs() < 1e-6);
        set(&mut s, "retrigger", 0.);
        let position = s.sample_position();
        note(&mut s, 67);
        assert_eq!(s.sample_position(), position);
        assert_eq!(s.sample_positions().into_iter().flatten().count(), 1);
    }
    let mut s = loaded();
    note(&mut s, 60);
    render(&mut s, 480, 64);
    note(&mut s, 72);
    render(&mut s, 120, 64);
    let positions = s.sample_positions();
    assert!((positions[0].unwrap() - 0.0125).abs() < 1e-6);
    assert!((positions[1].unwrap() - 0.005).abs() < 1e-6);
    set(&mut s, "release", 0.);
    s.event(NoteEvent::Off { key: 72, velocity: 0. });
    render(&mut s, 1, 1);
    assert!(s.sample_positions()[0].is_some());
    assert!(s.sample_positions()[1].is_none());
}

#[test]
fn filter_types_slopes_and_modulators_are_audible_and_block_independent() {
    let make = |kind, slope| {
        let mut s = loaded();
        for (id, value) in [
            ("filter", kind),
            ("filter_slope", slope),
            ("cutoff", 400.),
            ("filter_env", 2.),
            ("velocity_filter", 0.5),
            ("key_filter", 1.),
            ("filter_attack", 0.01),
            ("filter_decay", 0.02),
            ("filter_sustain", 0.3),
        ] {
            set(&mut s, id, value);
        }
        note(&mut s, 72);
        s
    };
    let mut outputs = Vec::new();
    for kind in [1., 2., 3., 4.] {
        for slope in [0., 1.] {
            let output = render(&mut make(kind, slope), 4096, 64);
            assert!(output.iter().all(|v| v.is_finite()));
            assert_eq!(output, render(&mut make(kind, slope), 4096, 127));
            assert!(outputs.iter().all(|other| other != &output));
            outputs.push(output);
        }
    }
    let baseline = render(&mut make(1., 0.), 4096, 64);
    for (id, value) in [("filter_env", 0.), ("velocity_filter", 0.), ("key_filter", 0.), ("filter_attack", 1.)] {
        let mut s = make(1., 0.);
        set(&mut s, id, value);
        assert_ne!(baseline, render(&mut s, 4096, 64), "{id}");
    }
}

#[test]
fn playback_changes_stop_voices_but_reselecting_the_mode_does_not() {
    for mode in [0., 1., 2.] {
        let mut s = loaded();
        set(&mut s, "playback", mode);
        note(&mut s, 60);
        render(&mut s, 64, 16);
        set(&mut s, "playback", mode);
        assert!(s.sample_position().is_some());
        set(&mut s, "playback", (mode + 1.) % 3.);
        assert_eq!(s.sample_position(), None);
        assert!(render(&mut s, 128, 32).iter().all(|v| *v == 0.));
        note(&mut s, 60);
        assert!(render(&mut s, 128, 32).iter().any(|v| *v != 0.));
    }
}

#[test]
fn compact_panel_keeps_controls_reachable_on_its_pages() {
    let ui = panel();
    let slice_by = ui.controls.iter().find(|c| c.x.as_ref().is_some_and(|b| b.parameter == "slice_by")).unwrap();
    assert_eq!(slice_by.bounds, [176., 102., 96., 18.]);
    assert_eq!(ui.size, [900., 168.]);
    assert!(ui.size[1] + 32. <= 200.);
    assert_eq!(ui.viewport_size, Some(ui.size));
    for c in &ui.controls {
        assert!(c.widget.is_some() && c.drawing.is_empty(), "{} must use an SDK widget", c.id);
        let [x, y, w, h] = c.bounds;
        assert!(x >= 0. && y >= 0. && x + w <= ui.size[0] && y + h <= ui.size[1], "{}", c.id);
    }
    for p in PARAMS {
        if p.id.starts_with("slice_")
            || matches!(
                p.id,
                "start"
                    | "end"
                    | "loop_start"
                    | "loop_end"
                    | "ping_pong"
                    | "grain_env"
                    | "envelope_tab"
                    | "warp_beats"
                    | "crossfade"
                    | "source_numerator"
                    | "source_denominator"
            )
        {
            continue;
        }
        assert!(ui.controls.iter().any(|c| c.x.iter().chain(c.y.iter()).any(|b| b.parameter == p.id)
                || matches!(&c.widget, Some(Widget::Envelope { attack, decay, level, release }) if [Some(attack.as_str()),Some(decay.as_str()),Some(level.as_str()),release.as_deref()].contains(&Some(p.id)))), "{}", p.id);
    }
    assert!(!ui.controls.iter().any(|c| c.id == "grain_cloud"));
    assert!(!ui.controls.iter().any(|c| c.x.iter().chain(c.y.iter()).any(|b| b.parameter == "crossfade")));
    assert_eq!(
        ui.controls
            .iter()
            .filter(|c| matches!(&c.widget, Some(Widget::Button { label, .. }) if label == "Grain"))
            .count(),
        1
    );
    for view in [0., 1., 2., 3.] {
        for mode in [0., 1., 2.] {
            let controls: Vec<_> = ui
                .controls
                .iter()
                .filter(|c| {
                    c.visible_when.as_ref().is_none_or(|v| {
                        v.evaluate(&|id, _, _| match id {
                            "panel_view" => view,
                            "mode" => mode,
                            _ => 0.,
                        }) > 0.
                    })
                })
                .collect();
            let waveforms: Vec<_> =
                controls.iter().filter(|c| matches!(c.widget, Some(Widget::SampleWaveform))).collect();
            assert_eq!(waveforms.len(), 1);
            assert_eq!(waveforms[0].bounds, [64., 22., 836., 78.]);
            assert_eq!(controls.iter().any(|c| c.id == "lfo_preview"), view == 2.);
            if view == 0. {
                for id in ["gain", "pan", "filter", if mode == 2. { "slice_overlap" } else { "voice_mode" }] {
                    assert!(controls.iter().any(|c| c.x.iter().chain(c.y.iter()).any(|b| b.parameter == id)));
                }
                assert_eq!(
                    controls.iter().any(|c| c.y.as_ref().is_some_and(|b| b.parameter == "position")),
                    mode == 1.
                );
            }
        }
    }
}

#[test]
fn slice_release_and_overlap_share_the_source_row() {
    let ui = panel();
    let bound = |id: &str| ui.controls.iter().find(|c| c.x.as_ref().is_some_and(|b| b.parameter == id)).unwrap().bounds;
    let overlap = bound("slice_overlap");
    let release = bound("slice_release");
    assert!(release[0] + release[2] <= overlap[0]);
    assert_eq!(overlap[1], release[1]);
}

#[test]
fn compact_panel_interactive_controls_never_overlap_on_any_page() {
    let ui = panel();
    for view in [0., 1., 2., 3.] {
        for mode in [0., 1., 2.] {
            for rate in 0..6 {
                let visible: Vec<_> = ui
                    .controls
                    .iter()
                    .filter(|c| {
                        !matches!(c.widget, Some(Widget::Label { .. } | Widget::Panel { .. } | Widget::Separator))
                            && c.visible_when.as_ref().is_none_or(|v| {
                                v.evaluate(&|id, _, _| match id {
                                    "panel_view" => view,
                                    "mode" => mode,
                                    "lfo_rate_mode" => rate as f32,
                                    _ => 0.,
                                }) > 0.
                            })
                    })
                    .collect();
                for (i, a) in visible.iter().enumerate() {
                    for b in visible.iter().skip(i + 1) {
                        let [ax, ay, aw, ah] = a.bounds;
                        let [bx, by, bw, bh] = b.bounds;
                        assert!(
                            ax + aw <= bx || bx + bw <= ax || ay + ah <= by || by + bh <= ay,
                            "{} overlaps {} in view {view}, mode {mode}, rate {rate}",
                            a.id,
                            b.id
                        );
                    }
                }
            }
        }
    }
    for id in ["cutoff", "resonance", "fade_in", "fade_out", "pitch", "velocity", "gain", "pan"] {
        assert!(
            ui.controls
                .iter()
                .any(|c| c.y.as_ref().is_some_and(|b| b.parameter == id)
                    && matches!(c.widget, Some(Widget::Dial { .. }))),
            "{id} uses a knob"
        );
    }
    assert!(ui.controls.iter().any(|c| c.y.as_ref().is_some_and(|b| b.parameter == "lfo_rate")
        && matches!(c.widget, Some(Widget::Number { bordered: true, .. }))));
}
#[test]
fn envelopes_have_independent_stages_and_responsive_release() {
    let ui = panel();
    for (prefix, ids) in [
        ("AMP", ["attack", "decay", "sustain", "release"]),
        ("FILTER", ["filter_attack", "filter_decay", "filter_sustain", "filter_release"]),
    ] {
        let envelope = ui.controls.iter().find(|c| c.id == format!("{prefix}_envelope")).unwrap();
        assert!(
            matches!(&envelope.widget, Some(Widget::Envelope { attack, decay, level, release }) if attack == ids[0] && decay == ids[1] && level == ids[2] && release.as_deref() == Some(ids[3]))
        );
        assert!(envelope.x.is_none() && envelope.y.is_none());
    }
}
#[test]
fn lfo_destinations_are_audible_bounded_and_block_independent_in_both_modes() {
    let make = |mode, destination: &str, depth| {
        let mut s = loaded();
        for (id, value) in [
            ("mode", mode),
            ("playback", 2.),
            ("attack", 0.),
            ("decay", 0.),
            ("sustain", 1.),
            ("filter", 1.),
            ("cutoff", 400.),
            ("lfo_rate", 5.),
        ] {
            set(&mut s, id, value);
        }
        if !destination.is_empty() {
            set(&mut s, destination, depth);
        }
        note(&mut s, 60);
        s
    };
    for mode in [0., 1.] {
        let baseline = render(&mut make(mode, "", 0.), 24000, 64);
        for (id, depth) in [("lfo_pitch", 4.), ("lfo_filter", 3.), ("lfo_volume", 1.), ("lfo_pan", 1.)] {
            let output = render(&mut make(mode, id, depth), 24000, 64);
            assert!(output.iter().all(|v| v.is_finite()));
            assert_ne!(output, baseline, "{mode}: {id}");
            assert_eq!(output, render(&mut make(mode, id, depth), 24000, 127), "{mode}: {id}");
            if matches!(id, "lfo_volume" | "lfo_pan") {
                assert!(output.iter().zip(&baseline).all(|(a, b)| a.abs() <= b.abs() + 1e-6), "{id} must never boost");
            }
            assert_eq!(baseline, render(&mut make(mode, id, 0.), 24000, 64), "zero {id} must preserve sound");
        }
    }
}

#[test]
fn volume_and_pan_lfo_match_the_sine_golden() {
    for destination in ["lfo_volume", "lfo_pan"] {
        let mut s = Sampler::new();
        s.prepare(48000.).unwrap();
        s.load_sample(48000, 1, vec![1.; 4800].into());
        for (id, value) in [("attack", 0.), ("decay", 0.), ("sustain", 1.), ("lfo_rate", 20.), (destination, 1.)] {
            set(&mut s, id, value);
        }
        note(&mut s, 60);
        let output = render(&mut s, 2400, 7);
        for (i, frame) in output.as_chunks::<2>().0.iter().enumerate() {
            let wave = ((i + 1) as f64 * std::f64::consts::TAU * 20. / 48000.).sin();
            let expected = if destination == "lfo_volume" {
                [0.5 - wave * 0.5; 2]
            } else {
                [1. - wave.max(0.), 1. + wave.min(0.)]
            };
            for channel in 0..2 {
                assert!((f64::from(frame[channel]) - expected[channel]).abs() < 1e-6, "{destination} frame {i}");
            }
        }
    }
}

#[test]
fn lfo_shapes_match_the_shared_oscillator_and_random_is_repeatable() {
    for shape_index in 0..6 {
        let make = || {
            let mut s = Sampler::new();
            s.load_sample(48000, 1, vec![1.; 48000].into());
            set(&mut s, "attack", 0.);
            set(&mut s, "sustain", 1.);
            set(&mut s, "lfo_rate", 20.);
            set(&mut s, "lfo_shape", shape_index as f64);
            set(&mut s, "lfo_volume", 1.);
            s.prepare(48000.).unwrap();
            note(&mut s, 60);
            s
        };
        let output = render(&mut make(), 10000, 64);
        assert_eq!(output, render(&mut make(), 10000, 127));
        let mut phase: f64 = 0.;
        let mut cycle = 0;
        for frame in output.as_chunks::<2>().0 {
            let next = phase + 20. / 48000.;
            cycle += usize::from(next >= 1.);
            phase = next.fract();
            let cycles = if shape_index == 5 { cycle as f64 } else { phase };
            let expected = (1. - waveform(shape_index, cycles)) * 0.5;
            assert!((f64::from(frame[0]) - expected).abs() < 1e-6, "shape {shape_index}");
        }
        let mut reset = make();
        render(&mut reset, 10000, 64);
        reset.reset();
        note(&mut reset, 60);
        assert_eq!(output, render(&mut reset, 10000, 64));
    }
}

#[test]
fn loop_edges_use_source_fades_without_shortening_the_loop() {
    for reverse in [0., 1.] {
        for warp in [0., 1.] {
            for (fade_in, fade_out) in [(0., 0.), (10., 0.), (0., 20.), (10., 20.)] {
                let make = |crossfade| {
                    let mut s = Sampler::new();
                    s.load_sample(48000, 1, vec![1.; 4800].into());
                    for (id, value) in [
                        ("attack", 0.),
                        ("sustain", 1.),
                        ("playback", 2.),
                        ("loop_start", 0.25),
                        ("loop_end", 0.75),
                        ("reverse", reverse),
                        ("fade_in", fade_in),
                        ("fade_out", fade_out),
                        ("crossfade", crossfade),
                        ("warp", warp),
                        ("warp_mode", 0.),
                        ("source_bpm", 120.),
                    ] {
                        set(&mut s, id, value);
                    }
                    note(&mut s, 60);
                    s
                };
                let output = render(&mut make(0.), 10800, 64);
                assert_eq!(output, render(&mut make(100.), 10800, 127), "legacy crossfade is inert");
                for (i, frame) in output.as_chunks::<2>().0.iter().enumerate() {
                    let offset = if i < 3600 { i } else { 1200 + (i - 3600) % 2400 };
                    let pos = if reverse > 0. { 4799 - offset } else { offset } as f64;
                    let (lo, hi) = if (1200. ..3600.).contains(&pos) { (1200., 3600.) } else { (0., 4800.) };
                    let fade =
                        |distance: f64, ms: f64| if ms == 0. { 1. } else { (distance / (ms * 48.)).clamp(0., 1.) };
                    let expected = fade(pos - lo, fade_in) * fade(hi - 1. - pos, fade_out);
                    assert!(
                        (f64::from(frame[0]) - expected).abs() < 1e-6,
                        "{reverse} {warp} {fade_in} {fade_out} frame {i}"
                    );
                }
            }
        }
    }
}

#[test]
fn source_fades_are_independent_and_match_linear_edges_in_both_directions() {
    for reverse in [0., 1.] {
        for (fade_in, fade_out) in [(0., 0.), (10., 0.), (0., 20.), (10., 20.)] {
            let make = || {
                let mut s = Sampler::new();
                s.load_sample(48000, 1, vec![1.; 4800].into());
                set(&mut s, "attack", 0.);
                set(&mut s, "sustain", 1.);
                set(&mut s, "reverse", reverse);
                set(&mut s, "fade_in", fade_in);
                set(&mut s, "fade_out", fade_out);
                note(&mut s, 60);
                s
            };
            let output = render(&mut make(), 4800, 64);
            assert_eq!(output, render(&mut make(), 4800, 127));
            for (i, frame) in output.as_chunks::<2>().0.iter().enumerate() {
                let pos = if reverse > 0. { 4799 - i } else { i } as f64;
                let fade = |distance: f64, ms: f64| if ms == 0. { 1. } else { (distance / (ms * 48.)).min(1.) };
                let expected = fade(pos, fade_in) * fade(4799. - pos, fade_out);
                assert!((f64::from(frame[0]) - expected).abs() < 1e-6, "{reverse} {fade_in} {fade_out} {i}");
            }
        }
    }
    for mode in [0., 1.] {
        let make = |fade| {
            let mut s = loaded();
            set(&mut s, "mode", mode);
            set(&mut s, "position", 0.);
            set(&mut s, "jitter", 0.);
            set(&mut s, "fade_in", fade);
            note(&mut s, 60);
            render(&mut s, 4096, 64)
        };
        assert_ne!(make(0.), make(500.));
    }
}

#[test]
fn changing_editor_tabs_does_not_change_audio() {
    let make = |view| {
        let mut s = loaded();
        set(&mut s, "panel_view", view);
        note(&mut s, 60);
        render(&mut s, 2048, 64)
    };
    assert_eq!(make(0.), make(1.));
    assert_eq!(make(1.), make(2.));
}

#[test]
fn mono_replaces_the_voice_and_reports_the_read_head() {
    let mut s = loaded();
    set(&mut s, "voice_mode", 1.);
    set(&mut s, "attack", 0.);
    note(&mut s, 60);
    render(&mut s, 480, 64);
    assert!((s.sample_position().unwrap() - 0.01).abs() < 0.0001);
    note(&mut s, 72);
    let actual = render(&mut s, 384, 64);
    let mut reference = loaded();
    set(&mut reference, "attack", 0.);
    note(&mut reference, 72);
    assert_eq!(&actual[512..], &render(&mut reference, 384, 64)[512..]);
    s.reset();
    assert_eq!(s.sample_position(), None);
}

#[test]
fn pitch_envelope_changes_audio_and_is_block_independent() {
    let mut plain = loaded();
    note(&mut plain, 60);
    let plain = render(&mut plain, 4096, 64);
    let mut s = loaded();
    set(&mut s, "pitch_env", 12.);
    note(&mut s, 60);
    let modulated = render(&mut s, 4096, 64);
    assert_ne!(plain, modulated);
    s.reset();
    note(&mut s, 60);
    assert_eq!(modulated, render(&mut s, 4096, 127));
}

#[test]
fn device_contract() {
    let failures = suhina_device_api::conformance::check(&|| {
        let mut sampler = Sampler::new();
        sampler.load_sample(
            48000,
            1,
            (0..48000)
                .map(|i| (i as f32 * std::f32::consts::TAU * 220. / 48000.).sin() * 0.2)
                .collect::<Vec<_>>()
                .into(),
        );
        Box::new(sampler)
    });
    assert!(failures.is_empty(), "{failures:?}");
}

#[test]
fn pitch_reverse_and_region_golden() {
    let mut s = Sampler::new();
    s.load_sample(48000, 1, (0..100).map(|i| i as f32 / 100.).collect::<Vec<_>>().into());
    for (id, v) in [("attack", 0.), ("decay", 0.), ("sustain", 1.)] {
        set(&mut s, id, v);
    }
    note(&mut s, 72);
    assert_eq!(render(&mut s, 4, 3), vec![0., 0., 0.02, 0.02, 0.04, 0.04, 0.06, 0.06]);
    s.reset();
    set(&mut s, "reverse", 1.);
    note(&mut s, 60);
    assert_eq!(render(&mut s, 3, 1), vec![0.99, 0.99, 0.98, 0.98, 0.97, 0.97]);
    s.reset();
    set(&mut s, "start", 0.9);
    set(&mut s, "end", 0.1);
    note(&mut s, 60);
    assert!(render(&mut s, 32, 7).iter().all(|v| *v == 0.));
}

#[test]
fn polyphonic_sum_keeps_headroom_and_hostile_audio_stays_finite() {
    let mut s = Sampler::new();
    s.load_sample(48000, 1, vec![1.; 16].into());
    for (id, v) in [("attack", 0.), ("decay", 0.), ("sustain", 1.), ("poly", 32.)] {
        set(&mut s, id, v);
    }
    for _ in 0..32 {
        note(&mut s, 60);
    }
    assert_eq!(render(&mut s, 1, 1), [32., 32.]);
    s.load_sample(48000, 1, vec![f32::MAX; 16].into());
    for _ in 0..32 {
        note(&mut s, 60);
    }
    assert!(render(&mut s, 1, 1).iter().all(|s| s.is_finite()));
}

#[test]
fn granular_is_deterministic_across_blocks_and_reset() {
    let make = || {
        let mut s = loaded();
        set(&mut s, "mode", 1.);
        set(&mut s, "scan", 1.);
        set(&mut s, "spread", 0.8);
        note(&mut s, 60);
        s
    };
    let expected = render(&mut make(), 16000, 1024);
    let mut s = make();
    assert_eq!(render(&mut s, 16000, 7), expected);
    assert!(expected.iter().any(|v| v.abs() > 0.01));
    s.reset();
    note(&mut s, 60);
    assert_eq!(render(&mut s, 16000, 127), expected);
    s.event(NoteEvent::Off { key: 60, velocity: 0. });
    set(&mut s, "release", 0.);
    assert!(render(&mut s, 16, 3).iter().all(|v| *v == 0.));
}

#[test]
fn reverse_changes_held_notes_and_glide_starts_from_the_previous_pitch() {
    let mut s = Sampler::new();
    s.load_sample(48000, 1, (0..100).map(|i| i as f32 / 100.).collect::<Vec<_>>().into());
    for (id, v) in [("attack", 0.), ("decay", 0.), ("sustain", 1.), ("poly", 1.)] {
        set(&mut s, id, v);
    }
    note(&mut s, 60);
    render(&mut s, 4, 1);
    set(&mut s, "reverse", 1.);
    assert_eq!(render(&mut s, 3, 1), [0.04, 0.04, 0.03, 0.03, 0.02, 0.02]);
    s.reset();
    set(&mut s, "reverse", 0.);
    set(&mut s, "glide", 0.1);
    note(&mut s, 60);
    render(&mut s, 4, 1);
    note(&mut s, 72);
    let output = render(&mut s, 3, 1);
    assert!((output[0] - 0.03).abs() < 1e-6, "{output:?}");
    assert!(s.sample_position().is_some_and(|p| p > 0.03 && p < 0.04));
}

#[test]
fn loop_modes_keep_playing_and_empty_source_is_silent() {
    for reverse in [0., 1.] {
        for ping in [0., 1.] {
            let mut s = loaded();
            for (id, value) in
                [("reverse", reverse), ("ping_pong", ping), ("playback", 2.), ("loop_start", 0.2), ("loop_end", 0.3)]
            {
                set(&mut s, id, value);
            }
            note(&mut s, 60);
            let out = render(&mut s, 96000, 128);
            assert!(out.iter().all(|v| v.is_finite()));
            assert!(out[180000..].iter().any(|v| v.abs() > 0.01));
        }
    }
    let mut s = Sampler::new();
    note(&mut s, 60);
    assert!(render(&mut s, 64, 8).iter().all(|v| *v == 0.));
}

#[test]
fn fractional_ping_pong_never_reads_past_the_region_boundary() {
    let mut s = Sampler::new();
    s.load_sample(48000, 1, vec![1.; 100].into());
    for (id, v) in [("attack", 0.), ("decay", 0.), ("sustain", 1.), ("playback", 2.), ("ping_pong", 1.), ("pitch", 7.)]
    {
        set(&mut s, id, v);
    }
    note(&mut s, 60);
    assert!(render(&mut s, 1000, 17).iter().all(|v| *v == 1.));
}
