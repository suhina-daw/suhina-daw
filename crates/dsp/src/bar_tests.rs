// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

fn one_mode(frequency_hz: f64, decay_seconds: f64) -> [BarMode; 1] {
    [BarMode { frequency_hz, decay_seconds, shape: 1.0, modal_mass: 0.1, output: 1.0 }]
}

fn bar_modes(count: usize, x: f64) -> Vec<BarMode> {
    (1..=count)
        .map(|n| BarMode {
            frequency_hz: 220.0 * uniform_ratio(n),
            decay_seconds: 2.0 / n as f64,
            shape: mode_shape(n, x),
            modal_mass: 0.1,
            output: 1.0,
        })
        .collect()
}

fn soft() -> Mallet {
    Mallet::from_contact_time(0.025, 2.5, 0.0025, 0.5)
}

fn hard() -> Mallet {
    Mallet::from_contact_time(0.025, 2.5, 0.00025, 0.05)
}

#[test]
fn free_free_shapes_are_normalised_orthogonal_and_have_the_textbook_nodes() {
    assert!((uniform_ratio(2) - 2.7565).abs() < 1e-4);
    assert!((uniform_ratio(4) - 8.9330).abs() < 1e-4);
    assert!((beta(17) - 35.0 * PI / 2.0).abs() < 1e-12);
    for n in 1..=20 {
        assert!((mode_shape(n, 0.0) - 1.0).abs() < 1e-9, "mode {n} at one end");
        assert!((mode_shape(n, 1.0).abs() - 1.0).abs() < 1e-9, "mode {n} at the other end");
    }
    assert!(mode_shape(2, 0.5).abs() < 1e-12, "mode 2 has a node at the centre");
    assert!(mode_shape(1, 0.2242).abs() < 1e-3, "mode 1's nodes are near 0.224");
    let integral = |f: &dyn Fn(f64) -> f64| (0..20000).map(|i| f((i as f64 + 0.5) / 20000.0)).sum::<f64>() / 20000.0;
    for m in 1..=16 {
        for n in m..=16 {
            let value = integral(&|x| mode_shape(m, x) * mode_shape(n, x));
            let expected = if m == n { 0.25 } else { 0.0 };
            assert!((value - expected).abs() < 1e-5, "modes {m} and {n}: {value}");
        }
    }
    assert_eq!(mode_shape(0, 0.5), 0.0);
    assert_eq!(mode_shape(3, f64::NAN), 0.0);
}

#[test]
fn contact_times_follow_the_closed_form_rigid_collision() {
    assert!((contact_integral(1.0) - PI / 2.0).abs() < 1e-12, "a linear spring's half period");
    let mallet = Mallet::from_contact_time(0.025, 2.5, 0.001, 0.0);
    assert!((mallet.contact_time(0.025, 1.0) - 0.001).abs() < 1e-12);
    // Faster strokes are shorter: tau ~ v^((1 - a) / (1 + a)).
    let ratio = mallet.contact_time(0.025, 4.0) / mallet.contact_time(0.025, 1.0);
    assert!((ratio - 4.0_f64.powf(-1.5 / 3.5)).abs() < 1e-12);
    // A simulated contact against a very heavy bar lasts as long as the rigid formula says.
    let mut bar = StruckBar::<1>::default();
    bar.set_modes(
        192000.0,
        &[BarMode { frequency_hz: 50.0, decay_seconds: 10.0, shape: 1.0, modal_mass: 1e3, output: 1.0 }],
    );
    bar.strike(Excitation::Contact, mallet, 1.0);
    let mut samples = 0;
    while bar.is_struck() {
        bar.next_sample();
        samples += 1;
    }
    let measured = samples as f64 / 192000.0;
    assert!((measured - 0.001).abs() < 2e-5, "contact lasted {measured}");
}

/// The response of one mode to the half-sine pulse by direct quadrature of the convolution with
/// its impulse response, independent of the exact per-sample integration.
#[test]
fn a_pulse_matches_the_quadrature_of_its_convolution() {
    let (rate, frequency, decay) = (48000.0, 700.0, 0.5);
    let mut bar = StruckBar::<1>::default();
    bar.set_modes(rate, &one_mode(frequency, decay));
    let mallet = soft();
    let reduced = mallet.mass * 0.1 / (mallet.mass + 0.1);
    let tau = mallet.contact_time(reduced, 2.0);
    let impulse = (1.0 + mallet.restitution(2.0)) * reduced * 2.0;
    bar.strike(Excitation::Pulse, mallet, 2.0);
    let sigma = 1000.0_f64.ln() / decay;
    let omega = TAU * frequency;
    let force = |t: f64| if (0.0..=tau).contains(&t) { PI * impulse / (2.0 * tau) * (PI * t / tau).sin() } else { 0.0 };
    // Velocity of the impulse response: d/dt [e^{-sigma t} sin(omega t) / (m omega)].
    let kernel = |t: f64| (-sigma * t).exp() * (omega * (omega * t).cos() - sigma * (omega * t).sin()) / (0.1 * omega);
    for n in 0..400 {
        let output = f64::from(bar.next_sample());
        let t = n as f64 / rate;
        let steps = 20000;
        let width = t.min(tau) / steps as f64;
        let expected = if t == 0.0 {
            0.0
        } else {
            (0..steps)
                .map(|i| {
                    let u = (i as f64 + 0.5) * width;
                    force(u) * kernel(t - u)
                })
                .sum::<f64>()
                * width
        };
        assert!((output - expected).abs() < 2e-5 * expected.abs().max(1e-3), "sample {n}: {output} vs {expected}");
    }
}

#[test]
fn pulses_superpose_on_a_ringing_bar() {
    let modes = bar_modes(8, 0.4);
    let mut first = StruckBar::<8>::default();
    let mut second = StruckBar::<8>::default();
    let mut both = StruckBar::<8>::default();
    for bar in [&mut first, &mut second, &mut both] {
        bar.set_modes(48000.0, &modes);
    }
    first.strike(Excitation::Pulse, soft(), 1.5);
    both.strike(Excitation::Pulse, soft(), 1.5);
    for n in 0..4000 {
        if n == 1234 {
            second.strike(Excitation::Pulse, soft(), 3.0);
            both.strike(Excitation::Pulse, soft(), 3.0);
        }
        let sum = first.next_sample() + second.next_sample();
        let together = both.next_sample();
        assert!((sum - together).abs() < 1e-6, "sample {n}");
    }
}

fn strokes(
    bar: &mut StruckBar<16>,
    excitation: Excitation,
    mallet: Mallet,
    schedule: &[(usize, f64)],
    frames: usize,
) -> f64 {
    let mut injected = 0.0;
    let mut peak_ratio: f64 = 0.0;
    for n in 0..frames {
        for &(at, speed) in schedule {
            if at == n {
                bar.strike(excitation, mallet, speed);
                injected += 0.5 * mallet.mass * speed * speed;
            }
        }
        let output = bar.next_sample();
        assert!(output.is_finite());
        if injected > 0.0 {
            peak_ratio = peak_ratio.max(bar.energy() / injected);
        }
        assert!(bar.last_stroke_steps() <= u64::from(MAX_CONTACT_STEPS) * 48000);
    }
    peak_ratio
}

#[test]
fn contact_never_gives_the_bar_more_energy_than_the_strokes_brought() {
    for rate in [8000.0, 22050.0, 48000.0, 192000.0] {
        for mallet in [soft(), hard()] {
            for x in [0.5, 0.38, 0.1] {
                let mut bar = StruckBar::<16>::default();
                bar.set_modes(rate, &bar_modes(16, x));
                let schedule: Vec<(usize, f64)> = (0..12).map(|i| (i * (rate as usize / 37) + i * 7, 4.0)).collect();
                let ratio = strokes(&mut bar, Excitation::Contact, mallet, &schedule, rate as usize / 2);
                assert!(ratio <= 1.01, "rate {rate}, x {x}: the bar holds {ratio} times the injected energy");
                assert!(ratio > 0.01, "rate {rate}, x {x}: strokes must reach the bar");
            }
        }
    }
}

#[test]
fn pulses_never_give_the_bar_more_energy_than_the_stroke_brought() {
    for mallet in [soft(), hard()] {
        let mut bar = StruckBar::<16>::default();
        bar.set_modes(48000.0, &bar_modes(16, 0.38));
        let ratio = strokes(&mut bar, Excitation::Pulse, mallet, &[(0, 4.0)], 4800);
        assert!(ratio <= 1.0, "{ratio}");
    }
}

#[test]
fn contact_steps_are_bounded_at_low_rates_with_the_hardest_mallet() {
    let mut bar = StruckBar::<16>::default();
    bar.set_modes(22050.0, &bar_modes(16, 0.5));
    bar.strike(Excitation::Contact, hard(), 100.0);
    let mut samples = 0;
    while bar.is_struck() {
        assert!(bar.next_sample().is_finite());
        samples += 1;
    }
    assert!(samples as f64 <= (MAX_CONTACT_SECONDS + MAX_APPROACH_SECONDS) * 22050.0 + 1.0);
    assert!(bar.last_stroke_steps() <= u64::from(MAX_CONTACT_STEPS) * samples);
    assert!(bar.energy().is_finite());
}

#[test]
fn hostile_values_are_silent_or_bounded() {
    let mut bar = StruckBar::<16>::default();
    for rate in [f64::NAN, f64::INFINITY, -1.0, 0.0, 999.0, 8000.0, 48000.0, 384000.0, 1e9] {
        for frequency in [f64::NAN, f64::INFINITY, -1.0, 0.0, 440.0, 200000.0] {
            for shape in [f64::NAN, 0.0, 1.0, 1e9] {
                for mass in [f64::NAN, -1.0, 0.0, 1e-30, 0.1, 1e30] {
                    let mode =
                        BarMode { frequency_hz: frequency, decay_seconds: 1.0, shape, modal_mass: mass, output: 1.0 };
                    bar.set_modes(rate, &[mode; 20]);
                    for speed in [f64::NAN, -1.0, 0.0, 1.0, 1e12] {
                        for excitation in [Excitation::Contact, Excitation::Pulse] {
                            bar.strike(excitation, hard(), speed);
                            for _ in 0..64 {
                                assert!(bar.next_sample().is_finite());
                            }
                        }
                    }
                    bar.reset();
                    assert_eq!(bar.next_sample(), 0.0);
                    assert!(!bar.is_active());
                }
            }
        }
    }
    let nan_mallet = Mallet { mass: f64::NAN, stiffness: f64::NAN, exponent: f64::NAN, dissipation: f64::NAN };
    bar.set_modes(48000.0, &bar_modes(4, 0.4));
    bar.strike(Excitation::Contact, nan_mallet, 1.0);
    bar.strike(Excitation::Pulse, Mallet::from_contact_time(f64::NAN, f64::NAN, f64::NAN, f64::NAN), 1.0);
    for _ in 0..512 {
        assert!(bar.next_sample().is_finite());
    }
}

#[test]
fn modes_above_the_anti_alias_margin_are_left_out() {
    let mut bar = StruckBar::<1>::default();
    bar.set_modes(8000.0, &one_mode(3600.0, 1.0));
    assert!(bar.point_mass().is_infinite());
    bar.strike(Excitation::Pulse, soft(), 1.0);
    assert!(!bar.is_active());
    assert_eq!(bar.next_sample(), 0.0);
}

#[test]
fn damping_and_retuning_keep_the_phase_and_never_add_energy() {
    let mut bar = StruckBar::<4>::default();
    bar.set_modes(48000.0, &bar_modes(4, 0.4));
    bar.strike(Excitation::Pulse, soft(), 2.0);
    for _ in 0..480 {
        bar.next_sample();
    }
    let before = bar.energy();
    let mut damped = bar;
    damped.damp(0.05);
    let mut free = bar;
    for _ in 0..4800 {
        damped.next_sample();
        free.next_sample();
    }
    assert!(damped.energy() < free.energy() * 1e-3);
    assert!(damped.energy() < before);
    damped.damp(f64::NAN);
    let mut up = bar;
    up.transpose(12.0);
    up.transpose(12.0);
    assert!(up.is_active() && up.energy().is_finite());
    let mut out = bar;
    out.transpose(96.0);
    // Its output fades out over the declick time instead of stopping at once.
    for _ in 0..(DECLICK_SECONDS * 48000.0) as usize + 2 {
        assert!(out.next_sample().is_finite());
    }
    assert!(!out.is_active(), "every mode crossed the margin and stopped");
}

fn largest_step(samples: &[f32]) -> f32 {
    samples.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0, f32::max)
}

#[test]
fn coefficient_changes_on_a_ringing_bar_do_not_click() {
    let modes = bar_modes(8, 0.4);
    let brighter: Vec<BarMode> = modes.iter().map(|mode| BarMode { output: mode.output * 3.0, ..*mode }).collect();
    for change in 0..4 {
        let mut bar = StruckBar::<8>::default();
        bar.set_modes(48000.0, &modes);
        bar.set_tube(Some(Tube { amount: 1.0, gain: 2.0, quality: 40.0, radiation_seconds: 1.2 }));
        bar.strike(Excitation::Pulse, soft(), 2.0);
        let mut before: Vec<f32> = (0..2400).map(|_| bar.next_sample()).collect();
        // Change where the output is large, so a step there would be a loud click.
        let largest = before.iter().fold(0.0f32, |m, x| m.max(x.abs()));
        while bar.current_output().abs() < 0.5 * f64::from(largest) && before.len() < 4800 {
            before.push(bar.next_sample());
        }
        let jump = bar.current_output();
        match change {
            0 => bar.set_modes(48000.0, &brighter),
            1 => bar.damp(0.01),
            2 => bar.transpose(2.0),
            _ => bar.set_tube(None),
        }
        let jump = (jump - bar.current_output()).abs();
        let mut across = vec![*before.last().unwrap()];
        across.extend((0..480).map(|_| bar.next_sample()));
        // The signal's own steps may grow or shrink with the change (a brighter bar moves faster).
        let normal = largest_step(&before[1200..]).max(largest_step(&across[200..]));
        if change == 0 || change == 3 {
            let step_before = largest_step(&before[1200..]);
            assert!(
                jump > f64::from(step_before),
                "change {change}: the change must move the output by more than a step"
            );
        }
        assert!(largest_step(&across) <= normal * 1.5, "change {change}: {} vs {normal}", largest_step(&across));
    }
}

#[test]
fn a_tube_adds_twice_its_amount_of_the_first_mode_once_built_up() {
    let mode = [BarMode { frequency_hz: 440.0, decay_seconds: 30.0, shape: 1.0, modal_mass: 0.1, output: 1.0 }];
    let render = |amount: f64| {
        let mut bar = StruckBar::<1>::default();
        bar.set_modes(48000.0, &mode);
        bar.set_tube(Some(Tube { amount, gain: 2.0, quality: 40.0, radiation_seconds: 600.0 }));
        bar.strike(Excitation::Pulse, soft(), 2.0);
        let signal: Vec<f32> = (0..24000).map(|_| bar.next_sample()).collect();
        signal[14400..19200].iter().map(|x| f64::from(*x).powi(2)).sum::<f64>().sqrt()
    };
    let none = render(0.0);
    for amount in [0.25, 0.5, 1.0] {
        let ratio = render(amount) / none;
        assert!((ratio - (1.0 + 2.0 * amount)).abs() < 0.04 * (1.0 + 2.0 * amount), "amount {amount}: {ratio}");
    }
    let mut plain = StruckBar::<1>::default();
    let mut zero = StruckBar::<1>::default();
    plain.set_modes(48000.0, &mode);
    zero.set_modes(48000.0, &mode);
    zero.set_tube(Some(Tube { amount: 0.0, gain: 2.0, quality: 40.0, radiation_seconds: 1.0 }));
    plain.strike(Excitation::Contact, hard(), 3.0);
    zero.strike(Excitation::Contact, hard(), 3.0);
    for _ in 0..4800 {
        assert_eq!(plain.next_sample(), zero.next_sample(), "amount 0 is no tube");
    }
}

#[test]
fn a_tube_shortens_the_first_mode_and_stays_bounded() {
    let modes = bar_modes(4, 0.5);
    let energy = |tube: Option<Tube>| {
        let mut bar = StruckBar::<4>::default();
        bar.set_modes(48000.0, &modes);
        bar.set_tube(tube);
        bar.strike(Excitation::Contact, soft(), 4.0);
        let mut peak: f32 = 0.0;
        for _ in 0..48000 {
            let x = bar.next_sample();
            assert!(x.is_finite());
            peak = peak.max(x.abs());
        }
        (bar.energy(), peak)
    };
    let (free, free_peak) = energy(None);
    let (tubed, tubed_peak) = energy(Some(Tube { amount: 1.0, gain: 4.0, quality: 40.0, radiation_seconds: 1.2 }));
    assert!(tubed < free * 0.5, "the tube draws energy from the bar: {tubed} vs {free}");
    assert!(tubed_peak < free_peak * 4.0);
    for tube in [
        Tube { amount: f64::NAN, gain: 4.0, quality: 40.0, radiation_seconds: 1.0 },
        Tube { amount: 1e9, gain: 1e9, quality: 1e-9, radiation_seconds: 1e-9 },
        Tube { amount: 1.0, gain: f64::NAN, quality: f64::INFINITY, radiation_seconds: -1.0 },
    ] {
        let mut bar = StruckBar::<4>::default();
        bar.set_modes(22050.0, &modes);
        bar.set_tube(Some(tube));
        bar.strike(Excitation::Contact, hard(), 10.0);
        for _ in 0..22050 {
            assert!(bar.next_sample().is_finite());
        }
    }
}

#[test]
fn set_modes_keeps_the_motion_of_a_ringing_bar() {
    let mut bar = StruckBar::<4>::default();
    let modes = bar_modes(4, 0.4);
    bar.set_modes(48000.0, &modes);
    bar.strike(Excitation::Pulse, soft(), 2.0);
    for _ in 0..480 {
        bar.next_sample();
    }
    let mut again = bar;
    again.set_modes(48000.0, &modes);
    for _ in 0..480 {
        assert_eq!(again.next_sample(), bar.next_sample());
    }
    again.set_modes(48000.0, &modes[..2]);
    let mut kept = 0;
    for _ in 0..4 {
        kept += usize::from(again.next_sample() != 0.0);
    }
    assert_eq!(kept, 4, "two modes still ring");
}

#[test]
fn mallet_noise_sounds_only_while_the_mallet_touches_and_leaves_the_bar_alone() {
    let modes = bar_modes(8, 0.4);
    for excitation in [Excitation::Contact, Excitation::Pulse] {
        let mut quiet = StruckBar::<8>::default();
        let mut noisy = StruckBar::<8>::default();
        for bar in [&mut quiet, &mut noisy] {
            bar.set_modes(48000.0, &modes);
        }
        noisy.set_mallet_noise(1e-4);
        quiet.strike(excitation, hard(), 4.0);
        noisy.strike(excitation, hard(), 4.0);
        let mut differed = false;
        let mut after = 0;
        for _ in 0..4800 {
            let touching = noisy.is_struck();
            let (a, b) = (quiet.next_sample(), noisy.next_sample());
            assert!(b.is_finite());
            if a != b {
                differed = true;
                assert!(touching || after == 0, "{excitation:?}: noise only during the contact and one sample after");
            }
            if !touching {
                after += 1;
            }
            assert_eq!(quiet.energy(), noisy.energy(), "{excitation:?}: the noise never moves the bar");
        }
        assert!(differed, "{excitation:?}: the stroke knocks");
    }
}

#[test]
fn harder_and_faster_strokes_knock_louder() {
    let modes = bar_modes(8, 0.4);
    let knock = |mallet: Mallet, speed: f64| {
        let mut quiet = StruckBar::<8>::default();
        let mut noisy = StruckBar::<8>::default();
        for bar in [&mut quiet, &mut noisy] {
            bar.set_modes(48000.0, &modes);
        }
        noisy.set_mallet_noise(1e-4);
        quiet.strike(Excitation::Contact, mallet, speed);
        noisy.strike(Excitation::Contact, mallet, speed);
        (0..960).map(|_| f64::from(noisy.next_sample() - quiet.next_sample()).powi(2)).sum::<f64>()
    };
    assert!(knock(soft(), 4.0) > knock(soft(), 1.0));
    assert!(knock(hard(), 4.0) > knock(soft(), 4.0));
    let mut bar = StruckBar::<8>::default();
    bar.set_modes(48000.0, &modes);
    for gain in [f64::NAN, f64::INFINITY, -1.0, 1e9] {
        bar.set_mallet_noise(gain);
        bar.strike(Excitation::Contact, hard(), 100.0);
        for _ in 0..2400 {
            assert!(bar.next_sample().is_finite());
        }
    }
}
