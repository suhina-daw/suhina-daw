// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

#[test]
fn the_own_functions_match_the_systems_closely() {
    for i in -400..=400 {
        let x = i as f32 * 0.137;
        assert!((sin(x) - x.sin()).abs() < 2e-6, "sin {x}");
        assert!((cos(x) - x.cos()).abs() < 2e-6, "cos {x}");
    }
    for i in -300..=10 {
        let x = i as f32 * 0.1;
        assert!(((exp(x) - x.exp()) / x.exp()).abs() < 1e-5, "exp {x}");
    }
    assert_eq!(sin(f32::NAN), 0.0);
    assert_eq!(exp(f32::NAN), 0.0);
    assert!(exp(-500.0) >= 0.0 && exp(-500.0) < 1e-30);
}

#[test]
fn plans_make_their_dots_inside_the_rectangle_and_the_same_every_time() {
    let cpu = points(&CPU_PLAN);
    assert_eq!(cpu.len(), CPU_PLAN.iter().map(|&(a, b)| a as usize * b as usize).sum::<usize>());
    assert_eq!(cpu, points(&CPU_PLAN));
    for [u, lane, ribbon, seed] in cpu {
        assert!((0.0..=1.0).contains(&u) && (-1.0..=1.0).contains(&lane) && (0.0..=1.0).contains(&seed));
        assert!((0.0..6.0).contains(&ribbon));
    }
}

fn colors() -> [[f32; 4]; 4] {
    [[1.0, 0.98, 0.93, 1.0], [0.87, 0.74, 0.53, 1.0], [0.5, 0.34, 0.21, 1.0], [0.78, 0.5, 0.2, 1.0]]
}

#[test]
fn at_rest_the_first_ribbons_flow_and_the_others_wait_for_their_modes() {
    let rest = Frame { colors: colors(), ..Frame::default() };
    let shared = rest.shared(880.0, 236.0);
    let dots: Vec<_> =
        points(&CPU_PLAN).into_iter().filter_map(|p| place(p, &shared, &rest.colors).map(|d| (p, d))).collect();
    assert!(dots.len() > 1000, "the ribbons show at rest: {}", dots.len());
    assert!(dots.iter().all(|(p, _)| p[2] < 3.0), "the modes' own ribbons wait for sound");
    // Fullest around the center, thin towards the edges.
    let spread = |from: f32, to: f32| {
        let ys: Vec<f32> = dots.iter().filter(|(p, _)| p[0] >= from && p[0] < to).map(|(_, d)| d.y).collect();
        let (lo, hi) = ys.iter().fold((f32::MAX, f32::MIN), |(lo, hi), y| (lo.min(*y), hi.max(*y)));
        hi - lo
    };
    assert!(spread(0.46, 0.54) > 4.0 * spread(0.0, 0.04), "{} against {}", spread(0.46, 0.54), spread(0.0, 0.04));
    for (_, dot) in &dots {
        assert!(dot.color.iter().all(|c| (0.0..=1.0).contains(c)) && dot.color[0] <= dot.color[3] + 1e-6);
        assert!(dot.size > 0.0 && dot.x.is_finite() && dot.y.is_finite());
    }
}

#[test]
fn sound_raises_the_modes_ribbons_and_a_strike_throws_sparks() {
    let sounding = Frame {
        modes: [0.9, 0.7, 0.5, 0.3, 0.0, 0.0],
        since: 0.3,
        velocity: 1.0,
        colors: colors(),
        ..Frame::default()
    };
    let shared = sounding.shared(880.0, 236.0);
    let count = |ribbon: f32| {
        points(&CPU_PLAN)
            .into_iter()
            .filter(|p| p[2] == ribbon)
            .filter_map(|p| place(p, &shared, &sounding.colors))
            .count()
    };
    assert!(count(3.0) > 0 && count(4.0) > 0 && count(5.0) > 0, "each sounding mode raises its ribbon");
    // Long after a strike its sparks are gone; just after one they show, and as it ages the shell
    // spreads: the same spark dots near the center are farther out a second on than at once.
    let quiet = Frame { modes: sounding.modes, colors: colors(), ..Frame::default() }.shared(880.0, 236.0);
    let sparks = |shared: &Shared| {
        points(&CPU_PLAN)
            .into_iter()
            .filter(|p| p[3] > 0.9 && p[2] < 3.0)
            .filter_map(|p| place(p, shared, &colors()))
            .count()
    };
    assert_eq!(sparks(&quiet), 0, "no sparks long after a strike");
    assert!(sparks(&shared) > 0, "sparks just after one");
    let at = |since: f32| Frame { since, ..sounding }.shared(880.0, 236.0);
    let (first, later) = (at(0.05), at(1.0));
    let from_center = |d: &Dot| ((d.x - 440.0).powi(2) + (d.y - 118.0).powi(2)).sqrt();
    let mut moved = 0;
    for p in
        points(&CPU_PLAN).into_iter().filter(|p| (0.8..0.9).contains(&p[3]) && p[2] < 3.0 && (0.4..0.6).contains(&p[0]))
    {
        if let (Some(after), Some(before)) = (place(p, &later, &colors()), place(p, &first, &colors())) {
            assert!(from_center(&after) >= from_center(&before) - 1e-3, "a spark flies outwards");
            if from_center(&after) > from_center(&before) + 1.0 {
                moved += 1;
            }
        }
    }
    assert!(moved > 10, "the sparks near the center fly out: {moved}");
    assert!(clock_rate(&sounding.modes, 0.5) > clock_rate(&[0.0; 6], 0.5), "the flow runs faster while it sounds");
}

#[test]
fn hostile_inputs_draw_nothing_wild() {
    let wild = Frame {
        modes: [f32::NAN, f32::INFINITY, -1.0, 9.0, f32::NAN, 0.0],
        since: f32::NAN,
        velocity: f32::INFINITY,
        height: f32::NAN,
        thickness: -4.0,
        speed: f32::INFINITY,
        presence: 7.0,
        clock: f32::NAN,
        colors: colors(),
    };
    let shared = wild.shared(880.0, 236.0);
    for p in points(&CPU_PLAN) {
        if let Some(dot) = place(p, &shared, &wild.colors) {
            assert!(dot.x.is_finite() && dot.y.is_finite() && dot.size.is_finite());
            assert!(dot.color.iter().all(|c| c.is_finite() && (0.0..=1.0).contains(c)));
        }
    }
}
