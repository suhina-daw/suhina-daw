// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Device groups in the chain (`docs/spec/device-groups.md`): branches summed at their volume
//! and pan, mute and solo, the mix and the gain, instruments layered, nesting, block sizes.

use crate::{RenderInputs, RenderOptions};
use suhina_model::*;

use crate::chain_tests::{SR, blocks, device, max_diff, note, peak, project};
use crate::{Renderer, render_offline};

/// -6.0206 dB: half the amplitude.
const HALF_DB: f64 = -6.0206;

fn branch(devices: Vec<Device>) -> Branch {
    Branch { devices, ..Branch::new("B") }
}

fn group(branches: Vec<Branch>, params: &[(&str, f64)]) -> Device {
    Device {
        params: params.iter().map(|(p, v)| DeviceParam::new(*p, *v)).collect(),
        ..Device::group("G", DeviceGroup::with_branches(branches))
    }
}

fn half() -> Device {
    device("utility", &[("gain", HALF_DB)])
}

fn render(devices: Vec<Device>) -> Vec<f32> {
    render_offline(&project(vec![note(0.0, 2.0, 69)], 2.0, devices), SR, 512).unwrap_or_default()
}

fn scaled(signal: &[f32], by: f32) -> Vec<f32> {
    signal.iter().map(|s| s * by).collect()
}

/// The sound of the notes through a neutral chain, to compare against.
fn reference() -> Vec<f32> {
    render(vec![device("utility", &[])])
}

#[test]
fn a_group_of_one_branch_sounds_exactly_like_its_devices() {
    let plain = render(vec![half()]);
    let grouped = render(vec![group(vec![branch(vec![half()])], &[])]);
    assert_eq!(plain, grouped, "bit for bit");
    let empty = render(vec![device("utility", &[]), group(vec![branch(vec![])], &[])]);
    assert_eq!(empty, reference(), "an empty branch is a clean passthrough");
}

#[test]
fn branches_add_up() {
    let two_halves = render(vec![group(vec![branch(vec![half()]), branch(vec![half()])], &[])]);
    let whole = reference();
    assert!(max_diff(&two_halves, &whole) < 1e-4, "{}", max_diff(&two_halves, &whole));
    assert!(peak(&whole) > 0.1);
}

#[test]
fn a_muted_branch_adds_nothing_and_a_soloed_one_is_all_that_sounds() {
    let whole = reference();
    let mut muted = branch(vec![device("utility", &[])]);
    muted.mute = true;
    let with_mute = render(vec![group(vec![branch(vec![half()]), muted], &[])]);
    assert!(max_diff(&with_mute, &scaled(&whole, 0.5)) < 1e-4);

    let mut soloed = branch(vec![half()]);
    soloed.solo = true;
    let with_solo = render(vec![group(vec![soloed, branch(vec![device("utility", &[])])], &[])]);
    assert!(max_diff(&with_solo, &scaled(&whole, 0.5)) < 1e-4);

    // Mute wins over solo; every branch muted leaves the dry part of the mix only.
    let mut both = branch(vec![device("utility", &[])]);
    both.solo = true;
    both.mute = true;
    let silent = render(vec![device("utility", &[]), group(vec![both], &[])]);
    assert!(peak(&silent) < 1e-6);
    let mut all_muted = branch(vec![device("utility", &[])]);
    all_muted.mute = true;
    let dry_half = render(vec![device("utility", &[]), group(vec![all_muted], &[("mix", 0.5)])]);
    assert!(max_diff(&dry_half, &scaled(&whole, 0.5)) < 1e-4);
}

#[test]
fn the_mix_crossfades_the_dry_signal_and_the_branches_linearly() {
    let whole = reference();
    for (mix, expected) in [(0.0, 1.0), (0.5, 0.75), (1.0, 0.5)] {
        let out = render(vec![device("utility", &[]), group(vec![branch(vec![half()])], &[("mix", mix)])]);
        assert!(
            max_diff(&out, &scaled(&whole, expected)) < 1e-4,
            "mix {mix}: {}",
            max_diff(&out, &scaled(&whole, expected))
        );
    }
}

#[test]
fn the_output_gain_scales_the_group() {
    let whole = reference();
    let out = render(vec![device("utility", &[]), group(vec![branch(vec![])], &[("gain", HALF_DB)])]);
    assert!(max_diff(&out, &scaled(&whole, 0.5)) < 1e-4);
    let silent = render(vec![device("utility", &[]), group(vec![branch(vec![])], &[("gain", SILENT_DB_FOR_TESTS)])]);
    assert!(peak(&silent) < 1e-9);
}

const SILENT_DB_FOR_TESTS: f64 = crate::SILENT_DB;

#[test]
fn a_branch_pans_with_the_channel_law() {
    let mut right = branch(vec![]);
    right.pan = 1.0;
    let out = render(vec![device("utility", &[]), group(vec![right], &[])]);
    let (l, r): (Vec<f32>, Vec<f32>) = out.as_chunks::<2>().0.iter().map(|[l, r]| (*l, *r)).unzip();
    assert!(peak(&l) < 1e-6 && peak(&r) > 0.1);
    let whole = reference();
    let whole_right: Vec<f32> = whole.as_chunks::<2>().0.iter().map(|[_, r]| *r).collect();
    // Hard right is √2 louder on the right than centred, as a channel's pan.
    assert!(max_diff(&r, &scaled(&whole_right, std::f32::consts::SQRT_2)) < 1e-4);
}

#[test]
fn a_nested_group_runs_inside_a_branch() {
    let inner = group(vec![branch(vec![half()]), branch(vec![half()])], &[]);
    let outer = group(vec![branch(vec![inner]), branch(vec![])], &[("gain", HALF_DB)]);
    let out = render(vec![device("utility", &[]), outer]);
    // (x + x) × ½ = x.
    let whole = reference();
    assert!(max_diff(&out, &whole) < 1e-4, "{}", max_diff(&out, &whole));
}

#[test]
fn a_switched_off_group_passes_its_input() {
    let mut off = group(vec![branch(vec![half()])], &[("gain", HALF_DB)]);
    off.enabled = false;
    assert_eq!(render(vec![device("utility", &[]), off]), reference());
}

#[test]
fn the_sound_does_not_depend_on_the_block_size() {
    let mut muted = branch(vec![half()]);
    muted.mute = true;
    let devices = vec![
        device("utility", &[]),
        group(vec![branch(vec![device("overdrive", &[("drive", 0.7)])]), muted, branch(vec![])], &[("mix", 0.6)]),
    ];
    let p = project(vec![note(0.0, 2.0, 69), note(0.5, 1.0, 76)], 2.0, devices);
    let big = render_offline(&p, SR, 512).unwrap_or_default();
    for frames in [64, 100, 1024] {
        let small = render_offline(&p, SR, frames).unwrap_or_default();
        assert!(max_diff(&small, &big) < 1e-5, "{frames}: {}", max_diff(&small, &big));
    }
}

#[test]
fn instruments_in_a_group_are_layered_and_hear_every_note() {
    let notes = vec![note(0.0, 1.0, 60), note(0.5, 1.0, 67)];
    let one = render_offline(&project(notes.clone(), 2.0, vec![device("fm-synth", &[])]), SR, 512).unwrap_or_default();
    // The mix is fixed at 1 in an instrument group: the 0 here changes nothing.
    let layered =
        group(vec![branch(vec![device("fm-synth", &[])]), branch(vec![device("fm-synth", &[])])], &[("mix", 0.0)]);
    let two = render_offline(&project(notes, 2.0, vec![layered]), SR, 512).unwrap_or_default();
    assert!(peak(&one) > 0.01);
    let len = one.len().min(two.len());
    let doubled = scaled(&one[..len], 2.0);
    assert!(max_diff(&two[..len], &doubled) < 1e-4, "{}", max_diff(&two[..len], &doubled));
}

#[test]
fn group_parameters_are_read_in_their_ranges() {
    let values = crate::group_param_values(&[DeviceParam::new("mix", 3.0), DeviceParam::new("gain", f64::NAN)]);
    assert_eq!(values.first(), Some(&1.0));
    assert_eq!(values.get(1), Some(&0.0));
    assert_eq!(values.len(), crate::GROUP_PARAMS.len());
    assert_eq!(crate::group_param_index("macro.8"), Some(9));
    assert_eq!(crate::group_param_index("width"), None);
}

#[test]
fn an_edit_to_a_branch_keeps_the_group_playing() {
    let layered = group(vec![branch(vec![device("fm-synth", &[])])], &[]);
    let p = project(vec![note(0.0, 8.0, 69)], 8.0, vec![layered]);
    let mut edited = p.clone();
    if let Some(g) = edited.channels[0].devices[0].as_group_mut() {
        g.branches[0].volume = 0.5;
    }
    let mut renderer =
        Renderer::build(&p, RenderInputs::from_audio(&crate::AudioPool::new()), SR, RenderOptions::PLAYING)
            .unwrap_or_else(|e| panic!("{e:?}"));
    let mut controller = renderer.controller();
    let mut out = blocks(&mut renderer, 50, 480);
    controller.update(&edited).unwrap_or_else(|e| panic!("{e}"));
    out.extend(blocks(&mut renderer, 100, 480));
    let mut untouched =
        Renderer::build(&p, RenderInputs::from_audio(&crate::AudioPool::new()), SR, RenderOptions::PLAYING)
            .unwrap_or_else(|e| panic!("{e:?}"));
    let reference = blocks(&mut untouched, 150, 480);
    // The note carries on through the swap (no new attack), and after the 20 ms ramp the branch
    // plays at half its level.
    let at = 50 * 480;
    assert_eq!(max_diff(&out[..at * 2], &reference[..at * 2]), 0.0);
    let settled = (at + 2_400) * 2;
    let half = scaled(&reference[settled..], 0.5);
    assert!(peak(&reference[settled..]) > 0.05);
    assert!(max_diff(&out[settled..], &half) < 1e-4, "{}", max_diff(&out[settled..], &half));
}

fn overdrive_dry() -> Device {
    // At mix 0 the overdrive passes its input, delayed by its latency (31 frames).
    device("overdrive", &[("mix", 0.0)])
}

#[test]
fn a_branch_with_latency_lines_up_with_a_dry_branch() {
    let single = render(vec![device("utility", &[]), overdrive_dry()]);
    let doubled = scaled(&single, 2.0);
    let grouped = render(vec![device("utility", &[]), group(vec![branch(vec![overdrive_dry()]), branch(vec![])], &[])]);
    let len = single.len().min(grouped.len());
    assert!(peak(&single) > 0.1);
    // Without compensation the dry branch would come 31 frames early and comb-filter the sum.
    assert!(max_diff(&grouped[..len], &doubled[..len]) < 1e-4, "{}", max_diff(&grouped[..len], &doubled[..len]));
    // The dry signal of the mix waits as long.
    let half_dry = render(vec![device("utility", &[]), group(vec![branch(vec![overdrive_dry()])], &[("mix", 0.5)])]);
    let len = single.len().min(half_dry.len());
    assert!(max_diff(&half_dry[..len], &single[..len]) < 1e-4, "{}", max_diff(&half_dry[..len], &single[..len]));
}

#[test]
fn a_nested_group_reports_its_latency_to_the_outer_one() {
    let single = render(vec![device("utility", &[]), overdrive_dry()]);
    let inner = group(vec![branch(vec![overdrive_dry()])], &[]);
    let outer = group(vec![branch(vec![inner]), branch(vec![])], &[]);
    let out = render(vec![device("utility", &[]), outer]);
    let len = single.len().min(out.len());
    let doubled = scaled(&single, 2.0);
    assert!(max_diff(&out[..len], &doubled[..len]) < 1e-4, "{}", max_diff(&out[..len], &doubled[..len]));

    let assets = crate::DeviceAssets::new();
    let mut plugins = crate::Plugins::new();
    let audio = crate::AudioPool::new();
    let mut builder = crate::group::Builder {
        sample_rate: SR,
        audio: &audio,
        assets: &assets,
        plugins: &mut plugins,
        keys: &[],
        channel: ChannelId(1),
        frames_per_beat: 24_000.0,
    };
    let nested = group(vec![branch(vec![group(vec![branch(vec![overdrive_dry(), overdrive_dry()])], &[])])], &[]);
    let built = nested.as_group().and_then(|g| builder.group(&nested, g, 0, &[0]));
    assert_eq!(built.map(|b| b.latency), Some(62));
}

#[test]
fn compensation_is_cut_at_two_seconds() {
    use crate::group::compensation;
    assert_eq!(compensation([31, 0, 10].into_iter(), 96_000), (31, vec![0, 31, 21]));
    assert_eq!(compensation([200_000, 0].into_iter(), 96_000), (96_000, vec![0, 96_000]));
    assert_eq!(compensation(std::iter::empty(), 96_000), (0, vec![]));
}

// A device in a group is built as the chain builds it (#766).

/// A second of noise as the sample "noise.wav".
fn noise_pool() -> crate::AudioPool {
    let mut pool = crate::AudioPool::new();
    let mut seed = 12_345u32;
    let samples = (0..SR as usize)
        .map(|_| {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (seed >> 8) as f32 / 8_388_608.0 - 1.0
        })
        .collect();
    pool.files
        .insert("noise.wav".into(), std::sync::Arc::new(crate::AudioData { sample_rate: SR, channels: 1, samples }));
    pool
}

fn sampler() -> Device {
    let mut sampler = device("sampler", &[]);
    sampler.set_sample_file("noise.wav");
    sampler
}

#[test]
fn a_sampler_in_a_group_plays_its_sample() {
    let pool = noise_pool();
    let play = |devices: Vec<Device>| {
        let project = project(vec![note(0.0, 1.0, 60)], 1.0, devices);
        crate::render_offline_from(&project, RenderInputs::from_audio(&pool), SR, 512, None).unwrap_or_default()
    };
    let plain = play(vec![sampler()]);
    assert!(peak(&plain) > 0.01, "the sampler sounds at all");
    let grouped = play(vec![group(vec![branch(vec![sampler()])], &[])]);
    assert_eq!(plain, grouped, "bit for bit, as in the chain");
    let nested = play(vec![group(vec![branch(vec![group(vec![branch(vec![sampler()])], &[])])], &[])]);
    assert_eq!(plain, nested);
}

/// The sampler playing slices set by hand: the key above the root key plays the second slice.
fn sliced_sampler(slices: &[f64]) -> Device {
    let mut sampler = device("sampler", &[("playback", 3.0), ("slice_by", 3.0)]);
    sampler.set_sample_file("noise.wav");
    if !slices.is_empty() {
        let _ = sampler.set_sample_slices(slices);
    }
    sampler
}

#[test]
fn a_sampler_in_a_group_plays_its_manual_slices() {
    let pool = noise_pool();
    let play = |devices: Vec<Device>| {
        let project = project(vec![note(0.0, 1.0, 61)], 1.0, devices);
        crate::render_offline_from(&project, RenderInputs::from_audio(&pool), SR, 512, None).unwrap_or_default()
    };
    let slices = [0.25, 0.5, 0.75];
    let plain = play(vec![sliced_sampler(&slices)]);
    assert!(peak(&plain) > 0.01, "the slice sounds at all");
    assert_ne!(plain, play(vec![sliced_sampler(&[])]), "the stored slices decide what is heard");
    let grouped = play(vec![group(vec![branch(vec![sliced_sampler(&slices)])], &[])]);
    assert_eq!(plain, grouped, "bit for bit, as in the chain");
    let nested = play(vec![group(vec![branch(vec![group(vec![branch(vec![sliced_sampler(&slices)])], &[])])], &[])]);
    assert_eq!(plain, nested);
}

fn lane(param: &str, points: &[(f64, f64)]) -> AutomationLane {
    AutomationLane {
        param: param.into(),
        points: points.iter().map(|&(beat, value)| AutomationPoint::new(beat, value)).collect(),
        enabled: true,
    }
}

#[test]
fn a_parameter_of_a_device_in_a_group_follows_its_lane() {
    // The utility's gain: unity, then a jump to half at beat 2.
    let mut utility = device("utility", &[("gain", -20.0)]);
    utility.automation = vec![lane("gain", &[(0.0, 0.0), (2.0, 0.0), (2.0, -6.0206), (3.0, -6.0206)])];
    let alone = render(vec![utility.clone()]);
    let grouped = render(vec![group(vec![branch(vec![utility.clone()])], &[])]);
    let nested =
        render(vec![group(vec![branch(vec![device("utility", &[]), group(vec![branch(vec![utility])], &[])])], &[])]);
    let plain = reference();
    let beat = SR as usize / 2;
    // Before the jump the lane's unity, not the set value (-20 dB), is heard; after it, half.
    assert!(max_diff(&alone[..beat * 4], &plain[..beat * 4]) < 1e-4, "the chain follows the lane");
    for (name, out) in [("a group", &grouped), ("a group in a group", &nested)] {
        assert_eq!(&alone, out, "{name} plays the lane like the chain");
    }
    let settle = SR as usize / 40;
    let tail = beat * 2 + settle..beat * 3;
    let half: Vec<f32> = plain[tail.start * 2..tail.end * 2].iter().map(|s| s * 0.5).collect();
    assert!(max_diff(&grouped[tail.start * 2..tail.end * 2], &half) < 1e-4);
}

#[test]
fn a_lane_in_a_group_keeps_playing_through_a_graph_swap() {
    let mut utility = device("utility", &[("gain", -20.0)]);
    utility.automation = vec![lane("gain", &[(0.0, 0.0), (2.0, 0.0), (2.0, -6.0206), (3.0, -6.0206)])];
    let p = project(vec![note(0.0, 8.0, 69)], 8.0, vec![group(vec![branch(vec![utility.clone()])], &[])]);
    // The edit moves the set value, which the lane overrides, and the jump at beat 2 to a quarter:
    // the swap adopts the running group and takes the edited lane.
    let mut edited = p.clone();
    if let Some(g) = edited.channels[0].devices[0].as_group_mut() {
        let device = &mut g.branches[0].devices[0];
        device.params = vec![DeviceParam::new("gain", -40.0)];
        device.automation = vec![lane("gain", &[(0.0, 0.0), (2.0, 0.0), (2.0, -12.0412), (3.0, -12.0412)])];
    }
    let mut renderer =
        Renderer::build(&p, RenderInputs::from_audio(&crate::AudioPool::new()), SR, RenderOptions::PLAYING)
            .unwrap_or_else(|e| panic!("{e:?}"));
    let mut controller = renderer.controller();
    let mut out = blocks(&mut renderer, 50, 480);
    controller.update(&edited).unwrap_or_else(|e| panic!("{e}"));
    out.extend(blocks(&mut renderer, 100, 480));
    let mut untouched =
        Renderer::build(&p, RenderInputs::from_audio(&crate::AudioPool::new()), SR, RenderOptions::PLAYING)
            .unwrap_or_else(|e| panic!("{e:?}"));
    let reference = blocks(&mut untouched, 150, 480);
    // The swap comes before the jump at beat 2 (frame 48 000): until then the lane's unity is
    // heard, not the edited value; after it, a quarter of the untouched project's half.
    let beat = SR as usize / 2;
    let jump = beat * 2 * 2;
    assert!(peak(&reference[..jump]) > 0.05);
    assert!(max_diff(&out[..jump], &reference[..jump]) < 1e-4, "{}", max_diff(&out[..jump], &reference[..jump]));
    let settled = (beat * 2 + SR as usize / 40) * 2;
    let half = scaled(&reference[settled..], 0.5);
    assert!(max_diff(&out[settled..], &half) < 1e-4, "{}", max_diff(&out[settled..], &half));
}

#[test]
fn a_file_device_in_a_group_is_known_by_its_file() {
    let assets = crate::DeviceAssets::new();
    let audio = noise_pool();
    let mut plugins = crate::Plugins::new();
    let mut identity = |state: &[u8]| {
        let mut cab = device("nam-cab-loader", &[]);
        cab.state = Some(state.to_vec());
        let outer = group(vec![branch(vec![cab.clone()])], &[]);
        let mut builder = crate::group::Builder {
            sample_rate: SR,
            audio: &audio,
            assets: &assets,
            plugins: &mut plugins,
            keys: &[],
            channel: ChannelId(1),
            frames_per_beat: 24_000.0,
        };
        let built = outer.as_group().and_then(|g| builder.group(&outer, g, 0, &[0])).map(|b| b.id);
        (built, crate::chain::device_identity("nam-cab-loader", &cab, &audio))
    };
    let (first, chain_first) = identity(b"one.wav");
    let (again, _) = identity(b"one.wav");
    let (other, chain_other) = identity(b"two.wav");
    assert_eq!(first, again);
    assert_ne!(first, other, "a new file is a new device: a swap rebuilds it");
    assert_ne!(chain_first, chain_other);
    assert!(first.is_some_and(|id| id.contains(&chain_first)), "the same identity as in a chain");
}
