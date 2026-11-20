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
    for _ in 0..16 {
        out.next_sample();
    }
    assert!(!out.is_active(), "every mode crossed the margin and stopped");
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
